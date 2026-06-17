use std::fs::{self, File, OpenOptions};
use std::io::Write;

use chrono::TimeZone;
use tempfile::tempdir;

use super::*;

#[test]
fn codex_archived_sessions_extracts_token_counts_without_content() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("rollout.jsonl");
    fs::write(
        &path,
        r#"{"timestamp":"2026-05-19T16:00:00Z","type":"session_meta","payload":{"id":"session_codex","cwd":"/repo"}}
{"timestamp":"2026-05-19T16:01:00Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":100,"cached_input_tokens":20,"output_tokens":5,"reasoning_output_tokens":2,"total_tokens":107},"total_token_usage":{"total_tokens":107},"model_context_window":258400}}}
{"timestamp":"2026-05-19T16:02:00Z","type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":100,"cached_input_tokens":20,"output_tokens":5,"reasoning_output_tokens":2,"total_tokens":107},"total_token_usage":{"total_tokens":107},"model_context_window":258400}}}
"#,
    )
    .unwrap();

    let (events, report) = codex_archived_sessions_since(dir.path(), None).unwrap();

    assert_eq!(report.files, 1);
    assert_eq!(report.events, 1);
    assert_eq!(events.len(), 1);
    let event = &events[0];
    assert_eq!(event.provider, "codex");
    assert_eq!(event.session_id.as_deref(), Some("session_codex"));
    assert_eq!(event.input_tokens, 100);
    assert_eq!(event.cached_input_tokens, 20);
    assert_eq!(event.output_tokens, 5);
    assert_eq!(event.reasoning_output_tokens, 2);
    assert_eq!(event.total_tokens, 107);
    assert_eq!(event.spent_tokens, 87); // uncached input (100-20) + output 5 + reasoning 2
    assert_eq!(event.cwd.as_deref(), Some(Path::new("/repo")));
    assert_eq!(event.model_context_window, 258400);
}

#[test]
fn claude_projects_dedupes_requests_and_summarizes_model_usage() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("projects").join("-repo");
    fs::create_dir_all(&root).unwrap();
    let path = root.join("session.jsonl");
    fs::write(
        &path,
        r#"{"timestamp":"2026-05-19T20:00:00Z","requestId":"req_1","sessionId":"session_claude","uuid":"turn_1","cwd":"/repo","message":{"id":"msg_1","model":"claude-opus-4-7","usage":{"input_tokens":1,"cache_creation_input_tokens":30,"cache_read_input_tokens":40,"output_tokens":5}}}
{"timestamp":"2026-05-19T20:00:01Z","requestId":"req_1","sessionId":"session_claude","uuid":"turn_1_dup","cwd":"/repo","message":{"id":"msg_1","model":"claude-opus-4-7","usage":{"input_tokens":1,"cache_creation_input_tokens":30,"cache_read_input_tokens":40,"output_tokens":5}}}
{"timestamp":"2026-05-19T20:01:00Z","requestId":"req_2","sessionId":"session_claude","uuid":"turn_2","cwd":"/repo","message":{"id":"msg_2","model":"claude-sonnet-4-5","usage":{"input_tokens":2,"cache_creation_input_tokens":3,"cache_read_input_tokens":4,"output_tokens":6}}}
"#,
    )
    .unwrap();

    let (events, report) = claude_projects_since(dir.path().join("projects"), None).unwrap();

    assert_eq!(report.files, 1);
    assert_eq!(report.events, 2);
    assert_eq!(events.len(), 2);
    assert_eq!(
        events.iter().map(|event| event.input_tokens).sum::<i64>(),
        3
    );
    assert_eq!(
        events
            .iter()
            .map(|event| event.cache_creation_input_tokens)
            .sum::<i64>(),
        33
    );
    assert_eq!(
        events
            .iter()
            .map(|event| event.cached_input_tokens)
            .sum::<i64>(),
        44
    );
    assert_eq!(
        events.iter().map(|event| event.output_tokens).sum::<i64>(),
        11
    );
    assert_eq!(
        events.iter().map(|event| event.total_tokens).sum::<i64>(),
        91
    );
    assert_eq!(
        events.iter().map(|event| event.spent_tokens).sum::<i64>(),
        47
    );
    let mut models = events
        .iter()
        .filter_map(|event| event.model.as_deref())
        .collect::<Vec<_>>();
    models.sort();
    models.dedup();
    assert_eq!(models, vec!["claude-opus-4-7", "claude-sonnet-4-5"]);
}

#[test]
fn pi_sessions_extract_token_counts_without_content() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("--repo--");
    fs::create_dir_all(&root).unwrap();
    let path = root.join("2026-06-01T10-00-00Z_session-pi.jsonl");
    fs::write(
        &path,
        r#"{"type":"session","version":3,"id":"session_pi","timestamp":"2026-06-01T10:00:00.000Z","cwd":"/repo"}
{"type":"message","id":"user_1","parentId":null,"timestamp":"2026-06-01T10:00:01.000Z","message":{"role":"user","content":"private prompt"}}
{"type":"message","id":"assistant_1","parentId":"user_1","timestamp":"2026-06-01T10:00:02.000Z","message":{"role":"assistant","content":[{"type":"text","text":"private response"}],"provider":"anthropic","model":"claude-sonnet-4-5","usage":{"input":100,"output":25,"cacheRead":40,"cacheWrite":5,"totalTokens":170},"stopReason":"stop"}}
{"type":"branch_summary","id":"branch_1","parentId":"assistant_1","timestamp":"2026-06-01T10:00:03.000Z","summary":"content-bearing summary ignored"}
"#,
    )
    .unwrap();

    let (events, report) = pi_sessions_since(dir.path(), None).unwrap();

    assert_eq!(report.files, 1);
    assert_eq!(report.events, 2);
    assert_eq!(events.len(), 2);
    assert!(matches!(events[0].kind, EventKind::UserInput));
    let event = &events[1];
    assert_eq!(event.provider, "pi");
    assert_eq!(event.source, "pi.sessions");
    assert_eq!(event.session_id.as_deref(), Some("session_pi"));
    assert_eq!(event.turn_id.as_deref(), Some("assistant_1"));
    assert_eq!(event.model.as_deref(), Some("claude-sonnet-4-5"));
    assert_eq!(event.cwd.as_deref(), Some(Path::new("/repo")));
    assert_eq!(event.input_tokens, 100);
    assert_eq!(event.cached_input_tokens, 40);
    assert_eq!(event.cache_creation_input_tokens, 5);
    assert_eq!(event.output_tokens, 25);
    assert_eq!(event.total_tokens, 170);
    assert_eq!(event.spent_tokens, 130);
}

#[test]
fn reader_scans_codex_claude_and_pi_roots_under_home() {
    let home = tempdir().unwrap();
    let codex = home.path().join(".codex").join("archived_sessions");
    fs::create_dir_all(&codex).unwrap();
    fs::write(
        codex.join("rollout.jsonl"),
        codex_fixture("session_codex", "/repo", "2026-05-19T16:00:00Z", 107, 107),
    )
    .unwrap();
    let claude = home.path().join(".claude").join("projects").join("-repo");
    fs::create_dir_all(&claude).unwrap();
    fs::write(
        claude.join("session.jsonl"),
        r#"{"timestamp":"2026-05-19T20:00:00Z","requestId":"req_1","sessionId":"session_claude","uuid":"turn_1","cwd":"/repo","message":{"id":"msg_1","model":"claude-opus-4-7","usage":{"input_tokens":1,"cache_creation_input_tokens":30,"cache_read_input_tokens":40,"output_tokens":5}}}
"#,
    )
    .unwrap();
    let pi = home
        .path()
        .join(".pi")
        .join("agent")
        .join("sessions")
        .join("--repo--");
    fs::create_dir_all(&pi).unwrap();
    fs::write(
        pi.join("2026-06-01T10-00-00Z_session-pi.jsonl"),
        r#"{"type":"session","version":3,"id":"session_pi","timestamp":"2026-06-01T10:00:00.000Z","cwd":"/repo"}
{"type":"message","id":"assistant_1","parentId":null,"timestamp":"2026-06-01T10:00:02.000Z","message":{"role":"assistant","content":[{"type":"text","text":"private response"}],"provider":"anthropic","model":"claude-sonnet-4-5","usage":{"input":10,"output":2,"cacheRead":3,"cacheWrite":4,"totalTokens":19},"stopReason":"stop"}}
"#,
    )
    .unwrap();

    let scan = Reader::new(home.path()).scan_since(None).unwrap();

    assert_eq!(scan.sources.len(), 3);
    assert_eq!(scan.events.len(), 3);
    assert_eq!(scan.sources[0].provider, "codex");
    assert_eq!(scan.sources[0].events, 1);
    assert_eq!(scan.sources[1].provider, "claude");
    assert_eq!(scan.sources[1].events, 1);
    assert_eq!(scan.sources[2].provider, "pi");
    assert_eq!(scan.sources[2].events, 1);
}

#[test]
fn reader_scans_live_codex_sessions_under_home() {
    let home = tempdir().unwrap();
    let live = home
        .path()
        .join(".codex")
        .join("sessions")
        .join("2026")
        .join("05")
        .join("28");
    fs::create_dir_all(&live).unwrap();
    fs::write(
        live.join("rollout.jsonl"),
        codex_fixture(
            "session_live_codex",
            "/repo",
            "2026-05-28T16:00:00Z",
            211,
            211,
        ),
    )
    .unwrap();

    let scan = Reader::new(home.path()).scan_since(None).unwrap();

    assert_eq!(scan.sources[0].provider, "codex");
    assert_eq!(scan.sources[0].files, 1);
    assert_eq!(scan.sources[0].events, 1);
    assert_eq!(scan.events.len(), 1);
    assert_eq!(scan.events[0].provider, "codex");
    assert_eq!(
        scan.events[0].session_id.as_deref(),
        Some("session_live_codex")
    );
    assert_eq!(scan.events[0].total_tokens, 211);
}

#[test]
fn lookback_scan_tails_large_live_codex_sessions() {
    let home = tempdir().unwrap();
    let live = home
        .path()
        .join(".codex")
        .join("sessions")
        .join("2026")
        .join("05")
        .join("28");
    fs::create_dir_all(&live).unwrap();
    let path = live.join("large.jsonl");
    let padding = strings_of_length("a", CODEX_LIVE_COLD_READ_LIMIT as usize);
    fs::write(
        &path,
        format!(
            r#"{{"timestamp":"2026-05-28T16:00:00Z","type":"session_meta","payload":{{"id":"session_live_tail","cwd":"/repo"}}}}
{{"timestamp":"2026-05-28T16:00:01Z","type":"event_msg","payload":{{"type":"ignored"}},"padding":"{padding}"}}
{}"#,
            codex_token_row("2026-05-28T16:00:02Z", 377, 377)
        ),
    )
    .unwrap();

    let since = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
    let (events, _) = Reader::new(home.path()).events_since(Some(since)).unwrap();

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].session_id.as_deref(), Some("session_live_tail"));
    assert_eq!(events[0].cwd.as_deref(), Some(Path::new("/repo")));
    assert_eq!(events[0].total_tokens, 377);
}

#[test]
fn live_tail_metadata_reader_keeps_session_identity_for_large_logs() {
    let home = tempdir().unwrap();
    let live = home
        .path()
        .join(".codex")
        .join("sessions")
        .join("2026")
        .join("05")
        .join("28");
    fs::create_dir_all(&live).unwrap();
    let path = live.join("large.jsonl");
    let padding = strings_of_length("x", CODEX_LIVE_COLD_READ_LIMIT as usize);
    fs::write(
        &path,
        format!(
            r#"{{"timestamp":"2026-05-28T16:00:00Z","type":"session_meta","payload":{{"id":"expected_session","cwd":"/expected/repo"}}}}
{{"timestamp":"2026-05-28T16:00:01Z","type":"event_msg","payload":{{"type":"ignored"}},"padding":"{padding}"}}
{}"#,
            codex_token_row("2026-05-28T16:00:02Z", 144, 144)
        ),
    )
    .unwrap();

    let since = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
    let (events, _) = Reader::new(home.path()).events_since(Some(since)).unwrap();

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].session_id.as_deref(), Some("expected_session"));
    assert_eq!(events[0].cwd.as_deref(), Some(Path::new("/expected/repo")));
}

#[test]
fn since_filters_event_timestamps() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("rollout.jsonl");
    fs::write(
        &path,
        codex_fixture("session_codex", "/repo", "2026-05-19T16:00:00Z", 107, 107)
            + &codex_token_row("2026-05-19T17:00:00Z", 211, 318),
    )
    .unwrap();

    let since = Utc.with_ymd_and_hms(2026, 5, 19, 16, 30, 0).unwrap();
    let (events, report) = codex_archived_sessions_since(dir.path(), Some(since)).unwrap();

    assert_eq!(report.events, 1);
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].total_tokens, 211);
}

#[test]
fn invalid_json_returns_source_path_error() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("rollout.jsonl");
    fs::write(&path, r#"{"bad""#).unwrap();

    let err = codex_archived_sessions_since(dir.path(), None).unwrap_err();

    assert!(err.to_string().contains("rollout.jsonl"));
}

#[test]
fn appended_older_timestamp_is_included_when_since_allows_it() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("rollout.jsonl");
    fs::write(
        &path,
        codex_fixture("session_codex", "/repo", "2026-05-19T16:00:00Z", 107, 107),
    )
    .unwrap();
    OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(codex_token_row("2026-05-19T15:30:00Z", 211, 318).as_bytes())
        .unwrap();

    let since = Utc.with_ymd_and_hms(2026, 5, 19, 15, 0, 0).unwrap();
    let (events, _) = codex_archived_sessions_since(dir.path(), Some(since)).unwrap();

    assert!(events.iter().any(|event| event.total_tokens == 211));
}

#[test]
fn reader_caches_returned_events_without_caller_mutation() {
    let home = tempdir().unwrap();
    let codex = home.path().join(".codex").join("archived_sessions");
    fs::create_dir_all(&codex).unwrap();
    fs::write(
        codex.join("rollout.jsonl"),
        codex_fixture("session_codex", "/repo", "2026-05-19T16:00:00Z", 107, 107),
    )
    .unwrap();
    let reader = Reader::new(home.path());

    let (mut events, _) = reader.events_since(None).unwrap();
    events[0].session_id = Some("mutated".to_string());
    let (events, _) = reader.events_since(None).unwrap();

    assert_eq!(events[0].session_id.as_deref(), Some("session_codex"));
}

#[test]
fn reader_prunes_deleted_provider_files() {
    let home = tempdir().unwrap();
    let codex = home.path().join(".codex").join("archived_sessions");
    fs::create_dir_all(&codex).unwrap();
    let path = codex.join("rollout.jsonl");
    fs::write(
        &path,
        codex_fixture("session_codex", "/repo", "2026-05-19T16:00:00Z", 107, 107),
    )
    .unwrap();
    let reader = Reader::new(home.path());
    assert_eq!(reader.events_since(None).unwrap().0.len(), 1);

    fs::remove_file(&path).unwrap();
    let (events, reports) = reader.events_since(None).unwrap();

    assert!(events.is_empty());
    assert_eq!(reports[0].files, 0);
    assert_eq!(reports[0].events, 0);
}

#[test]
fn reader_persists_cache_and_reads_appended_bytes_after_restart() {
    let home = tempdir().unwrap();
    let state = tempdir().unwrap();
    let codex = home.path().join(".codex").join("archived_sessions");
    fs::create_dir_all(&codex).unwrap();
    let path = codex.join("rollout.jsonl");
    fs::write(
        &path,
        codex_fixture("session_codex", "/repo", "2026-05-19T16:00:00Z", 107, 107),
    )
    .unwrap();
    let reader = Reader::with_state(home.path(), state.path());
    assert_eq!(reader.events_since(None).unwrap().0.len(), 1);

    OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(codex_token_row("2026-05-19T16:02:00Z", 211, 318).as_bytes())
        .unwrap();
    let restarted = Reader::with_state(home.path(), state.path());
    let (events, _) = restarted.events_since(None).unwrap();

    assert!(has_event(&events, 107, "2026-05-19T16:00:00Z"));
    assert!(has_event(&events, 211, "2026-05-19T16:02:00Z"));
    assert!(state.path().join("usage-cache.json").exists());
}

#[test]
fn reader_rejects_same_path_replacement_as_append() {
    let home = tempdir().unwrap();
    let state = tempdir().unwrap();
    let codex = home.path().join(".codex").join("archived_sessions");
    fs::create_dir_all(&codex).unwrap();
    let path = codex.join("rollout.jsonl");
    let initial = codex_fixture("session_codex", "/repo", "2026-05-19T16:00:00Z", 107, 107);
    fs::write(&path, &initial).unwrap();
    let reader = Reader::with_state(home.path(), state.path());
    assert_eq!(reader.events_since(None).unwrap().0.len(), 1);

    let replaced = strings_of_length("not-json", initial.len())
        + &codex_token_row("2026-05-19T16:02:00Z", 211, 318);
    fs::write(&path, replaced).unwrap();
    let restarted = Reader::with_state(home.path(), state.path());

    assert!(restarted.events_since(None).is_err());
}

#[test]
fn reader_rejects_same_path_replacement_after_unchanged_prefix() {
    let home = tempdir().unwrap();
    let state = tempdir().unwrap();
    let codex = home.path().join(".codex").join("archived_sessions");
    fs::create_dir_all(&codex).unwrap();
    let path = codex.join("rollout.jsonl");
    let prefix = strings_of_length(" ", 4096);
    let initial =
        prefix.clone() + &codex_fixture("session_codex", "/repo", "2026-05-19T16:00:00Z", 107, 107);
    fs::write(&path, &initial).unwrap();
    let reader = Reader::with_state(home.path(), state.path());
    assert_eq!(reader.events_since(None).unwrap().0.len(), 1);

    let replaced = prefix
        + &strings_of_length("not-json", initial.len() - 4096)
        + &codex_token_row("2026-05-19T16:02:00Z", 211, 318);
    fs::write(&path, replaced).unwrap();
    let restarted = Reader::with_state(home.path(), state.path());

    assert!(restarted.events_since(None).is_err());
}

#[test]
fn reader_scan_reports_provider_errors_without_losing_other_provider() {
    let home = tempdir().unwrap();
    let codex = home.path().join(".codex").join("archived_sessions");
    fs::create_dir_all(&codex).unwrap();
    fs::write(codex.join("bad.jsonl"), r#"{"bad""#).unwrap();
    let claude = home.path().join(".claude").join("projects").join("-repo");
    fs::create_dir_all(&claude).unwrap();
    fs::write(
        claude.join("session.jsonl"),
        r#"{"timestamp":"2026-05-19T20:00:00Z","requestId":"req_1","sessionId":"session_claude","uuid":"turn_1","cwd":"/repo","message":{"id":"msg_1","model":"claude-opus-4-7","usage":{"input_tokens":1,"cache_creation_input_tokens":30,"cache_read_input_tokens":40,"output_tokens":5}}}
"#,
    )
    .unwrap();

    let scan = Reader::new(home.path()).scan_since(None).unwrap();

    assert!(scan.error.is_some());
    assert_eq!(scan.sources[0].provider, "codex");
    assert!(scan.sources[0].error.is_some());
    assert_eq!(scan.sources[1].provider, "claude");
    assert_eq!(scan.sources[1].events, 1);
    assert_eq!(scan.sources[2].provider, "pi");
    assert_eq!(scan.sources[2].events, 0);
    assert_eq!(scan.events.len(), 1);
}

#[cfg(unix)]
#[test]
fn reader_reports_symlinked_provider_files_as_source_health_errors() {
    use std::os::unix::fs::symlink;

    let home = tempdir().unwrap();
    let outside = tempdir().unwrap();
    let outside_log = outside.path().join("outside.jsonl");
    fs::write(
        &outside_log,
        codex_fixture("outside_session", "/repo", "2026-05-19T16:00:00Z", 107, 107),
    )
    .unwrap();
    let codex = home.path().join(".codex").join("archived_sessions");
    fs::create_dir_all(&codex).unwrap();
    symlink(&outside_log, codex.join("escaped.jsonl")).unwrap();

    let scan = Reader::new(home.path()).scan_since(None).unwrap();

    assert!(
        scan.error
            .as_deref()
            .unwrap_or_default()
            .contains("symlink")
    );
    assert!(scan.events.is_empty());
    assert_eq!(scan.sources[0].provider, "codex");
    assert!(
        scan.sources[0]
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("symlink")
    );
}

#[test]
fn oversized_usage_files_are_source_health_errors_before_parsing() {
    let home = tempdir().unwrap();
    let codex = home.path().join(".codex").join("archived_sessions");
    fs::create_dir_all(&codex).unwrap();
    let path = codex.join("huge.jsonl");
    File::create(&path)
        .unwrap()
        .set_len(USAGE_FILE_MAX_BYTES + 1)
        .unwrap();

    let scan = Reader::new(home.path()).scan_since(None).unwrap();

    assert!(
        scan.error
            .as_deref()
            .unwrap_or_default()
            .contains("exceeds")
    );
    assert!(scan.events.is_empty());
    assert_eq!(scan.sources[0].provider, "codex");
    assert!(
        scan.sources[0]
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("exceeds")
    );
}

#[test]
fn one_oversized_file_does_not_blind_the_whole_provider() {
    let home = tempdir().unwrap();
    let codex = home.path().join(".codex").join("archived_sessions");
    fs::create_dir_all(&codex).unwrap();
    // A healthy session Curb can read.
    fs::write(
        codex.join("good.jsonl"),
        codex_fixture("session_good", "/repo", "2026-05-19T16:00:00Z", 107, 107),
    )
    .unwrap();
    // A sibling file whose single line blows the 1 MiB cap.
    let padding = lines::oversized_line_padding();
    fs::write(
        codex.join("huge.jsonl"),
        format!(
            r#"{{"timestamp":"2026-05-19T16:00:00Z","type":"session_meta","payload":{{"id":"s","cwd":"/repo"}},"padding":"{padding}"}}
"#
        ),
    )
    .unwrap();

    let scan = Reader::new(home.path()).scan_since(None).unwrap();

    // The healthy session is still ingested instead of the whole provider going dark.
    assert_eq!(
        scan.events.len(),
        1,
        "a healthy session must survive a sibling oversized file"
    );
    assert_eq!(scan.events[0].session_id.as_deref(), Some("session_good"));
    // ...and the oversized file is still surfaced as a source-health error.
    let codex_source = scan
        .sources
        .iter()
        .find(|source| source.provider == "codex")
        .expect("codex source health present");
    assert!(
        codex_source
            .error
            .as_deref()
            .unwrap_or_default()
            .contains("exceeds"),
        "the oversized file must still surface as a source-health error"
    );
}

#[test]
fn oversized_usage_lines_fail_before_json_parsing() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("large-line.jsonl");
    let padding = lines::oversized_line_padding();
    fs::write(
        &path,
        format!(
            r#"{{"timestamp":"2026-05-19T16:00:00Z","type":"session_meta","payload":{{"id":"s","cwd":"/repo"}},"padding":"{padding}"}}
"#
        ),
    )
    .unwrap();

    let err = codex_archived_sessions_since(dir.path(), None).unwrap_err();

    assert!(err.to_string().contains("line exceeds"));
}

#[test]
fn reader_hydrates_persisted_dedup_keys() {
    let home = tempdir().unwrap();
    let state = tempdir().unwrap();
    let codex = home.path().join(".codex").join("archived_sessions");
    fs::create_dir_all(&codex).unwrap();
    let path = codex.join("rollout.jsonl");
    fs::write(
        &path,
        codex_fixture("session_codex", "/repo", "2026-05-19T16:00:00Z", 107, 107),
    )
    .unwrap();
    let reader = Reader::with_state(home.path(), state.path());
    assert_eq!(reader.events_since(None).unwrap().0.len(), 1);

    OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(codex_token_row("2026-05-19T16:01:00Z", 107, 107).as_bytes())
        .unwrap();
    let restarted = Reader::with_state(home.path(), state.path());
    let (events, _) = restarted.events_since(None).unwrap();

    assert_eq!(events.len(), 1);
}

fn spent_after_last_boundary(events: &[Event]) -> i64 {
    let start = events
        .iter()
        .rposition(|event| matches!(event.kind, EventKind::UserInput))
        .map_or(0, |index| index + 1);
    events[start..]
        .iter()
        .filter(|event| matches!(event.kind, EventKind::TokenCheckpoint))
        .map(|event| event.spent_tokens)
        .sum()
}

#[test]
fn codex_user_message_emits_a_turn_boundary() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("rollout.jsonl");
    fs::write(
        &path,
        format!(
            r#"{{"timestamp":"2026-05-29T16:00:00Z","type":"session_meta","payload":{{"id":"s","cwd":"/repo"}}}}
{{"timestamp":"2026-05-29T16:00:01Z","type":"event_msg","payload":{{"type":"user_message"}}}}
{}{}"#,
            codex_token_row("2026-05-29T16:00:02Z", 100, 100),
            codex_token_row("2026-05-29T16:00:03Z", 200, 300),
        ),
    )
    .unwrap();

    let (events, _) = codex_archived_sessions_since(dir.path(), None).unwrap();
    let boundaries = events
        .iter()
        .filter(|event| matches!(event.kind, EventKind::UserInput))
        .count();

    assert_eq!(boundaries, 1);
    // Both checkpoints land after the boundary → one turn's spend. Each
    // fixture row spends uncached input (100-20) + output 5 + reasoning 2 = 87,
    // independent of the cached context size: 87 + 87 = 174.
    assert_eq!(spent_after_last_boundary(&events), 174);
}

#[test]
fn codex_response_item_user_message_resets_the_turn() {
    // Codex often records a prompt only as a `response_item` message with
    // role "user" (no `user_message` UI event). That must still end the turn,
    // or spend accumulates across prompts instead of resetting.
    let dir = tempdir().unwrap();
    let path = dir.path().join("rollout.jsonl");
    fs::write(
        &path,
        format!(
            r#"{{"timestamp":"2026-05-29T16:00:00Z","type":"session_meta","payload":{{"id":"s","cwd":"/repo"}}}}
{{"timestamp":"2026-05-29T16:00:01Z","type":"response_item","payload":{{"type":"message","role":"user"}}}}
{}{{"timestamp":"2026-05-29T16:00:03Z","type":"response_item","payload":{{"type":"message","role":"user"}}}}
{}"#,
            codex_token_row("2026-05-29T16:00:02Z", 100, 100),
            codex_token_row("2026-05-29T16:00:04Z", 200, 300),
        ),
    )
    .unwrap();

    let (events, _) = codex_archived_sessions_since(dir.path(), None).unwrap();
    let boundaries = events
        .iter()
        .filter(|event| matches!(event.kind, EventKind::UserInput))
        .count();
    assert_eq!(boundaries, 2);
    // Spend resets at the second prompt, so only the final checkpoint counts:
    // uncached (100-20) + output 5 + reasoning 2 = 87, not 174.
    assert_eq!(spent_after_last_boundary(&events), 87);
}

#[test]
fn codex_turn_spend_excludes_re_read_cached_context() {
    // A realistic tool-loop turn: three model calls, each re-reading a large
    // cached context. Fresh work each call is tiny (uncached input + output).
    // Turn spend must reflect only the fresh work, never the cached prefix.
    let dir = tempdir().unwrap();
    let path = dir.path().join("rollout.jsonl");
    let row = |at: &str, input: i64, cached: i64, output: i64, total: i64| {
        format!(
            r#"{{"timestamp":"{at}","type":"event_msg","payload":{{"type":"token_count","info":{{"last_token_usage":{{"input_tokens":{input},"cached_input_tokens":{cached},"output_tokens":{output},"reasoning_output_tokens":0,"total_tokens":{total}}},"total_token_usage":{{"total_tokens":{total}}},"model_context_window":258400}}}}}}
"#
        )
    };
    fs::write(
        &path,
        format!(
            r#"{{"timestamp":"2026-05-29T16:00:00Z","type":"session_meta","payload":{{"id":"s","cwd":"/repo"}}}}
{{"timestamp":"2026-05-29T16:00:01Z","type":"event_msg","payload":{{"type":"user_message"}}}}
{}{}{}"#,
            row("2026-05-29T16:00:02Z", 50_000, 49_000, 200, 50_200),
            row("2026-05-29T16:00:03Z", 120_000, 119_000, 300, 120_300),
            row("2026-05-29T16:00:04Z", 260_000, 259_000, 400, 260_400),
        ),
    )
    .unwrap();

    let (events, _) = codex_archived_sessions_since(dir.path(), None).unwrap();
    // Naive sum of per-call totals would be ~430k. True fresh spend is just
    // (1000+200) + (1000+300) + (1000+400) = 3900.
    assert_eq!(spent_after_last_boundary(&events), 3900);
}

#[test]
fn claude_human_text_is_a_boundary_but_tool_results_are_not() {
    let dir = tempdir().unwrap();
    let root = dir.path().join("projects");
    fs::create_dir_all(&root).unwrap();
    let path = root.join("session.jsonl");
    fs::write(
        &path,
        r#"{"timestamp":"2026-05-29T20:00:00Z","type":"user","sessionId":"s","cwd":"/repo","message":{"role":"user","content":"do the thing"}}
{"timestamp":"2026-05-29T20:00:01Z","type":"assistant","sessionId":"s","cwd":"/repo","message":{"id":"m1","model":"claude-opus-4-8","usage":{"input_tokens":10,"cache_creation_input_tokens":20,"cache_read_input_tokens":9999,"output_tokens":30}}}
{"timestamp":"2026-05-29T20:00:02Z","type":"user","sessionId":"s","cwd":"/repo","toolUseResult":{"ok":true},"message":{"role":"user","content":[{"type":"tool_result","content":"x"}]}}
{"timestamp":"2026-05-29T20:00:03Z","type":"assistant","sessionId":"s","cwd":"/repo","message":{"id":"m2","model":"claude-opus-4-8","usage":{"input_tokens":5,"cache_creation_input_tokens":0,"cache_read_input_tokens":9999,"output_tokens":40}}}
"#,
    )
    .unwrap();

    let (events, _) = claude_projects_since(&root, None).unwrap();
    let boundaries = events
        .iter()
        .filter(|event| matches!(event.kind, EventKind::UserInput))
        .count();

    // The typed message is a boundary; the tool_result is not.
    assert_eq!(boundaries, 1);
    // Turn spend excludes cache_read: (10+20+30) + (5+0+40) = 105.
    assert_eq!(spent_after_last_boundary(&events), 105);
}

fn codex_fixture(session_id: &str, cwd: &str, at: &str, total: i64, cumulative: i64) -> String {
    format!(
        r#"{{"timestamp":"{at}","type":"session_meta","payload":{{"id":"{session_id}","cwd":"{cwd}"}}}}
{}"#,
        codex_token_row(at, total, cumulative)
    )
}

fn codex_token_row(at: &str, total: i64, cumulative: i64) -> String {
    format!(
        r#"{{"timestamp":"{at}","type":"event_msg","payload":{{"type":"token_count","info":{{"last_token_usage":{{"input_tokens":100,"cached_input_tokens":20,"output_tokens":5,"reasoning_output_tokens":2,"total_tokens":{total}}},"total_token_usage":{{"total_tokens":{cumulative}}},"model_context_window":258400}}}}}}
"#
    )
}

fn has_event(events: &[Event], total: i64, at: &str) -> bool {
    events.iter().any(|event| {
        event.total_tokens == total
            && event.timestamp
                == DateTime::parse_from_rfc3339(at)
                    .ok()
                    .map(|time| time.with_timezone(&Utc))
    })
}

fn strings_of_length(pattern: &str, len: usize) -> String {
    pattern.chars().cycle().take(len).collect()
}
