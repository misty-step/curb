use chrono::TimeZone;
use serde_json::{Map, Value};
use std::path::Path;

use super::*;
use crate::config::{Config, HumanDuration};
use crate::usage::Event;

#[test]
fn active_stop_session_is_actionable_only_in_enforcement_mode() {
    let mut cfg = Config::load(crate::config::example_config_path()).unwrap();
    cfg.mode = crate::config::Mode::Enforcement;
    cfg.usage.warn_turn_tokens = 100;
    cfg.usage.kill_turn_tokens = 200;
    let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
    let processes = process_snapshot(now, "codex", "/repo");
    let snapshot = build_snapshot_with_processes(
        &cfg,
        Some(&processes),
        &[event("codex", "s1", now, 250)],
        Vec::new(),
        now,
    );

    assert_eq!(snapshot.overview.status, "ACTION");
    assert_eq!(snapshot.sessions[0].alert, "kill");
    assert!(snapshot.sessions[0].can_stop);
    assert_eq!(snapshot.sessions[0].pid, Some(100));
    assert_eq!(snapshot.sessions[0].project.as_deref(), Some("repo"));
    assert_eq!(snapshot.agents[0].project.as_deref(), Some("repo"));
    assert_eq!(snapshot.agents[0].running_for_seconds, Some(600));
}

#[test]
fn alert_mode_reports_would_stop_without_actionability() {
    let mut cfg = Config::load(crate::config::example_config_path()).unwrap();
    cfg.mode = crate::config::Mode::Alert;
    cfg.usage.warn_turn_tokens = 100;
    cfg.usage.kill_turn_tokens = 200;
    let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
    let processes = process_snapshot(now, "codex", "/repo");
    let snapshot = build_snapshot_with_processes(
        &cfg,
        Some(&processes),
        &[event("codex", "s1", now, 250)],
        Vec::new(),
        now,
    );

    assert_eq!(snapshot.sessions[0].alert, "kill");
    assert!(!snapshot.sessions[0].can_stop);
}

#[test]
fn uncorrelated_stop_usage_is_blocked_not_actionable() {
    let mut cfg = Config::load(crate::config::example_config_path()).unwrap();
    cfg.mode = crate::config::Mode::Enforcement;
    cfg.usage.warn_turn_tokens = 100;
    cfg.usage.kill_turn_tokens = 200;
    let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
    let snapshot = build_snapshot(&cfg, &[event("codex", "s1", now, 250)], Vec::new(), now);

    assert_eq!(snapshot.overview.status, "ACTION");
    assert_eq!(snapshot.sessions[0].alert, "kill");
    assert_eq!(snapshot.sessions[0].pid, None);
    assert!(!snapshot.sessions[0].can_stop);
}

#[test]
fn watch_only_app_match_blocks_termination() {
    let mut cfg = Config::load(crate::config::example_config_path()).unwrap();
    cfg.agents = vec![crate::config::Agent {
        id: "codex-desktop".to_string(),
        label: "Codex Desktop".to_string(),
        family: "codex".to_string(),
        kind: crate::config::AgentKind::App,
        matcher: crate::config::Match {
            process_names: vec!["Codex".to_string()],
            ..Default::default()
        },
        policy: None,
    }];
    cfg.mode = crate::config::Mode::Enforcement;
    cfg.usage.warn_turn_tokens = 100;
    cfg.usage.kill_turn_tokens = 200;
    let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
    let processes = process_snapshot(now, "Codex", "/repo");
    let snapshot = build_snapshot_with_processes(
        &cfg,
        Some(&processes),
        &[event("codex", "s1", now, 250)],
        Vec::new(),
        now,
    );

    assert_eq!(snapshot.sessions[0].alert, "kill");
    assert!(!snapshot.sessions[0].can_stop);
    assert_eq!(snapshot.sessions[0].pid, Some(100));
    assert!(snapshot.sessions[0].explanation.contains("watch-only"));
}

#[test]
fn multiple_sessions_can_correlate_to_one_worker() {
    let mut cfg = Config::load(crate::config::example_config_path()).unwrap();
    cfg.usage.warn_turn_tokens = 100;
    cfg.usage.kill_turn_tokens = 200;
    let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
    let processes = process_snapshot(now, "codex", "/repo");
    let snapshot = build_snapshot_with_processes(
        &cfg,
        Some(&processes),
        &[
            event("codex", "s1", now, 50),
            event("codex", "s2", now - chrono::Duration::minutes(1), 40),
        ],
        Vec::new(),
        now,
    );

    assert_eq!(snapshot.agents.len(), 1);
    assert_eq!(snapshot.sessions.len(), 2);
    assert!(
        snapshot
            .sessions
            .iter()
            .all(|session| session.pid == Some(100))
    );
}

#[test]
fn agent_view_uses_newest_matching_session_when_scores_tie() {
    let mut cfg = Config::load(crate::config::example_config_path()).unwrap();
    cfg.usage.scan_interval = HumanDuration::seconds(5);
    cfg.usage.warn_turn_tokens = 100;
    cfg.usage.kill_turn_tokens = 200;
    let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
    let processes = process_snapshot(now, "codex", "/repo");
    let snapshot = build_snapshot_with_processes(
        &cfg,
        Some(&processes),
        &[
            event("codex", "stale", now - chrono::Duration::minutes(10), 150),
            event("codex", "fresh", now, 125),
        ],
        Vec::new(),
        now,
    );

    assert_eq!(
        snapshot.agents[0].session_key.as_deref(),
        Some("codex:fresh")
    );
    assert_eq!(snapshot.agents[0].status, "working");
}

#[test]
fn overview_delta_reports_new_usage_alerts_agents_and_source_errors() {
    let mut cfg = Config::load(crate::config::example_config_path()).unwrap();
    cfg.usage.warn_turn_tokens = 100;
    cfg.usage.kill_turn_tokens = 200;
    let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
    let previous_processes = platform::Snapshot::new([process(now, 100, "codex", "/repo")]);
    let next_processes = platform::Snapshot::new([process(now, 101, "codex", "/repo")]);
    let previous = build_snapshot_with_processes(
        &cfg,
        Some(&previous_processes),
        &[event("codex", "old", now, 50)],
        vec![
            SourceReport {
                provider: "codex".to_string(),
                files: 1,
                events: 1,
                error: None,
            },
            SourceReport {
                provider: "claude".to_string(),
                files: 1,
                events: 0,
                error: Some("permission denied".to_string()),
            },
        ],
        now,
    );
    let next = build_snapshot_with_processes(
        &cfg,
        Some(&next_processes),
        &[
            event("codex", "old", now, 50),
            event("codex", "old", now + chrono::Duration::seconds(1), 250),
            event("codex", "new", now + chrono::Duration::seconds(2), 25),
        ],
        vec![
            SourceReport {
                provider: "codex".to_string(),
                files: 1,
                events: 3,
                error: Some("schema changed".to_string()),
            },
            SourceReport {
                provider: "claude".to_string(),
                files: 1,
                events: 0,
                error: Some("permission denied".to_string()),
            },
        ],
        now,
    );

    let annotated = annotate_overview_delta(Some(&previous), next);

    assert_eq!(annotated.overview.changes.new_sessions, 1);
    assert_eq!(annotated.overview.changes.sessions_with_new_turns, 2);
    assert_eq!(annotated.overview.changes.tokens_added, 275);
    assert_eq!(annotated.overview.changes.new_alerts, 1);
    assert_eq!(annotated.overview.changes.agents_started, 1);
    assert_eq!(annotated.overview.changes.agents_ended, 1);
    assert_eq!(annotated.overview.changes.source_errors, 1);
}

#[test]
fn overview_exposes_sanitized_source_health_recovery() {
    let cfg = Config::load(crate::config::example_config_path()).unwrap();
    let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
    let snapshot = build_snapshot(
        &cfg,
        &[],
        vec![SourceReport {
            provider: "claude".to_string(),
            files: 0,
            events: 0,
            error: Some(
                "usage scan: usage line exceeds 1048576 bytes: /Users/phaedrus/.claude/private prompt payload"
                    .to_string(),
            ),
        }],
        now,
    );

    let source_error = snapshot.overview.sources[0].error.as_deref().unwrap();
    assert_eq!(
        source_error,
        "usage line exceeded the 1 MiB metadata safety cap"
    );
    assert!(!source_error.contains("/Users/"));
    assert!(!source_error.contains("prompt payload"));

    let recovery = &snapshot.overview.recovery[0];
    assert_eq!(recovery.id, "source-claude");
    assert_eq!(recovery.label, "claude source");
    assert_eq!(recovery.command.as_deref(), Some("curb usage --since 24h"));
    assert!(recovery.message.contains(source_error));
    assert!(!recovery.message.contains("/Users/"));
    assert!(!recovery.message.contains("prompt payload"));
}

#[test]
fn cwd_correlation_uses_path_components_not_string_prefixes() {
    let mut cfg = Config::load(crate::config::example_config_path()).unwrap();
    cfg.usage.warn_turn_tokens = 100;
    cfg.usage.kill_turn_tokens = 200;
    let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
    let processes = platform::Snapshot::new([
        process(now, 100, "codex", "/work/project-other"),
        process(now, 200, "codex", "/work/project/src"),
    ]);
    let snapshot = build_snapshot_with_processes(
        &cfg,
        Some(&processes),
        &[event("codex", "s1", now, 50).with_cwd("/work/project")],
        Vec::new(),
        now,
    );

    assert_eq!(snapshot.sessions[0].pid, Some(200));
}

#[test]
fn cwd_prefix_correlation_rejects_root_or_top_level_paths() {
    let mut cfg = Config::load(crate::config::example_config_path()).unwrap();
    cfg.usage.warn_turn_tokens = 100;
    cfg.usage.kill_turn_tokens = 200;
    let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
    let processes = platform::Snapshot::new([
        process(now, 100, "codex", "/repo/a"),
        process(now, 200, "codex", "/Users/phaedrus/project"),
    ]);
    let snapshot = build_snapshot_with_processes(
        &cfg,
        Some(&processes),
        &[
            event("codex", "root", now, 50).with_cwd("/"),
            event("codex", "top", now, 50).with_cwd("/Users"),
        ],
        Vec::new(),
        now,
    );

    assert_eq!(snapshot.sessions[0].pid, None);
    assert_eq!(snapshot.sessions[1].pid, None);
}

#[test]
fn old_high_usage_is_idle_high_not_active_stop() {
    let mut cfg = Config::load(crate::config::example_config_path()).unwrap();
    cfg.usage.warn_turn_tokens = 100;
    cfg.usage.kill_turn_tokens = 200;
    let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
    let old = now - chrono::Duration::hours(2);
    let snapshot = build_snapshot(&cfg, &[event("codex", "s1", old, 250)], Vec::new(), now);

    assert_eq!(snapshot.overview.status, "OK");
    assert_eq!(snapshot.sessions[0].alert, "ok");
    assert_eq!(snapshot.sessions[0].status, "idle");
}

#[test]
fn terminated_sessions_are_dropped_from_the_snapshot() {
    let mut cfg = Config::load(crate::config::example_config_path()).unwrap();
    cfg.usage.warn_turn_tokens = 100;
    cfg.usage.kill_turn_tokens = 200;
    let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
    let events = [
        event("codex", "s1", now, 250),
        event("codex", "s2", now, 150),
    ];

    let full = build_snapshot_with_processes(&cfg, None, &events, Vec::new(), now);
    assert_eq!(full.sessions.len(), 2);

    let terminated: std::collections::BTreeSet<String> =
        ["codex:s1".to_string()].into_iter().collect();
    let filtered = build_snapshot_filtered(&cfg, None, &events, Vec::new(), now, &terminated);
    assert_eq!(filtered.sessions.len(), 1);
    assert_eq!(filtered.sessions[0].key, "codex:s2");
}

#[test]
fn stale_policy_warning_is_not_fresh_activity() {
    let mut cfg = Config::load(crate::config::example_config_path()).unwrap();
    cfg.usage.scan_interval = HumanDuration::seconds(5);
    cfg.usage.warn_turn_tokens = 100;
    cfg.usage.kill_turn_tokens = 200;
    let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
    let stale_but_in_window = now - chrono::Duration::minutes(3);
    let processes = process_snapshot(now, "codex", "/repo");
    let snapshot = build_snapshot_with_processes(
        &cfg,
        Some(&processes),
        &[event("codex", "s1", stale_but_in_window, 150)],
        Vec::new(),
        now,
    );

    assert_eq!(snapshot.overview.status, "WATCH");
    assert_eq!(snapshot.overview.working, 0);
    assert_eq!(snapshot.sessions[0].alert, "warn");
    assert_eq!(snapshot.sessions[0].status, "idle");
    assert_eq!(snapshot.agents[0].status, "idle");
}

#[test]
fn turn_spend_resets_after_a_user_input_boundary() {
    let mut cfg = Config::load(crate::config::example_config_path()).unwrap();
    cfg.usage.scan_interval = HumanDuration::seconds(5);
    cfg.usage.warn_turn_tokens = 100;
    cfg.usage.kill_turn_tokens = 200;
    let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
    let processes = process_snapshot(now, "codex", "/repo");
    // An expensive past turn (250 > kill), then the human steered again,
    // then a cheap fresh turn (40 < warn). Only the fresh turn counts.
    let events = vec![
        event("codex", "s1", now - chrono::Duration::seconds(20), 250),
        user_input("codex", "s1", now - chrono::Duration::seconds(10)),
        event("codex", "s1", now - chrono::Duration::seconds(2), 40),
    ];
    let snapshot = build_snapshot_with_processes(&cfg, Some(&processes), &events, Vec::new(), now);

    assert_eq!(snapshot.sessions[0].turn_tokens, 40);
    assert_eq!(snapshot.sessions[0].alert, "ok");
}

#[test]
fn process_matching_applies_parent_command_exclusions_to_parent() {
    let mut cfg = Config::load(crate::config::example_config_path()).unwrap();
    cfg.service.min_confidence = 1;
    let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
    let mut parent = process(now, 100, "codex", "/repo");
    parent.command =
        "/Applications/Codex.app/Contents/MacOS/codex app-server --listen stdio://".to_string();
    let mut child = process(now, 101, "claude", "/repo");
    child.ppid = Some(parent.pid);
    child.command = "/usr/local/bin/claude --print worker".to_string();
    let snapshot = platform::Snapshot::new([parent, child]);

    let matches = process_matches(&cfg, &snapshot);

    assert!(!matches.iter().any(|matched| {
        matched.agent.id == "claude-code" && matched.process.pid == platform::Pid::new(101)
    }));
}

#[test]
fn synthetic_shell_worker_requires_portable_process_names_to_clear_confidence_floor() {
    let marker = "curb-e2e-worker-linux-dash";
    let mut cfg = Config::local_default(crate::config::Mode::Enforcement, "/tmp/curb".into());
    cfg.agents = vec![crate::config::Agent {
        id: "e2e-worker".to_string(),
        label: "E2E Worker".to_string(),
        family: "codex".to_string(),
        kind: crate::config::AgentKind::Process,
        matcher: crate::config::Match {
            process_names: vec!["bash".to_string(), "dash".to_string(), "sh".to_string()],
            command_regex: vec![marker.to_string()],
            require_command_regex: vec![marker.to_string()],
            ..Default::default()
        },
        policy: None,
    }];
    cfg.refresh_agent_policies();
    let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
    let mut worker = process(now, 100, "dash", "/repo");
    worker.command = format!("sh -c while :; do sleep 1; done # {marker}");
    let snapshot = platform::Snapshot::new([worker]);

    assert!(
        process_matches(&cfg, &snapshot).iter().any(|matched| {
            matched.agent.id == "e2e-worker" && matched.process.pid == platform::Pid::new(100)
        }),
        "Ubuntu /bin/sh is commonly dash; the marker survives in argv but the old e2e matcher scored only the command regex signal"
    );
}

#[test]
fn project_name_handles_unix_and_windows_paths() {
    assert_eq!(
        project_name(Path::new("/Users/me/repo")).as_deref(),
        Some("repo")
    );
    assert_eq!(
        project_name(Path::new(r"C:\Users\me\repo\")).as_deref(),
        Some("repo")
    );
    assert_eq!(
        project_name(Path::new(r"C:\Users\me/repo")).as_deref(),
        Some("repo")
    );
    assert_eq!(project_name(Path::new("/")).as_deref(), None);
}

#[test]
fn running_for_seconds_clamps_future_and_omits_missing_start() {
    let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
    assert_eq!(
        running_for_seconds(Some(now - chrono::Duration::seconds(42)), now),
        Some(42)
    );
    assert_eq!(
        running_for_seconds(Some(now + chrono::Duration::seconds(42)), now),
        Some(0)
    );
    assert_eq!(running_for_seconds(None, now), None);
}

#[test]
fn event_views_classify_ledger_events_and_apply_limits() {
    let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
    let events = vec![
        ledger_event("service_started", 1, now),
        ledger_event("usage_warning", 2, now + chrono::Duration::seconds(1)),
        ledger_event(
            "usage_would_terminate",
            3,
            now + chrono::Duration::seconds(2),
        ),
        ledger_event(
            "session_ack_received",
            4,
            now + chrono::Duration::seconds(3),
        ),
    ];

    let views = event_views(&events, 3);

    assert_eq!(views.len(), 3);
    assert_eq!(
        (views[0].category.as_str(), views[0].kind.as_str()),
        ("alert", "warning")
    );
    assert_eq!(
        (views[1].category.as_str(), views[1].kind.as_str()),
        ("alert", "would_stop")
    );
    assert_eq!(
        (views[2].category.as_str(), views[2].kind.as_str()),
        ("ack", "received")
    );
    assert_eq!(views[2].message, "Acknowledgement received.");
}

#[test]
fn alert_views_filter_limit_order_and_project_session_actions() {
    let mut cfg = Config::load(crate::config::example_config_path()).unwrap();
    cfg.usage.warn_turn_tokens = 100;
    cfg.usage.kill_turn_tokens = 200;
    let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
    let snapshot = build_snapshot_with_processes(
        &cfg,
        Some(&process_snapshot(now, "codex", "/repo")),
        &[event("codex", "s1", now, 150)],
        Vec::new(),
        now,
    );
    let events = vec![
        ledger_event("run_started", 1, now),
        ledger_event("usage_warning", 2, now + chrono::Duration::seconds(1))
            .with_data(alert_data("codex", "s1", "/repo")),
        ledger_event(
            "usage_would_terminate",
            3,
            now + chrono::Duration::seconds(2),
        )
        .with_message("would stop"),
        ledger_event(
            "usage_termination_completed",
            4,
            now + chrono::Duration::seconds(3),
        ),
    ];

    let alerts = alert_views(&events, Some(&snapshot), 2);

    assert_eq!(alerts.len(), 2);
    assert_eq!(alerts[0].category, "would_stop");
    assert_eq!(alerts[0].severity, "watch");
    assert_eq!(alerts[1].category, "stopped");
    assert_eq!(alerts[1].severity, "stop");
    assert!(alerts[1].actionable);

    let all = alert_views(&events, Some(&snapshot), 0);
    assert_eq!(all.len(), 3);
    assert_eq!(all[0].session_key.as_deref(), Some("codex:s1"));
    assert!(all[0].can_acknowledge);
    assert_eq!(all[0].cwd.as_deref(), Some("/repo"));
}

#[test]
fn alert_views_do_not_ack_missing_or_already_acknowledged_sessions() {
    let mut cfg = Config::load(crate::config::example_config_path()).unwrap();
    cfg.service.state_dir = tempfile::tempdir().unwrap().keep();
    cfg.ledger.path = cfg.service.state_dir.join("runs.ndjson");
    cfg.usage.warn_turn_tokens = 100;
    cfg.usage.kill_turn_tokens = 200;
    let now = Utc.with_ymd_and_hms(2026, 5, 28, 16, 0, 0).unwrap();
    crate::write_path::write_session_ack(
        &cfg.service.state_dir,
        "codex:s1",
        std::time::Duration::from_secs(60),
        "handled",
        now,
    )
    .unwrap();
    let snapshot = build_snapshot_with_processes(
        &cfg,
        Some(&process_snapshot(now, "codex", "/repo")),
        &[event("codex", "s1", now, 150)],
        Vec::new(),
        now,
    );
    let events = vec![
        ledger_event("usage_warning", 1, now).with_data(alert_data("codex", "s1", "/repo")),
        ledger_event("usage_warning", 2, now).with_data(alert_data("codex", "missing", "/repo")),
    ];

    let alerts = alert_views(&events, Some(&snapshot), 0);

    assert_eq!(alerts.len(), 2);
    assert_eq!(alerts[0].session_key.as_deref(), Some("codex:s1"));
    assert!(!alerts[0].can_acknowledge);
    assert_eq!(alerts[1].session_key, None);
    assert!(!alerts[1].can_acknowledge);
}

fn event(provider: &str, session: &str, at: DateTime<Utc>, total: i64) -> Event {
    Event {
        kind: crate::usage::EventKind::TokenCheckpoint,
        provider: provider.to_string(),
        source: "test".to_string(),
        source_path: "fixture.jsonl".into(),
        session_id: Some(session.to_string()),
        turn_id: None,
        request_id: None,
        model: Some("model".to_string()),
        cwd: Some("/repo".into()),
        timestamp: Some(at),
        input_tokens: total,
        cached_input_tokens: 0,
        cache_creation_input_tokens: 0,
        output_tokens: 0,
        reasoning_output_tokens: 0,
        total_tokens: total,
        spent_tokens: total,
        cumulative_tokens: total,
        model_context_window: 0,
    }
}

fn user_input(provider: &str, session: &str, at: DateTime<Utc>) -> Event {
    Event {
        kind: crate::usage::EventKind::UserInput,
        provider: provider.to_string(),
        source: "test".to_string(),
        source_path: "fixture.jsonl".into(),
        session_id: Some(session.to_string()),
        turn_id: None,
        request_id: None,
        model: None,
        cwd: Some("/repo".into()),
        timestamp: Some(at),
        input_tokens: 0,
        cached_input_tokens: 0,
        cache_creation_input_tokens: 0,
        output_tokens: 0,
        reasoning_output_tokens: 0,
        total_tokens: 0,
        spent_tokens: 0,
        cumulative_tokens: 0,
        model_context_window: 0,
    }
}

fn ledger_event(event_type: &str, seq: i64, at: DateTime<Utc>) -> ledger::Event {
    ledger::Event {
        event_type: event_type.to_string(),
        seq,
        ts: at,
        run_id: None,
        agent_id: Some("codex-cli".to_string()),
        mode: Some("alert".to_string()),
        message: None,
        data: None,
        prev_hash: None,
        event_hash: None,
    }
}

fn alert_data(provider: &str, session_id: &str, cwd: &str) -> Map<String, Value> {
    let mut data = Map::new();
    data.insert("provider".to_string(), Value::String(provider.to_string()));
    data.insert(
        "session_id".to_string(),
        Value::String(session_id.to_string()),
    );
    data.insert("cwd".to_string(), Value::String(cwd.to_string()));
    data
}

fn process_snapshot(now: DateTime<Utc>, name: &str, cwd: &str) -> platform::Snapshot {
    platform::Snapshot::new([process(now, 100, name, cwd)])
}

fn process(now: DateTime<Utc>, pid: i32, name: &str, cwd: &str) -> platform::Process {
    platform::Process {
        pid: platform::Pid::new(pid),
        ppid: None,
        name: name.to_string(),
        executable: Some(PathBuf::from("/usr/local/bin/codex")),
        command: name.to_string(),
        cwd: Some(PathBuf::from(cwd)),
        started_at: Some(now - chrono::Duration::minutes(10)),
        username: Some("tester".to_string()),
        bundle_id: None,
        team_id: None,
    }
}

trait EventTestExt {
    fn with_cwd(self, cwd: &str) -> Self;
}

impl EventTestExt for Event {
    fn with_cwd(mut self, cwd: &str) -> Self {
        self.cwd = Some(PathBuf::from(cwd));
        self
    }
}
