use assert_cmd::Command;
use predicates::prelude::*;
use tempfile::tempdir;

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
fn serve_rejects_non_loopback_address_before_binding() {
    let mut cmd = Command::cargo_bin("curb").expect("curb binary");

    cmd.args(["serve", "--addr", "0.0.0.0:0"])
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
