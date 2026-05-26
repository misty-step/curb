package main

import (
	"context"
	"errors"
	"flag"
	"fmt"
	"os"
	"os/signal"
	"syscall"

	"github.com/phaedrus/curb/internal/config"
	"github.com/phaedrus/curb/internal/ledger"
	servicepkg "github.com/phaedrus/curb/internal/service"
)

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
