package usage

import (
	"os"
	"path/filepath"
	"strconv"
	"testing"
	"time"
)

func TestCodexArchivedSessionsExtractsTokenCountsWithoutContent(t *testing.T) {
	dir := t.TempDir()
	path := filepath.Join(dir, "rollout.jsonl")
	content := `{"timestamp":"2026-05-19T16:00:00Z","type":"session_meta","payload":{"id":"session_codex","cwd":"/repo"}}
{"timestamp":"2026-05-19T16:01:00Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":100,"cached_input_tokens":20,"output_tokens":5,"reasoning_output_tokens":2,"total_tokens":107},"total_token_usage":{"total_tokens":107},"model_context_window":258400}}}
{"timestamp":"2026-05-19T16:02:00Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":100,"cached_input_tokens":20,"output_tokens":5,"reasoning_output_tokens":2,"total_tokens":107},"total_token_usage":{"total_tokens":107},"model_context_window":258400}}}
`
	if err := os.WriteFile(path, []byte(content), 0o600); err != nil {
		t.Fatal(err)
	}

	events, report, err := CodexArchivedSessions(dir)
	if err != nil {
		t.Fatal(err)
	}
	if report.Files != 1 || report.Events != 1 {
		t.Fatalf("report = %#v", report)
	}
	if len(events) != 1 {
		t.Fatalf("events = %d", len(events))
	}
	event := events[0]
	if event.Provider != "codex" || event.SessionID != "session_codex" {
		t.Fatalf("event identity = %#v", event)
	}
	if event.Input != 100 || event.CachedInput != 20 || event.Output != 5 || event.Reasoning != 2 || event.Total != 107 {
		t.Fatalf("event tokens = %#v", event)
	}
	if event.CWD != "/repo" || event.ModelContext != 258400 {
		t.Fatalf("event metadata = %#v", event)
	}
}

func TestReaderCachesUnchangedFilesAndReadsAppends(t *testing.T) {
	home := t.TempDir()
	dir := filepath.Join(home, ".codex", "archived_sessions")
	if err := os.MkdirAll(dir, 0o700); err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(dir, "rollout.jsonl")
	first := codexFixture("session_codex", "/repo", "2026-05-19T16:00:00Z", 107, 107)
	if err := os.WriteFile(path, []byte(first), 0o600); err != nil {
		t.Fatal(err)
	}
	reader := NewReader(home)

	events, report, err := reader.EventsSince(time.Time{})
	if err != nil {
		t.Fatal(err)
	}
	if len(events) != 1 || report[0].Events != 1 {
		t.Fatalf("initial events=%#v report=%#v", events, report)
	}
	info, err := os.Stat(path)
	if err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, []byte(stringsOfLength("not-json", int(info.Size()))), 0o600); err != nil {
		t.Fatal(err)
	}
	if err := os.Chtimes(path, info.ModTime(), info.ModTime()); err != nil {
		t.Fatal(err)
	}
	events, _, err = reader.EventsSince(time.Time{})
	if err != nil {
		t.Fatal(err)
	}
	if len(events) != 1 || events[0].SessionID != "session_codex" {
		t.Fatalf("cached events = %#v", events)
	}
	events[0].SessionID = "mutated"
	events, _, err = reader.EventsSince(time.Time{})
	if err != nil {
		t.Fatal(err)
	}
	if len(events) != 1 || events[0].SessionID != "session_codex" {
		t.Fatalf("cache was mutated by caller: %#v", events)
	}

	second := first + codexTokenRow("2026-05-19T16:02:00Z", 211, 318)
	if err := os.WriteFile(path, []byte(second), 0o600); err != nil {
		t.Fatal(err)
	}
	events, report, err = reader.EventsSince(time.Time{})
	if err != nil {
		t.Fatal(err)
	}
	if len(events) != 2 || report[0].Events != 2 {
		t.Fatalf("appended events=%#v report=%#v", events, report)
	}
	events, report, err = reader.EventsSince(time.Date(2026, 5, 19, 16, 1, 0, 0, time.UTC))
	if err != nil {
		t.Fatal(err)
	}
	if len(events) != 1 || report[0].Events != 1 || events[0].Total != 211 {
		t.Fatalf("filtered events=%#v report=%#v", events, report)
	}
}

func TestReaderPrunesDeletedProviderFiles(t *testing.T) {
	home := t.TempDir()
	dir := filepath.Join(home, ".codex", "archived_sessions")
	if err := os.MkdirAll(dir, 0o700); err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(dir, "rollout.jsonl")
	if err := os.WriteFile(path, []byte(codexFixture("session_codex", "/repo", "2026-05-19T16:00:00Z", 107, 107)), 0o600); err != nil {
		t.Fatal(err)
	}
	reader := NewReader(home)
	events, _, err := reader.EventsSince(time.Time{})
	if err != nil {
		t.Fatal(err)
	}
	if len(events) != 1 {
		t.Fatalf("initial events = %#v", events)
	}
	if err := os.Remove(path); err != nil {
		t.Fatal(err)
	}
	events, report, err := reader.EventsSince(time.Time{})
	if err != nil {
		t.Fatal(err)
	}
	if len(events) != 0 || report[0].Events != 0 || report[0].Files != 0 {
		t.Fatalf("deleted file remained cached: events=%#v report=%#v", events, report)
	}
}

func TestReaderReportsChangedInvalidJSONAndClearsCache(t *testing.T) {
	home := t.TempDir()
	dir := filepath.Join(home, ".codex", "archived_sessions")
	if err := os.MkdirAll(dir, 0o700); err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(dir, "rollout.jsonl")
	if err := os.WriteFile(path, []byte(codexFixture("session_codex", "/repo", "2026-05-19T16:00:00Z", 107, 107)), 0o600); err != nil {
		t.Fatal(err)
	}
	reader := NewReader(home)
	if _, _, err := reader.EventsSince(time.Time{}); err != nil {
		t.Fatal(err)
	}
	if err := os.WriteFile(path, []byte(`{"bad"`+"\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	events, reports, err := reader.EventsSince(time.Time{})
	if err == nil {
		t.Fatal("expected invalid JSON to fail")
	}
	if len(events) != 0 || reports[0].Error == "" {
		t.Fatalf("events=%#v reports=%#v err=%v", events, reports, err)
	}
	if err := os.WriteFile(path, []byte(codexFixture("session_codex", "/repo", "2026-05-19T16:03:00Z", 211, 211)), 0o600); err != nil {
		t.Fatal(err)
	}
	events, _, err = reader.EventsSince(time.Time{})
	if err != nil {
		t.Fatal(err)
	}
	if len(events) != 1 || events[0].Total != 211 {
		t.Fatalf("recovered events = %#v", events)
	}
}

func TestReaderIncludesLateWrittenOlderTimestampRows(t *testing.T) {
	home := t.TempDir()
	dir := filepath.Join(home, ".codex", "archived_sessions")
	if err := os.MkdirAll(dir, 0o700); err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(dir, "rollout.jsonl")
	if err := os.WriteFile(path, []byte(codexFixture("session_codex", "/repo", "2026-05-19T16:00:00Z", 107, 107)), 0o600); err != nil {
		t.Fatal(err)
	}
	reader := NewReader(home)
	if _, _, err := reader.EventsSince(time.Date(2026, 5, 19, 15, 0, 0, 0, time.UTC)); err != nil {
		t.Fatal(err)
	}
	lateOlderRow := codexFixture("session_codex", "/repo", "2026-05-19T15:30:00Z", 211, 318)
	if err := os.WriteFile(path, []byte(lateOlderRow), 0o600); err != nil {
		t.Fatal(err)
	}
	events, _, err := reader.EventsSince(time.Date(2026, 5, 19, 15, 0, 0, 0, time.UTC))
	if err != nil {
		t.Fatal(err)
	}
	if len(events) != 1 || events[0].Timestamp.Format(time.RFC3339) != "2026-05-19T15:30:00Z" {
		t.Fatalf("late-written events = %#v", events)
	}
}

func TestReaderIncludesAppendedOlderTimestampRows(t *testing.T) {
	home := t.TempDir()
	dir := filepath.Join(home, ".codex", "archived_sessions")
	if err := os.MkdirAll(dir, 0o700); err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(dir, "rollout.jsonl")
	initial := codexFixture("session_codex", "/repo", "2026-05-19T16:00:00Z", 107, 107)
	if err := os.WriteFile(path, []byte(initial), 0o600); err != nil {
		t.Fatal(err)
	}
	reader := NewReader(home)
	if _, _, err := reader.EventsSince(time.Date(2026, 5, 19, 15, 0, 0, 0, time.UTC)); err != nil {
		t.Fatal(err)
	}
	file, err := os.OpenFile(path, os.O_APPEND|os.O_WRONLY, 0)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := file.WriteString(codexTokenRow("2026-05-19T15:30:00Z", 211, 318)); err != nil {
		_ = file.Close()
		t.Fatal(err)
	}
	if err := file.Close(); err != nil {
		t.Fatal(err)
	}

	events, _, err := reader.EventsSince(time.Date(2026, 5, 19, 15, 0, 0, 0, time.UTC))
	if err != nil {
		t.Fatal(err)
	}
	if !hasEvent(events, 211, "2026-05-19T15:30:00Z") {
		t.Fatalf("appended older events = %#v", events)
	}
	events, _, err = reader.EventsSince(time.Date(2026, 5, 19, 16, 30, 0, 0, time.UTC))
	if err != nil {
		t.Fatal(err)
	}
	if hasEvent(events, 211, "2026-05-19T15:30:00Z") {
		t.Fatalf("events older than since were returned: %#v", events)
	}
}

func TestReaderPersistsCacheAcrossRestartAndParsesOnlyAppendedBytes(t *testing.T) {
	home := t.TempDir()
	stateDir := filepath.Join(t.TempDir(), "state")
	dir := filepath.Join(home, ".codex", "archived_sessions")
	if err := os.MkdirAll(dir, 0o700); err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(dir, "rollout.jsonl")
	initial := codexFixture("session_codex", "/repo", "2026-05-19T16:00:00Z", 107, 107)
	if err := os.WriteFile(path, []byte(initial), 0o600); err != nil {
		t.Fatal(err)
	}
	reader := NewReaderWithState(home, stateDir)
	if _, _, err := reader.EventsSince(time.Time{}); err != nil {
		t.Fatal(err)
	}

	restarted := NewReaderWithState(home, stateDir)
	file, err := os.OpenFile(path, os.O_APPEND|os.O_WRONLY, 0)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := file.WriteString(codexTokenRow("2026-05-19T16:02:00Z", 211, 318)); err != nil {
		_ = file.Close()
		t.Fatal(err)
	}
	if err := file.Close(); err != nil {
		t.Fatal(err)
	}
	events, _, err := restarted.EventsSince(time.Time{})
	if err != nil {
		t.Fatal(err)
	}
	if !hasEvent(events, 107, "2026-05-19T16:00:00Z") || !hasEvent(events, 211, "2026-05-19T16:02:00Z") {
		t.Fatalf("persisted append events = %#v", events)
	}
}

func TestReaderRejectsSamePathReplacementAsAppend(t *testing.T) {
	home := t.TempDir()
	stateDir := filepath.Join(t.TempDir(), "state")
	dir := filepath.Join(home, ".codex", "archived_sessions")
	if err := os.MkdirAll(dir, 0o700); err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(dir, "rollout.jsonl")
	initial := codexFixture("session_codex", "/repo", "2026-05-19T16:00:00Z", 107, 107)
	if err := os.WriteFile(path, []byte(initial), 0o600); err != nil {
		t.Fatal(err)
	}
	reader := NewReaderWithState(home, stateDir)
	if _, _, err := reader.EventsSince(time.Time{}); err != nil {
		t.Fatal(err)
	}

	restarted := NewReaderWithState(home, stateDir)
	replaced := stringsOfLength("not-json", len(initial)) + codexTokenRow("2026-05-19T16:02:00Z", 211, 318)
	if err := os.WriteFile(path, []byte(replaced), 0o600); err != nil {
		t.Fatal(err)
	}
	events, reports, err := restarted.EventsSince(time.Time{})
	if err == nil {
		t.Fatal("expected replaced invalid prefix to fail full reparse")
	}
	if len(events) != 0 || reports[0].Error == "" {
		t.Fatalf("replacement leaked cached events: events=%#v reports=%#v err=%v", events, reports, err)
	}
}

func TestReaderRejectsSamePathReplacementAfterUnchangedPrefix(t *testing.T) {
	home := t.TempDir()
	stateDir := filepath.Join(t.TempDir(), "state")
	dir := filepath.Join(home, ".codex", "archived_sessions")
	if err := os.MkdirAll(dir, 0o700); err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(dir, "rollout.jsonl")
	prefix := stringsOfLength(" ", 4096)
	initial := prefix + codexFixture("session_codex", "/repo", "2026-05-19T16:00:00Z", 107, 107)
	if err := os.WriteFile(path, []byte(initial), 0o600); err != nil {
		t.Fatal(err)
	}
	reader := NewReaderWithState(home, stateDir)
	if _, _, err := reader.EventsSince(time.Time{}); err != nil {
		t.Fatal(err)
	}

	restarted := NewReaderWithState(home, stateDir)
	replaced := prefix + stringsOfLength("not-json", len(initial)-len(prefix)) + codexTokenRow("2026-05-19T16:02:00Z", 211, 318)
	if err := os.WriteFile(path, []byte(replaced), 0o600); err != nil {
		t.Fatal(err)
	}
	events, reports, err := restarted.EventsSince(time.Time{})
	if err == nil {
		t.Fatal("expected changed cached prefix after 4096 bytes to fail full reparse")
	}
	if len(events) != 0 || reports[0].Error == "" {
		t.Fatalf("replacement leaked cached events: events=%#v reports=%#v err=%v", events, reports, err)
	}
}

func TestReaderHydratesPersistedDedupKeys(t *testing.T) {
	home := t.TempDir()
	stateDir := filepath.Join(t.TempDir(), "state")
	dir := filepath.Join(home, ".codex", "archived_sessions")
	if err := os.MkdirAll(dir, 0o700); err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(dir, "rollout.jsonl")
	initial := codexFixture("session_codex", "/repo", "2026-05-19T16:00:00Z", 107, 107)
	if err := os.WriteFile(path, []byte(initial), 0o600); err != nil {
		t.Fatal(err)
	}
	reader := NewReaderWithState(home, stateDir)
	if _, _, err := reader.EventsSince(time.Time{}); err != nil {
		t.Fatal(err)
	}

	restarted := NewReaderWithState(home, stateDir)
	file, err := os.OpenFile(path, os.O_APPEND|os.O_WRONLY, 0)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := file.WriteString(codexTokenRow("2026-05-19T16:01:00Z", 107, 107)); err != nil {
		_ = file.Close()
		t.Fatal(err)
	}
	if err := file.Close(); err != nil {
		t.Fatal(err)
	}
	events, _, err := restarted.EventsSince(time.Time{})
	if err != nil {
		t.Fatal(err)
	}
	if len(events) != 1 {
		t.Fatalf("duplicate persisted event was not deduped: %#v", events)
	}
}

func TestClaudeProjectsDedupesRequestsAndSummarizesModelUsage(t *testing.T) {
	root := filepath.Join(t.TempDir(), "projects", "-repo")
	if err := os.MkdirAll(root, 0o700); err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(root, "session.jsonl")
	content := `{"timestamp":"2026-05-19T20:00:00Z","requestId":"req_1","sessionId":"session_claude","uuid":"turn_1","cwd":"/repo","message":{"id":"msg_1","model":"claude-opus-4-7","usage":{"input_tokens":1,"cache_creation_input_tokens":30,"cache_read_input_tokens":40,"output_tokens":5}}}
{"timestamp":"2026-05-19T20:00:01Z","requestId":"req_1","sessionId":"session_claude","uuid":"turn_1_dup","cwd":"/repo","message":{"id":"msg_1","model":"claude-opus-4-7","usage":{"input_tokens":1,"cache_creation_input_tokens":30,"cache_read_input_tokens":40,"output_tokens":5}}}
{"timestamp":"2026-05-19T20:01:00Z","requestId":"req_2","sessionId":"session_claude","uuid":"turn_2","cwd":"/repo","message":{"id":"msg_2","model":"claude-sonnet-4-5","usage":{"input_tokens":2,"cache_creation_input_tokens":3,"cache_read_input_tokens":4,"output_tokens":6}}}
`
	if err := os.WriteFile(path, []byte(content), 0o600); err != nil {
		t.Fatal(err)
	}

	events, report, err := ClaudeProjects(filepath.Dir(root))
	if err != nil {
		t.Fatal(err)
	}
	if report.Files != 1 || report.Events != 2 {
		t.Fatalf("report = %#v", report)
	}
	if len(events) != 2 {
		t.Fatalf("events = %d", len(events))
	}
	summaries := Summarize(events)
	if len(summaries) != 1 {
		t.Fatalf("summaries = %d", len(summaries))
	}
	summary := summaries[0]
	if summary.Provider != "claude" || summary.SessionID != "session_claude" {
		t.Fatalf("summary identity = %#v", summary)
	}
	if summary.Input != 3 || summary.CacheCreation != 33 || summary.CachedInput != 44 || summary.Output != 11 || summary.Total != 91 {
		t.Fatalf("summary tokens = %#v", summary)
	}
	if len(summary.Models) != 2 || summary.Models[0] != "claude-opus-4-7" || summary.Models[1] != "claude-sonnet-4-5" {
		t.Fatalf("models = %#v", summary.Models)
	}
}

func codexFixture(sessionID, cwd, at string, total, cumulative int64) string {
	return `{"timestamp":"` + at + `","type":"session_meta","payload":{"id":"` + sessionID + `","cwd":"` + cwd + `"}}
` + codexTokenRow(at, total, cumulative)
}

func codexTokenRow(at string, total, cumulative int64) string {
	return `{"timestamp":"` + at + `","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":100,"cached_input_tokens":20,"output_tokens":5,"reasoning_output_tokens":2,"total_tokens":` + itoa(total) + `},"total_token_usage":{"total_tokens":` + itoa(cumulative) + `},"model_context_window":258400}}}
`
}

func itoa(n int64) string {
	return strconv.FormatInt(n, 10)
}

func stringsOfLength(seed string, length int) string {
	out := seed
	for len(out) < length {
		out += seed
	}
	return out[:length]
}

func hasEvent(events []Event, total int64, at string) bool {
	for _, event := range events {
		if event.Total == total && event.Timestamp.Format(time.RFC3339) == at {
			return true
		}
	}
	return false
}
