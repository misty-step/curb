use std::collections::{BTreeSet, HashMap};
use std::path::PathBuf;
use std::time::Duration as StdDuration;

use anyhow::{Result, bail};
use chrono::{DateTime, Duration, Utc};
use curb_core::usage::{Event, SourceReport};
use serde::Serialize;

pub fn usage_command(home: PathBuf, json: bool, since: String, all: bool) -> Result<()> {
    let since = if all {
        None
    } else {
        let duration =
            curb_core::config::parse_duration_for_cli(&since).map_err(anyhow::Error::msg)?;
        Some(Utc::now() - Duration::from_std(duration)?)
    };
    let scan = curb_core::usage::Reader::new(home).scan_since(since)?;
    if let Some(error) = scan.error {
        bail!("usage scan: {error}");
    }
    let report = UsageReport {
        generated_at: Utc::now(),
        sources: scan.sources,
        sessions: summarize_usage_sessions(&scan.events),
    };
    if json {
        serde_json::to_writer_pretty(std::io::stdout(), &report)?;
        println!();
    } else {
        println!("curb usage");
        println!("  sources: {}", usage_source_line(&report.sources));
        println!("  sessions: {}", report.sessions.len());
        for session in report.sessions.iter().take(12) {
            println!(
                "  {} {} calls={} total={} cwd={}",
                session.provider,
                session.session_id,
                session.events,
                session.total_tokens,
                session
                    .cwd
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "-".to_string())
            );
        }
    }
    Ok(())
}

pub fn tail_command(
    home: PathBuf,
    since: StdDuration,
    interval: StdDuration,
    once: bool,
) -> Result<()> {
    let reader = curb_core::usage::Reader::new(home);
    let mut state = curb_core::tail::TailState::default();
    println!("curb tail");
    if once {
        println!(
            "  scanning usage events from the last {}",
            short_duration(since)
        );
    } else {
        println!(
            "  watching usage events every {}; Ctrl-C to stop",
            short_duration(interval)
        );
    }
    println!();
    loop {
        let now = Utc::now();
        let since_at = now - Duration::from_std(since)?;
        let scan =
            curb_core::tail::scan_once(&reader, &mut state, std::io::stdout(), since_at, now)?;
        if let Some(error) = scan.source_error {
            eprintln!("curb: tail: {error}");
        }
        if once {
            break;
        }
        std::thread::sleep(interval);
    }
    Ok(())
}

fn usage_source_line(sources: &[SourceReport]) -> String {
    sources
        .iter()
        .map(|source| match &source.error {
            Some(_) => format!("{} unavailable", source.provider),
            None => format!("{} {} events", source.provider, source.events),
        })
        .collect::<Vec<_>>()
        .join("; ")
}

#[derive(Serialize)]
struct UsageReport {
    generated_at: DateTime<Utc>,
    sources: Vec<SourceReport>,
    sessions: Vec<UsageSessionSummary>,
}

#[derive(Serialize)]
struct UsageSessionSummary {
    provider: String,
    session_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    cwd: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    last: Option<DateTime<Utc>>,
    events: usize,
    models: Vec<String>,
    input_tokens: i64,
    cached_input_tokens: i64,
    cache_creation_input_tokens: i64,
    output_tokens: i64,
    reasoning_output_tokens: i64,
    total_tokens: i64,
}

fn summarize_usage_sessions(events: &[Event]) -> Vec<UsageSessionSummary> {
    let mut by_key: HashMap<String, UsageSessionSummary> = HashMap::new();
    let mut models: HashMap<String, BTreeSet<String>> = HashMap::new();
    for event in events {
        let session_id = event.session_id.clone().unwrap_or_default();
        let key = if session_id.is_empty() {
            format!("{}:{}", event.provider, event.source_path.display())
        } else {
            format!("{}:{session_id}", event.provider)
        };
        let summary = by_key
            .entry(key.clone())
            .or_insert_with(|| UsageSessionSummary {
                provider: event.provider.clone(),
                session_id: session_id.clone(),
                cwd: event.cwd.clone(),
                last: None,
                events: 0,
                models: Vec::new(),
                input_tokens: 0,
                cached_input_tokens: 0,
                cache_creation_input_tokens: 0,
                output_tokens: 0,
                reasoning_output_tokens: 0,
                total_tokens: 0,
            });
        if event.timestamp > summary.last {
            summary.last = event.timestamp;
        }
        if summary.cwd.is_none() {
            summary.cwd = event.cwd.clone();
        }
        summary.events += 1;
        summary.input_tokens += event.input_tokens;
        summary.cached_input_tokens += event.cached_input_tokens;
        summary.cache_creation_input_tokens += event.cache_creation_input_tokens;
        summary.output_tokens += event.output_tokens;
        summary.reasoning_output_tokens += event.reasoning_output_tokens;
        summary.total_tokens += event.total_tokens;
        if let Some(model) = &event.model {
            models.entry(key).or_default().insert(model.clone());
        }
    }
    for (key, summary) in &mut by_key {
        summary.models = models.remove(key).unwrap_or_default().into_iter().collect();
    }
    let mut out = by_key.into_values().collect::<Vec<_>>();
    out.sort_by_key(|right| std::cmp::Reverse(right.last));
    out
}

fn short_duration(duration: StdDuration) -> String {
    let seconds = duration.as_secs();
    if seconds != 0 && seconds.is_multiple_of(3600) {
        format!("{}h", seconds / 3600)
    } else if seconds != 0 && seconds.is_multiple_of(60) {
        format!("{}m", seconds / 60)
    } else if seconds == 0 && duration.subsec_millis() > 0 {
        format!("{}ms", duration.as_millis())
    } else {
        format!("{seconds}s")
    }
}
