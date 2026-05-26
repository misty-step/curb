package usagewatch

import (
	"context"
	"path/filepath"
	"testing"
	"time"

	"github.com/phaedrus/curb/internal/config"
	"github.com/phaedrus/curb/internal/ledger"
	"github.com/phaedrus/curb/internal/platform"
	"github.com/phaedrus/curb/internal/usage"
	"github.com/phaedrus/curb/internal/watchdog"
)

func TestBuildSessionsExtractsLastTurnTokens(t *testing.T) {
	now := time.Date(2026, 5, 19, 20, 0, 0, 0, time.UTC)
	events := []usage.Event{
		{Provider: "codex", SessionID: "s1", CWD: "/repo", Timestamp: now.Add(-20 * time.Minute), Total: 100},
		{Provider: "codex", SessionID: "s1", CWD: "/repo", Timestamp: now.Add(-2 * time.Minute), Total: 200, Output: 10},
	}
	sessions := BuildSessions(events)
	if len(sessions) != 1 {
		t.Fatalf("sessions = %d", len(sessions))
	}
	if sessions[0].Total != 300 || sessions[0].LastTurnTokens != 200 || sessions[0].Output != 10 {
		t.Fatalf("session totals = %#v", sessions[0])
	}
	if !sessions[0].LastUsage.Equal(now.Add(-2 * time.Minute)) {
		t.Fatalf("last usage = %s", sessions[0].LastUsage)
	}
}

func TestBuildSessionsKeepsLatestTokenTurnWhenSyntheticEventArrivesLater(t *testing.T) {
	now := time.Date(2026, 5, 19, 20, 0, 0, 0, time.UTC)
	events := []usage.Event{
		{Provider: "claude", SessionID: "s1", CWD: "/repo", Timestamp: now.Add(-2 * time.Minute), Total: 1_300_000, Output: 10},
		{Provider: "claude", SessionID: "s1", CWD: "/repo", Timestamp: now.Add(-time.Minute), Total: 0, Model: "<synthetic>"},
	}
	sessions := BuildSessions(events)
	if len(sessions) != 1 {
		t.Fatalf("sessions = %d", len(sessions))
	}
	if sessions[0].LastTurnTokens != 1_300_000 {
		t.Fatalf("last turn tokens = %d", sessions[0].LastTurnTokens)
	}
	if !sessions[0].Last.Equal(now.Add(-time.Minute)) {
		t.Fatalf("last seen = %s", sessions[0].Last)
	}
	if !sessions[0].LastUsage.Equal(now.Add(-2 * time.Minute)) {
		t.Fatalf("last usage = %s", sessions[0].LastUsage)
	}
}

func TestBuildSessionsUsesSourcePathWhenSessionIDIsMissing(t *testing.T) {
	now := time.Date(2026, 5, 19, 20, 0, 0, 0, time.UTC)
	events := []usage.Event{
		{Provider: "codex", SourcePath: "/logs/one.jsonl", CWD: "/repo/one", Timestamp: now, Total: 100},
		{Provider: "codex", SourcePath: "/logs/two.jsonl", CWD: "/repo/two", Timestamp: now, Total: 200},
	}

	sessions := BuildSessions(events)

	if len(sessions) != 2 {
		t.Fatalf("sessions = %#v", sessions)
	}
	if sessions[0].Key == sessions[1].Key {
		t.Fatalf("session keys collapsed: %#v", sessions)
	}
}

func TestEvaluateSessionPolicyUsesExactThresholdAndWindowBoundaries(t *testing.T) {
	now := time.Date(2026, 5, 19, 20, 0, 0, 0, time.UTC)
	cfg := usageConfig(t.TempDir(), config.ModeAlert)
	cfg.Usage.WarnTurnTokens = 1_000
	cfg.Usage.KillTurnTokens = 1_500
	cfg.Usage.Window.Duration = 15 * time.Minute

	warn := EvaluateSessionPolicy(Session{
		LastUsage:      now.Add(-15 * time.Minute),
		LastTurnTokens: 1_000,
	}, cfg.Usage, now)
	if warn.State != "warn" || !warn.Active {
		t.Fatalf("warn policy = %#v", warn)
	}

	stop := EvaluateSessionPolicy(Session{
		LastUsage:      now,
		LastTurnTokens: 1_500,
	}, cfg.Usage, now)
	if stop.State != "stop" || !stop.Active {
		t.Fatalf("stop policy = %#v", stop)
	}
}

func TestClassifySessionOwnsProcessUsageAndActionAxes(t *testing.T) {
	ackUntil := time.Date(2026, 5, 19, 20, 30, 0, 0, time.UTC)
	correlated := Correlation{Matched: true}

	tests := []struct {
		name        string
		decision    SessionDecision
		correlation Correlation
		mode        config.Mode
		ackUntil    *time.Time
		want        SessionClassification
	}{
		{
			name:        "active correlated usage",
			decision:    SessionDecision{State: "active", UsageState: "active", Explanation: "latest turn within limits"},
			correlation: correlated,
			mode:        config.ModeAlert,
			want:        SessionClassification{State: "active", AgentState: "spending", ProcessState: "running", UsageState: "spending", ActionState: "none", RiskRank: 1, Explanation: "latest turn within limits"},
		},
		{
			name:        "idle correlated session",
			decision:    SessionDecision{State: "idle", Explanation: "no recent usage"},
			correlation: correlated,
			mode:        config.ModeAlert,
			want:        SessionClassification{State: "idle", AgentState: "idle", ProcessState: "running", UsageState: "quiet", ActionState: "none", RiskRank: 4, Explanation: "no recent usage"},
		},
		{
			name:        "idle high correlated session",
			decision:    SessionDecision{State: "idle-high", UsageState: "idle-high", Explanation: "historical usage is high, but not active"},
			correlation: correlated,
			mode:        config.ModeAlert,
			want:        SessionClassification{State: "idle-high", AgentState: "idle-high", ProcessState: "running", UsageState: "quiet-high", ActionState: "none", RiskRank: 3, Explanation: "historical usage is high, but not active"},
		},
		{
			name:        "warn needs acknowledgement",
			decision:    SessionDecision{State: "warn", UsageState: "warn", Explanation: "latest turn over warning threshold"},
			correlation: correlated,
			mode:        config.ModeAlert,
			want:        SessionClassification{State: "warn", AgentState: "warn", ProcessState: "running", UsageState: "warn", ActionState: "acknowledge", CanAcknowledge: true, RiskRank: 1, Explanation: "latest turn over warning threshold"},
		},
		{
			name:        "alert mode would stop correlated stop",
			decision:    SessionDecision{State: "stop", UsageState: "stop", Explanation: "latest turn over stop threshold"},
			correlation: correlated,
			mode:        config.ModeAlert,
			want:        SessionClassification{State: "stop", AgentState: "stop", ProcessState: "running", UsageState: "stop", ActionState: "would-stop", CanAcknowledge: true, RiskRank: 1, Explanation: "latest turn over stop threshold"},
		},
		{
			name:        "enforcement stop is actionable",
			decision:    SessionDecision{State: "stop", UsageState: "stop", Actionable: true, Explanation: "latest turn over stop threshold"},
			correlation: correlated,
			mode:        config.ModeEnforcement,
			want:        SessionClassification{State: "stop", AgentState: "stop", ProcessState: "running", UsageState: "stop", ActionState: "stop-pending", Actionable: true, CanAcknowledge: true, RiskRank: 0, Explanation: "latest turn over stop threshold"},
		},
		{
			name:     "uncorrelated usage is not live",
			decision: SessionDecision{State: "uncorrelated", UsageState: "stop", Explanation: "usage crossed threshold, but no live process matched; Curb will not stop anything"},
			mode:     config.ModeEnforcement,
			want:     SessionClassification{State: "uncorrelated", AgentState: "uncorrelated", ProcessState: "unknown", UsageState: "stop", ActionState: "blocked", CanAcknowledge: true, RiskRank: 1, Explanation: "usage crossed threshold, but no live process matched; Curb will not stop anything"},
		},
		{
			name:        "watch only match blocks stop",
			decision:    SessionDecision{State: "watch-only", UsageState: "stop", Explanation: "usage crossed threshold, but matched agent is watch-only; Curb will not stop desktop apps"},
			correlation: Correlation{Matched: true},
			mode:        config.ModeEnforcement,
			want:        SessionClassification{State: "watch-only", AgentState: "watch-only", ProcessState: "watch-only", UsageState: "stop", ActionState: "blocked", CanAcknowledge: true, RiskRank: 1, Explanation: "usage crossed threshold, but matched agent is watch-only; Curb will not stop desktop apps"},
		},
		{
			name:        "acknowledged stop suppresses action",
			decision:    SessionDecision{State: "stop", UsageState: "stop", Explanation: "latest turn over stop threshold"},
			correlation: correlated,
			mode:        config.ModeAlert,
			ackUntil:    &ackUntil,
			want:        SessionClassification{State: "acknowledged", AgentState: "acknowledged", ProcessState: "running", UsageState: "stop", ActionState: "acknowledged", RiskRank: 2, Explanation: "usage crossed threshold, but this session is acknowledged until 2026-05-19T20:30:00Z"},
		},
	}

	for _, tt := range tests {
		t.Run(tt.name, func(t *testing.T) {
			got := ClassifySession(tt.decision, tt.correlation, tt.mode, tt.ackUntil, time.Minute)
			if got != tt.want {
				t.Fatalf("classification = %#v, want %#v", got, tt.want)
			}
		})
	}
}

func TestCorrelateRequiresProviderAndCWD(t *testing.T) {
	session := Session{Provider: "codex", SessionID: "s1", CWD: "/repo"}
	matches := matchFixtures{
		{family: "claude", cwd: "/repo", pid: 1},
		{family: "codex", cwd: "/other", pid: 2},
		{family: "codex", cwd: "/repo", pid: 3},
	}.matches()
	correlation := Correlate(session, matches)
	if !correlation.Matched || correlation.Process.PID != 3 || correlation.Reason != "provider+cwd" {
		t.Fatalf("correlation = %#v", correlation)
	}
}

func TestCorrelateRejectsProviderOnlyMatches(t *testing.T) {
	matches := matchFixtures{
		{family: "codex", cwd: "/repo", pid: 1},
	}.matches()

	if correlation := Correlate(Session{Provider: "codex", SessionID: "s1"}, matches); correlation.Matched {
		t.Fatalf("matched missing session cwd: %#v", correlation)
	}
	if correlation := Correlate(Session{Provider: "codex", SessionID: "s1", CWD: "/repo"}, matchFixtures{{family: "codex", pid: 2}}.matches()); correlation.Matched {
		t.Fatalf("matched missing process cwd: %#v", correlation)
	}
}

func TestCorrelateRequiresPathSegmentBoundaryForPrefixes(t *testing.T) {
	session := Session{Provider: "codex", SessionID: "s1", CWD: "/repo"}
	matches := matchFixtures{
		{family: "codex", cwd: "/repo2", pid: 1},
	}.matches()

	if correlation := Correlate(session, matches); correlation.Matched {
		t.Fatalf("matched sibling path: %#v", correlation)
	}

	matches = matchFixtures{
		{family: "codex", cwd: "/repo/worktree", pid: 2},
	}.matches()
	correlation := Correlate(session, matches)
	if !correlation.Matched || correlation.Reason != "provider+cwd-prefix" {
		t.Fatalf("did not match child path: %#v", correlation)
	}
}

func TestBestSessionForMatchSelectsHighestScoringCorrelation(t *testing.T) {
	match := matchFixtures{{family: "codex", cwd: "/repo/worktree", pid: 9}}.matches()[0]
	sessions := []Session{
		{Provider: "codex", SessionID: "parent", CWD: "/repo"},
		{Provider: "codex", SessionID: "exact", CWD: "/repo/worktree"},
		{Provider: "claude", SessionID: "wrong-provider", CWD: "/repo/worktree"},
	}

	got, ok := BestSessionForMatch(match, sessions)
	if !ok || got.SessionID != "exact" {
		t.Fatalf("best session = %#v ok=%v", got, ok)
	}
}

func TestAlertModeWarnsAndWouldTerminateWithoutKilling(t *testing.T) {
	dir := t.TempDir()
	cfg := usageConfig(dir, config.ModeAlert)
	l, err := ledger.Open(cfg.Ledger.Path)
	if err != nil {
		t.Fatal(err)
	}
	now := time.Date(2026, 5, 19, 20, 0, 0, 0, time.UTC)
	svc := New(cfg, l)
	svc.now = func() time.Time { return now }
	svc.reader = func(time.Time) ([]usage.Event, []usage.SourceReport, error) {
		return []usage.Event{{Provider: "codex", SessionID: "s1", CWD: "/repo", Timestamp: now, Total: 2_000}}, nil, nil
	}
	svc.capture = func(context.Context) (*platform.Snapshot, error) { return snapshot(now, "/repo"), nil }
	killed := false
	svc.terminate = func(context.Context, platform.TerminationTarget, time.Duration) platform.TerminationResult {
		killed = true
		return platform.TerminationResult{}
	}
	if err := svc.Scan(context.Background()); err != nil {
		t.Fatal(err)
	}
	if killed {
		t.Fatal("alert mode killed a process")
	}
	events, err := ledger.Read(cfg.Ledger.Path)
	if err != nil {
		t.Fatal(err)
	}
	requireLedgerEvent(t, events, "usage_warning")
	requireLedgerEvent(t, events, "usage_would_terminate")
}

func TestStaleHighUsageDoesNotWarnWithoutWindowActivity(t *testing.T) {
	dir := t.TempDir()
	cfg := usageConfig(dir, config.ModeAlert)
	l, err := ledger.Open(cfg.Ledger.Path)
	if err != nil {
		t.Fatal(err)
	}
	now := time.Date(2026, 5, 19, 20, 0, 0, 0, time.UTC)
	svc := New(cfg, l)
	svc.now = func() time.Time { return now }
	svc.reader = func(time.Time) ([]usage.Event, []usage.SourceReport, error) {
		return []usage.Event{{Provider: "codex", SessionID: "s1", CWD: "/repo", Timestamp: now.Add(-time.Hour), Total: 2_000}}, nil, nil
	}
	svc.capture = func(context.Context) (*platform.Snapshot, error) { return snapshot(now, "/repo"), nil }
	if err := svc.Scan(context.Background()); err != nil {
		t.Fatal(err)
	}
	events, err := ledger.Read(cfg.Ledger.Path)
	if err != nil {
		t.Fatal(err)
	}
	if hasLedgerEvent(events, "usage_warning") {
		t.Fatalf("stale usage produced warning: %#v", events)
	}
}

func TestSyntheticEventDoesNotMakeStaleUsageRecent(t *testing.T) {
	dir := t.TempDir()
	cfg := usageConfig(dir, config.ModeAlert)
	l, err := ledger.Open(cfg.Ledger.Path)
	if err != nil {
		t.Fatal(err)
	}
	now := time.Date(2026, 5, 19, 20, 0, 0, 0, time.UTC)
	svc := New(cfg, l)
	svc.now = func() time.Time { return now }
	svc.reader = func(time.Time) ([]usage.Event, []usage.SourceReport, error) {
		return []usage.Event{
			{Provider: "codex", SessionID: "s1", CWD: "/repo", Timestamp: now.Add(-time.Hour), Total: 2_000},
			{Provider: "codex", SessionID: "s1", CWD: "/repo", Timestamp: now, Total: 0},
		}, nil, nil
	}
	svc.capture = func(context.Context) (*platform.Snapshot, error) { return snapshot(now, "/repo"), nil }
	if err := svc.Scan(context.Background()); err != nil {
		t.Fatal(err)
	}
	events, err := ledger.Read(cfg.Ledger.Path)
	if err != nil {
		t.Fatal(err)
	}
	if hasLedgerEvent(events, "usage_warning") {
		t.Fatalf("synthetic event made stale usage active: %#v", events)
	}
}

func TestEnforcementTerminatesAfterUsageGrace(t *testing.T) {
	dir := t.TempDir()
	cfg := usageConfig(dir, config.ModeEnforcement)
	cfg.Usage.GracePeriod.Duration = time.Nanosecond
	l, err := ledger.Open(cfg.Ledger.Path)
	if err != nil {
		t.Fatal(err)
	}
	now := time.Date(2026, 5, 19, 20, 0, 0, 0, time.UTC)
	processStart := now.Add(-time.Minute)
	svc := New(cfg, l)
	svc.now = func() time.Time { return now }
	svc.reader = func(time.Time) ([]usage.Event, []usage.SourceReport, error) {
		return []usage.Event{{Provider: "codex", SessionID: "s1", CWD: "/repo", Timestamp: now, Total: 2_000}}, nil, nil
	}
	svc.capture = func(context.Context) (*platform.Snapshot, error) {
		return snapshot(processStart, "/repo"), nil
	}
	killedPID := int32(0)
	svc.terminate = func(_ context.Context, target platform.TerminationTarget, _ time.Duration) platform.TerminationResult {
		killedPID = target.Root().PID
		return platform.TerminationResult{SoftSignaled: []int32{target.Root().PID}}
	}
	if err := svc.Scan(context.Background()); err != nil {
		t.Fatal(err)
	}
	now = now.Add(time.Second)
	if err := svc.Scan(context.Background()); err != nil {
		t.Fatal(err)
	}
	if killedPID != 4242 {
		t.Fatalf("killed pid = %d", killedPID)
	}
	events, err := ledger.Read(cfg.Ledger.Path)
	if err != nil {
		t.Fatal(err)
	}
	requireLedgerEvent(t, events, "usage_grace_started")
	requireLedgerEvent(t, events, "usage_termination_completed")
}

func TestReconfigurePreservesGraceAndTargets(t *testing.T) {
	dir := t.TempDir()
	cfg := usageConfig(dir, config.ModeEnforcement)
	cfg.Usage.GracePeriod.Duration = time.Second
	l, err := ledger.Open(cfg.Ledger.Path)
	if err != nil {
		t.Fatal(err)
	}
	now := time.Date(2026, 5, 19, 20, 0, 0, 0, time.UTC)
	originalStart := now.Add(-time.Minute)
	svc := New(cfg, l)
	svc.now = func() time.Time { return now }
	svc.reader = func(time.Time) ([]usage.Event, []usage.SourceReport, error) {
		return []usage.Event{{Provider: "codex", SessionID: "s1", CWD: "/repo", Timestamp: now, Total: 2_000}}, nil, nil
	}
	svc.capture = func(context.Context) (*platform.Snapshot, error) {
		return snapshot(originalStart, "/repo"), nil
	}
	killedPID := int32(0)
	svc.terminate = func(_ context.Context, target platform.TerminationTarget, _ time.Duration) platform.TerminationResult {
		killedPID = target.Root().PID
		return platform.TerminationResult{SoftSignaled: []int32{target.Root().PID}}
	}
	if err := svc.Scan(context.Background()); err != nil {
		t.Fatal(err)
	}
	next := *cfg
	next.Usage.WarnTurnTokens = 500
	log, err := ledger.Open(cfg.Ledger.Path)
	if err != nil {
		t.Fatal(err)
	}
	svc.Reconfigure(&next, log)
	now = now.Add(2 * time.Second)
	if err := svc.Scan(context.Background()); err != nil {
		t.Fatal(err)
	}
	if killedPID != 4242 {
		t.Fatalf("killed pid = %d", killedPID)
	}
}

func TestSessionAckSuppressesWarningAndTerminationUntilExpiry(t *testing.T) {
	dir := t.TempDir()
	cfg := usageConfig(dir, config.ModeEnforcement)
	cfg.Usage.GracePeriod.Duration = time.Nanosecond
	l, err := ledger.Open(cfg.Ledger.Path)
	if err != nil {
		t.Fatal(err)
	}
	now := time.Date(2026, 5, 19, 20, 0, 0, 0, time.UTC)
	processStart := now.Add(-time.Minute)
	if _, err := WriteSessionAck(cfg.Service.StateDir, "codex:s1", 5*time.Minute, "still supervising", now); err != nil {
		t.Fatal(err)
	}
	svc := New(cfg, l)
	svc.now = func() time.Time { return now }
	svc.reader = func(time.Time) ([]usage.Event, []usage.SourceReport, error) {
		return []usage.Event{{Provider: "codex", SessionID: "s1", CWD: "/repo", Timestamp: now, Total: 2_000}}, nil, nil
	}
	svc.capture = func(context.Context) (*platform.Snapshot, error) {
		return snapshot(processStart, "/repo"), nil
	}
	killed := false
	svc.terminate = func(context.Context, platform.TerminationTarget, time.Duration) platform.TerminationResult {
		killed = true
		return platform.TerminationResult{}
	}

	if err := svc.Scan(context.Background()); err != nil {
		t.Fatal(err)
	}
	if killed {
		t.Fatal("acknowledged session was terminated")
	}
	events, err := ledger.Read(cfg.Ledger.Path)
	if err != nil {
		t.Fatal(err)
	}
	if hasLedgerEvent(events, "usage_warning") || hasLedgerEvent(events, "usage_grace_started") || hasLedgerEvent(events, "usage_termination_completed") {
		t.Fatalf("acknowledged session produced policy events: %#v", events)
	}

	now = now.Add(6 * time.Minute)
	if err := svc.Scan(context.Background()); err != nil {
		t.Fatal(err)
	}
	events, err = ledger.Read(cfg.Ledger.Path)
	if err != nil {
		t.Fatal(err)
	}
	requireLedgerEvent(t, events, "usage_warning")
	requireLedgerEvent(t, events, "usage_grace_started")
}

func TestEnforcementRejectsPIDReuseBeforeTermination(t *testing.T) {
	dir := t.TempDir()
	cfg := usageConfig(dir, config.ModeEnforcement)
	cfg.Usage.GracePeriod.Duration = time.Nanosecond
	l, err := ledger.Open(cfg.Ledger.Path)
	if err != nil {
		t.Fatal(err)
	}
	now := time.Date(2026, 5, 19, 20, 0, 0, 0, time.UTC)
	originalStart := now.Add(-time.Minute)
	reusedStart := now.Add(time.Minute)
	svc := New(cfg, l)
	svc.now = func() time.Time { return now }
	svc.reader = func(time.Time) ([]usage.Event, []usage.SourceReport, error) {
		return []usage.Event{{Provider: "codex", SessionID: "s1", CWD: "/repo", Timestamp: now, Total: 2_000}}, nil, nil
	}
	svc.capture = func(context.Context) (*platform.Snapshot, error) {
		if now.Before(time.Date(2026, 5, 19, 20, 0, 1, 0, time.UTC)) {
			return snapshot(originalStart, "/repo"), nil
		}
		return snapshot(reusedStart, "/repo"), nil
	}
	killed := false
	svc.terminate = func(context.Context, platform.TerminationTarget, time.Duration) platform.TerminationResult {
		killed = true
		return platform.TerminationResult{}
	}
	if err := svc.Scan(context.Background()); err != nil {
		t.Fatal(err)
	}
	now = now.Add(time.Second)
	if err := svc.Scan(context.Background()); err != nil {
		t.Fatal(err)
	}
	if killed {
		t.Fatal("terminated reused pid")
	}
	events, err := ledger.Read(cfg.Ledger.Path)
	if err != nil {
		t.Fatal(err)
	}
	requireLedgerEvent(t, events, "usage_termination_failed")
}

type matchFixture struct {
	family string
	cwd    string
	pid    int32
}

type matchFixtures []matchFixture

func (fixtures matchFixtures) matches() []watchdog.Match {
	matches := make([]watchdog.Match, 0, len(fixtures))
	for _, fixture := range fixtures {
		matches = append(matches, watchdog.Match{
			Agent:   config.Agent{ID: fixture.family + "-test", Family: fixture.family, Kind: config.AgentKindProcess},
			Process: platform.Process{PID: fixture.pid, CWD: fixture.cwd},
		})
	}
	return matches
}

func usageConfig(dir string, mode config.Mode) *config.Config {
	enabled := true
	return &config.Config{
		Version: 1,
		Mode:    mode,
		Service: config.ServiceConfig{MinConfidence: 50, StateDir: dir},
		Usage: config.UsageConfig{
			Enabled:        &enabled,
			ScanInterval:   config.Duration{Duration: time.Second},
			Lookback:       config.Duration{Duration: time.Hour},
			Window:         config.Duration{Duration: 15 * time.Minute},
			WarnTurnTokens: 1_000,
			KillTurnTokens: 1_500,
			GracePeriod:    config.Duration{Duration: time.Second},
		},
		Defaults: config.Policy{
			WarnAfter:       config.Duration{Duration: time.Hour},
			KillAfter:       config.Duration{Duration: 2 * time.Hour},
			KillGracePeriod: config.Duration{Duration: time.Second},
			MinLifetime:     config.Duration{Duration: time.Nanosecond},
		},
		Agents: []config.Agent{{
			ID:     "codex-test",
			Label:  "Codex Test",
			Family: "codex",
			Kind:   config.AgentKindProcess,
			Match:  config.Match{ProcessNames: []string{"codex"}},
		}},
		Ledger: config.LedgerConfig{Path: filepath.Join(dir, "runs.ndjson")},
	}
}

func snapshot(start time.Time, cwd string) *platform.Snapshot {
	return &platform.Snapshot{
		At:       time.Now(),
		Platform: "test",
		Processes: map[int32]platform.Process{
			4242: {PID: 4242, Name: "codex", Exe: "/usr/local/bin/codex", CWD: cwd, Create: start, StartedOK: true, Username: "tester"},
		},
		Children: map[int32][]int32{},
	}
}

func requireLedgerEvent(t *testing.T, events []ledger.Event, eventType string) {
	t.Helper()
	if hasLedgerEvent(events, eventType) {
		return
	}
	t.Fatalf("missing %s in %#v", eventType, events)
}

func hasLedgerEvent(events []ledger.Event, eventType string) bool {
	for _, event := range events {
		if event.Type == eventType {
			return true
		}
	}
	return false
}
