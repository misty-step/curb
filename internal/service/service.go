package service

import (
	"context"
	"runtime"
	"sync"
	"time"

	"github.com/phaedrus/curb/internal/config"
	"github.com/phaedrus/curb/internal/ledger"
	"github.com/phaedrus/curb/internal/platform"
	"github.com/phaedrus/curb/internal/usage"
	"github.com/phaedrus/curb/internal/usagewatch"
)

type CaptureFunc func(context.Context) (*platform.Snapshot, error)
type NotifyFunc func(string, string) error
type NotificationCapabilityFunc func() platform.NotificationCapability
type TerminateFunc func(context.Context, platform.TerminationTarget, time.Duration) platform.TerminationResult

type Service struct {
	configPath string
	capture    CaptureFunc
	notify     NotifyFunc
	notifyCaps NotificationCapabilityFunc
	terminate  TerminateFunc
	cache      *SnapshotCache
	reader     *usage.Reader

	mu      sync.RWMutex
	cfg     *config.Config
	usage   *usagewatch.Service
	onEvent func(ledger.Event)
	scanMu  sync.Mutex

	notificationMu sync.Mutex
	notification   NotificationView
}

func New(configPath string, capture CaptureFunc) (*Service, error) {
	if capture == nil {
		capture = platform.Capture
	}
	cfg, err := config.Load(configPath)
	if err != nil {
		return nil, err
	}
	s := &Service{
		configPath: configPath,
		capture:    capture,
		notify:     platform.Notify,
		notifyCaps: platform.NotificationCapabilityStatus,
		terminate:  platform.TerminateTree,
		reader:     usage.NewReaderWithState("", cfg.Service.StateDir),
		cfg:        cfg,
	}
	s.cache = NewSnapshotCache(s.buildSnapshot)
	usageService, _, err := s.buildUsageWatch(cfg)
	if err != nil {
		return nil, err
	}
	s.usage = usageService
	return s, nil
}

func (s *Service) Start(ctx context.Context) {
	_ = s.Run(ctx)
}

func (s *Service) Run(ctx context.Context) error {
	if err := s.Scan(ctx); err != nil {
		return err
	}
	for {
		interval := s.currentConfig().Usage.ScanInterval.Duration
		if interval <= 0 {
			interval = 5 * time.Second
		}
		timer := time.NewTimer(interval)
		select {
		case <-ctx.Done():
			timer.Stop()
			return nil
		case <-timer.C:
			_ = s.Scan(ctx)
		}
	}
}

func (s *Service) OnEvent(fn func(ledger.Event)) {
	s.mu.Lock()
	defer s.mu.Unlock()
	s.onEvent = fn
	if s.usage != nil {
		s.usage.OnEvent(fn)
	}
}

func (s *Service) Scan(ctx context.Context) error {
	if err := s.Refresh(ctx); err != nil {
		return err
	}
	return s.ScanPolicy(ctx)
}

func (s *Service) Rescan(ctx context.Context) (Snapshot, error) {
	if err := s.Scan(ctx); err != nil {
		return Snapshot{}, err
	}
	if err := s.Refresh(ctx); err != nil {
		return Snapshot{}, err
	}
	return s.Snapshot(ctx)
}

func (s *Service) ScanPolicy(ctx context.Context) error {
	s.mu.RLock()
	usageService := s.usage
	s.mu.RUnlock()
	if usageService == nil {
		return nil
	}
	s.scanMu.Lock()
	defer s.scanMu.Unlock()
	return usageService.Scan(ctx)
}

func (s *Service) Refresh(ctx context.Context) error {
	return s.cache.Refresh(ctx)
}

func (s *Service) Snapshot(ctx context.Context) (Snapshot, error) {
	return s.cache.Current(ctx)
}

func (s *Service) SnapshotSince(ctx context.Context, since time.Time) (Snapshot, error) {
	return s.buildSnapshotSince(ctx, since)
}

func (s *Service) SessionTurns(_ context.Context, key string, query TurnQuery) ([]TurnView, error) {
	cfg := s.currentConfig()
	lookbackStart := time.Now().Add(-cfg.Usage.Lookback.Duration)
	readSince := lookbackStart
	if !query.Since.IsZero() && query.Since.Before(readSince) {
		readSince = query.Since
	}
	events, _, err := s.reader.EventsSince(readSince)
	if err != nil {
		return nil, err
	}
	sessions := usagewatch.BuildSessions(events)
	canonical := ""
	for _, session := range sessions {
		if session.Key == key || session.SessionID == key {
			canonical = session.Key
			break
		}
	}
	if canonical == "" {
		return nil, ErrSessionNotFound
	}
	turnSince := query.Since
	if turnSince.IsZero() {
		turnSince = lookbackStart
	}
	return limitTurns(buildTurnsSince(events, turnSince)[canonical], query.Limit), nil
}

func (s *Service) recentLedgerEvents(limit int) ([]ledger.Event, error) {
	events, err := ledger.Read(s.currentConfig().Ledger.Path)
	if err != nil {
		return nil, err
	}
	if events == nil {
		events = []ledger.Event{}
	}
	if limit <= 0 || limit >= len(events) {
		return events, nil
	}
	return events[len(events)-limit:], nil
}

func (s *Service) Config(context.Context) (ConfigView, error) {
	return NewConfigView(s.configPath, s.currentConfig()), nil
}

func (s *Service) UpdateConfig(ctx context.Context, update ConfigUpdate) (ConfigView, error) {
	s.mu.Lock()
	next := config.Clone(s.cfg)
	if err := ApplyConfigUpdate(next, update); err != nil {
		s.mu.Unlock()
		return ConfigView{}, err
	}
	usageService, log, err := s.buildUsageWatch(next)
	if err != nil {
		s.mu.Unlock()
		return ConfigView{}, err
	}
	s.scanMu.Lock()
	defer s.scanMu.Unlock()
	if err := config.Save(s.configPath, next); err != nil {
		s.mu.Unlock()
		return ConfigView{}, err
	}
	s.reader.SetStateDir(next.Service.StateDir)
	if s.usage != nil && usageService != nil {
		s.usage.Reconfigure(config.Clone(next), log)
		usageService = s.usage
	}
	s.cfg = next
	s.usage = usageService
	view := NewConfigView(s.configPath, next)
	s.mu.Unlock()
	_ = s.Refresh(ctx)
	return view, nil
}

func (s *Service) StateDir() string {
	return s.currentConfig().Service.StateDir
}

func (s *Service) buildSnapshot(ctx context.Context) (Snapshot, error) {
	cfg := s.currentConfig()
	return s.buildSnapshotSince(ctx, time.Now().Add(-cfg.Usage.Lookback.Duration))
}

func (s *Service) buildSnapshotSince(ctx context.Context, since time.Time) (Snapshot, error) {
	cfg := s.currentConfig()
	events, sources, err := s.reader.EventsSince(since)
	if err != nil {
		return Snapshot{}, err
	}
	now := time.Now().UTC()
	snap, captureErr := s.capture(ctx)
	if captureErr != nil {
		snap = &platform.Snapshot{
			At:        now,
			Platform:  runtime.GOOS,
			Processes: map[int32]platform.Process{},
			Children:  map[int32][]int32{},
		}
	}
	out := BuildSnapshot(cfg, snap, events, sources, now)
	out.Overview.Capabilities = s.platformCapabilities(cfg, snap, captureErr, newNotificationView(cfg.Alerts.LocalNotifications, s.notificationCapability()), out.Agents)
	return out, nil
}

func (s *Service) currentConfig() *config.Config {
	s.mu.RLock()
	defer s.mu.RUnlock()
	return config.Clone(s.cfg)
}

func (s *Service) buildUsageWatch(cfg *config.Config) (*usagewatch.Service, *ledger.Ledger, error) {
	if !cfg.Usage.IsEnabled() {
		return nil, nil, nil
	}
	log, err := ledger.Open(cfg.Ledger.Path)
	if err != nil {
		return nil, nil, err
	}
	usageService := usagewatch.New(config.Clone(cfg), log)
	usageService.SetCapture(usagewatch.Capture(s.capture))
	usageService.SetNotify(usagewatch.Notify(s.notify))
	usageService.SetTerminate(usagewatch.Terminate(s.terminate))
	usageService.SetReader(usagewatch.EventReader(s.reader.EventsSince))
	usageService.OnEvent(s.onEvent)
	return usageService, log, nil
}
