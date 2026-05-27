package config

import (
	"os"
	"path/filepath"
	"testing"
	"time"
)

func TestLoadExampleConfig(t *testing.T) {
	cfg, err := Load(filepath.Join("..", "..", "configs", "curb.example.yaml"))
	if err != nil {
		t.Fatal(err)
	}
	if cfg.Version != 1 {
		t.Fatalf("version = %d", cfg.Version)
	}
	if cfg.Mode != ModeVisibility {
		t.Fatalf("mode = %s", cfg.Mode)
	}
	if len(cfg.Agents) < 2 {
		t.Fatalf("expected default agents, got %d", len(cfg.Agents))
	}
}

func TestRejectsPromptContentCapture(t *testing.T) {
	path := filepath.Join(t.TempDir(), "curb.yaml")
	err := os.WriteFile(path, []byte(`
version: 1
mode: visibility
defaults:
  warn_after: 1m
  kill_after: 2m
agents:
  - id: test
    label: Test
    match:
      process_names: [sleep]
ledger:
  include_prompt_content: true
`), 0o600)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := Load(path); err == nil {
		t.Fatal("expected include_prompt_content rejection")
	}
}

func TestRejectsInvalidLedgerForwardURL(t *testing.T) {
	path := filepath.Join(t.TempDir(), "curb.yaml")
	err := os.WriteFile(path, []byte(`
version: 1
mode: visibility
defaults:
  warn_after: 1m
  kill_after: 2m
agents:
  - id: test
    label: Test
    match:
      process_names: [sleep]
ledger:
  forward_url: "file:///tmp/curb.ndjson"
`), 0o600)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := Load(path); err == nil {
		t.Fatal("expected forward_url rejection")
	}
}

func TestRejectsUnknownFields(t *testing.T) {
	path := filepath.Join(t.TempDir(), "curb.yaml")
	err := os.WriteFile(path, []byte(`
version: 1
mode: visibility
bogus: true
agents: []
`), 0o600)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := Load(path); err == nil {
		t.Fatal("expected unknown field error")
	}
}

func TestPolicyForMergesOverrides(t *testing.T) {
	cfg := Config{
		Defaults: Policy{
			WarnAfter:       Duration{Duration: minute(90)},
			KillAfter:       Duration{Duration: minute(120)},
			AckExtension:    Duration{Duration: minute(30)},
			MaxExtensions:   2,
			KillGracePeriod: Duration{Duration: minute(1)},
		},
	}
	agent := Agent{Policy: &Policy{
		WarnAfter:        Duration{Duration: minute(10)},
		MaxExtensions:    1,
		AllowAppRootKill: true,
	}}
	policy := cfg.PolicyFor(agent)
	if policy.WarnAfter.Duration != minute(10) {
		t.Fatalf("warn_after = %s", policy.WarnAfter)
	}
	if policy.KillAfter.Duration != minute(120) {
		t.Fatalf("kill_after = %s", policy.KillAfter)
	}
	if policy.MaxExtensions != 1 {
		t.Fatalf("max_extensions = %d", policy.MaxExtensions)
	}
	if !policy.AllowAppRootKill {
		t.Fatal("allow_app_root_kill not merged")
	}
}

func TestAgentTerminationAllowedDefaultsDesktopAppsToWatchOnly(t *testing.T) {
	app := Agent{
		ID:    "codex-desktop",
		Label: "Codex Desktop",
		Match: Match{BundleIDs: []string{"com.openai.codex"}},
	}
	if app.TerminationAllowed() {
		t.Fatal("desktop app should be watch-only by default")
	}
	agent := Agent{
		ID:    "codex-cli",
		Label: "Codex CLI",
		Match: Match{ProcessNames: []string{"codex"}},
	}
	if !agent.TerminationAllowed() {
		t.Fatal("cli agent should be terminable by default")
	}
}

func TestRejectsBadRegexAndBadMode(t *testing.T) {
	path := filepath.Join(t.TempDir(), "curb.yaml")
	err := os.WriteFile(path, []byte(`
version: 1
mode: chaos
defaults:
  warn_after: 1m
  kill_after: 2m
agents:
  - id: test
    label: Test
    match:
      command_regex: ["["]
`), 0o600)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := Load(path); err == nil {
		t.Fatal("expected invalid mode or regex")
	}
}

func minute(n int) time.Duration {
	return time.Duration(n) * time.Minute
}
