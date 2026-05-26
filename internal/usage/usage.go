package usage

import (
	"bufio"
	"crypto/sha256"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"io/fs"
	"os"
	"path/filepath"
	"sort"
	"strings"
	"sync"
	"time"
)

type Event struct {
	Provider      string    `json:"provider"`
	Source        string    `json:"source"`
	SourcePath    string    `json:"source_path"`
	SessionID     string    `json:"session_id,omitempty"`
	TurnID        string    `json:"turn_id,omitempty"`
	RequestID     string    `json:"request_id,omitempty"`
	Model         string    `json:"model,omitempty"`
	CWD           string    `json:"cwd,omitempty"`
	Timestamp     time.Time `json:"timestamp"`
	Input         int64     `json:"input_tokens,omitempty"`
	CachedInput   int64     `json:"cached_input_tokens,omitempty"`
	CacheCreation int64     `json:"cache_creation_input_tokens,omitempty"`
	Output        int64     `json:"output_tokens,omitempty"`
	Reasoning     int64     `json:"reasoning_output_tokens,omitempty"`
	Total         int64     `json:"total_tokens,omitempty"`
	Cumulative    int64     `json:"cumulative_tokens,omitempty"`
	ModelContext  int64     `json:"model_context_window,omitempty"`
	dedupKey      string
}

type SessionSummary struct {
	Provider      string    `json:"provider"`
	SessionID     string    `json:"session_id"`
	CWD           string    `json:"cwd,omitempty"`
	Last          time.Time `json:"last"`
	Events        int       `json:"events"`
	Models        []string  `json:"models,omitempty"`
	Input         int64     `json:"input_tokens"`
	CachedInput   int64     `json:"cached_input_tokens"`
	CacheCreation int64     `json:"cache_creation_input_tokens"`
	Output        int64     `json:"output_tokens"`
	Reasoning     int64     `json:"reasoning_output_tokens"`
	Total         int64     `json:"total_tokens"`
}

type SourceReport struct {
	Provider string `json:"provider"`
	Files    int    `json:"files"`
	Events   int    `json:"events"`
	Error    string `json:"error,omitempty"`
}

type Report struct {
	GeneratedAt time.Time        `json:"generated_at"`
	Sources     []SourceReport   `json:"sources"`
	Sessions    []SessionSummary `json:"sessions"`
}

type Reader struct {
	home     string
	stateDir string
	mu       sync.Mutex
	loaded   bool
	files    map[string]cachedFile
}

type cachedFile struct {
	size           int64
	modTime        time.Time
	prefixHash     string
	events         []Event
	codexSessionID string
	codexCWD       string
}

func NewReader(home string) *Reader {
	return &Reader{home: home, files: map[string]cachedFile{}}
}

func NewReaderWithState(home, stateDir string) *Reader {
	return &Reader{home: home, stateDir: stateDir, files: map[string]cachedFile{}}
}

func (r *Reader) SetStateDir(stateDir string) {
	r.mu.Lock()
	defer r.mu.Unlock()
	if r.stateDir == stateDir {
		return
	}
	r.stateDir = stateDir
	r.files = map[string]cachedFile{}
	r.loaded = false
}

func DefaultReport() (Report, error) {
	events, sources, err := DefaultEvents()
	report := Report{
		GeneratedAt: time.Now().UTC(),
		Sources:     sources,
		Sessions:    Summarize(events),
	}
	return report, err
}

func DefaultEvents() ([]Event, []SourceReport, error) {
	return EventsSince(time.Now().Add(-7 * 24 * time.Hour))
}

func ReportSince(since time.Time) (Report, error) {
	events, sources, err := EventsSince(since)
	report := Report{
		GeneratedAt: time.Now().UTC(),
		Sources:     sources,
		Sessions:    Summarize(events),
	}
	return report, err
}

func EventsSince(since time.Time) ([]Event, []SourceReport, error) {
	return NewReader("").EventsSince(since)
}

func (r *Reader) EventsSince(since time.Time) ([]Event, []SourceReport, error) {
	home := r.home
	if home == "" {
		var err error
		home, err = os.UserHomeDir()
		if err != nil {
			return nil, nil, err
		}
	}
	return r.eventsSince(home, since)
}

func (r *Reader) eventsSince(home string, since time.Time) ([]Event, []SourceReport, error) {
	r.mu.Lock()
	defer r.mu.Unlock()

	if r.files == nil {
		r.files = map[string]cachedFile{}
	}
	if err := r.loadState(); err != nil {
		return nil, []SourceReport{{Provider: "codex", Error: err.Error()}, {Provider: "claude", Error: err.Error()}}, err
	}
	home, err := filepath.Abs(home)
	if err != nil {
		return nil, nil, err
	}
	var all []Event
	var reports []SourceReport
	var errs []error

	codexEvents, codexReport, err := r.codexArchivedSessionsSince(filepath.Join(home, ".codex", "archived_sessions"), since)
	if err != nil {
		errs = append(errs, err)
		codexReport.Error = err.Error()
	}
	all = append(all, codexEvents...)
	reports = append(reports, codexReport)

	claudeEvents, claudeReport, err := r.claudeProjectsSince(filepath.Join(home, ".claude", "projects"), since)
	if err != nil {
		errs = append(errs, err)
		claudeReport.Error = err.Error()
	}
	all = append(all, claudeEvents...)
	reports = append(reports, claudeReport)

	sortEvents(all)
	return all, reports, errors.Join(errs...)
}

func CodexArchivedSessions(root string) ([]Event, SourceReport, error) {
	return CodexArchivedSessionsSince(root, time.Time{})
}

func CodexArchivedSessionsSince(root string, since time.Time) ([]Event, SourceReport, error) {
	return NewReader("").codexArchivedSessionsSince(root, since)
}

func (r *Reader) codexArchivedSessionsSince(root string, since time.Time) ([]Event, SourceReport, error) {
	report := SourceReport{Provider: "codex"}
	paths, err := globJSONL(root)
	if err != nil {
		return nil, report, err
	}
	paths = recentFiles(paths, since)
	report.Files = len(paths)
	r.pruneMissing(root, paths)
	var out []Event
	for _, path := range paths {
		events, err := r.readCodexCached(path)
		if err != nil {
			if errors.Is(err, os.ErrNotExist) {
				continue
			}
			return out, report, err
		}
		out = append(out, events...)
	}
	out = dedupe(out)
	out = filterEventsSince(out, since)
	report.Events = len(out)
	return out, report, nil
}

func ClaudeProjects(root string) ([]Event, SourceReport, error) {
	return ClaudeProjectsSince(root, time.Time{})
}

func ClaudeProjectsSince(root string, since time.Time) ([]Event, SourceReport, error) {
	return NewReader("").claudeProjectsSince(root, since)
}

func (r *Reader) claudeProjectsSince(root string, since time.Time) ([]Event, SourceReport, error) {
	report := SourceReport{Provider: "claude"}
	paths, err := walkJSONL(root)
	if err != nil {
		return nil, report, err
	}
	paths = recentFiles(paths, since)
	report.Files = len(paths)
	r.pruneMissing(root, paths)
	var out []Event
	for _, path := range paths {
		events, err := r.readClaudeCached(path)
		if err != nil {
			if errors.Is(err, os.ErrNotExist) {
				continue
			}
			return out, report, err
		}
		out = append(out, events...)
	}
	out = dedupe(out)
	out = filterEventsSince(out, since)
	report.Events = len(out)
	return out, report, nil
}

func (r *Reader) readCodexCached(path string) ([]Event, error) {
	return r.readCached(path, func(start int64, cached cachedFile) (cachedFile, error) {
		if start == 0 {
			events, sessionID, cwd, err := parseCodexFile(path)
			return cachedFile{events: events, codexSessionID: sessionID, codexCWD: cwd}, err
		}
		events, sessionID, cwd, err := parseCodexFileFrom(path, start, cached.codexSessionID, cached.codexCWD)
		if sessionID == "" {
			sessionID = cached.codexSessionID
		}
		if cwd == "" {
			cwd = cached.codexCWD
		}
		combined := append(cloneEvents(cached.events), events...)
		return cachedFile{events: dedupe(combined), codexSessionID: sessionID, codexCWD: cwd}, err
	})
}

func (r *Reader) readClaudeCached(path string) ([]Event, error) {
	return r.readCached(path, func(start int64, cached cachedFile) (cachedFile, error) {
		if start == 0 {
			events, err := readClaudeFile(path)
			return cachedFile{events: events}, err
		}
		events, err := parseClaudeFileFrom(path, start)
		combined := append(cloneEvents(cached.events), events...)
		return cachedFile{events: dedupe(combined)}, err
	})
}

func (r *Reader) readCached(path string, read func(start int64, cached cachedFile) (cachedFile, error)) ([]Event, error) {
	info, err := os.Stat(path)
	if err != nil {
		delete(r.files, path)
		_ = r.saveState()
		return nil, err
	}
	if cached, ok := r.files[path]; ok && cached.size == info.Size() && cached.modTime.Equal(info.ModTime()) {
		return cloneEvents(cached.events), nil
	}
	start := int64(0)
	cached := r.files[path]
	if cached.size > 0 && info.Size() > cached.size {
		prefixHash, err := filePrefixHash(path, cached.size)
		if err != nil {
			delete(r.files, path)
			_ = r.saveState()
			return nil, err
		}
		if prefixHash == cached.prefixHash {
			start = cached.size
		}
	}
	next, err := read(start, cached)
	if err != nil {
		delete(r.files, path)
		_ = r.saveState()
		return nil, err
	}
	next.size = info.Size()
	next.modTime = info.ModTime()
	prefixHash, err := filePrefixHash(path, info.Size())
	if err != nil {
		delete(r.files, path)
		_ = r.saveState()
		return nil, err
	}
	next.prefixHash = prefixHash
	next.events = cloneEvents(next.events)
	r.files[path] = next
	if err := r.saveState(); err != nil {
		return nil, err
	}
	return cloneEvents(next.events), nil
}

func cloneEvents(events []Event) []Event {
	if events == nil {
		return nil
	}
	out := make([]Event, len(events))
	copy(out, events)
	return out
}

func (r *Reader) pruneMissing(root string, paths []string) {
	if root == "" {
		return
	}
	root = filepath.Clean(root)
	current := map[string]bool{}
	for _, path := range paths {
		current[filepath.Clean(path)] = true
	}
	changed := false
	for path := range r.files {
		clean := filepath.Clean(path)
		if pathWithin(clean, root) && !current[clean] {
			delete(r.files, path)
			changed = true
		}
	}
	if changed {
		_ = r.saveState()
	}
}

func pathWithin(path, root string) bool {
	rel, err := filepath.Rel(root, path)
	if err != nil {
		return false
	}
	return rel == "." || (rel != ".." && !strings.HasPrefix(rel, ".."+string(os.PathSeparator)))
}

type persistedReaderState struct {
	Version int                            `json:"version"`
	Files   map[string]persistedCachedFile `json:"files"`
}

const persistedReaderStateVersion = 1

type persistedCachedFile struct {
	Size           int64     `json:"size"`
	ModTime        time.Time `json:"mod_time"`
	PrefixHash     string    `json:"prefix_hash"`
	Events         []Event   `json:"events"`
	CodexSessionID string    `json:"codex_session_id,omitempty"`
	CodexCWD       string    `json:"codex_cwd,omitempty"`
}

func (r *Reader) loadState() error {
	if r.loaded {
		return nil
	}
	r.loaded = true
	if r.stateDir == "" {
		return nil
	}
	content, err := os.ReadFile(r.statePath())
	if err != nil {
		if errors.Is(err, os.ErrNotExist) {
			return nil
		}
		return err
	}
	var persisted persistedReaderState
	if err := json.Unmarshal(content, &persisted); err != nil {
		return err
	}
	if persisted.Version != persistedReaderStateVersion {
		return nil
	}
	r.files = map[string]cachedFile{}
	for path, file := range persisted.Files {
		events := cloneEvents(file.Events)
		hydrateDedupKeys(events)
		r.files[path] = cachedFile{
			size:           file.Size,
			modTime:        file.ModTime,
			prefixHash:     file.PrefixHash,
			events:         events,
			codexSessionID: file.CodexSessionID,
			codexCWD:       file.CodexCWD,
		}
	}
	return nil
}

func (r *Reader) saveState() error {
	if r.stateDir == "" {
		return nil
	}
	persisted := persistedReaderState{Version: persistedReaderStateVersion, Files: map[string]persistedCachedFile{}}
	for path, file := range r.files {
		persisted.Files[path] = persistedCachedFile{
			Size:           file.size,
			ModTime:        file.modTime,
			PrefixHash:     file.prefixHash,
			Events:         cloneEvents(file.events),
			CodexSessionID: file.codexSessionID,
			CodexCWD:       file.codexCWD,
		}
	}
	content, err := json.MarshalIndent(persisted, "", "  ")
	if err != nil {
		return err
	}
	if err := os.MkdirAll(r.stateDir, 0o700); err != nil {
		return err
	}
	path := r.statePath()
	tmp, err := os.CreateTemp(r.stateDir, ".usage-cache-*")
	if err != nil {
		return err
	}
	tmpPath := tmp.Name()
	defer func() {
		_ = os.Remove(tmpPath)
	}()
	if _, err := tmp.Write(content); err != nil {
		_ = tmp.Close()
		return err
	}
	if err := tmp.Chmod(0o600); err != nil {
		_ = tmp.Close()
		return err
	}
	if err := tmp.Close(); err != nil {
		return err
	}
	return os.Rename(tmpPath, path)
}

func (r *Reader) statePath() string {
	return filepath.Join(r.stateDir, "usage-cache.json")
}

func hydrateDedupKeys(events []Event) {
	for i := range events {
		events[i].dedupKey = eventDedupKey(events[i])
	}
}

func filterEventsSince(events []Event, since time.Time) []Event {
	if since.IsZero() {
		return events
	}
	out := events[:0]
	for _, event := range events {
		if event.Timestamp.IsZero() || !event.Timestamp.Before(since) {
			out = append(out, event)
		}
	}
	return out
}

func eventDedupKey(event Event) string {
	switch event.Provider {
	case "codex":
		if event.Cumulative != 0 || event.Total != 0 {
			return fmt.Sprintf("codex:%s:%d:%d", event.SessionID, event.Cumulative, event.Total)
		}
	case "claude":
		if event.RequestID != "" {
			return "claude:" + event.SessionID + ":" + event.RequestID
		}
	}
	return ""
}

func filePrefixHash(path string, size int64) (string, error) {
	file, err := os.Open(path)
	if err != nil {
		return "", err
	}
	defer file.Close()
	hash := sha256.New()
	if _, err := io.CopyN(hash, file, size); err != nil && !errors.Is(err, io.EOF) {
		return "", err
	}
	return fmt.Sprintf("%x", hash.Sum(nil)), nil
}

func Summarize(events []Event) []SessionSummary {
	byKey := map[string]*SessionSummary{}
	models := map[string]map[string]bool{}
	for _, event := range dedupe(events) {
		key := event.Provider + ":" + event.SessionID
		if event.SessionID == "" {
			key = event.Provider + ":" + event.SourcePath
		}
		summary := byKey[key]
		if summary == nil {
			summary = &SessionSummary{Provider: event.Provider, SessionID: event.SessionID, CWD: event.CWD}
			byKey[key] = summary
			models[key] = map[string]bool{}
		}
		if event.Timestamp.After(summary.Last) {
			summary.Last = event.Timestamp
		}
		if summary.CWD == "" {
			summary.CWD = event.CWD
		}
		summary.Events++
		summary.Input += event.Input
		summary.CachedInput += event.CachedInput
		summary.CacheCreation += event.CacheCreation
		summary.Output += event.Output
		summary.Reasoning += event.Reasoning
		summary.Total += event.Total
		if event.Model != "" {
			models[key][event.Model] = true
		}
	}

	out := make([]SessionSummary, 0, len(byKey))
	for key, summary := range byKey {
		for model := range models[key] {
			summary.Models = append(summary.Models, model)
		}
		sort.Strings(summary.Models)
		out = append(out, *summary)
	}
	sort.Slice(out, func(i, j int) bool { return out[i].Last.After(out[j].Last) })
	return out
}

func readCodexFile(path string) ([]Event, error) {
	events, _, _, err := parseCodexFile(path)
	return events, err
}

func parseCodexFile(path string) ([]Event, string, string, error) {
	return parseCodexFileFrom(path, 0, strings.TrimSuffix(filepath.Base(path), filepath.Ext(path)), "")
}

func parseCodexFileFrom(path string, offset int64, sessionID, cwd string) ([]Event, string, string, error) {
	file, err := os.Open(path)
	if err != nil {
		return nil, sessionID, cwd, err
	}
	defer file.Close()
	if offset > 0 {
		if _, err := file.Seek(offset, 0); err != nil {
			return nil, sessionID, cwd, err
		}
	}

	var out []Event
	scanner := bufio.NewScanner(file)
	scanner.Buffer(make([]byte, 0, 64*1024), 16*1024*1024)
	for scanner.Scan() {
		var row codexRow
		if err := json.Unmarshal(scanner.Bytes(), &row); err != nil {
			return nil, sessionID, cwd, fmt.Errorf("%s: %w", path, err)
		}
		ts, _ := parseTime(row.Timestamp)
		if row.Type == "session_meta" {
			if row.Payload.ID != "" {
				sessionID = row.Payload.ID
			}
			cwd = row.Payload.CWD
			continue
		}
		if row.Type != "event_msg" || row.Payload.Type != "token_count" {
			continue
		}
		last := row.Payload.Info.LastTokenUsage
		total := last.TotalTokens
		if total == 0 {
			total = last.InputTokens + last.CachedInputTokens + last.OutputTokens + last.ReasoningOutputTokens
		}
		cumulative := row.Payload.Info.TotalTokenUsage.TotalTokens
		out = append(out, Event{
			Provider:     "codex",
			Source:       "codex.archived_sessions",
			SourcePath:   path,
			SessionID:    sessionID,
			CWD:          cwd,
			Timestamp:    ts,
			Input:        last.InputTokens,
			CachedInput:  last.CachedInputTokens,
			Output:       last.OutputTokens,
			Reasoning:    last.ReasoningOutputTokens,
			Total:        total,
			Cumulative:   cumulative,
			ModelContext: row.Payload.Info.ModelContextWindow,
			dedupKey:     fmt.Sprintf("codex:%s:%d:%d", sessionID, cumulative, total),
		})
	}
	return out, sessionID, cwd, scanner.Err()
}

func readClaudeFile(path string) ([]Event, error) {
	return parseClaudeFileFrom(path, 0)
}

func parseClaudeFileFrom(path string, offset int64) ([]Event, error) {
	file, err := os.Open(path)
	if err != nil {
		return nil, err
	}
	defer file.Close()
	if offset > 0 {
		if _, err := file.Seek(offset, 0); err != nil {
			return nil, err
		}
	}

	var out []Event
	scanner := bufio.NewScanner(file)
	scanner.Buffer(make([]byte, 0, 64*1024), 16*1024*1024)
	for scanner.Scan() {
		var row claudeRow
		if err := json.Unmarshal(scanner.Bytes(), &row); err != nil {
			return nil, fmt.Errorf("%s: %w", path, err)
		}
		if row.Message.Usage == nil {
			continue
		}
		ts, _ := parseTime(row.Timestamp)
		usage := row.Message.Usage
		total := usage.InputTokens + usage.CacheReadInputTokens + usage.CacheCreationInputTokens + usage.OutputTokens
		requestID := row.RequestID
		if requestID == "" {
			requestID = row.Message.ID
		}
		out = append(out, Event{
			Provider:      "claude",
			Source:        "claude.projects",
			SourcePath:    path,
			SessionID:     row.SessionID,
			TurnID:        row.UUID,
			RequestID:     requestID,
			Model:         row.Message.Model,
			CWD:           row.CWD,
			Timestamp:     ts,
			Input:         usage.InputTokens,
			CachedInput:   usage.CacheReadInputTokens,
			CacheCreation: usage.CacheCreationInputTokens,
			Output:        usage.OutputTokens,
			Total:         total,
			dedupKey:      "claude:" + row.SessionID + ":" + requestID,
		})
	}
	return out, scanner.Err()
}

type codexRow struct {
	Timestamp string `json:"timestamp"`
	Type      string `json:"type"`
	Payload   struct {
		ID   string `json:"id"`
		CWD  string `json:"cwd"`
		Type string `json:"type"`
		Info struct {
			LastTokenUsage     codexTokenUsage `json:"last_token_usage"`
			TotalTokenUsage    codexTokenUsage `json:"total_token_usage"`
			ModelContextWindow int64           `json:"model_context_window"`
		} `json:"info"`
	} `json:"payload"`
}

type codexTokenUsage struct {
	InputTokens           int64 `json:"input_tokens"`
	CachedInputTokens     int64 `json:"cached_input_tokens"`
	OutputTokens          int64 `json:"output_tokens"`
	ReasoningOutputTokens int64 `json:"reasoning_output_tokens"`
	TotalTokens           int64 `json:"total_tokens"`
}

type claudeRow struct {
	Timestamp string `json:"timestamp"`
	RequestID string `json:"requestId"`
	SessionID string `json:"sessionId"`
	UUID      string `json:"uuid"`
	CWD       string `json:"cwd"`
	Message   struct {
		ID    string       `json:"id"`
		Model string       `json:"model"`
		Usage *claudeUsage `json:"usage"`
	} `json:"message"`
}

type claudeUsage struct {
	InputTokens              int64 `json:"input_tokens"`
	CacheCreationInputTokens int64 `json:"cache_creation_input_tokens"`
	CacheReadInputTokens     int64 `json:"cache_read_input_tokens"`
	OutputTokens             int64 `json:"output_tokens"`
}

func parseTime(raw string) (time.Time, error) {
	if raw == "" {
		return time.Time{}, nil
	}
	ts, err := time.Parse(time.RFC3339Nano, raw)
	if err == nil {
		return ts, nil
	}
	return time.Parse(time.RFC3339, raw)
}

func globJSONL(root string) ([]string, error) {
	matches, err := filepath.Glob(filepath.Join(root, "*.jsonl"))
	if err != nil {
		return nil, err
	}
	sort.Strings(matches)
	return matches, nil
}

func walkJSONL(root string) ([]string, error) {
	var paths []string
	err := filepath.WalkDir(root, func(path string, entry fs.DirEntry, err error) error {
		if err != nil {
			if errors.Is(err, os.ErrPermission) {
				return nil
			}
			return err
		}
		if entry.IsDir() {
			return nil
		}
		if strings.EqualFold(filepath.Ext(path), ".jsonl") {
			paths = append(paths, path)
		}
		return nil
	})
	if errors.Is(err, os.ErrNotExist) {
		return paths, nil
	}
	sort.Strings(paths)
	return paths, err
}

func dedupe(events []Event) []Event {
	seen := map[string]bool{}
	out := make([]Event, 0, len(events))
	for _, event := range events {
		key := event.dedupKey
		if key == "" {
			key = fmt.Sprintf("%s:%s:%s:%s:%s", event.Provider, event.SessionID, event.RequestID, event.Timestamp.Format(time.RFC3339Nano), event.SourcePath)
		}
		if seen[key] {
			continue
		}
		seen[key] = true
		out = append(out, event)
	}
	sortEvents(out)
	return out
}

func sortEvents(events []Event) {
	sort.Slice(events, func(i, j int) bool {
		if events[i].Timestamp.Equal(events[j].Timestamp) {
			return events[i].Provider < events[j].Provider
		}
		return events[i].Timestamp.Before(events[j].Timestamp)
	})
}

func recentFiles(paths []string, since time.Time) []string {
	if since.IsZero() {
		return paths
	}
	out := paths[:0]
	for _, path := range paths {
		info, err := os.Stat(path)
		if err != nil {
			if errors.Is(err, os.ErrNotExist) {
				continue
			}
			out = append(out, path)
			continue
		}
		if !info.ModTime().Before(since) {
			out = append(out, path)
		}
	}
	return out
}
