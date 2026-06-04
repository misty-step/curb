use std::env;
use std::path::PathBuf;

use super::{Agent, AgentKind, Match};

/// The user's home directory, derived from the environment.
///
/// Prefers `HOME` (Unix) and falls back to `USERPROFILE` (Windows). Returns
/// `None` when neither is set. Used by path-compaction rendering and by the
/// binary's CLI to resolve default config/state locations.
pub fn default_home_dir() -> Option<PathBuf> {
    env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("USERPROFILE").map(PathBuf::from))
}

pub(super) fn default_state_dir() -> PathBuf {
    if let Ok(xdg) = env::var("XDG_STATE_HOME") {
        return PathBuf::from(xdg).join("curb");
    }
    if let Ok(local) = env::var("LOCALAPPDATA") {
        return PathBuf::from(local).join("Curb");
    }
    if let Ok(home) = env::var("HOME") {
        return PathBuf::from(home)
            .join(".local")
            .join("state")
            .join("curb");
    }
    PathBuf::from(".curb")
}

pub(super) fn default_process_agents() -> Vec<Agent> {
    vec![
        Agent {
            id: "codex-desktop-worker".to_string(),
            label: "Codex Desktop Worker".to_string(),
            family: "codex".to_string(),
            kind: AgentKind::Process,
            matcher: Match {
                process_names: vec!["codex".to_string()],
                require_command_regex: vec![
                    "\\bapp-server\\b".to_string(),
                    "--listen\\s+stdio://".to_string(),
                ],
                command_regex: vec![
                    "\\bapp-server\\b".to_string(),
                    "--listen\\s+stdio://".to_string(),
                ],
                ..Match::default()
            },
            policy: None,
        },
        Agent {
            id: "codex-cli".to_string(),
            label: "Codex CLI".to_string(),
            family: "codex".to_string(),
            kind: AgentKind::Process,
            matcher: Match {
                process_names: vec!["codex".to_string()],
                command_regex: vec!["(^|/|\\\\)codex(\\.js|\\.cmd|\\.exe)?(\\s|$)".to_string()],
                exclude_command_regex: vec!["/Applications/Codex.app".to_string()],
                ..Match::default()
            },
            policy: None,
        },
        Agent {
            id: "claude-code".to_string(),
            label: "Claude Code".to_string(),
            family: "claude".to_string(),
            kind: AgentKind::Process,
            matcher: Match {
                process_names: vec!["claude".to_string(), "claude-code".to_string()],
                command_regex: vec!["(^|/|\\\\)claude(-code)?(\\.cmd|\\.exe)?(\\s|$)".to_string()],
                exclude_command_regex: vec!["/Applications/Claude.app".to_string()],
                exclude_parent_regex: vec![
                    "/Applications/Codex\\.app/.+\\bapp-server\\b".to_string(),
                ],
                ..Match::default()
            },
            policy: None,
        },
        Agent {
            id: "antigravity-cli".to_string(),
            label: "Anti-Gravity CLI".to_string(),
            family: "antigravity".to_string(),
            kind: AgentKind::Process,
            matcher: Match {
                process_names: vec!["agy".to_string()],
                command_regex: vec!["(^|/|\\\\)agy(\\.cmd|\\.exe)?(\\s|$)".to_string()],
                ..Match::default()
            },
            policy: None,
        },
    ]
}
