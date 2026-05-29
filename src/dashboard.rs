use std::io::{self, Write};
use std::path::Path;

use chrono::Local;

use crate::config::Config;
use crate::service::{SessionView, Snapshot};

/// Render the terminal dashboard: one status line, one row per agent showing a
/// spend bar against the warn and kill lines, and a quiet footer.
pub fn render(
    mut writer: impl Write,
    path: &Path,
    cfg: &Config,
    snapshot: &Snapshot,
    limit: usize,
) -> io::Result<()> {
    let overview = &snapshot.overview;
    writeln!(writer, "curb · {} · {}", overview.mode, overview.message)?;
    writeln!(writer, "  config: {}", compact_home(path))?;
    writeln!(writer)?;
    render_sessions(&mut writer, cfg, &snapshot.sessions, limit)?;
    render_footer(&mut writer, cfg, snapshot)
}

fn render_sessions(
    writer: &mut impl Write,
    cfg: &Config,
    sessions: &[SessionView],
    limit: usize,
) -> io::Result<()> {
    if sessions.is_empty() {
        writeln!(writer, "  No agent usage found yet.")?;
        return Ok(());
    }
    let warn = cfg.usage.warn_turn_tokens;
    let kill = cfg.usage.kill_turn_tokens;
    let shown = normalized_limit(limit, sessions.len());
    for session in &sessions[..shown] {
        writeln!(
            writer,
            "  {}   {} · {}",
            project_label(session.cwd.as_deref(), session.project.as_deref()),
            session.provider,
            session
                .cwd
                .as_deref()
                .map(compact_home)
                .unwrap_or_else(|| "-".to_string()),
        )?;
        writeln!(
            writer,
            "  {} {:>6}  {}",
            spend_bar(session.turn_tokens, warn, kill),
            short_tokens(session.turn_tokens),
            status_tag(session),
        )?;
    }
    if sessions.len() > shown {
        writeln!(
            writer,
            "  … {} more; use --limit {} or --json",
            sessions.len() - shown,
            sessions.len()
        )?;
    }
    writeln!(writer)
}

fn render_footer(writer: &mut impl Write, cfg: &Config, snapshot: &Snapshot) -> io::Result<()> {
    let running = snapshot.agents.len();
    writeln!(
        writer,
        "  warn {} · kill {} per turn · {} agent{} running · scanned {}",
        short_tokens(cfg.usage.warn_turn_tokens),
        short_tokens(cfg.usage.kill_turn_tokens),
        running,
        if running == 1 { "" } else { "s" },
        snapshot
            .overview
            .last_scan
            .with_timezone(&Local)
            .format("%H:%M:%S"),
    )?;
    let unavailable = snapshot
        .overview
        .sources
        .iter()
        .filter(|source| source.error.is_some())
        .map(|source| source.provider.as_str())
        .collect::<Vec<_>>();
    if !unavailable.is_empty() {
        writeln!(writer, "  sources unavailable: {}", unavailable.join(", "))?;
    }
    Ok(())
}

/// A fixed-width bar filled in proportion to the kill line, so the eye reads
/// "how close to a kill" instantly. The warn line is the lighter shaded zone.
fn spend_bar(turn: i64, warn: i64, kill: i64) -> String {
    const WIDTH: usize = 24;
    let scale = kill.max(warn).max(1) as f64;
    let cell = |tokens: i64| ((tokens as f64 / scale) * WIDTH as f64).round() as usize;
    let filled = cell(turn).min(WIDTH);
    let warn_at = cell(warn).min(WIDTH);
    let mut bar = String::with_capacity(WIDTH);
    for index in 0..WIDTH {
        bar.push(if index < filled {
            '█'
        } else if index < warn_at {
            '·'
        } else {
            '░'
        });
    }
    format!("[{bar}]")
}

/// The one-word status shown beside the bar.
fn status_tag(session: &SessionView) -> &'static str {
    match (session.alert.as_str(), session.status.as_str()) {
        ("kill", _) if session.acknowledged_until.is_some() => "kill · acknowledged",
        ("kill", _) if session.can_stop => "KILL · stop after grace",
        ("kill", _) => "KILL · warn only",
        ("warn", _) => "WARN",
        (_, "working") => "working",
        _ => "idle",
    }
}

fn normalized_limit(limit: usize, len: usize) -> usize {
    if limit == 0 || limit > len {
        len
    } else {
        limit
    }
}

/// Compact token scale: 920k, 2.4M, 3M. Used everywhere a token count is shown.
fn short_tokens(value: i64) -> String {
    if value >= 1_000_000 {
        let millions = format!("{:.1}", value as f64 / 1_000_000.0);
        format!("{}M", millions.strip_suffix(".0").unwrap_or(&millions))
    } else if value >= 1_000 {
        format!("{}k", value / 1_000)
    } else {
        value.to_string()
    }
}

fn compact_home(path: &Path) -> String {
    let rendered = path.display().to_string();
    if let Some(home) = crate::cli::default_home_dir() {
        let home = home.display().to_string();
        if let Some(rest) = rendered.strip_prefix(&home) {
            return format!("~{rest}");
        }
    }
    rendered
}

fn project_label(cwd: Option<&Path>, project: Option<&str>) -> String {
    if let Some(project) = project.filter(|project| !project.is_empty()) {
        return project.to_string();
    }
    cwd.and_then(Path::file_name)
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "-".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_tokens_uses_compact_scale() {
        assert_eq!(short_tokens(920), "920");
        assert_eq!(short_tokens(920_000), "920k");
        assert_eq!(short_tokens(2_400_000), "2.4M");
        assert_eq!(short_tokens(3_000_000), "3M");
    }

    #[test]
    fn spend_bar_fills_toward_the_kill_line() {
        let empty = spend_bar(0, 1_000_000, 3_000_000);
        assert!(!empty.contains('█'));
        let over = spend_bar(3_000_000, 1_000_000, 3_000_000);
        assert_eq!(over.matches('█').count(), 24);
    }
}
