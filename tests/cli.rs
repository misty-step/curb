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

    let cfg = curb::config::Config::load(&config).expect("config");
    assert_eq!(cfg.mode, curb::config::Mode::Visibility);
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

    let cfg = curb::config::Config::load(&config).expect("config");
    assert_eq!(cfg.mode, curb::config::Mode::Enforcement);
    assert_eq!(cfg.usage.warn_turn_tokens, 250_000);
    assert_eq!(cfg.usage.kill_turn_tokens, 750_000);
    assert_eq!(cfg.usage.scan_interval.as_std().as_secs(), 1);
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
    let mut cfg = curb::config::Config::local_default(
        curb::config::Mode::Visibility,
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
        .stdout(predicate::str::contains("curb dashboard"))
        .stdout(predicate::str::contains("status:"))
        .stdout(predicate::str::contains("live agents:"))
        .stdout(predicate::str::contains("sessions"))
        .stdout(predicate::str::contains("codex"))
        .stdout(predicate::str::contains("session: session_codex"));
}

#[test]
fn dashboard_json_prints_snapshot_read_model() {
    let home = tempdir().expect("home");
    let state = tempdir().expect("state");
    let config_path = state.path().join("curb.yaml");
    write_synthetic_codex_usage(home.path(), "session_codex", "/repo", 107);
    let cfg = curb::config::Config::local_default(
        curb::config::Mode::Visibility,
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
    let cfg = curb::config::Config::local_default(
        curb::config::Mode::Visibility,
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

    let events =
        curb::ledger::read(state.path().join("state").join("runs.ndjson")).expect("ledger events");
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
fn watch_once_runs_a_single_usage_policy_scan() {
    let home = tempdir().expect("home");
    let state = tempdir().expect("state");
    let config_path = state.path().join("curb.yaml");
    let mut cfg = curb::config::Config::load("configs/curb.example.yaml").expect("config");
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
        .stdout(predicate::str::contains("curb rust watcher"))
        .stdout(predicate::str::contains("scan: status="));
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
