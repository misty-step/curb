package usagewatch

import (
	"context"
	"fmt"
	"path/filepath"
	"sort"
	"strings"
	"time"

	"github.com/phaedrus/curb/internal/config"
	"github.com/phaedrus/curb/internal/ledger"
	"github.com/phaedrus/curb/internal/platform"
	"github.com/phaedrus/curb/internal/usage"
	"github.com/phaedrus/curb/internal/watchdog"
)

type EventReader func(time.Time) ([]usage.Event, []usage.SourceReport, error)
type Capture func(context.Context) (*platform.Snapshot, error)
type Notify func(string, string) error
type Terminate func(context.Context, platform.TerminationTarget, time.Duration) platform.TerminationResult

type Service struct {
	cfg       *config.Config
	ledger    *ledger.Ledger
	reader    EventReader
	capture   Capture
	notify    Notify
	terminate Terminate
	now       func() time.Time
	onEvent   func(ledger.Event)
	warned    map[string]bool
	grace     map[string]time.Time
	targets   map[string]platform.Process
}

type Session struct {
	Key            string
	Provider       string
	SessionID      string
	CWD            string
	Last           time.Time
	LastUsage      time.Time
	Events         int
	Input          int64
	CachedInput    int64
	CacheCreation  int64
	Output         int64
	Reasoning      int64
	Total          int64
	LastTurnTokens int64
	Models         []string
}

type PolicyResult struct {
	State       string
	Active      bool
	WindowStart time.Time
	Explanation string
}

type SessionDecision struct {
	State       string
	UsageState  string
	Actionable  bool
	Policy      PolicyResult
	Explanation string
}

type SessionClassification struct {
	State          string
	ProcessState   string
	UsageState     string
	ActionState    string
	Actionable     bool
	CanAcknowledge bool
	RiskRank       int
	Explanation    string
}

type Correlation struct {
	Matched    bool
	Agent      config.Agent
	Process    platform.Process
	Score      int
	Reason     string
	Confidence int
	Evidence   []string
}

func New(cfg *config.Config, l *ledger.Ledger) *Service {
	return &Service{
		cfg:       cfg,
		ledger:    l,
		reader:    usage.EventsSince,
		capture:   platform.Capture,
		notify:    platform.Notify,
		terminate: platform.TerminateTree,
		now:       time.Now,
		warned:    map[string]bool{},
		grace:     map[string]time.Time{},
		targets:   map[string]platform.Process{},
	}
}

func (s *Service) OnEvent(fn func(ledger.Event)) {
	s.onEvent = fn
}

func (s *Service) SetCapture(capture Capture) {
	if capture != nil {
		s.capture = capture
	}
}

func (s *Service) SetNotify(notify Notify) {
	if notify != nil {
		s.notify = notify
	}
}

func (s *Service) SetTerminate(terminate Terminate) {
	if terminate != nil {
		s.terminate = terminate
	}
}

func (s *Service) SetReader(reader EventReader) {
	if reader != nil {
		s.reader = reader
	}
}

func (s *Service) Reconfigure(cfg *config.Config, l *ledger.Ledger) {
	s.cfg = cfg
	s.ledger = l
}

func (s *Service) Run(ctx context.Context) error {
	if !s.cfg.Usage.IsEnabled() {
		return nil
	}
	ticker := time.NewTicker(s.cfg.Usage.ScanInterval.Duration)
	defer ticker.Stop()
	if err := s.Scan(ctx); err != nil {
		return err
	}
	for {
		select {
		case <-ctx.Done():
			return nil
		case <-ticker.C:
			if err := s.Scan(ctx); err != nil {
				_ = s.append(ledger.Event{Type: "usage_scan_failed", Mode: string(s.cfg.Mode), Message: err.Error()})
			}
		}
	}
}

func (s *Service) Scan(ctx context.Context) error {
	now := s.now()
	events, _, err := s.reader(now.Add(-s.cfg.Usage.Lookback.Duration))
	if err != nil {
		return err
	}
	sessions := BuildSessions(events)
	if len(sessions) == 0 {
		return nil
	}
	snap, err := s.capture(ctx)
	if err != nil {
		return err
	}
	matches := processMatches(s.cfg, s.ledger, snap)
	for _, session := range sessions {
		policy := EvaluateSessionPolicy(session, s.cfg.Usage, now)
		if policy.State != "warn" && policy.State != "stop" {
			continue
		}
		_, ok, err := ActiveSessionAck(s.cfg.Service.StateDir, session.Key, now)
		if err != nil {
			_ = s.append(ledger.Event{Type: "usage_ack_failed", Mode: string(s.cfg.Mode), Message: err.Error(), Data: map[string]any{"session_key": session.Key}})
		}
		if ok {
			s.suppressUntilAckExpires(session.Key)
			continue
		}
		correlation := Correlate(session, matches)
		if err := s.evaluate(ctx, snap, session, correlation, now); err != nil {
			return err
		}
	}
	return nil
}

func BuildSessions(events []usage.Event) []Session {
	byKey := map[string]*Session{}
	models := map[string]map[string]bool{}
	for _, event := range events {
		key := SessionKey(event.Provider, event.SessionID, event.SourcePath)
		session := byKey[key]
		if session == nil {
			session = &Session{Key: key, Provider: event.Provider, SessionID: event.SessionID, CWD: event.CWD}
			byKey[key] = session
			models[key] = map[string]bool{}
		}
		session.Events++
		session.Input += event.Input
		session.CachedInput += event.CachedInput
		session.CacheCreation += event.CacheCreation
		session.Output += event.Output
		session.Reasoning += event.Reasoning
		session.Total += event.Total
		if !event.Timestamp.Before(session.Last) {
			session.Last = event.Timestamp
		}
		if event.Total > 0 && !event.Timestamp.Before(session.LastUsage) {
			session.LastUsage = event.Timestamp
			session.LastTurnTokens = event.Total
		}
		if session.CWD == "" {
			session.CWD = event.CWD
		}
		if event.Model != "" {
			models[key][event.Model] = true
		}
	}
	out := make([]Session, 0, len(byKey))
	for key, session := range byKey {
		for model := range models[key] {
			session.Models = append(session.Models, model)
		}
		sort.Strings(session.Models)
		out = append(out, *session)
	}
	sort.Slice(out, func(i, j int) bool { return out[i].Last.After(out[j].Last) })
	return out
}

func SessionKey(provider, sessionID, sourcePath string) string {
	if sessionID != "" {
		return provider + ":" + sessionID
	}
	return provider + ":" + sourcePath
}

func (s Session) RecentUsage(windowStart time.Time) bool {
	return !s.LastUsage.IsZero() && !s.LastUsage.Before(windowStart)
}

func EvaluateSessionPolicy(session Session, usageCfg config.UsageConfig, now time.Time) PolicyResult {
	if now.IsZero() {
		now = time.Now()
	}
	windowStart := now.Add(-usageCfg.Window.Duration)
	active := session.RecentUsage(windowStart)
	result := PolicyResult{
		State:       "idle",
		Active:      active,
		WindowStart: windowStart,
		Explanation: "no usage in current window",
	}
	if active {
		result.State = "active"
		result.Explanation = "latest turn within limits"
	}
	switch {
	case active && session.LastTurnTokens >= usageCfg.KillTurnTokens:
		result.State = "stop"
		result.Explanation = "latest turn over stop threshold"
	case active && session.LastTurnTokens >= usageCfg.WarnTurnTokens:
		result.State = "warn"
		result.Explanation = "latest turn over warning threshold"
	case !active && session.LastTurnTokens >= usageCfg.KillTurnTokens:
		result.State = "idle-high"
		result.Explanation = "large historical turn, not currently spending"
	}
	return result
}

func Correlate(session Session, matches []watchdog.Match) Correlation {
	var best Correlation
	sessionCWD := cleanPath(session.CWD)
	if sessionCWD == "" {
		return best
	}
	for _, match := range matches {
		if !sameProvider(session.Provider, match.Agent.Family) {
			continue
		}
		processCWD := cleanPath(match.Process.CWD)
		if processCWD == "" {
			continue
		}
		score := 25
		reason := "provider"
		switch {
		case processCWD == sessionCWD:
			score += 100
			reason = "provider+cwd"
		case pathContains(processCWD, sessionCWD) || pathContains(sessionCWD, processCWD):
			score += 50
			reason = "provider+cwd-prefix"
		default:
			continue
		}
		if score > best.Score {
			best = Correlation{
				Matched:    true,
				Agent:      match.Agent,
				Process:    match.Process,
				Score:      score,
				Reason:     reason,
				Confidence: match.Confidence,
				Evidence:   append([]string(nil), match.Evidence...),
			}
		}
	}
	return best
}

func EvaluateSessionDecision(session Session, cfg *config.Config, correlation Correlation, now time.Time) SessionDecision {
	policy := EvaluateSessionPolicy(session, cfg.Usage, now)
	decision := SessionDecision{
		State:       policy.State,
		UsageState:  policy.State,
		Policy:      policy,
		Explanation: policy.Explanation,
	}
	if policy.State == "idle" {
		decision.UsageState = ""
		return decision
	}
	if policy.State != "warn" && policy.State != "stop" {
		return decision
	}
	switch {
	case !correlation.Matched:
		decision.State = "uncorrelated"
		decision.Explanation = "usage crossed threshold, but no live process matched; Curb will not stop anything"
	case !correlation.Agent.TerminationAllowed():
		decision.State = "watch-only"
		decision.Explanation = "usage crossed threshold, but matched agent is watch-only; Curb will not stop desktop apps"
	case cfg.Mode == config.ModeEnforcement && policy.State == "stop":
		decision.Actionable = true
	}
	return decision
}

func ClassifySession(decision SessionDecision, correlation Correlation, mode config.Mode, acknowledgedUntil *time.Time, ackExtension time.Duration) SessionClassification {
	classification := SessionClassification{
		State:       decision.State,
		UsageState:  sessionUsageState(decision),
		Actionable:  decision.Actionable,
		Explanation: decision.Explanation,
	}
	if acknowledgedUntil != nil && (classification.UsageState == "warn" || classification.UsageState == "stop") {
		classification.State = "acknowledged"
		classification.Actionable = false
		classification.Explanation = "usage crossed threshold, but this session is acknowledged until " + acknowledgedUntil.Format(time.RFC3339)
	}
	classification.ProcessState = sessionProcessState(classification, correlation)
	classification.ActionState = sessionActionState(classification, mode, correlation, acknowledgedUntil != nil)
	classification.CanAcknowledge = acknowledgedUntil == nil && ackExtension > 0 && (classification.UsageState == "warn" || classification.UsageState == "stop")
	classification.RiskRank = sessionRiskRank(classification)
	return classification
}

func sessionProcessState(classification SessionClassification, correlation Correlation) string {
	switch {
	case classification.State == "watch-only":
		return "watch-only"
	case correlation.Matched:
		return "running"
	case classification.State == "uncorrelated" || classification.UsageState == "warn" || classification.UsageState == "stop":
		return "unknown"
	default:
		return "no-process"
	}
}

func sessionUsageState(decision SessionDecision) string {
	switch {
	case decision.UsageState == "warn" || decision.UsageState == "stop":
		return decision.UsageState
	case decision.State == "warn" || decision.State == "stop":
		return decision.State
	case decision.State == "active":
		return "spending"
	case decision.State == "idle-high":
		return "quiet-high"
	default:
		return "quiet"
	}
}

func sessionActionState(classification SessionClassification, mode config.Mode, correlation Correlation, acknowledged bool) string {
	if acknowledged {
		return "acknowledged"
	}
	if classification.UsageState == "stop" || classification.State == "stop" {
		if classification.Actionable {
			return "stop-pending"
		}
		if mode != config.ModeEnforcement && correlation.Matched && classification.ProcessState == "running" {
			return "would-stop"
		}
		return "blocked"
	}
	if classification.UsageState == "warn" || classification.State == "warn" {
		return "acknowledge"
	}
	return "none"
}

func sessionRiskRank(classification SessionClassification) int {
	switch {
	case classification.Actionable:
		return 0
	case classification.State == "acknowledged":
		return 2
	case classification.State == "idle-high":
		return 3
	case classification.UsageState == "stop" || classification.UsageState == "warn" || classification.UsageState == "spending" || classification.State == "active":
		return 1
	case classification.ProcessState == "running":
		return 4
	default:
		return 5
	}
}

func (s *Service) suppressUntilAckExpires(sessionKey string) {
	s.warned[sessionKey] = false
	s.warned["would:"+sessionKey] = false
	s.warned["uncorrelated:"+sessionKey] = false
	delete(s.grace, sessionKey)
	delete(s.targets, sessionKey)
}

func (s *Service) evaluate(ctx context.Context, snap *platform.Snapshot, session Session, correlation Correlation, now time.Time) error {
	key := session.Key
	policy := EvaluateSessionPolicy(session, s.cfg.Usage, now)
	overKill := policy.State == "stop"
	msg := fmt.Sprintf("%s session %s latest turn used %s tokens (total %s in %d calls)",
		session.Provider, shortID(session.SessionID), formatTokens(session.LastTurnTokens), formatTokens(session.Total), session.Events)

	if !s.warned[key] {
		s.warned[key] = true
		if s.cfg.Alerts.LocalNotifications {
			if err := s.notify("Curb usage warning", msg); err != nil {
				_ = s.append(ledger.Event{Type: "notification_failed", Mode: string(s.cfg.Mode), Message: err.Error()})
			}
		}
		if err := s.append(ledger.Event{
			Type:    "usage_warning",
			AgentID: correlation.Agent.ID,
			Mode:    string(s.cfg.Mode),
			Message: msg,
			Data:    eventData(session, correlation),
		}); err != nil {
			return err
		}
	}
	if !overKill {
		return nil
	}
	if !correlation.Matched {
		if !s.warned["uncorrelated:"+key] {
			s.warned["uncorrelated:"+key] = true
			return s.append(ledger.Event{
				Type:    "usage_kill_blocked",
				Mode:    string(s.cfg.Mode),
				Message: "usage threshold exceeded but no live process correlation was found",
				Data:    eventData(session, correlation),
			})
		}
		return nil
	}
	if !correlation.Agent.TerminationAllowed() {
		return s.append(ledger.Event{
			Type:    "usage_kill_blocked",
			AgentID: correlation.Agent.ID,
			Mode:    string(s.cfg.Mode),
			Message: "usage threshold exceeded but matched agent is watch-only",
			Data:    eventData(session, correlation),
		})
	}
	if s.cfg.Mode != config.ModeEnforcement {
		if !s.warned["would:"+key] {
			s.warned["would:"+key] = true
			if s.cfg.Alerts.LocalNotifications {
				_ = s.notify("Curb would stop agent", msg)
			}
			return s.append(ledger.Event{
				Type:    "usage_would_terminate",
				AgentID: correlation.Agent.ID,
				Mode:    string(s.cfg.Mode),
				Message: msg,
				Data:    eventData(session, correlation),
			})
		}
		return nil
	}
	started := s.grace[key]
	if started.IsZero() {
		s.grace[key] = now
		s.targets[key] = correlation.Process
		if s.cfg.Alerts.LocalNotifications {
			_ = s.notify("Curb usage grace period", msg)
		}
		return s.append(ledger.Event{
			Type:    "usage_grace_started",
			AgentID: correlation.Agent.ID,
			Mode:    string(s.cfg.Mode),
			Message: msg,
			Data:    eventData(session, correlation),
		})
	}
	if now.Sub(started) < s.cfg.Usage.GracePeriod.Duration {
		return nil
	}

	target := correlation.Process
	if stored, ok := s.targets[key]; ok {
		target = stored
	}
	terminationCorrelation := correlation
	terminationCorrelation.Process = target
	terminationTarget, ok := snap.TerminationTarget(target)
	if !ok {
		return s.append(ledger.Event{
			Type:    "usage_termination_failed",
			AgentID: correlation.Agent.ID,
			Mode:    string(s.cfg.Mode),
			Message: "safety guard rejected termination",
			Data:    eventData(session, terminationCorrelation),
		})
	}
	if err := s.append(ledger.Event{
		Type:    "usage_termination_started",
		AgentID: correlation.Agent.ID,
		Mode:    string(s.cfg.Mode),
		Message: msg,
		Data:    eventData(session, terminationCorrelation),
	}); err != nil {
		return err
	}
	result := s.terminate(ctx, terminationTarget, s.cfg.Usage.GracePeriod.Duration)
	data := eventData(session, terminationCorrelation)
	data["result"] = result
	return s.append(ledger.Event{
		Type:    "usage_termination_completed",
		AgentID: correlation.Agent.ID,
		Mode:    string(s.cfg.Mode),
		Message: msg,
		Data:    data,
	})
}

func (s *Service) append(event ledger.Event) error {
	if s.ledger != nil {
		if err := s.ledger.Append(event); err != nil {
			return err
		}
	}
	if s.onEvent != nil {
		s.onEvent(event)
	}
	return nil
}

func processMatches(cfg *config.Config, l *ledger.Ledger, snap *platform.Snapshot) []watchdog.Match {
	service := watchdog.New(cfg, l)
	return service.Match(snap)
}

func eventData(session Session, correlation Correlation) map[string]any {
	data := map[string]any{
		"provider":     session.Provider,
		"session_id":   session.SessionID,
		"cwd":          session.CWD,
		"calls":        session.Events,
		"total_tokens": session.Total,
		"turn_tokens":  session.LastTurnTokens,
		"last":         session.Last.UTC(),
	}
	if !session.LastUsage.IsZero() {
		data["last_usage"] = session.LastUsage.UTC()
	}
	if len(session.Models) > 0 {
		data["models"] = session.Models
	}
	if correlation.Matched {
		data["pid"] = correlation.Process.PID
		data["agent_id"] = correlation.Agent.ID
		data["correlation"] = correlation.Reason
		data["correlation_score"] = correlation.Score
	}
	return data
}

func sameProvider(provider, family string) bool {
	return strings.EqualFold(provider, family)
}

func cleanPath(path string) string {
	if path == "" {
		return ""
	}
	if abs, err := filepath.Abs(path); err == nil {
		path = abs
	}
	return filepath.Clean(path)
}

func pathContains(parent, child string) bool {
	if parent == "" || child == "" || parent == child {
		return false
	}
	if !strings.HasPrefix(child, parent) {
		return false
	}
	if strings.HasSuffix(parent, string(filepath.Separator)) {
		return true
	}
	return len(child) > len(parent) && child[len(parent)] == filepath.Separator
}

func shortID(id string) string {
	if len(id) <= 12 {
		return id
	}
	return id[:8] + "..." + id[len(id)-4:]
}

func formatTokens(n int64) string {
	if n >= 1_000_000 {
		return fmt.Sprintf("%.1fM", float64(n)/1_000_000)
	}
	if n >= 10_000 {
		return fmt.Sprintf("%dk", n/1_000)
	}
	return fmt.Sprintf("%d", n)
}
