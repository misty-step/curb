package main

import (
	"context"
	"encoding/json"
	"flag"
	"fmt"
	"os"
	"os/signal"
	"sort"
	"strings"
	"syscall"
	"time"

	"github.com/phaedrus/curb/internal/config"
	servicepkg "github.com/phaedrus/curb/internal/service"
	usagepkg "github.com/phaedrus/curb/internal/usage"
	"github.com/phaedrus/curb/internal/usagewatch"
)

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
	if err != nil {
		return err
	}
	report := usagepkg.Report{GeneratedAt: time.Now().UTC(), Sources: sources, Sessions: usagepkg.Summarize(events)}
	if *jsonOut {
		return json.NewEncoder(os.Stdout).Encode(report)
	}
	printUsageReport(report, usageSessions(events), cfg, *limit)
	return nil
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
