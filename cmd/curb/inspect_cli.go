package main

import (
	"context"
	"encoding/json"
	"flag"
	"fmt"
	"os"
	"sort"
	"strings"
	"time"

	"github.com/phaedrus/curb/internal/config"
	"github.com/phaedrus/curb/internal/ledger"
	"github.com/phaedrus/curb/internal/usagewatch"
	"github.com/phaedrus/curb/internal/watchdog"
)

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
	if len(active) > 0 {
		var runs []runSummary
		for _, run := range active {
			runs = append(runs, run)
		}
		sort.Slice(runs, func(i, j int) bool { return runs[i].Started.After(runs[j].Started) })
		fmt.Printf("%-16s %-7s %s\n", "AGENT", "PID", "ACTION")
		for _, run := range runs {
			fmt.Printf("%-16s %-7v %s\n", run.AgentID, run.PID, run.Action)
		}
	}
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
	fmt.Printf("%-10s %-16s %-7s %-10s %-8s %-18s %s\n", "STATE", "AGENT", "PID", "RUNTIME", "STARTED", "ACTION", "RUN")
	for _, run := range runs {
		state := "active"
		if run.Ended {
			state = "ended"
		}
		fmt.Printf("%-10s %-16s %-7v %-10s %-8s %-18s %s\n", state, run.AgentID, run.PID, shortDuration(run.Elapsed), run.Started.Local().Format("15:04:05"), run.Action, run.RunID)
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
	Action  string        `json:"action"`
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
		if action := runAction(event.Type); action != "" {
			run.Action = action
		}
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

func runAction(eventType string) string {
	switch eventType {
	case "run_started", "run_heartbeat":
		return "monitor"
	case "policy_warning":
		return "review or ack"
	case "would_terminate":
		return "would stop"
	case "watch_only":
		return "watch-only"
	case "grace_started":
		return "ack now"
	case "termination_started":
		return "stopping"
	case "termination_completed":
		return "stopped"
	case "termination_failed":
		return "review failure"
	case "ack_received":
		return "extended"
	case "run_stopped":
		return "ended"
	default:
		return ""
	}
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
