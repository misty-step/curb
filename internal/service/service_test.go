package service

import (
	"context"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/phaedrus/curb/internal/config"
	"github.com/phaedrus/curb/internal/ledger"
	"github.com/phaedrus/curb/internal/platform"
	"github.com/phaedrus/curb/internal/usagewatch"
)

func TestServiceServesSnapshotFromFileBackedConfig(t *testing.T) {
	path := writeServiceTestConfig(t)
	svc := newTestService(t, path)

	if err := svc.Refresh(context.Background()); err != nil {
		t.Fatal(err)
	}
	snapshot, err := svc.Snapshot(context.Background())
	if err != nil {
		t.Fatal(err)
	}

	if snapshot.Overview.ActiveAgents != 1 {
		t.Fatalf("active agents = %d", snapshot.Overview.ActiveAgents)
	}
	if len(snapshot.Agents) != 1 || snapshot.Agents[0].ID != "synthetic-sleep" || snapshot.Agents[0].State != "running" {
		t.Fatalf("agents = %#v", snapshot.Agents)
	}
	view, err := svc.Config(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if view.Path != path || view.Mode != "visibility" {
		t.Fatalf("config view = %#v", view)
	}
}

func TestServiceUpdateConfigPersistsAndRefreshesSnapshotPolicy(t *testing.T) {
	path := writeServiceTestConfig(t)
	svc := newTestService(t, path)
	if err := svc.Refresh(context.Background()); err != nil {
		t.Fatal(err)
	}
	mode := "enforcement"
	warn := int64(2000)
	kill := int64(4000)
	view, err := svc.UpdateConfig(context.Background(), ConfigUpdate{
		Mode:           &mode,
		WarnTurnTokens: &warn,
		KillTurnTokens: &kill,
	})
	if err != nil {
		t.Fatal(err)
	}

	if view.Mode != "enforcement" || view.WarnTurnTokens != warn || view.KillTurnTokens != kill {
		t.Fatalf("updated view = %#v", view)
	}
	reloaded, err := config.Load(path)
	if err != nil {
		t.Fatal(err)
	}
	if reloaded.Mode != config.ModeEnforcement || reloaded.Usage.WarnTurnTokens != warn || reloaded.Usage.KillTurnTokens != kill {
		t.Fatalf("reloaded = %#v", reloaded)
	}
	if len(reloaded.Agents) != 1 || len(reloaded.Agents[0].Match.ProcessNames) != 1 {
		t.Fatalf("agent match data was not preserved: %#v", reloaded.Agents)
	}
	snapshot, err := svc.Snapshot(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if snapshot.Overview.Mode != "enforcement" {
		t.Fatalf("snapshot mode = %q", snapshot.Overview.Mode)
	}
}

func TestServiceRescanRefreshesCachedSnapshot(t *testing.T) {
	path := writeServiceTestConfig(t)
	includeProcess := false
	svc, err := New(path, func(context.Context) (*platform.Snapshot, error) {
		now := time.Now()
		processes := map[int32]platform.Process{}
		if includeProcess {
			processes[42] = platform.Process{
				PID:       42,
				Name:      "sleep",
				Exe:       "/bin/sleep",
				Cmdline:   "sleep 600",
				CWD:       filepath.Dir(path),
				Create:    now.Add(-time.Minute),
				StartedOK: true,
			}
		}
		return &platform.Snapshot{At: now, Platform: "test", Processes: processes, Children: map[int32][]int32{}}, nil
	})
	if err != nil {
		t.Fatal(err)
	}
	if err := svc.Refresh(context.Background()); err != nil {
		t.Fatal(err)
	}
	before, err := svc.Snapshot(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if before.Overview.ActiveAgents != 0 {
		t.Fatalf("before = %#v", before.Overview)
	}

	includeProcess = true
	after, err := svc.Rescan(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if after.Overview.ActiveAgents != 1 || len(after.Agents) != 1 || after.Agents[0].ID != "synthetic-sleep" {
		t.Fatalf("after = %#v", after)
	}
}

func TestServiceSnapshotReportsPlatformCapabilities(t *testing.T) {
	path := writeServiceTestConfig(t)
	svc := newTestService(t, path)
	svc.notifyCaps = func() platform.NotificationCapability {
		return platform.NotificationCapability{Supported: true, Status: "available", Message: "available"}
	}

	if err := svc.Refresh(context.Background()); err != nil {
		t.Fatal(err)
	}
	snapshot, err := svc.Snapshot(context.Background())
	if err != nil {
		t.Fatal(err)
	}

	caps := snapshot.Overview.Capabilities
	if caps.Platform != "test" {
		t.Fatalf("platform = %q", caps.Platform)
	}
	if !caps.Notifications.Available || caps.Notifications.Status != "ready" {
		t.Fatalf("notification capability = %#v", caps.Notifications)
	}
	if !caps.ProcessCapture.Available || caps.ProcessCapture.Status != "ready" {
		t.Fatalf("process capture capability = %#v", caps.ProcessCapture)
	}
	if !caps.ProcessIdentity.Available || caps.ProcessIdentity.Status != "ready" {
		t.Fatalf("process identity capability = %#v", caps.ProcessIdentity)
	}
	if caps.Enforcement.Available || caps.Enforcement.Status != "disabled" {
		t.Fatalf("enforcement capability = %#v", caps.Enforcement)
	}
}

func TestServiceCapabilitiesBlockEnforcementWhenIdentityIsWeak(t *testing.T) {
	path := writeServiceTestConfig(t)
	cfg, err := config.Load(path)
	if err != nil {
		t.Fatal(err)
	}
	cfg.Mode = config.ModeEnforcement
	if err := config.Save(path, cfg); err != nil {
		t.Fatal(err)
	}
	svc, err := New(path, func(context.Context) (*platform.Snapshot, error) {
		now := time.Now()
		return &platform.Snapshot{
			At:       now,
			Platform: "test",
			Processes: map[int32]platform.Process{
				42: {
					PID:  42,
					Name: "sleep",
				},
			},
			Children: map[int32][]int32{},
		}, nil
	})
	if err != nil {
		t.Fatal(err)
	}
	svc.notifyCaps = func() platform.NotificationCapability {
		return platform.NotificationCapability{Supported: true, Status: "available", Message: "available"}
	}

	if err := svc.Refresh(context.Background()); err != nil {
		t.Fatal(err)
	}
	snapshot, err := svc.Snapshot(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	caps := snapshot.Overview.Capabilities
	if caps.ProcessIdentity.Available || caps.ProcessIdentity.Status != "degraded" {
		t.Fatalf("process identity capability = %#v", caps.ProcessIdentity)
	}
	if caps.Enforcement.Available || caps.Enforcement.Status != "blocked" {
		t.Fatalf("enforcement capability = %#v", caps.Enforcement)
	}
}

func TestServiceCapabilitiesReportEnforcementReadyForLiveEnforceableWorker(t *testing.T) {
	path := writeServiceTestConfig(t)
	cfg, err := config.Load(path)
	if err != nil {
		t.Fatal(err)
	}
	cfg.Mode = config.ModeEnforcement
	if err := config.Save(path, cfg); err != nil {
		t.Fatal(err)
	}
	svc := newTestService(t, path)
	svc.notifyCaps = func() platform.NotificationCapability {
		return platform.NotificationCapability{Supported: true, Status: "available", Message: "available"}
	}

	if err := svc.Refresh(context.Background()); err != nil {
		t.Fatal(err)
	}
	snapshot, err := svc.Snapshot(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if !snapshot.Overview.Capabilities.Enforcement.Available || snapshot.Overview.Capabilities.Enforcement.Status != "ready" {
		t.Fatalf("enforcement capability = %#v", snapshot.Overview.Capabilities.Enforcement)
	}
}

func TestServiceRefreshReportsCaptureFailureWithoutStaleCapabilities(t *testing.T) {
	path := writeServiceTestConfig(t)
	captureOK := true
	svc, err := New(path, func(context.Context) (*platform.Snapshot, error) {
		if !captureOK {
			return nil, errors.New("capture unavailable")
		}
		now := time.Now()
		return &platform.Snapshot{
			At:       now,
			Platform: "test",
			Processes: map[int32]platform.Process{
				42: {
					PID:       42,
					Name:      "sleep",
					Exe:       "/bin/sleep",
					Cmdline:   "sleep 600",
					CWD:       filepath.Dir(path),
					Create:    now.Add(-time.Minute),
					StartedOK: true,
				},
			},
			Children: map[int32][]int32{},
		}, nil
	})
	if err != nil {
		t.Fatal(err)
	}
	svc.notifyCaps = func() platform.NotificationCapability {
		return platform.NotificationCapability{Supported: true, Status: "available", Message: "available"}
	}
	if err := svc.Refresh(context.Background()); err != nil {
		t.Fatal(err)
	}
	captureOK = false
	if err := svc.Refresh(context.Background()); err != nil {
		t.Fatal(err)
	}
	snapshot, err := svc.Snapshot(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	caps := snapshot.Overview.Capabilities
	if caps.ProcessCapture.Available || caps.ProcessCapture.Status != "error" || !strings.Contains(caps.ProcessCapture.Message, "capture unavailable") {
		t.Fatalf("process capture capability = %#v", caps.ProcessCapture)
	}
	if len(snapshot.Agents) != 0 {
		t.Fatalf("agents should be empty after capture failure: %#v", snapshot.Agents)
	}
}

func TestServiceRejectsInvalidConfigUpdateAtomically(t *testing.T) {
	path := writeServiceTestConfig(t)
	svc := newTestService(t, path)
	before, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	warn := int64(5000)
	kill := int64(4000)

	if _, err := svc.UpdateConfig(context.Background(), ConfigUpdate{WarnTurnTokens: &warn, KillTurnTokens: &kill}); err == nil {
		t.Fatal("expected invalid threshold update to fail")
	}
	view, err := svc.Config(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if view.WarnTurnTokens != 1000 || view.KillTurnTokens != 3000 {
		t.Fatalf("in-memory config mutated: %#v", view)
	}
	after, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	if string(after) != string(before) {
		t.Fatalf("config file changed after invalid update\nbefore:\n%s\nafter:\n%s", before, after)
	}
}

func TestServiceNotificationHealthAndTest(t *testing.T) {
	path := writeServiceTestConfig(t)
	svc := newTestService(t, path)
	svc.notifyCaps = func() platform.NotificationCapability {
		return platform.NotificationCapability{Supported: true, Status: "available", Message: "available"}
	}
	calls := 0
	svc.notify = func(title, message string) error {
		calls++
		if title == "" || message == "" {
			t.Fatalf("empty notification title=%q message=%q", title, message)
		}
		return nil
	}

	health, err := svc.NotificationHealth(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if !health.Enabled || health.Status == "disabled" {
		t.Fatalf("health = %#v", health)
	}

	tested, err := svc.TestNotification(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if calls != 1 || tested.Status != "delivered" || tested.LastTestAt == "" {
		t.Fatalf("calls=%d tested=%#v", calls, tested)
	}

	health, err = svc.NotificationHealth(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if health.Status != "delivered" || health.LastTestAt == "" {
		t.Fatalf("health after test = %#v", health)
	}
}

func TestServiceNotificationTestDisabledDoesNotNotify(t *testing.T) {
	path := writeServiceTestConfig(t)
	cfg, err := config.Load(path)
	if err != nil {
		t.Fatal(err)
	}
	cfg.Alerts.LocalNotifications = false
	if err := config.Save(path, cfg); err != nil {
		t.Fatal(err)
	}
	svc := newTestService(t, path)
	svc.notifyCaps = func() platform.NotificationCapability {
		return platform.NotificationCapability{Supported: true, Status: "available", Message: "available"}
	}
	svc.notify = func(string, string) error {
		t.Fatal("notify should not be called when local notifications are disabled")
		return nil
	}

	view, err := svc.TestNotification(context.Background())
	if !errors.Is(err, ErrNotificationsDisabled) {
		t.Fatalf("err=%v view=%#v", err, view)
	}
	if view.Status != "disabled" || view.Enabled {
		t.Fatalf("view=%#v", view)
	}
}

func TestServiceNotificationTestRecordsNotifyError(t *testing.T) {
	path := writeServiceTestConfig(t)
	svc := newTestService(t, path)
	svc.notifyCaps = func() platform.NotificationCapability {
		return platform.NotificationCapability{Supported: true, Status: "available", Message: "available"}
	}
	svc.notify = func(string, string) error {
		return errors.New("permission denied")
	}

	view, err := svc.TestNotification(context.Background())
	if err == nil || view.Status != "error" || view.LastError != "permission denied" || view.LastTestAt == "" {
		t.Fatalf("err=%v view=%#v", err, view)
	}
	health, err := svc.NotificationHealth(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if health.Status != "error" || health.LastError != "permission denied" {
		t.Fatalf("health = %#v", health)
	}
}

func TestServiceNotificationHealthReportsUnavailableCapability(t *testing.T) {
	path := writeServiceTestConfig(t)
	svc := newTestService(t, path)
	svc.notifyCaps = func() platform.NotificationCapability {
		return platform.NotificationCapability{Supported: false, Status: "unavailable", Message: "notify-send not found"}
	}

	health, err := svc.NotificationHealth(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if health.Available || health.Status != "unavailable" || health.Message != "notify-send not found" {
		t.Fatalf("health = %#v", health)
	}

	view, err := svc.TestNotification(context.Background())
	if !errors.Is(err, ErrNotificationsUnavailable) {
		t.Fatalf("err=%v view=%#v", err, view)
	}
}

func TestServiceNotificationHealthDoesNotMaskCurrentUnavailabilityWithPastSuccess(t *testing.T) {
	path := writeServiceTestConfig(t)
	svc := newTestService(t, path)
	available := true
	svc.notifyCaps = func() platform.NotificationCapability {
		if available {
			return platform.NotificationCapability{Supported: true, Status: "available", Message: "available"}
		}
		return platform.NotificationCapability{Supported: false, Status: "unavailable", Message: "notify-send not found"}
	}
	svc.notify = func(string, string) error {
		return nil
	}

	tested, err := svc.TestNotification(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if tested.Status != "delivered" {
		t.Fatalf("tested = %#v", tested)
	}

	available = false
	health, err := svc.NotificationHealth(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if health.Status != "unavailable" || health.Available || health.LastTestAt == "" {
		t.Fatalf("health = %#v", health)
	}
}

func TestServiceOnboardingRequiredAndCompletionMarker(t *testing.T) {
	path := writeServiceTestConfig(t)
	svc := newTestService(t, path)
	svc.notifyCaps = func() platform.NotificationCapability {
		return platform.NotificationCapability{Supported: true, Status: "available", Message: "available"}
	}
	if err := svc.Refresh(context.Background()); err != nil {
		t.Fatal(err)
	}

	view, err := svc.Onboarding(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if !view.Required || view.ConfigPath != path || view.Mode != "visibility" || view.FinalSentence == "" {
		t.Fatalf("view = %#v", view)
	}
	if view.EnforceableAgentTypes != 1 || view.WatchOnlyAgentTypes != 0 {
		t.Fatalf("agent counts = %#v", view)
	}
	if view.Capabilities.Platform != "test" || view.Capabilities.ProcessCapture.Status != "ready" {
		t.Fatalf("capabilities = %#v", view.Capabilities)
	}
	if len(view.Steps) == 0 {
		t.Fatal("missing onboarding steps")
	}

	completed, err := svc.CompleteOnboarding(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if completed.Required {
		t.Fatalf("completed view = %#v", completed)
	}
}

func TestServiceOnboardingAlertModeFinalSentenceAndWatchOnlyCounts(t *testing.T) {
	path := writeServiceTestConfig(t)
	cfg, err := config.Load(path)
	if err != nil {
		t.Fatal(err)
	}
	cfg.Mode = config.ModeAlert
	cfg.Agents = append(cfg.Agents, config.Agent{
		ID:     "codex-desktop",
		Label:  "Codex Desktop",
		Family: "codex",
		Kind:   config.AgentKindApp,
		Match:  config.Match{ProcessNames: []string{"Codex"}},
	})
	if err := config.Save(path, cfg); err != nil {
		t.Fatal(err)
	}
	svc := newTestService(t, path)
	svc.notifyCaps = func() platform.NotificationCapability {
		return platform.NotificationCapability{Supported: false, Status: "unavailable", Message: "notifications unavailable"}
	}

	view, err := svc.Onboarding(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	want := "Curb will notify on high-token turns. It will not stop any process in Alert mode. Desktop app roots are watch-only."
	if view.FinalSentence != want || view.ModeCanTerminate {
		t.Fatalf("view = %#v", view)
	}
	if view.EnforceableAgentTypes != 1 || view.WatchOnlyAgentTypes != 1 {
		t.Fatalf("agent counts = %#v", view)
	}
	requireOnboardingStep(t, view, "notifications", "action")
	requireOnboardingStep(t, view, "safety", "done")
}

func TestServiceOnboardingRefreshesColdSnapshotAndShowsDetections(t *testing.T) {
	path := writeServiceTestConfig(t)
	writeCodexUsageFixture(t, "session-1", filepath.Dir(path), time.Now().UTC())
	svc := newTestService(t, path)
	svc.notifyCaps = func() platform.NotificationCapability {
		return platform.NotificationCapability{Supported: true, Status: "available", Message: "available"}
	}

	view, err := svc.Onboarding(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if !containsString(view.DetectedProviders, "codex") {
		t.Fatalf("detected providers = %#v", view.DetectedProviders)
	}
	if !containsString(view.DetectedWorkers, "Synthetic Sleep") {
		t.Fatalf("detected workers = %#v", view.DetectedWorkers)
	}
	requireOnboardingStep(t, view, "sources", "done")
}

func TestServiceOnboardingSurfacesSnapshotFailure(t *testing.T) {
	path := writeServiceTestConfig(t)
	svc, err := New(path, func(context.Context) (*platform.Snapshot, error) {
		return nil, errors.New("process capture failed")
	})
	if err != nil {
		t.Fatal(err)
	}
	svc.notifyCaps = func() platform.NotificationCapability {
		return platform.NotificationCapability{Supported: true, Status: "available", Message: "available"}
	}

	view, err := svc.Onboarding(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	step := requireOnboardingStep(t, view, "sources", "action")
	if !strings.Contains(step.Message, "process capture failed") {
		t.Fatalf("sources step = %#v", step)
	}
}

func TestServiceOnboardingEnforcementModeCanTerminateOnlyWithEnforceableAgents(t *testing.T) {
	path := writeServiceTestConfig(t)
	cfg, err := config.Load(path)
	if err != nil {
		t.Fatal(err)
	}
	cfg.Mode = config.ModeEnforcement
	cfg.Agents = []config.Agent{{
		ID:     "codex-desktop",
		Label:  "Codex Desktop",
		Family: "codex",
		Kind:   config.AgentKindApp,
		Match:  config.Match{ProcessNames: []string{"Codex"}},
	}}
	if err := config.Save(path, cfg); err != nil {
		t.Fatal(err)
	}
	svc := newTestService(t, path)
	svc.notifyCaps = func() platform.NotificationCapability {
		return platform.NotificationCapability{Supported: true, Status: "available", Message: "available"}
	}

	view, err := svc.Onboarding(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if view.ModeCanTerminate || view.EnforceableAgentTypes != 0 || view.WatchOnlyAgentTypes != 1 {
		t.Fatalf("view = %#v", view)
	}
}

func requireOnboardingStep(t *testing.T, view OnboardingView, id, status string) OnboardingStepView {
	t.Helper()
	for _, step := range view.Steps {
		if step.ID == id {
			if step.Status != status {
				t.Fatalf("step %s status = %q, want %q; view=%#v", id, step.Status, status, view)
			}
			return step
		}
	}
	t.Fatalf("missing step %s in %#v", id, view.Steps)
	return OnboardingStepView{}
}

func containsString(values []string, want string) bool {
	for _, value := range values {
		if value == want {
			return true
		}
	}
	return false
}

func TestServiceEventsProjectRealLedgerLimit(t *testing.T) {
	path := writeServiceTestConfig(t)
	cfg, err := config.Load(path)
	if err != nil {
		t.Fatal(err)
	}
	log, err := ledger.Open(cfg.Ledger.Path)
	if err != nil {
		t.Fatal(err)
	}
	for _, typ := range []string{"run_started", "usage_warning", "usage_would_terminate"} {
		if err := log.Append(ledger.Event{Type: typ}); err != nil {
			t.Fatal(err)
		}
	}
	svc := newTestService(t, path)

	events, err := svc.Events(context.Background(), 2)
	if err != nil {
		t.Fatal(err)
	}
	if len(events) != 2 ||
		events[0].Category != "alert" || events[0].Kind != "warning" ||
		events[1].Category != "alert" || events[1].Kind != "would_stop" {
		t.Fatalf("events = %#v", events)
	}
}

func TestServiceScanRunsUsagePolicy(t *testing.T) {
	now := time.Now().UTC()
	writeCodexUsageFixtureWithTotal(t, "policy-session", "/tmp/curb-service-policy", now, 1500)
	path := writeServiceTestConfig(t)
	cfg, err := config.Load(path)
	if err != nil {
		t.Fatal(err)
	}
	cfg.Alerts.LocalNotifications = false
	if err := config.Save(path, cfg); err != nil {
		t.Fatal(err)
	}
	svc := newTestService(t, path)

	if err := svc.Scan(context.Background()); err != nil {
		t.Fatal(err)
	}

	events, err := svc.Events(context.Background(), 10)
	if err != nil {
		t.Fatal(err)
	}
	if len(events) != 1 || events[0].Category != "alert" || events[0].Kind != "warning" {
		t.Fatalf("events = %#v", events)
	}
	snapshot, err := svc.Snapshot(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	session := snapshot.Sessions[0]
	if session.ID != "policy-session" || session.State != "uncorrelated" || session.UsageState != "warn" {
		t.Fatalf("session = %#v", session)
	}
}

func TestServiceSessionTurnsUsesLookbackNotPolicyWindow(t *testing.T) {
	now := time.Now().UTC()
	writeCodexUsageFixtureWithTotal(t, "history-session", "/tmp/curb-service-history", now.Add(-30*time.Minute), 900)
	path := writeServiceTestConfig(t)
	svc := newTestService(t, path)
	if err := svc.Refresh(context.Background()); err != nil {
		t.Fatal(err)
	}
	snapshot, err := svc.Snapshot(context.Background())
	if err != nil {
		t.Fatal(err)
	}
	if len(snapshot.Turns) != 0 || snapshot.Overview.WindowTokens != 0 {
		t.Fatalf("snapshot should exclude old turn: turns=%#v overview=%#v", snapshot.Turns, snapshot.Overview)
	}

	turns, err := svc.SessionTurns(context.Background(), "history-session", TurnQuery{Limit: 10})
	if err != nil {
		t.Fatal(err)
	}
	if len(turns) != 1 || turns[0].SessionID != "history-session" || turns[0].TotalTokens != 900 {
		t.Fatalf("turns = %#v", turns)
	}
}

func TestServiceSessionTurnsAppliesSinceAndLimit(t *testing.T) {
	now := time.Now().UTC()
	writeCodexUsageFixtureRows(t, "turn-limit-session", "/tmp/curb-service-turns", []usageFixtureRow{
		{At: now.Add(-45 * time.Minute), Total: 100},
		{At: now.Add(-30 * time.Minute), Total: 200},
		{At: now.Add(-15 * time.Minute), Total: 300},
	})
	svc := newTestService(t, writeServiceTestConfig(t))

	turns, err := svc.SessionTurns(context.Background(), "codex:turn-limit-session", TurnQuery{
		Since: now.Add(-40 * time.Minute),
		Limit: 1,
	})
	if err != nil {
		t.Fatal(err)
	}
	if len(turns) != 1 || turns[0].TotalTokens != 300 {
		t.Fatalf("turns = %#v", turns)
	}
}

func TestServiceSessionTurnsReturnsEmptyForKnownSessionWithoutTurnsSince(t *testing.T) {
	now := time.Now().UTC()
	writeCodexUsageFixtureWithTotal(t, "quiet-range-session", "/tmp/curb-service-quiet-range", now.Add(-30*time.Minute), 900)
	svc := newTestService(t, writeServiceTestConfig(t))

	turns, err := svc.SessionTurns(context.Background(), "quiet-range-session", TurnQuery{Since: now.Add(-time.Minute)})
	if err != nil {
		t.Fatal(err)
	}
	if len(turns) != 0 {
		t.Fatalf("turns = %#v", turns)
	}
}

func TestServiceAcknowledgesSessionWithDurableStateAndLedgerEvent(t *testing.T) {
	writeCodexUsageFixture(t, "session-1", "/tmp/curb-service-test", time.Now().UTC())
	path := writeServiceTestConfig(t)
	cfg, err := config.Load(path)
	if err != nil {
		t.Fatal(err)
	}
	svc := newTestService(t, path)
	if err := svc.Refresh(context.Background()); err != nil {
		t.Fatal(err)
	}

	ack, err := svc.AcknowledgeSession(context.Background(), "session-1", AckRequest{
		ExtendSeconds: 120,
		Reason:        "still supervising",
	})
	if err != nil {
		t.Fatal(err)
	}
	if ack.SessionKey != "codex:session-1" || ack.ExtendSeconds != 60 || ack.Reason != "still supervising" {
		t.Fatalf("ack = %#v", ack)
	}
	stored, ok, err := usagewatch.ReadSessionAck(cfg.Service.StateDir, "codex:session-1")
	if err != nil {
		t.Fatal(err)
	}
	if !ok || stored.SessionKey != "codex:session-1" || !time.Now().Before(stored.Until) {
		t.Fatalf("stored ack = %#v ok=%v", stored, ok)
	}
	events, err := svc.Events(context.Background(), 10)
	if err != nil {
		t.Fatal(err)
	}
	if len(events) != 1 || events[0].Category != "ack" || events[0].Kind != "received" {
		t.Fatalf("events = %#v", events)
	}
}

func TestServiceRejectsNegativeSessionAckExtension(t *testing.T) {
	svc := newTestService(t, writeServiceTestConfig(t))

	if _, err := svc.AcknowledgeSession(context.Background(), "synthetic:session-1", AckRequest{ExtendSeconds: -1}); !errors.Is(err, ErrInvalidAck) {
		t.Fatalf("err = %v, want ErrInvalidAck", err)
	}
}

func TestServiceRestoresPreviousSessionAckWhenLedgerAppendFails(t *testing.T) {
	writeCodexUsageFixture(t, "session-1", "/tmp/curb-service-test", time.Now().UTC())
	path := writeServiceTestConfig(t)
	cfg, err := config.Load(path)
	if err != nil {
		t.Fatal(err)
	}
	previous, err := usagewatch.WriteSessionAck(cfg.Service.StateDir, "codex:session-1", time.Minute, "previous", time.Now().UTC().Add(-time.Second))
	if err != nil {
		t.Fatal(err)
	}
	svc := newTestService(t, path)
	if err := svc.Refresh(context.Background()); err != nil {
		t.Fatal(err)
	}
	svc.mu.Lock()
	svc.cfg.Ledger.Path = cfg.Service.StateDir
	svc.mu.Unlock()

	if _, err := svc.AcknowledgeSession(context.Background(), "session-1", AckRequest{ExtendSeconds: 30, Reason: "replacement"}); err == nil {
		t.Fatal("expected ledger append failure")
	}
	stored, ok, err := usagewatch.ReadSessionAck(cfg.Service.StateDir, "codex:session-1")
	if err != nil {
		t.Fatal(err)
	}
	if !ok || stored.Reason != previous.Reason || !stored.Until.Equal(previous.Until) {
		t.Fatalf("stored ack = %#v, previous = %#v, ok=%v", stored, previous, ok)
	}
}

func TestServiceRejectsMissingSessionAck(t *testing.T) {
	svc := newTestService(t, writeServiceTestConfig(t))
	if err := svc.Refresh(context.Background()); err != nil {
		t.Fatal(err)
	}

	if _, err := svc.AcknowledgeSession(context.Background(), "missing", AckRequest{}); !errors.Is(err, ErrSessionNotFound) {
		t.Fatalf("err = %v, want ErrSessionNotFound", err)
	}
}

func TestServiceStopSessionRevalidatesIdentityAndTerminatesTree(t *testing.T) {
	started := time.Now().UTC().Add(-time.Minute)
	path := writeStopServiceTestConfig(t)
	writeCodexUsageFixtureWithTotal(t, "stop-session", "/tmp/curb-stop-success", time.Now().UTC(), 5_000)
	svc, terminated := newStopTestService(t, path, platform.Process{
		PID:       4242,
		Name:      "codex",
		Exe:       "/usr/local/bin/codex",
		Cmdline:   "codex",
		CWD:       "/tmp/curb-stop-success",
		Username:  "phaedrus",
		Create:    started,
		StartedOK: true,
	})

	view, err := svc.StopSession(context.Background(), "stop-session", StopRequest{
		Confirm: true,
		Scope:   "tree",
		Reason:  "manual test",
		Expected: StopExpectedIdentity{
			PID:        4242,
			StartedAt:  started,
			Owner:      "phaedrus",
			Executable: "/usr/local/bin/codex",
		},
	})
	if err != nil {
		t.Fatal(err)
	}
	if view.SessionKey != "codex:stop-session" || view.AgentID != "codex-cli" || view.PID != 4242 || view.Owner != "phaedrus" || view.Executable != "/usr/local/bin/codex" {
		t.Fatalf("stop view = %#v", view)
	}
	if got := *terminated; len(got) != 2 || got[0] != 4242 || got[1] != 4243 {
		t.Fatalf("terminated pids = %#v", got)
	}
	events, err := ledger.Read(configMustLoad(t, path).Ledger.Path)
	if err != nil {
		t.Fatal(err)
	}
	if !ledgerContains(events, "manual_stop_started") || !ledgerContains(events, "manual_stop_completed") {
		t.Fatalf("manual stop events = %#v", events)
	}
}

func TestServiceStopSessionRejectsStaleIdentity(t *testing.T) {
	started := time.Now().UTC().Add(-time.Minute)
	path := writeStopServiceTestConfig(t)
	writeCodexUsageFixtureWithTotal(t, "stop-session", "/tmp/curb-stop-stale", time.Now().UTC(), 5_000)
	svc, terminated := newStopTestService(t, path, platform.Process{
		PID:       4242,
		Name:      "codex",
		Exe:       "/usr/local/bin/codex",
		Cmdline:   "codex",
		CWD:       "/tmp/curb-stop-stale",
		Username:  "phaedrus",
		Create:    started,
		StartedOK: true,
	})

	_, err := svc.StopSession(context.Background(), "stop-session", StopRequest{
		Confirm: true,
		Scope:   "tree",
		Expected: StopExpectedIdentity{
			PID:        4242,
			StartedAt:  started.Add(-time.Second),
			Owner:      "phaedrus",
			Executable: "/usr/local/bin/codex",
		},
	})
	if !errors.Is(err, ErrStopConflict) {
		t.Fatalf("err = %v, want ErrStopConflict", err)
	}
	if len(*terminated) != 0 {
		t.Fatalf("terminated on stale identity: %#v", *terminated)
	}
}

func TestServiceStopSessionRejectsWatchOnlyAndWeakIdentity(t *testing.T) {
	started := time.Now().UTC().Add(-time.Minute)
	path := writeStopServiceTestConfig(t)
	cfg := configMustLoad(t, path)
	cfg.Agents[0].Kind = config.AgentKindApp
	if err := config.Save(path, cfg); err != nil {
		t.Fatal(err)
	}
	writeCodexUsageFixtureWithTotal(t, "stop-session", "/tmp/curb-stop-watch-only", time.Now().UTC(), 5_000)
	svc, terminated := newStopTestService(t, path, platform.Process{
		PID:       4242,
		Name:      "codex",
		Exe:       "/Applications/Codex.app/Contents/Resources/codex",
		Cmdline:   "codex",
		CWD:       "/tmp/curb-stop-watch-only",
		Username:  "phaedrus",
		Create:    started,
		StartedOK: true,
	})

	_, err := svc.StopSession(context.Background(), "stop-session", StopRequest{
		Confirm: true,
		Scope:   "tree",
		Expected: StopExpectedIdentity{
			PID:        4242,
			StartedAt:  started,
			Owner:      "phaedrus",
			Executable: "/Applications/Codex.app/Contents/Resources/codex",
		},
	})
	if !errors.Is(err, ErrStopConflict) {
		t.Fatalf("watch-only err = %v, want ErrStopConflict", err)
	}
	if len(*terminated) != 0 {
		t.Fatalf("terminated watch-only target: %#v", *terminated)
	}
}

func TestServiceStopSessionRejectsWithoutEnforcementMode(t *testing.T) {
	started := time.Now().UTC().Add(-time.Minute)
	path := writeStopServiceTestConfig(t)
	cfg := configMustLoad(t, path)
	cfg.Mode = config.ModeAlert
	if err := config.Save(path, cfg); err != nil {
		t.Fatal(err)
	}
	writeCodexUsageFixtureWithTotal(t, "stop-session", "/tmp/curb-stop-alert", time.Now().UTC(), 5_000)
	svc, terminated := newStopTestService(t, path, platform.Process{
		PID:       4242,
		Name:      "codex",
		Exe:       "/usr/local/bin/codex",
		Cmdline:   "codex",
		CWD:       "/tmp/curb-stop-alert",
		Username:  "phaedrus",
		Create:    started,
		StartedOK: true,
	})

	_, err := svc.StopSession(context.Background(), "stop-session", stopRequestFor(4242, started))
	if !errors.Is(err, ErrStopConflict) {
		t.Fatalf("err = %v, want ErrStopConflict", err)
	}
	if len(*terminated) != 0 {
		t.Fatalf("terminated outside enforcement: %#v", *terminated)
	}
}

func TestServiceStopSessionRejectsUncorrelatedSession(t *testing.T) {
	started := time.Now().UTC().Add(-time.Minute)
	path := writeStopServiceTestConfig(t)
	writeCodexUsageFixtureWithTotal(t, "stop-session", "/tmp/curb-stop-uncorrelated", time.Now().UTC(), 5_000)
	svc, terminated := newStopTestService(t, path, platform.Process{
		PID:       4242,
		Name:      "codex",
		Exe:       "/usr/local/bin/codex",
		Cmdline:   "codex",
		CWD:       "/tmp/other-project",
		Username:  "phaedrus",
		Create:    started,
		StartedOK: true,
	})

	_, err := svc.StopSession(context.Background(), "stop-session", stopRequestFor(4242, started))
	if !errors.Is(err, ErrStopConflict) {
		t.Fatalf("err = %v, want ErrStopConflict", err)
	}
	if len(*terminated) != 0 {
		t.Fatalf("terminated uncorrelated target: %#v", *terminated)
	}
}

func TestServiceStopSessionRejectsInvalidConfirmation(t *testing.T) {
	path := writeStopServiceTestConfig(t)
	svc := newTestService(t, path)

	if _, err := svc.StopSession(context.Background(), "stop-session", StopRequest{}); !errors.Is(err, ErrInvalidStop) {
		t.Fatalf("missing confirmation err = %v, want ErrInvalidStop", err)
	}
	if _, err := svc.StopSession(context.Background(), "stop-session", StopRequest{Confirm: true, Scope: "pid"}); !errors.Is(err, ErrInvalidStop) {
		t.Fatalf("unsupported scope err = %v, want ErrInvalidStop", err)
	}
	if _, err := svc.StopSession(context.Background(), "stop-session", StopRequest{Confirm: true, Scope: "tree", Expected: StopExpectedIdentity{PID: 4242}}); !errors.Is(err, ErrInvalidStop) {
		t.Fatalf("missing identity err = %v, want ErrInvalidStop", err)
	}
}

func TestServiceStopSessionRejectsWeakFreshProcessIdentity(t *testing.T) {
	started := time.Now().UTC().Add(-time.Minute)
	path := writeStopServiceTestConfig(t)
	writeCodexUsageFixtureWithTotal(t, "stop-session", "/tmp/curb-stop-weak-identity", time.Now().UTC(), 5_000)
	svc, terminated := newStopTestService(t, path, platform.Process{
		PID:       4242,
		Name:      "codex",
		Cmdline:   "codex",
		CWD:       "/tmp/curb-stop-weak-identity",
		Username:  "phaedrus",
		Create:    started,
		StartedOK: true,
	})

	_, err := svc.StopSession(context.Background(), "stop-session", stopRequestFor(4242, started))
	if !errors.Is(err, ErrStopConflict) {
		t.Fatalf("err = %v, want ErrStopConflict", err)
	}
	if len(*terminated) != 0 {
		t.Fatalf("terminated weak identity target: %#v", *terminated)
	}
}

func TestServiceStopSessionRejectsCorrelatedSessionBelowStopThreshold(t *testing.T) {
	started := time.Now().UTC().Add(-time.Minute)
	path := writeStopServiceTestConfig(t)
	writeCodexUsageFixtureWithTotal(t, "stop-session", "/tmp/curb-stop-below-threshold", time.Now().UTC(), 2_000)
	svc, terminated := newStopTestService(t, path, platform.Process{
		PID:       4242,
		Name:      "codex",
		Exe:       "/usr/local/bin/codex",
		Cmdline:   "codex",
		CWD:       "/tmp/curb-stop-below-threshold",
		Username:  "phaedrus",
		Create:    started,
		StartedOK: true,
	})

	_, err := svc.StopSession(context.Background(), "stop-session", stopRequestFor(4242, started))
	if !errors.Is(err, ErrStopConflict) {
		t.Fatalf("err = %v, want ErrStopConflict", err)
	}
	if len(*terminated) != 0 {
		t.Fatalf("terminated below-threshold target: %#v", *terminated)
	}
}

func TestServiceStopSessionRejectsAcknowledgedSession(t *testing.T) {
	started := time.Now().UTC().Add(-time.Minute)
	path := writeStopServiceTestConfig(t)
	cfg := configMustLoad(t, path)
	writeCodexUsageFixtureWithTotal(t, "stop-session", "/tmp/curb-stop-acknowledged", time.Now().UTC(), 5_000)
	if _, err := usagewatch.WriteSessionAck(cfg.Service.StateDir, "codex:stop-session", time.Minute, "still supervising", time.Now().UTC()); err != nil {
		t.Fatal(err)
	}
	svc, terminated := newStopTestService(t, path, platform.Process{
		PID:       4242,
		Name:      "codex",
		Exe:       "/usr/local/bin/codex",
		Cmdline:   "codex",
		CWD:       "/tmp/curb-stop-acknowledged",
		Username:  "phaedrus",
		Create:    started,
		StartedOK: true,
	})

	_, err := svc.StopSession(context.Background(), "stop-session", stopRequestFor(4242, started))
	if !errors.Is(err, ErrStopConflict) {
		t.Fatalf("err = %v, want ErrStopConflict", err)
	}
	if len(*terminated) != 0 {
		t.Fatalf("terminated acknowledged target: %#v", *terminated)
	}
}

func newTestService(t *testing.T, path string) *Service {
	t.Helper()
	svc, err := New(path, func(context.Context) (*platform.Snapshot, error) {
		now := time.Now()
		return &platform.Snapshot{
			At:       now,
			Platform: "test",
			Processes: map[int32]platform.Process{
				42: {
					PID:       42,
					Name:      "sleep",
					Exe:       "/bin/sleep",
					Cmdline:   "sleep 600",
					CWD:       filepath.Dir(path),
					Create:    now.Add(-time.Minute),
					StartedOK: true,
				},
			},
			Children: map[int32][]int32{},
		}, nil
	})
	if err != nil {
		t.Fatal(err)
	}
	return svc
}

func writeServiceTestConfig(t *testing.T) string {
	t.Helper()
	dir := t.TempDir()
	enabled := true
	cfg := &config.Config{
		Version: 1,
		Mode:    config.ModeVisibility,
		Service: config.ServiceConfig{
			ScanInterval:   config.Duration{Duration: time.Second},
			PolicyInterval: config.Duration{Duration: time.Second},
			StateDir:       filepath.Join(dir, "state"),
		},
		Usage: config.UsageConfig{
			Enabled:        &enabled,
			ScanInterval:   config.Duration{Duration: time.Second},
			Lookback:       config.Duration{Duration: time.Hour},
			Window:         config.Duration{Duration: time.Minute},
			WarnTurnTokens: 1000,
			KillTurnTokens: 3000,
			GracePeriod:    config.Duration{Duration: time.Second},
		},
		Defaults: config.Policy{
			WarnAfter:         config.Duration{Duration: time.Minute},
			KillAfter:         config.Duration{Duration: 2 * time.Minute},
			AckExtension:      config.Duration{Duration: time.Minute},
			KillGracePeriod:   config.Duration{Duration: time.Second},
			CooldownAfterKill: config.Duration{Duration: time.Minute},
			MinLifetime:       config.Duration{Duration: time.Second},
			MaxRunGap:         config.Duration{Duration: time.Second},
		},
		Agents: []config.Agent{{
			ID:     "synthetic-sleep",
			Label:  "Synthetic Sleep",
			Family: "synthetic",
			Kind:   config.AgentKindProcess,
			Match:  config.Match{ProcessNames: []string{"sleep"}, CommandRegex: []string{"sleep"}},
		}},
		Alerts: config.AlertConfig{LocalNotifications: true},
		Ledger: config.LedgerConfig{Path: filepath.Join(dir, "state", "runs.ndjson")},
	}
	path := filepath.Join(dir, "config.yaml")
	if err := os.MkdirAll(cfg.Service.StateDir, 0o700); err != nil {
		t.Fatal(err)
	}
	if err := config.Save(path, cfg); err != nil {
		t.Fatal(err)
	}
	return path
}

func writeStopServiceTestConfig(t *testing.T) string {
	t.Helper()
	path := writeServiceTestConfig(t)
	cfg := configMustLoad(t, path)
	cfg.Mode = config.ModeEnforcement
	cfg.Agents = []config.Agent{{
		ID:     "codex-cli",
		Label:  "Codex CLI",
		Family: "codex",
		Kind:   config.AgentKindProcess,
		Match:  config.Match{ProcessNames: []string{"codex"}, CommandRegex: []string{"codex"}},
	}}
	cfg.Usage.WarnTurnTokens = 1_000
	cfg.Usage.KillTurnTokens = 3_000
	if err := config.Save(path, cfg); err != nil {
		t.Fatal(err)
	}
	return path
}

func newStopTestService(t *testing.T, path string, proc platform.Process) (*Service, *[]int32) {
	t.Helper()
	terminated := []int32{}
	svc, err := New(path, func(context.Context) (*platform.Snapshot, error) {
		return &platform.Snapshot{
			At:        time.Now().UTC(),
			Platform:  "test",
			Processes: map[int32]platform.Process{proc.PID: proc, proc.PID + 1: {PID: proc.PID + 1, PPID: proc.PID, Name: "child", Exe: "/usr/local/bin/child", Username: proc.Username, Create: proc.Create, StartedOK: true}},
			Children:  map[int32][]int32{proc.PID: {proc.PID + 1}},
		}, nil
	})
	if err != nil {
		t.Fatal(err)
	}
	svc.terminate = func(_ context.Context, target platform.TerminationTarget, _ time.Duration) platform.TerminationResult {
		terminated = target.PIDs()
		return platform.TerminationResult{SoftSignaled: target.PIDs()}
	}
	return svc, &terminated
}

func stopRequestFor(pid int32, started time.Time) StopRequest {
	return StopRequest{
		Confirm: true,
		Scope:   "tree",
		Expected: StopExpectedIdentity{
			PID:        pid,
			StartedAt:  started,
			Owner:      "phaedrus",
			Executable: "/usr/local/bin/codex",
		},
	}
}

func configMustLoad(t *testing.T, path string) *config.Config {
	t.Helper()
	cfg, err := config.Load(path)
	if err != nil {
		t.Fatal(err)
	}
	return cfg
}

func ledgerContains(events []ledger.Event, typ string) bool {
	for _, event := range events {
		if event.Type == typ {
			return true
		}
	}
	return false
}

func writeCodexUsageFixture(t *testing.T, sessionID, cwd string, at time.Time) {
	writeCodexUsageFixtureWithTotal(t, sessionID, cwd, at, 150)
}

func writeCodexUsageFixtureWithTotal(t *testing.T, sessionID, cwd string, at time.Time, total int64) {
	writeCodexUsageFixtureRows(t, sessionID, cwd, []usageFixtureRow{{At: at, Total: total}})
}

type usageFixtureRow struct {
	At    time.Time
	Total int64
}

func writeCodexUsageFixtureRows(t *testing.T, sessionID, cwd string, rows []usageFixtureRow) {
	t.Helper()
	home := t.TempDir()
	t.Setenv("HOME", home)
	dir := filepath.Join(home, ".codex", "archived_sessions")
	if err := os.MkdirAll(dir, 0o700); err != nil {
		t.Fatal(err)
	}
	content := `{"timestamp":"` + rows[0].At.Format(time.RFC3339Nano) + `","type":"session_meta","payload":{"id":"` + sessionID + `","cwd":"` + cwd + `"}}` + "\n"
	for _, row := range rows {
		content += `{"timestamp":"` + row.At.Format(time.RFC3339Nano) + `","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":100,"cached_input_tokens":0,"output_tokens":50,"total_tokens":` + fmt.Sprintf("%d", row.Total) + `},"total_token_usage":{"total_tokens":` + fmt.Sprintf("%d", row.Total) + `}}}}` + "\n"
	}
	if err := os.WriteFile(filepath.Join(dir, sessionID+".jsonl"), []byte(content), 0o600); err != nil {
		t.Fatal(err)
	}
}
