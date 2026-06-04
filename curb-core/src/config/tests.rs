use std::time::Duration as StdDuration;

use super::defaults::default_process_agents;
use super::duration::parse_duration;
use super::*;

#[test]
fn loads_example_config_with_defaults() {
    let cfg = Config::load(example_config_path()).unwrap();

    assert_eq!(cfg.version, 1);
    assert_eq!(cfg.mode, Mode::Visibility);
    assert_eq!(cfg.usage.warn_turn_tokens, 1_000_000);
    assert_eq!(cfg.usage.kill_turn_tokens, 3_000_000);
    assert_eq!(cfg.agents.len(), 6);
    assert!(!cfg.ledger.include_prompt_content);
}

#[test]
fn save_round_trips_yaml_with_lowercase_enums() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("curb.yaml");
    let mut cfg = Config::load(example_config_path()).unwrap();
    cfg.mode = Mode::Enforcement;
    cfg.usage.warn_turn_tokens = 2_000;
    cfg.usage.kill_turn_tokens = 4_000;

    cfg.save(&path).unwrap();
    let raw = std::fs::read_to_string(&path).unwrap();
    let reloaded = Config::load(&path).unwrap();

    assert!(raw.contains("mode: enforcement"));
    assert!(raw.contains("kind: process"));
    assert_eq!(reloaded.mode, Mode::Enforcement);
    assert_eq!(reloaded.usage.warn_turn_tokens, 2_000);
    assert_eq!(reloaded.usage.kill_turn_tokens, 4_000);
    assert_eq!(reloaded.agents.len(), cfg.agents.len());
    assert_eq!(reloaded.ledger.path, cfg.ledger.path);
}

#[test]
fn local_default_builds_private_process_agent_config() {
    let dir = tempfile::tempdir().unwrap();
    let cfg = Config::local_default(Mode::Alert, dir.path().join("state"));

    assert_eq!(cfg.version, 1);
    assert_eq!(cfg.profile, "local-default");
    assert_eq!(cfg.mode, Mode::Alert);
    assert_eq!(cfg.agents.len(), 4);
    assert!(cfg.agents.iter().all(Agent::termination_allowed));
    assert_eq!(
        cfg.ledger.path,
        dir.path().join("state").join("runs.ndjson")
    );
    assert!(cfg.validate().is_ok());
}

#[test]
fn presets_keep_custom_process_agents_and_drop_watch_only_apps() {
    let dir = tempfile::tempdir().unwrap();
    let mut cfg = Config::local_default(Mode::Visibility, dir.path().join("state"));
    cfg.agents.push(Agent {
        id: "custom-worker".to_string(),
        label: "Custom Worker".to_string(),
        kind: AgentKind::Process,
        matcher: Match {
            process_names: vec!["custom".to_string()],
            ..Match::default()
        },
        ..Agent::default()
    });
    cfg.agents.push(Agent {
        id: "custom-app".to_string(),
        label: "Custom App".to_string(),
        kind: AgentKind::App,
        matcher: Match {
            app_paths: vec!["/Applications/Custom.app".to_string()],
            ..Match::default()
        },
        ..Agent::default()
    });

    cfg.apply_preset(Preset::Aggressive);

    assert_eq!(cfg.mode, Mode::Enforcement);
    assert_eq!(cfg.usage.warn_turn_tokens, 250_000);
    assert!(cfg.agents.iter().any(|agent| agent.id == "custom-worker"));
    assert!(!cfg.agents.iter().any(|agent| agent.id == "custom-app"));
    assert!(cfg.agents.iter().all(|agent| {
        agent
            .policy
            .as_ref()
            .is_some_and(|policy| !policy.allow_app_root_kill)
    }));
}

#[test]
fn presets_select_expected_modes_and_usage_thresholds() {
    let dir = tempfile::tempdir().unwrap();
    let mut reasonable = Config::local_default(Mode::Visibility, dir.path().join("reasonable"));
    reasonable.apply_preset(Preset::Reasonable);

    assert_eq!(reasonable.mode, Mode::Alert);
    assert_eq!(reasonable.service.scan_interval, HumanDuration::seconds(15));
    assert_eq!(reasonable.usage.scan_interval, HumanDuration::seconds(5));
    assert_eq!(reasonable.usage.window, HumanDuration::minutes(15));
    assert_eq!(reasonable.usage.warn_turn_tokens, 1_000_000);
    assert_eq!(reasonable.usage.kill_turn_tokens, 3_000_000);
    assert!(reasonable.validate().is_ok());

    let mut observe = Config::local_default(Mode::Enforcement, dir.path().join("observe"));
    observe.apply_preset(Preset::Observe);

    assert_eq!(observe.mode, Mode::Visibility);
    assert_eq!(observe.service.scan_interval, HumanDuration::seconds(15));
    assert_eq!(observe.usage.scan_interval, HumanDuration::seconds(10));
    assert_eq!(observe.usage.window, HumanDuration::minutes(15));
    assert_eq!(observe.usage.warn_turn_tokens, 5_000_000);
    assert_eq!(observe.usage.kill_turn_tokens, 10_000_000);
    assert!(observe.validate().is_ok());
}

#[test]
fn save_validates_before_replacing_existing_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("curb.yaml");
    let mut cfg = Config::load(example_config_path()).unwrap();
    cfg.save(&path).unwrap();
    let original = std::fs::read(&path).unwrap();
    cfg.usage.warn_turn_tokens = cfg.usage.kill_turn_tokens + 1;

    let err = cfg.save(&path).unwrap_err();

    assert!(matches!(err, ConfigError::InvalidUsageThresholds));
    assert_eq!(std::fs::read(&path).unwrap(), original);
    assert_eq!(
        std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_name().to_string_lossy().contains(".tmp"))
            .count(),
        0
    );
}

#[test]
fn rejects_prompt_capture() {
    let err = load_from_str(
        r#"
version: 1
ledger:
  include_prompt_content: true
agents:
  - id: codex
    label: Codex
    match:
      process_names: [codex]
"#,
    )
    .unwrap_err();

    assert!(matches!(err, ConfigError::PromptCaptureUnsupported));
}

#[test]
fn rejects_unimplemented_egress_fields() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("curb.yaml");
    std::fs::write(
        &path,
        r#"
version: 1
alerts:
  webhook_url: https://example.invalid/curb/alerts
  slack_webhook_url: https://example.invalid/slack
ledger:
  forward_url: https://example.invalid/curb/events
agents:
  - id: codex
    label: Codex
    match:
      process_names: [codex]
"#,
    )
    .unwrap();

    let err = Config::load(&path).unwrap_err();

    assert!(matches!(err, ConfigError::Parse { .. }));
}

#[test]
fn rejects_duplicate_agent_ids() {
    let err = load_from_str(
        r#"
version: 1
agents:
  - id: codex
    label: Codex
    match:
      process_names: [codex]
  - id: codex
    label: Codex Again
    match:
      process_names: [codex]
"#,
    )
    .unwrap_err();

    assert!(matches!(err, ConfigError::DuplicateAgentId(id) if id == "codex"));
}

#[test]
fn desktop_app_roots_are_not_termination_targets_by_default() {
    let agent = Agent {
        id: "codex-desktop".to_string(),
        label: "Codex Desktop".to_string(),
        matcher: Match {
            bundle_ids: vec!["com.openai.codex".to_string()],
            ..Match::default()
        },
        ..Agent::default()
    };

    assert!(!agent.termination_allowed());
}

#[test]
fn supervised_desktop_worker_is_watch_only_unless_escalated() {
    let worker = default_process_agents()
        .into_iter()
        .find(|agent| agent.id == "codex-desktop-worker")
        .expect("codex-desktop-worker is a default agent");
    // It is a real process, but supervised — futile to kill the leaf.
    assert!(worker.termination_allowed());
    assert!(worker.is_supervised());
    assert!(!worker.can_terminate(false));
    assert!(worker.can_terminate(true));

    let cli = default_process_agents()
        .into_iter()
        .find(|agent| agent.id == "codex-cli")
        .expect("codex-cli is a default agent");
    assert!(!cli.is_supervised());
    assert!(cli.can_terminate(false));
}

#[test]
fn parses_composite_duration_like_go() {
    assert_eq!(
        parse_duration("1h30m").unwrap(),
        StdDuration::from_secs(90 * 60)
    );
    assert_eq!(
        parse_duration("15m10s").unwrap(),
        StdDuration::from_secs(15 * 60 + 10)
    );
}

fn load_from_str(raw: &str) -> Result<Config, ConfigError> {
    let mut cfg: Config = serde_yaml::from_str(raw).unwrap();
    cfg.set_defaults();
    cfg.validate()?;
    Ok(cfg)
}
