use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::tempdir;

#[test]
fn init_creates_default_user_config_without_synthetic_demo_agent() {
    let dir = tempdir().expect("dir");
    let config = dir.path().join("config.yaml");
    let mut cmd = Command::cargo_bin("curb").expect("curb binary");

    cmd.args(["init", "--config"])
        .arg(&config)
        .assert()
        .success()
        .stdout(predicate::str::contains("created config:"))
        .stdout(predicate::str::contains("next: curb app"));

    let cfg = curb_core::config::Config::load(&config).expect("config");
    assert_eq!(cfg.mode, curb_core::config::Mode::Visibility);
    assert_eq!(cfg.agents.len(), 4);
    assert!(cfg.agents.iter().all(|agent| agent.termination_allowed()));
    assert_eq!(
        cfg.ledger.path,
        config.parent().unwrap().join("runs.ndjson")
    );
}

#[test]
fn config_presets_update_default_config_path() {
    let dir = tempdir().expect("dir");
    let config = dir.path().join("curb.yaml");
    let mut init = Command::cargo_bin("curb").expect("curb binary");
    init.args(["init", "--config"])
        .arg(&config)
        .assert()
        .success();

    let mut cmd = Command::cargo_bin("curb").expect("curb binary");
    cmd.env("CURB_CONFIG", &config)
        .args(["config", "aggressive"])
        .assert()
        .success()
        .stdout(predicate::str::contains("mode: enforcement"))
        .stdout(predicate::str::contains("warn: 250k tokens per turn"))
        .stdout(predicate::str::contains("Codex Desktop Worker"));

    let cfg = curb_core::config::Config::load(&config).expect("config");
    assert_eq!(cfg.mode, curb_core::config::Mode::Enforcement);
    assert_eq!(cfg.usage.warn_turn_tokens, 250_000);
    assert_eq!(cfg.usage.kill_turn_tokens, 750_000);
    assert_eq!(cfg.usage.scan_interval.as_std().as_secs(), 1);
}

#[test]
fn config_set_updates_first_class_policy_fields() {
    let dir = tempdir().expect("dir");
    let config = dir.path().join("curb.yaml");
    let mut init = Command::cargo_bin("curb").expect("curb binary");
    init.args(["init", "--config"])
        .arg(&config)
        .assert()
        .success();

    let mut cmd = Command::cargo_bin("curb").expect("curb binary");
    cmd.env("CURB_CONFIG", &config)
        .args([
            "config",
            "set",
            "--mode",
            "enforcement",
            "--warn-after",
            "2m",
            "--kill-after",
            "4m",
            "--grace",
            "30s",
            "--scan",
            "5s",
            "--usage",
            "true",
            "--warn-turn-tokens",
            "1000",
            "--kill-turn-tokens",
            "2000",
            "--usage-window",
            "10m",
            "--usage-scan",
            "2s",
            "--ledger-forward-url",
            "off",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("mode: enforcement"))
        .stdout(predicate::str::contains("warn: 1k tokens per turn"))
        .stdout(predicate::str::contains("stop: 2k tokens per turn"));

    let cfg = curb_core::config::Config::load(&config).expect("config");
    assert_eq!(cfg.mode, curb_core::config::Mode::Enforcement);
    assert_eq!(cfg.defaults.warn_after.as_std().as_secs(), 120);
    assert_eq!(cfg.defaults.kill_after.as_std().as_secs(), 240);
    assert_eq!(cfg.defaults.kill_grace_period.as_std().as_secs(), 30);
    assert_eq!(cfg.usage.grace_period.as_std().as_secs(), 30);
    assert_eq!(cfg.service.scan_interval.as_std().as_secs(), 5);
    assert_eq!(cfg.usage.warn_turn_tokens, 1000);
    assert_eq!(cfg.usage.kill_turn_tokens, 2000);
    assert_eq!(cfg.usage.window.as_std().as_secs(), 600);
    assert_eq!(cfg.usage.scan_interval.as_std().as_secs(), 2);
    assert!(cfg.ledger.forward_url.is_empty());
    assert!(cfg.agents.iter().all(|agent| {
        agent
            .policy
            .as_ref()
            .is_some_and(|policy| policy.kill_grace_period.as_std().as_secs() == 30)
    }));
}

#[test]
fn config_path_uses_curb_config_environment() {
    let dir = tempdir().expect("dir");
    let config = dir.path().join("curb.yaml");
    let mut cmd = Command::cargo_bin("curb").expect("curb binary");

    cmd.env("CURB_CONFIG", &config)
        .args(["config", "path"])
        .assert()
        .success()
        .stdout(predicate::str::contains(config.display().to_string()));
}

#[test]
fn install_copies_current_binary_to_prefix_bin() {
    let dir = tempdir().expect("dir");
    let mut cmd = Command::cargo_bin("curb").expect("curb binary");

    cmd.args(["install", "--prefix"])
        .arg(dir.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("installed:"))
        .stdout(predicate::str::contains("next: add"));

    let name = if cfg!(windows) { "curb.exe" } else { "curb" };
    assert!(dir.path().join("bin").join(name).is_file());
}

#[test]
fn validate_config_matches_go_oracle_shape() {
    let mut cmd = Command::cargo_bin("curb").expect("curb binary");

    cmd.args(["validate-config", "configs/curb.example.yaml"])
        .assert()
        .success()
        .stdout(predicate::str::contains(
            "ok config=configs/curb.example.yaml mode=visibility agents=5 ledger=.curb/runs.ndjson",
        ));
}

#[test]
fn usage_reads_synthetic_provider_metadata() {
    let home = tempdir().expect("home");
    let codex_dir = home.path().join(".codex").join("archived_sessions");
    std::fs::create_dir_all(&codex_dir).expect("codex dir");
    std::fs::write(
        codex_dir.join("rollout.jsonl"),
        r#"{"timestamp":"2026-05-19T16:00:00Z","type":"session_meta","payload":{"id":"session_codex","cwd":"/repo"}}
{"timestamp":"2026-05-19T16:01:00Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":100,"cached_input_tokens":20,"output_tokens":5,"reasoning_output_tokens":2,"total_tokens":107},"total_token_usage":{"total_tokens":107},"model_context_window":258400}}}
"#,
    )
    .expect("codex fixture");

    let mut cmd = Command::cargo_bin("curb").expect("curb binary");
    cmd.args(["usage", "--home"])
        .arg(home.path())
        .arg("--all")
        .assert()
        .success()
        .stdout(predicate::str::contains("curb usage"))
        .stdout(predicate::str::contains("codex 1 events"))
        .stdout(predicate::str::contains("session_codex"))
        .stdout(predicate::str::contains("total=107"));
}

#[test]
fn tail_once_prints_recent_synthetic_usage_events() {
    let home = tempdir().expect("home");
    write_synthetic_codex_usage(home.path(), "session_codex", "/repo", 107);

    let mut cmd = Command::cargo_bin("curb").expect("curb binary");
    cmd.args(["tail", "--once", "--home"])
        .arg(home.path())
        .args(["--since", "1h"])
        .assert()
        .success()
        .stdout(predicate::str::contains("curb tail"))
        .stdout(predicate::str::contains(
            "scanning usage events from the last 1h",
        ))
        .stdout(predicate::str::contains("codex"))
        .stdout(predicate::str::contains("session_codex"))
        .stdout(predicate::str::contains("total=107"))
        .stdout(predicate::str::contains("output=5"))
        .stdout(predicate::str::contains("cwd=/repo"));
}

#[test]
fn tail_rejects_invalid_duration() {
    let home = tempdir().expect("home");
    let mut cmd = Command::cargo_bin("curb").expect("curb binary");

    cmd.args(["tail", "--once", "--home"])
        .arg(home.path())
        .args(["--since", "soon"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid duration"));
}

#[test]
fn dashboard_prints_service_snapshot_from_synthetic_usage() {
    let home = tempdir().expect("home");
    let state = tempdir().expect("state");
    let config_path = state.path().join("curb.yaml");
    write_synthetic_codex_usage(home.path(), "session_codex", "/repo", 107);
    let mut cfg = curb_core::config::Config::local_default(
        curb_core::config::Mode::Visibility,
        state.path().join("state"),
    );
    cfg.ledger.path = cfg.service.state_dir.join("runs.ndjson");
    cfg.save(&config_path).expect("save config");

    let mut cmd = Command::cargo_bin("curb").expect("curb binary");
    cmd.args(["dashboard", "--config"])
        .arg(&config_path)
        .arg("--home")
        .arg(home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("curb ·"))
        .stdout(predicate::str::contains("codex"))
        .stdout(predicate::str::contains("repo"))
        .stdout(predicate::str::contains("per turn"));
}

#[test]
fn dashboard_json_prints_snapshot_read_model() {
    let home = tempdir().expect("home");
    let state = tempdir().expect("state");
    let config_path = state.path().join("curb.yaml");
    write_synthetic_codex_usage(home.path(), "session_codex", "/repo", 107);
    let cfg = curb_core::config::Config::local_default(
        curb_core::config::Mode::Visibility,
        state.path().join("state"),
    );
    cfg.save(&config_path).expect("save config");

    let mut cmd = Command::cargo_bin("curb").expect("curb binary");
    cmd.args(["dashboard", "--json", "--config"])
        .arg(&config_path)
        .arg("--home")
        .arg(home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("\"overview\""))
        .stdout(predicate::str::contains("\"sessions\""))
        .stdout(predicate::str::contains("\"provider\": \"codex\""));
}

#[test]
fn doctor_checks_config_state_ledger_and_process_capture() {
    let state = tempdir().expect("state");
    let config_path = state.path().join("curb.yaml");
    let cfg = curb_core::config::Config::local_default(
        curb_core::config::Mode::Visibility,
        state.path().join("state"),
    );
    cfg.save(&config_path).expect("save config");

    let mut cmd = Command::cargo_bin("curb").expect("curb binary");
    cmd.args(["doctor", "--config"])
        .arg(&config_path)
        .assert()
        .success()
        .stdout(predicate::str::contains("config: ok"))
        .stdout(predicate::str::contains("state_dir: ok"))
        .stdout(predicate::str::contains("ledger: ok"))
        .stdout(predicate::str::contains("process_snapshot: ok"))
        .stdout(predicate::str::contains("notifications:"));

    let events = curb_core::ledger::read(state.path().join("state").join("runs.ndjson"))
        .expect("ledger events");
    assert!(events.iter().any(|event| event.event_type == "doctor"));
}

#[test]
fn serve_rejects_non_loopback_address_before_binding() {
    let mut cmd = Command::cargo_bin("curb").expect("curb binary");

    cmd.args(["serve", "--addr", "0.0.0.0:0"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("serve address must be loopback"));
}

#[test]
fn app_rejects_non_loopback_address_before_opening() {
    let mut cmd = Command::cargo_bin("curb").expect("curb binary");

    cmd.args(["app", "--addr", "0.0.0.0:0"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("serve address must be loopback"));
}

#[test]
fn daemon_and_api_aliases_route_to_serve_command() {
    let mut daemon = Command::cargo_bin("curb").expect("curb binary");
    daemon
        .args(["daemon", "--addr", "0.0.0.0:0"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("serve address must be loopback"));

    let mut api = Command::cargo_bin("curb").expect("curb binary");
    api.args(["api", "--addr", "0.0.0.0:0"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("serve address must be loopback"));
}

#[test]
fn help_advanced_lists_compatibility_aliases() {
    let mut cmd = Command::cargo_bin("curb").expect("curb binary");

    cmd.args(["help", "advanced"])
        .assert()
        .success()
        .stdout(predicate::str::contains("serve|daemon|api"))
        .stdout(predicate::str::contains("run|start|watch"))
        .stdout(predicate::str::contains("scan"));
}

#[test]
fn scan_reports_no_matches_for_unmatched_config() {
    let home = tempdir().expect("home");
    let state = tempdir().expect("state");
    let config_path = state.path().join("curb.yaml");
    let mut cfg = curb_core::config::Config::local_default(
        curb_core::config::Mode::Visibility,
        state.path().join("state"),
    );
    cfg.agents = vec![curb_core::config::Agent {
        id: "impossible-agent".to_string(),
        label: "Impossible Agent".to_string(),
        family: "test".to_string(),
        kind: curb_core::config::AgentKind::Process,
        matcher: curb_core::config::Match {
            process_names: vec!["curb-test-process-that-does-not-exist".to_string()],
            ..curb_core::config::Match::default()
        },
        policy: None,
    }];
    cfg.save(&config_path).expect("save config");

    let mut cmd = Command::cargo_bin("curb").expect("curb binary");
    cmd.args(["scan", "--config"])
        .arg(&config_path)
        .arg("--home")
        .arg(home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("curb scan"))
        .stdout(predicate::str::contains(
            "no configured agent workers matched",
        ));
}

#[test]
fn watch_once_runs_a_single_usage_policy_scan() {
    let home = tempdir().expect("home");
    let state = tempdir().expect("state");
    let config_path = state.path().join("curb.yaml");
    let mut cfg = curb_core::config::Config::load("configs/curb.example.yaml").expect("config");
    cfg.service.state_dir = state.path().join("state");
    cfg.ledger.path = cfg.service.state_dir.join("runs.ndjson");
    cfg.save(&config_path).expect("save config");

    let mut cmd = Command::cargo_bin("curb").expect("curb binary");
    cmd.args(["watch", "--once", "--config"])
        .arg(&config_path)
        .arg("--home")
        .arg(home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("curb watcher"))
        .stdout(predicate::str::contains("scan: status="));
}

#[test]
fn status_reports_warning_sessions_from_usage_read_model() {
    let home = tempdir().expect("home");
    let state = tempdir().expect("state");
    let config_path = warning_config(state.path());
    write_synthetic_codex_usage(home.path(), "session_codex", "/repo", 150);

    let mut cmd = Command::cargo_bin("curb").expect("curb binary");
    cmd.args(["status", "--config"])
        .arg(&config_path)
        .arg("--home")
        .arg(home.path())
        .assert()
        .success()
        .stdout(predicate::str::contains("curb status"))
        .stdout(predicate::str::contains("status: WATCH"))
        .stdout(predicate::str::contains("1 warn"))
        .stdout(predicate::str::contains("attention"))
        .stdout(predicate::str::contains("codex:session_codex"))
        .stdout(predicate::str::contains(
            "next: curb ack codex:session_codex",
        ));
}

#[test]
fn runs_prints_and_filters_session_read_model() {
    let home = tempdir().expect("home");
    let state = tempdir().expect("state");
    let config_path = warning_config(state.path());
    write_synthetic_codex_usage(home.path(), "session_codex", "/repo", 150);

    let mut cmd = Command::cargo_bin("curb").expect("curb binary");
    cmd.args([
        "sessions",
        "--active",
        "--state",
        "attention",
        "--provider",
        "codex",
        "--config",
    ])
    .arg(&config_path)
    .arg("--home")
    .arg(home.path())
    .assert()
    .success()
    .stdout(predicate::str::contains("curb runs"))
    .stdout(predicate::str::contains("sessions"))
    .stdout(predicate::str::contains("codex:session_codex"))
    .stdout(predicate::str::contains("curb ack"));
}

#[test]
fn ack_acknowledges_usage_session_and_records_ledger_event() {
    let home = tempdir().expect("home");
    let state = tempdir().expect("state");
    let config_path = warning_config(state.path());
    write_synthetic_codex_usage(home.path(), "session_codex", "/repo", 150);

    let mut cmd = Command::cargo_bin("curb").expect("curb binary");
    cmd.args(["ack", "codex:session_codex", "--config"])
        .arg(&config_path)
        .arg("--home")
        .arg(home.path())
        .args(["--extend", "30s", "--reason", "still watching"])
        .assert()
        .success()
        .stdout(predicate::str::contains("acknowledged codex:session_codex"))
        .stdout(predicate::str::contains("extended: 30s"))
        .stdout(predicate::str::contains("reason: still watching"));

    let events = curb_core::ledger::read(state.path().join("state").join("runs.ndjson"))
        .expect("ledger events");
    assert!(events.iter().any(|event| {
        event.event_type == "session_ack_received"
            && event.message.as_deref() == Some("still watching")
    }));
}

fn write_synthetic_codex_usage(home: &std::path::Path, session: &str, cwd: &str, total: i64) {
    let codex_dir = home.join(".codex").join("archived_sessions");
    std::fs::create_dir_all(&codex_dir).expect("codex dir");
    let timestamp = chrono::Utc::now().to_rfc3339();
    std::fs::write(
        codex_dir.join("rollout.jsonl"),
        format!(
            r#"{{"timestamp":"{timestamp}","type":"session_meta","payload":{{"id":"{session}","cwd":"{cwd}"}}}}
{{"timestamp":"{timestamp}","type":"event_msg","payload":{{"type":"token_count","info":{{"last_token_usage":{{"input_tokens":100,"cached_input_tokens":20,"output_tokens":5,"reasoning_output_tokens":2,"total_tokens":{total}}},"total_token_usage":{{"total_tokens":{total}}},"model_context_window":258400}}}}}}
"#
        ),
    )
    .expect("codex fixture");
}

fn warning_config(root: &std::path::Path) -> std::path::PathBuf {
    let config_path = root.join("curb.yaml");
    let mut cfg = curb_core::config::Config::local_default(
        curb_core::config::Mode::Alert,
        root.join("state"),
    );
    cfg.ledger.path = cfg.service.state_dir.join("runs.ndjson");
    // Synthetic codex fixture spends 87 tokens/turn (uncached input + output +
    // reasoning), so warn below that and kill above it to land in the warn band.
    cfg.usage.warn_turn_tokens = 50;
    cfg.usage.kill_turn_tokens = 300;
    cfg.defaults.ack_extension = curb_core::config::HumanDuration::seconds(60);
    cfg.save(&config_path).expect("save config");
    config_path
}
