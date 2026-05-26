package service

import (
	"context"
	"errors"
	"fmt"
	"os"
	"path/filepath"
)

type OnboardingView struct {
	Required              bool                 `json:"required"`
	ConfigPath            string               `json:"config_path,omitempty"`
	Mode                  string               `json:"mode"`
	Action                string               `json:"action"`
	ModeCanTerminate      bool                 `json:"mode_can_terminate"`
	DetectedProviders     []string             `json:"detected_providers"`
	DetectedWorkers       []string             `json:"detected_workers"`
	EnforceableAgentTypes int                  `json:"enforceable_agent_types"`
	WatchOnlyAgentTypes   int                  `json:"watch_only_agent_types"`
	Notifications         NotificationView     `json:"notifications"`
	Capabilities          PlatformCapabilities `json:"capabilities"`
	Sources               []SourceHealth       `json:"sources"`
	FinalSentence         string               `json:"final_sentence"`
	Steps                 []OnboardingStepView `json:"steps"`
}

type OnboardingStepView struct {
	ID      string `json:"id"`
	Label   string `json:"label"`
	Status  string `json:"status"`
	Message string `json:"message"`
}

func (s *Service) Onboarding(ctx context.Context) (OnboardingView, error) {
	cfg := s.currentConfig()
	configView := NewConfigView(s.configPath, cfg)
	notifications, err := s.NotificationHealth(ctx)
	if err != nil {
		return OnboardingView{}, err
	}
	snapshot, snapshotErr := s.onboardingSnapshot(ctx)
	capabilities := snapshot.Overview.Capabilities
	if capabilities.Platform == "" || snapshotErr != nil {
		capabilities = s.platformCapabilities(cfg, nil, snapshotErr, notifications, nil)
	} else {
		capabilities.Notifications = notificationCapabilityView(notifications)
	}

	view := OnboardingView{
		Required:          !s.onboardingCompleted(),
		ConfigPath:        configView.Path,
		Mode:              configView.Mode,
		Action:            actionLabel(cfg.Mode),
		Notifications:     notifications,
		Capabilities:      capabilities,
		Sources:           append([]SourceHealth(nil), snapshot.Overview.Sources...),
		DetectedProviders: detectedProviders(snapshot),
		DetectedWorkers:   detectedWorkers(snapshot),
		FinalSentence:     onboardingFinalSentence(configView),
	}
	for _, agent := range configView.Agents {
		if agent.Terminates {
			view.EnforceableAgentTypes++
		} else {
			view.WatchOnlyAgentTypes++
		}
	}
	view.ModeCanTerminate = configView.Mode == "enforcement" && view.EnforceableAgentTypes > 0
	view.Steps = []OnboardingStepView{
		configStep(configView),
		agentStep(configView),
		sourceStep(snapshot.Overview.Sources, snapshotErr, capabilities.ProcessCapture),
		notificationStep(configView.Mode, notifications),
		safetyStep(configView),
	}
	return view, nil
}

func (s *Service) onboardingSnapshot(ctx context.Context) (Snapshot, error) {
	snapshot, err := s.Snapshot(ctx)
	if err == nil {
		return snapshot, nil
	}
	if errors.Is(err, ErrSnapshotUnavailable) {
		if refreshErr := s.Refresh(ctx); refreshErr != nil {
			return Snapshot{}, refreshErr
		}
		return s.Snapshot(ctx)
	}
	return Snapshot{}, err
}

func (s *Service) CompleteOnboarding(ctx context.Context) (OnboardingView, error) {
	cfg := s.currentConfig()
	if err := os.MkdirAll(cfg.Service.StateDir, 0o700); err != nil {
		return OnboardingView{}, err
	}
	if err := os.WriteFile(onboardingMarkerPath(cfg.Service.StateDir), []byte("complete\n"), 0o600); err != nil {
		return OnboardingView{}, err
	}
	return s.Onboarding(ctx)
}

func (s *Service) onboardingCompleted() bool {
	cfg := s.currentConfig()
	_, err := os.Stat(onboardingMarkerPath(cfg.Service.StateDir))
	return err == nil
}

func onboardingMarkerPath(stateDir string) string {
	return filepath.Join(stateDir, "onboarding.complete")
}

func configStep(config ConfigView) OnboardingStepView {
	if config.Path == "" {
		return OnboardingStepView{ID: "config", Label: "Config", Status: "action", Message: "config path is not available"}
	}
	return OnboardingStepView{ID: "config", Label: "Config", Status: "done", Message: "using " + config.Path}
}

func agentStep(config ConfigView) OnboardingStepView {
	if len(config.Agents) == 0 {
		return OnboardingStepView{ID: "agents", Label: "Agents", Status: "action", Message: "no agent matchers are configured"}
	}
	return OnboardingStepView{ID: "agents", Label: "Agents", Status: "done", Message: agentCountMessage(config.Agents)}
}

func sourceStep(sources []SourceHealth, snapshotErr error, capture CapabilityView) OnboardingStepView {
	if snapshotErr != nil {
		return OnboardingStepView{ID: "sources", Label: "Sources", Status: "action", Message: "unable to scan current agent state: " + snapshotErr.Error()}
	}
	if capture.Status == "error" {
		return OnboardingStepView{ID: "sources", Label: "Sources", Status: "action", Message: capture.Message}
	}
	if len(sources) == 0 {
		return OnboardingStepView{ID: "sources", Label: "Sources", Status: "waiting", Message: "usage sources have not been scanned yet"}
	}
	for _, source := range sources {
		if source.Error != "" {
			return OnboardingStepView{ID: "sources", Label: "Sources", Status: "action", Message: source.Provider + ": " + source.Error}
		}
	}
	events := 0
	files := 0
	for _, source := range sources {
		events += source.Events
		files += source.Files
	}
	if events == 0 {
		return OnboardingStepView{ID: "sources", Label: "Sources", Status: "waiting", Message: "scanned usage sources; no local usage events found yet"}
	}
	return OnboardingStepView{ID: "sources", Label: "Sources", Status: "done", Message: formatCount(events, "usage event") + " from " + formatCount(files, "file")}
}

func notificationStep(mode string, notifications NotificationView) OnboardingStepView {
	if mode == "visibility" {
		return OnboardingStepView{ID: "notifications", Label: "Notifications", Status: "waiting", Message: "visibility mode records activity without requiring notifications"}
	}
	if !notifications.Enabled {
		return OnboardingStepView{ID: "notifications", Label: "Notifications", Status: "action", Message: "local notifications are disabled"}
	}
	if !notifications.Available {
		return OnboardingStepView{ID: "notifications", Label: "Notifications", Status: "action", Message: notifications.Message}
	}
	return OnboardingStepView{ID: "notifications", Label: "Notifications", Status: "done", Message: notifications.Message}
}

func safetyStep(config ConfigView) OnboardingStepView {
	for _, agent := range config.Agents {
		if agent.Kind == "app" && agent.Terminates {
			return OnboardingStepView{ID: "safety", Label: "Safety", Status: "action", Message: agent.Label + " is an app root but is enforceable"}
		}
	}
	return OnboardingStepView{ID: "safety", Label: "Safety", Status: "done", Message: "desktop app roots are watch-only; Curb stops only enforceable workers"}
}

func detectedProviders(snapshot Snapshot) []string {
	seen := map[string]bool{}
	var out []string
	for _, source := range snapshot.Overview.Sources {
		if source.Provider != "" && !seen[source.Provider] {
			seen[source.Provider] = true
			out = append(out, source.Provider)
		}
	}
	for _, session := range snapshot.Sessions {
		if session.Provider != "" && !seen[session.Provider] {
			seen[session.Provider] = true
			out = append(out, session.Provider)
		}
	}
	return out
}

func detectedWorkers(snapshot Snapshot) []string {
	seen := map[string]bool{}
	var out []string
	for _, agent := range snapshot.Agents {
		label := agent.Label
		if label == "" {
			label = agent.ID
		}
		if label != "" && !seen[label] {
			seen[label] = true
			out = append(out, label)
		}
	}
	return out
}

func onboardingFinalSentence(config ConfigView) string {
	switch config.Mode {
	case "alert":
		return "Curb will notify on high-token turns. It will not stop any process in Alert mode. Desktop app roots are watch-only."
	case "enforcement":
		return "Curb can stop only correlated enforceable workers after policy and grace checks. Desktop app roots are watch-only."
	default:
		return "Curb will record local agent activity. It will not notify or stop any process in Visibility mode. Desktop app roots are watch-only."
	}
}

func agentCountMessage(agents []ConfigAgentView) string {
	enforceable := 0
	watchOnly := 0
	for _, agent := range agents {
		if agent.Terminates {
			enforceable++
		} else {
			watchOnly++
		}
	}
	if watchOnly == 0 {
		return formatCount(enforceable, "enforceable agent")
	}
	return formatCount(enforceable, "enforceable agent") + ", " + formatCount(watchOnly, "watch-only agent")
}

func formatCount(count int, singular string) string {
	if count == 1 {
		return "1 " + singular
	}
	return fmt.Sprintf("%d %ss", count, singular)
}
