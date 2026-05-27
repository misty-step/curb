package main

import (
	"bufio"
	"flag"
	"fmt"
	"os"
	"path/filepath"
	"strings"
	"time"

	"github.com/phaedrus/curb/internal/config"
)

func cmdInstall(args []string) error {
	fs := flag.NewFlagSet("install", flag.ExitOnError)
	prefix := fs.String("prefix", filepath.Join(os.Getenv("HOME"), ".local"), "install prefix")
	if err := fs.Parse(args); err != nil {
		return err
	}
	exe, err := os.Executable()
	if err != nil {
		return err
	}
	destDir := filepath.Join(*prefix, "bin")
	if err := os.MkdirAll(destDir, 0o755); err != nil {
		return err
	}
	dest := filepath.Join(destDir, "curb")
	content, err := os.ReadFile(exe)
	if err != nil {
		return err
	}
	if err := os.WriteFile(dest, content, 0o755); err != nil {
		return err
	}
	fmt.Printf("installed: %s\n", dest)
	fmt.Printf("next: add %s to PATH if needed\n", destDir)
	return nil
}

func cmdInit(args []string) error {
	fs := flag.NewFlagSet("init", flag.ExitOnError)
	configPath := fs.String("config", defaultConfigPath(), "config file to create")
	force := fs.Bool("force", false, "overwrite an existing config")
	mode := fs.String("mode", "visibility", "initial mode: visibility, alert, or enforcement")
	if err := fs.Parse(args); err != nil {
		return err
	}
	path, created, err := writeDefaultConfig(*configPath, *mode, *force)
	if err != nil {
		return err
	}
	if created {
		fmt.Printf("created config: %s\n", path)
	} else {
		fmt.Printf("config already exists: %s\n", path)
	}
	fmt.Println("next: curb")
	return nil
}

func cmdConfig(args []string) error {
	path := defaultConfigPath()
	if len(args) == 0 || args[0] == "show" {
		cfg, err := config.Load(path)
		if err != nil {
			return err
		}
		if len(args) == 0 && stdinIsTerminal() {
			return promptConfig(path, cfg)
		}
		printConfigSummary(path, cfg)
		return nil
	}
	if args[0] == "path" {
		fmt.Println(defaultConfigPath())
		return nil
	}
	if args[0] == "aggressive" || args[0] == "reasonable" || args[0] == "observe" {
		return applyPreset(defaultConfigPath(), args[0])
	}
	if args[0] == "set" {
		return cmdConfigSet(args[1:])
	}
	usageConfig()
	return fmt.Errorf("unknown config command %q", args[0])
}

func cmdConfigSet(args []string) error {
	fs := flag.NewFlagSet("config set", flag.ExitOnError)
	configPath := fs.String("config", defaultConfigPath(), "config file")
	mode := fs.String("mode", "", "visibility, alert, or enforcement")
	warnAfter := fs.String("warn-after", "", "warning threshold")
	killAfter := fs.String("kill-after", "", "kill threshold")
	grace := fs.String("grace", "", "kill grace period")
	scan := fs.String("scan", "", "scan interval")
	usageEnabled := fs.String("usage", "", "enable usage guard: true or false")
	warnTurnTokens := fs.Int64("warn-turn-tokens", 0, "warn when a session reaches this many tokens in a turn")
	killTurnTokens := fs.Int64("kill-turn-tokens", 0, "kill when a session reaches this many tokens in a turn")
	usageWindow := fs.String("usage-window", "", "usage rolling window")
	usageScan := fs.String("usage-scan", "", "usage scan interval")
	ledgerForwardURL := fs.String("ledger-forward-url", "", "forward ledger events to this HTTP(S) endpoint")
	if err := fs.Parse(args); err != nil {
		return err
	}
	cfg, err := config.Load(*configPath)
	if err != nil {
		return err
	}
	if *mode != "" {
		cfg.Mode = config.Mode(*mode)
	}
	if *warnAfter != "" {
		d, err := time.ParseDuration(*warnAfter)
		if err != nil {
			return err
		}
		cfg.Defaults.WarnAfter.Duration = d
	}
	if *killAfter != "" {
		d, err := time.ParseDuration(*killAfter)
		if err != nil {
			return err
		}
		cfg.Defaults.KillAfter.Duration = d
	}
	if *grace != "" {
		d, err := time.ParseDuration(*grace)
		if err != nil {
			return err
		}
		cfg.Defaults.KillGracePeriod.Duration = d
	}
	if *scan != "" {
		d, err := time.ParseDuration(*scan)
		if err != nil {
			return err
		}
		cfg.Service.ScanInterval.Duration = d
	}
	if *usageEnabled != "" {
		switch strings.ToLower(*usageEnabled) {
		case "true", "yes", "on", "1":
			cfg.Usage.Enabled = boolPtr(true)
		case "false", "no", "off", "0":
			cfg.Usage.Enabled = boolPtr(false)
		default:
			return fmt.Errorf("--usage must be true or false")
		}
	}
	if *warnTurnTokens > 0 {
		cfg.Usage.WarnTurnTokens = *warnTurnTokens
	}
	if *killTurnTokens > 0 {
		cfg.Usage.KillTurnTokens = *killTurnTokens
	}
	if *usageWindow != "" {
		d, err := time.ParseDuration(*usageWindow)
		if err != nil {
			return err
		}
		cfg.Usage.Window.Duration = d
	}
	if *usageScan != "" {
		d, err := time.ParseDuration(*usageScan)
		if err != nil {
			return err
		}
		cfg.Usage.ScanInterval.Duration = d
	}
	if *ledgerForwardURL != "" {
		if *ledgerForwardURL == "off" || *ledgerForwardURL == "none" {
			cfg.Ledger.ForwardURL = ""
		} else {
			cfg.Ledger.ForwardURL = *ledgerForwardURL
		}
	}
	applyPolicyToAgents(cfg)
	if err := saveConfig(*configPath, cfg); err != nil {
		return err
	}
	printConfigSummary(*configPath, cfg)
	return nil
}

func printConfigSummary(path string, cfg *config.Config) {
	fmt.Println("curb config")
	fmt.Printf("  path: %s\n", compactHome(path))
	fmt.Printf("  mode: %s\n", cfg.Mode)
	fmt.Printf("  action: %s\n", actionLabel(cfg.Mode))
	if cfg.Mode != config.ModeEnforcement {
		fmt.Println("  safety: Curb will warn only; it will not stop processes in this mode.")
	} else {
		fmt.Println("  safety: enforcement is enabled; Curb can stop correlated agent workers after grace.")
	}
	fmt.Println()
	fmt.Println("runtime fallback")
	fmt.Printf("  warn after %s; stop after %s; grace %s; extend %s x%d\n",
		shortDuration(cfg.Defaults.WarnAfter.Duration),
		shortDuration(cfg.Defaults.KillAfter.Duration),
		shortDuration(cfg.Defaults.KillGracePeriod.Duration),
		shortDuration(cfg.Defaults.AckExtension.Duration),
		cfg.Defaults.MaxExtensions,
	)
	fmt.Println("  note: runtime limits are legacy metadata; the product watcher enforces usage policy.")
	fmt.Println()
	fmt.Println("usage policy")
	if cfg.Usage.IsEnabled() {
		fmt.Printf("  warn: %s per turn\n", tokenCount(cfg.Usage.WarnTurnTokens))
		fmt.Printf("  stop: %s per turn\n", tokenCount(cfg.Usage.KillTurnTokens))
		fmt.Printf("  scan: every %s; grace %s\n", shortDuration(cfg.Usage.ScanInterval.Duration), shortDuration(cfg.Usage.GracePeriod.Duration))
	} else {
		fmt.Println("  disabled")
	}
	if cfg.Ledger.ForwardURL != "" {
		fmt.Printf("  export: forwarding ledger events to %s\n", cfg.Ledger.ForwardURL)
	} else {
		fmt.Println("  export: local ledger only")
	}
	fmt.Println()
	fmt.Println("watched agents")
	fmt.Printf("  %s\n", agentLabels(cfg.Agents))
	fmt.Println()
	fmt.Println("next")
	fmt.Println("  curb dashboard                 see live workers, current usage, and risk")
	fmt.Println("  curb watch                     run automatic warning/enforcement loop")
	fmt.Println("  curb config reasonable         return to notify-only defaults")
	fmt.Println("  curb config aggressive         local enforcement test thresholds")
	fmt.Println()
	fmt.Println("presets:")
	fmt.Println("  curb config aggressive   test agent kills quickly: warn 30s, kill 60s")
	fmt.Println("  curb config reasonable   warn only: warn 90m, kill 120m")
	fmt.Println("  curb config observe      record only: no warnings or kills")
	fmt.Println("  curb config set ...      custom thresholds")
}

func applyPreset(path, preset string) error {
	cfg, err := config.Load(path)
	if err != nil {
		return err
	}
	keepProcessAgents(cfg)
	cfg.Service.MinConfidence = 50
	switch preset {
	case "aggressive":
		cfg.Mode = config.ModeEnforcement
		cfg.Service.ScanInterval.Duration = time.Second
		cfg.Service.HeartbeatInterval.Duration = 5 * time.Second
		cfg.Usage.Enabled = boolPtr(true)
		cfg.Usage.ScanInterval.Duration = time.Second
		cfg.Usage.Window.Duration = time.Minute
		cfg.Usage.Window.Duration = time.Minute
		cfg.Usage.WarnTurnTokens = 250_000
		cfg.Usage.KillTurnTokens = 750_000
		cfg.Usage.GracePeriod.Duration = 10 * time.Second
		cfg.Defaults.WarnAfter.Duration = 30 * time.Second
		cfg.Defaults.KillAfter.Duration = 60 * time.Second
		cfg.Defaults.KillGracePeriod.Duration = 10 * time.Second
		cfg.Defaults.AckExtension.Duration = 30 * time.Second
		cfg.Defaults.MaxExtensions = 1
		cfg.Defaults.MinLifetime.Duration = time.Second
		cfg.Defaults.MaxRunGap.Duration = 2 * time.Second
		applyPolicyToAgents(cfg)
	case "reasonable":
		cfg.Mode = config.ModeAlert
		cfg.Service.ScanInterval.Duration = 15 * time.Second
		cfg.Service.HeartbeatInterval.Duration = time.Minute
		cfg.Usage.Enabled = boolPtr(true)
		cfg.Usage.ScanInterval.Duration = 5 * time.Second
		cfg.Usage.Window.Duration = 15 * time.Minute
		cfg.Usage.WarnTurnTokens = 1_000_000
		cfg.Usage.KillTurnTokens = 3_000_000
		cfg.Usage.GracePeriod.Duration = time.Minute
		cfg.Defaults.WarnAfter.Duration = 90 * time.Minute
		cfg.Defaults.KillAfter.Duration = 2 * time.Hour
		cfg.Defaults.KillGracePeriod.Duration = time.Minute
		cfg.Defaults.AckExtension.Duration = 30 * time.Minute
		cfg.Defaults.MaxExtensions = 2
		applyPolicyToAgents(cfg)
	case "observe":
		cfg.Mode = config.ModeVisibility
		cfg.Service.ScanInterval.Duration = 15 * time.Second
		cfg.Usage.Enabled = boolPtr(true)
		cfg.Usage.ScanInterval.Duration = 10 * time.Second
		cfg.Usage.Window.Duration = 15 * time.Minute
		cfg.Usage.WarnTurnTokens = 5_000_000
		cfg.Usage.KillTurnTokens = 10_000_000
		cfg.Usage.GracePeriod.Duration = time.Minute
		cfg.Defaults.WarnAfter.Duration = 24 * time.Hour
		cfg.Defaults.KillAfter.Duration = 48 * time.Hour
		cfg.Defaults.KillGracePeriod.Duration = time.Minute
		cfg.Defaults.AckExtension.Duration = 30 * time.Minute
		cfg.Defaults.MaxExtensions = 2
		applyPolicyToAgents(cfg)
	default:
		return fmt.Errorf("unknown preset %q", preset)
	}
	if err := saveConfig(path, cfg); err != nil {
		return err
	}
	printConfigSummary(path, cfg)
	return nil
}

func applyPolicyToAgents(cfg *config.Config) {
	for i := range cfg.Agents {
		policy := cfg.Defaults
		policy.AllowAppRootKill = false
		cfg.Agents[i].Policy = &policy
	}
}

func keepProcessAgents(cfg *config.Config) {
	agents := defaultProcessAgents()
	seen := map[string]bool{}
	for _, agent := range agents {
		seen[agent.ID] = true
	}
	for _, agent := range cfg.Agents {
		if seen[agent.ID] {
			continue
		}
		if agent.TerminationAllowed() {
			if agent.Kind == "" {
				agent.Kind = config.AgentKindProcess
			}
			agents = append(agents, agent)
			seen[agent.ID] = true
		}
	}
	cfg.Agents = agents
}

func promptConfig(path string, cfg *config.Config) error {
	printConfigSummary(path, cfg)
	fmt.Println()
	fmt.Println("Choose a setup:")
	fmt.Println("  1  Observe: record only")
	fmt.Println("  2  Reasonable: warn only after 90m")
	fmt.Println("  3  Aggressive test: kill agent processes after 60s")
	fmt.Println("  4  Custom")
	fmt.Print("Selection [2]: ")

	reader := bufio.NewReader(os.Stdin)
	choice, _ := reader.ReadString('\n')
	switch strings.TrimSpace(choice) {
	case "", "2", "reasonable":
		return applyPreset(path, "reasonable")
	case "1", "observe":
		return applyPreset(path, "observe")
	case "3", "aggressive":
		return applyPreset(path, "aggressive")
	case "4", "custom":
		return promptCustomConfig(path, cfg, reader)
	default:
		return fmt.Errorf("unknown selection %q", strings.TrimSpace(choice))
	}
}

func promptCustomConfig(path string, cfg *config.Config, reader *bufio.Reader) error {
	keepProcessAgents(cfg)
	fmt.Print("Warn after [90m]: ")
	warnAfter, _ := reader.ReadString('\n')
	fmt.Print("Kill after [120m]: ")
	killAfter, _ := reader.ReadString('\n')
	fmt.Print("Actually kill agent processes? [y/N]: ")
	kill, _ := reader.ReadString('\n')

	if strings.TrimSpace(warnAfter) == "" {
		warnAfter = "90m"
	}
	if strings.TrimSpace(killAfter) == "" {
		killAfter = "120m"
	}
	warn, err := time.ParseDuration(strings.TrimSpace(warnAfter))
	if err != nil {
		return err
	}
	limit, err := time.ParseDuration(strings.TrimSpace(killAfter))
	if err != nil {
		return err
	}
	cfg.Mode = config.ModeAlert
	if strings.EqualFold(strings.TrimSpace(kill), "y") || strings.EqualFold(strings.TrimSpace(kill), "yes") {
		cfg.Mode = config.ModeEnforcement
	}
	cfg.Defaults.WarnAfter.Duration = warn
	cfg.Defaults.KillAfter.Duration = limit
	applyPolicyToAgents(cfg)
	if err := saveConfig(path, cfg); err != nil {
		return err
	}
	printConfigSummary(path, cfg)
	return nil
}

func stdinIsTerminal() bool {
	stat, err := os.Stdin.Stat()
	return err == nil && (stat.Mode()&os.ModeCharDevice) != 0
}

func saveConfig(path string, cfg *config.Config) error {
	return config.Save(path, cfg)
}

func boolPtr(value bool) *bool {
	return &value
}

func defaultConfigPath() string {
	if path := os.Getenv("CURB_CONFIG"); path != "" {
		return path
	}
	if _, err := os.Stat("curb.yaml"); err == nil {
		return "curb.yaml"
	}
	if path := userConfigPath(); path != "" {
		if _, err := os.Stat(path); err == nil {
			return path
		}
		return path
	}
	return filepath.Join("configs", "curb.example.yaml")
}

func userConfigPath() string {
	base, err := os.UserConfigDir()
	if err != nil || base == "" {
		if home, homeErr := os.UserHomeDir(); homeErr == nil {
			base = filepath.Join(home, ".config")
		} else {
			return ""
		}
	}
	return filepath.Join(base, "curb", "config.yaml")
}

func ensureDefaultConfig(force bool) (string, error) {
	path := defaultConfigPath()
	if path == "" {
		return "", fmt.Errorf("could not determine default config path")
	}
	if path == "curb.yaml" {
		return path, nil
	}
	if _, err := os.Stat(path); err == nil && !force {
		return path, nil
	}
	createdPath, _, err := writeDefaultConfig(path, "visibility", force)
	return createdPath, err
}

func writeDefaultConfig(path, mode string, force bool) (string, bool, error) {
	if mode != "visibility" && mode != "alert" && mode != "enforcement" {
		return "", false, fmt.Errorf("invalid mode %q", mode)
	}
	if path == "" {
		return "", false, fmt.Errorf("config path is required")
	}
	if _, err := os.Stat(path); err == nil && !force {
		return path, false, nil
	}
	if err := os.MkdirAll(filepath.Dir(path), 0o700); err != nil {
		return "", false, err
	}
	stateDir := filepath.Dir(path)
	content := defaultConfig(mode, stateDir)
	if err := os.WriteFile(path, []byte(content), 0o600); err != nil {
		return "", false, err
	}
	return path, true, nil
}

func defaultConfig(mode, stateDir string) string {
	ledgerPath := filepath.Join(stateDir, "runs.ndjson")
	return fmt.Sprintf(`version: 1
profile: local-default
mode: %s

service:
  scan_interval: 15s
  policy_interval: 5s
  heartbeat_interval: 60s
  min_confidence: 50
  state_dir: %s

usage:
  enabled: true
  scan_interval: 5s
  lookback: 24h
  window: 15m
  warn_turn_tokens: 1000000
  kill_turn_tokens: 3000000
  grace_period: 60s

defaults:
  warn_after: 90m
  kill_after: 120m
  ack_extension: 30m
  max_extensions: 2
  kill_grace_period: 60s
  cooldown_after_kill: 15m
  min_lifetime: 10s
  max_run_gap: 20s
  allow_app_root_kill: false

agents:
  - id: codex-desktop-worker
    label: Codex Desktop Worker
    family: codex
    kind: process
    match:
      process_names:
        - codex
      require_command_regex:
        - "\\bapp-server\\b"
        - "--listen\\s+stdio://"
      command_regex:
        - "\\bapp-server\\b"
        - "--listen\\s+stdio://"

  - id: codex-cli
    label: Codex CLI
    family: codex
    kind: process
    match:
      process_names:
        - codex
      command_regex:
        - "(^|/|\\\\)codex(\\.js|\\.cmd|\\.exe)?(\\s|$)"
      exclude_command_regex:
        - "/Applications/Codex.app"

  - id: claude-code
    label: Claude Code
    family: claude
    kind: process
    match:
      process_names:
        - claude
        - claude-code
      command_regex:
        - "(^|/|\\\\)claude(-code)?(\\.cmd|\\.exe)?(\\s|$)"
      exclude_command_regex:
        - "/Applications/Claude.app"
      exclude_parent_command_regex:
        - "/Applications/Codex\\.app/.+\\bapp-server\\b"

  - id: antigravity-cli
    label: Anti-Gravity CLI
    family: antigravity
    kind: process
    match:
      process_names:
        - agy
      command_regex:
        - "(^|/|\\\\)agy(\\.cmd|\\.exe)?(\\s|$)"

alerts:
  local_notifications: true
  webhook_url: ""
  slack_webhook_url: ""

ledger:
  path: %s
  include_prompt_content: false
  forward_url: ""
`, mode, stateDir, ledgerPath)
}

func defaultProcessAgents() []config.Agent {
	return []config.Agent{
		{
			ID:     "codex-desktop-worker",
			Label:  "Codex Desktop Worker",
			Family: "codex",
			Kind:   config.AgentKindProcess,
			Match: config.Match{
				ProcessNames:        []string{"codex"},
				RequireCommandRegex: []string{"\\bapp-server\\b", "--listen\\s+stdio://"},
				CommandRegex:        []string{"\\bapp-server\\b", "--listen\\s+stdio://"},
			},
		},
		{
			ID:     "codex-cli",
			Label:  "Codex CLI",
			Family: "codex",
			Kind:   config.AgentKindProcess,
			Match: config.Match{
				ProcessNames:        []string{"codex"},
				CommandRegex:        []string{"(^|/|\\\\)codex(\\.js|\\.cmd|\\.exe)?(\\s|$)"},
				ExcludeCommandRegex: []string{"/Applications/Codex.app"},
			},
		},
		{
			ID:     "claude-code",
			Label:  "Claude Code",
			Family: "claude",
			Kind:   config.AgentKindProcess,
			Match: config.Match{
				ProcessNames:        []string{"claude", "claude-code"},
				CommandRegex:        []string{"(^|/|\\\\)claude(-code)?(\\.cmd|\\.exe)?(\\s|$)"},
				ExcludeCommandRegex: []string{"/Applications/Claude.app"},
				ExcludeParentRegex:  []string{"/Applications/Codex\\.app/.+\\bapp-server\\b"},
			},
		},
		{
			ID:     "antigravity-cli",
			Label:  "Anti-Gravity CLI",
			Family: "antigravity",
			Kind:   config.AgentKindProcess,
			Match: config.Match{
				ProcessNames: []string{"agy"},
				CommandRegex: []string{"(^|/|\\\\)agy(\\.cmd|\\.exe)?(\\s|$)"},
			},
		},
	}
}
