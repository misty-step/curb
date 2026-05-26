package service

import (
	"context"
	"testing"
	"time"

	"github.com/phaedrus/curb/internal/config"
	"github.com/phaedrus/curb/internal/ledger"
	"github.com/phaedrus/curb/internal/usagewatch"
)

func TestServiceAlertsProjectLedgerEvents(t *testing.T) {
	path := writeServiceTestConfig(t)
	cfg, err := config.Load(path)
	if err != nil {
		t.Fatal(err)
	}
	log, err := ledger.Open(cfg.Ledger.Path)
	if err != nil {
		t.Fatal(err)
	}
	for _, event := range []ledger.Event{
		{Type: "run_started", AgentID: "synthetic-sleep", Message: "not an alert"},
		{Type: "usage_warning", AgentID: "synthetic-sleep", Mode: "alert", Message: "warning", Data: map[string]any{"cwd": "/tmp/project", "session_id": "session-one"}},
		{Type: "run_stopped", AgentID: "synthetic-sleep", Message: "noise"},
		{Type: "usage_would_terminate", AgentID: "synthetic-sleep", Mode: "alert", Message: "would stop"},
		{Type: "scan_failed", AgentID: "synthetic-sleep", Message: "noise"},
		{Type: "usage_termination_completed", AgentID: "synthetic-sleep", Mode: "enforcement", Message: "stopped"},
	} {
		if err := log.Append(event); err != nil {
			t.Fatal(err)
		}
	}
	svc := newTestService(t, path)

	alerts, err := svc.Alerts(context.Background(), 10)
	if err != nil {
		t.Fatal(err)
	}
	if len(alerts) != 3 {
		t.Fatalf("alerts = %#v", alerts)
	}
	if alerts[0].Severity != "warn" || alerts[0].Label != "warning" || alerts[0].CWD != "/tmp/project" || alerts[0].SessionID != "session-one" {
		t.Fatalf("warning alert = %#v", alerts[0])
	}
	if alerts[1].Severity != "watch" || alerts[1].Actionable {
		t.Fatalf("would alert = %#v", alerts[1])
	}
	if alerts[2].Severity != "stop" || !alerts[2].Actionable {
		t.Fatalf("stop alert = %#v", alerts[2])
	}

	limited, err := svc.Alerts(context.Background(), 2)
	if err != nil {
		t.Fatal(err)
	}
	if len(limited) != 2 || limited[0].Category != "would_stop" || limited[1].Category != "stopped" {
		t.Fatalf("limited alerts = %#v", limited)
	}
}

func TestServiceAlertsProjectCurrentAcknowledgementAffordance(t *testing.T) {
	now := time.Now().UTC()
	writeCodexUsageFixtureWithTotal(t, "alert-session", "/tmp/curb-service-alerts", now, 1500)
	path := writeServiceTestConfig(t)
	cfg, err := config.Load(path)
	if err != nil {
		t.Fatal(err)
	}
	log, err := ledger.Open(cfg.Ledger.Path)
	if err != nil {
		t.Fatal(err)
	}
	if err := log.Append(ledger.Event{
		Type:    "usage_warning",
		Mode:    "alert",
		Data:    map[string]any{"provider": "codex", "session_id": "alert-session", "cwd": "/tmp/curb-service-alerts"},
		Message: "warning",
	}); err != nil {
		t.Fatal(err)
	}
	svc := newTestService(t, path)
	if err := svc.Refresh(context.Background()); err != nil {
		t.Fatal(err)
	}

	alerts, err := svc.Alerts(context.Background(), 10)
	if err != nil {
		t.Fatal(err)
	}
	if len(alerts) != 1 || alerts[0].SessionKey != "codex:alert-session" || !alerts[0].CanAck {
		t.Fatalf("alerts = %#v", alerts)
	}
}

func TestServiceAlertsDoNotAcknowledgeMissingOrAlreadyAcknowledgedSessions(t *testing.T) {
	now := time.Now().UTC()
	writeCodexUsageFixtureWithTotal(t, "acked-session", "/tmp/curb-service-alerts", now, 1500)
	path := writeServiceTestConfig(t)
	cfg, err := config.Load(path)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := usagewatch.WriteSessionAck(cfg.Service.StateDir, "codex:acked-session", time.Minute, "already handled", now); err != nil {
		t.Fatal(err)
	}
	log, err := ledger.Open(cfg.Ledger.Path)
	if err != nil {
		t.Fatal(err)
	}
	for _, event := range []ledger.Event{
		{Type: "usage_warning", Mode: "alert", Data: map[string]any{"provider": "codex", "session_id": "acked-session"}},
		{Type: "usage_warning", Mode: "alert", Data: map[string]any{"provider": "codex", "session_id": "missing-session"}},
	} {
		if err := log.Append(event); err != nil {
			t.Fatal(err)
		}
	}
	svc := newTestService(t, path)
	if err := svc.Refresh(context.Background()); err != nil {
		t.Fatal(err)
	}

	alerts, err := svc.Alerts(context.Background(), 10)
	if err != nil {
		t.Fatal(err)
	}
	if len(alerts) != 2 {
		t.Fatalf("alerts = %#v", alerts)
	}
	for _, alert := range alerts {
		if alert.CanAck {
			t.Fatalf("alert should not be acknowledgeable: %#v", alert)
		}
	}
}
