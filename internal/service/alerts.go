package service

import (
	"context"
	"errors"
	"strings"
	"time"

	"github.com/phaedrus/curb/internal/ledger"
)

type AlertView struct {
	Severity    string    `json:"severity"`
	Label       string    `json:"label"`
	Category    string    `json:"category"`
	Message     string    `json:"message"`
	At          time.Time `json:"at"`
	Seq         int64     `json:"seq"`
	RunID       string    `json:"run_id,omitempty"`
	AgentID     string    `json:"agent_id,omitempty"`
	Provider    string    `json:"provider,omitempty"`
	Mode        string    `json:"mode,omitempty"`
	CWD         string    `json:"cwd,omitempty"`
	SessionKey  string    `json:"session_key,omitempty"`
	SessionID   string    `json:"session_id,omitempty"`
	Actionable  bool      `json:"actionable"`
	CanAck      bool      `json:"can_acknowledge"`
	Explanation string    `json:"explanation"`
}

func (s *Service) Alerts(ctx context.Context, limit int) ([]AlertView, error) {
	events, err := s.recentLedgerEvents(0)
	if err != nil {
		return nil, err
	}
	sessionByProviderID := map[string]SessionView{}
	snapshot, snapshotErr := s.Snapshot(ctx)
	if errors.Is(snapshotErr, ErrSnapshotUnavailable) {
		if err := s.Refresh(ctx); err == nil {
			snapshot, snapshotErr = s.Snapshot(ctx)
		}
	}
	if snapshotErr == nil {
		for _, session := range snapshot.Sessions {
			if session.Provider != "" && session.ID != "" {
				sessionByProviderID[session.Provider+"\x00"+session.ID] = session
			}
		}
	}
	if limit <= 0 {
		limit = len(events)
	}
	alerts := []AlertView{}
	for i := len(events) - 1; i >= 0 && len(alerts) < limit; i-- {
		event := events[i]
		alert, ok := newAlertView(event)
		if ok {
			projectAlertAction(&alert, sessionByProviderID)
			alerts = append(alerts, alert)
		}
	}
	for i, j := 0, len(alerts)-1; i < j; i, j = i+1, j-1 {
		alerts[i], alerts[j] = alerts[j], alerts[i]
	}
	return alerts, nil
}

func newAlertView(event ledger.Event) (AlertView, bool) {
	if !alertEvent(event.Type) {
		return AlertView{}, false
	}
	alert := AlertView{
		Severity:    alertSeverity(event),
		Label:       alertLabel(event.Type),
		Category:    alertCategory(event.Type),
		Message:     event.Message,
		At:          event.Time,
		Seq:         event.Seq,
		RunID:       event.RunID,
		AgentID:     event.AgentID,
		Provider:    stringData(event, "provider"),
		Mode:        event.Mode,
		CWD:         stringData(event, "cwd"),
		SessionID:   stringData(event, "session_id"),
		Actionable:  actionableEvent(event),
		Explanation: alertExplanation(event),
	}
	if alert.Message == "" {
		alert.Message = defaultAlertMessage(alert.Category)
	}
	return alert, true
}

func defaultAlertMessage(category string) string {
	switch category {
	case "stopped":
		return "Curb stopped a correlated worker."
	case "grace":
		return "Curb started an enforcement grace period."
	case "would_stop":
		return "Curb would stop a correlated worker in enforcement mode."
	case "blocked":
		return "Curb blocked termination for an uncorrelated or protected process."
	case "failed":
		return "Curb could not complete a policy action."
	default:
		return "Usage or runtime crossed policy."
	}
}

func alertCategory(eventType string) string {
	switch {
	case strings.Contains(eventType, "completed"):
		return "stopped"
	case strings.Contains(eventType, "started") || strings.Contains(eventType, "grace"):
		return "grace"
	case strings.Contains(eventType, "would"):
		return "would_stop"
	case strings.Contains(eventType, "blocked"):
		return "blocked"
	case strings.Contains(eventType, "failed"):
		return "failed"
	default:
		return "warning"
	}
}

func alertEvent(eventType string) bool {
	return strings.Contains(eventType, "warning") ||
		strings.Contains(eventType, "terminate") ||
		strings.Contains(eventType, "termination") ||
		strings.Contains(eventType, "kill") ||
		strings.Contains(eventType, "grace")
}

func alertSeverity(event ledger.Event) string {
	switch {
	case event.Type == "usage_termination_completed":
		return "stop"
	case strings.Contains(event.Type, "failed"):
		return "error"
	case strings.Contains(event.Type, "blocked"):
		return "blocked"
	case strings.Contains(event.Type, "would") || strings.Contains(event.Type, "grace"):
		return "watch"
	default:
		return "warn"
	}
}

func alertLabel(eventType string) string {
	switch {
	case strings.Contains(eventType, "completed"):
		return "stopped"
	case strings.Contains(eventType, "started") || strings.Contains(eventType, "grace"):
		return "grace"
	case strings.Contains(eventType, "would"):
		return "would stop"
	case strings.Contains(eventType, "blocked"):
		return "blocked"
	case strings.Contains(eventType, "failed"):
		return "failed"
	default:
		return "warning"
	}
}

func actionableEvent(event ledger.Event) bool {
	return event.Type == "usage_termination_started" || event.Type == "usage_termination_completed"
}

func projectAlertAction(alert *AlertView, sessions map[string]SessionView) {
	if alert.Provider == "" || alert.SessionID == "" {
		return
	}
	session, ok := sessions[alert.Provider+"\x00"+alert.SessionID]
	if !ok {
		return
	}
	alert.SessionKey = session.Key
	switch alert.Category {
	case "warning", "would_stop", "blocked", "grace":
		alert.CanAck = session.CanAcknowledge
	}
}

func alertExplanation(event ledger.Event) string {
	switch event.Type {
	case "usage_would_terminate":
		return "Alert mode: Curb would stop this correlated worker in enforcement mode."
	case "usage_kill_blocked":
		return "Curb did not stop anything because the session was uncorrelated or watch-only."
	case "usage_grace_started":
		return "Enforcement grace period started for a correlated worker."
	case "usage_termination_started":
		return "Curb started terminating a correlated worker."
	case "usage_termination_completed":
		return "Curb completed termination for a correlated worker."
	case "policy_warning", "usage_warning":
		return "Usage or runtime crossed the warning policy."
	default:
		return ""
	}
}

func stringData(event ledger.Event, key string) string {
	if event.Data == nil {
		return ""
	}
	value, _ := event.Data[key].(string)
	return value
}
