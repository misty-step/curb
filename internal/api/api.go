package api

import (
	"context"
	"encoding/json"
	"errors"
	"net"
	"net/http"
	"net/url"
	"strconv"
	"strings"
	"time"

	"github.com/phaedrus/curb/internal/service"
)

type Backend interface {
	Snapshot(context.Context) (service.Snapshot, error)
	Rescan(context.Context) (service.Snapshot, error)
	Events(context.Context, int) ([]service.EventView, error)
	Alerts(context.Context, int) ([]service.AlertView, error)
	Config(context.Context) (service.ConfigView, error)
	UpdateConfig(context.Context, service.ConfigUpdate) (service.ConfigView, error)
	NotificationHealth(context.Context) (service.NotificationView, error)
	TestNotification(context.Context) (service.NotificationView, error)
	Onboarding(context.Context) (service.OnboardingView, error)
	CompleteOnboarding(context.Context) (service.OnboardingView, error)
	SessionTurns(context.Context, string, service.TurnQuery) ([]service.TurnView, error)
	AcknowledgeSession(context.Context, string, service.AckRequest) (service.AckView, error)
	StopSession(context.Context, string, service.StopRequest) (service.StopView, error)
}

type Server struct {
	token   string
	backend Backend
	ui      http.Handler
}

const tokenCookie = "curb_token"

func New(token string, backend Backend) (*Server, error) {
	if strings.TrimSpace(token) == "" {
		return nil, errors.New("api token is required")
	}
	if backend == nil {
		return nil, errors.New("api backend is required")
	}
	return &Server{token: token, backend: backend}, nil
}

func (s *Server) ServeUI(ui http.Handler) {
	s.ui = ui
}

func (s *Server) Handler() http.Handler {
	mux := http.NewServeMux()
	mux.HandleFunc("/v1/health", s.handleHealth)
	mux.HandleFunc("/v1/snapshot", s.handleSnapshot)
	mux.HandleFunc("/v1/overview", s.handleOverview)
	mux.HandleFunc("/v1/agents", s.handleAgents)
	mux.HandleFunc("/v1/sessions", s.handleSessions)
	mux.HandleFunc("/v1/sessions/", s.handleSession)
	mux.HandleFunc("/v1/service/rescan", s.handleRescan)
	mux.HandleFunc("/v1/events", s.handleEvents)
	mux.HandleFunc("/v1/alerts", s.handleAlerts)
	mux.HandleFunc("/v1/config", s.handleConfig)
	mux.HandleFunc("/v1/notifications/health", s.handleNotificationHealth)
	mux.HandleFunc("/v1/notifications/test", s.handleNotificationTest)
	mux.HandleFunc("/v1/onboarding", s.handleOnboarding)
	mux.HandleFunc("/v1/onboarding/complete", s.handleOnboardingComplete)
	api := s.auth(mux)
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if strings.HasPrefix(r.URL.Path, "/v1/") {
			api.ServeHTTP(w, r)
			return
		}
		if s.ui != nil && (r.Method == http.MethodGet || r.Method == http.MethodHead) {
			s.setTokenCookie(w, r)
			s.ui.ServeHTTP(w, r)
			return
		}
		http.NotFound(w, r)
	})
}

func (s *Server) auth(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if r.URL.Path == "" || !strings.HasPrefix(r.URL.Path, "/v1/") {
			http.NotFound(w, r)
			return
		}
		applyLocalCORS(w, r)
		if r.Method == http.MethodOptions {
			w.WriteHeader(http.StatusNoContent)
			return
		}
		if !s.authorized(r) {
			writeError(w, http.StatusUnauthorized, "unauthorized")
			return
		}
		if s.usesCookieAuth(r) && unsafeMethod(r.Method) && !sameOrigin(r) {
			writeError(w, http.StatusForbidden, "forbidden")
			return
		}
		next.ServeHTTP(w, r)
	})
}

func applyLocalCORS(w http.ResponseWriter, r *http.Request) {
	origin := r.Header.Get("Origin")
	if origin == "" || !localOrigin(origin) {
		return
	}
	w.Header().Set("Access-Control-Allow-Origin", origin)
	w.Header().Set("Vary", "Origin")
	w.Header().Set("Access-Control-Allow-Headers", "Authorization, Content-Type, X-Curb-Token")
	w.Header().Set("Access-Control-Allow-Methods", "GET, POST, PUT, OPTIONS")
}

func localOrigin(origin string) bool {
	parsed, err := url.Parse(origin)
	if err != nil {
		return false
	}
	switch parsed.Scheme {
	case "http", "https", "tauri":
	default:
		return false
	}
	host := parsed.Hostname()
	return host == "localhost" || net.ParseIP(host).IsLoopback()
}

func (s *Server) authorized(r *http.Request) bool {
	if subtleTokenEqual(bearerToken(r.Header.Get("Authorization")), s.token) {
		return true
	}
	if subtleTokenEqual(r.Header.Get("X-Curb-Token"), s.token) {
		return true
	}
	cookie, err := r.Cookie(tokenCookie)
	return err == nil && subtleTokenEqual(cookie.Value, s.token)
}

func (s *Server) usesCookieAuth(r *http.Request) bool {
	if subtleTokenEqual(bearerToken(r.Header.Get("Authorization")), s.token) || subtleTokenEqual(r.Header.Get("X-Curb-Token"), s.token) {
		return false
	}
	cookie, err := r.Cookie(tokenCookie)
	return err == nil && subtleTokenEqual(cookie.Value, s.token)
}

func unsafeMethod(method string) bool {
	return method != http.MethodGet && method != http.MethodHead && method != http.MethodOptions
}

func sameOrigin(r *http.Request) bool {
	origin := r.Header.Get("Origin")
	if origin == "" {
		return false
	}
	parsed, err := url.Parse(origin)
	if err != nil {
		return false
	}
	return strings.EqualFold(parsed.Scheme, requestScheme(r)) && strings.EqualFold(parsed.Host, r.Host)
}

func requestScheme(r *http.Request) string {
	if r.TLS != nil {
		return "https"
	}
	return "http"
}

func (s *Server) setTokenCookie(w http.ResponseWriter, r *http.Request) {
	http.SetCookie(w, &http.Cookie{
		Name:     tokenCookie,
		Value:    s.token,
		Path:     "/v1/",
		HttpOnly: true,
		SameSite: http.SameSiteStrictMode,
		Secure:   r.TLS != nil,
	})
}

func (s *Server) handleHealth(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		writeError(w, http.StatusMethodNotAllowed, "method not allowed")
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"ok": true, "app": "curb", "api_version": 1})
}

func (s *Server) handleOverview(w http.ResponseWriter, r *http.Request) {
	snapshot, ok := s.currentSnapshot(w, r)
	if !ok {
		return
	}
	writeJSON(w, http.StatusOK, snapshot.Overview)
}

func (s *Server) handleSnapshot(w http.ResponseWriter, r *http.Request) {
	snapshot, ok := s.currentSnapshot(w, r)
	if !ok {
		return
	}
	writeJSON(w, http.StatusOK, snapshot)
}

func (s *Server) handleAgents(w http.ResponseWriter, r *http.Request) {
	snapshot, ok := s.currentSnapshot(w, r)
	if !ok {
		return
	}
	writeJSON(w, http.StatusOK, snapshot.Agents)
}

func (s *Server) handleSessions(w http.ResponseWriter, r *http.Request) {
	if r.URL.Path != "/v1/sessions" {
		http.NotFound(w, r)
		return
	}
	snapshot, ok := s.currentSnapshot(w, r)
	if !ok {
		return
	}
	writeJSON(w, http.StatusOK, snapshot.Sessions)
}

func (s *Server) handleSession(w http.ResponseWriter, r *http.Request) {
	rest := strings.TrimPrefix(r.URL.EscapedPath(), "/v1/sessions/")
	parts := strings.Split(strings.Trim(rest, "/"), "/")
	if len(parts) == 0 || parts[0] == "" {
		http.NotFound(w, r)
		return
	}
	key, err := url.PathUnescape(parts[0])
	if err != nil {
		writeError(w, http.StatusBadRequest, "invalid session key")
		return
	}
	if len(parts) == 2 && parts[1] == "ack" {
		if r.Method != http.MethodPost {
			writeError(w, http.StatusMethodNotAllowed, "method not allowed")
			return
		}
		var ack service.AckRequest
		if err := json.NewDecoder(r.Body).Decode(&ack); err != nil {
			writeError(w, http.StatusBadRequest, "invalid ack request")
			return
		}
		view, err := s.backend.AcknowledgeSession(r.Context(), key, ack)
		if err != nil {
			if errors.Is(err, service.ErrSessionNotFound) {
				writeError(w, http.StatusNotFound, "session not found")
				return
			}
			if errors.Is(err, service.ErrInvalidAck) {
				writeError(w, http.StatusBadRequest, err.Error())
				return
			}
			writeError(w, http.StatusInternalServerError, err.Error())
			return
		}
		writeJSON(w, http.StatusOK, view)
		return
	}
	if len(parts) == 2 && parts[1] == "stop" {
		if r.Method != http.MethodPost {
			writeError(w, http.StatusMethodNotAllowed, "method not allowed")
			return
		}
		var stop service.StopRequest
		if err := json.NewDecoder(r.Body).Decode(&stop); err != nil {
			writeError(w, http.StatusBadRequest, "invalid stop request")
			return
		}
		view, err := s.backend.StopSession(r.Context(), key, stop)
		if err != nil {
			if errors.Is(err, service.ErrSessionNotFound) {
				writeError(w, http.StatusNotFound, "session not found")
				return
			}
			if errors.Is(err, service.ErrInvalidStop) {
				writeError(w, http.StatusBadRequest, err.Error())
				return
			}
			if errors.Is(err, service.ErrStopConflict) {
				writeError(w, http.StatusConflict, err.Error())
				return
			}
			writeError(w, http.StatusInternalServerError, err.Error())
			return
		}
		writeJSON(w, http.StatusOK, view)
		return
	}
	if len(parts) == 2 && parts[1] == "turns" {
		if r.Method != http.MethodGet {
			writeError(w, http.StatusMethodNotAllowed, "method not allowed")
			return
		}
		turns, err := s.backend.SessionTurns(r.Context(), key, service.TurnQuery{
			Since: sinceParam(r),
			Limit: limitParam(r, 200),
		})
		if err != nil {
			if errors.Is(err, service.ErrSessionNotFound) {
				writeError(w, http.StatusNotFound, "session not found")
				return
			}
			writeError(w, http.StatusInternalServerError, err.Error())
			return
		}
		writeJSON(w, http.StatusOK, turns)
		return
	}
	snapshot, ok := s.currentSnapshot(w, r)
	if !ok {
		return
	}
	session, found := findSession(snapshot.Sessions, key)
	if !found {
		writeError(w, http.StatusNotFound, "session not found")
		return
	}
	if len(parts) == 1 {
		if r.Method != http.MethodGet {
			writeError(w, http.StatusMethodNotAllowed, "method not allowed")
			return
		}
		writeJSON(w, http.StatusOK, session)
		return
	}
	http.NotFound(w, r)
}

func (s *Server) handleRescan(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		writeError(w, http.StatusMethodNotAllowed, "method not allowed")
		return
	}
	snapshot, err := s.backend.Rescan(r.Context())
	if err != nil {
		writeError(w, http.StatusInternalServerError, err.Error())
		return
	}
	writeJSON(w, http.StatusOK, snapshot)
}

func (s *Server) handleEvents(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		writeError(w, http.StatusMethodNotAllowed, "method not allowed")
		return
	}
	events, err := s.backend.Events(r.Context(), limitParam(r, 200))
	if err != nil {
		writeError(w, http.StatusInternalServerError, err.Error())
		return
	}
	writeJSON(w, http.StatusOK, events)
}

func (s *Server) handleAlerts(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		writeError(w, http.StatusMethodNotAllowed, "method not allowed")
		return
	}
	alerts, err := s.backend.Alerts(r.Context(), limitParam(r, 50))
	if err != nil {
		writeError(w, http.StatusInternalServerError, err.Error())
		return
	}
	writeJSON(w, http.StatusOK, alerts)
}

func (s *Server) handleConfig(w http.ResponseWriter, r *http.Request) {
	switch r.Method {
	case http.MethodGet:
		view, err := s.backend.Config(r.Context())
		if err != nil {
			writeError(w, http.StatusInternalServerError, err.Error())
			return
		}
		writeJSON(w, http.StatusOK, view)
	case http.MethodPut:
		var update service.ConfigUpdate
		if err := json.NewDecoder(r.Body).Decode(&update); err != nil {
			writeError(w, http.StatusBadRequest, "invalid config update")
			return
		}
		view, err := s.backend.UpdateConfig(r.Context(), update)
		if err != nil {
			writeError(w, http.StatusBadRequest, err.Error())
			return
		}
		writeJSON(w, http.StatusOK, view)
	default:
		writeError(w, http.StatusMethodNotAllowed, "method not allowed")
	}
}

func (s *Server) handleNotificationHealth(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		writeError(w, http.StatusMethodNotAllowed, "method not allowed")
		return
	}
	view, err := s.backend.NotificationHealth(r.Context())
	if err != nil {
		writeError(w, http.StatusInternalServerError, err.Error())
		return
	}
	writeJSON(w, http.StatusOK, view)
}

func (s *Server) handleNotificationTest(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		writeError(w, http.StatusMethodNotAllowed, "method not allowed")
		return
	}
	view, err := s.backend.TestNotification(r.Context())
	if err != nil {
		switch {
		case errors.Is(err, service.ErrNotificationsDisabled):
			writeJSON(w, http.StatusConflict, view)
		case errors.Is(err, service.ErrNotificationsUnavailable):
			writeJSON(w, http.StatusServiceUnavailable, view)
		default:
			writeJSON(w, http.StatusServiceUnavailable, view)
		}
		return
	}
	writeJSON(w, http.StatusOK, view)
}

func (s *Server) handleOnboarding(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodGet {
		writeError(w, http.StatusMethodNotAllowed, "method not allowed")
		return
	}
	view, err := s.backend.Onboarding(r.Context())
	if err != nil {
		writeError(w, http.StatusInternalServerError, err.Error())
		return
	}
	writeJSON(w, http.StatusOK, view)
}

func (s *Server) handleOnboardingComplete(w http.ResponseWriter, r *http.Request) {
	if r.Method != http.MethodPost {
		writeError(w, http.StatusMethodNotAllowed, "method not allowed")
		return
	}
	view, err := s.backend.CompleteOnboarding(r.Context())
	if err != nil {
		writeError(w, http.StatusInternalServerError, err.Error())
		return
	}
	writeJSON(w, http.StatusOK, view)
}

func (s *Server) currentSnapshot(w http.ResponseWriter, r *http.Request) (service.Snapshot, bool) {
	if r.Method != http.MethodGet {
		writeError(w, http.StatusMethodNotAllowed, "method not allowed")
		return service.Snapshot{}, false
	}
	snapshot, err := s.backend.Snapshot(r.Context())
	if err != nil {
		writeError(w, http.StatusInternalServerError, err.Error())
		return service.Snapshot{}, false
	}
	return snapshot, true
}

func limitParam(r *http.Request, fallback int) int {
	raw := r.URL.Query().Get("limit")
	if raw == "" {
		return fallback
	}
	parsed, err := strconv.Atoi(raw)
	if err != nil || parsed <= 0 {
		return fallback
	}
	if parsed > 1000 {
		return 1000
	}
	return parsed
}

func sinceParam(r *http.Request) time.Time {
	raw := strings.TrimSpace(r.URL.Query().Get("since"))
	if raw == "" {
		return time.Time{}
	}
	if duration, err := time.ParseDuration(raw); err == nil {
		return time.Now().Add(-duration)
	}
	parsed, err := time.Parse(time.RFC3339, raw)
	if err != nil {
		return time.Time{}
	}
	return parsed
}

func findSession(sessions []service.SessionView, key string) (service.SessionView, bool) {
	for _, session := range sessions {
		if session.Key == key || session.ID == key {
			return session, true
		}
	}
	return service.SessionView{}, false
}

func writeJSON(w http.ResponseWriter, status int, value any) {
	w.Header().Set("Content-Type", "application/json")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(value)
}

func writeError(w http.ResponseWriter, status int, message string) {
	writeJSON(w, status, map[string]string{"error": message})
}

func bearerToken(value string) string {
	prefix := "Bearer "
	if !strings.HasPrefix(value, prefix) {
		return ""
	}
	return strings.TrimSpace(strings.TrimPrefix(value, prefix))
}

func subtleTokenEqual(got, want string) bool {
	if len(got) != len(want) {
		return false
	}
	var diff byte
	for i := range got {
		diff |= got[i] ^ want[i]
	}
	return diff == 0
}
