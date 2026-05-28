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
