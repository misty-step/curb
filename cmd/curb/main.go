package main

import (
	"bufio"
	"context"
	"encoding/json"
	"errors"
	"flag"
	"fmt"
	"io"
	"net"
	"net/http"
	"os"
	"os/exec"
	"os/signal"
	"path/filepath"
	"runtime"
	"sort"
	"strings"
	"sync"
	"syscall"
	"time"

	apipkg "github.com/phaedrus/curb/internal/api"
	"github.com/phaedrus/curb/internal/config"
	"github.com/phaedrus/curb/internal/ledger"
	"github.com/phaedrus/curb/internal/platform"
	servicepkg "github.com/phaedrus/curb/internal/service"
	usagepkg "github.com/phaedrus/curb/internal/usage"
	"github.com/phaedrus/curb/internal/usagewatch"
	"github.com/phaedrus/curb/internal/watchdog"
	"github.com/phaedrus/curb/internal/web"
)

func main() {
	if err := run(os.Args); err != nil {
		fmt.Fprintln(os.Stderr, "curb:", err)
		os.Exit(1)
	}
}

func run(args []string) error {
	return runWithDeps(args, platform.Capture, platform.Notify)
}

func runWithDeps(args []string, capture processCapture, notify notifier) error {
	if len(args) < 2 {
		if _, err := ensureDefaultConfig(false); err != nil {
			return err
		}
		return cmdWatch(nil)
	}
	switch args[1] {
	case "help", "-h", "--help":
		if len(args) > 2 && args[2] == "advanced" {
			usageAdvanced()
			return nil
		}
		usage()
		return nil
	case "init":
		return cmdInit(args[2:])
	case "install":
		return cmdInstall(args[2:])
	case "config":
		return cmdConfig(args[2:])
	case "usage":
		return cmdUsage(args[2:])
	case "dashboard", "dash":
		return cmdDashboard(args[2:], capture)
	case "daemon", "api", "serve":
		return cmdDaemon(args[2:], capture)
	case "app":
		return cmdApp(args[2:], capture)
	case "tail":
		return cmdTail(args[2:])
	case "curb", "run", "start", "watch":
		return cmdWatch(args[2:])
	case "scan":
		return cmdScan(args[2:], capture)
	case "validate-config":
		return cmdValidate(args[2:])
	case "status":
		return cmdStatus(args[2:])
	case "runs":
		return cmdRuns(args[2:])
	case "ack":
		return cmdAck(args[2:])
	case "doctor":
		return cmdDoctor(args[2:], capture, notify)
	default:
		usage()
		return fmt.Errorf("unknown command %q", args[1])
	}
}

func cmdDashboard(args []string, capture processCapture) error {
	fs := flag.NewFlagSet("dashboard", flag.ExitOnError)
	configPath := fs.String("config", defaultConfigPath(), "config file")
	sinceRaw := fs.String("since", "24h", "usage lookback window")
	limit := fs.Int("limit", 10, "maximum sessions to print")
	jsonOut := fs.Bool("json", false, "print UI read model JSON")
	if err := fs.Parse(args); err != nil {
		return err
	}
	cfg, err := config.Load(*configPath)
	if err != nil {
		return err
	}
	sinceDuration, err := time.ParseDuration(*sinceRaw)
	if err != nil {
		return err
	}
	events, sources, err := usagepkg.EventsSince(time.Now().Add(-sinceDuration))
	if err != nil {
		return err
	}
	report := usagepkg.Report{GeneratedAt: time.Now().UTC(), Sources: sources, Sessions: usagepkg.Summarize(events)}
	snap, err := capture(context.Background())
	if err != nil {
		return err
	}
	snapshot := servicepkg.BuildSnapshot(cfg, snap, events, sources, report.GeneratedAt)
	if *jsonOut {
		return json.NewEncoder(os.Stdout).Encode(snapshot)
	}
	printDashboard(*configPath, cfg, snapshot, *limit)
	return nil
}

func cmdDaemon(args []string, capture processCapture) error {
	return serveDaemon(args, capture, false)
}

func cmdApp(args []string, capture processCapture) error {
	return serveDaemon(args, capture, true)
}

func serveDaemon(args []string, capture processCapture, openDashboard bool) error {
	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()
	return serveDaemonContext(ctx, args, capture, openDashboard)
}

func serveDaemonContext(ctx context.Context, args []string, capture processCapture, openDashboard bool) error {
	fs := flag.NewFlagSet("daemon", flag.ExitOnError)
	configPath := fs.String("config", defaultConfigPath(), "config file")
	addr := fs.String("addr", "127.0.0.1:8765", "loopback listen address")
	if err := fs.Parse(args); err != nil {
		return err
	}
	if *configPath == defaultConfigPath() {
		path, err := ensureDefaultConfig(false)
		if err != nil {
			return err
		}
		*configPath = path
	}
	host, _, err := net.SplitHostPort(*addr)
	if err != nil {
		return err
	}
	if host != "127.0.0.1" && host != "localhost" && host != "::1" {
		return fmt.Errorf("daemon API must bind to loopback, got %q", *addr)
	}
	cfg, err := config.Load(*configPath)
	if err != nil {
		return err
	}
	token, tokenPath, err := apipkg.LoadOrCreateToken(cfg.Service.StateDir)
	if err != nil {
		return err
	}
	listener, err := net.Listen("tcp", *addr)
	if err != nil {
		if openDashboard && addressInUse(err) {
			return openExistingDaemon(ctx, *addr, token)
		}
		return err
	}
	defer listener.Close()
	service, err := servicepkg.New(*configPath, servicepkg.CaptureFunc(capture))
	if err != nil {
		return err
	}
	if err := service.Refresh(context.Background()); err != nil {
		return err
	}
	go service.Start(ctx)

	server, err := apipkg.New(token, service)
	if err != nil {
		return err
	}
	ui, err := web.Handler()
	if err != nil {
		return err
	}
	server.ServeUI(ui)
	httpServer := &http.Server{Handler: server.Handler()}
	go func() {
		<-ctx.Done()
		shutdownCtx, cancel := context.WithTimeout(context.Background(), 2*time.Second)
		defer cancel()
		_ = httpServer.Shutdown(shutdownCtx)
	}()
	fmt.Println("curb daemon")
	fmt.Printf("  listening: http://%s\n", listener.Addr().String())
	fmt.Printf("  dashboard: http://%s/\n", listener.Addr().String())
	if openDashboard {
		fmt.Println("  auth: same-origin browser cookie")
		fmt.Printf("  token: %s (advanced clients only)\n", tokenPath)
	} else {
		fmt.Printf("  token: %s\n", tokenPath)
		fmt.Println("  auth: Authorization: Bearer $(cat token-file)")
	}
	serveErr := make(chan error, 1)
	go func() {
		if err := httpServer.Serve(listener); err != nil && !errors.Is(err, http.ErrServerClosed) {
			serveErr <- err
			return
		}
		serveErr <- nil
	}()
	if openDashboard {
		if err := callOpenBrowser("http://" + listener.Addr().String() + "/"); err != nil {
			fmt.Printf("  open: %v\n", err)
		}
	}
	return <-serveErr
}

func openExistingDaemon(ctx context.Context, addr string, token string) error {
	baseURL := "http://" + addr + "/"
	if err := existingDaemonHealth(ctx, baseURL, token); err != nil {
		return fmt.Errorf("address %s is already in use, but no compatible curb daemon answered: %w", addr, err)
	}
	fmt.Println("curb app")
	fmt.Printf("  connected: %s\n", baseURL)
	if err := callOpenBrowser(baseURL); err != nil {
		return fmt.Errorf("open dashboard: %w", err)
	}
	return nil
}

func existingDaemonHealth(ctx context.Context, baseURL string, token string) error {
	checkCtx, cancel := context.WithTimeout(ctx, time.Second)
	defer cancel()
	healthURL := strings.TrimRight(baseURL, "/") + "/v1/health"
	req, err := http.NewRequestWithContext(checkCtx, http.MethodGet, healthURL, nil)
	if err != nil {
		return err
	}
	req.Header.Set("Authorization", "Bearer "+token)
	res, err := http.DefaultClient.Do(req)
	if err != nil {
		return err
	}
	defer res.Body.Close()
	if res.StatusCode != http.StatusOK {
		return fmt.Errorf("health status %d", res.StatusCode)
	}
	var health struct {
		OK         bool   `json:"ok"`
		App        string `json:"app"`
		APIVersion int    `json:"api_version"`
	}
	if err := json.NewDecoder(res.Body).Decode(&health); err != nil {
		return err
	}
	if !health.OK || health.App != "curb" || health.APIVersion != 1 {
		return fmt.Errorf("not a curb daemon")
	}
	req, err = http.NewRequestWithContext(checkCtx, http.MethodGet, healthURL, nil)
	if err != nil {
		return err
	}
	res, err = http.DefaultClient.Do(req)
	if err != nil {
		return err
	}
	defer res.Body.Close()
	if res.StatusCode != http.StatusUnauthorized && res.StatusCode != http.StatusForbidden {
		return fmt.Errorf("health endpoint does not require curb token")
	}
	req, err = http.NewRequestWithContext(checkCtx, http.MethodGet, baseURL, nil)
	if err != nil {
		return err
	}
	dashboardClient := &http.Client{
		CheckRedirect: func(*http.Request, []*http.Request) error {
			return http.ErrUseLastResponse
		},
	}
	res, err = dashboardClient.Do(req)
	if err != nil {
		return err
	}
	defer res.Body.Close()
	if res.StatusCode != http.StatusOK {
		return fmt.Errorf("dashboard status %d", res.StatusCode)
	}
	body, err := io.ReadAll(io.LimitReader(res.Body, 128*1024))
	if err != nil {
		return err
	}
	if !strings.Contains(string(body), `id="root"`) {
		return fmt.Errorf("dashboard shell marker missing")
	}
	return nil
}

func addressInUse(err error) bool {
	return errors.Is(err, syscall.EADDRINUSE) || strings.Contains(strings.ToLower(err.Error()), "address already in use")
}

var (
	openBrowserMu sync.RWMutex
	openBrowser   = defaultOpenBrowser
)

func callOpenBrowser(url string) error {
	openBrowserMu.RLock()
	fn := openBrowser
	openBrowserMu.RUnlock()
	return fn(url)
}

func setOpenBrowserForTest(fn func(string) error) func() {
	openBrowserMu.Lock()
	old := openBrowser
	openBrowser = fn
	openBrowserMu.Unlock()
	return func() {
		openBrowserMu.Lock()
		openBrowser = old
		openBrowserMu.Unlock()
	}
}

func defaultOpenBrowser(url string) error {
	var cmd *exec.Cmd
	switch runtime.GOOS {
	case "darwin":
		cmd = exec.Command("open", url)
	case "windows":
		cmd = exec.Command("rundll32", "url.dll,FileProtocolHandler", url)
	default:
		cmd = exec.Command("xdg-open", url)
	}
	return cmd.Start()
}

func cmdTail(args []string) error {
	fs := flag.NewFlagSet("tail", flag.ExitOnError)
	sinceRaw := fs.String("since", "5m", "initial lookback window")
	intervalRaw := fs.String("interval", "2s", "poll interval")
	if err := fs.Parse(args); err != nil {
		return err
	}
	sinceDuration, err := time.ParseDuration(*sinceRaw)
	if err != nil {
		return err
	}
	interval, err := time.ParseDuration(*intervalRaw)
	if err != nil {
		return err
	}
	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()
	seen := map[string]bool{}
	since := time.Now().Add(-sinceDuration)
	fmt.Println("curb tail")
	fmt.Printf("  watching usage events every %s; Ctrl-C to stop\n\n", interval.Round(time.Second))
	ticker := time.NewTicker(interval)
	defer ticker.Stop()
	for {
		if err := printNewUsageEvents(since, seen); err != nil {
			fmt.Fprintf(os.Stderr, "curb: tail: %v\n", err)
		}
		since = time.Now().Add(-sinceDuration)
		select {
		case <-ctx.Done():
			fmt.Println("\ncurb tail stopped")
			return nil
		case <-ticker.C:
		}
	}
}

func cmdUsage(args []string) error {
	fs := flag.NewFlagSet("usage", flag.ExitOnError)
	configPath := fs.String("config", defaultConfigPath(), "config file")
	jsonOut := fs.Bool("json", false, "print JSON")
	limit := fs.Int("limit", 12, "maximum sessions to print")
	sinceRaw := fs.String("since", "168h", "lookback window")
	all := fs.Bool("all", false, "scan all known local logs")
	if err := fs.Parse(args); err != nil {
		return err
	}
	var since time.Time
	if !*all {
		sinceDuration, err := time.ParseDuration(*sinceRaw)
		if err != nil {
			return err
		}
		since = time.Now().Add(-sinceDuration)
	}
	cfg, err := config.Load(*configPath)
	if err != nil {
		return err
	}
	events, sources, err := usagepkg.EventsSince(since)
	report := usagepkg.Report{GeneratedAt: time.Now().UTC(), Sources: sources, Sessions: usagepkg.Summarize(events)}
	if *jsonOut {
		encodeErr := json.NewEncoder(os.Stdout).Encode(report)
		if encodeErr != nil {
			return encodeErr
		}
		return err
	}
	printUsageReport(report, usageSessions(events), cfg, *limit)
	return err
}

func cmdInstall(args []string) error {
	fs := flag.NewFlagSet("install", flag.ExitOnError)
	prefix := fs.String("prefix", filepath.Join(os.Getenv("HOME"), ".local"), "install prefix")
	if err := fs.Parse(args); err != nil {
		return err
	}
	exe, err := os.Executable()
	if err != nil {
		return err
	}
	destDir := filepath.Join(*prefix, "bin")
	if err := os.MkdirAll(destDir, 0o755); err != nil {
		return err
	}
	dest := filepath.Join(destDir, "curb")
	content, err := os.ReadFile(exe)
	if err != nil {
		return err
	}
	if err := os.WriteFile(dest, content, 0o755); err != nil {
		return err
	}
	fmt.Printf("installed: %s\n", dest)
	fmt.Printf("next: add %s to PATH if needed\n", destDir)
	return nil
}

type processCapture func(context.Context) (*platform.Snapshot, error)
type notifier func(string, string) error

func cmdInit(args []string) error {
	fs := flag.NewFlagSet("init", flag.ExitOnError)
	configPath := fs.String("config", defaultConfigPath(), "config file to create")
	force := fs.Bool("force", false, "overwrite an existing config")
	mode := fs.String("mode", "visibility", "initial mode: visibility, alert, or enforcement")
	if err := fs.Parse(args); err != nil {
		return err
	}
	path, created, err := writeDefaultConfig(*configPath, *mode, *force)
	if err != nil {
		return err
	}
	if created {
		fmt.Printf("created config: %s\n", path)
	} else {
		fmt.Printf("config already exists: %s\n", path)
	}
	fmt.Println("next: curb")
	return nil
}

func cmdWatch(args []string) error {
	fs := flag.NewFlagSet("watch", flag.ExitOnError)
	configPath := fs.String("config", defaultConfigPath(), "config file")
	if err := fs.Parse(args); err != nil {
		return err
	}
	if *configPath == defaultConfigPath() {
		path, err := ensureDefaultConfig(false)
		if err != nil {
			return err
		}
		*configPath = path
	}
	cfg, err := config.Load(*configPath)
	if err != nil {
		return err
	}
	ctx, stop := signal.NotifyContext(context.Background(), os.Interrupt, syscall.SIGTERM)
	defer stop()
	printWatchBanner(*configPath, cfg)
	service, err := servicepkg.New(*configPath, nil)
	if err != nil {
		return err
	}
	service.OnEvent(printWatchEvent)
	if err := service.Run(ctx); err != nil && !errors.Is(err, context.Canceled) {
		return err
	}
	fmt.Println("\ncurb stopped")
	return nil
}

func printWatchEvent(event ledger.Event) {
	switch event.Type {
	case "run_started":
		fmt.Printf("started   %-16s pid=%v run=%s\n", event.AgentID, event.Data["pid"], event.RunID)
	case "policy_warning":
		fmt.Printf("warning   %-16s %s\n", event.AgentID, event.Message)
	case "would_terminate":
		fmt.Printf("would-kill %-15s pid=%v run=%s\n", event.AgentID, event.Data["pid"], event.RunID)
	case "watch_only":
		fmt.Printf("watch-only %-14s pid=%v %s\n", event.AgentID, event.Data["pid"], event.Message)
	case "grace_started":
		fmt.Printf("grace     %-16s pid=%v grace=%v\n", event.AgentID, event.Data["pid"], event.Data["grace_period"])
	case "termination_started":
		fmt.Printf("killing   %-16s pid=%v run=%s\n", event.AgentID, event.Data["pid"], event.RunID)
	case "termination_completed":
		fmt.Printf("killed    %-16s run=%s\n", event.AgentID, event.RunID)
	case "ack_received":
		fmt.Printf("extended  %-16s %v\n", event.AgentID, event.Data["extend"])
	case "run_stopped":
		fmt.Printf("stopped   %-16s run=%s\n", event.AgentID, event.RunID)
	case "usage_warning":
		fmt.Printf("usage     %-16s %s\n", event.AgentID, event.Message)
	case "usage_would_terminate":
		fmt.Printf("would-stop %-14s pid=%v %s\n", event.AgentID, event.Data["pid"], event.Message)
	case "usage_grace_started":
		fmt.Printf("usage-grace %-13s pid=%v %s\n", event.AgentID, event.Data["pid"], event.Message)
	case "usage_kill_blocked":
		fmt.Printf("blocked   %-16s %s\n", event.AgentID, event.Message)
	case "usage_termination_started":
		fmt.Printf("usage-kill %-14s pid=%v %s\n", event.AgentID, event.Data["pid"], event.Message)
	case "usage_termination_completed":
		fmt.Printf("usage-killed %-12s pid=%v\n", event.AgentID, event.Data["pid"])
	}
}

func cmdConfig(args []string) error {
	path := defaultConfigPath()
	if len(args) == 0 || args[0] == "show" {
		cfg, err := config.Load(path)
		if err != nil {
			return err
		}
		if len(args) == 0 && stdinIsTerminal() {
			return promptConfig(path, cfg)
		}
		printConfigSummary(path, cfg)
		return nil
	}
	if args[0] == "path" {
		fmt.Println(defaultConfigPath())
		return nil
	}
	if args[0] == "aggressive" || args[0] == "reasonable" || args[0] == "observe" {
		return applyPreset(defaultConfigPath(), args[0])
	}
	if args[0] == "set" {
		return cmdConfigSet(args[1:])
	}
	usageConfig()
	return fmt.Errorf("unknown config command %q", args[0])
}

func cmdConfigSet(args []string) error {
	fs := flag.NewFlagSet("config set", flag.ExitOnError)
	configPath := fs.String("config", defaultConfigPath(), "config file")
	mode := fs.String("mode", "", "visibility, alert, or enforcement")
	warnAfter := fs.String("warn-after", "", "warning threshold")
	killAfter := fs.String("kill-after", "", "kill threshold")
	grace := fs.String("grace", "", "kill grace period")
	scan := fs.String("scan", "", "scan interval")
	usageEnabled := fs.String("usage", "", "enable usage guard: true or false")
	warnTurnTokens := fs.Int64("warn-turn-tokens", 0, "warn when a session reaches this many tokens in a turn")
	killTurnTokens := fs.Int64("kill-turn-tokens", 0, "kill when a session reaches this many tokens in a turn")
	usageWindow := fs.String("usage-window", "", "usage rolling window")
	usageScan := fs.String("usage-scan", "", "usage scan interval")
	if err := fs.Parse(args); err != nil {
		return err
	}
	cfg, err := config.Load(*configPath)
	if err != nil {
		return err
	}
	if *mode != "" {
		cfg.Mode = config.Mode(*mode)
	}
	if *warnAfter != "" {
		d, err := time.ParseDuration(*warnAfter)
		if err != nil {
			return err
		}
		cfg.Defaults.WarnAfter.Duration = d
	}
	if *killAfter != "" {
		d, err := time.ParseDuration(*killAfter)
		if err != nil {
			return err
		}
		cfg.Defaults.KillAfter.Duration = d
	}
	if *grace != "" {
		d, err := time.ParseDuration(*grace)
		if err != nil {
			return err
		}
		cfg.Defaults.KillGracePeriod.Duration = d
	}
	if *scan != "" {
		d, err := time.ParseDuration(*scan)
		if err != nil {
			return err
		}
		cfg.Service.ScanInterval.Duration = d
	}
	if *usageEnabled != "" {
		switch strings.ToLower(*usageEnabled) {
		case "true", "yes", "on", "1":
			cfg.Usage.Enabled = boolPtr(true)
		case "false", "no", "off", "0":
			cfg.Usage.Enabled = boolPtr(false)
		default:
			return fmt.Errorf("--usage must be true or false")
		}
	}
	if *warnTurnTokens > 0 {
		cfg.Usage.WarnTurnTokens = *warnTurnTokens
	}
	if *killTurnTokens > 0 {
		cfg.Usage.KillTurnTokens = *killTurnTokens
	}
	if *usageWindow != "" {
		d, err := time.ParseDuration(*usageWindow)
		if err != nil {
			return err
		}
		cfg.Usage.Window.Duration = d
	}
	if *usageScan != "" {
		d, err := time.ParseDuration(*usageScan)
		if err != nil {
			return err
		}
		cfg.Usage.ScanInterval.Duration = d
	}
	keepProcessAgents(cfg)
	applyPolicyToAgents(cfg)
	if err := saveConfig(*configPath, cfg); err != nil {
		return err
	}
	printConfigSummary(*configPath, cfg)
	return nil
}

func cmdScan(args []string, capture processCapture) error {
	fs := flag.NewFlagSet("scan", flag.ExitOnError)
	configPath := fs.String("config", defaultConfigPath(), "config file")
	jsonOut := fs.Bool("json", false, "print JSON matches")
	if err := fs.Parse(args); err != nil {
		return err
	}
	cfg, err := config.Load(*configPath)
	if err != nil {
		return err
	}
	l, err := ledger.Open(cfg.Ledger.Path)
	if err != nil {
		return err
	}
	service := watchdog.New(cfg, l)
	snap, err := capture(context.Background())
	if err != nil {
		return err
	}
	matches := service.Match(snap)
	if *jsonOut {
		return json.NewEncoder(os.Stdout).Encode(redactedMatches(matches))
	}
	for _, match := range matches {
		target := "enforceable"
		if !match.Agent.TerminationAllowed() {
			target = "watch-only"
		}
		fmt.Printf("%-22s pid=%-7d confidence=%-3d target=%-11s name=%s exe=%s\n", match.Agent.ID, match.Process.PID, match.Confidence, target, match.Process.Name, match.Process.Exe)
		if len(match.Evidence) > 0 {
			fmt.Printf("  evidence: %s\n", strings.Join(match.Evidence, ", "))
		}
	}
	return nil
}

func redactedMatches(matches []watchdog.Match) []watchdog.Match {
	out := make([]watchdog.Match, len(matches))
	for i, match := range matches {
		out[i] = match
		if out[i].Process.Cmdline != "" {
			out[i].Process.Cmdline = "<redacted>"
		}
	}
	return out
}

func cmdValidate(args []string) error {
	fs := flag.NewFlagSet("validate-config", flag.ExitOnError)
	if err := fs.Parse(args); err != nil {
		return err
	}
	path := defaultConfigPath()
	if fs.NArg() > 0 {
		path = fs.Arg(0)
	}
	cfg, err := config.Load(path)
	if err != nil {
		return err
	}
	fmt.Printf("ok config=%s mode=%s agents=%d ledger=%s\n", path, cfg.Mode, len(cfg.Agents), cfg.Ledger.Path)
	return nil
}

func cmdStatus(args []string) error {
	fs := flag.NewFlagSet("status", flag.ExitOnError)
	configPath := fs.String("config", defaultConfigPath(), "config file")
	if err := fs.Parse(args); err != nil {
		return err
	}
	cfg, err := config.Load(*configPath)
	if err != nil {
		return err
	}
	events, err := ledger.Read(cfg.Ledger.Path)
	if err != nil {
		return err
	}
	active := activeRuns(events)
	printConfigSummary(*configPath, cfg)
	fmt.Printf("\nactive runs: %d\n", len(active))
	fmt.Printf("ledger: %s\n", cfg.Ledger.Path)
	return nil
}

func cmdRuns(args []string) error {
	fs := flag.NewFlagSet("runs", flag.ExitOnError)
	configPath := fs.String("config", defaultConfigPath(), "config file")
	jsonOut := fs.Bool("json", false, "print JSON")
	activeOnly := fs.Bool("active", false, "active runs only")
	all := fs.Bool("all", false, "show historical and duplicate runs")
	if err := fs.Parse(args); err != nil {
		return err
	}
	cfg, err := config.Load(*configPath)
	if err != nil {
		return err
	}
	events, err := ledger.Read(cfg.Ledger.Path)
	if err != nil {
		return err
	}
	runs := summarizeRuns(events)
	if !*all {
		runs = compactRuns(runs)
	}
	if *activeOnly || !*all {
		filtered := runs[:0]
		for _, run := range runs {
			if !run.Ended {
				filtered = append(filtered, run)
			}
		}
		runs = filtered
	}
	if *jsonOut {
		return json.NewEncoder(os.Stdout).Encode(runs)
	}
	if len(runs) == 0 {
		fmt.Println("no runs")
		return nil
	}
	fmt.Printf("%-10s %-16s %-7s %-10s %-8s %s\n", "STATE", "AGENT", "PID", "RUNTIME", "STARTED", "RUN")
	for _, run := range runs {
		state := "active"
		if run.Ended {
			state = "ended"
		}
		fmt.Printf("%-10s %-16s %-7v %-10s %-8s %s\n", state, run.AgentID, run.PID, shortDuration(run.Elapsed), run.Started.Local().Format("15:04:05"), run.RunID)
	}
	return nil
}

func cmdAck(args []string) error {
	runID, configPath, extend, reason, err := parseAckArgs(args)
	if err != nil {
		return err
	}
	cfg, err := config.Load(configPath)
	if err != nil {
		return err
	}
	if err := watchdog.WriteAck(cfg.Service.StateDir, runID, extend, reason); err != nil {
		return err
	}
	fmt.Printf("ack queued for %s\n", runID)
	return nil
}

func parseAckArgs(args []string) (runID, configPath, extend, reason string, err error) {
	configPath = defaultConfigPath()
	extend = "30m"
	nextValue := func(i *int, name string) (string, error) {
		*i = *i + 1
		if *i >= len(args) {
			return "", fmt.Errorf("%s requires a value", name)
		}
		return args[*i], nil
	}

	for i := 0; i < len(args); i++ {
		arg := args[i]
		switch {
		case arg == "--config":
			value, err := nextValue(&i, "--config")
			if err != nil {
				return "", "", "", "", err
			}
			configPath = value
		case strings.HasPrefix(arg, "--config="):
			configPath = strings.TrimPrefix(arg, "--config=")
		case arg == "--extend":
			value, err := nextValue(&i, "--extend")
			if err != nil {
				return "", "", "", "", err
			}
			extend = value
		case strings.HasPrefix(arg, "--extend="):
			extend = strings.TrimPrefix(arg, "--extend=")
		case arg == "--reason":
			value, err := nextValue(&i, "--reason")
			if err != nil {
				return "", "", "", "", err
			}
			reason = value
		case strings.HasPrefix(arg, "--reason="):
			reason = strings.TrimPrefix(arg, "--reason=")
		case strings.HasPrefix(arg, "-"):
			return "", "", "", "", fmt.Errorf("unknown ack option %q", arg)
		default:
			if runID != "" {
				return "", "", "", "", fmt.Errorf("usage: curb ack <run-id> --extend 30m")
			}
			runID = arg
		}
	}
	if runID == "" {
		return "", "", "", "", fmt.Errorf("usage: curb ack <run-id> --extend 30m")
	}
	return runID, configPath, extend, reason, nil
}

func cmdDoctor(args []string, capture processCapture, notify notifier) error {
	fs := flag.NewFlagSet("doctor", flag.ExitOnError)
	configPath := fs.String("config", defaultConfigPath(), "config file")
	if err := fs.Parse(args); err != nil {
		return err
	}
	cfg, err := config.Load(*configPath)
	if err != nil {
		return err
	}
	fmt.Printf("config: ok %s\n", *configPath)
	if err := os.MkdirAll(cfg.Service.StateDir, 0o700); err != nil {
		return err
	}
	fmt.Printf("state_dir: ok %s\n", cfg.Service.StateDir)
	l, err := ledger.Open(cfg.Ledger.Path)
	if err != nil {
		return err
	}
	if err := l.Append(ledger.Event{Type: "doctor", Mode: string(cfg.Mode), Message: "ledger write check"}); err != nil {
		return err
	}
	fmt.Printf("ledger: ok %s\n", cfg.Ledger.Path)
	snap, err := capture(context.Background())
	if err != nil {
		return err
	}
	fmt.Printf("process_snapshot: ok processes=%d platform=%s\n", len(snap.Processes), snap.Platform)
	if err := notify("Curb doctor", "Notification check"); err != nil {
		fmt.Printf("notifications: unavailable %v\n", err)
	} else {
		fmt.Println("notifications: ok")
	}
	return nil
}

type runSummary struct {
	RunID   string        `json:"run_id"`
	AgentID string        `json:"agent_id"`
	Started time.Time     `json:"started"`
	Last    time.Time     `json:"last"`
	Elapsed time.Duration `json:"elapsed"`
	PID     any           `json:"pid,omitempty"`
	Ended   bool          `json:"ended"`
}

func activeRuns(events []ledger.Event) map[string]runSummary {
	runs := compactRuns(summarizeRuns(events))
	out := map[string]runSummary{}
	for _, run := range runs {
		if !run.Ended {
			out[run.RunID] = run
		}
	}
	return out
}

func summarizeRuns(events []ledger.Event) []runSummary {
	byID := map[string]*runSummary{}
	for _, event := range events {
		if event.RunID == "" {
			continue
		}
		run := byID[event.RunID]
		if run == nil {
			run = &runSummary{RunID: event.RunID, AgentID: event.AgentID, Started: event.Time, Last: event.Time}
			byID[event.RunID] = run
		}
		if event.AgentID != "" {
			run.AgentID = event.AgentID
		}
		if event.Time.Before(run.Started) {
			run.Started = event.Time
		}
		if event.Time.After(run.Last) {
			run.Last = event.Time
		}
		run.Elapsed = time.Since(run.Started)
		if pid, ok := event.Data["pid"]; ok {
			run.PID = pid
		}
		switch event.Type {
		case "run_stopped", "termination_completed", "termination_failed":
			run.Ended = true
			run.Elapsed = event.Time.Sub(run.Started)
		}
	}
	var runs []runSummary
	for _, run := range byID {
		runs = append(runs, *run)
	}
	sort.Slice(runs, func(i, j int) bool { return runs[i].Started.After(runs[j].Started) })
	return runs
}

func compactRuns(runs []runSummary) []runSummary {
	latest := map[string]runSummary{}
	var ended []runSummary
	for _, run := range runs {
		if run.Ended {
			ended = append(ended, run)
			continue
		}
		key := fmt.Sprintf("%s:%v", run.AgentID, run.PID)
		if existing, ok := latest[key]; !ok || run.Started.After(existing.Started) {
			latest[key] = run
		}
	}
	out := make([]runSummary, 0, len(latest)+len(ended))
	for _, run := range latest {
		out = append(out, run)
	}
	sort.Slice(out, func(i, j int) bool { return out[i].Started.After(out[j].Started) })
	return out
}

func printWatchBanner(path string, cfg *config.Config) {
	fmt.Println("curb is watching")
	fmt.Printf("  mode:   %s\n", cfg.Mode)
	fmt.Printf("  config: %s\n", path)
	fmt.Printf("  policy: warn %s, kill %s, grace %s\n", shortDuration(cfg.Defaults.WarnAfter.Duration), shortDuration(cfg.Defaults.KillAfter.Duration), shortDuration(cfg.Defaults.KillGracePeriod.Duration))
	if cfg.Usage.IsEnabled() {
		fmt.Printf("  usage:  warn %s/turn; stop %s/turn\n",
			tokenCount(cfg.Usage.WarnTurnTokens),
			tokenCount(cfg.Usage.KillTurnTokens),
		)
	} else {
		fmt.Println("  usage:  disabled; watch refreshes visibility only")
	}
	fmt.Printf("  agents: %s\n", agentLabels(cfg.Agents))
	if cfg.Alerts.LocalNotifications {
		fmt.Println("  alerts: local desktop notifications")
	}
	if cfg.Mode != config.ModeEnforcement {
		fmt.Println("  action: alert only; no processes will be killed")
	} else {
		fmt.Println("  action: enforcement enabled for agent processes only")
	}
	fmt.Println()
}

func printConfigSummary(path string, cfg *config.Config) {
	fmt.Println("curb config")
	fmt.Printf("  path: %s\n", compactHome(path))
	fmt.Printf("  mode: %s\n", cfg.Mode)
	fmt.Printf("  action: %s\n", actionLabel(cfg.Mode))
	if cfg.Mode != config.ModeEnforcement {
		fmt.Println("  safety: Curb will warn only; it will not stop processes in this mode.")
	} else {
		fmt.Println("  safety: enforcement is enabled; Curb can stop correlated agent workers after grace.")
	}
	fmt.Println()
	fmt.Println("runtime fallback")
	fmt.Printf("  warn after %s; stop after %s; grace %s; extend %s x%d\n",
		shortDuration(cfg.Defaults.WarnAfter.Duration),
		shortDuration(cfg.Defaults.KillAfter.Duration),
		shortDuration(cfg.Defaults.KillGracePeriod.Duration),
		shortDuration(cfg.Defaults.AckExtension.Duration),
		cfg.Defaults.MaxExtensions,
	)
	fmt.Println("  note: runtime limits are legacy metadata; the product watcher enforces usage policy.")
	fmt.Println()
	fmt.Println("usage policy")
	if cfg.Usage.IsEnabled() {
		fmt.Printf("  warn: %s per turn\n", tokenCount(cfg.Usage.WarnTurnTokens))
		fmt.Printf("  stop: %s per turn\n", tokenCount(cfg.Usage.KillTurnTokens))
		fmt.Printf("  scan: every %s; grace %s\n", shortDuration(cfg.Usage.ScanInterval.Duration), shortDuration(cfg.Usage.GracePeriod.Duration))
	} else {
		fmt.Println("  disabled")
	}
	fmt.Println()
	fmt.Println("watched agents")
	fmt.Printf("  %s\n", agentLabels(cfg.Agents))
	fmt.Println()
	fmt.Println("next")
	fmt.Println("  curb dashboard                 see live workers, current usage, and risk")
	fmt.Println("  curb watch                     run automatic warning/enforcement loop")
	fmt.Println("  curb config reasonable         return to notify-only defaults")
	fmt.Println("  curb config aggressive         local enforcement test thresholds")
	fmt.Println()
	fmt.Println("presets:")
	fmt.Println("  curb config aggressive   test agent kills quickly: warn 30s, kill 60s")
	fmt.Println("  curb config reasonable   warn only: warn 90m, kill 120m")
	fmt.Println("  curb config observe      record only: no warnings or kills")
	fmt.Println("  curb config set ...      custom thresholds")
}

func printUsageReport(report usagepkg.Report, sessions []usagewatch.Session, cfg *config.Config, limit int) {
	view := classifyUsage(sessions, cfg, report.GeneratedAt)
	fmt.Println("curb usage")
	printUsageHeader(report, view, cfg)
	printUsageFindings(view)
	printUsageTable(view, limit)
}

func printDashboard(path string, cfg *config.Config, snapshot servicepkg.Snapshot, limit int) {
	fmt.Println("curb dashboard")
	fmt.Printf("  config: %s\n", compactHome(path))
	fmt.Printf("  action: %s\n", actionLabel(cfg.Mode))
	fmt.Println()
	printDashboardHeader(snapshot, cfg)
	printDashboardFindings(snapshot)
	printDashboardAgents(snapshot.Agents)
	fmt.Println()
	printDashboardSessions(snapshot.Sessions, limit)
}

type usageRow struct {
	Session usagewatch.Session
	Status  string
	Reason  string
	Risk    int
	Active  bool
}

type usageView struct {
	Rows        []usageRow
	Window      time.Duration
	Active      int
	Warn        int
	Stop        int
	IdleHigh    int
	TurnTokens  int64
	TotalTokens int64
}

func usageSessions(events []usagepkg.Event) []usagewatch.Session {
	return usagewatch.BuildSessions(events)
}

func classifyUsage(sessions []usagewatch.Session, cfg *config.Config, now time.Time) usageView {
	if now.IsZero() {
		now = time.Now()
	}
	if cfg == nil {
		cfg = &config.Config{Version: 1}
		_ = cfg.SetDefaults()
	}
	window := cfg.Usage.Window.Duration
	view := usageView{Window: window}
	for _, session := range sessions {
		decision := usagewatch.EvaluateSessionDecision(session, cfg, usagewatch.Correlation{}, now)
		classification := usagewatch.ClassifySession(decision, usagewatch.Correlation{}, cfg.Mode, nil, cfg.Defaults.AckExtension.Duration)
		active := decision.Policy.Active
		if active {
			view.Active++
			view.TurnTokens += session.LastTurnTokens
		}
		view.TotalTokens += session.Total
		switch classification.UsageState {
		case "stop":
			if active {
				view.Stop++
			} else {
				view.IdleHigh++
			}
		case "warn":
			view.Warn++
		}
		if classification.State == "idle-high" {
			view.IdleHigh++
		}
		status := classification.State
		if classification.UsageState != "" && classification.UsageState != classification.State {
			status = classification.State + "/" + classification.UsageState
		}
		view.Rows = append(view.Rows, usageRow{
			Session: session,
			Status:  status,
			Reason:  classification.Explanation,
			Risk:    classification.RiskRank,
			Active:  active,
		})
	}
	sort.Slice(view.Rows, func(i, j int) bool {
		if view.Rows[i].Risk != view.Rows[j].Risk {
			return view.Rows[i].Risk < view.Rows[j].Risk
		}
		if view.Rows[i].Session.LastTurnTokens != view.Rows[j].Session.LastTurnTokens {
			return view.Rows[i].Session.LastTurnTokens > view.Rows[j].Session.LastTurnTokens
		}
		return view.Rows[i].Session.Last.After(view.Rows[j].Session.Last)
	})
	return view
}

func printUsageHeader(report usagepkg.Report, view usageView, cfg *config.Config) {
	state := "OK"
	message := "no active over-budget usage"
	if view.Stop > 0 {
		state = "ACTION"
		message = "active usage is over a stop threshold"
	} else if view.Warn > 0 {
		state = "WATCH"
		message = "active usage is over a warning threshold"
	} else if view.Active > 0 {
		state = "ACTIVE"
		message = "agents are spending tokens within policy"
	}
	fmt.Printf("  status: %s - %s\n", state, message)
	fmt.Printf("  window: %s; active sessions: %d; turn tokens: %s; lookback tokens: %s\n",
		shortDuration(view.Window), view.Active, tokenCount(view.TurnTokens), tokenCount(view.TotalTokens))
	printUsagePolicy(cfg)
	fmt.Printf("  scanned: %s\n", report.GeneratedAt.Local().Format("2006-01-02 15:04:05"))
	var sourceLabels []string
	for _, source := range report.Sources {
		label := fmt.Sprintf("%s %d events", source.Provider, source.Events)
		if source.Error != "" {
			label = source.Provider + " unavailable"
		}
		sourceLabels = append(sourceLabels, label)
	}
	if len(sourceLabels) > 0 {
		fmt.Printf("  sources: %s\n", strings.Join(sourceLabels, "; "))
	}
	fmt.Println()
}

func printDashboardHeader(snapshot servicepkg.Snapshot, cfg *config.Config) {
	overview := snapshot.Overview
	fmt.Printf("  status: %s - %s\n", overview.Status, overview.Message)
	fmt.Printf("  window tokens: %s; lookback tokens: %s; live agents: %d; active sessions: %d\n",
		tokenCount(overview.WindowTokens),
		tokenCount(overview.LookbackTokens),
		overview.ActiveAgents,
		overview.ActiveSessions,
	)
	printUsagePolicy(cfg)
	fmt.Printf("  scanned: %s\n", overview.LastScan.Local().Format("2006-01-02 15:04:05"))
	var sourceLabels []string
	for _, source := range overview.Sources {
		label := fmt.Sprintf("%s %d events", source.Provider, source.Events)
		if source.Error != "" {
			label = source.Provider + " unavailable"
		}
		sourceLabels = append(sourceLabels, label)
	}
	if len(sourceLabels) > 0 {
		fmt.Printf("  sources: %s\n", strings.Join(sourceLabels, "; "))
	}
	fmt.Println()
}

func printDashboardFindings(snapshot servicepkg.Snapshot) {
	fmt.Println("attention")
	overview := snapshot.Overview
	switch {
	case overview.StopSessions > 0:
		fmt.Printf("  %d actionable session(s) are over stop thresholds. Curb can stop correlated workers after grace in enforcement mode.\n", overview.StopSessions)
	case overview.WarningSessions > 0:
		fmt.Printf("  %d session(s) need attention, but are not immediately actionable. Check usage state, correlation, and mode before enabling enforcement.\n", overview.WarningSessions)
	default:
		fmt.Println("  none. Historical high-turn sessions are visible below, but idle sessions are not treated as runaway spend.")
	}
	if overview.IdleHighSessions > 0 {
		fmt.Printf("  note: %d large historical turn session(s) are idle-high, meaning expensive but not currently spending.\n", overview.IdleHighSessions)
	}
	fmt.Println()
}

func printDashboardAgents(agents []servicepkg.AgentView) {
	fmt.Printf("live agents: %d\n", len(agents))
	if len(agents) == 0 {
		fmt.Println("  none matched")
		return
	}
	fmt.Printf("  %-7s %-20s %-10s %-12s %-12s %s\n", "PID", "AGENT", "STATE", "USAGE", "LATEST_TURN", "PROJECT")
	for _, agent := range agents {
		usageState := agent.UsageState
		if usageState == "" {
			usageState = "-"
		}
		latestTurn := "-"
		if agent.LatestTurnTokens > 0 {
			latestTurn = tokenCount(agent.LatestTurnTokens)
		}
		fmt.Printf("  %-7d %-20s %-10s %-12s %-12s %s\n",
			agent.PID,
			agent.ID,
			agent.State,
			usageState,
			latestTurn,
			projectLabel(agent.CWD),
		)
	}
}

func printUsagePolicy(cfg *config.Config) {
	if cfg == nil || !cfg.Usage.IsEnabled() {
		fmt.Println("  policy: usage monitoring disabled")
		return
	}
	fmt.Printf("  policy: warn %s/turn; stop %s/turn\n",
		tokenCount(cfg.Usage.WarnTurnTokens),
		tokenCount(cfg.Usage.KillTurnTokens),
	)
}

func printUsageFindings(view usageView) {
	fmt.Println("attention")
	switch {
	case view.Stop > 0:
		fmt.Printf("  %d active session(s) are over stop thresholds. In enforcement mode Curb will stop correlated workers after grace.\n", view.Stop)
	case view.Warn > 0:
		fmt.Printf("  %d active session(s) are over warning thresholds. Watch or acknowledge before enabling enforcement.\n", view.Warn)
	default:
		fmt.Println("  none. Historical high-turn sessions are visible below, but idle sessions are not treated as runaway spend.")
	}
	if view.IdleHigh > 0 {
		fmt.Printf("  note: %d large historical turn session(s) are idle-high, meaning expensive but not currently spending.\n", view.IdleHigh)
	}
	fmt.Println()
}

func printUsageTable(view usageView, limit int) {
	if len(view.Rows) == 0 {
		fmt.Println("sessions")
		fmt.Println("  no local usage events found")
		return
	}
	if limit <= 0 || limit > len(view.Rows) {
		limit = len(view.Rows)
	}
	fmt.Println("sessions")
	fmt.Printf("  %-9s %-7s %-8s %-11s %-9s %-7s %-18s %s\n", "STATUS", "AGENT", "LAST", "LATEST_TURN", "TOTAL", "CALLS", "PROJECT", "WHY")
	for _, row := range view.Rows[:limit] {
		session := row.Session
		fmt.Printf("  %-9s %-7s %-8s %-11s %-9s %-7d %-18s %s\n",
			row.Status,
			session.Provider,
			relativeTime(sessionDisplayTime(session)),
			tokenCount(session.LastTurnTokens),
			tokenCount(session.Total),
			session.Events,
			projectLabel(session.CWD),
			row.Reason,
		)
		if len(session.Models) > 0 {
			fmt.Printf("    models: %s\n", strings.Join(session.Models, ", "))
		}
		fmt.Printf("    path: %s  session: %s\n", compactHome(session.CWD), shortSessionID(session.SessionID))
	}
	if len(view.Rows) > limit {
		fmt.Printf("\nshowing %d of %d sessions; use --limit %d or --json for more\n", limit, len(view.Rows), len(view.Rows))
	}
}

func printDashboardSessions(sessions []servicepkg.SessionView, limit int) {
	if len(sessions) == 0 {
		fmt.Println("sessions")
		fmt.Println("  no local usage events found")
		return
	}
	if limit <= 0 || limit > len(sessions) {
		limit = len(sessions)
	}
	fmt.Println("sessions")
	fmt.Printf("  %-13s %-7s %-8s %-11s %-9s %-7s %-18s %s\n", "STATUS", "AGENT", "LAST", "LATEST_TURN", "TOTAL", "CALLS", "PROJECT", "WHY")
	for _, session := range sessions[:limit] {
		status := session.State
		if session.UsageState != "" && session.UsageState != session.State {
			status = session.State + "/" + session.UsageState
		}
		fmt.Printf("  %-13s %-7s %-8s %-11s %-9s %-7d %-18s %s\n",
			status,
			session.Provider,
			relativeTime(sessionDisplayTimeView(session)),
			tokenCount(session.LatestTurnTokens),
			tokenCount(session.TotalTokens),
			session.Calls,
			projectLabel(session.CWD),
			session.Explanation,
		)
		if len(session.Models) > 0 {
			fmt.Printf("    models: %s\n", strings.Join(session.Models, ", "))
		}
		process := "uncorrelated"
		if session.CorrelatedPID != 0 {
			process = fmt.Sprintf("pid %d via %s", session.CorrelatedPID, session.CorrelationReason)
		}
		fmt.Printf("    path: %s  session: %s  process: %s\n", compactHome(session.CWD), shortSessionID(session.ID), process)
	}
	if len(sessions) > limit {
		fmt.Printf("\nshowing %d of %d sessions; use --limit %d or --json for more\n", limit, len(sessions), len(sessions))
	}
}

func sessionDisplayTime(session usagewatch.Session) time.Time {
	if !session.LastUsage.IsZero() {
		return session.LastUsage
	}
	return session.Last
}

func sessionDisplayTimeView(session servicepkg.SessionView) time.Time {
	if session.LastUsageAt != nil {
		return *session.LastUsageAt
	}
	return session.LastSeenAt
}

func printLiveAgentSummary(matches []watchdog.Match, sessions []usagewatch.Session) {
	fmt.Printf("live agents: %d", len(matches))
	byAgent := map[string]int{}
	for _, match := range matches {
		byAgent[match.Agent.ID]++
	}
	if len(byAgent) > 0 {
		var parts []string
		for agent, count := range byAgent {
			parts = append(parts, fmt.Sprintf("%s %d", agent, count))
		}
		sort.Strings(parts)
		fmt.Printf(" (%s)", strings.Join(parts, ", "))
	}
	fmt.Println()
	if len(matches) == 0 {
		fmt.Println("  none matched")
		return
	}
	fmt.Printf("  %-7s %-20s %-8s %-12s %s\n", "PID", "AGENT", "RUNNING", "LATEST_TURN", "PROJECT")
	for _, match := range matches {
		elapsed := "unknown"
		if match.Process.StartedOK {
			elapsed = shortDuration(time.Since(match.Process.Create))
		}
		turnSpend := "-"
		if sess, found := usagewatch.BestSessionForMatch(match, sessions); found {
			turnSpend = tokenCount(sess.LastTurnTokens)
		}
		fmt.Printf("  %-7d %-20s %-8s %-12s %s\n", match.Process.PID, match.Agent.ID, elapsed, turnSpend, projectLabel(match.Process.CWD))
	}
}

type liveAgentGroup struct {
	AgentID string
	Project string
	Count   int
	Newest  time.Time
}

func liveAgentGroups(matches []watchdog.Match) []liveAgentGroup {
	byKey := map[string]*liveAgentGroup{}
	for _, match := range matches {
		project := projectLabel(match.Process.CWD)
		key := match.Agent.ID + ":" + project
		group := byKey[key]
		if group == nil {
			group = &liveAgentGroup{AgentID: match.Agent.ID, Project: project}
			byKey[key] = group
		}
		group.Count++
		if match.Process.StartedOK && match.Process.Create.After(group.Newest) {
			group.Newest = match.Process.Create
		}
	}
	var out []liveAgentGroup
	for _, group := range byKey {
		out = append(out, *group)
	}
	sort.Slice(out, func(i, j int) bool {
		if out[i].Count != out[j].Count {
			return out[i].Count > out[j].Count
		}
		return out[i].Newest.After(out[j].Newest)
	})
	return out
}

func printNewUsageEvents(since time.Time, seen map[string]bool) error {
	events, _, err := usagepkg.EventsSince(since)
	if err != nil {
		return err
	}
	sort.Slice(events, func(i, j int) bool { return events[i].Timestamp.Before(events[j].Timestamp) })
	for _, event := range events {
		key := fmt.Sprintf("%s:%s:%s:%s:%d:%d", event.Provider, event.SessionID, event.RequestID, event.Timestamp.Format(time.RFC3339Nano), event.Total, event.Cumulative)
		if seen[key] {
			continue
		}
		seen[key] = true
		if time.Since(event.Timestamp) > 24*time.Hour {
			continue
		}
		model := event.Model
		if model == "" {
			model = "-"
		}
		fmt.Printf("%s %-7s %-12s total=%-8s output=%-7s model=%s cwd=%s\n",
			event.Timestamp.Local().Format("15:04:05"),
			event.Provider,
			shortSessionID(event.SessionID),
			tokenCount(event.Total),
			tokenCount(event.Output),
			model,
			compactHome(event.CWD),
		)
	}
	return nil
}

func actionLabel(mode config.Mode) string {
	switch mode {
	case config.ModeEnforcement:
		return "kill agent processes after the limit"
	case config.ModeAlert:
		return "notify only; never kill"
	default:
		return "record only; never kill"
	}
}

func agentLabels(agents []config.Agent) string {
	labels := make([]string, 0, len(agents))
	for _, agent := range agents {
		labels = append(labels, agent.ID)
	}
	return strings.Join(labels, ", ")
}

func applyPreset(path, preset string) error {
	cfg, err := config.Load(path)
	if err != nil {
		return err
	}
	keepProcessAgents(cfg)
	cfg.Service.MinConfidence = 50
	switch preset {
	case "aggressive":
		cfg.Mode = config.ModeEnforcement
		cfg.Service.ScanInterval.Duration = time.Second
		cfg.Service.HeartbeatInterval.Duration = 5 * time.Second
		cfg.Usage.Enabled = boolPtr(true)
		cfg.Usage.ScanInterval.Duration = time.Second
		cfg.Usage.Window.Duration = time.Minute
		cfg.Usage.Window.Duration = time.Minute
		cfg.Usage.WarnTurnTokens = 250_000
		cfg.Usage.KillTurnTokens = 750_000
		cfg.Usage.GracePeriod.Duration = 10 * time.Second
		cfg.Defaults.WarnAfter.Duration = 30 * time.Second
		cfg.Defaults.KillAfter.Duration = 60 * time.Second
		cfg.Defaults.KillGracePeriod.Duration = 10 * time.Second
		cfg.Defaults.AckExtension.Duration = 30 * time.Second
		cfg.Defaults.MaxExtensions = 1
		cfg.Defaults.MinLifetime.Duration = time.Second
		cfg.Defaults.MaxRunGap.Duration = 2 * time.Second
		applyPolicyToAgents(cfg)
	case "reasonable":
		cfg.Mode = config.ModeAlert
		cfg.Service.ScanInterval.Duration = 15 * time.Second
		cfg.Service.HeartbeatInterval.Duration = time.Minute
		cfg.Usage.Enabled = boolPtr(true)
		cfg.Usage.ScanInterval.Duration = 5 * time.Second
		cfg.Usage.Window.Duration = 15 * time.Minute
		cfg.Usage.WarnTurnTokens = 1_000_000
		cfg.Usage.KillTurnTokens = 3_000_000
		cfg.Usage.GracePeriod.Duration = time.Minute
		cfg.Defaults.WarnAfter.Duration = 90 * time.Minute
		cfg.Defaults.KillAfter.Duration = 2 * time.Hour
		cfg.Defaults.KillGracePeriod.Duration = time.Minute
		cfg.Defaults.AckExtension.Duration = 30 * time.Minute
		cfg.Defaults.MaxExtensions = 2
		applyPolicyToAgents(cfg)
	case "observe":
		cfg.Mode = config.ModeVisibility
		cfg.Service.ScanInterval.Duration = 15 * time.Second
		cfg.Usage.Enabled = boolPtr(true)
		cfg.Usage.ScanInterval.Duration = 10 * time.Second
		cfg.Usage.Window.Duration = 15 * time.Minute
		cfg.Usage.WarnTurnTokens = 5_000_000
		cfg.Usage.KillTurnTokens = 10_000_000
		cfg.Usage.GracePeriod.Duration = time.Minute
		cfg.Defaults.WarnAfter.Duration = 24 * time.Hour
		cfg.Defaults.KillAfter.Duration = 48 * time.Hour
		cfg.Defaults.KillGracePeriod.Duration = time.Minute
		cfg.Defaults.AckExtension.Duration = 30 * time.Minute
		cfg.Defaults.MaxExtensions = 2
		applyPolicyToAgents(cfg)
	default:
		return fmt.Errorf("unknown preset %q", preset)
	}
	if err := saveConfig(path, cfg); err != nil {
		return err
	}
	printConfigSummary(path, cfg)
	return nil
}

func applyPolicyToAgents(cfg *config.Config) {
	for i := range cfg.Agents {
		policy := cfg.Defaults
		policy.AllowAppRootKill = false
		cfg.Agents[i].Policy = &policy
	}
}

func keepProcessAgents(cfg *config.Config) {
	agents := defaultProcessAgents()
	seen := map[string]bool{}
	for _, agent := range agents {
		seen[agent.ID] = true
	}
	for _, agent := range cfg.Agents {
		if seen[agent.ID] {
			continue
		}
		if agent.TerminationAllowed() {
			if agent.Kind == "" {
				agent.Kind = config.AgentKindProcess
			}
			agents = append(agents, agent)
			seen[agent.ID] = true
		}
	}
	cfg.Agents = agents
}

func promptConfig(path string, cfg *config.Config) error {
	printConfigSummary(path, cfg)
	fmt.Println()
	fmt.Println("Choose a setup:")
	fmt.Println("  1  Observe: record only")
	fmt.Println("  2  Reasonable: warn only after 90m")
	fmt.Println("  3  Aggressive test: kill agent processes after 60s")
	fmt.Println("  4  Custom")
	fmt.Print("Selection [2]: ")

	reader := bufio.NewReader(os.Stdin)
	choice, _ := reader.ReadString('\n')
	switch strings.TrimSpace(choice) {
	case "", "2", "reasonable":
		return applyPreset(path, "reasonable")
	case "1", "observe":
		return applyPreset(path, "observe")
	case "3", "aggressive":
		return applyPreset(path, "aggressive")
	case "4", "custom":
		return promptCustomConfig(path, cfg, reader)
	default:
		return fmt.Errorf("unknown selection %q", strings.TrimSpace(choice))
	}
}

func promptCustomConfig(path string, cfg *config.Config, reader *bufio.Reader) error {
	keepProcessAgents(cfg)
	fmt.Print("Warn after [90m]: ")
	warnAfter, _ := reader.ReadString('\n')
	fmt.Print("Kill after [120m]: ")
	killAfter, _ := reader.ReadString('\n')
	fmt.Print("Actually kill agent processes? [y/N]: ")
	kill, _ := reader.ReadString('\n')

	if strings.TrimSpace(warnAfter) == "" {
		warnAfter = "90m"
	}
	if strings.TrimSpace(killAfter) == "" {
		killAfter = "120m"
	}
	warn, err := time.ParseDuration(strings.TrimSpace(warnAfter))
	if err != nil {
		return err
	}
	limit, err := time.ParseDuration(strings.TrimSpace(killAfter))
	if err != nil {
		return err
	}
	cfg.Mode = config.ModeAlert
	if strings.EqualFold(strings.TrimSpace(kill), "y") || strings.EqualFold(strings.TrimSpace(kill), "yes") {
		cfg.Mode = config.ModeEnforcement
	}
	cfg.Defaults.WarnAfter.Duration = warn
	cfg.Defaults.KillAfter.Duration = limit
	applyPolicyToAgents(cfg)
	if err := saveConfig(path, cfg); err != nil {
		return err
	}
	printConfigSummary(path, cfg)
	return nil
}

func stdinIsTerminal() bool {
	stat, err := os.Stdin.Stat()
	return err == nil && (stat.Mode()&os.ModeCharDevice) != 0
}

func saveConfig(path string, cfg *config.Config) error {
	return config.Save(path, cfg)
}

func shortDuration(d time.Duration) string {
	d = d.Round(time.Second)
	if d < time.Minute {
		return d.String()
	}
	if d < time.Hour {
		return fmt.Sprintf("%dm%02ds", int(d.Minutes()), int(d.Seconds())%60)
	}
	return fmt.Sprintf("%dh%02dm", int(d.Hours()), int(d.Minutes())%60)
}

func relativeTime(t time.Time) string {
	if t.IsZero() {
		return "unknown"
	}
	elapsed := time.Since(t)
	if elapsed < 0 {
		elapsed = 0
	}
	if elapsed < time.Minute {
		return "now"
	}
	if elapsed < time.Hour {
		return fmt.Sprintf("%dm ago", int(elapsed.Minutes()))
	}
	if elapsed < 24*time.Hour {
		return fmt.Sprintf("%dh ago", int(elapsed.Hours()))
	}
	return fmt.Sprintf("%dd ago", int(elapsed.Hours()/24))
}

func tokenCount(n int64) string {
	if n >= 1_000_000 {
		return fmt.Sprintf("%.1fM", float64(n)/1_000_000)
	}
	if n >= 10_000 {
		return fmt.Sprintf("%dk", n/1_000)
	}
	return fmt.Sprintf("%d", n)
}

func shortSessionID(id string) string {
	if id == "" {
		return "-"
	}
	if len(id) <= 18 {
		return id
	}
	return id[:8] + "..." + id[len(id)-6:]
}

func compactHome(path string) string {
	home, err := os.UserHomeDir()
	if err == nil && home != "" && strings.HasPrefix(path, home) {
		return "~" + strings.TrimPrefix(path, home)
	}
	return path
}

func projectLabel(path string) string {
	if path == "" {
		return "-"
	}
	clean := filepath.Clean(path)
	base := filepath.Base(clean)
	parent := filepath.Base(filepath.Dir(clean))
	if parent == "worktrees" || parent == ".codex" || parent == "Development" || parent == "Documents" {
		return base
	}
	if len(base) <= 18 {
		return base
	}
	return base[:15] + "..."
}

func boolPtr(value bool) *bool {
	return &value
}

func defaultConfigPath() string {
	if path := os.Getenv("CURB_CONFIG"); path != "" {
		return path
	}
	if _, err := os.Stat("curb.yaml"); err == nil {
		return "curb.yaml"
	}
	if path := userConfigPath(); path != "" {
		if _, err := os.Stat(path); err == nil {
			return path
		}
		return path
	}
	return filepath.Join("configs", "curb.example.yaml")
}

func userConfigPath() string {
	base, err := os.UserConfigDir()
	if err != nil || base == "" {
		if home, homeErr := os.UserHomeDir(); homeErr == nil {
			base = filepath.Join(home, ".config")
		} else {
			return ""
		}
	}
	return filepath.Join(base, "curb", "config.yaml")
}

func ensureDefaultConfig(force bool) (string, error) {
	path := defaultConfigPath()
	if path == "" {
		return "", fmt.Errorf("could not determine default config path")
	}
	if path == "curb.yaml" {
		return path, nil
	}
	if _, err := os.Stat(path); err == nil && !force {
		return path, nil
	}
	createdPath, _, err := writeDefaultConfig(path, "visibility", force)
	return createdPath, err
}

func writeDefaultConfig(path, mode string, force bool) (string, bool, error) {
	if mode != "visibility" && mode != "alert" && mode != "enforcement" {
		return "", false, fmt.Errorf("invalid mode %q", mode)
	}
	if path == "" {
		return "", false, fmt.Errorf("config path is required")
	}
	if _, err := os.Stat(path); err == nil && !force {
		return path, false, nil
	}
	if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
		return "", false, err
	}
	stateDir := filepath.Dir(path)
	content := defaultConfig(mode, stateDir)
	if err := os.WriteFile(path, []byte(content), 0o600); err != nil {
		return "", false, err
	}
	return path, true, nil
}

func defaultConfig(mode, stateDir string) string {
	ledgerPath := filepath.Join(stateDir, "runs.ndjson")
	return fmt.Sprintf(`version: 1
profile: local-default
mode: %s

service:
  scan_interval: 15s
  policy_interval: 5s
  heartbeat_interval: 60s
  min_confidence: 50
  state_dir: %s

usage:
  enabled: true
  scan_interval: 5s
  lookback: 24h
  window: 15m
  warn_turn_tokens: 1000000
  kill_turn_tokens: 3000000
  grace_period: 60s

defaults:
  warn_after: 90m
  kill_after: 120m
  ack_extension: 30m
  max_extensions: 2
  kill_grace_period: 60s
  cooldown_after_kill: 15m
  min_lifetime: 10s
  max_run_gap: 20s
  allow_app_root_kill: false

agents:
  - id: codex-desktop-worker
    label: Codex Desktop Worker
    family: codex
    kind: process
    match:
      process_names:
        - codex
      require_command_regex:
        - "\\bapp-server\\b"
        - "--listen\\s+stdio://"
      command_regex:
        - "\\bapp-server\\b"
        - "--listen\\s+stdio://"

  - id: codex-cli
    label: Codex CLI
    family: codex
    kind: process
    match:
      process_names:
        - codex
      command_regex:
        - "(^|/|\\\\)codex(\\.js|\\.cmd|\\.exe)?(\\s|$)"
      exclude_command_regex:
        - "/Applications/Codex.app"

  - id: claude-code
    label: Claude Code
    family: claude
    kind: process
    match:
      process_names:
        - claude
        - claude-code
      command_regex:
        - "(^|/|\\\\)claude(-code)?(\\.cmd|\\.exe)?(\\s|$)"
      exclude_command_regex:
        - "/Applications/Claude.app"
      exclude_parent_command_regex:
        - "/Applications/Codex\\.app/.+\\bapp-server\\b"

  - id: antigravity-cli
    label: Anti-Gravity CLI
    family: antigravity
    kind: process
    match:
      process_names:
        - agy
      command_regex:
        - "(^|/|\\\\)agy(\\.cmd|\\.exe)?(\\s|$)"

alerts:
  local_notifications: true
  webhook_url: ""
  slack_webhook_url: ""

ledger:
  path: %s
  include_prompt_content: false
`, mode, stateDir, ledgerPath)
}

func defaultProcessAgents() []config.Agent {
	return []config.Agent{
		{
			ID:     "codex-desktop-worker",
			Label:  "Codex Desktop Worker",
			Family: "codex",
			Kind:   config.AgentKindProcess,
			Match: config.Match{
				ProcessNames:        []string{"codex"},
				RequireCommandRegex: []string{"\\bapp-server\\b", "--listen\\s+stdio://"},
				CommandRegex:        []string{"\\bapp-server\\b", "--listen\\s+stdio://"},
			},
		},
		{
			ID:     "codex-cli",
			Label:  "Codex CLI",
			Family: "codex",
			Kind:   config.AgentKindProcess,
			Match: config.Match{
				ProcessNames:        []string{"codex"},
				CommandRegex:        []string{"(^|/|\\\\)codex(\\.js|\\.cmd|\\.exe)?(\\s|$)"},
				ExcludeCommandRegex: []string{"/Applications/Codex.app"},
			},
		},
		{
			ID:     "claude-code",
			Label:  "Claude Code",
			Family: "claude",
			Kind:   config.AgentKindProcess,
			Match: config.Match{
				ProcessNames:        []string{"claude", "claude-code"},
				CommandRegex:        []string{"(^|/|\\\\)claude(-code)?(\\.cmd|\\.exe)?(\\s|$)"},
				ExcludeCommandRegex: []string{"/Applications/Claude.app"},
				ExcludeParentRegex:  []string{"/Applications/Codex\\.app/.+\\bapp-server\\b"},
			},
		},
		{
			ID:     "antigravity-cli",
			Label:  "Anti-Gravity CLI",
			Family: "antigravity",
			Kind:   config.AgentKindProcess,
			Match: config.Match{
				ProcessNames: []string{"agy"},
				CommandRegex: []string{"(^|/|\\\\)agy(\\.cmd|\\.exe)?(\\s|$)"},
			},
		},
	}
}

func usage() {
	fmt.Println(`curb

  curb                  start watching
  curb config           configure warnings and limits
  curb dashboard        show live agents and usage
  curb app              serve and open the local dashboard
  curb daemon           serve the local UI/API on loopback
  curb usage            show local agent token usage
  curb tail             stream local usage events
  curb runs             show active runs
  curb install          install to ~/.local/bin/curb

Advanced commands: curb help advanced`)
}

func usageAdvanced() {
	fmt.Println(`curb advanced commands:
  init              create a user config
  install           install this binary to ~/.local/bin/curb
  config            show or update config
  dashboard         show live agents plus recent usage
  app               serve and open the local dashboard
  daemon|api|serve  serve token-gated local API
  usage             summarize local Codex and Claude usage logs
  tail              stream new usage events
  run|start|watch   run the watchdog loop
  scan              print current process matches once
  validate-config   validate config
  status            print config and active run count
  runs              summarize ledger runs
  ack               legacy run-ledger acknowledgement
  doctor            check local capabilities`)
}

func usageConfig() {
	fmt.Println(`curb config commands:
  curb config                         show current config
  curb config path                    print config path
  curb config aggressive              enforcement, warn 30s, kill 60s
  curb config reasonable              alert-only, warn 90m, kill 120m
  curb config observe                 visibility-only
  curb config set --mode alert --warn-after 5m --kill-after 10m
  curb config set --warn-turn-tokens 1000000 --kill-turn-tokens 3000000`)
}
