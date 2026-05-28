use std::io::{self, Write};
use std::path::Path;

use chrono::{DateTime, Local, Utc};

use crate::config::Config;
use crate::service::{AgentView, SessionView, Snapshot};

pub fn render(
    mut writer: impl Write,
    path: &Path,
    cfg: &Config,
    snapshot: &Snapshot,
    limit: usize,
) -> io::Result<()> {
    writeln!(writer, "curb dashboard")?;
    writeln!(writer, "  config: {}", compact_home(path))?;
    writeln!(writer, "  action: {}", action_label(&cfg.mode.to_string()))?;
    writeln!(writer)?;
    render_header(&mut writer, snapshot, cfg)?;
    render_attention(&mut writer, snapshot)?;
    render_agents(&mut writer, &snapshot.agents)?;
    writeln!(writer)?;
    render_sessions(&mut writer, &snapshot.sessions, limit)
}

fn render_header(writer: &mut impl Write, snapshot: &Snapshot, cfg: &Config) -> io::Result<()> {
    let overview = &snapshot.overview;
    writeln!(
        writer,
        "  status: {} - {}",
        overview.status, overview.message
    )?;
    writeln!(
        writer,
        "  window tokens: {}; lookback tokens: {}; live agents: {}; active sessions: {}",
        token_count(overview.window_tokens),
        token_count(overview.lookback_tokens),
        overview.active_agents,
        overview.active_sessions
    )?;
    if cfg.usage.enabled() {
        writeln!(
            writer,
            "  policy: warn {}/turn; stop {}/turn",
            token_count(cfg.usage.warn_turn_tokens),
            token_count(cfg.usage.kill_turn_tokens)
        )?;
    } else {
        writeln!(writer, "  policy: usage monitoring disabled")?;
    }
    writeln!(
        writer,
        "  scanned: {}",
        overview
            .last_scan
            .with_timezone(&Local)
            .format("%Y-%m-%d %H:%M:%S")
    )?;
    let sources = overview
        .sources
        .iter()
        .map(|source| match &source.error {
            Some(_) => format!("{} unavailable", source.provider),
            None => format!("{} {} events", source.provider, source.events),
        })
        .collect::<Vec<_>>();
    if !sources.is_empty() {
        writeln!(writer, "  sources: {}", sources.join("; "))?;
    }
    writeln!(writer)
}

fn render_attention(writer: &mut impl Write, snapshot: &Snapshot) -> io::Result<()> {
    writeln!(writer, "attention")?;
    let overview = &snapshot.overview;
    if overview.stop_sessions > 0 {
        writeln!(
            writer,
            "  {} actionable session(s) are over stop thresholds. Curb can stop correlated workers after grace in enforcement mode.",
            overview.stop_sessions
        )?;
    } else if overview.warning_sessions > 0 {
        writeln!(
            writer,
            "  {} session(s) need attention. Check usage state, correlation, and mode before enabling enforcement.",
            overview.warning_sessions
        )?;
    } else {
        writeln!(
            writer,
            "  none. Historical high-turn sessions are visible below, but idle sessions are not treated as runaway spend."
        )?;
    }
    if overview.idle_high_sessions > 0 {
        writeln!(
            writer,
            "  note: {} large historical turn session(s) are idle-high, meaning expensive but not currently spending.",
            overview.idle_high_sessions
        )?;
    }
    writeln!(writer)
}

fn render_agents(writer: &mut impl Write, agents: &[AgentView]) -> io::Result<()> {
    writeln!(writer, "live agents: {}", agents.len())?;
    if agents.is_empty() {
        writeln!(writer, "  none matched")?;
        return Ok(());
    }
    writeln!(
        writer,
        "  {:<7} {:<22} {:<12} {:<12} {:<12} PROJECT",
        "PID", "AGENT", "STATE", "USAGE", "LATEST_TURN"
    )?;
    for agent in agents {
        writeln!(
            writer,
            "  {:<7} {:<22} {:<12} {:<12} {:<12} {}",
            agent.pid,
            agent.id,
            agent.state,
            blank_as_dash(&agent.usage_state),
            token_or_dash(agent.latest_turn_tokens),
            project_label(agent.cwd.as_deref(), agent.project.as_deref())
        )?;
    }
    Ok(())
}

fn render_sessions(
    writer: &mut impl Write,
    sessions: &[SessionView],
    limit: usize,
) -> io::Result<()> {
    writeln!(writer, "sessions")?;
    if sessions.is_empty() {
        writeln!(writer, "  no local usage events found")?;
        return Ok(());
    }
    let limit = normalized_limit(limit, sessions.len());
    writeln!(
        writer,
        "  {:<13} {:<8} {:<8} {:<12} {:<10} {:<7} {:<18} WHY",
        "STATUS", "AGENT", "LAST", "LATEST_TURN", "TOTAL", "CALLS", "PROJECT"
    )?;
    for session in &sessions[..limit] {
        writeln!(
            writer,
            "  {:<13} {:<8} {:<8} {:<12} {:<10} {:<7} {:<18} {}",
            session_status(session),
            session.provider,
            relative_time(session_display_time(session)),
            token_or_dash(session.latest_turn_tokens),
            token_count(session.total_tokens),
            session.calls,
            project_label(session.cwd.as_deref(), session.project.as_deref()),
            session.explanation
        )?;
        if !session.models.is_empty() {
            writeln!(writer, "    models: {}", session.models.join(", "))?;
        }
        writeln!(
            writer,
            "    path: {}  session: {}  process: {}",
            session
                .cwd
                .as_deref()
                .map(compact_home)
                .unwrap_or_else(|| "-".to_string()),
            short_session_id(&session.id),
            session_process(session)
        )?;
    }
    if sessions.len() > limit {
        writeln!(
            writer,
            "\nshowing {} of {} sessions; use --limit {} or --json for more",
            limit,
            sessions.len(),
            sessions.len()
        )?;
    }
    Ok(())
}

fn normalized_limit(limit: usize, len: usize) -> usize {
    if limit == 0 || limit > len {
        len
    } else {
        limit
    }
}

fn action_label(mode: &str) -> &'static str {
    match mode {
        "visibility" => "record only",
        "alert" => "notify only",
        "enforcement" => "warn and stop correlated workers",
        _ => "not configured",
    }
}

fn session_status(session: &SessionView) -> String {
    if !session.usage_state.is_empty() && session.usage_state != session.state {
        format!("{}/{}", session.state, session.usage_state)
    } else {
        session.state.clone()
    }
}

fn session_display_time(session: &SessionView) -> DateTime<Utc> {
    session.last_usage_at.unwrap_or(session.last_seen_at)
}

fn session_process(session: &SessionView) -> String {
    match (
        session.correlated_pid,
        session.correlation_reason.as_deref(),
    ) {
        (Some(pid), Some(reason)) => format!("pid {pid} via {reason}"),
        (Some(pid), None) => format!("pid {pid}"),
        _ => "uncorrelated".to_string(),
    }
}

fn token_count(value: i64) -> String {
    if value >= 1_000_000 {
        let millions = value as f64 / 1_000_000.0;
        let rendered = format!("{millions:.1}");
        format!("{}M tokens", rendered.trim_end_matches(".0"))
    } else if value >= 10_000 {
        format!("{}k tokens", value / 1_000)
    } else {
        format!("{value} tokens")
    }
}

fn token_or_dash(value: i64) -> String {
    if value > 0 {
        token_count(value)
    } else {
        "-".to_string()
    }
}

fn relative_time(at: DateTime<Utc>) -> String {
    let elapsed = Utc::now().signed_duration_since(at);
    if elapsed.num_seconds() < 60 {
        "now".to_string()
    } else if elapsed.num_minutes() < 60 {
        format!("{}m ago", elapsed.num_minutes())
    } else if elapsed.num_hours() < 24 {
        format!("{}h ago", elapsed.num_hours())
    } else {
        format!("{}d ago", elapsed.num_days())
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

fn blank_as_dash(value: &str) -> &str {
    if value.is_empty() { "-" } else { value }
}

fn project_label(cwd: Option<&Path>, project: Option<&str>) -> String {
    if let Some(project) = project.filter(|project| !project.is_empty()) {
        return project.to_string();
    }
    cwd.and_then(Path::file_name)
        .map(|name| name.to_string_lossy().to_string())
        .unwrap_or_else(|| "-".to_string())
}

fn short_session_id(id: &str) -> String {
    if id.len() <= 18 {
        id.to_string()
    } else {
        format!("{}...{}", &id[..8], &id[id.len() - 6..])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_count_uses_dashboard_scale() {
        assert_eq!(token_count(9999), "9999 tokens");
        assert_eq!(token_count(10_000), "10k tokens");
        assert_eq!(token_count(1_500_000), "1.5M tokens");
        assert_eq!(token_count(3_000_000), "3M tokens");
    }
}
