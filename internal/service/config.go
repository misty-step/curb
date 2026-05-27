package service

import (
	"errors"
	"time"

	"github.com/phaedrus/curb/internal/config"
)

type ConfigView struct {
	Path                string            `json:"path,omitempty"`
	MachineID           string            `json:"machine_id,omitempty"`
	Mode                string            `json:"mode"`
	UsageEnabled        bool              `json:"usage_enabled"`
	WarnTurnTokens      int64             `json:"warn_turn_tokens"`
	KillTurnTokens      int64             `json:"kill_turn_tokens"`
	UsageWindowSeconds  int64             `json:"usage_window_seconds"`
	UsageScanSeconds    int64             `json:"usage_scan_seconds"`
	LookbackSeconds     int64             `json:"lookback_seconds"`
	ProcessWarnSeconds  int64             `json:"process_warn_seconds"`
	ProcessKillSeconds  int64             `json:"process_kill_seconds"`
	AckExtensionSeconds int64             `json:"ack_extension_seconds"`
	LocalNotifications  bool              `json:"local_notifications"`
	LedgerForwardURL    string            `json:"ledger_forward_url,omitempty"`
	Agents              []ConfigAgentView `json:"agents"`
}

type ConfigAgentView struct {
	ID          string `json:"id"`
	Label       string `json:"label"`
	Family      string `json:"family"`
	Kind        string `json:"kind"`
	Terminates  bool   `json:"terminates"`
	Description string `json:"description"`
}

type ConfigUpdate struct {
	Mode               *string `json:"mode,omitempty"`
	UsageEnabled       *bool   `json:"usage_enabled,omitempty"`
	WarnTurnTokens     *int64  `json:"warn_turn_tokens,omitempty"`
	KillTurnTokens     *int64  `json:"kill_turn_tokens,omitempty"`
	UsageWindowSeconds *int64  `json:"usage_window_seconds,omitempty"`
	UsageScanSeconds   *int64  `json:"usage_scan_seconds,omitempty"`
	LookbackSeconds    *int64  `json:"lookback_seconds,omitempty"`
	ProcessWarnSeconds *int64  `json:"process_warn_seconds,omitempty"`
	ProcessKillSeconds *int64  `json:"process_kill_seconds,omitempty"`
	LocalNotifications *bool   `json:"local_notifications,omitempty"`
}

func NewConfigView(path string, cfg *config.Config, machineID string) ConfigView {
	agents := make([]ConfigAgentView, 0, len(cfg.Agents))
	for _, agent := range cfg.Agents {
		kind := agent.Kind
		if kind == "" {
			kind = config.AgentKindProcess
			if !agent.TerminationAllowed() {
				kind = config.AgentKindApp
			}
		}
		agents = append(agents, ConfigAgentView{
			ID:          agent.ID,
			Label:       agent.Label,
			Family:      agent.Family,
			Kind:        kind,
			Terminates:  agent.TerminationAllowed(),
			Description: agentConfigDescription(agent),
		})
	}
	return ConfigView{
		Path:                path,
		MachineID:           machineID,
		Mode:                string(cfg.Mode),
		UsageEnabled:        cfg.Usage.IsEnabled(),
		WarnTurnTokens:      cfg.Usage.WarnTurnTokens,
		KillTurnTokens:      cfg.Usage.KillTurnTokens,
		UsageWindowSeconds:  int64(cfg.Usage.Window.Duration / time.Second),
		UsageScanSeconds:    int64(cfg.Usage.ScanInterval.Duration / time.Second),
		LookbackSeconds:     int64(cfg.Usage.Lookback.Duration / time.Second),
		ProcessWarnSeconds:  int64(cfg.Defaults.WarnAfter.Duration / time.Second),
		ProcessKillSeconds:  int64(cfg.Defaults.KillAfter.Duration / time.Second),
		AckExtensionSeconds: int64(cfg.Defaults.AckExtension.Duration / time.Second),
		LocalNotifications:  cfg.Alerts.LocalNotifications,
		LedgerForwardURL:    cfg.Ledger.ForwardURL,
		Agents:              agents,
	}
}

func ApplyConfigUpdate(cfg *config.Config, update ConfigUpdate) error {
	if update.Mode != nil {
		cfg.Mode = config.Mode(*update.Mode)
	}
	if update.UsageEnabled != nil {
		cfg.Usage.Enabled = boolPtr(*update.UsageEnabled)
	}
	if update.WarnTurnTokens != nil {
		cfg.Usage.WarnTurnTokens = *update.WarnTurnTokens
	}
	if update.KillTurnTokens != nil {
		cfg.Usage.KillTurnTokens = *update.KillTurnTokens
	}
	if update.UsageWindowSeconds != nil {
		cfg.Usage.Window.Duration = secondsDuration(*update.UsageWindowSeconds)
	}
	if update.UsageScanSeconds != nil {
		cfg.Usage.ScanInterval.Duration = secondsDuration(*update.UsageScanSeconds)
	}
	if update.LookbackSeconds != nil {
		cfg.Usage.Lookback.Duration = secondsDuration(*update.LookbackSeconds)
	}
	if update.ProcessWarnSeconds != nil {
		cfg.Defaults.WarnAfter.Duration = secondsDuration(*update.ProcessWarnSeconds)
	}
	if update.ProcessKillSeconds != nil {
		cfg.Defaults.KillAfter.Duration = secondsDuration(*update.ProcessKillSeconds)
	}
	if update.LocalNotifications != nil {
		cfg.Alerts.LocalNotifications = *update.LocalNotifications
	}
	if err := rejectNonPositiveDurations(update); err != nil {
		return err
	}
	return cfg.Validate()
}

func rejectNonPositiveDurations(update ConfigUpdate) error {
	fields := []struct {
		name  string
		value *int64
	}{
		{"usage_window_seconds", update.UsageWindowSeconds},
		{"usage_scan_seconds", update.UsageScanSeconds},
		{"lookback_seconds", update.LookbackSeconds},
		{"process_warn_seconds", update.ProcessWarnSeconds},
		{"process_kill_seconds", update.ProcessKillSeconds},
	}
	for _, field := range fields {
		if field.value != nil && *field.value <= 0 {
			return errors.New(field.name + " must be positive")
		}
	}
	return nil
}

func agentConfigDescription(agent config.Agent) string {
	if agent.TerminationAllowed() {
		return "worker process; eligible for enforcement"
	}
	return "app or shell process; visibility only"
}

func secondsDuration(seconds int64) time.Duration {
	return time.Duration(seconds) * time.Second
}

func boolPtr(value bool) *bool {
	return &value
}
