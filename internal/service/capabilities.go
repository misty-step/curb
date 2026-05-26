package service

import (
	"runtime"

	"github.com/phaedrus/curb/internal/config"
	"github.com/phaedrus/curb/internal/platform"
)

type PlatformCapabilities struct {
	Platform        string         `json:"platform"`
	Notifications   CapabilityView `json:"notifications"`
	ProcessCapture  CapabilityView `json:"process_capture"`
	ProcessIdentity CapabilityView `json:"process_identity"`
	Enforcement     CapabilityView `json:"enforcement"`
}

type CapabilityView struct {
	Available bool   `json:"available"`
	Status    string `json:"status"`
	Message   string `json:"message"`
}

func (s *Service) platformCapabilities(cfg *config.Config, snap *platform.Snapshot, snapshotErr error, notifications NotificationView, agents []AgentView) PlatformCapabilities {
	caps := PlatformCapabilities{
		Platform:        runtime.GOOS,
		Notifications:   notificationCapabilityView(notifications),
		ProcessCapture:  processCaptureCapability(snap, snapshotErr),
		ProcessIdentity: processIdentityCapability(snap, snapshotErr),
		Enforcement:     enforcementCapability(cfg, snap, snapshotErr, agents),
	}
	if snap != nil && snap.Platform != "" {
		caps.Platform = snap.Platform
	}
	return caps
}

func notificationCapabilityView(view NotificationView) CapabilityView {
	return CapabilityView{
		Available: view.Enabled && view.Available,
		Status:    view.Status,
		Message:   view.Message,
	}
}

func processCaptureCapability(snap *platform.Snapshot, err error) CapabilityView {
	if err != nil {
		return CapabilityView{Available: false, Status: "error", Message: "process capture failed: " + err.Error()}
	}
	if snap == nil {
		return CapabilityView{Available: false, Status: "waiting", Message: "process capture has not run yet"}
	}
	return CapabilityView{Available: true, Status: "ready", Message: formatCount(len(snap.Processes), "process") + " captured"}
}

func processIdentityCapability(snap *platform.Snapshot, err error) CapabilityView {
	if err != nil {
		return CapabilityView{Available: false, Status: "error", Message: "process identity unavailable until capture succeeds"}
	}
	if snap == nil {
		return CapabilityView{Available: false, Status: "waiting", Message: "process identity has not been sampled yet"}
	}
	if len(snap.Processes) == 0 {
		return CapabilityView{Available: false, Status: "waiting", Message: "no processes captured yet"}
	}
	withIdentityBoundary := 0
	for _, proc := range snap.Processes {
		if proc.StartedOK && proc.HasSemanticIdentity() {
			withIdentityBoundary++
		}
	}
	if withIdentityBoundary == 0 {
		return CapabilityView{Available: false, Status: "degraded", Message: "captured processes lack start-time or executable identity evidence"}
	}
	return CapabilityView{Available: true, Status: "ready", Message: formatCount(withIdentityBoundary, "process") + " with identity evidence"}
}

func enforcementCapability(cfg *config.Config, snap *platform.Snapshot, err error, agents []AgentView) CapabilityView {
	if cfg.Mode != config.ModeEnforcement {
		return CapabilityView{Available: false, Status: "disabled", Message: "current mode will not terminate processes"}
	}
	if enforceableAgentTypes(cfg) == 0 {
		return CapabilityView{Available: false, Status: "blocked", Message: "no enforceable agent types are configured"}
	}
	identity := processIdentityCapability(snap, err)
	if !identity.Available {
		return CapabilityView{Available: false, Status: "blocked", Message: "process identity is not strong enough for enforcement"}
	}
	if !hasLiveEnforceableAgent(cfg, agents) {
		return CapabilityView{Available: false, Status: "blocked", Message: "no live enforceable worker is currently matched"}
	}
	return CapabilityView{Available: true, Status: "ready", Message: "enforcement can target revalidated worker processes only"}
}

func enforceableAgentTypes(cfg *config.Config) int {
	count := 0
	for _, agent := range cfg.Agents {
		if agent.TerminationAllowed() {
			count++
		}
	}
	return count
}

func hasLiveEnforceableAgent(cfg *config.Config, agents []AgentView) bool {
	enforceable := map[string]bool{}
	for _, agent := range cfg.Agents {
		if agent.TerminationAllowed() {
			enforceable[agent.ID] = true
		}
	}
	for _, agent := range agents {
		if enforceable[agent.ID] && agent.PID > 0 && agent.ProcessStartedAt != nil {
			return true
		}
	}
	return false
}
