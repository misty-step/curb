package watchdog

import (
	"context"
	"os/exec"
	"strings"
	"testing"
	"time"

	"github.com/phaedrus/curb/internal/config"
	"github.com/phaedrus/curb/internal/ledger"
	"github.com/phaedrus/curb/internal/platform"
)

func TestMatchPrefersBundleIdentity(t *testing.T) {
	cfg := &config.Config{
		Version: 1,
		Mode:    config.ModeVisibility,
		Service: config.ServiceConfig{MinConfidence: 50},
		Defaults: config.Policy{
			WarnAfter: config.Duration{Duration: time.Minute},
			KillAfter: config.Duration{Duration: 2 * time.Minute},
		},
		Agents: []config.Agent{{
			ID:    "codex-desktop",
			Label: "Codex Desktop",
			Match: config.Match{
				BundleIDs:    []string{"com.openai.codex"},
				ProcessNames: []string{"Codex"},
			},
		}},
	}
	svc := New(cfg, nil)
	snap := &platform.Snapshot{
		At:       time.Now(),
		Platform: "darwin",
		Processes: map[int32]platform.Process{
			42: {PID: 42, Name: "Codex", BundleID: "com.openai.codex", Create: time.Now().Add(-time.Hour), StartedOK: true},
		},
		Children: map[int32][]int32{},
	}
	matches := svc.Match(snap)
	if len(matches) != 1 {
		t.Fatalf("matches = %d", len(matches))
	}
	if matches[0].Confidence < 100 {
		t.Fatalf("confidence = %d", matches[0].Confidence)
	}
}

func TestSafeToTerminateRejectsPIDReuse(t *testing.T) {
	start := time.Now().Add(-time.Hour)
	run := &Run{Root: platform.Process{PID: 10, Name: "agent", Username: "u", Create: start, StartedOK: true}}
	snap := &platform.Snapshot{
		Processes: map[int32]platform.Process{
			10: {PID: 10, Name: "agent", Username: "u", Create: start.Add(time.Minute), StartedOK: true},
		},
	}
	if safeToTerminate(snap, run) {
		t.Fatal("expected stale pid/start-time rejection")
	}
}

func TestMatchRejectsOpaqueProcessEvenWithParentEvidence(t *testing.T) {
	cfg := &config.Config{
		Version: 1,
		Mode:    config.ModeVisibility,
		Service: config.ServiceConfig{MinConfidence: 40},
		Defaults: config.Policy{
			WarnAfter: config.Duration{Duration: time.Minute},
			KillAfter: config.Duration{Duration: 2 * time.Minute},
		},
		Agents: []config.Agent{{
			ID:     "agent-child",
			Label:  "Agent Child",
			Family: "agent",
			Kind:   config.AgentKindProcess,
			Match: config.Match{
				ParentProcessNames: []string{"agent-parent"},
			},
		}},
	}
	start := time.Now().Add(-time.Minute)
	snap := &platform.Snapshot{
		At:       time.Now(),
		Platform: "test",
		Processes: map[int32]platform.Process{
			1: {PID: 1, Name: "agent-parent", Create: start, StartedOK: true},
			2: {PID: 2, PPID: 1, Create: start, StartedOK: true},
		},
		Children: map[int32][]int32{1: {2}},
	}
	if matches := New(cfg, nil).Match(snap); len(matches) != 0 {
		t.Fatalf("opaque child matched: %#v", matches)
	}
}

func TestVisibilityLifecycleWarningAckAndWouldTerminate(t *testing.T) {
	dir := t.TempDir()
	cfg := lifecycleConfig(dir, config.ModeVisibility)
	cfg.Service.ScanInterval = config.Duration{Duration: time.Hour}
	l, err := ledger.Open(cfg.Ledger.Path)
	if err != nil {
		t.Fatal(err)
	}

	start := time.Date(2026, 5, 18, 10, 0, 0, 0, time.UTC)
	now := start.Add(3 * time.Second)
	snap := syntheticSnapshot(start)
	service := New(cfg, l)
	service.capture = func(context.Context) (*platform.Snapshot, error) { return snap, nil }
	service.notify = func(string, string) error { return nil }
	service.newRunID = func() string { return "run_test" }
	service.now = func() time.Time { return now }

	if err := service.Scan(context.Background()); err != nil {
		t.Fatal(err)
	}
	events, err := ledger.Read(cfg.Ledger.Path)
	if err != nil {
		t.Fatal(err)
	}
	requireEvent(t, events, "run_started")
	requireEvent(t, events, "policy_warning")
	requireEvent(t, events, "would_terminate")

	if err := WriteAck(cfg.Service.StateDir, "run_test", "5s", "still watching"); err != nil {
		t.Fatal(err)
	}
	now = start.Add(4 * time.Second)
	if err := service.Scan(context.Background()); err != nil {
		t.Fatal(err)
	}
	events, err = ledger.Read(cfg.Ledger.Path)
	if err != nil {
		t.Fatal(err)
	}
	requireEvent(t, events, "ack_received")
}

func TestEnforcementUsesTerminationAfterGrace(t *testing.T) {
	dir := t.TempDir()
	cfg := lifecycleConfig(dir, config.ModeEnforcement)
	l, err := ledger.Open(cfg.Ledger.Path)
	if err != nil {
		t.Fatal(err)
	}
	start := time.Date(2026, 5, 18, 10, 0, 0, 0, time.UTC)
	now := start.Add(3 * time.Second)
	snap := syntheticSnapshot(start)
	terminated := false

	service := New(cfg, l)
	service.capture = func(context.Context) (*platform.Snapshot, error) { return snap, nil }
	service.notify = func(string, string) error { return nil }
	service.terminate = func(context.Context, platform.TerminationTarget, time.Duration) platform.TerminationResult {
		terminated = true
		return platform.TerminationResult{SoftSignaled: []int32{4242}}
	}
	service.newRunID = func() string { return "run_enforce" }
	service.now = func() time.Time { return now }

	if err := service.Scan(context.Background()); err != nil {
		t.Fatal(err)
	}
	if terminated {
		t.Fatal("terminated before grace elapsed")
	}
	now = start.Add(5 * time.Second)
	if err := service.Scan(context.Background()); err != nil {
		t.Fatal(err)
	}
	if !terminated {
		t.Fatal("expected termination after grace elapsed")
	}
	events, err := ledger.Read(cfg.Ledger.Path)
	if err != nil {
		t.Fatal(err)
	}
	requireEvent(t, events, "grace_started")
	requireEvent(t, events, "termination_completed")
}

func TestEnforcementDoesNotTerminateDesktopApps(t *testing.T) {
	dir := t.TempDir()
	cfg := lifecycleConfig(dir, config.ModeEnforcement)
	cfg.Agents[0].ID = "codex-desktop"
	cfg.Agents[0].Label = "Codex Desktop"
	cfg.Agents[0].Kind = config.AgentKindApp
	l, err := ledger.Open(cfg.Ledger.Path)
	if err != nil {
		t.Fatal(err)
	}
	start := time.Date(2026, 5, 18, 10, 0, 0, 0, time.UTC)
	now := start.Add(5 * time.Second)
	snap := syntheticSnapshot(start)
	terminated := false

	service := New(cfg, l)
	service.capture = func(context.Context) (*platform.Snapshot, error) { return snap, nil }
	service.notify = func(string, string) error { return nil }
	service.terminate = func(context.Context, platform.TerminationTarget, time.Duration) platform.TerminationResult {
		terminated = true
		return platform.TerminationResult{SoftSignaled: []int32{4242}}
	}
	service.newRunID = func() string { return "run_app" }
	service.now = func() time.Time { return now }

	if err := service.Scan(context.Background()); err != nil {
		t.Fatal(err)
	}
	if terminated {
		t.Fatal("desktop app was terminated")
	}
	events, err := ledger.Read(cfg.Ledger.Path)
	if err != nil {
		t.Fatal(err)
	}
	requireEvent(t, events, "watch_only")
}

func TestRunStopsWhenProcessDisappears(t *testing.T) {
	dir := t.TempDir()
	cfg := lifecycleConfig(dir, config.ModeVisibility)
	l, err := ledger.Open(cfg.Ledger.Path)
	if err != nil {
		t.Fatal(err)
	}
	start := time.Date(2026, 5, 18, 10, 0, 0, 0, time.UTC)
	now := start.Add(500 * time.Millisecond)
	snap := syntheticSnapshot(start)
	service := New(cfg, l)
	service.capture = func(context.Context) (*platform.Snapshot, error) { return snap, nil }
	service.notify = func(string, string) error { return nil }
	service.newRunID = func() string { return "run_stop" }
	service.now = func() time.Time { return now }
	if err := service.Scan(context.Background()); err != nil {
		t.Fatal(err)
	}
	snap = &platform.Snapshot{At: now, Platform: "test", Processes: map[int32]platform.Process{}, Children: map[int32][]int32{}}
	now = start.Add(time.Second)
	if err := service.Scan(context.Background()); err != nil {
		t.Fatal(err)
	}
	events, err := ledger.Read(cfg.Ledger.Path)
	if err != nil {
		t.Fatal(err)
	}
	requireEvent(t, events, "run_stopped")
}

func TestRunWritesServiceLifecycle(t *testing.T) {
	dir := t.TempDir()
	cfg := lifecycleConfig(dir, config.ModeVisibility)
	cfg.Service.ScanInterval = config.Duration{Duration: time.Hour}
	l, err := ledger.Open(cfg.Ledger.Path)
	if err != nil {
		t.Fatal(err)
	}
	ctx, cancel := context.WithCancel(context.Background())
	service := New(cfg, l)
	service.capture = func(context.Context) (*platform.Snapshot, error) {
		cancel()
		return &platform.Snapshot{At: time.Now(), Platform: "test", Processes: map[int32]platform.Process{}, Children: map[int32][]int32{}}, nil
	}
	if err := service.Run(ctx); err != nil {
		t.Fatal(err)
	}
	events, err := ledger.Read(cfg.Ledger.Path)
	if err != nil {
		t.Fatal(err)
	}
	requireEvent(t, events, "service_started")
	requireEvent(t, events, "service_stopped")
}

func TestAckRejectionWhenBudgetExhausted(t *testing.T) {
	dir := t.TempDir()
	cfg := lifecycleConfig(dir, config.ModeVisibility)
	l, err := ledger.Open(cfg.Ledger.Path)
	if err != nil {
		t.Fatal(err)
	}
	start := time.Date(2026, 5, 18, 10, 0, 0, 0, time.UTC)
	run := &Run{
		ID:         "run_ack",
		Agent:      cfg.Agents[0],
		Policy:     cfg.Defaults,
		Extensions: 1,
	}
	service := New(cfg, l)
	service.now = func() time.Time { return start }
	if err := WriteAck(cfg.Service.StateDir, "run_ack", "5s", "too many"); err != nil {
		t.Fatal(err)
	}
	service.applyAcks(run)
	events, err := ledger.Read(cfg.Ledger.Path)
	if err != nil {
		t.Fatal(err)
	}
	requireEvent(t, events, "ack_rejected")
}

func TestScoreExcludesAndPathMatches(t *testing.T) {
	snap := &platform.Snapshot{
		Processes: map[int32]platform.Process{
			1: {PID: 1, Name: "parent"},
			2: {PID: 2, PPID: 1, Name: "child", Exe: "/opt/Agent/bin/agent", Cmdline: "agent run"},
		},
	}
	match := config.Match{
		ExecutablePaths:    []string{"/opt/Agent"},
		ParentProcessNames: []string{"parent"},
		CommandRegex:       []string{"agent run"},
	}
	confidence, evidence := score(match, snap.Processes[2], snap)
	if confidence < 190 {
		t.Fatalf("confidence=%d evidence=%#v", confidence, evidence)
	}
	match.ExcludeNames = []string{"child"}
	confidence, _ = score(match, snap.Processes[2], snap)
	if confidence != 0 {
		t.Fatalf("excluded confidence=%d", confidence)
	}
}

func TestScoreRequiresCodexDesktopWorkerShape(t *testing.T) {
	match := config.Match{
		ProcessNames:        []string{"codex"},
		RequireCommandRegex: []string{"\\bapp-server\\b", "--listen\\s+stdio://"},
		CommandRegex:        []string{"\\bapp-server\\b", "--listen\\s+stdio://"},
	}
	snap := &platform.Snapshot{Processes: map[int32]platform.Process{}, Children: map[int32][]int32{}}

	worker := platform.Process{PID: 1, Name: "codex", Cmdline: "/Applications/Codex.app/Contents/Resources/codex app-server --listen stdio://"}
	confidence, _ := score(match, worker, snap)
	if confidence == 0 {
		t.Fatal("expected Codex stdio app-server worker match")
	}

	mainServer := platform.Process{PID: 2, Name: "codex", Cmdline: "/Applications/Codex.app/Contents/Resources/codex app-server --analytics-default-enabled"}
	confidence, _ = score(match, mainServer, snap)
	if confidence != 0 {
		t.Fatalf("main app server matched as worker: confidence=%d", confidence)
	}
}

func TestScoreAvoidsClaudeMentionFalsePositive(t *testing.T) {
	match := config.Match{
		ProcessNames:       []string{"claude", "claude-code"},
		CommandRegex:       []string{"(^|/|\\\\)claude(-code)?(\\.cmd|\\.exe)?(\\s|$)"},
		ExcludeParentRegex: []string{"/Applications/Codex\\.app/.+\\bapp-server\\b"},
	}
	snap := &platform.Snapshot{Processes: map[int32]platform.Process{}, Children: map[int32][]int32{}}

	agent := platform.Process{PID: 1, Name: "2.1.143", Cmdline: "claude --dangerously-skip-permissions"}
	confidence, _ := score(match, agent, snap)
	if confidence == 0 {
		t.Fatal("expected Claude Code command match")
	}

	observer := platform.Process{PID: 2, Name: "jq", Cmdline: `jq '.[] | select(.Agent.ID == "claude-code")'`}
	confidence, _ = score(match, observer, snap)
	if confidence != 0 {
		t.Fatalf("observer command matched as Claude Code: confidence=%d", confidence)
	}

	snap.Processes[10] = platform.Process{PID: 10, Name: "codex", Cmdline: "/Applications/Codex.app/Contents/Resources/codex app-server --analytics-default-enabled"}
	codexDispatched := platform.Process{PID: 11, PPID: 10, Name: "claude", Cmdline: "claude -p --dangerously-skip-permissions"}
	confidence, _ = score(match, codexDispatched, snap)
	if confidence != 0 {
		t.Fatalf("Codex-dispatched Claude process matched as standalone Claude Code: confidence=%d", confidence)
	}
}

func TestWriteAckValidationAndRunIDFormat(t *testing.T) {
	if err := WriteAck(t.TempDir(), "", "1s", ""); err == nil {
		t.Fatal("expected missing run id error")
	}
	if err := WriteAck(t.TempDir(), "run", "nope", ""); err == nil {
		t.Fatal("expected bad duration error")
	}
	if id := newRunID(); !strings.HasPrefix(id, "run_") {
		t.Fatalf("run id = %q", id)
	}
}

func TestCaptureSeesRealSubprocessWithoutMocks(t *testing.T) {
	cmd := exec.Command("sleep", "5")
	if err := cmd.Start(); err != nil {
		t.Skipf("sleep unavailable: %v", err)
	}
	defer func() {
		_ = cmd.Process.Kill()
		_, _ = cmd.Process.Wait()
	}()

	var snap *platform.Snapshot
	var err error
	for i := 0; i < 10; i++ {
		snap, err = platform.Capture(context.Background())
		if err != nil {
			t.Fatal(err)
		}
		if _, ok := snap.Processes[int32(cmd.Process.Pid)]; ok {
			return
		}
		time.Sleep(50 * time.Millisecond)
	}
	t.Fatalf("process %d not found in snapshot", cmd.Process.Pid)
}

func lifecycleConfig(dir string, mode config.Mode) *config.Config {
	return &config.Config{
		Version: 1,
		Mode:    mode,
		Service: config.ServiceConfig{
			MinConfidence:     50,
			HeartbeatInterval: config.Duration{Duration: time.Nanosecond},
			StateDir:          dir,
		},
		Defaults: config.Policy{
			WarnAfter:       config.Duration{Duration: time.Second},
			KillAfter:       config.Duration{Duration: 2 * time.Second},
			AckExtension:    config.Duration{Duration: 5 * time.Second},
			MaxExtensions:   1,
			KillGracePeriod: config.Duration{Duration: time.Second},
			MinLifetime:     config.Duration{Duration: time.Nanosecond},
			MaxRunGap:       config.Duration{Duration: time.Nanosecond},
		},
		Agents: []config.Agent{{
			ID:    "sleep",
			Label: "Sleep",
			Match: config.Match{ProcessNames: []string{"sleep"}},
		}},
		Ledger: config.LedgerConfig{Path: dir + "/runs.ndjson"},
	}
}

func syntheticSnapshot(start time.Time) *platform.Snapshot {
	return &platform.Snapshot{
		At:       start,
		Platform: "test",
		Processes: map[int32]platform.Process{
			4242: {
				PID:       4242,
				PPID:      1,
				Name:      "sleep",
				Exe:       "/bin/sleep",
				Cmdline:   "sleep 60",
				Username:  "tester",
				Create:    start,
				StartedOK: true,
			},
		},
		Children: map[int32][]int32{1: {4242}},
	}
}

func requireEvent(t *testing.T, events []ledger.Event, eventType string) {
	t.Helper()
	for _, event := range events {
		if event.Type == eventType {
			return
		}
	}
	t.Fatalf("missing event %s in %#v", eventType, events)
}
