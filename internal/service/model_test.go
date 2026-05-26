package service

import (
	"encoding/json"
	"strings"
	"testing"
	"time"

	"github.com/phaedrus/curb/internal/config"
	"github.com/phaedrus/curb/internal/platform"
	"github.com/phaedrus/curb/internal/usage"
	"github.com/phaedrus/curb/internal/usagewatch"
)

func TestBuildSnapshotSeparatesRealIdleFromWarningUsage(t *testing.T) {
	now := time.Date(2026, 5, 21, 18, 0, 0, 0, time.UTC)
	cfg := testConfig()
	events := []usage.Event{
		{
			Provider:      "codex",
			SessionID:     "codex-hot",
			CWD:           "/work/hot",
			Timestamp:     now.Add(-time.Minute),
			Input:         200_000,
			CachedInput:   350_000,
			CacheCreation: 10_000,
			Output:        70_000,
			Total:         620_000,
		},
		{
			Provider:  "codex",
			SessionID: "codex-hot",
			CWD:       "/work/hot",
			Timestamp: now,
			Total:     0,
		},
		{
			Provider:  "claude",
			SessionID: "claude-normal",
			CWD:       "/work/normal",
			Timestamp: now.Add(-2 * time.Minute),
			Model:     "claude-sonnet-4-5",
			Input:     10_000,
			Output:    2_000,
			Total:     12_000,
		},
		{
			Provider:  "codex",
			SessionID: "codex-old",
			CWD:       "/work/old",
			Timestamp: now.Add(-time.Hour),
			Total:     900_000,
		},
	}
	snap := &platform.Snapshot{
		At:       now,
		Platform: "test",
		Processes: map[int32]platform.Process{
			100: {PID: 100, Name: "codex", CWD: "/work/hot", Cmdline: "codex", Create: now.Add(-10 * time.Minute), StartedOK: true},
			200: {PID: 200, Name: "claude", CWD: "/work/normal", Cmdline: "claude", Create: now.Add(-9 * time.Minute), StartedOK: true},
		},
		Children: map[int32][]int32{},
	}

	view := BuildSnapshot(cfg, snap, events, []usage.SourceReport{{Provider: "codex", Events: 3}, {Provider: "claude", Events: 1}}, now)

	if view.Overview.Status != "WATCH" || view.Overview.WarningSessions != 1 || view.Overview.StopSessions != 0 {
		t.Fatalf("overview = %#v", view.Overview)
	}
	if view.Overview.ActiveSessions != 2 {
		t.Fatalf("active sessions = %d", view.Overview.ActiveSessions)
	}
	if view.Overview.WindowTokens != 632_000 {
		t.Fatalf("window tokens = %d", view.Overview.WindowTokens)
	}
	if len(view.Sessions) != 3 {
		t.Fatalf("sessions = %d", len(view.Sessions))
	}
	if view.Sessions[0].ID != "codex-hot" || view.Sessions[0].State != "warn" {
		t.Fatalf("first session = %#v", view.Sessions[0])
	}
	if view.Sessions[0].LatestTurnTokens != 620_000 {
		t.Fatalf("latest turn = %d", view.Sessions[0].LatestTurnTokens)
	}
	if view.Sessions[0].ProcessState != "running" || view.Sessions[0].UsageState != "warn" || view.Sessions[0].ActionState != "acknowledge" || !view.Sessions[0].CanAcknowledge {
		t.Fatalf("session states = process %q usage %q action %q can_ack %v", view.Sessions[0].ProcessState, view.Sessions[0].UsageState, view.Sessions[0].ActionState, view.Sessions[0].CanAcknowledge)
	}
	if len(view.Turns) != 2 {
		t.Fatalf("turns = %#v", view.Turns)
	}
	if view.Turns[0].CacheCreation != 10_000 {
		t.Fatalf("cache creation tokens = %d", view.Turns[0].CacheCreation)
	}
	if view.Agents[0].ProcessState == "" || view.Agents[0].UsageState == "" || view.Agents[0].ActionState == "" {
		t.Fatalf("agent projected states missing: %#v", view.Agents[0])
	}
	requireAgentState(t, view.Agents, "codex-test", "warn")
	requireAgentState(t, view.Agents, "claude-test", "spending")
}

func TestBuildSnapshotSortsSessionsByCurrentSpendAndRisk(t *testing.T) {
	now := time.Date(2026, 5, 21, 18, 0, 0, 0, time.UTC)
	cfg := testConfig()
	cfg.Mode = config.ModeEnforcement
	cfg.Service.StateDir = t.TempDir()
	cfg.Agents = append(cfg.Agents, config.Agent{
		ID:     "claude-desktop",
		Label:  "Claude Desktop",
		Family: "claude",
		Kind:   config.AgentKindApp,
		Match:  config.Match{ProcessNames: []string{"ClaudeDesktop"}},
	})
	if _, err := usagewatch.WriteSessionAck(cfg.Service.StateDir, "codex:acked-stop", 5*time.Minute, "still supervising", now); err != nil {
		t.Fatal(err)
	}
	events := []usage.Event{
		{Provider: "codex", SessionID: "actionable-stop", CWD: "/work/actionable", Timestamp: now, Total: 800_000},
		{Provider: "codex", SessionID: "bigger-stop", CWD: "/work/uncorrelated-big", Timestamp: now, Total: 950_000},
		{Provider: "claude", SessionID: "watch-only-stop", CWD: "/work/watch-only", Timestamp: now, Total: 900_000},
		{Provider: "codex", SessionID: "warn", CWD: "/work/warn", Timestamp: now, Total: 300_000},
		{Provider: "claude", SessionID: "spending", CWD: "/work/spending", Timestamp: now, Total: 20_000},
		{Provider: "codex", SessionID: "acked-stop", CWD: "/work/acked", Timestamp: now, Total: 850_000},
		{Provider: "codex", SessionID: "idle-high", CWD: "/work/idle-high", Timestamp: now.Add(-time.Hour), Total: 900_000},
	}
	snap := &platform.Snapshot{
		At:       now,
		Platform: "test",
		Processes: map[int32]platform.Process{
			100: {PID: 100, Name: "codex", CWD: "/work/actionable", Cmdline: "codex", Create: now.Add(-10 * time.Minute), StartedOK: true},
			200: {PID: 200, Name: "ClaudeDesktop", CWD: "/work/watch-only", Cmdline: "ClaudeDesktop", Create: now.Add(-10 * time.Minute), StartedOK: true},
			300: {PID: 300, Name: "codex", CWD: "/work/warn", Cmdline: "codex", Create: now.Add(-10 * time.Minute), StartedOK: true},
			400: {PID: 400, Name: "claude", CWD: "/work/spending", Cmdline: "claude", Create: now.Add(-10 * time.Minute), StartedOK: true},
			500: {PID: 500, Name: "codex", CWD: "/work/acked", Cmdline: "codex", Create: now.Add(-10 * time.Minute), StartedOK: true},
			600: {PID: 600, Name: "codex", CWD: "/work/idle-high", Cmdline: "codex", Create: now.Add(-10 * time.Minute), StartedOK: true},
		},
		Children: map[int32][]int32{},
	}

	view := BuildSnapshot(cfg, snap, events, nil, now)
	got := sessionIDs(view.Sessions)
	want := []string{"actionable-stop", "bigger-stop", "watch-only-stop", "warn", "spending", "acked-stop", "idle-high"}
	if strings.Join(got, ",") != strings.Join(want, ",") {
		t.Fatalf("session order = %#v, want %#v", got, want)
	}
}

func TestSessionRiskRankCollapsesCurrentSpendStates(t *testing.T) {
	correlated := usagewatch.Correlation{Matched: true}
	stopRank := usagewatch.ClassifySession(usagewatch.SessionDecision{State: "watch-only", UsageState: "stop"}, correlated, config.ModeAlert, nil, time.Minute).RiskRank
	warnRank := usagewatch.ClassifySession(usagewatch.SessionDecision{State: "warn", UsageState: "warn"}, correlated, config.ModeAlert, nil, time.Minute).RiskRank
	spendingRank := usagewatch.ClassifySession(usagewatch.SessionDecision{State: "active", UsageState: "spending"}, correlated, config.ModeAlert, nil, time.Minute).RiskRank
	activeRank := usagewatch.ClassifySession(usagewatch.SessionDecision{State: "active"}, correlated, config.ModeAlert, nil, time.Minute).RiskRank

	if stopRank != warnRank || warnRank != spendingRank || spendingRank != activeRank {
		t.Fatalf("current spend ranks = stop %d warn %d spending %d active %d", stopRank, warnRank, spendingRank, activeRank)
	}
	if actionableRank := usagewatch.ClassifySession(usagewatch.SessionDecision{State: "stop", UsageState: "stop", Actionable: true}, correlated, config.ModeEnforcement, nil, time.Minute).RiskRank; actionableRank >= stopRank {
		t.Fatalf("actionable rank = %d, current spend rank = %d", actionableRank, stopRank)
	}
	ackUntil := time.Date(2026, 5, 21, 18, 30, 0, 0, time.UTC)
	if ackRank := usagewatch.ClassifySession(usagewatch.SessionDecision{State: "stop", UsageState: "stop"}, correlated, config.ModeAlert, &ackUntil, time.Minute).RiskRank; ackRank <= stopRank {
		t.Fatalf("acknowledged rank = %d, current spend rank = %d", ackRank, stopRank)
	}
}

func TestSortSessionViewsOrdersSameBucketByCurrentSpend(t *testing.T) {
	base := time.Date(2026, 5, 21, 18, 0, 0, 0, time.UTC)
	sessions := []SessionView{
		{ID: "older", State: "warn", UsageState: "warn", ProcessState: "running", RiskRank: 1, LatestTurnTokens: 100, WindowTokens: 500, LastSeenAt: base.Add(3 * time.Minute)},
		{ID: "window", State: "active", UsageState: "spending", ProcessState: "running", RiskRank: 1, LatestTurnTokens: 100, WindowTokens: 900, LastSeenAt: base.Add(time.Minute)},
		{ID: "latest", State: "watch-only", UsageState: "stop", ProcessState: "running", RiskRank: 1, LatestTurnTokens: 200, WindowTokens: 100, LastSeenAt: base},
		{ID: "recent", State: "warn", UsageState: "warn", ProcessState: "running", RiskRank: 1, LatestTurnTokens: 100, WindowTokens: 500, LastSeenAt: base.Add(5 * time.Minute)},
	}

	sortSessionViews(sessions)

	got := sessionIDs(sessions)
	want := []string{"latest", "window", "recent", "older"}
	if strings.Join(got, ",") != strings.Join(want, ",") {
		t.Fatalf("session order = %#v, want %#v", got, want)
	}
}

func TestSortSessionViewsOrdersActionableSessionsByCurrentSpend(t *testing.T) {
	base := time.Date(2026, 5, 21, 18, 0, 0, 0, time.UTC)
	sessions := []SessionView{
		{ID: "acknowledged", State: "acknowledged", UsageState: "stop", ProcessState: "running", RiskRank: 2, LatestTurnTokens: 1_000, WindowTokens: 1_000, LastSeenAt: base.Add(3 * time.Minute)},
		{ID: "lower-action", Actionable: true, State: "stop", UsageState: "stop", ProcessState: "running", RiskRank: 0, LatestTurnTokens: 300, WindowTokens: 900, LastSeenAt: base.Add(2 * time.Minute)},
		{ID: "current", State: "warn", UsageState: "warn", ProcessState: "running", RiskRank: 1, LatestTurnTokens: 700, WindowTokens: 700, LastSeenAt: base.Add(4 * time.Minute)},
		{ID: "higher-action", Actionable: true, State: "stop", UsageState: "stop", ProcessState: "running", RiskRank: 0, LatestTurnTokens: 600, WindowTokens: 600, LastSeenAt: base},
	}

	sortSessionViews(sessions)

	got := sessionIDs(sessions)
	want := []string{"higher-action", "lower-action", "current", "acknowledged"}
	if strings.Join(got, ",") != strings.Join(want, ",") {
		t.Fatalf("session order = %#v, want %#v", got, want)
	}
}

func TestSortSessionViewsOrdersAcknowledgedAndIdleHighBucketsByCurrentSpend(t *testing.T) {
	base := time.Date(2026, 5, 21, 18, 0, 0, 0, time.UTC)
	sessions := []SessionView{
		{ID: "idle-older", State: "idle-high", UsageState: "stop", ProcessState: "idle", RiskRank: 3, LatestTurnTokens: 100, WindowTokens: 500, LastSeenAt: base.Add(3 * time.Minute)},
		{ID: "ack-window", State: "acknowledged", UsageState: "stop", ProcessState: "running", RiskRank: 2, LatestTurnTokens: 100, WindowTokens: 900, LastSeenAt: base.Add(time.Minute)},
		{ID: "idle-latest", State: "idle-high", UsageState: "warn", ProcessState: "idle", RiskRank: 3, LatestTurnTokens: 200, WindowTokens: 100, LastSeenAt: base},
		{ID: "ack-latest", State: "acknowledged", UsageState: "stop", ProcessState: "running", RiskRank: 2, LatestTurnTokens: 200, WindowTokens: 100, LastSeenAt: base},
		{ID: "idle-recent", State: "idle-high", UsageState: "warn", ProcessState: "idle", RiskRank: 3, LatestTurnTokens: 100, WindowTokens: 500, LastSeenAt: base.Add(5 * time.Minute)},
		{ID: "ack-recent", State: "acknowledged", UsageState: "stop", ProcessState: "running", RiskRank: 2, LatestTurnTokens: 100, WindowTokens: 900, LastSeenAt: base.Add(5 * time.Minute)},
	}

	sortSessionViews(sessions)

	got := sessionIDs(sessions)
	want := []string{"ack-latest", "ack-recent", "ack-window", "idle-latest", "idle-recent", "idle-older"}
	if strings.Join(got, ",") != strings.Join(want, ",") {
		t.Fatalf("session order = %#v, want %#v", got, want)
	}
}

func sessionIDs(sessions []SessionView) []string {
	out := make([]string, 0, len(sessions))
	for _, session := range sessions {
		out = append(out, session.ID)
	}
	return out
}

func TestBuildSnapshotExplainsRunningAgentWithoutUsage(t *testing.T) {
	now := time.Date(2026, 5, 21, 18, 0, 0, 0, time.UTC)
	cfg := testConfig()
	snap := &platform.Snapshot{
		At:       now,
		Platform: "test",
		Processes: map[int32]platform.Process{
			100: {PID: 100, Name: "codex", CWD: "/work/no-usage", Cmdline: "codex", Create: now.Add(-10 * time.Minute), StartedOK: true},
		},
		Children: map[int32][]int32{},
	}

	view := BuildSnapshot(cfg, snap, nil, nil, now)

	if view.Overview.Status != "OK" || view.Overview.ActiveAgents != 1 || view.Overview.ActiveSessions != 0 {
		t.Fatalf("overview = %#v", view.Overview)
	}
	requireAgentState(t, view.Agents, "codex-test", "running")
	if view.Agents[0].Explanation != "process is running with no correlated usage" {
		t.Fatalf("agent explanation = %q", view.Agents[0].Explanation)
	}
}

func TestBuildSnapshotMarksUncorrelatedOverBudgetUsageAsNotActionable(t *testing.T) {
	now := time.Date(2026, 5, 21, 18, 0, 0, 0, time.UTC)
	cfg := testConfig()
	events := []usage.Event{{
		Provider:  "codex",
		SessionID: "codex-hot",
		CWD:       "/work/no-process",
		Timestamp: now,
		Total:     cfg.Usage.KillTurnTokens,
	}}

	view := BuildSnapshot(cfg, emptySnapshot(now), events, nil, now)

	if view.Overview.Status != "WATCH" || view.Overview.WarningSessions != 1 || view.Overview.StopSessions != 0 {
		t.Fatalf("overview = %#v", view.Overview)
	}
	session := requireSession(t, view.Sessions, "codex-hot")
	if session.State != "uncorrelated" || session.UsageState != "stop" || session.ActionState != "blocked" || session.Actionable || !session.CanAcknowledge {
		t.Fatalf("session = %#v", session)
	}
}

func TestBuildSnapshotActionableRequiresEnforcementMode(t *testing.T) {
	now := time.Date(2026, 5, 21, 18, 0, 0, 0, time.UTC)
	cfg := testConfig()
	events := []usage.Event{{
		Provider:  "codex",
		SessionID: "codex-hot",
		CWD:       "/work/hot",
		Timestamp: now,
		Total:     cfg.Usage.KillTurnTokens,
	}}
	snap := &platform.Snapshot{
		At:       now,
		Platform: "test",
		Processes: map[int32]platform.Process{
			100: {PID: 100, Name: "codex", CWD: "/work/hot", Cmdline: "codex", Create: now.Add(-10 * time.Minute), StartedOK: true},
		},
		Children: map[int32][]int32{},
	}

	alertView := BuildSnapshot(cfg, snap, events, nil, now)
	alertSession := requireSession(t, alertView.Sessions, "codex-hot")
	if alertSession.State != "stop" || alertSession.ActionState != "would-stop" || alertSession.Actionable || !alertSession.CanAcknowledge || alertView.Overview.Status != "WATCH" {
		t.Fatalf("alert session = %#v overview=%#v", alertSession, alertView.Overview)
	}

	cfg.Mode = config.ModeEnforcement
	enforcementView := BuildSnapshot(cfg, snap, events, nil, now)
	enforcementSession := requireSession(t, enforcementView.Sessions, "codex-hot")
	if enforcementSession.State != "stop" || enforcementSession.ActionState != "stop-pending" || !enforcementSession.Actionable || !enforcementSession.CanAcknowledge || enforcementView.Overview.Status != "ACTION" {
		t.Fatalf("enforcement session = %#v overview=%#v", enforcementSession, enforcementView.Overview)
	}
	if enforcementSession.CorrelatedPID != 100 || enforcementSession.CorrelatedStarted == nil || enforcementSession.CorrelationScore == 0 {
		t.Fatalf("missing process identity: %#v", enforcementSession)
	}
}

func TestBuildSnapshotMarksUncorrelatedWarningUsageAsNotActionable(t *testing.T) {
	now := time.Date(2026, 5, 21, 18, 0, 0, 0, time.UTC)
	cfg := testConfig()
	events := []usage.Event{{
		Provider:  "claude",
		SessionID: "claude-warn",
		CWD:       "/work/no-process",
		Timestamp: now,
		Total:     cfg.Usage.WarnTurnTokens,
	}}

	view := BuildSnapshot(cfg, emptySnapshot(now), events, nil, now)

	session := requireSession(t, view.Sessions, "claude-warn")
	if session.State != "uncorrelated" || session.UsageState != "warn" || session.Actionable || !session.CanAcknowledge {
		t.Fatalf("session = %#v", session)
	}
	if view.Overview.Status != "WATCH" || view.Overview.WarningSessions != 1 || view.Overview.StopSessions != 0 {
		t.Fatalf("overview = %#v", view.Overview)
	}
}

func TestBuildSnapshotPreservesWatchOnlyForDesktopMatches(t *testing.T) {
	now := time.Date(2026, 5, 21, 18, 0, 0, 0, time.UTC)
	cfg := testConfig()
	cfg.Agents = []config.Agent{{
		ID:     "codex-desktop",
		Label:  "Codex Desktop",
		Family: "codex",
		Kind:   config.AgentKindApp,
		Match:  config.Match{ProcessNames: []string{"Codex"}},
	}}
	events := []usage.Event{{
		Provider:  "codex",
		SessionID: "desktop-session",
		CWD:       "/work/desktop",
		Timestamp: now,
		Total:     cfg.Usage.KillTurnTokens,
	}}
	snap := &platform.Snapshot{
		At:       now,
		Platform: "test",
		Processes: map[int32]platform.Process{
			100: {PID: 100, Name: "Codex", CWD: "/work/desktop", Cmdline: "Codex", Create: now.Add(-10 * time.Minute), StartedOK: true},
		},
		Children: map[int32][]int32{},
	}

	view := BuildSnapshot(cfg, snap, events, nil, now)

	if view.Overview.Status != "WATCH" || view.Overview.StopSessions != 0 {
		t.Fatalf("overview = %#v", view.Overview)
	}
	requireAgentState(t, view.Agents, "codex-desktop", "watch-only")
	agent := requireAgent(t, view.Agents, "codex-desktop")
	if agent.UsageState != "stop" || agent.Actionable {
		t.Fatalf("agent = %#v", agent)
	}
	session := requireSession(t, view.Sessions, "desktop-session")
	if session.State != "watch-only" || session.UsageState != "stop" || session.ActionState != "blocked" || session.Actionable || !session.CanAcknowledge {
		t.Fatalf("session = %#v", session)
	}
}

func TestBuildSnapshotMarksCorrelatedStaleSessionIdle(t *testing.T) {
	now := time.Date(2026, 5, 21, 18, 0, 0, 0, time.UTC)
	cfg := testConfig()
	events := []usage.Event{{
		Provider:  "codex",
		SessionID: "codex-old",
		CWD:       "/work/old",
		Timestamp: now.Add(-time.Hour),
		Total:     cfg.Usage.WarnTurnTokens,
	}}
	snap := &platform.Snapshot{
		At:       now,
		Platform: "test",
		Processes: map[int32]platform.Process{
			100: {PID: 100, Name: "codex", CWD: "/work/old", Cmdline: "codex", Create: now.Add(-10 * time.Minute), StartedOK: true},
		},
		Children: map[int32][]int32{},
	}

	view := BuildSnapshot(cfg, snap, events, nil, now)

	requireAgentState(t, view.Agents, "codex-test", "idle")
}

func TestBuildSnapshotProjectsAcknowledgedOverBudgetSession(t *testing.T) {
	now := time.Date(2026, 5, 21, 18, 0, 0, 0, time.UTC)
	cfg := testConfig()
	cfg.Mode = config.ModeEnforcement
	cfg.Service.StateDir = t.TempDir()
	if _, err := usagewatch.WriteSessionAck(cfg.Service.StateDir, "codex:codex-hot", 5*time.Minute, "still supervising", now); err != nil {
		t.Fatal(err)
	}
	events := []usage.Event{{
		Provider:  "codex",
		SessionID: "codex-hot",
		CWD:       "/work/hot",
		Timestamp: now,
		Total:     cfg.Usage.KillTurnTokens,
	}}
	snap := &platform.Snapshot{
		At:       now,
		Platform: "test",
		Processes: map[int32]platform.Process{
			100: {PID: 100, Name: "codex", CWD: "/work/hot", Cmdline: "codex", Create: now.Add(-10 * time.Minute), StartedOK: true},
		},
		Children: map[int32][]int32{},
	}

	view := BuildSnapshot(cfg, snap, events, nil, now)

	session := requireSession(t, view.Sessions, "codex-hot")
	if session.State != "acknowledged" || session.UsageState != "stop" || session.Actionable || session.CanAcknowledge || !session.Acknowledged || session.AcknowledgedUntil == nil {
		t.Fatalf("session = %#v", session)
	}
	if view.Overview.Status != "ACTIVE" || view.Overview.StopSessions != 0 || view.Overview.WarningSessions != 0 {
		t.Fatalf("overview = %#v", view.Overview)
	}
}

func TestBuildSnapshotKeepsEmptySessionIDSourcePathsSeparate(t *testing.T) {
	now := time.Date(2026, 5, 21, 18, 0, 0, 0, time.UTC)
	cfg := testConfig()
	events := []usage.Event{
		{Provider: "codex", SourcePath: "/logs/one.jsonl", CWD: "/work/one", Timestamp: now, Total: 10_000},
		{Provider: "codex", SourcePath: "/logs/two.jsonl", CWD: "/work/two", Timestamp: now, Total: 20_000},
	}

	view := BuildSnapshot(cfg, emptySnapshot(now), events, nil, now)

	if len(view.Sessions) != 2 {
		t.Fatalf("sessions = %#v", view.Sessions)
	}
	if len(view.Turns) != 2 {
		t.Fatalf("turns = %#v", view.Turns)
	}
	if view.Sessions[0].Key == view.Sessions[1].Key {
		t.Fatalf("session keys collapsed: %#v", view.Sessions)
	}
}

func TestBuildSnapshotIncludesTurnsAtWindowStart(t *testing.T) {
	now := time.Date(2026, 5, 21, 18, 0, 0, 0, time.UTC)
	cfg := testConfig()
	windowStart := now.Add(-cfg.Usage.Window.Duration)
	view := BuildSnapshot(cfg, emptySnapshot(now), []usage.Event{{
		Provider:  "codex",
		SessionID: "edge",
		CWD:       "/work/edge",
		Timestamp: windowStart,
		Total:     10_000,
	}}, nil, now)

	if len(view.Turns) != 1 || view.Sessions[0].WindowTokens != 10_000 {
		t.Fatalf("snapshot = %#v", view)
	}
}

func TestBuildSnapshotDoesNotCountUncorrelatedUsageAsActiveLiveSession(t *testing.T) {
	now := time.Date(2026, 5, 21, 18, 0, 0, 0, time.UTC)
	view := BuildSnapshot(testConfig(), emptySnapshot(now), []usage.Event{{
		Provider:  "claude",
		SessionID: "recent-no-process",
		CWD:       "/work/no-process",
		Timestamp: now,
		Total:     10_000,
	}}, nil, now)

	if view.Overview.Status != "OK" || view.Overview.ActiveSessions != 0 || view.Overview.WindowTokens != 0 {
		t.Fatalf("overview = %#v", view.Overview)
	}
	session := requireSession(t, view.Sessions, "recent-no-process")
	if session.ProcessState != "no-process" || session.WindowTokens != 10_000 {
		t.Fatalf("session = %#v", session)
	}
}

func TestBuildSnapshotJSONOmitsZeroLastUsageAt(t *testing.T) {
	now := time.Date(2026, 5, 21, 18, 0, 0, 0, time.UTC)
	view := BuildSnapshot(testConfig(), emptySnapshot(now), []usage.Event{{
		Provider:  "codex",
		SessionID: "synthetic",
		CWD:       "/work/synthetic",
		Timestamp: now,
		Total:     0,
	}}, nil, now)

	data, err := json.Marshal(view)
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(string(data), "0001-01-01") {
		t.Fatalf("serialized zero time: %s", data)
	}
	if strings.Contains(string(data), `"turns":null`) || strings.Contains(string(data), `"sources":null`) {
		t.Fatalf("serialized null arrays: %s", data)
	}
}

func TestProjectNameHandlesWindowsPaths(t *testing.T) {
	if got := projectName(`C:\Users\me\repo\`); got != "repo" {
		t.Fatalf("projectName windows = %q", got)
	}
	if got := projectName(`C:\Users\me/repo`); got != "repo" {
		t.Fatalf("projectName mixed = %q", got)
	}
}

func requireAgentState(t *testing.T, agents []AgentView, id, state string) {
	t.Helper()
	for _, agent := range agents {
		if agent.ID == id {
			if agent.State != state {
				t.Fatalf("agent %s state = %q, want %q", id, agent.State, state)
			}
			return
		}
	}
	t.Fatalf("missing agent %s in %#v", id, agents)
}

func requireAgent(t *testing.T, agents []AgentView, id string) AgentView {
	t.Helper()
	for _, agent := range agents {
		if agent.ID == id {
			return agent
		}
	}
	t.Fatalf("missing agent %s in %#v", id, agents)
	return AgentView{}
}

func requireSession(t *testing.T, sessions []SessionView, id string) SessionView {
	t.Helper()
	for _, session := range sessions {
		if session.ID == id {
			return session
		}
	}
	t.Fatalf("missing session %s in %#v", id, sessions)
	return SessionView{}
}

func emptySnapshot(now time.Time) *platform.Snapshot {
	return &platform.Snapshot{
		At:        now,
		Platform:  "test",
		Processes: map[int32]platform.Process{},
		Children:  map[int32][]int32{},
	}
}

func testConfig() *config.Config {
	enabled := true
	return &config.Config{
		Version: 1,
		Mode:    config.ModeAlert,
		Service: config.ServiceConfig{MinConfidence: 50},
		Usage: config.UsageConfig{
			Enabled:        &enabled,
			Window:         config.Duration{Duration: 15 * time.Minute},
			WarnTurnTokens: 250_000,
			KillTurnTokens: 750_000,
		},
		Defaults: config.Policy{
			WarnAfter:    config.Duration{Duration: time.Hour},
			KillAfter:    config.Duration{Duration: 2 * time.Hour},
			AckExtension: config.Duration{Duration: 30 * time.Minute},
		},
		Agents: []config.Agent{
			{
				ID:     "codex-test",
				Label:  "Codex Test",
				Family: "codex",
				Kind:   config.AgentKindProcess,
				Match:  config.Match{ProcessNames: []string{"codex"}},
			},
			{
				ID:     "claude-test",
				Label:  "Claude Test",
				Family: "claude",
				Kind:   config.AgentKindProcess,
				Match:  config.Match{ProcessNames: []string{"claude"}},
			},
		},
	}
}
