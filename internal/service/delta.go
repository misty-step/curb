package service

import (
	"fmt"
	"time"
)

func annotateOverviewDelta(previous Snapshot, next Snapshot, hasPrevious bool) Snapshot {
	if !hasPrevious {
		next.Overview.Changes = OverviewDelta{}
		return next
	}
	next.Overview.Changes = buildOverviewDelta(previous, next)
	return next
}

func buildOverviewDelta(previous Snapshot, next Snapshot) OverviewDelta {
	previousSessions := map[string]SessionView{}
	for _, session := range previous.Sessions {
		previousSessions[session.Key] = session
	}
	previousTurns := map[string]struct{}{}
	for _, turn := range previous.Turns {
		previousTurns[turnKey(turn)] = struct{}{}
	}
	sessionsWithTurns := map[string]struct{}{}
	delta := OverviewDelta{}
	for _, session := range next.Sessions {
		if _, ok := previousSessions[session.Key]; !ok {
			delta.NewSessions++
		}
		if isAlertingSession(session) && !isAlertingSession(previousSessions[session.Key]) {
			delta.NewAlerts++
		}
	}
	for _, turn := range next.Turns {
		if _, ok := previousTurns[turnKey(turn)]; ok {
			continue
		}
		delta.TokensAdded += turn.TotalTokens
		if turn.SessionKey != "" {
			sessionsWithTurns[turn.SessionKey] = struct{}{}
		}
	}
	delta.SessionsWithNewTurns = len(sessionsWithTurns)

	previousAgents := map[string]struct{}{}
	for _, agent := range previous.Agents {
		previousAgents[agentKey(agent)] = struct{}{}
	}
	nextAgents := map[string]struct{}{}
	for _, agent := range next.Agents {
		key := agentKey(agent)
		nextAgents[key] = struct{}{}
		if _, ok := previousAgents[key]; !ok {
			delta.AgentsStarted++
		}
	}
	for key := range previousAgents {
		if _, ok := nextAgents[key]; !ok {
			delta.AgentsEnded++
		}
	}
	previousSourceErrors := map[string]string{}
	for _, source := range previous.Overview.Sources {
		if source.Error != "" {
			previousSourceErrors[source.Provider] = source.Error
		}
	}
	for _, source := range next.Overview.Sources {
		if source.Error != "" && previousSourceErrors[source.Provider] != source.Error {
			delta.SourceErrors++
		}
	}
	return delta
}

func isAlertingSession(session SessionView) bool {
	return session.UsageState == "warn" || session.UsageState == "stop" || session.State == "warn" || session.State == "stop"
}

func turnKey(turn TurnView) string {
	if turn.ID != "" {
		return turn.Provider + ":" + turn.SessionKey + ":" + turn.ID
	}
	if turn.RequestID != "" {
		return turn.Provider + ":" + turn.SessionKey + ":" + turn.RequestID
	}
	return fmt.Sprintf("%s:%s:%s:%s:%d:%d", turn.Provider, turn.SessionKey, turn.Model, turn.At.UTC().Format(time.RFC3339Nano), turn.TotalTokens, turn.CumulativeTokens)
}

func agentKey(agent AgentView) string {
	started := ""
	if agent.ProcessStartedAt != nil {
		started = agent.ProcessStartedAt.UTC().Format(time.RFC3339Nano)
	}
	return fmt.Sprintf("%s:%d:%s", agent.ID, agent.PID, started)
}
