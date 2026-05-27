package api

import (
	"context"
	"encoding/json"
	"errors"
	"net/http"
	"net/http/httptest"
	"os"
	"path/filepath"
	"strings"
	"testing"
	"time"

	"github.com/phaedrus/curb/internal/config"
	"github.com/phaedrus/curb/internal/ledger"
	"github.com/phaedrus/curb/internal/platform"
	"github.com/phaedrus/curb/internal/service"
	"github.com/phaedrus/curb/internal/usagewatch"
)

func TestHandlerRequiresBearerToken(t *testing.T) {
	server := testServer(t)
	for _, path := range []string{"/v1/overview", "/v1/alerts"} {
		req := httptest.NewRequest(http.MethodGet, path, nil)
		res := httptest.NewRecorder()
		server.Handler().ServeHTTP(res, req)
		if res.Code != http.StatusUnauthorized {
			t.Fatalf("%s status = %d body=%s", path, res.Code, res.Body.String())
		}
	}
}

func TestHandlerAllowsLocalCORSPreflight(t *testing.T) {
	server := testServer(t)
	req := httptest.NewRequest(http.MethodOptions, "/v1/overview", nil)
	req.Header.Set("Origin", "http://127.0.0.1:5173")
	res := httptest.NewRecorder()

	server.Handler().ServeHTTP(res, req)

	if res.Code != http.StatusNoContent {
		t.Fatalf("status = %d body=%s", res.Code, res.Body.String())
	}
	if got := res.Header().Get("Access-Control-Allow-Origin"); got != "http://127.0.0.1:5173" {
		t.Fatalf("allow origin = %q", got)
	}
	if got := res.Header().Get("Access-Control-Allow-Methods"); !strings.Contains(got, http.MethodPost) {
		t.Fatalf("allow methods = %q, want %s", got, http.MethodPost)
	}
}

func TestHandlerServesUIWithoutWeakeningAPIAuth(t *testing.T) {
	server := testServer(t)
	server.ServeUI(http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		w.Header().Set("Content-Type", "text/html")
		_, _ = w.Write([]byte("curb ui"))
	}))

	req := httptest.NewRequest(http.MethodGet, "/", nil)
	res := httptest.NewRecorder()
	server.Handler().ServeHTTP(res, req)
	if res.Code != http.StatusOK || !strings.Contains(res.Body.String(), "curb ui") {
		t.Fatalf("ui status=%d body=%s", res.Code, res.Body.String())
	}
	cookies := res.Result().Cookies()
	if len(cookies) != 1 || cookies[0].Name != tokenCookie || !cookies[0].HttpOnly || cookies[0].Path != "/v1/" || cookies[0].SameSite != http.SameSiteStrictMode {
		t.Fatalf("cookies = %#v", cookies)
	}

	req = httptest.NewRequest(http.MethodGet, "/v1/snapshot", nil)
	req.AddCookie(cookies[0])
	res = httptest.NewRecorder()
	server.Handler().ServeHTTP(res, req)
	if res.Code != http.StatusOK {
		t.Fatalf("cookie api status=%d body=%s", res.Code, res.Body.String())
	}

	req = httptest.NewRequest(http.MethodGet, "/v1/snapshot", nil)
	res = httptest.NewRecorder()
	server.Handler().ServeHTTP(res, req)
	if res.Code != http.StatusUnauthorized {
		t.Fatalf("api status=%d body=%s", res.Code, res.Body.String())
	}

	req = httptest.NewRequest(http.MethodGet, "/v1/missing", nil)
	res = httptest.NewRecorder()
	server.Handler().ServeHTTP(res, req)
	if res.Code != http.StatusUnauthorized {
		t.Fatalf("missing api status=%d body=%s", res.Code, res.Body.String())
	}
}

func TestCookieAuthRequiresSameOriginForUnsafeMethods(t *testing.T) {
	server := testServer(t)
	cookie := &http.Cookie{Name: tokenCookie, Value: "test-token", Path: "/v1/"}

	req := httptest.NewRequest(http.MethodPut, "/v1/config", strings.NewReader(`{"mode":"alert"}`))
	req.AddCookie(cookie)
	res := httptest.NewRecorder()
	server.Handler().ServeHTTP(res, req)
	if res.Code != http.StatusForbidden {
		t.Fatalf("missing origin status=%d body=%s", res.Code, res.Body.String())
	}

	req = httptest.NewRequest(http.MethodPut, "http://127.0.0.1:8765/v1/config", strings.NewReader(`{"mode":"alert"}`))
	req.Host = "127.0.0.1:8765"
	req.Header.Set("Origin", "http://evil.example")
	req.AddCookie(cookie)
	res = httptest.NewRecorder()
	server.Handler().ServeHTTP(res, req)
	if res.Code != http.StatusForbidden {
		t.Fatalf("cross-origin status=%d body=%s", res.Code, res.Body.String())
	}

	req = httptest.NewRequest(http.MethodPut, "https://127.0.0.1:8765/v1/config", strings.NewReader(`{"mode":"alert"}`))
	req.Host = "127.0.0.1:8765"
	req.Header.Set("Origin", "http://127.0.0.1:8765")
	req.AddCookie(cookie)
	res = httptest.NewRecorder()
	server.Handler().ServeHTTP(res, req)
	if res.Code != http.StatusForbidden {
		t.Fatalf("scheme-mismatch status=%d body=%s", res.Code, res.Body.String())
	}

	req = httptest.NewRequest(http.MethodPut, "http://127.0.0.1:8765/v1/config", strings.NewReader(`{"mode":"alert"}`))
	req.Host = "127.0.0.1:8765"
	req.Header.Set("Origin", "http://127.0.0.1:8765")
	req.AddCookie(cookie)
	res = httptest.NewRecorder()
	server.Handler().ServeHTTP(res, req)
	if res.Code != http.StatusOK {
		t.Fatalf("same-origin status=%d body=%s", res.Code, res.Body.String())
	}

	req = httptest.NewRequest(http.MethodPost, "http://127.0.0.1:8765/v1/service/rescan", nil)
	req.Host = "127.0.0.1:8765"
	req.Header.Set("Origin", "http://127.0.0.1:8765")
	req.AddCookie(cookie)
	res = httptest.NewRecorder()
	server.Handler().ServeHTTP(res, req)
	if res.Code != http.StatusOK {
		t.Fatalf("same-origin rescan status=%d body=%s", res.Code, res.Body.String())
	}
}

func TestHandlerServesSnapshotSlices(t *testing.T) {
	server := testServer(t)

	var snapshot service.Snapshot
	getJSON(t, server, "/v1/snapshot", &snapshot)
	if snapshot.Overview.Status != "WATCH" || len(snapshot.Agents) != 1 || len(snapshot.Sessions) != 2 {
		t.Fatalf("snapshot = %#v", snapshot)
	}

	var overview service.Overview
	getJSON(t, server, "/v1/overview", &overview)
	if overview.Status != "WATCH" || overview.ActiveAgents != 1 {
		t.Fatalf("overview = %#v", overview)
	}

	var agents []service.AgentView
	getJSON(t, server, "/v1/agents", &agents)
	if len(agents) != 1 || agents[0].ID != "codex-worker" {
		t.Fatalf("agents = %#v", agents)
	}

	var sessions []service.SessionView
	getJSON(t, server, "/v1/sessions", &sessions)
	if len(sessions) != 2 || sessions[0].Key != "codex:session/one" {
		t.Fatalf("sessions = %#v", sessions)
	}
}

func TestHandlerLooksUpSessionByStableKeyAndFiltersTurns(t *testing.T) {
	server := testServer(t)

	var session service.SessionView
	getJSON(t, server, "/v1/sessions/codex:session%2Fone", &session)
	if session.ID != "session/one" {
		t.Fatalf("session = %#v", session)
	}

	var turns []service.TurnView
	getJSON(t, server, "/v1/sessions/codex:session%2Fone/turns?limit=1&since=24h", &turns)
	if len(turns) != 1 || turns[0].SessionKey != "codex:session/one" || turns[0].TotalTokens != 789 {
		t.Fatalf("turns = %#v", turns)
	}
}

func TestHandlerRescansService(t *testing.T) {
	calls := 0
	server, err := New("test-token", &testBackend{
		rescan: func(context.Context) (service.Snapshot, error) {
			calls++
			return service.Snapshot{Overview: service.Overview{Status: "ACTIVE", ActiveAgents: 2}}, nil
		},
	})
	if err != nil {
		t.Fatal(err)
	}

	req := httptest.NewRequest(http.MethodPost, "/v1/service/rescan", nil)
	req.Header.Set("Authorization", "Bearer test-token")
	res := httptest.NewRecorder()
	server.Handler().ServeHTTP(res, req)

	if res.Code != http.StatusOK {
		t.Fatalf("status=%d body=%s", res.Code, res.Body.String())
	}
	if calls != 1 {
		t.Fatalf("rescan calls = %d", calls)
	}
	var snapshot service.Snapshot
	if err := json.Unmarshal(res.Body.Bytes(), &snapshot); err != nil {
		t.Fatal(err)
	}
	if snapshot.Overview.Status != "ACTIVE" || snapshot.Overview.ActiveAgents != 2 {
		t.Fatalf("snapshot = %#v", snapshot)
	}
}

func TestHandlerRescanRequiresPostAndAuth(t *testing.T) {
	server := testServer(t)

	req := httptest.NewRequest(http.MethodGet, "/v1/service/rescan", nil)
	req.Header.Set("Authorization", "Bearer test-token")
	res := httptest.NewRecorder()
	server.Handler().ServeHTTP(res, req)
	if res.Code != http.StatusMethodNotAllowed {
		t.Fatalf("GET status=%d body=%s", res.Code, res.Body.String())
	}

	req = httptest.NewRequest(http.MethodPost, "/v1/service/rescan", nil)
	res = httptest.NewRecorder()
	server.Handler().ServeHTTP(res, req)
	if res.Code != http.StatusUnauthorized {
		t.Fatalf("unauthorized status=%d body=%s", res.Code, res.Body.String())
	}
}

func TestHandlerTurnsForUnknownSessionReturnsNotFound(t *testing.T) {
	server, err := New("test-token", &testBackend{
		turns: func(context.Context, string, service.TurnQuery) ([]service.TurnView, error) {
			return nil, service.ErrSessionNotFound
		},
	})
	if err != nil {
		t.Fatal(err)
	}

	req := httptest.NewRequest(http.MethodGet, "/v1/sessions/missing/turns", nil)
	req.Header.Set("Authorization", "Bearer test-token")
	res := httptest.NewRecorder()
	server.Handler().ServeHTTP(res, req)

	if res.Code != http.StatusNotFound {
		t.Fatalf("status=%d body=%s", res.Code, res.Body.String())
	}
}

func TestHandlerTurnsReturnsEmptyArrayForKnownSessionWithoutTurnsInRange(t *testing.T) {
	server, err := New("test-token", &testBackend{
		turns: func(context.Context, string, service.TurnQuery) ([]service.TurnView, error) {
			return []service.TurnView{}, nil
		},
	})
	if err != nil {
		t.Fatal(err)
	}

	var turns []service.TurnView
	getJSON(t, server, "/v1/sessions/codex:session%2Fone/turns?since=1m", &turns)
	if len(turns) != 0 {
		t.Fatalf("turns = %#v", turns)
	}
}

func TestHandlerAcknowledgesSession(t *testing.T) {
	server := testServer(t)

	req := httptest.NewRequest(http.MethodPost, "/v1/sessions/codex:session%2Fone/ack", strings.NewReader(`{"extend_seconds":60,"reason":"still supervising"}`))
	req.Header.Set("Authorization", "Bearer test-token")
	res := httptest.NewRecorder()
	server.Handler().ServeHTTP(res, req)

	if res.Code != http.StatusOK {
		t.Fatalf("status=%d body=%s", res.Code, res.Body.String())
	}
	var ack service.AckView
	if err := json.Unmarshal(res.Body.Bytes(), &ack); err != nil {
		t.Fatal(err)
	}
	if ack.SessionKey != "codex:session/one" || ack.ExtendSeconds != 60 || ack.Reason != "still supervising" {
		t.Fatalf("ack = %#v", ack)
	}
}

func TestHandlerRejectsUnknownSessionAck(t *testing.T) {
	server, err := New("test-token", &testBackend{
		ackSession: func(context.Context, string, service.AckRequest) (service.AckView, error) {
			return service.AckView{}, service.ErrSessionNotFound
		},
	})
	if err != nil {
		t.Fatal(err)
	}

	req := httptest.NewRequest(http.MethodPost, "/v1/sessions/missing/ack", strings.NewReader(`{"extend_seconds":60}`))
	req.Header.Set("Authorization", "Bearer test-token")
	res := httptest.NewRecorder()
	server.Handler().ServeHTTP(res, req)

	if res.Code != http.StatusNotFound {
		t.Fatalf("status=%d body=%s", res.Code, res.Body.String())
	}
}

func TestHandlerClassifiesSessionAckErrors(t *testing.T) {
	server, err := New("test-token", &testBackend{
		ackSession: func(context.Context, string, service.AckRequest) (service.AckView, error) {
			return service.AckView{}, service.ErrInvalidAck
		},
	})
	if err != nil {
		t.Fatal(err)
	}
	req := httptest.NewRequest(http.MethodPost, "/v1/sessions/codex:session%2Fone/ack", strings.NewReader(`{"extend_seconds":-1}`))
	req.Header.Set("Authorization", "Bearer test-token")
	res := httptest.NewRecorder()
	server.Handler().ServeHTTP(res, req)
	if res.Code != http.StatusBadRequest {
		t.Fatalf("validation status=%d body=%s", res.Code, res.Body.String())
	}

	server, err = New("test-token", &testBackend{
		ackSession: func(context.Context, string, service.AckRequest) (service.AckView, error) {
			return service.AckView{}, os.ErrPermission
		},
	})
	if err != nil {
		t.Fatal(err)
	}
	req = httptest.NewRequest(http.MethodPost, "/v1/sessions/codex:session%2Fone/ack", strings.NewReader(`{"extend_seconds":60}`))
	req.Header.Set("Authorization", "Bearer test-token")
	res = httptest.NewRecorder()
	server.Handler().ServeHTTP(res, req)
	if res.Code != http.StatusInternalServerError {
		t.Fatalf("persistence status=%d body=%s", res.Code, res.Body.String())
	}
}

func TestHandlerStopsSession(t *testing.T) {
	started := time.Now().UTC().Add(-time.Minute)
	server := testServer(t)
	req := httptest.NewRequest(http.MethodPost, "/v1/sessions/codex:session%2Fone/stop", strings.NewReader(`{
		"confirm": true,
		"scope": "tree",
		"reason": "manual stop",
		"expected": {
			"pid": 4242,
			"started_at": "`+started.Format(time.RFC3339Nano)+`",
			"owner": "phaedrus",
			"executable": "/usr/local/bin/codex"
		}
	}`))
	req.Header.Set("Authorization", "Bearer test-token")
	res := httptest.NewRecorder()
	server.Handler().ServeHTTP(res, req)

	if res.Code != http.StatusOK {
		t.Fatalf("status=%d body=%s", res.Code, res.Body.String())
	}
	var stopped service.StopView
	if err := json.Unmarshal(res.Body.Bytes(), &stopped); err != nil {
		t.Fatal(err)
	}
	if stopped.SessionKey != "codex:session/one" || stopped.PID != 4242 || stopped.Scope != "tree" {
		t.Fatalf("stop view = %#v", stopped)
	}
}

func TestHandlerClassifiesSessionStopErrors(t *testing.T) {
	for _, tc := range []struct {
		name string
		err  error
		want int
	}{
		{name: "missing", err: service.ErrSessionNotFound, want: http.StatusNotFound},
		{name: "invalid", err: service.ErrInvalidStop, want: http.StatusBadRequest},
		{name: "conflict", err: service.ErrStopConflict, want: http.StatusConflict},
		{name: "other", err: os.ErrPermission, want: http.StatusInternalServerError},
	} {
		t.Run(tc.name, func(t *testing.T) {
			server, err := New("test-token", &testBackend{
				stopSession: func(context.Context, string, service.StopRequest) (service.StopView, error) {
					return service.StopView{}, tc.err
				},
			})
			if err != nil {
				t.Fatal(err)
			}
			req := httptest.NewRequest(http.MethodPost, "/v1/sessions/codex:session%2Fone/stop", strings.NewReader(`{"confirm":true}`))
			req.Header.Set("Authorization", "Bearer test-token")
			res := httptest.NewRecorder()
			server.Handler().ServeHTTP(res, req)
			if res.Code != tc.want {
				t.Fatalf("status=%d body=%s", res.Code, res.Body.String())
			}
		})
	}
}

func TestHandlerReturnsEventsAndAlerts(t *testing.T) {
	server := testServer(t)

	var events []service.EventView
	getJSON(t, server, "/v1/events?limit=1", &events)
	if len(events) != 1 || events[0].Category != "alert" || events[0].Kind != "warning" {
		t.Fatalf("events = %#v", events)
	}

	var alerts []service.AlertView
	getJSON(t, server, "/v1/alerts?limit=1", &alerts)
	if len(alerts) != 1 || alerts[0].Category != "warning" {
		t.Fatalf("alerts = %#v", alerts)
	}
}

func TestHandlerReturnsEmptyArraysForEventsAndAlerts(t *testing.T) {
	server, err := New("test-token", &testBackend{})
	if err != nil {
		t.Fatal(err)
	}

	req := httptest.NewRequest(http.MethodGet, "/v1/events", nil)
	req.Header.Set("Authorization", "Bearer test-token")
	res := httptest.NewRecorder()
	server.Handler().ServeHTTP(res, req)
	if res.Code != http.StatusOK || strings.TrimSpace(res.Body.String()) != "[]" {
		t.Fatalf("events status=%d body=%s", res.Code, res.Body.String())
	}

	req = httptest.NewRequest(http.MethodGet, "/v1/alerts", nil)
	req.Header.Set("Authorization", "Bearer test-token")
	res = httptest.NewRecorder()
	server.Handler().ServeHTTP(res, req)
	if res.Code != http.StatusOK || strings.TrimSpace(res.Body.String()) != "[]" {
		t.Fatalf("alerts status=%d body=%s", res.Code, res.Body.String())
	}
}

func TestHandlerServesAndUpdatesConfig(t *testing.T) {
	cfg := testConfig()
	server := testServerWithConfig(t, &cfg)

	var view service.ConfigView
	getJSON(t, server, "/v1/config", &view)
	if view.Mode != "alert" || view.WarnTurnTokens != 1000 || !view.LocalNotifications {
		t.Fatalf("config = %#v", view)
	}
	if len(view.Agents) != 1 || !view.Agents[0].Terminates {
		t.Fatalf("agents = %#v", view.Agents)
	}

	req := httptest.NewRequest(http.MethodPut, "/v1/config", strings.NewReader(`{
		"mode":"visibility",
		"warn_turn_tokens":2000,
		"kill_turn_tokens":4000,
		"usage_window_seconds":120,
		"local_notifications":false
	}`))
	req.Header.Set("Authorization", "Bearer test-token")
	res := httptest.NewRecorder()
	server.Handler().ServeHTTP(res, req)
	if res.Code != http.StatusOK {
		t.Fatalf("status=%d body=%s", res.Code, res.Body.String())
	}
	var updated service.ConfigView
	if err := json.Unmarshal(res.Body.Bytes(), &updated); err != nil {
		t.Fatal(err)
	}
	if updated.Mode != "visibility" || updated.WarnTurnTokens != 2000 || updated.KillTurnTokens != 4000 || updated.UsageWindowSeconds != 120 || updated.LocalNotifications {
		t.Fatalf("updated = %#v", updated)
	}
}

func TestHandlerServesNotificationHealthAndTest(t *testing.T) {
	calls := 0
	server, err := New("test-token", &testBackend{
		notificationHealth: func(context.Context) (service.NotificationView, error) {
			return service.NotificationView{Enabled: true, Available: true, Status: "ready", Message: "ready"}, nil
		},
		testNotification: func(context.Context) (service.NotificationView, error) {
			calls++
			return service.NotificationView{Enabled: true, Available: true, Status: "delivered", Message: "ok", LastTestAt: "2026-05-22T12:00:00Z"}, nil
		},
	})
	if err != nil {
		t.Fatal(err)
	}

	var health service.NotificationView
	getJSON(t, server, "/v1/notifications/health", &health)
	if health.Status != "ready" || !health.Enabled || !health.Available {
		t.Fatalf("health = %#v", health)
	}

	req := httptest.NewRequest(http.MethodPost, "/v1/notifications/test", nil)
	req.Header.Set("Authorization", "Bearer test-token")
	res := httptest.NewRecorder()
	server.Handler().ServeHTTP(res, req)
	if res.Code != http.StatusOK {
		t.Fatalf("status=%d body=%s", res.Code, res.Body.String())
	}
	var tested service.NotificationView
	if err := json.Unmarshal(res.Body.Bytes(), &tested); err != nil {
		t.Fatal(err)
	}
	if calls != 1 || tested.Status != "delivered" || tested.LastTestAt == "" {
		t.Fatalf("calls=%d tested=%#v", calls, tested)
	}
}

func TestHandlerServesAndCompletesOnboarding(t *testing.T) {
	completed := false
	server, err := New("test-token", &testBackend{
		onboarding: func(context.Context) (service.OnboardingView, error) {
			return service.OnboardingView{Required: !completed, Mode: "alert", FinalSentence: "Curb will notify."}, nil
		},
		completeOnboarding: func(context.Context) (service.OnboardingView, error) {
			completed = true
			return service.OnboardingView{Required: false, Mode: "alert", FinalSentence: "Curb will notify."}, nil
		},
	})
	if err != nil {
		t.Fatal(err)
	}

	var view service.OnboardingView
	getJSON(t, server, "/v1/onboarding", &view)
	if !view.Required || view.Mode != "alert" {
		t.Fatalf("onboarding = %#v", view)
	}

	req := httptest.NewRequest(http.MethodPost, "/v1/onboarding/complete", nil)
	req.Header.Set("Authorization", "Bearer test-token")
	res := httptest.NewRecorder()
	server.Handler().ServeHTTP(res, req)
	if res.Code != http.StatusOK {
		t.Fatalf("status=%d body=%s", res.Code, res.Body.String())
	}
	if err := json.Unmarshal(res.Body.Bytes(), &view); err != nil {
		t.Fatal(err)
	}
	if view.Required || !completed {
		t.Fatalf("completed=%v view=%#v", completed, view)
	}
}

func TestHandlerOnboardingEndpointsEnforceMethodsAndAuth(t *testing.T) {
	server, err := New("test-token", &testBackend{})
	if err != nil {
		t.Fatal(err)
	}

	req := httptest.NewRequest(http.MethodGet, "/v1/onboarding", nil)
	res := httptest.NewRecorder()
	server.Handler().ServeHTTP(res, req)
	if res.Code != http.StatusUnauthorized {
		t.Fatalf("unauthorized status=%d", res.Code)
	}

	req = httptest.NewRequest(http.MethodPost, "/v1/onboarding", nil)
	req.Header.Set("Authorization", "Bearer test-token")
	res = httptest.NewRecorder()
	server.Handler().ServeHTTP(res, req)
	if res.Code != http.StatusMethodNotAllowed {
		t.Fatalf("onboarding method status=%d", res.Code)
	}

	req = httptest.NewRequest(http.MethodGet, "/v1/onboarding/complete", nil)
	req.Header.Set("Authorization", "Bearer test-token")
	res = httptest.NewRecorder()
	server.Handler().ServeHTTP(res, req)
	if res.Code != http.StatusMethodNotAllowed {
		t.Fatalf("complete method status=%d", res.Code)
	}
}

func TestHandlerHealthReturnsCurbCompatibilityMarker(t *testing.T) {
	server, err := New("test-token", &testBackend{})
	if err != nil {
		t.Fatal(err)
	}
	var health struct {
		OK         bool   `json:"ok"`
		App        string `json:"app"`
		APIVersion int    `json:"api_version"`
	}
	getJSON(t, server, "/v1/health", &health)
	if !health.OK || health.App != "curb" || health.APIVersion != 1 {
		t.Fatalf("health = %#v", health)
	}
}

func TestHandlerHealthRequiresAuth(t *testing.T) {
	server, err := New("test-token", &testBackend{})
	if err != nil {
		t.Fatal(err)
	}
	req := httptest.NewRequest(http.MethodGet, "/v1/health", nil)
	res := httptest.NewRecorder()
	server.Handler().ServeHTTP(res, req)
	if res.Code != http.StatusUnauthorized {
		t.Fatalf("status = %d, body = %s", res.Code, res.Body.String())
	}
}

func TestHandlerOnboardingCompleteCookieAuthRequiresSameOrigin(t *testing.T) {
	server, err := New("test-token", &testBackend{})
	if err != nil {
		t.Fatal(err)
	}
	req := httptest.NewRequest(http.MethodPost, "/v1/onboarding/complete", nil)
	req.Host = "127.0.0.1:8765"
	req.Header.Set("Origin", "http://evil.test")
	req.AddCookie(&http.Cookie{Name: tokenCookie, Value: "test-token"})
	res := httptest.NewRecorder()
	server.Handler().ServeHTTP(res, req)
	if res.Code != http.StatusForbidden {
		t.Fatalf("status=%d body=%s", res.Code, res.Body.String())
	}
}

func TestHandlerOnboardingBackendErrorsReturnServerError(t *testing.T) {
	server, err := New("test-token", &testBackend{
		onboarding: func(context.Context) (service.OnboardingView, error) {
			return service.OnboardingView{}, errors.New("onboarding unavailable")
		},
		completeOnboarding: func(context.Context) (service.OnboardingView, error) {
			return service.OnboardingView{}, errors.New("completion unavailable")
		},
	})
	if err != nil {
		t.Fatal(err)
	}

	req := httptest.NewRequest(http.MethodGet, "/v1/onboarding", nil)
	req.Header.Set("Authorization", "Bearer test-token")
	res := httptest.NewRecorder()
	server.Handler().ServeHTTP(res, req)
	if res.Code != http.StatusInternalServerError || !strings.Contains(res.Body.String(), "onboarding unavailable") {
		t.Fatalf("status=%d body=%s", res.Code, res.Body.String())
	}

	req = httptest.NewRequest(http.MethodPost, "/v1/onboarding/complete", nil)
	req.Header.Set("Authorization", "Bearer test-token")
	res = httptest.NewRecorder()
	server.Handler().ServeHTTP(res, req)
	if res.Code != http.StatusInternalServerError || !strings.Contains(res.Body.String(), "completion unavailable") {
		t.Fatalf("status=%d body=%s", res.Code, res.Body.String())
	}
}

func TestHandlerNotificationTestDisabledReturnsConflict(t *testing.T) {
	server, err := New("test-token", &testBackend{
		testNotification: func(context.Context) (service.NotificationView, error) {
			return service.NotificationView{Enabled: false, Available: true, Status: "disabled", Message: "disabled"}, service.ErrNotificationsDisabled
		},
	})
	if err != nil {
		t.Fatal(err)
	}

	req := httptest.NewRequest(http.MethodPost, "/v1/notifications/test", nil)
	req.Header.Set("Authorization", "Bearer test-token")
	res := httptest.NewRecorder()
	server.Handler().ServeHTTP(res, req)
	if res.Code != http.StatusConflict {
		t.Fatalf("status=%d body=%s", res.Code, res.Body.String())
	}
}

func TestHandlerNotificationEndpointsEnforceMethodsAndAuth(t *testing.T) {
	server, err := New("test-token", &testBackend{})
	if err != nil {
		t.Fatal(err)
	}

	req := httptest.NewRequest(http.MethodGet, "/v1/notifications/health", nil)
	res := httptest.NewRecorder()
	server.Handler().ServeHTTP(res, req)
	if res.Code != http.StatusUnauthorized {
		t.Fatalf("unauthorized status=%d", res.Code)
	}

	req = httptest.NewRequest(http.MethodPost, "/v1/notifications/health", nil)
	req.Header.Set("Authorization", "Bearer test-token")
	res = httptest.NewRecorder()
	server.Handler().ServeHTTP(res, req)
	if res.Code != http.StatusMethodNotAllowed {
		t.Fatalf("health method status=%d", res.Code)
	}

	req = httptest.NewRequest(http.MethodGet, "/v1/notifications/test", nil)
	req.Header.Set("Authorization", "Bearer test-token")
	res = httptest.NewRecorder()
	server.Handler().ServeHTTP(res, req)
	if res.Code != http.StatusMethodNotAllowed {
		t.Fatalf("test method status=%d", res.Code)
	}
}

func TestHandlerRejectsInvalidConfigUpdate(t *testing.T) {
	cfg := testConfig()
	server := testServerWithConfig(t, &cfg)

	req := httptest.NewRequest(http.MethodPut, "/v1/config", strings.NewReader(`{"warn_turn_tokens":5000,"kill_turn_tokens":4000}`))
	req.Header.Set("Authorization", "Bearer test-token")
	res := httptest.NewRecorder()
	server.Handler().ServeHTTP(res, req)

	if res.Code != http.StatusBadRequest {
		t.Fatalf("status=%d body=%s", res.Code, res.Body.String())
	}
	if cfg.Usage.WarnTurnTokens != 1000 || cfg.Usage.KillTurnTokens != 3000 {
		t.Fatalf("config mutated after invalid update: %#v", cfg.Usage)
	}
}

func TestHandlerWithRealServiceUpdatesConfigRefreshesSnapshotAndReadsLedger(t *testing.T) {
	path := writeAPITestConfig(t)
	writeAPICodexUsageFixture(t, "api-session", filepath.Dir(path), time.Now().UTC())
	cfg, err := config.Load(path)
	if err != nil {
		t.Fatal(err)
	}
	log, err := ledger.Open(cfg.Ledger.Path)
	if err != nil {
		t.Fatal(err)
	}
	for _, typ := range []string{"run_started", "usage_warning", "usage_would_terminate"} {
		if err := log.Append(ledger.Event{Type: typ}); err != nil {
			t.Fatal(err)
		}
	}
	svc, err := service.New(path, func(context.Context) (*platform.Snapshot, error) {
		now := time.Now()
		return &platform.Snapshot{
			At:       now,
			Platform: "test",
			Processes: map[int32]platform.Process{
				42: {
					PID:       42,
					Name:      "sleep",
					Exe:       "/bin/sleep",
					Cmdline:   "sleep 600",
					CWD:       filepath.Dir(path),
					Create:    now.Add(-time.Minute),
					StartedOK: true,
				},
			},
			Children: map[int32][]int32{},
		}, nil
	})
	if err != nil {
		t.Fatal(err)
	}
	if err := svc.Refresh(context.Background()); err != nil {
		t.Fatal(err)
	}
	server, err := New("test-token", svc)
	if err != nil {
		t.Fatal(err)
	}

	putJSON(t, server, `{"mode":"enforcement","warn_turn_tokens":2000,"kill_turn_tokens":4000}`)
	var updated service.ConfigView
	getJSON(t, server, "/v1/config", &updated)
	if updated.Mode != "enforcement" || updated.WarnTurnTokens != 2000 || updated.KillTurnTokens != 4000 {
		t.Fatalf("updated config = %#v", updated)
	}
	reloaded, err := config.Load(path)
	if err != nil {
		t.Fatal(err)
	}
	if reloaded.Mode != config.ModeEnforcement || reloaded.Usage.WarnTurnTokens != 2000 || reloaded.Usage.KillTurnTokens != 4000 {
		t.Fatalf("reloaded config = %#v", reloaded)
	}
	var snapshot service.Snapshot
	getJSON(t, server, "/v1/snapshot", &snapshot)
	if snapshot.Overview.Mode != "enforcement" || len(snapshot.Agents) != 1 {
		t.Fatalf("snapshot = %#v", snapshot)
	}

	beforeInvalid, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	req := httptest.NewRequest(http.MethodPut, "/v1/config", strings.NewReader(`{"warn_turn_tokens":9000,"kill_turn_tokens":4000}`))
	req.Header.Set("Authorization", "Bearer test-token")
	res := httptest.NewRecorder()
	server.Handler().ServeHTTP(res, req)
	if res.Code != http.StatusBadRequest {
		t.Fatalf("status=%d body=%s", res.Code, res.Body.String())
	}
	afterInvalid, err := os.ReadFile(path)
	if err != nil {
		t.Fatal(err)
	}
	if string(afterInvalid) != string(beforeInvalid) {
		t.Fatalf("config changed after invalid update\nbefore:\n%s\nafter:\n%s", beforeInvalid, afterInvalid)
	}
	var stillUpdated service.ConfigView
	getJSON(t, server, "/v1/config", &stillUpdated)
	if stillUpdated.WarnTurnTokens != 2000 || stillUpdated.KillTurnTokens != 4000 {
		t.Fatalf("in-memory config changed after invalid update: %#v", stillUpdated)
	}

	var events []service.EventView
	getJSON(t, server, "/v1/events?limit=2", &events)
	if len(events) != 2 ||
		events[0].Category != "alert" || events[0].Kind != "warning" ||
		events[1].Category != "alert" || events[1].Kind != "would_stop" {
		t.Fatalf("events = %#v", events)
	}
	ackReq := httptest.NewRequest(http.MethodPost, "/v1/sessions/api-session/ack", strings.NewReader(`{"extend_seconds":30,"reason":"demo"}`))
	ackReq.Header.Set("Authorization", "Bearer test-token")
	ackRes := httptest.NewRecorder()
	server.Handler().ServeHTTP(ackRes, ackReq)
	if ackRes.Code != http.StatusOK {
		t.Fatalf("ack status=%d body=%s", ackRes.Code, ackRes.Body.String())
	}
	stored, ok, err := usagewatch.ReadSessionAck(cfg.Service.StateDir, "codex:api-session")
	if err != nil {
		t.Fatal(err)
	}
	if !ok || stored.Reason != "demo" {
		t.Fatalf("stored ack = %#v ok=%v", stored, ok)
	}
	var ackEvents []service.EventView
	getJSON(t, server, "/v1/events?limit=1", &ackEvents)
	if len(ackEvents) != 1 || ackEvents[0].Category != "ack" || ackEvents[0].Kind != "received" {
		t.Fatalf("ack events = %#v", ackEvents)
	}
	if err := log.Append(ledger.Event{Type: "usage_warning", AgentID: "synthetic-sleep", Message: "warning"}); err != nil {
		t.Fatal(err)
	}
	var alerts []service.AlertView
	getJSON(t, server, "/v1/alerts?limit=1", &alerts)
	if len(alerts) != 1 || alerts[0].Category != "warning" || alerts[0].AgentID != "synthetic-sleep" {
		t.Fatalf("alerts = %#v", alerts)
	}

}

func TestLoadOrCreateTokenPersists0600Token(t *testing.T) {
	dir := t.TempDir()
	token, path, err := LoadOrCreateToken(dir)
	if err != nil {
		t.Fatal(err)
	}
	if len(token) != 64 {
		t.Fatalf("token len = %d", len(token))
	}
	info, err := os.Stat(path)
	if err != nil {
		t.Fatal(err)
	}
	if got := info.Mode().Perm(); got != 0o600 {
		t.Fatalf("mode = %o", got)
	}
	again, samePath, err := LoadOrCreateToken(dir)
	if err != nil {
		t.Fatal(err)
	}
	if again != token || samePath != path {
		t.Fatalf("token was not stable: %q/%q", again, samePath)
	}
}

func TestLoadOrCreateTokenRejectsEmptyExistingToken(t *testing.T) {
	dir := t.TempDir()
	if err := os.WriteFile(dir+"/api.token", []byte("\n"), 0o600); err != nil {
		t.Fatal(err)
	}
	if _, _, err := LoadOrCreateToken(dir); err == nil {
		t.Fatal("expected empty token error")
	}
}

func TestLoadOrCreateTokenRepairsExistingPermissions(t *testing.T) {
	dir := t.TempDir()
	path := dir + "/api.token"
	if err := os.WriteFile(path, []byte("existing-token\n"), 0o644); err != nil {
		t.Fatal(err)
	}
	token, _, err := LoadOrCreateToken(dir)
	if err != nil {
		t.Fatal(err)
	}
	if token != "existing-token" {
		t.Fatalf("token = %q", token)
	}
	info, err := os.Stat(path)
	if err != nil {
		t.Fatal(err)
	}
	if got := info.Mode().Perm(); got != 0o600 {
		t.Fatalf("mode = %o", got)
	}
}

func getJSON(t *testing.T, server *Server, path string, out any) {
	t.Helper()
	req := httptest.NewRequest(http.MethodGet, path, nil)
	req.Header.Set("Authorization", "Bearer test-token")
	res := httptest.NewRecorder()
	server.Handler().ServeHTTP(res, req)
	if res.Code != http.StatusOK {
		t.Fatalf("%s status=%d body=%s", path, res.Code, res.Body.String())
	}
	if err := json.Unmarshal(res.Body.Bytes(), out); err != nil {
		t.Fatalf("decode %s: %v", path, err)
	}
}

func putJSON(t *testing.T, server *Server, body string) {
	t.Helper()
	req := httptest.NewRequest(http.MethodPut, "/v1/config", strings.NewReader(body))
	req.Header.Set("Authorization", "Bearer test-token")
	res := httptest.NewRecorder()
	server.Handler().ServeHTTP(res, req)
	if res.Code != http.StatusOK {
		t.Fatalf("PUT /v1/config status=%d body=%s", res.Code, res.Body.String())
	}
}

func testServer(t *testing.T) *Server {
	t.Helper()
	snapshot := service.Snapshot{
		Overview: service.Overview{Status: "WATCH", ActiveAgents: 1},
		Agents: []service.AgentView{{
			ID:    "codex-worker",
			State: "running",
		}},
		Sessions: []service.SessionView{
			{Key: "codex:session/one", ID: "session/one", Provider: "codex"},
			{Key: "codex:session/two", ID: "session/two", Provider: "codex"},
		},
		Turns: []service.TurnView{
			{SessionKey: "codex:session/one", SessionID: "session/one", TotalTokens: 123},
			{SessionKey: "codex:session/two", SessionID: "session/two", TotalTokens: 456},
		},
	}
	server, err := New("test-token", &testBackend{
		snapshot: snapshot,
		turns: func(_ context.Context, key string, query service.TurnQuery) ([]service.TurnView, error) {
			if key != "codex:session/one" || query.Limit != 1 || query.Since.IsZero() {
				return nil, nil
			}
			return []service.TurnView{{SessionKey: "codex:session/one", SessionID: "session/one", TotalTokens: 789}}, nil
		},
		events: func(_ context.Context, limit int) ([]service.EventView, error) {
			events := []service.EventView{
				{Category: "alert", Kind: "warning", AgentID: "codex-worker"},
				{Category: "alert", Kind: "would_stop", AgentID: "codex-worker"},
			}
			if limit < len(events) {
				return events[:limit], nil
			}
			return events, nil
		},
		config: func() service.ConfigView {
			cfg := testConfig()
			return service.NewConfigView("/tmp/curb.yaml", &cfg, "machine_test")
		}(),
	})
	if err != nil {
		t.Fatal(err)
	}
	return server
}

func testServerWithConfig(t *testing.T, cfg *config.Config) *Server {
	t.Helper()
	server, err := New("test-token", &testBackend{
		updateConfig: func(_ context.Context, update service.ConfigUpdate) (service.ConfigView, error) {
			next := *cfg
			next.Agents = append([]config.Agent(nil), cfg.Agents...)
			if err := service.ApplyConfigUpdate(&next, update); err != nil {
				return service.ConfigView{}, err
			}
			*cfg = next
			return service.NewConfigView("/tmp/curb.yaml", cfg, "machine_test"), nil
		},
		configFunc: func(context.Context) (service.ConfigView, error) {
			return service.NewConfigView("/tmp/curb.yaml", cfg, "machine_test"), nil
		},
	})
	if err != nil {
		t.Fatal(err)
	}
	return server
}

type testBackend struct {
	snapshot           service.Snapshot
	rescan             func(context.Context) (service.Snapshot, error)
	events             func(context.Context, int) ([]service.EventView, error)
	config             service.ConfigView
	configFunc         func(context.Context) (service.ConfigView, error)
	updateConfig       func(context.Context, service.ConfigUpdate) (service.ConfigView, error)
	notificationHealth func(context.Context) (service.NotificationView, error)
	testNotification   func(context.Context) (service.NotificationView, error)
	onboarding         func(context.Context) (service.OnboardingView, error)
	completeOnboarding func(context.Context) (service.OnboardingView, error)
	turns              func(context.Context, string, service.TurnQuery) ([]service.TurnView, error)
	ackSession         func(context.Context, string, service.AckRequest) (service.AckView, error)
	stopSession        func(context.Context, string, service.StopRequest) (service.StopView, error)
}

func (b *testBackend) Snapshot(context.Context) (service.Snapshot, error) {
	return b.snapshot, nil
}

func (b *testBackend) Rescan(ctx context.Context) (service.Snapshot, error) {
	if b.rescan != nil {
		return b.rescan(ctx)
	}
	return b.snapshot, nil
}

func (b *testBackend) Events(ctx context.Context, limit int) ([]service.EventView, error) {
	if b.events == nil {
		return []service.EventView{}, nil
	}
	return b.events(ctx, limit)
}

func (b *testBackend) Alerts(context.Context, int) ([]service.AlertView, error) {
	if b.events == nil {
		return []service.AlertView{}, nil
	}
	return []service.AlertView{{Category: "warning", AgentID: "codex-worker"}}, nil
}

func (b *testBackend) Config(ctx context.Context) (service.ConfigView, error) {
	if b.configFunc != nil {
		return b.configFunc(ctx)
	}
	return b.config, nil
}

func (b *testBackend) UpdateConfig(ctx context.Context, update service.ConfigUpdate) (service.ConfigView, error) {
	if b.updateConfig == nil {
		return service.ConfigView{}, nil
	}
	return b.updateConfig(ctx, update)
}

func (b *testBackend) NotificationHealth(ctx context.Context) (service.NotificationView, error) {
	if b.notificationHealth != nil {
		return b.notificationHealth(ctx)
	}
	return service.NotificationView{Enabled: true, Available: true, Status: "ready", Message: "ready"}, nil
}

func (b *testBackend) TestNotification(ctx context.Context) (service.NotificationView, error) {
	if b.testNotification != nil {
		return b.testNotification(ctx)
	}
	return service.NotificationView{Enabled: true, Available: true, Status: "delivered", Message: "ok", LastTestAt: time.Now().UTC().Format(time.RFC3339)}, nil
}

func (b *testBackend) Onboarding(ctx context.Context) (service.OnboardingView, error) {
	if b.onboarding != nil {
		return b.onboarding(ctx)
	}
	return service.OnboardingView{Required: true, Mode: "alert", FinalSentence: "Curb will notify."}, nil
}

func (b *testBackend) CompleteOnboarding(ctx context.Context) (service.OnboardingView, error) {
	if b.completeOnboarding != nil {
		return b.completeOnboarding(ctx)
	}
	return service.OnboardingView{Required: false, Mode: "alert", FinalSentence: "Curb will notify."}, nil
}

func (b *testBackend) SessionTurns(ctx context.Context, key string, query service.TurnQuery) ([]service.TurnView, error) {
	if b.turns != nil {
		return b.turns(ctx, key, query)
	}
	return []service.TurnView{}, nil
}

func (b *testBackend) AcknowledgeSession(ctx context.Context, key string, request service.AckRequest) (service.AckView, error) {
	if b.ackSession != nil {
		return b.ackSession(ctx, key, request)
	}
	return service.AckView{
		SessionKey:    key,
		ExtendSeconds: request.ExtendSeconds,
		Reason:        request.Reason,
		Until:         time.Now().Add(time.Duration(request.ExtendSeconds) * time.Second),
	}, nil
}

func (b *testBackend) StopSession(ctx context.Context, key string, request service.StopRequest) (service.StopView, error) {
	if b.stopSession != nil {
		return b.stopSession(ctx, key, request)
	}
	return service.StopView{
		SessionKey: key,
		AgentID:    "codex-cli",
		PID:        request.Expected.PID,
		StartedAt:  request.Expected.StartedAt,
		Owner:      request.Expected.Owner,
		Executable: request.Expected.Executable,
		Scope:      "tree",
		ScopePIDs:  []int32{request.Expected.PID},
	}, nil
}

func testConfig() config.Config {
	enabled := true
	return config.Config{
		Version: 1,
		Mode:    config.ModeAlert,
		Service: config.ServiceConfig{ScanInterval: config.Duration{Duration: time.Second}},
		Usage: config.UsageConfig{
			Enabled:        &enabled,
			ScanInterval:   config.Duration{Duration: time.Second},
			Lookback:       config.Duration{Duration: time.Hour},
			Window:         config.Duration{Duration: time.Minute},
			WarnTurnTokens: 1000,
			KillTurnTokens: 3000,
		},
		Defaults: config.Policy{
			WarnAfter:       config.Duration{Duration: time.Minute},
			KillAfter:       config.Duration{Duration: 2 * time.Minute},
			KillGracePeriod: config.Duration{Duration: 10 * time.Second},
		},
		Alerts: config.AlertConfig{LocalNotifications: true},
		Agents: []config.Agent{{
			ID:     "codex-worker",
			Label:  "Codex worker",
			Family: "codex",
			Kind:   config.AgentKindProcess,
			Match:  config.Match{ProcessNames: []string{"codex"}},
		}},
		Ledger: config.LedgerConfig{Path: "/tmp/runs.ndjson"},
	}
}

func writeAPITestConfig(t *testing.T) string {
	t.Helper()
	dir := t.TempDir()
	cfg := testConfig()
	cfg.Mode = config.ModeVisibility
	cfg.Service.StateDir = filepath.Join(dir, "state")
	cfg.Ledger.Path = filepath.Join(dir, "state", "runs.ndjson")
	cfg.Agents = []config.Agent{{
		ID:     "synthetic-sleep",
		Label:  "Synthetic Sleep",
		Family: "synthetic",
		Kind:   config.AgentKindProcess,
		Match:  config.Match{ProcessNames: []string{"sleep"}, CommandRegex: []string{"sleep"}},
	}}
	if err := os.MkdirAll(cfg.Service.StateDir, 0o700); err != nil {
		t.Fatal(err)
	}
	path := filepath.Join(dir, "config.yaml")
	if err := config.Save(path, &cfg); err != nil {
		t.Fatal(err)
	}
	return path
}

func writeAPICodexUsageFixture(t *testing.T, sessionID, cwd string, at time.Time) {
	t.Helper()
	home := t.TempDir()
	t.Setenv("HOME", home)
	dir := filepath.Join(home, ".codex", "archived_sessions")
	if err := os.MkdirAll(dir, 0o700); err != nil {
		t.Fatal(err)
	}
	rows := `{"timestamp":"` + at.Format(time.RFC3339Nano) + `","type":"session_meta","payload":{"id":"` + sessionID + `","cwd":"` + cwd + `"}}
{"timestamp":"` + at.Format(time.RFC3339Nano) + `","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":100,"output_tokens":50,"total_tokens":150},"total_token_usage":{"total_tokens":150}}}}
`
	if err := os.WriteFile(filepath.Join(dir, sessionID+".jsonl"), []byte(rows), 0o600); err != nil {
		t.Fatal(err)
	}
}
