package service

import (
	"context"
	"time"

	"github.com/phaedrus/curb/internal/ledger"
)

type EventView struct {
	Seq      int64     `json:"seq"`
	At       time.Time `json:"at"`
	Category string    `json:"category"`
	Kind     string    `json:"kind"`
	Message  string    `json:"message"`
	RunID    string    `json:"run_id,omitempty"`
	AgentID  string    `json:"agent_id,omitempty"`
	Mode     string    `json:"mode,omitempty"`
}

func (s *Service) Events(_ context.Context, limit int) ([]EventView, error) {
	events, err := s.recentLedgerEvents(limit)
	if err != nil {
		return nil, err
	}
	views := make([]EventView, 0, len(events))
	for _, event := range events {
		views = append(views, newEventView(event))
	}
	return views, nil
}

func newEventView(event ledger.Event) EventView {
	category, kind := eventClass(event.Type)
	message := event.Message
	if message == "" {
		message = defaultEventMessage(category, kind)
	}
	return EventView{
		Seq:      event.Seq,
		At:       event.Time,
		Category: category,
		Kind:     kind,
		Message:  message,
		RunID:    event.RunID,
		AgentID:  event.AgentID,
		Mode:     event.Mode,
	}
}

func eventClass(eventType string) (string, string) {
	switch eventType {
	case "service_started":
		return "service", "started"
	case "service_stopped":
		return "service", "stopped"
	case "run_started":
		return "run", "started"
	case "run_stopped":
		return "run", "stopped"
	case "ack_received", "session_ack_received":
		return "ack", "received"
	case "ack_rejected":
		return "ack", "rejected"
	case "policy_warning", "usage_warning":
		return "alert", "warning"
	case "usage_would_terminate":
		return "alert", "would_stop"
	case "usage_kill_blocked":
		return "alert", "blocked"
	case "usage_grace_started":
		return "alert", "grace"
	case "usage_termination_started", "termination_started":
		return "termination", "started"
	case "usage_termination_completed", "termination_completed":
		return "termination", "completed"
	case "usage_termination_failed", "termination_failed":
		return "termination", "failed"
	case "scan_failed", "usage_scan_failed":
		return "error", "scan_failed"
	case "notification_failed":
		return "error", "notification_failed"
	default:
		return "other", "recorded"
	}
}

func defaultEventMessage(category, kind string) string {
	switch category {
	case "service":
		return "Curb service " + kind + "."
	case "run":
		return "Agent run " + kind + "."
	case "ack":
		return "Acknowledgement " + kind + "."
	case "alert":
		return "Policy alert recorded."
	case "termination":
		return "Termination " + kind + "."
	case "error":
		return "Curb recorded an error."
	default:
		return "Curb recorded an event."
	}
}
