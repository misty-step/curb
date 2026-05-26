package main

import (
	"bytes"
	"context"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net"
	"net/http"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	apipkg "github.com/phaedrus/curb/internal/api"
	"github.com/phaedrus/curb/internal/config"
	"github.com/phaedrus/curb/internal/ledger"
	"github.com/phaedrus/curb/internal/platform"
	servicepkg "github.com/phaedrus/curb/internal/service"
	"github.com/phaedrus/curb/internal/usagewatch"
)

func TestCLIValidateStatusRunsAndAck(t *testing.T) {
	dir := t.TempDir()
	configPath := writeTestConfig(t, dir)

	out, err := captureStdout(func() error {
		return run([]string{"curb", "validate-config", configPath})
	})
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(out, "ok config=") {
		t.Fatalf("validate output = %q", out)
	}

	l, err := ledger.Open(filepath.Join(dir, "runs.ndjson"))
	if err != nil {
		t.Fatal(err)
	}
	if err := l.Append(ledger.Event{Type: "run_started", RunID: "run_cli", AgentID: "sleep", Data: map[string]any{"pid": 1234}}); err != nil {
		t.Fatal(err)
	}

	out, err = captureStdout(func() error {
		return run([]string{"curb", "status", "--config", configPath})
	})
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(out, "active runs: 1") {
		t.Fatalf("status output = %q", out)
	}

	out, err = captureStdout(func() error {
		return run([]string{"curb", "runs", "--config", configPath})
	})
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(out, "run_cli") {
		t.Fatalf("runs output = %q", out)
	}

	out, err = captureStdout(func() error {
		return run([]string{"curb", "ack", "run_cli", "--config", configPath, "--extend", "1s", "--reason", "test"})
	})
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(out, "ack queued") {
		t.Fatalf("ack output = %q", out)
	}
	if _, err := os.Stat(filepath.Join(dir, "acks", "run_cli.json")); err != nil {
		t.Fatal(err)
	}
}

func TestCLIInitCreatesUsableConfig(t *testing.T) {
	dir := t.TempDir()
	configPath := filepath.Join(dir, "config.yaml")
	out, err := captureStdout(func() error {
		return run([]string{"curb", "init", "--config", configPath, "--mode", "alert"})
	})
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(out, "created config:") {
		t.Fatalf("init output = %q", out)
	}
	if _, err := os.Stat(configPath); err != nil {
		t.Fatal(err)
	}
	out, err = captureStdout(func() error {
		return run([]string{"curb", "validate-config", configPath})
	})
	if err != nil {
		t.Fatal(err)
	}
	if !strings.Contains(out, "mode=alert") {
		t.Fatalf("validate output = %q", out)
	}
}

func TestAggressivePresetKeepsOnlyProcessAgents(t *testing.T) {
	dir := t.TempDir()
	configPath := filepath.Join(dir, "config.yaml")
	if _, _, err := writeDefaultConfig(configPath, "alert", true); err != nil {
		t.Fatal(err)
	}
	content, err := os.ReadFile(configPath)
	if err != nil {
		t.Fatal(err)
	}
	withDesktop := strings.Replace(string(content), "agents:\n", `agents:
  - id: codex-desktop
    label: Codex Desktop
    family: codex
    match:
      bundle_ids: [com.openai.codex]
      process_names: [Codex]
`, 1)
	if err := os.WriteFile(configPath, []byte(withDesktop), 0o600); err != nil {
		t.Fatal(err)
	}

	out, err := captureStdout(func() error {
		return applyPreset(configPath, "aggressive")
	})
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(out, "Codex Desktop,") || strings.Contains(out, "agents: codex-desktop,") {
		t.Fatalf("desktop agent remained in preset output: %q", out)
	}
	if !strings.Contains(out, "codex-desktop-worker") || !strings.Contains(out, "codex-cli") || !strings.Contains(out, "claude-code") || !strings.Contains(out, "antigravity-cli") {
		t.Fatalf("process agents missing from preset output: %q", out)
	}
}

func TestCLIScanJSONUsesRealProcessTable(t *testing.T) {
	dir := t.TempDir()
	configPath := writeTestConfig(t, dir)
	out, err := captureStdout(func() error {
		return runWithDeps(
			[]string{"curb", "scan", "--config", configPath, "--json"},
			func(context.Context) (*platform.Snapshot, error) {
				now := time.Now().Add(-time.Minute)
				return &platform.Snapshot{
					At:       now,
					Platform: "test",
					Processes: map[int32]platform.Process{
						42: {
							PID:       42,
							Name:      "sleep",
							Exe:       "/bin/sleep",
							Cmdline:   "sleep 60 SECRET_TOKEN=do-not-print",
							CWD:       dir,
							Username:  "tester",
							Create:    now,
							StartedOK: true,
						},
					},
					Children: map[int32][]int32{},
				}, nil
			},
			func(string, string) error { return nil },
		)
	})
	if err != nil {
		t.Fatal(err)
	}
	if strings.TrimSpace(out) == "" {
		t.Fatal("expected JSON output")
	}
	if strings.Contains(out, "SECRET_TOKEN") || strings.Contains(out, "sleep 60") || !strings.Contains(out, "redacted") {
		t.Fatalf("scan JSON leaked command line: %q", out)
	}
}

func TestCLIScanTextShowsMatchEvidence(t *testing.T) {
	dir := t.TempDir()
	configPath := writeTestConfig(t, dir)
	now := time.Now().Add(-time.Minute)
	out, err := captureStdout(func() error {
		return runWithDeps(
			[]string{"curb", "scan", "--config", configPath},
			func(context.Context) (*platform.Snapshot, error) {
				return &platform.Snapshot{
					At:       now,
					Platform: "test",
					Processes: map[int32]platform.Process{
						42: {
							PID:       42,
							PPID:      1,
							Name:      "sleep",
							Exe:       "/bin/sleep",
							Cmdline:   "sleep 60",
							CWD:       dir,
							Username:  "tester",
							Create:    now,
							StartedOK: true,
						},
					},
					Children: map[int32][]int32{1: {42}},
				}, nil
			},
			func(string, string) error { return nil },
		)
	})
	if err != nil {
		t.Fatal(err)
	}
	for _, want := range []string{"sleep", "target=enforceable", "evidence:", "process_name:sleep", "pid:42", "started_at:", "user:tester", "cwd:" + dir} {
		if !strings.Contains(out, want) {
			t.Fatalf("scan output missing %q:\n%s", want, out)
		}
	}
}

func TestCLIUsageSurfacesConfigLoadErrors(t *testing.T) {
	err := run([]string{"curb", "usage", "--config", filepath.Join(t.TempDir(), "missing.yaml")})
	if err == nil || !strings.Contains(err.Error(), "missing.yaml") {
		t.Fatalf("usage config error = %v", err)
	}
}

func TestCLIDashboardJSONUsesUIReadModel(t *testing.T) {
	dir := t.TempDir()
	configPath := writeTestConfig(t, dir)
	now := time.Now().Add(-time.Minute)
	out, err := captureStdout(func() error {
		return runWithDeps(
			[]string{"curb", "dashboard", "--config", configPath, "--since", "0s", "--json"},
			func(context.Context) (*platform.Snapshot, error) {
				return &platform.Snapshot{
					At:       now,
					Platform: "test",
					Processes: map[int32]platform.Process{
						42: {PID: 42, Name: "sleep", CWD: dir, Create: now, StartedOK: true},
					},
					Children: map[int32][]int32{},
				}, nil
			},
			func(string, string) error { return nil },
		)
	})
	if err != nil {
		t.Fatal(err)
	}
	var decoded struct {
		Overview struct {
			Status       string `json:"status"`
			ActiveAgents int    `json:"active_agents"`
		} `json:"overview"`
		Agents []struct {
			ID    string `json:"id"`
			State string `json:"state"`
		} `json:"agents"`
	}
	if err := json.Unmarshal([]byte(out), &decoded); err != nil {
		t.Fatalf("invalid json %q: %v", out, err)
	}
	if decoded.Overview.Status != "OK" || decoded.Overview.ActiveAgents != 1 {
		t.Fatalf("overview = %#v", decoded.Overview)
	}
	if len(decoded.Agents) != 1 || decoded.Agents[0].ID != "sleep" || decoded.Agents[0].State != "running" {
		t.Fatalf("agents = %#v", decoded.Agents)
	}
}

func TestCLIDashboardJSONMatchesDaemonSnapshotForSameRuntimeTruth(t *testing.T) {
	dir := t.TempDir()
	configPath := writeRuntimeTruthConfig(t, dir)
	now := time.Now().UTC().Add(-10 * time.Second)
	repo := filepath.Join(dir, "repo")
	ghost := filepath.Join(dir, "ghost")
	home := writeUsageHome(t,
		codexRuntimeFixture{SessionID: "codex-one", CWD: repo, At: now, Total: 150},
		codexRuntimeFixture{SessionID: "codex-two", CWD: repo, At: now.Add(-time.Second), Total: 125},
	)
	writeClaudeRuntimeFixtures(t, home, []claudeRuntimeFixture{{SessionID: "claude-log", CWD: ghost, At: now, Total: 91}})
	t.Setenv("HOME", home)
	capture := func(context.Context) (*platform.Snapshot, error) {
		return &platform.Snapshot{
			At:       now,
			Platform: "test",
			Processes: map[int32]platform.Process{
				42: {PID: 42, Name: "codex", CWD: repo, Cmdline: "codex", Create: now.Add(-10 * time.Minute), StartedOK: true},
			},
			Children: map[int32][]int32{},
		}, nil
	}

	cliOut, err := captureStdout(func() error {
		return runWithDeps(
			[]string{"curb", "dashboard", "--config", configPath, "--since", "1h", "--json"},
			capture,
			func(string, string) error { return nil },
		)
	})
	if err != nil {
		t.Fatal(err)
	}
	var cli servicepkg.Snapshot
	if err := json.Unmarshal([]byte(cliOut), &cli); err != nil {
		t.Fatalf("cli json = %q: %v", cliOut, err)
	}

	opened := make(chan string, 1)
	t.Cleanup(setOpenBrowserForTest(func(url string) error {
		opened <- url
		return nil
	}))
	ctx, cancel := context.WithCancel(context.Background())
	errs := make(chan error, 1)
	go func() {
		errs <- serveDaemonContext(ctx, []string{"--config", configPath, "--addr", "127.0.0.1:0"}, capture, true)
	}()
	var baseURL string
	select {
	case baseURL = <-opened:
	case <-time.After(10 * time.Second):
		cancel()
		t.Fatal("dashboard was not opened")
	}
	dashboardRes, _ := getResponseWithRetry(t, baseURL)
	cookies := dashboardRes.Cookies()
	if len(cookies) == 0 {
		cancel()
		t.Fatal("dashboard did not set auth cookie")
	}
	req, err := http.NewRequest(http.MethodGet, baseURL+"v1/snapshot", nil)
	if err != nil {
		cancel()
		t.Fatal(err)
	}
	req.AddCookie(cookies[0])
	res, err := http.DefaultClient.Do(req)
	if err != nil {
		cancel()
		t.Fatal(err)
	}
	defer res.Body.Close()
	if res.StatusCode != http.StatusOK {
		cancel()
		t.Fatalf("snapshot status = %d", res.StatusCode)
	}
	var api servicepkg.Snapshot
	if err := json.NewDecoder(res.Body).Decode(&api); err != nil {
		cancel()
		t.Fatal(err)
	}
	cancel()
	select {
	case err := <-errs:
		if err != nil {
			t.Fatal(err)
		}
	case <-time.After(3 * time.Second):
		t.Fatal("server did not stop")
	}

	assertRuntimeTruthParity(t, cli, api)
}

func TestClassifyUsageUsesSharedSessionClassifier(t *testing.T) {
	now := time.Date(2026, 5, 26, 20, 0, 0, 0, time.UTC)
	cfg := testUsageConfig()
	view := classifyUsage([]usagewatch.Session{
		{Provider: "codex", SessionID: "stop", CWD: "/repo", Last: now, LastUsage: now, LastTurnTokens: 3_000, Total: 3_000},
		{Provider: "codex", SessionID: "warn", CWD: "/repo", Last: now, LastUsage: now, LastTurnTokens: 1_500, Total: 1_500},
		{Provider: "codex", SessionID: "idle-high", CWD: "/repo", Last: now.Add(-time.Hour), LastUsage: now.Add(-time.Hour), LastTurnTokens: 3_500, Total: 3_500},
	}, cfg, now)

	if view.Stop != 1 || view.Warn != 1 || view.IdleHigh != 1 || view.Active != 2 {
		t.Fatalf("usage counts = stop %d warn %d idle_high %d active %d", view.Stop, view.Warn, view.IdleHigh, view.Active)
	}
	got := []string{view.Rows[0].Status, view.Rows[1].Status, view.Rows[2].Status}
	want := []string{"uncorrelated/stop", "uncorrelated/warn", "idle-high/quiet-high"}
	if strings.Join(got, ",") != strings.Join(want, ",") {
		t.Fatalf("usage statuses = %#v, want %#v", got, want)
	}
	if view.Rows[0].Reason != "usage crossed threshold, but no live process matched; Curb will not stop anything" {
		t.Fatalf("reason = %q", view.Rows[0].Reason)
	}
}

func TestCLIDoctorUsesRealTempLedgerAndInjectedOSBoundary(t *testing.T) {
	dir := t.TempDir()
	configPath := writeTestConfig(t, dir)
	out, err := captureStdout(func() error {
		return runWithDeps(
			[]string{"curb", "doctor", "--config", configPath},
			func(context.Context) (*platform.Snapshot, error) {
				return &platform.Snapshot{Platform: "test", Processes: map[int32]platform.Process{1: {PID: 1}}}, nil
			},
			func(string, string) error { return errors.New("notifications disabled in test") },
		)
	})
	if err != nil {
		t.Fatal(err)
	}
	for _, want := range []string{"config: ok", "ledger: ok", "process_snapshot: ok", "notifications: unavailable"} {
		if !strings.Contains(out, want) {
			t.Fatalf("doctor output missing %q: %q", want, out)
		}
	}
}

func TestUnknownCommand(t *testing.T) {
	if err := run([]string{"curb", "missing"}); err == nil {
		t.Fatal("expected unknown command error")
	}
}

func TestAppRejectsNonLoopbackAddress(t *testing.T) {
	dir := t.TempDir()
	configPath := writeTestConfig(t, dir)
	err := runWithDeps(
		[]string{"curb", "app", "--config", configPath, "--addr", "0.0.0.0:8765"},
		func(context.Context) (*platform.Snapshot, error) {
			return &platform.Snapshot{Platform: "test", Processes: map[int32]platform.Process{}}, nil
		},
		func(string, string) error { return nil },
	)
	if err == nil || !strings.Contains(err.Error(), "loopback") {
		t.Fatalf("err = %v, want loopback rejection", err)
	}
}

func TestAppServesEmbeddedUIWithCookieAuthAndProtectedAPI(t *testing.T) {
	dir := t.TempDir()
	configPath := writeTestConfig(t, dir)
	opened := make(chan string, 1)
	t.Cleanup(setOpenBrowserForTest(func(url string) error {
		opened <- url
		return nil
	}))

	ctx, cancel := context.WithCancel(context.Background())
	errs := make(chan error, 1)
	go func() {
		errs <- serveDaemonContext(
			ctx,
			[]string{"--config", configPath, "--addr", "127.0.0.1:0"},
			func(context.Context) (*platform.Snapshot, error) {
				return &platform.Snapshot{Platform: "test", Processes: map[int32]platform.Process{}}, nil
			},
			true,
		)
	}()

	var baseURL string
	select {
	case baseURL = <-opened:
	case <-time.After(10 * time.Second):
		cancel()
		t.Fatal("browser was not opened")
	}

	dashboardRes, body := getResponseWithRetry(t, baseURL)
	if !strings.Contains(body, `<div id="root">`) {
		cancel()
		t.Fatalf("dashboard body = %q", body)
	}
	cookies := dashboardRes.Cookies()
	if len(cookies) != 1 || cookies[0].Name != "curb_token" || !cookies[0].HttpOnly || cookies[0].Path != "/v1/" {
		cancel()
		t.Fatalf("dashboard cookies = %#v", cookies)
	}
	res, err := http.Get(baseURL + "v1/health")
	if err != nil {
		cancel()
		t.Fatal(err)
	}
	_ = res.Body.Close()
	if res.StatusCode != http.StatusUnauthorized {
		cancel()
		t.Fatalf("api status = %d", res.StatusCode)
	}

	req, err := http.NewRequest(http.MethodGet, baseURL+"v1/snapshot", nil)
	if err != nil {
		cancel()
		t.Fatal(err)
	}
	req.AddCookie(cookies[0])
	res, err = http.DefaultClient.Do(req)
	if err != nil {
		cancel()
		t.Fatal(err)
	}
	_ = res.Body.Close()
	if res.StatusCode != http.StatusOK {
		cancel()
		t.Fatalf("cookie-authenticated snapshot status = %d", res.StatusCode)
	}

	cancel()
	select {
	case err := <-errs:
		if err != nil {
			t.Fatal(err)
		}
	case <-time.After(3 * time.Second):
		t.Fatal("server did not stop")
	}
}

func TestAppConnectsToExistingCompatibleDaemon(t *testing.T) {
	dir := t.TempDir()
	configPath := writeTestConfig(t, dir)
	if err := os.WriteFile(filepath.Join(dir, "api.token"), []byte("test-token\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	svc, err := servicepkg.New(configPath, func(context.Context) (*platform.Snapshot, error) {
		return &platform.Snapshot{Platform: "test", Processes: map[int32]platform.Process{}}, nil
	})
	if err != nil {
		t.Fatal(err)
	}
	apiServer, err := apipkg.New("test-token", svc)
	if err != nil {
		t.Fatal(err)
	}
	apiServer.ServeUI(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		_, _ = w.Write([]byte(`<div id="root"></div>`))
	}))
	server := &http.Server{Handler: apiServer.Handler()}
	done := make(chan error, 1)
	go func() {
		if err := server.Serve(listener); err != nil && !errors.Is(err, http.ErrServerClosed) {
			done <- err
			return
		}
		done <- nil
	}()
	t.Cleanup(func() {
		ctx, cancel := context.WithTimeout(context.Background(), time.Second)
		defer cancel()
		_ = server.Shutdown(ctx)
		if err := <-done; err != nil {
			t.Error(err)
		}
	})
	opened := make(chan string, 1)
	t.Cleanup(setOpenBrowserForTest(func(url string) error {
		opened <- url
		return nil
	}))
	localCaptures := 0

	out, err := captureStdout(func() error {
		return serveDaemonContext(
			context.Background(),
			[]string{"--config", configPath, "--addr", listener.Addr().String()},
			func(context.Context) (*platform.Snapshot, error) {
				localCaptures++
				return nil, errors.New("local capture ran")
			},
			true,
		)
	})
	if err != nil {
		t.Fatal(err)
	}
	if localCaptures != 0 {
		t.Fatalf("local captures = %d, want 0", localCaptures)
	}
	wantURL := "http://" + listener.Addr().String() + "/"
	if !strings.Contains(out, "connected: "+wantURL) {
		t.Fatalf("output = %q, want connected URL", out)
	}
	select {
	case got := <-opened:
		if got != wantURL {
			t.Fatalf("opened = %q, want %q", got, wantURL)
		}
	default:
		t.Fatal("browser was not opened")
	}
}

func TestAppDoesNotConnectToIncompatibleDaemon(t *testing.T) {
	dir := t.TempDir()
	configPath := writeTestConfig(t, dir)
	if err := os.WriteFile(filepath.Join(dir, "api.token"), []byte("test-token\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	mux := http.NewServeMux()
	mux.HandleFunc("/v1/health", func(w http.ResponseWriter, r *http.Request) {
		_, _ = w.Write([]byte(`{"ok":true}`))
	})
	server := &http.Server{Handler: mux}
	done := make(chan error, 1)
	go func() {
		if err := server.Serve(listener); err != nil && !errors.Is(err, http.ErrServerClosed) {
			done <- err
			return
		}
		done <- nil
	}()
	t.Cleanup(func() {
		ctx, cancel := context.WithTimeout(context.Background(), time.Second)
		defer cancel()
		_ = server.Shutdown(ctx)
		if err := <-done; err != nil {
			t.Error(err)
		}
	})
	t.Cleanup(setOpenBrowserForTest(func(url string) error {
		t.Fatalf("browser should not open incompatible daemon: %s", url)
		return nil
	}))

	err = serveDaemonContext(
		context.Background(),
		[]string{"--config", configPath, "--addr", listener.Addr().String()},
		func(context.Context) (*platform.Snapshot, error) {
			return &platform.Snapshot{Platform: "test", Processes: map[int32]platform.Process{}}, nil
		},
		true,
	)
	if err == nil || !strings.Contains(err.Error(), "no compatible curb daemon") {
		t.Fatalf("err = %v, want incompatible daemon error", err)
	}
}

func TestAppDoesNotConnectToUnauthenticatedHealthEndpoint(t *testing.T) {
	dir := t.TempDir()
	configPath := writeTestConfig(t, dir)
	if err := os.WriteFile(filepath.Join(dir, "api.token"), []byte("test-token\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	mux := http.NewServeMux()
	mux.HandleFunc("/v1/health", func(w http.ResponseWriter, r *http.Request) {
		_, _ = w.Write([]byte(`{"ok":true,"app":"curb","api_version":1}`))
	})
	server := &http.Server{Handler: mux}
	done := make(chan error, 1)
	go func() {
		if err := server.Serve(listener); err != nil && !errors.Is(err, http.ErrServerClosed) {
			done <- err
			return
		}
		done <- nil
	}()
	t.Cleanup(func() {
		ctx, cancel := context.WithTimeout(context.Background(), time.Second)
		defer cancel()
		_ = server.Shutdown(ctx)
		if err := <-done; err != nil {
			t.Error(err)
		}
	})
	t.Cleanup(setOpenBrowserForTest(func(url string) error {
		t.Fatalf("browser should not open unauthenticated daemon: %s", url)
		return nil
	}))

	err = serveDaemonContext(
		context.Background(),
		[]string{"--config", configPath, "--addr", listener.Addr().String()},
		func(context.Context) (*platform.Snapshot, error) {
			return &platform.Snapshot{Platform: "test", Processes: map[int32]platform.Process{}}, nil
		},
		true,
	)
	if err == nil || !strings.Contains(err.Error(), "no compatible curb daemon") {
		t.Fatalf("err = %v, want incompatible daemon error", err)
	}
}

func TestAppDoesNotConnectToDaemonWithoutDashboard(t *testing.T) {
	dir := t.TempDir()
	configPath := writeTestConfig(t, dir)
	if err := os.WriteFile(filepath.Join(dir, "api.token"), []byte("test-token\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	svc, err := servicepkg.New(configPath, func(context.Context) (*platform.Snapshot, error) {
		return &platform.Snapshot{Platform: "test", Processes: map[int32]platform.Process{}}, nil
	})
	if err != nil {
		t.Fatal(err)
	}
	apiServer, err := apipkg.New("test-token", svc)
	if err != nil {
		t.Fatal(err)
	}
	server := &http.Server{Handler: apiServer.Handler()}
	done := make(chan error, 1)
	go func() {
		if err := server.Serve(listener); err != nil && !errors.Is(err, http.ErrServerClosed) {
			done <- err
			return
		}
		done <- nil
	}()
	t.Cleanup(func() {
		ctx, cancel := context.WithTimeout(context.Background(), time.Second)
		defer cancel()
		_ = server.Shutdown(ctx)
		if err := <-done; err != nil {
			t.Error(err)
		}
	})
	t.Cleanup(setOpenBrowserForTest(func(url string) error {
		t.Fatalf("browser should not open daemon without dashboard: %s", url)
		return nil
	}))

	err = serveDaemonContext(
		context.Background(),
		[]string{"--config", configPath, "--addr", listener.Addr().String()},
		func(context.Context) (*platform.Snapshot, error) {
			return &platform.Snapshot{Platform: "test", Processes: map[int32]platform.Process{}}, nil
		},
		true,
	)
	if err == nil || !strings.Contains(err.Error(), "no compatible curb daemon") {
		t.Fatalf("err = %v, want incompatible daemon error", err)
	}
}

func TestAppDoesNotConnectToGenericRootHandler(t *testing.T) {
	dir := t.TempDir()
	configPath := writeTestConfig(t, dir)
	if err := os.WriteFile(filepath.Join(dir, "api.token"), []byte("test-token\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	mux := http.NewServeMux()
	mux.HandleFunc("/v1/health", func(w http.ResponseWriter, r *http.Request) {
		if r.Header.Get("Authorization") != "Bearer test-token" {
			w.WriteHeader(http.StatusUnauthorized)
			return
		}
		_, _ = w.Write([]byte(`{"ok":true,"app":"curb","api_version":1}`))
	})
	mux.HandleFunc("/", func(w http.ResponseWriter, r *http.Request) {
		_, _ = w.Write([]byte(`<html><body>not curb</body></html>`))
	})
	server := &http.Server{Handler: mux}
	done := make(chan error, 1)
	go func() {
		if err := server.Serve(listener); err != nil && !errors.Is(err, http.ErrServerClosed) {
			done <- err
			return
		}
		done <- nil
	}()
	t.Cleanup(func() {
		ctx, cancel := context.WithTimeout(context.Background(), time.Second)
		defer cancel()
		_ = server.Shutdown(ctx)
		if err := <-done; err != nil {
			t.Error(err)
		}
	})
	t.Cleanup(setOpenBrowserForTest(func(url string) error {
		t.Fatalf("browser should not open generic root handler: %s", url)
		return nil
	}))

	err = serveDaemonContext(
		context.Background(),
		[]string{"--config", configPath, "--addr", listener.Addr().String()},
		func(context.Context) (*platform.Snapshot, error) {
			return &platform.Snapshot{Platform: "test", Processes: map[int32]platform.Process{}}, nil
		},
		true,
	)
	if err == nil || !strings.Contains(err.Error(), "no compatible curb daemon") {
		t.Fatalf("err = %v, want incompatible daemon error", err)
	}
}

func TestAppDoesNotFollowDashboardRedirect(t *testing.T) {
	dir := t.TempDir()
	configPath := writeTestConfig(t, dir)
	if err := os.WriteFile(filepath.Join(dir, "api.token"), []byte("test-token\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	listener, err := net.Listen("tcp", "127.0.0.1:0")
	if err != nil {
		t.Fatal(err)
	}
	mux := http.NewServeMux()
	mux.HandleFunc("/v1/health", func(w http.ResponseWriter, r *http.Request) {
		if r.Header.Get("Authorization") != "Bearer test-token" {
			w.WriteHeader(http.StatusUnauthorized)
			return
		}
		_, _ = w.Write([]byte(`{"ok":true,"app":"curb","api_version":1}`))
	})
	mux.HandleFunc("/", func(w http.ResponseWriter, r *http.Request) {
		http.Redirect(w, r, "/elsewhere", http.StatusFound)
	})
	mux.HandleFunc("/elsewhere", func(w http.ResponseWriter, r *http.Request) {
		_, _ = w.Write([]byte(`<div id="root"></div>`))
	})
	server := &http.Server{Handler: mux}
	done := make(chan error, 1)
	go func() {
		if err := server.Serve(listener); err != nil && !errors.Is(err, http.ErrServerClosed) {
			done <- err
			return
		}
		done <- nil
	}()
	t.Cleanup(func() {
		ctx, cancel := context.WithTimeout(context.Background(), time.Second)
		defer cancel()
		_ = server.Shutdown(ctx)
		if err := <-done; err != nil {
			t.Error(err)
		}
	})
	t.Cleanup(setOpenBrowserForTest(func(url string) error {
		t.Fatalf("browser should not open redirected dashboard: %s", url)
		return nil
	}))

	err = serveDaemonContext(
		context.Background(),
		[]string{"--config", configPath, "--addr", listener.Addr().String()},
		func(context.Context) (*platform.Snapshot, error) {
			return &platform.Snapshot{Platform: "test", Processes: map[int32]platform.Process{}}, nil
		},
		true,
	)
	if err == nil || !strings.Contains(err.Error(), "no compatible curb daemon") {
		t.Fatalf("err = %v, want incompatible daemon error", err)
	}
}

func getWithRetry(t *testing.T, url string) string {
	t.Helper()
	_, body := getResponseWithRetry(t, url)
	return body
}

func getResponseWithRetry(t *testing.T, url string) (*http.Response, string) {
	t.Helper()
	var lastErr error
	for i := 0; i < 20; i++ {
		res, err := http.Get(url)
		if err == nil {
			body, readErr := io.ReadAll(res.Body)
			_ = res.Body.Close()
			if readErr != nil {
				t.Fatal(readErr)
			}
			if res.StatusCode == http.StatusOK {
				return res, string(body)
			}
			lastErr = fmt.Errorf("status %d: %s", res.StatusCode, body)
		} else {
			lastErr = err
		}
		time.Sleep(25 * time.Millisecond)
	}
	t.Fatalf("GET %s failed: %v", url, lastErr)
	return nil, ""
}

func TestSummarizeRunsEndsCompletedRuns(t *testing.T) {
	start := time.Now().Add(-time.Minute)
	events := []ledger.Event{
		{Type: "run_started", RunID: "run_1", AgentID: "sleep", Time: start},
		{Type: "run_stopped", RunID: "run_1", AgentID: "sleep", Time: start.Add(time.Second)},
	}
	runs := summarizeRuns(events)
	if len(runs) != 1 {
		t.Fatalf("runs = %d", len(runs))
	}
	if !runs[0].Ended {
		t.Fatal("expected ended run")
	}
}

func TestActiveRunsCompactsDuplicateWatcherStarts(t *testing.T) {
	start := time.Now().Add(-time.Minute)
	events := []ledger.Event{
		{Type: "run_started", RunID: "old", AgentID: "codex", Time: start, Data: map[string]any{"pid": 42}},
		{Type: "run_started", RunID: "new", AgentID: "codex", Time: start.Add(time.Second), Data: map[string]any{"pid": 42}},
	}
	active := activeRuns(events)
	if len(active) != 1 {
		t.Fatalf("active len = %d", len(active))
	}
	if _, ok := active["new"]; !ok {
		t.Fatalf("expected newest run, got %#v", active)
	}
}

func writeTestConfig(t *testing.T, dir string) string {
	t.Helper()
	path := filepath.Join(dir, "curb.yaml")
	content := `version: 1
mode: visibility
service:
  state_dir: ` + dir + `
  min_confidence: 50
usage:
  enabled: false
  scan_interval: 1s
  lookback: 1h
  window: 1m
  warn_turn_tokens: 1000
  kill_turn_tokens: 3000
  grace_period: 1s
defaults:
  warn_after: 1m
  kill_after: 2m
  max_extensions: 1
agents:
  - id: sleep
    label: Sleep
    match:
      process_names: [sleep]
ledger:
  path: ` + filepath.Join(dir, "runs.ndjson") + `
  include_prompt_content: false
`
	if err := os.WriteFile(path, []byte(content), 0o600); err != nil {
		t.Fatal(err)
	}
	return path
}

func writeRuntimeTruthConfig(t *testing.T, dir string) string {
	t.Helper()
	path := filepath.Join(dir, "runtime-truth.yaml")
	content := `version: 1
mode: alert
service:
  state_dir: ` + dir + `
  min_confidence: 50
usage:
  enabled: true
  scan_interval: 1s
  lookback: 1h
  window: 15m
  warn_turn_tokens: 1000
  kill_turn_tokens: 3000
  grace_period: 1s
defaults:
  warn_after: 1m
  kill_after: 2m
  ack_extension: 30m
  max_extensions: 1
agents:
  - id: codex-test
    label: Codex Test
    family: codex
    match:
      process_names: [codex]
  - id: claude-test
    label: Claude Test
    family: claude
    match:
      process_names: [claude]
ledger:
  path: ` + filepath.Join(dir, "runs.ndjson") + `
  include_prompt_content: false
`
	if err := os.WriteFile(path, []byte(content), 0o600); err != nil {
		t.Fatal(err)
	}
	return path
}

type codexRuntimeFixture struct {
	SessionID string
	CWD       string
	At        time.Time
	Total     int64
}

type claudeRuntimeFixture struct {
	SessionID string
	CWD       string
	At        time.Time
	Total     int64
}

func writeUsageHome(t *testing.T, codexFixtures ...codexRuntimeFixture) string {
	t.Helper()
	home := t.TempDir()
	writeCodexRuntimeFixtures(t, home, codexFixtures)
	return home
}

func writeCodexRuntimeFixtures(t *testing.T, home string, fixtures []codexRuntimeFixture) {
	t.Helper()
	dir := filepath.Join(home, ".codex", "archived_sessions")
	if err := os.MkdirAll(dir, 0o700); err != nil {
		t.Fatal(err)
	}
	for _, fixture := range fixtures {
		content := `{"timestamp":"` + fixture.At.Format(time.RFC3339Nano) + `","type":"session_meta","payload":{"id":"` + fixture.SessionID + `","cwd":"` + fixture.CWD + `"}}
{"timestamp":"` + fixture.At.Format(time.RFC3339Nano) + `","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":100,"cached_input_tokens":0,"output_tokens":50,"total_tokens":` + fmt.Sprintf("%d", fixture.Total) + `},"total_token_usage":{"total_tokens":` + fmt.Sprintf("%d", fixture.Total) + `}}}}
`
		if err := os.WriteFile(filepath.Join(dir, fixture.SessionID+".jsonl"), []byte(content), 0o600); err != nil {
			t.Fatal(err)
		}
	}
}

func writeClaudeRuntimeFixtures(t *testing.T, home string, fixtures []claudeRuntimeFixture) {
	t.Helper()
	for _, fixture := range fixtures {
		dir := filepath.Join(home, ".claude", "projects", "-"+filepath.Base(fixture.CWD))
		if err := os.MkdirAll(dir, 0o700); err != nil {
			t.Fatal(err)
		}
		content := `{"timestamp":"` + fixture.At.Format(time.RFC3339Nano) + `","requestId":"req_` + fixture.SessionID + `","sessionId":"` + fixture.SessionID + `","uuid":"turn_` + fixture.SessionID + `","cwd":"` + fixture.CWD + `","message":{"id":"msg_` + fixture.SessionID + `","model":"claude-sonnet-4-5","usage":{"input_tokens":40,"cache_creation_input_tokens":0,"cache_read_input_tokens":0,"output_tokens":` + fmt.Sprintf("%d", fixture.Total-40) + `}}}
`
		if err := os.WriteFile(filepath.Join(dir, fixture.SessionID+".jsonl"), []byte(content), 0o600); err != nil {
			t.Fatal(err)
		}
	}
}

func assertRuntimeTruthParity(t *testing.T, cli, api servicepkg.Snapshot) {
	t.Helper()
	if cli.Overview.ActiveAgents != api.Overview.ActiveAgents || cli.Overview.ActiveSessions != api.Overview.ActiveSessions || cli.Overview.WarningSessions != api.Overview.WarningSessions || cli.Overview.StopSessions != api.Overview.StopSessions {
		t.Fatalf("overview parity failed: cli=%#v api=%#v", cli.Overview, api.Overview)
	}
	if len(cli.Agents) != 1 || len(api.Agents) != 1 {
		t.Fatalf("agents: cli=%#v api=%#v", cli.Agents, api.Agents)
	}
	for _, snapshot := range []struct {
		name string
		view servicepkg.Snapshot
	}{{"cli", cli}, {"api", api}} {
		if snapshot.view.Agents[0].Provider != "codex" || snapshot.view.Agents[0].State != "spending" || snapshot.view.Agents[0].PID != 42 {
			t.Fatalf("%s agent = %#v", snapshot.name, snapshot.view.Agents[0])
		}
		if len(snapshot.view.Sessions) != 3 {
			t.Fatalf("%s sessions = %#v", snapshot.name, snapshot.view.Sessions)
		}
		codexCorrelated := 0
		claudeLiveRows := 0
		for _, session := range snapshot.view.Sessions {
			if session.Provider == "codex" && session.CorrelatedPID == 42 {
				codexCorrelated++
			}
			if session.Provider == "claude" && session.CorrelatedPID != 0 {
				claudeLiveRows++
			}
		}
		if codexCorrelated != 2 || claudeLiveRows != 0 {
			t.Fatalf("%s correlation counts: codex=%d claude_live=%d sessions=%#v", snapshot.name, codexCorrelated, claudeLiveRows, snapshot.view.Sessions)
		}
	}
}

func testUsageConfig() *config.Config {
	cfg := &config.Config{Version: 1}
	if err := cfg.SetDefaults(); err != nil {
		panic(err)
	}
	cfg.Mode = config.ModeAlert
	cfg.Usage.Window.Duration = 15 * time.Minute
	cfg.Usage.WarnTurnTokens = 1_000
	cfg.Usage.KillTurnTokens = 3_000
	return cfg
}

func captureStdout(fn func() error) (string, error) {
	old := os.Stdout
	reader, writer, err := os.Pipe()
	if err != nil {
		return "", err
	}
	os.Stdout = writer
	err = fn()
	_ = writer.Close()
	os.Stdout = old
	var buf bytes.Buffer
	_, copyErr := io.Copy(&buf, reader)
	if err != nil {
		return buf.String(), err
	}
	return buf.String(), copyErr
}
