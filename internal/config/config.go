package config

import (
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"regexp"
	"strings"
	"time"

	"gopkg.in/yaml.v3"
)

type Duration struct {
	time.Duration
}

func (d *Duration) UnmarshalYAML(value *yaml.Node) error {
	var raw string
	if err := value.Decode(&raw); err != nil {
		return err
	}
	parsed, err := time.ParseDuration(raw)
	if err != nil {
		return fmt.Errorf("invalid duration %q: %w", raw, err)
	}
	d.Duration = parsed
	return nil
}

func (d Duration) MarshalYAML() (any, error) {
	return d.String(), nil
}

type Config struct {
	Version  int           `yaml:"version"`
	Profile  string        `yaml:"profile"`
	Mode     Mode          `yaml:"mode"`
	Service  ServiceConfig `yaml:"service"`
	Usage    UsageConfig   `yaml:"usage"`
	Defaults Policy        `yaml:"defaults"`
	Agents   []Agent       `yaml:"agents"`
	Alerts   AlertConfig   `yaml:"alerts"`
	Ledger   LedgerConfig  `yaml:"ledger"`
}

type Mode string

const (
	ModeVisibility  Mode = "visibility"
	ModeAlert       Mode = "alert"
	ModeEnforcement Mode = "enforcement"
)

type ServiceConfig struct {
	ScanInterval      Duration `yaml:"scan_interval"`
	PolicyInterval    Duration `yaml:"policy_interval"`
	StateDir          string   `yaml:"state_dir"`
	MinConfidence     int      `yaml:"min_confidence"`
	HeartbeatInterval Duration `yaml:"heartbeat_interval"`
}

type UsageConfig struct {
	Enabled        *bool    `yaml:"enabled"`
	ScanInterval   Duration `yaml:"scan_interval"`
	Lookback       Duration `yaml:"lookback"`
	Window         Duration `yaml:"window"`
	WarnTurnTokens int64    `yaml:"warn_turn_tokens"`
	KillTurnTokens int64    `yaml:"kill_turn_tokens"`
	GracePeriod    Duration `yaml:"grace_period"`
}

func (u UsageConfig) IsEnabled() bool {
	return u.Enabled == nil || *u.Enabled
}

type Agent struct {
	ID     string  `yaml:"id"`
	Label  string  `yaml:"label"`
	Family string  `yaml:"family"`
	Kind   string  `yaml:"kind,omitempty"`
	Match  Match   `yaml:"match"`
	Policy *Policy `yaml:"policy"`
}

type Match struct {
	BundleIDs           []string        `yaml:"bundle_ids"`
	CodeSignatures      []CodeSignature `yaml:"code_signatures"`
	AppPaths            []string        `yaml:"app_paths"`
	WindowsPaths        []string        `yaml:"windows_paths"`
	LinuxPaths          []string        `yaml:"linux_paths"`
	ExecutablePaths     []string        `yaml:"executable_paths"`
	ProcessNames        []string        `yaml:"process_names"`
	ParentProcessNames  []string        `yaml:"parent_process_names"`
	CommandRegex        []string        `yaml:"command_regex"`
	RequireCommandRegex []string        `yaml:"require_command_regex"`
	ExcludeNames        []string        `yaml:"exclude_process_names"`
	ExcludeCommandRegex []string        `yaml:"exclude_command_regex"`
	ExcludeParentRegex  []string        `yaml:"exclude_parent_command_regex"`
}

type CodeSignature struct {
	Identifier string `yaml:"identifier"`
	TeamID     string `yaml:"team_id"`
}

type Policy struct {
	WarnAfter         Duration `yaml:"warn_after"`
	KillAfter         Duration `yaml:"kill_after"`
	AckExtension      Duration `yaml:"ack_extension"`
	MaxExtensions     int      `yaml:"max_extensions"`
	KillGracePeriod   Duration `yaml:"kill_grace_period"`
	CooldownAfterKill Duration `yaml:"cooldown_after_kill"`
	MinLifetime       Duration `yaml:"min_lifetime"`
	MaxRunGap         Duration `yaml:"max_run_gap"`
	AllowAppRootKill  bool     `yaml:"allow_app_root_kill"`
}

const (
	AgentKindProcess = "process"
	AgentKindApp     = "app"
)

type AlertConfig struct {
	LocalNotifications bool   `yaml:"local_notifications"`
	WebhookURL         string `yaml:"webhook_url"`
	SlackWebhookURL    string `yaml:"slack_webhook_url"`
}

type LedgerConfig struct {
	Path                 string `yaml:"path"`
	IncludePromptContent bool   `yaml:"include_prompt_content"`
}

func Load(path string) (*Config, error) {
	content, err := os.ReadFile(path)
	if err != nil {
		return nil, err
	}

	var cfg Config
	decoder := yaml.NewDecoder(strings.NewReader(string(content)))
	decoder.KnownFields(true)
	if err := decoder.Decode(&cfg); err != nil {
		return nil, err
	}
	if err := cfg.SetDefaults(); err != nil {
		return nil, err
	}
	return &cfg, cfg.Validate()
}

func Save(path string, cfg *Config) error {
	if err := cfg.Validate(); err != nil {
		return err
	}
	out, err := yaml.Marshal(cfg)
	if err != nil {
		return err
	}
	dir := filepath.Dir(path)
	if err := os.MkdirAll(dir, 0o700); err != nil {
		return err
	}
	tmp, err := os.CreateTemp(dir, ".curb-config-*")
	if err != nil {
		return err
	}
	tmpPath := tmp.Name()
	defer func() {
		_ = os.Remove(tmpPath)
	}()
	if _, err := tmp.Write(out); err != nil {
		_ = tmp.Close()
		return err
	}
	if err := tmp.Chmod(0o600); err != nil {
		_ = tmp.Close()
		return err
	}
	if err := tmp.Sync(); err != nil {
		_ = tmp.Close()
		return err
	}
	if err := tmp.Close(); err != nil {
		return err
	}
	if err := replaceFile(tmpPath, path); err != nil {
		return err
	}
	return syncDir(dir)
}

func Clone(cfg *Config) *Config {
	if cfg == nil {
		return nil
	}
	copy := *cfg
	copy.Agents = make([]Agent, len(cfg.Agents))
	for i, agent := range cfg.Agents {
		copy.Agents[i] = agent
		copy.Agents[i].Match.BundleIDs = append([]string(nil), agent.Match.BundleIDs...)
		copy.Agents[i].Match.CodeSignatures = append([]CodeSignature(nil), agent.Match.CodeSignatures...)
		copy.Agents[i].Match.AppPaths = append([]string(nil), agent.Match.AppPaths...)
		copy.Agents[i].Match.WindowsPaths = append([]string(nil), agent.Match.WindowsPaths...)
		copy.Agents[i].Match.LinuxPaths = append([]string(nil), agent.Match.LinuxPaths...)
		copy.Agents[i].Match.ExecutablePaths = append([]string(nil), agent.Match.ExecutablePaths...)
		copy.Agents[i].Match.ProcessNames = append([]string(nil), agent.Match.ProcessNames...)
		copy.Agents[i].Match.ParentProcessNames = append([]string(nil), agent.Match.ParentProcessNames...)
		copy.Agents[i].Match.CommandRegex = append([]string(nil), agent.Match.CommandRegex...)
		copy.Agents[i].Match.RequireCommandRegex = append([]string(nil), agent.Match.RequireCommandRegex...)
		copy.Agents[i].Match.ExcludeNames = append([]string(nil), agent.Match.ExcludeNames...)
		copy.Agents[i].Match.ExcludeCommandRegex = append([]string(nil), agent.Match.ExcludeCommandRegex...)
		copy.Agents[i].Match.ExcludeParentRegex = append([]string(nil), agent.Match.ExcludeParentRegex...)
		if agent.Policy != nil {
			policy := *agent.Policy
			copy.Agents[i].Policy = &policy
		}
	}
	return &copy
}

func (c *Config) SetDefaults() error {
	if c.Mode == "" {
		c.Mode = ModeVisibility
	}
	if c.Service.ScanInterval.Duration == 0 {
		c.Service.ScanInterval.Duration = 15 * time.Second
	}
	if c.Service.PolicyInterval.Duration == 0 {
		c.Service.PolicyInterval.Duration = 5 * time.Second
	}
	if c.Service.HeartbeatInterval.Duration == 0 {
		c.Service.HeartbeatInterval.Duration = time.Minute
	}
	if c.Service.MinConfidence == 0 {
		c.Service.MinConfidence = 50
	}
	if c.Service.StateDir == "" {
		c.Service.StateDir = defaultStateDir()
	}
	if c.Usage.ScanInterval.Duration == 0 {
		c.Usage.ScanInterval.Duration = 5 * time.Second
	}
	if c.Usage.Lookback.Duration == 0 {
		c.Usage.Lookback.Duration = 24 * time.Hour
	}
	if c.Usage.Window.Duration == 0 {
		c.Usage.Window.Duration = 15 * time.Minute
	}
	if c.Usage.WarnTurnTokens == 0 {
		c.Usage.WarnTurnTokens = 1_000_000
	}
	if c.Usage.KillTurnTokens == 0 {
		c.Usage.KillTurnTokens = 3_000_000
	}
	if c.Usage.GracePeriod.Duration == 0 {
		c.Usage.GracePeriod.Duration = time.Minute
	}
	if c.Ledger.Path == "" {
		c.Ledger.Path = c.Service.StateDir + string(os.PathSeparator) + "runs.ndjson"
	}
	if c.Defaults.WarnAfter.Duration == 0 {
		c.Defaults.WarnAfter.Duration = 90 * time.Minute
	}
	if c.Defaults.KillAfter.Duration == 0 {
		c.Defaults.KillAfter.Duration = 2 * time.Hour
	}
	if c.Defaults.AckExtension.Duration == 0 {
		c.Defaults.AckExtension.Duration = 30 * time.Minute
	}
	if c.Defaults.KillGracePeriod.Duration == 0 {
		c.Defaults.KillGracePeriod.Duration = time.Minute
	}
	if c.Defaults.MinLifetime.Duration == 0 {
		c.Defaults.MinLifetime.Duration = 10 * time.Second
	}
	if c.Defaults.MaxRunGap.Duration == 0 {
		c.Defaults.MaxRunGap.Duration = 20 * time.Second
	}
	return nil
}

func (c Config) Validate() error {
	if c.Version != 1 {
		return fmt.Errorf("version must be 1, got %d", c.Version)
	}
	switch c.Mode {
	case ModeVisibility, ModeAlert, ModeEnforcement:
	default:
		return fmt.Errorf("invalid mode %q", c.Mode)
	}
	if c.Defaults.WarnAfter.Duration >= c.Defaults.KillAfter.Duration {
		return errors.New("defaults.warn_after must be less than defaults.kill_after")
	}
	if c.Ledger.IncludePromptContent {
		return errors.New("ledger.include_prompt_content is not supported by launch implementation")
	}
	if c.Usage.IsEnabled() {
		if c.Usage.WarnTurnTokens >= c.Usage.KillTurnTokens {
			return errors.New("usage.warn_turn_tokens must be less than usage.kill_turn_tokens")
		}
		if c.Usage.ScanInterval.Duration <= 0 || c.Usage.Lookback.Duration <= 0 || c.Usage.Window.Duration <= 0 {
			return errors.New("usage intervals must be positive")
		}
	}

	seen := map[string]bool{}
	for _, agent := range c.Agents {
		if agent.ID == "" {
			return errors.New("agent id is required")
		}
		if seen[agent.ID] {
			return fmt.Errorf("duplicate agent id %q", agent.ID)
		}
		seen[agent.ID] = true
		if agent.Label == "" {
			return fmt.Errorf("agent %q label is required", agent.ID)
		}
		switch agent.Kind {
		case "", AgentKindProcess, AgentKindApp:
		default:
			return fmt.Errorf("agent %q kind must be process or app", agent.ID)
		}
		if agent.Match.Empty() {
			return fmt.Errorf("agent %q must define at least one matcher", agent.ID)
		}
		for _, raw := range agent.Match.CommandRegex {
			if _, err := regexp.Compile(raw); err != nil {
				return fmt.Errorf("agent %q command_regex %q: %w", agent.ID, raw, err)
			}
		}
		for _, raw := range agent.Match.RequireCommandRegex {
			if _, err := regexp.Compile(raw); err != nil {
				return fmt.Errorf("agent %q require_command_regex %q: %w", agent.ID, raw, err)
			}
		}
		for _, raw := range agent.Match.ExcludeCommandRegex {
			if _, err := regexp.Compile(raw); err != nil {
				return fmt.Errorf("agent %q exclude_command_regex %q: %w", agent.ID, raw, err)
			}
		}
		for _, raw := range agent.Match.ExcludeParentRegex {
			if _, err := regexp.Compile(raw); err != nil {
				return fmt.Errorf("agent %q exclude_parent_command_regex %q: %w", agent.ID, raw, err)
			}
		}
		policy := c.PolicyFor(agent)
		if policy.WarnAfter.Duration >= policy.KillAfter.Duration {
			return fmt.Errorf("agent %q warn_after must be less than kill_after", agent.ID)
		}
	}
	return nil
}

func (a Agent) TerminationAllowed() bool {
	if a.Kind == AgentKindProcess {
		return true
	}
	if a.Kind == AgentKindApp {
		return false
	}
	if strings.Contains(strings.ToLower(a.ID), "desktop") {
		return false
	}
	if len(a.Match.BundleIDs) > 0 || len(a.Match.AppPaths) > 0 {
		return false
	}
	return true
}

func (m Match) Empty() bool {
	return len(m.BundleIDs) == 0 &&
		len(m.CodeSignatures) == 0 &&
		len(m.AppPaths) == 0 &&
		len(m.WindowsPaths) == 0 &&
		len(m.LinuxPaths) == 0 &&
		len(m.ExecutablePaths) == 0 &&
		len(m.ProcessNames) == 0 &&
		len(m.ParentProcessNames) == 0 &&
		len(m.CommandRegex) == 0 &&
		len(m.RequireCommandRegex) == 0
}

func (c Config) PolicyFor(agent Agent) Policy {
	policy := c.Defaults
	if agent.Policy == nil {
		return policy
	}
	override := *agent.Policy
	if override.WarnAfter.Duration != 0 {
		policy.WarnAfter = override.WarnAfter
	}
	if override.KillAfter.Duration != 0 {
		policy.KillAfter = override.KillAfter
	}
	if override.AckExtension.Duration != 0 {
		policy.AckExtension = override.AckExtension
	}
	if override.MaxExtensions != 0 {
		policy.MaxExtensions = override.MaxExtensions
	}
	if override.KillGracePeriod.Duration != 0 {
		policy.KillGracePeriod = override.KillGracePeriod
	}
	if override.CooldownAfterKill.Duration != 0 {
		policy.CooldownAfterKill = override.CooldownAfterKill
	}
	if override.MinLifetime.Duration != 0 {
		policy.MinLifetime = override.MinLifetime
	}
	if override.MaxRunGap.Duration != 0 {
		policy.MaxRunGap = override.MaxRunGap
	}
	if override.AllowAppRootKill {
		policy.AllowAppRootKill = true
	}
	return policy
}

func defaultStateDir() string {
	if xdg := os.Getenv("XDG_STATE_HOME"); xdg != "" {
		return xdg + string(os.PathSeparator) + "curb"
	}
	if local := os.Getenv("LOCALAPPDATA"); local != "" {
		return local + string(os.PathSeparator) + "Curb"
	}
	if home, err := os.UserHomeDir(); err == nil {
		return home + string(os.PathSeparator) + ".local" + string(os.PathSeparator) + "state" + string(os.PathSeparator) + "curb"
	}
	return ".curb"
}
