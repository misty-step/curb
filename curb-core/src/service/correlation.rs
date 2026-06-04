use std::path::{Path, PathBuf};

use regex::Regex;

use super::Session;
use crate::config::{Agent, Config};
use crate::platform;

#[derive(Clone, Debug)]
pub(crate) struct ProcessMatch {
    pub(crate) agent: Agent,
    pub(crate) process: platform::Process,
    confidence: i64,
}

#[derive(Clone, Debug, Default)]
pub(crate) struct Correlation {
    pub(crate) matched: bool,
    pub(crate) agent: Option<Agent>,
    pub(crate) process: Option<platform::Process>,
    pub(crate) score: i64,
    pub(crate) reason: String,
}

pub(crate) fn process_matches(cfg: &Config, snapshot: &platform::Snapshot) -> Vec<ProcessMatch> {
    let mut matches = Vec::new();
    for process in snapshot.processes() {
        for agent in &cfg.agents {
            let confidence = match_agent(agent, process, snapshot);
            if confidence >= cfg.service.min_confidence {
                matches.push(ProcessMatch {
                    agent: agent.clone(),
                    process: process.clone(),
                    confidence,
                });
            }
        }
    }
    matches.sort_by(|left, right| {
        right
            .confidence
            .cmp(&left.confidence)
            .then_with(|| left.process.pid.get().cmp(&right.process.pid.get()))
    });
    matches
}

/// Score how strongly a process looks like a configured agent. Exclusion and
/// require filters veto a match (score 0); positive signals add confidence.
fn match_agent(agent: &Agent, process: &platform::Process, snapshot: &platform::Snapshot) -> i64 {
    let matcher = &agent.matcher;
    let excluded = matcher
        .exclude_names
        .iter()
        .any(|name| name.eq_ignore_ascii_case(&process.name))
        || any_regex_matches(&matcher.exclude_command_regex, &process.command)
        || process
            .ppid
            .and_then(|pid| snapshot.process(pid))
            .is_some_and(|parent| {
                any_regex_matches(&matcher.exclude_parent_regex, &parent.command)
            });
    let missing_required = !matcher.require_command_regex.is_empty()
        && !matcher
            .require_command_regex
            .iter()
            .all(|pattern| regex_matches(pattern, &process.command));
    if excluded || missing_required {
        return 0;
    }

    let mut confidence = 0;
    if matcher
        .process_names
        .iter()
        .any(|name| name.eq_ignore_ascii_case(&process.name))
    {
        confidence += 50;
    }
    if any_regex_matches(&matcher.command_regex, &process.command) {
        confidence += 40;
    }
    if process.executable.as_ref().is_some_and(|executable| {
        matcher
            .executable_paths
            .iter()
            .any(|path| Path::new(path) == executable.as_path())
    }) {
        confidence += 80;
    }
    if process.bundle_id.as_ref().is_some_and(|bundle_id| {
        matcher
            .bundle_ids
            .iter()
            .any(|expected| expected == bundle_id)
    }) {
        confidence += 80;
    }
    confidence
}

fn any_regex_matches(patterns: &[String], value: &str) -> bool {
    patterns.iter().any(|pattern| regex_matches(pattern, value))
}

fn regex_matches(pattern: &str, value: &str) -> bool {
    Regex::new(pattern)
        .map(|regex| regex.is_match(value))
        .unwrap_or(false)
}

pub(crate) fn correlate(session: &Session, matches: &[ProcessMatch]) -> Correlation {
    let Some(session_cwd) = clean_path(session.cwd.as_ref()) else {
        return Correlation::default();
    };
    let mut best = Correlation::default();
    for matched in matches {
        if !same_provider(&session.provider, &matched.agent.family) {
            continue;
        }
        let Some(process_cwd) = clean_path(matched.process.cwd.as_ref()) else {
            continue;
        };
        let (score, reason) = if process_cwd == session_cwd {
            (125, "provider+cwd")
        } else if safe_cwd_prefix_match(&process_cwd, &session_cwd)
            || safe_cwd_prefix_match(&session_cwd, &process_cwd)
        {
            (75, "provider+cwd-prefix")
        } else {
            continue;
        };
        if score > best.score {
            best = Correlation {
                matched: true,
                agent: Some(matched.agent.clone()),
                process: Some(matched.process.clone()),
                score,
                reason: reason.to_string(),
            };
        }
    }
    best
}

pub(crate) fn best_session_for_match<'a>(
    matched: &ProcessMatch,
    sessions: &'a [Session],
) -> Option<&'a Session> {
    sessions
        .iter()
        .filter_map(|session| {
            let correlation = correlate(session, std::slice::from_ref(matched));
            correlation.matched.then_some((
                correlation.score,
                session.last_usage,
                session.last,
                session,
            ))
        })
        .max_by_key(|(score, last_usage, last, _)| (*score, *last_usage, *last))
        .map(|(_, _, _, session)| session)
}

fn same_provider(left: &str, right: &str) -> bool {
    left.eq_ignore_ascii_case(right)
}

fn clean_path(path: Option<&PathBuf>) -> Option<PathBuf> {
    let path = path?;
    if path.as_os_str().is_empty() {
        None
    } else {
        Some(path.components().collect())
    }
}

/// Correlate by working directory only when both paths are specific enough that
/// one containing the other is meaningful — never `/` or `/Users`.
fn safe_cwd_prefix_match(parent: &Path, child: &Path) -> bool {
    path_specificity(parent) >= 2 && path_specificity(child) >= 2 && child.starts_with(parent)
}

fn path_specificity(path: &Path) -> usize {
    path.components()
        .filter(|component| matches!(component, std::path::Component::Normal(_)))
        .count()
}
