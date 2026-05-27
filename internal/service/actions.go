package service

import (
	"context"
	"errors"
	"fmt"
	"time"

	"github.com/phaedrus/curb/internal/config"
	"github.com/phaedrus/curb/internal/ledger"
	"github.com/phaedrus/curb/internal/platform"
	"github.com/phaedrus/curb/internal/usagewatch"
	"github.com/phaedrus/curb/internal/watchdog"
)

var ErrSessionNotFound = errors.New("session not found")
var ErrInvalidAck = errors.New("invalid acknowledgement")
var ErrInvalidStop = errors.New("invalid stop request")
var ErrStopConflict = errors.New("session cannot be stopped safely")

type AckRequest struct {
	ExtendSeconds int64  `json:"extend_seconds,omitempty"`
	Reason        string `json:"reason,omitempty"`
}

type AckView struct {
	SessionKey    string    `json:"session_key"`
	ExtendSeconds int64     `json:"extend_seconds"`
	Until         time.Time `json:"until"`
	Reason        string    `json:"reason,omitempty"`
}

type StopRequest struct {
	Confirm  bool                 `json:"confirm"`
	Scope    string               `json:"scope,omitempty"`
	Reason   string               `json:"reason,omitempty"`
	Expected StopExpectedIdentity `json:"expected"`
}

type StopExpectedIdentity struct {
	PID        int32     `json:"pid"`
	StartedAt  time.Time `json:"started_at"`
	Owner      string    `json:"owner,omitempty"`
	Executable string    `json:"executable,omitempty"`
	BundleID   string    `json:"bundle_id,omitempty"`
	TeamID     string    `json:"team_id,omitempty"`
}

type StopView struct {
	SessionKey string                     `json:"session_key"`
	AgentID    string                     `json:"agent_id"`
	PID        int32                      `json:"pid"`
	StartedAt  time.Time                  `json:"started_at"`
	Owner      string                     `json:"owner,omitempty"`
	Executable string                     `json:"executable,omitempty"`
	BundleID   string                     `json:"bundle_id,omitempty"`
	TeamID     string                     `json:"team_id,omitempty"`
	Scope      string                     `json:"scope"`
	ScopePIDs  []int32                    `json:"scope_pids"`
	Result     platform.TerminationResult `json:"result"`
}

func (s *Service) AcknowledgeSession(ctx context.Context, sessionKey string, request AckRequest) (AckView, error) {
	if sessionKey == "" {
		return AckView{}, fmt.Errorf("%w: session key is required", ErrInvalidAck)
	}
	if request.ExtendSeconds < 0 {
		return AckView{}, fmt.Errorf("%w: extension must be positive", ErrInvalidAck)
	}
	cfg := s.currentConfig()
	extend := time.Duration(request.ExtendSeconds) * time.Second
	if extend <= 0 {
		extend = cfg.Defaults.AckExtension.Duration
	}
	if extend <= 0 {
		return AckView{}, fmt.Errorf("%w: ack extension must be configured", ErrInvalidAck)
	}
	if cfg.Defaults.AckExtension.Duration > 0 && extend > cfg.Defaults.AckExtension.Duration {
		extend = cfg.Defaults.AckExtension.Duration
	}
	sessionKey, err := s.canonicalSessionKey(ctx, sessionKey)
	if err != nil {
		return AckView{}, err
	}
	previous, hadPrevious, err := usagewatch.ReadSessionAck(cfg.Service.StateDir, sessionKey)
	if err != nil {
		return AckView{}, err
	}
	now := time.Now().UTC()
	ack, err := usagewatch.WriteSessionAck(cfg.Service.StateDir, sessionKey, extend, request.Reason, now)
	if err != nil {
		return AckView{}, err
	}
	if err := s.appendSessionAck(cfg, ack, extend); err != nil {
		rollbackErr := rollbackSessionAck(cfg.Service.StateDir, sessionKey, previous, hadPrevious)
		if rollbackErr != nil {
			return AckView{}, fmt.Errorf("%w; rollback failed: %v", err, rollbackErr)
		}
		return AckView{}, err
	}
	return AckView{
		SessionKey:    ack.SessionKey,
		ExtendSeconds: int64(extend / time.Second),
		Until:         ack.Until,
		Reason:        ack.Reason,
	}, nil
}

func (s *Service) StopSession(ctx context.Context, sessionKey string, request StopRequest) (StopView, error) {
	if sessionKey == "" {
		return StopView{}, fmt.Errorf("%w: session key is required", ErrInvalidStop)
	}
	if !request.Confirm {
		return StopView{}, fmt.Errorf("%w: confirmation is required", ErrInvalidStop)
	}
	scope := request.Scope
	if scope == "" {
		scope = "tree"
	}
	if scope != "tree" {
		return StopView{}, fmt.Errorf("%w: only process tree scope is supported", ErrInvalidStop)
	}
	if err := validateExpectedStopIdentity(request.Expected); err != nil {
		return StopView{}, err
	}
	cfg := s.currentConfig()
	if cfg.Mode != config.ModeEnforcement {
		return StopView{}, fmt.Errorf("%w: enforcement mode is required", ErrStopConflict)
	}
	session, err := s.freshUsageSession(sessionKey, cfg)
	if err != nil {
		return StopView{}, err
	}
	snap, err := s.capture(ctx)
	if err != nil {
		return StopView{}, err
	}
	correlation := usagewatch.Correlate(session, watchdog.New(cfg, nil).Match(snap))
	if !correlation.Matched {
		return StopView{}, fmt.Errorf("%w: no live process correlation", ErrStopConflict)
	}
	if !correlation.Agent.TerminationAllowed() {
		return StopView{}, fmt.Errorf("%w: matched agent is watch-only", ErrStopConflict)
	}
	if _, ok, err := usagewatch.ActiveSessionAck(cfg.Service.StateDir, session.Key, time.Now().UTC()); err != nil {
		return StopView{}, err
	} else if ok {
		return StopView{}, fmt.Errorf("%w: session is acknowledged", ErrStopConflict)
	}
	decision := usagewatch.EvaluateSessionDecision(session, cfg, correlation, time.Now().UTC())
	if decision.UsageState != "stop" || !decision.Actionable {
		return StopView{}, fmt.Errorf("%w: session is not an actionable stop candidate", ErrStopConflict)
	}
	if err := validateStopExpectation(request.Expected, correlation.Process); err != nil {
		return StopView{}, err
	}
	target, ok := snap.TerminationTarget(correlation.Process)
	if !ok {
		return StopView{}, fmt.Errorf("%w: process identity could not be revalidated", ErrStopConflict)
	}
	root := target.Root()
	if err := validateStopTargetIdentity(root); err != nil {
		return StopView{}, err
	}
	started := ledger.Event{
		Type:    "manual_stop_started",
		AgentID: correlation.Agent.ID,
		Mode:    string(cfg.Mode),
		Message: request.Reason,
		Data:    manualStopEventData(session, correlation, target),
	}
	if err := s.appendLedgerEvent(cfg, started); err != nil {
		return StopView{}, err
	}
	result := s.terminate(ctx, target, cfg.Usage.GracePeriod.Duration)
	data := manualStopEventData(session, correlation, target)
	data["result"] = result
	if err := s.appendLedgerEvent(cfg, ledger.Event{
		Type:    "manual_stop_completed",
		AgentID: correlation.Agent.ID,
		Mode:    string(cfg.Mode),
		Message: request.Reason,
		Data:    data,
	}); err != nil {
		return StopView{}, err
	}
	_ = s.Refresh(ctx)
	return StopView{
		SessionKey: session.Key,
		AgentID:    correlation.Agent.ID,
		PID:        root.PID,
		StartedAt:  root.Create.UTC(),
		Owner:      root.Username,
		Executable: root.Exe,
		BundleID:   root.BundleID,
		TeamID:     root.TeamID,
		Scope:      scope,
		ScopePIDs:  target.PIDs(),
		Result:     result,
	}, nil
}

func rollbackSessionAck(stateDir, sessionKey string, previous usagewatch.SessionAck, hadPrevious bool) error {
	if !hadPrevious {
		return usagewatch.DeleteSessionAck(stateDir, sessionKey)
	}
	_, err := usagewatch.WriteSessionAck(stateDir, previous.SessionKey, previous.Until.Sub(previous.CreatedAt), previous.Reason, previous.CreatedAt)
	return err
}

func validateExpectedStopIdentity(expected StopExpectedIdentity) error {
	switch {
	case expected.PID == 0:
		return fmt.Errorf("%w: expected pid is required", ErrInvalidStop)
	case expected.StartedAt.IsZero():
		return fmt.Errorf("%w: expected process start time is required", ErrInvalidStop)
	case expected.Owner == "":
		return fmt.Errorf("%w: expected owner is required", ErrInvalidStop)
	case expected.Executable == "" && expected.BundleID == "" && expected.TeamID == "":
		return fmt.Errorf("%w: expected executable or app identity is required", ErrInvalidStop)
	default:
		return nil
	}
}

func validateStopExpectation(expected StopExpectedIdentity, actual platform.Process) error {
	if actual.PID != expected.PID {
		return fmt.Errorf("%w: pid changed", ErrStopConflict)
	}
	if !actual.StartedOK || !actual.Create.Equal(expected.StartedAt) {
		return fmt.Errorf("%w: process start time changed", ErrStopConflict)
	}
	if actual.Username == "" || actual.Username != expected.Owner {
		return fmt.Errorf("%w: process owner changed", ErrStopConflict)
	}
	if expected.Executable != "" && actual.Exe != expected.Executable {
		return fmt.Errorf("%w: executable changed", ErrStopConflict)
	}
	if expected.BundleID != "" && actual.BundleID != expected.BundleID {
		return fmt.Errorf("%w: bundle id changed", ErrStopConflict)
	}
	if expected.TeamID != "" && actual.TeamID != expected.TeamID {
		return fmt.Errorf("%w: team id changed", ErrStopConflict)
	}
	return validateStopTargetIdentity(actual)
}

func validateStopTargetIdentity(proc platform.Process) error {
	if !proc.StartedOK || proc.Create.IsZero() {
		return fmt.Errorf("%w: process start time is unavailable", ErrStopConflict)
	}
	if proc.Username == "" {
		return fmt.Errorf("%w: process owner is unavailable", ErrStopConflict)
	}
	if proc.Exe == "" && proc.BundleID == "" && proc.TeamID == "" {
		return fmt.Errorf("%w: executable or app identity is unavailable", ErrStopConflict)
	}
	return nil
}

func (s *Service) freshUsageSession(key string, cfg *config.Config) (usagewatch.Session, error) {
	events, _, err := s.reader.EventsSince(time.Now().Add(-cfg.Usage.Lookback.Duration))
	if err != nil {
		return usagewatch.Session{}, err
	}
	for _, session := range usagewatch.BuildSessions(events) {
		if session.Key == key || session.SessionID == key {
			return session, nil
		}
	}
	return usagewatch.Session{}, ErrSessionNotFound
}

func (s *Service) canonicalSessionKey(ctx context.Context, key string) (string, error) {
	snapshot, err := s.Snapshot(ctx)
	if errors.Is(err, ErrSnapshotUnavailable) {
		if refreshErr := s.Refresh(ctx); refreshErr != nil {
			return "", refreshErr
		}
		snapshot, err = s.Snapshot(ctx)
	}
	if err != nil {
		return "", err
	}
	for _, session := range snapshot.Sessions {
		if session.Key == key || session.ID == key {
			return session.Key, nil
		}
	}
	return "", ErrSessionNotFound
}

func (s *Service) appendSessionAck(cfg *config.Config, ack usagewatch.SessionAck, extend time.Duration) error {
	return s.appendLedgerEvent(cfg, ledger.Event{
		Type:    "session_ack_received",
		Message: ack.Reason,
		Data: map[string]any{
			"session_key": ack.SessionKey,
			"extend":      extend.String(),
			"until":       ack.Until,
		},
	})
}

func (s *Service) appendLedgerEvent(cfg *config.Config, event ledger.Event) error {
	log, err := s.openLedger(cfg)
	if err != nil {
		return err
	}
	return log.Append(event)
}

func manualStopEventData(session usagewatch.Session, correlation usagewatch.Correlation, target platform.TerminationTarget) map[string]any {
	root := target.Root()
	return map[string]any{
		"session_key":       session.Key,
		"session_id":        session.SessionID,
		"provider":          session.Provider,
		"cwd":               session.CWD,
		"turn_tokens":       session.LastTurnTokens,
		"agent_id":          correlation.Agent.ID,
		"pid":               root.PID,
		"started_at":        root.Create.UTC(),
		"owner":             root.Username,
		"executable":        root.Exe,
		"bundle_id":         root.BundleID,
		"team_id":           root.TeamID,
		"scope":             "tree",
		"scope_pids":        target.PIDs(),
		"correlation":       correlation.Reason,
		"correlation_score": correlation.Score,
	}
}
