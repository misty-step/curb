package main

import (
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
	"runtime"
	"strings"
	"sync"
	"syscall"
	"time"

	apipkg "github.com/phaedrus/curb/internal/api"
	"github.com/phaedrus/curb/internal/config"
	servicepkg "github.com/phaedrus/curb/internal/service"
	"github.com/phaedrus/curb/internal/web"
)

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
	service, err := servicepkg.New(*configPath, servicepkg.CaptureFunc(capture))
	if err != nil {
		return err
	}
	snapshot, err := service.SnapshotSince(context.Background(), time.Now().Add(-sinceDuration))
	if err != nil {
		return err
	}
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
