package service

import (
	"sort"
	"time"

	"github.com/phaedrus/curb/internal/config"
	"github.com/phaedrus/curb/internal/platform"
	"github.com/phaedrus/curb/internal/usage"
	"github.com/phaedrus/curb/internal/usagewatch"
	"github.com/phaedrus/curb/internal/watchdog"
)

type SourceHealth struct {
	Provider string `json:"provider"`
	Files    int    `json:"files"`
	Events   int    `json:"events"`
	Error    string `json:"error,omitempty"`
}

type Overview struct {
	Mode             string               `json:"mode"`
	Action           string               `json:"action"`
	Status           string               `json:"status"`
	Message          string               `json:"message"`
	ActiveAgents     int                  `json:"active_agents"`
	ActiveSessions   int                  `json:"active_sessions"`
	WarningSessions  int                  `json:"warning_sessions"`
	StopSessions     int                  `json:"stop_sessions"`
	IdleHighSessions int                  `json:"idle_high_sessions"`
	WindowTokens     int64                `json:"window_tokens"`
	LookbackTokens   int64                `json:"lookback_tokens"`
	LastScan         time.Time            `json:"last_scan"`
	Sources          []SourceHealth       `json:"sources"`
	Changes          OverviewDelta        `json:"changes"`
	Capabilities     PlatformCapabilities `json:"capabilities"`
}

type OverviewDelta struct {
	NewSessions          int   `json:"new_sessions"`
	SessionsWithNewTurns int   `json:"sessions_with_new_turns"`
	TokensAdded          int64 `json:"tokens_added"`
	NewAlerts            int   `json:"new_alerts"`
	AgentsStarted        int   `json:"agents_started"`
	AgentsEnded          int   `json:"agents_ended"`
	SourceErrors         int   `json:"source_errors"`
}

type AgentView struct {
	ID               string     `json:"id"`
	Provider         string     `json:"provider"`
	Label            string     `json:"label"`
	State            string     `json:"state"`
	ProcessState     string     `json:"process_state"`
	UsageState       string     `json:"usage_state,omitempty"`
	ActionState      string     `json:"action_state"`
	Actionable       bool       `json:"actionable"`
	PID              int32      `json:"pid"`
	ProcessStartedAt *time.Time `json:"process_started_at,omitempty"`
	RunningFor       int64      `json:"running_for_seconds,omitempty"`
	Project          string     `json:"project,omitempty"`
	CWD              string     `json:"cwd,omitempty"`
	MatchedBy        []string   `json:"matched_by,omitempty"`
	Confidence       int        `json:"confidence"`
	CPU              float64    `json:"cpu_percent,omitempty"`
	LatestSessionID  string     `json:"latest_session_id,omitempty"`
	LatestTurnTokens int64      `json:"latest_turn_tokens,omitempty"`
	WindowTokens     int64      `json:"window_tokens,omitempty"`
	Explanation      string     `json:"explanation"`
}

type SessionView struct {
	Key               string     `json:"key"`
	ID                string     `json:"id,omitempty"`
	Provider          string     `json:"provider"`
	State             string     `json:"state"`
	ProcessState      string     `json:"process_state"`
	UsageState        string     `json:"usage_state,omitempty"`
	ActionState       string     `json:"action_state"`
	AgentState        string     `json:"agent_state,omitempty"`
	Actionable        bool       `json:"actionable"`
	CanAcknowledge    bool       `json:"can_acknowledge"`
	Project           string     `json:"project,omitempty"`
	CWD               string     `json:"cwd,omitempty"`
	Models            []string   `json:"models,omitempty"`
	LastSeenAt        time.Time  `json:"last_seen_at"`
	LastUsageAt       *time.Time `json:"last_usage_at,omitempty"`
	Calls             int        `json:"calls"`
	LatestTurnTokens  int64      `json:"latest_turn_tokens,omitempty"`
	WindowTokens      int64      `json:"window_tokens,omitempty"`
	TotalTokens       int64      `json:"total_tokens"`
	CorrelatedAgentID string     `json:"correlated_agent_id,omitempty"`
	CorrelatedPID     int32      `json:"correlated_pid,omitempty"`
	CorrelatedStarted *time.Time `json:"correlated_process_started_at,omitempty"`
	CorrelatedOwner   string     `json:"correlated_owner,omitempty"`
	CorrelatedExe     string     `json:"correlated_executable,omitempty"`
	CorrelatedBundle  string     `json:"correlated_bundle_id,omitempty"`
	CorrelatedTeam    string     `json:"correlated_team_id,omitempty"`
	CorrelationReason string     `json:"correlation_reason,omitempty"`
	CorrelationScore  int        `json:"correlation_score,omitempty"`
	Confidence        int        `json:"confidence,omitempty"`
	MatchedBy         []string   `json:"matched_by,omitempty"`
	RiskRank          int        `json:"risk_rank"`
	Acknowledged      bool       `json:"acknowledged"`
	AcknowledgedUntil *time.Time `json:"acknowledged_until,omitempty"`
	Explanation       string     `json:"explanation"`
}

type TurnView struct {
	ID                string    `json:"id,omitempty"`
	RequestID         string    `json:"request_id,omitempty"`
	SessionKey        string    `json:"session_key,omitempty"`
	SessionID         string    `json:"session_id,omitempty"`
	Provider          string    `json:"provider"`
	At                time.Time `json:"at"`
	Model             string    `json:"model,omitempty"`
	InputTokens       int64     `json:"input_tokens,omitempty"`
	CachedInputTokens int64     `json:"cached_input_tokens,omitempty"`
	OutputTokens      int64     `json:"output_tokens,omitempty"`
	CacheCreation     int64     `json:"cache_creation_input_tokens,omitempty"`
	ReasoningTokens   int64     `json:"reasoning_output_tokens,omitempty"`
	TotalTokens       int64     `json:"total_tokens,omitempty"`
	CumulativeTokens  int64     `json:"cumulative_tokens,omitempty"`
	Source            string    `json:"source,omitempty"`
}

type TurnQuery struct {
	Since time.Time
	Limit int
}

type Snapshot struct {
	Overview Overview      `json:"overview"`
	Agents   []AgentView   `json:"agents"`
	Sessions []SessionView `json:"sessions"`
	Turns    []TurnView    `json:"turns"`
}

func BuildSnapshot(cfg *config.Config, snap *platform.Snapshot, events []usage.Event, sources []usage.SourceReport, now time.Time) Snapshot {
	if now.IsZero() {
		now = time.Now()
	}
	matches := watchdog.New(cfg, nil).Match(snap)
	sessions := usagewatch.BuildSessions(events)
	windowStart := now.Add(-cfg.Usage.Window.Duration)
	turnsBySession := buildTurnsSince(events, windowStart)

	sessionViews := make([]SessionView, 0, len(sessions))
	correlations := map[string]usagewatch.Correlation{}
	for _, session := range sessions {
		correlation := usagewatch.Correlate(session, matches)
		correlations[session.Key] = correlation
		view := buildSessionView(cfg, session, correlation, turnsBySession[session.Key], now)
		sessionViews = append(sessionViews, view)
	}
	sortSessionViews(sessionViews)

	agentViews := make([]AgentView, 0, len(matches))
	for _, match := range matches {
		session, found := usagewatch.BestSessionForMatch(match, sessions)
		var sessionView SessionView
		if found {
			sessionView = buildSessionView(cfg, session, correlations[session.Key], turnsBySession[session.Key], now)
		}
		agentViews = append(agentViews, buildAgentView(match, sessionView, found, now))
	}
	sort.Slice(agentViews, func(i, j int) bool {
		if agentViews[i].State == agentViews[j].State {
			if agentViews[i].Project == agentViews[j].Project {
				return agentViews[i].ID < agentViews[j].ID
			}
			return agentViews[i].Project < agentViews[j].Project
		}
		priority := map[string]int{"stop": 0, "warn": 1, "spending": 2, "running": 3, "watch-only": 4, "idle": 5}
		return priority[agentViews[i].State] < priority[agentViews[j].State]
	})

	allTurns := flattenTurns(turnsBySession)
	out := Snapshot{
		Overview: buildOverview(cfg, agentViews, sessionViews, sources, now),
		Agents:   agentViews,
		Sessions: sessionViews,
		Turns:    allTurns,
	}
	return out
}

func buildOverview(cfg *config.Config, agents []AgentView, sessions []SessionView, sources []usage.SourceReport, now time.Time) Overview {
	overview := Overview{
		Mode:         string(cfg.Mode),
		Action:       actionLabel(cfg.Mode),
		Status:       "OK",
		Message:      "no active over-budget usage",
		ActiveAgents: len(agents),
		LastScan:     now.UTC(),
		Sources:      []SourceHealth{},
	}
	for _, source := range sources {
		overview.Sources = append(overview.Sources, SourceHealth(source))
	}
	for _, session := range sessions {
		overview.LookbackTokens += session.TotalTokens
		if session.State == "idle-high" {
			overview.IdleHighSessions++
		}
		if session.ProcessState == "running" && (session.State == "active" || session.State == "warn" || session.State == "stop" || session.State == "watch-only" || session.State == "acknowledged") {
			overview.ActiveSessions++
			overview.WindowTokens += session.WindowTokens
		}
		switch session.State {
		case "stop":
			if session.Actionable {
				overview.StopSessions++
			} else {
				overview.WarningSessions++
			}
		case "warn":
			overview.WarningSessions++
		case "uncorrelated", "watch-only":
			if session.UsageState == "warn" || session.UsageState == "stop" {
				overview.WarningSessions++
			}
		}
	}
	switch {
	case overview.StopSessions > 0:
		overview.Status = "ACTION"
		overview.Message = "active usage is over a stop threshold"
	case overview.WarningSessions > 0:
		overview.Status = "WATCH"
		overview.Message = "active usage is over a warning threshold"
	case overview.ActiveSessions > 0:
		overview.Status = "ACTIVE"
		overview.Message = "recent usage is within policy"
	}
	return overview
}

func sortSessionViews(sessions []SessionView) {
	sort.Slice(sessions, func(i, j int) bool {
		left, right := sessions[i].RiskRank, sessions[j].RiskRank
		if left != right {
			return left < right
		}
		if sessions[i].LatestTurnTokens != sessions[j].LatestTurnTokens {
			return sessions[i].LatestTurnTokens > sessions[j].LatestTurnTokens
		}
		if sessions[i].WindowTokens != sessions[j].WindowTokens {
			return sessions[i].WindowTokens > sessions[j].WindowTokens
		}
		return sessions[i].LastSeenAt.After(sessions[j].LastSeenAt)
	})
}

func buildSessionView(cfg *config.Config, session usagewatch.Session, correlation usagewatch.Correlation, turns []TurnView, now time.Time) SessionView {
	windowTokens := int64(0)
	for _, turn := range turns {
		windowTokens += turn.TotalTokens
	}
	decision := usagewatch.EvaluateSessionDecision(session, cfg, correlation, now)
	ackUntil := activeAckUntil(cfg.Service.StateDir, session.Key, now)
	classification := usagewatch.ClassifySession(decision, correlation, cfg.Mode, ackUntil, cfg.Defaults.AckExtension.Duration)
	view := SessionView{
		Key:               session.Key,
		ID:                session.SessionID,
		Provider:          session.Provider,
		State:             classification.State,
		AgentState:        classification.AgentState,
		ProcessState:      classification.ProcessState,
		UsageState:        classification.UsageState,
		ActionState:       classification.ActionState,
		Actionable:        classification.Actionable,
		CanAcknowledge:    classification.CanAcknowledge,
		Project:           projectName(session.CWD),
		CWD:               session.CWD,
		Models:            append([]string(nil), session.Models...),
		LastSeenAt:        session.Last.UTC(),
		LastUsageAt:       timePtr(session.LastUsage),
		Calls:             session.Events,
		LatestTurnTokens:  session.LastTurnTokens,
		WindowTokens:      windowTokens,
		TotalTokens:       session.Total,
		RiskRank:          classification.RiskRank,
		Acknowledged:      ackUntil != nil,
		AcknowledgedUntil: ackUntil,
		Explanation:       classification.Explanation,
	}
	if correlation.Matched {
		view.CorrelatedAgentID = correlation.Agent.ID
		view.CorrelatedPID = correlation.Process.PID
		view.CorrelatedStarted = timePtr(correlation.Process.Create)
		view.CorrelatedOwner = correlation.Process.Username
		view.CorrelatedExe = correlation.Process.Exe
		view.CorrelatedBundle = correlation.Process.BundleID
		view.CorrelatedTeam = correlation.Process.TeamID
		view.CorrelationReason = correlation.Reason
		view.CorrelationScore = correlation.Score
		view.Confidence = correlation.Confidence
		view.MatchedBy = append([]string(nil), correlation.Evidence...)
	}
	return view
}

func activeAckUntil(stateDir, sessionKey string, now time.Time) *time.Time {
	ack, ok, err := usagewatch.ActiveSessionAck(stateDir, sessionKey, now)
	if err != nil || !ok {
		return nil
	}
	until := ack.Until.UTC()
	return &until
}

func buildAgentView(match watchdog.Match, session SessionView, found bool, now time.Time) AgentView {
	state := "running"
	explanation := "process is running with no correlated usage"
	actionable := false
	if !match.Agent.TerminationAllowed() {
		state = "watch-only"
		explanation = "matched agent is watch-only"
	}
	if found {
		if !match.Agent.TerminationAllowed() {
			state = "watch-only"
			explanation = session.Explanation
		} else {
			state = session.AgentState
			if state == "" {
				state = session.State
			}
			actionable = session.Actionable
			explanation = session.Explanation
			if state == "idle" {
				explanation = "process is running; correlated session is not currently spending"
			}
		}
	}
	var started *time.Time
	runningFor := int64(0)
	if match.Process.StartedOK {
		started = timePtr(match.Process.Create)
		runningFor = int64(now.Sub(match.Process.Create).Seconds())
		if runningFor < 0 {
			runningFor = 0
		}
	}
	view := AgentView{
		ID:               match.Agent.ID,
		Provider:         match.Agent.Family,
		Label:            match.Agent.Label,
		State:            state,
		ProcessState:     agentProcessState(match.Agent.TerminationAllowed()),
		UsageState:       "quiet",
		ActionState:      "none",
		Actionable:       actionable,
		PID:              match.Process.PID,
		ProcessStartedAt: started,
		RunningFor:       runningFor,
		Project:          projectName(match.Process.CWD),
		CWD:              match.Process.CWD,
		MatchedBy:        append([]string(nil), match.Evidence...),
		Confidence:       match.Confidence,
		CPU:              match.Process.CPU,
		Explanation:      explanation,
	}
	if found {
		view.LatestSessionID = session.ID
		view.UsageState = session.UsageState
		view.ActionState = session.ActionState
		view.Actionable = session.Actionable
		view.LatestTurnTokens = session.LatestTurnTokens
		view.WindowTokens = session.WindowTokens
	}
	return view
}

func agentProcessState(terminates bool) string {
	if !terminates {
		return "watch-only"
	}
	return "running"
}

func buildTurnsSince(events []usage.Event, since time.Time) map[string][]TurnView {
	out := map[string][]TurnView{}
	for _, event := range events {
		if event.Total <= 0 || event.Timestamp.Before(since) {
			continue
		}
		key := usagewatch.SessionKey(event.Provider, event.SessionID, event.SourcePath)
		out[key] = append(out[key], TurnView{
			ID:                event.TurnID,
			RequestID:         event.RequestID,
			SessionKey:        key,
			SessionID:         event.SessionID,
			Provider:          event.Provider,
			At:                event.Timestamp.UTC(),
			Model:             event.Model,
			InputTokens:       event.Input,
			CachedInputTokens: event.CachedInput,
			CacheCreation:     event.CacheCreation,
			OutputTokens:      event.Output,
			ReasoningTokens:   event.Reasoning,
			TotalTokens:       event.Total,
			CumulativeTokens:  event.Cumulative,
			Source:            event.Source,
		})
	}
	for key := range out {
		sort.Slice(out[key], func(i, j int) bool { return out[key][i].At.Before(out[key][j].At) })
	}
	return out
}

func limitTurns(turns []TurnView, limit int) []TurnView {
	if limit <= 0 || len(turns) <= limit {
		return turns
	}
	return turns[len(turns)-limit:]
}

func flattenTurns(turnsBySession map[string][]TurnView) []TurnView {
	out := []TurnView{}
	for _, turns := range turnsBySession {
		out = append(out, turns...)
	}
	sort.Slice(out, func(i, j int) bool { return out[i].At.After(out[j].At) })
	return out
}

func projectName(cwd string) string {
	if cwd == "" {
		return ""
	}
	cleaned := platformClean(cwd)
	base := cleaned
	for i := len(cleaned) - 1; i >= 0; i-- {
		if cleaned[i] == '/' || cleaned[i] == '\\' {
			base = cleaned[i+1:]
			break
		}
	}
	return base
}

func timePtr(t time.Time) *time.Time {
	if t.IsZero() {
		return nil
	}
	u := t.UTC()
	return &u
}

func platformClean(path string) string {
	for len(path) > 1 && (path[len(path)-1] == '/' || path[len(path)-1] == '\\') {
		path = path[:len(path)-1]
	}
	return path
}

func actionLabel(mode config.Mode) string {
	switch mode {
	case config.ModeVisibility:
		return "record only; no warnings or kills"
	case config.ModeAlert:
		return "notify only; never kill"
	case config.ModeEnforcement:
		return "enforcement enabled"
	default:
		return string(mode)
	}
}
