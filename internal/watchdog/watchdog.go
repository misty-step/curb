package watchdog

import (
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"runtime"
	"sort"
	"strings"
	"time"

	"github.com/phaedrus/curb/internal/config"
	"github.com/phaedrus/curb/internal/ledger"
	"github.com/phaedrus/curb/internal/platform"
)

type Service struct {
	cfg       *config.Config
	ledger    *ledger.Ledger
	runs      map[string]*Run
	warned    map[string]bool
	lastBeat  map[string]time.Time
	capture   func(context.Context) (*platform.Snapshot, error)
	notify    func(string, string) error
	terminate func(context.Context, platform.TerminationTarget, time.Duration) platform.TerminationResult
	newRunID  func() string
	now       func() time.Time
	onEvent   func(ledger.Event)
}

type Run struct {
	ID             string
	Agent          config.Agent
	Policy         config.Policy
	Root           platform.Process
	StartedAt      time.Time
	LastSeenAt     time.Time
	LastActivityAt time.Time
	State          string
	Extensions     int
	ExtendedBy     time.Duration
	WarningEmitted bool
	GraceStartedAt *time.Time
	Terminated     bool
}

func (s *Service) OnEvent(fn func(ledger.Event)) {
	s.onEvent = fn
}

func (s *Service) append(event ledger.Event) error {
	if err := s.ledger.Append(event); err != nil {
		return err
	}
	if s.onEvent != nil {
		s.onEvent(event)
	}
	return nil
}

type Match struct {
	Agent      config.Agent
	Policy     config.Policy
	Process    platform.Process
	Confidence int
	Evidence   []string
}

func New(cfg *config.Config, l *ledger.Ledger) *Service {
	return &Service{
		cfg:       cfg,
		ledger:    l,
		runs:      map[string]*Run{},
		warned:    map[string]bool{},
		lastBeat:  map[string]time.Time{},
		capture:   platform.Capture,
		notify:    platform.Notify,
		terminate: platform.TerminateTree,
		newRunID:  newRunID,
		now:       time.Now,
	}
}

func (s *Service) Run(ctx context.Context) error {
	if err := s.append(ledger.Event{
		Type: "service_started",
		Mode: string(s.cfg.Mode),
		Data: map[string]any{
			"profile":  s.cfg.Profile,
			"platform": runtime.GOOS,
		},
	}); err != nil {
		return err
	}

	ticker := time.NewTicker(s.cfg.Service.ScanInterval.Duration)
	defer ticker.Stop()
	if err := s.Scan(ctx); err != nil {
		return err
	}
	for {
		select {
		case <-ctx.Done():
			_ = s.append(ledger.Event{Type: "service_stopped", Mode: string(s.cfg.Mode), Message: ctx.Err().Error()})
			return nil
		case <-ticker.C:
			if err := s.Scan(ctx); err != nil {
				_ = s.append(ledger.Event{Type: "scan_failed", Mode: string(s.cfg.Mode), Message: err.Error()})
			}
		}
	}
}

func (s *Service) Scan(ctx context.Context) error {
	snap, err := s.capture(ctx)
	if err != nil {
		return err
	}
	matches := s.Match(snap)
	activeKeys := map[string]bool{}
	now := s.now()
	for _, match := range matches {
		key := runKey(match)
		activeKeys[key] = true
		run := s.runs[key]
		if run == nil {
			if match.Process.StartedOK && now.Sub(match.Process.Create) < match.Policy.MinLifetime.Duration {
				continue
			}
			run = &Run{
				ID:             s.newRunID(),
				Agent:          match.Agent,
				Policy:         match.Policy,
				Root:           match.Process,
				StartedAt:      runStart(match.Process, now),
				LastSeenAt:     now,
				LastActivityAt: now,
				State:          "active",
			}
			s.runs[key] = run
			if err := s.append(ledger.Event{
				Type:    "run_started",
				RunID:   run.ID,
				AgentID: run.Agent.ID,
				Mode:    string(s.cfg.Mode),
				Data: map[string]any{
					"pid":         run.Root.PID,
					"ppid":        run.Root.PPID,
					"name":        run.Root.Name,
					"exe":         run.Root.Exe,
					"cmdline":     run.Root.Cmdline,
					"bundle_id":   run.Root.BundleID,
					"team_id":     run.Root.TeamID,
					"confidence":  match.Confidence,
					"evidence":    match.Evidence,
					"started_at":  run.StartedAt.UTC(),
					"agent_label": run.Agent.Label,
				},
			}); err != nil {
				return err
			}
		}
		run.LastSeenAt = now
		run.Root = match.Process
		if match.Process.CPU > 0 || len(snap.Descendants(match.Process.PID)) > 0 {
			run.LastActivityAt = now
		}
		s.applyAcks(run)
		if err := s.evaluate(ctx, snap, run, now); err != nil {
			return err
		}
		s.heartbeat(run, snap, now)
	}

	for key, run := range s.runs {
		if activeKeys[key] || run.Terminated {
			continue
		}
		if now.Sub(run.LastSeenAt) < run.Policy.MaxRunGap.Duration {
			continue
		}
		run.State = "stopped"
		run.Terminated = true
		if err := s.append(ledger.Event{
			Type:    "run_stopped",
			RunID:   run.ID,
			AgentID: run.Agent.ID,
			Mode:    string(s.cfg.Mode),
			Data: map[string]any{
				"elapsed_seconds": int(now.Sub(run.StartedAt).Seconds()),
			},
		}); err != nil {
			return err
		}
	}
	return nil
}

func (s *Service) Match(snap *platform.Snapshot) []Match {
	var matches []Match
	for _, proc := range snap.Processes {
		for _, agent := range s.cfg.Agents {
			confidence, evidence := score(agent.Match, proc, snap)
			if confidence < s.cfg.Service.MinConfidence {
				continue
			}
			matches = append(matches, Match{
				Agent:      agent,
				Policy:     s.cfg.PolicyFor(agent),
				Process:    proc,
				Confidence: confidence,
				Evidence:   evidence,
			})
		}
	}
	sort.Slice(matches, func(i, j int) bool {
		if matches[i].Confidence == matches[j].Confidence {
			return matches[i].Process.PID < matches[j].Process.PID
		}
		return matches[i].Confidence > matches[j].Confidence
	})
	return matches
}

func (s *Service) evaluate(ctx context.Context, snap *platform.Snapshot, run *Run, now time.Time) error {
	elapsed := now.Sub(run.StartedAt) - run.ExtendedBy
	if elapsed >= run.Policy.WarnAfter.Duration && !run.WarningEmitted {
		run.WarningEmitted = true
		run.State = "warned"
		msg := fmt.Sprintf("%s has been active for %s; kill threshold is %s", run.Agent.Label, elapsed.Round(time.Second), run.Policy.KillAfter.Duration)
		if s.cfg.Alerts.LocalNotifications {
			if err := s.notify("Curb warning", msg); err != nil {
				_ = s.append(ledger.Event{Type: "notification_failed", RunID: run.ID, AgentID: run.Agent.ID, Mode: string(s.cfg.Mode), Message: err.Error()})
			}
		}
		if err := s.append(ledger.Event{
			Type:    "policy_warning",
			RunID:   run.ID,
			AgentID: run.Agent.ID,
			Mode:    string(s.cfg.Mode),
			Message: msg,
			Data: map[string]any{
				"elapsed_seconds": int(elapsed.Seconds()),
				"warn_after":      run.Policy.WarnAfter.String(),
				"kill_after":      run.Policy.KillAfter.String(),
			},
		}); err != nil {
			return err
		}
	}

	if elapsed < run.Policy.KillAfter.Duration || run.Terminated {
		return nil
	}
	if !run.Agent.TerminationAllowed() {
		if !s.warned["watch-only:"+run.ID] {
			s.warned["watch-only:"+run.ID] = true
			msg := fmt.Sprintf("%s is watch-only; Curb will not terminate desktop apps", run.Agent.Label)
			if s.cfg.Alerts.LocalNotifications {
				if err := s.notify("Curb watch-only", msg); err != nil {
					_ = s.append(ledger.Event{Type: "notification_failed", RunID: run.ID, AgentID: run.Agent.ID, Mode: string(s.cfg.Mode), Message: err.Error()})
				}
			}
			return s.append(ledger.Event{
				Type:    "watch_only",
				RunID:   run.ID,
				AgentID: run.Agent.ID,
				Mode:    string(s.cfg.Mode),
				Message: msg,
				Data: map[string]any{
					"pid":             run.Root.PID,
					"elapsed_seconds": int(elapsed.Seconds()),
				},
			})
		}
		return nil
	}
	if s.cfg.Mode != config.ModeEnforcement {
		if !s.warned["would:"+run.ID] {
			s.warned["would:"+run.ID] = true
			msg := fmt.Sprintf("%s exceeded %s; Curb would terminate in enforcement mode", run.Agent.Label, run.Policy.KillAfter.Duration)
			if s.cfg.Alerts.LocalNotifications {
				if err := s.notify("Curb would terminate", msg); err != nil {
					_ = s.append(ledger.Event{Type: "notification_failed", RunID: run.ID, AgentID: run.Agent.ID, Mode: string(s.cfg.Mode), Message: err.Error()})
				}
			}
			return s.append(ledger.Event{
				Type:    "would_terminate",
				RunID:   run.ID,
				AgentID: run.Agent.ID,
				Mode:    string(s.cfg.Mode),
				Message: msg,
				Data: map[string]any{
					"pid":             run.Root.PID,
					"elapsed_seconds": int(elapsed.Seconds()),
				},
			})
		}
		return nil
	}

	if run.GraceStartedAt == nil {
		start := now
		run.GraceStartedAt = &start
		run.State = "grace"
		msg := fmt.Sprintf("%s will be terminated in %s unless acknowledged", run.Agent.Label, run.Policy.KillGracePeriod.Duration)
		if s.cfg.Alerts.LocalNotifications {
			if err := s.notify("Curb grace period", msg); err != nil {
				_ = s.append(ledger.Event{Type: "notification_failed", RunID: run.ID, AgentID: run.Agent.ID, Mode: string(s.cfg.Mode), Message: err.Error()})
			}
		}
		return s.append(ledger.Event{
			Type:    "grace_started",
			RunID:   run.ID,
			AgentID: run.Agent.ID,
			Mode:    string(s.cfg.Mode),
			Message: msg,
			Data: map[string]any{
				"pid":          run.Root.PID,
				"grace_period": run.Policy.KillGracePeriod.String(),
			},
		})
	}
	if now.Sub(*run.GraceStartedAt) < run.Policy.KillGracePeriod.Duration {
		return nil
	}

	target, ok := snap.TerminationTarget(run.Root)
	if !ok {
		run.State = "error"
		run.Terminated = true
		return s.append(ledger.Event{
			Type:    "termination_failed",
			RunID:   run.ID,
			AgentID: run.Agent.ID,
			Mode:    string(s.cfg.Mode),
			Message: "safety guard rejected termination",
		})
	}

	run.State = "terminating"
	if err := s.append(ledger.Event{Type: "termination_started", RunID: run.ID, AgentID: run.Agent.ID, Mode: string(s.cfg.Mode), Data: map[string]any{"pid": run.Root.PID}}); err != nil {
		return err
	}
	result := s.terminate(ctx, target, run.Policy.KillGracePeriod.Duration)
	run.Terminated = true
	run.State = "terminated"
	msg := fmt.Sprintf("%s was terminated after exceeding %s", run.Agent.Label, run.Policy.KillAfter.Duration)
	if s.cfg.Alerts.LocalNotifications {
		if err := s.notify("Curb terminated agent", msg); err != nil {
			_ = s.append(ledger.Event{Type: "notification_failed", RunID: run.ID, AgentID: run.Agent.ID, Mode: string(s.cfg.Mode), Message: err.Error()})
		}
	}
	return s.append(ledger.Event{
		Type:    "termination_completed",
		RunID:   run.ID,
		AgentID: run.Agent.ID,
		Mode:    string(s.cfg.Mode),
		Message: msg,
		Data: map[string]any{
			"result": result,
		},
	})
}

func (s *Service) heartbeat(run *Run, snap *platform.Snapshot, now time.Time) {
	last := s.lastBeat[run.ID]
	if !last.IsZero() && now.Sub(last) < s.cfg.Service.HeartbeatInterval.Duration {
		return
	}
	s.lastBeat[run.ID] = now
	_ = s.append(ledger.Event{
		Type:    "run_heartbeat",
		RunID:   run.ID,
		AgentID: run.Agent.ID,
		Mode:    string(s.cfg.Mode),
		Data: map[string]any{
			"pid":             run.Root.PID,
			"tree":            snap.Tree(run.Root.PID),
			"elapsed_seconds": int(now.Sub(run.StartedAt).Seconds()),
			"state":           run.State,
		},
	})
}

func (s *Service) applyAcks(run *Run) {
	path := filepath.Join(s.cfg.Service.StateDir, "acks", run.ID+".json")
	content, err := os.ReadFile(path)
	if err != nil {
		return
	}
	var ack struct {
		Extend string `json:"extend"`
		Reason string `json:"reason"`
	}
	if err := json.Unmarshal(content, &ack); err != nil {
		return
	}
	_ = os.Remove(path)
	extend, err := time.ParseDuration(ack.Extend)
	if err != nil || extend <= 0 {
		return
	}
	if extend > run.Policy.AckExtension.Duration {
		extend = run.Policy.AckExtension.Duration
	}
	if run.Extensions >= run.Policy.MaxExtensions {
		_ = s.append(ledger.Event{Type: "ack_rejected", RunID: run.ID, AgentID: run.Agent.ID, Mode: string(s.cfg.Mode), Message: "extension budget exhausted"})
		return
	}
	run.Extensions++
	run.ExtendedBy += extend
	run.WarningEmitted = false
	run.GraceStartedAt = nil
	run.State = "active"
	_ = s.append(ledger.Event{
		Type:    "ack_received",
		RunID:   run.ID,
		AgentID: run.Agent.ID,
		Mode:    string(s.cfg.Mode),
		Message: ack.Reason,
		Data: map[string]any{
			"extend":     extend.String(),
			"extensions": run.Extensions,
		},
	})
}

func WriteAck(stateDir, runID, extend, reason string) error {
	if runID == "" {
		return fmt.Errorf("run id is required")
	}
	if _, err := time.ParseDuration(extend); err != nil {
		return err
	}
	dir := filepath.Join(stateDir, "acks")
	if err := os.MkdirAll(dir, 0o700); err != nil {
		return err
	}
	content, err := json.MarshalIndent(map[string]string{"extend": extend, "reason": reason}, "", "  ")
	if err != nil {
		return err
	}
	return os.WriteFile(filepath.Join(dir, runID+".json"), content, 0o600)
}

func score(match config.Match, proc platform.Process, snap *platform.Snapshot) (int, []string) {
	if !proc.HasSemanticIdentity() {
		return 0, nil
	}
	if containsFold(match.ExcludeNames, proc.Name) {
		return 0, nil
	}
	for _, raw := range match.ExcludeCommandRegex {
		re := regexp.MustCompile(raw)
		if re.MatchString(proc.Cmdline) || re.MatchString(proc.Exe) {
			return 0, nil
		}
	}
	if parent, ok := snap.Processes[proc.PPID]; ok {
		for _, raw := range match.ExcludeParentRegex {
			re := regexp.MustCompile(raw)
			if re.MatchString(parent.Cmdline) || re.MatchString(parent.Exe) {
				return 0, nil
			}
		}
	}
	for _, raw := range match.RequireCommandRegex {
		if !regexp.MustCompile(raw).MatchString(proc.Cmdline) {
			return 0, nil
		}
	}
	score := 0
	var evidence []string
	add := func(points int, label string) {
		score += points
		evidence = append(evidence, label)
	}
	if containsFold(match.BundleIDs, proc.BundleID) {
		add(100, "bundle_id:"+proc.BundleID)
	}
	for _, sig := range match.CodeSignatures {
		if sig.Identifier != "" && strings.EqualFold(sig.Identifier, proc.BundleID) && (sig.TeamID == "" || strings.EqualFold(sig.TeamID, proc.TeamID)) {
			add(100, "code_signature:"+sig.Identifier)
		}
	}
	if pathMatches(match.AppPaths, proc.Exe) || pathMatches(match.ExecutablePaths, proc.Exe) || pathMatches(match.WindowsPaths, proc.Exe) || pathMatches(match.LinuxPaths, proc.Exe) {
		add(90, "path:"+proc.Exe)
	}
	if containsFold(match.ProcessNames, proc.Name) {
		add(60, "process_name:"+proc.Name)
	}
	for _, raw := range match.CommandRegex {
		re := regexp.MustCompile(raw)
		if re.MatchString(proc.Cmdline) {
			add(55, "command_regex:"+raw)
		}
	}
	if parent, ok := snap.Processes[proc.PPID]; ok && containsFold(match.ParentProcessNames, parent.Name) {
		add(45, "parent_process_name:"+parent.Name)
	}
	return score, evidence
}

func pathMatches(patterns []string, path string) bool {
	for _, pattern := range patterns {
		if pattern == "" {
			continue
		}
		if strings.EqualFold(path, pattern) || strings.HasPrefix(strings.ToLower(path), strings.ToLower(pattern)) {
			return true
		}
	}
	return false
}

func containsFold(values []string, needle string) bool {
	if needle == "" {
		return false
	}
	for _, value := range values {
		if strings.EqualFold(value, needle) {
			return true
		}
	}
	return false
}

func runKey(match Match) string {
	return fmt.Sprintf("%s:%d:%d", match.Agent.ID, match.Process.PID, match.Process.Create.UnixNano())
}

func runStart(proc platform.Process, fallback time.Time) time.Time {
	if proc.StartedOK {
		return proc.Create
	}
	return fallback
}

func safeToTerminate(snap *platform.Snapshot, run *Run) bool {
	return snap.SafeToTerminate(run.Root)
}

func newRunID() string {
	var b [10]byte
	if _, err := rand.Read(b[:]); err != nil {
		return fmt.Sprintf("run_%d", time.Now().UnixNano())
	}
	return "run_" + hex.EncodeToString(b[:])
}
