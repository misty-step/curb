use assert_cmd::Command;
use predicates::prelude::*;

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
