use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration as StdDuration;

use anyhow::{Context, Result, bail};
use chrono::Utc;

use crate::config::{Config, Mode, Preset};
use crate::ledger::{Event, Ledger};
use crate::platform::SystemPlatform;
use crate::platform::{NotificationCapability, PlatformError};
use crate::runtime::Runtime;
use crate::service::{AckRequest, SessionView};

pub fn init_config(path: PathBuf, force: bool, mode: &str) -> Result<()> {
    let mode = Mode::from_str(mode).map_err(anyhow::Error::msg)?;
    if path.exists() && !force {
        println!("config already exists: {}", path.display());
        println!("next: curb app");
        return Ok(());
    }
    let cfg = Config::local_default(mode, state_dir_for_config(&path));
    cfg.save(&path)?;
    println!("created config: {}", path.display());
    println!("next: curb app");
    Ok(())
}

pub fn install_binary(prefix: Option<PathBuf>) -> Result<()> {
    let prefix = prefix.unwrap_or_else(default_install_prefix);
    let source = std::env::current_exe().context("find current executable")?;
    let dest_dir = prefix.join("bin");
    fs::create_dir_all(&dest_dir).with_context(|| format!("create {}", dest_dir.display()))?;
    let dest = dest_dir.join(if cfg!(windows) { "curb.exe" } else { "curb" });
    fs::copy(&source, &dest)
        .with_context(|| format!("copy {} to {}", source.display(), dest.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&dest, fs::Permissions::from_mode(0o755))
            .with_context(|| format!("chmod {}", dest.display()))?;
    }
    println!("installed: {}", dest.display());
    println!("next: add {} to PATH if needed", dest_dir.display());
    Ok(())
}

pub fn config_command(action: Option<String>) -> Result<()> {
    let path = default_config_path();
    match action.as_deref() {
        Some("path") => {
            println!("{}", path.display());
            Ok(())
        }
        Some("aggressive" | "reasonable" | "observe") => {
            let mut cfg = load_or_default_config(&path)?;
            let preset =
                Preset::from_str(action.as_deref().unwrap()).map_err(anyhow::Error::msg)?;
            cfg.apply_preset(preset);
            cfg.save(&path)?;
            print_config_summary(&path, &cfg);
            Ok(())
        }
        Some("show") | None => {
            let cfg = load_or_default_config(&path)?;
            print_config_summary(&path, &cfg);
            Ok(())
        }
        Some(other) => bail!("unknown config command {other:?}"),
    }
}

pub fn dashboard_command(
    config_path: PathBuf,
    home: PathBuf,
    limit: usize,
    json: bool,
) -> Result<()> {
    let cfg = Config::load(&config_path)?;
    let runtime =
        Arc::new(Runtime::new(cfg.clone(), home, SystemPlatform).with_config_path(&config_path));
    let snapshot = runtime.rescan(Utc::now()).map_err(anyhow::Error::msg)?;
    if json {
        serde_json::to_writer_pretty(std::io::stdout(), &snapshot)?;
        println!();
        return Ok(());
    }
    crate::dashboard::render(std::io::stdout(), &config_path, &cfg, &snapshot, limit)?;
    Ok(())
}

pub fn status_command(config_path: PathBuf, home: PathBuf, json: bool) -> Result<()> {
    let cfg = Config::load(&config_path)?;
    let runtime = Runtime::new(cfg.clone(), home, SystemPlatform).with_config_path(&config_path);
    let snapshot = runtime.rescan(Utc::now()).map_err(anyhow::Error::msg)?;
    if json {
        serde_json::to_writer_pretty(std::io::stdout(), &snapshot.overview)?;
        println!();
        return Ok(());
    }
    println!("curb status");
    println!("  config: {}", compact_home(&config_path));
    println!(
        "  status: {} - {}",
        snapshot.overview.status, snapshot.overview.message
    );
    println!(
        "  sessions: {} active, {} warning, {} stop, {} idle-high",
        snapshot.overview.active_sessions,
        snapshot.overview.warning_sessions,
        snapshot.overview.stop_sessions,
        snapshot.overview.idle_high_sessions
    );
    println!(
        "  usage: {} in window, {} lookback",
        token_count(snapshot.overview.window_tokens),
        token_count(snapshot.overview.lookback_tokens)
    );
    println!("  action: {}", snapshot.overview.action);
    println!("  ledger: {}", compact_home(&cfg.ledger.path));
    let attention = attention_sessions(&snapshot.sessions);
    if !attention.is_empty() {
        println!();
        print_session_table("attention", attention.iter().copied(), 5);
    }
    Ok(())
}

pub fn runs_command(
    config_path: PathBuf,
    home: PathBuf,
    active_only: bool,
    state: &str,
    provider: Option<&str>,
    json: bool,
    limit: usize,
) -> Result<()> {
    let cfg = Config::load(&config_path)?;
    let runtime = Runtime::new(cfg, home, SystemPlatform).with_config_path(&config_path);
    let snapshot = runtime.rescan(Utc::now()).map_err(anyhow::Error::msg)?;
    let mut sessions = snapshot.sessions;
    if active_only {
        sessions.retain(|session| {
            matches!(session.usage_state.as_str(), "spending" | "warn" | "stop")
                || session.process_state == "running"
        });
    }
    if let Some(provider) = provider {
        sessions.retain(|session| session.provider == provider);
    }
    let state = state.to_ascii_lowercase();
    if state != "all" {
        sessions.retain(|session| session_matches_state(session, &state));
    }
    if json {
        serde_json::to_writer_pretty(std::io::stdout(), &sessions)?;
        println!();
        return Ok(());
    }
    println!("curb runs");
    if sessions.is_empty() {
        println!("  no sessions");
        return Ok(());
    }
    print_session_table("sessions", sessions.iter(), limit);
    Ok(())
}

pub fn ack_command(
    config_path: PathBuf,
    home: PathBuf,
    key: String,
    extend: &str,
    reason: String,
) -> Result<()> {
    let cfg = Config::load(&config_path)?;
    let runtime = Runtime::new(cfg, home, SystemPlatform).with_config_path(&config_path);
    let extend_seconds = crate::config::parse_duration_for_cli(extend)
        .map_err(anyhow::Error::msg)?
        .as_secs() as i64;
    let ack = runtime
        .acknowledge_session(
            &key,
            AckRequest {
                extend_seconds,
                reason,
            },
            Utc::now(),
        )
        .map_err(anyhow::Error::msg)?;
    println!("acknowledged {}", ack.session_key);
    println!(
        "  extended: {}",
        short_duration(StdDuration::from_secs(ack.extend_seconds as u64))
    );
    println!(
        "  until: {}",
        ack.until
            .with_timezone(&chrono::Local)
            .format("%Y-%m-%d %H:%M:%S")
    );
    if !ack.reason.is_empty() {
        println!("  reason: {}", ack.reason);
    }
    Ok(())
}

pub trait DoctorPlatform {
    fn capture_processes(&self) -> Result<usize, PlatformError>;
    fn notification_capability(&self) -> NotificationCapability;
    fn notify(&self, title: &str, body: &str) -> Result<(), PlatformError>;
    fn platform_name(&self) -> &'static str;
}

impl DoctorPlatform for SystemPlatform {
    fn capture_processes(&self) -> Result<usize, PlatformError> {
        Ok(<Self as crate::platform::Platform>::capture(self)?
            .processes()
            .count())
    }

    fn notification_capability(&self) -> NotificationCapability {
        <Self as crate::platform::Platform>::notification_capability(self)
    }

    fn notify(&self, title: &str, body: &str) -> Result<(), PlatformError> {
        <Self as crate::platform::Platform>::notify(self, title, body)
    }

    fn platform_name(&self) -> &'static str {
        std::env::consts::OS
    }
}

pub fn doctor_command(config_path: PathBuf, test_notification: bool) -> Result<()> {
    doctor_with_platform(config_path, test_notification, &SystemPlatform)
}

pub fn doctor_with_platform(
    config_path: PathBuf,
    test_notification: bool,
    platform: &impl DoctorPlatform,
) -> Result<()> {
    let cfg = Config::load(&config_path)?;
    println!("config: ok {}", config_path.display());
    fs::create_dir_all(&cfg.service.state_dir)
        .with_context(|| format!("create state dir {}", cfg.service.state_dir.display()))?;
    set_private_dir(&cfg.service.state_dir)?;
    println!("state_dir: ok {}", cfg.service.state_dir.display());

    let ledger = Ledger::open(&cfg.ledger.path)?;
    ledger.append(
        Event::new("doctor")
            .with_message("ledger write check")
            .with_mode(cfg.mode.to_string()),
    )?;
    println!("ledger: ok {}", cfg.ledger.path.display());

    let processes = platform.capture_processes()?;
    println!(
        "process_snapshot: ok processes={} platform={}",
        processes,
        platform.platform_name()
    );

    let capability = platform.notification_capability();
    if !capability.supported {
        println!("notifications: unavailable {}", capability.message);
        return Ok(());
    }
    if test_notification {
        match platform.notify("Curb doctor", "Notification check") {
            Ok(()) => println!("notifications: ok"),
            Err(error) => println!("notifications: unavailable {error}"),
        }
    } else {
        println!(
            "notifications: {} {}",
            capability.status, capability.message
        );
    }
    Ok(())
}

pub fn load_or_default_config(path: &Path) -> Result<Config> {
    if path.exists() {
        return Config::load(path).map_err(anyhow::Error::from);
    }
    let cfg = Config::local_default(Mode::Visibility, state_dir_for_config(path));
    cfg.save(path)?;
    Ok(cfg)
}

fn set_private_dir(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .with_context(|| format!("chmod {}", path.display()))?;
    }
    Ok(())
}

pub fn default_config_path() -> PathBuf {
    if let Some(path) = std::env::var_os("CURB_CONFIG") {
        return PathBuf::from(path);
    }
    let local = PathBuf::from("curb.yaml");
    if local.exists() {
        return local;
    }
    user_config_path().unwrap_or_else(|| PathBuf::from("configs/curb.example.yaml"))
}

pub fn default_home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
}

fn print_config_summary(path: &Path, cfg: &Config) {
    println!("curb config");
    println!("  path: {}", compact_home(path));
    println!("  mode: {}", cfg.mode);
    println!("  action: {}", action_label(cfg.mode));
    if cfg.mode == Mode::Enforcement {
        println!("  safety: enforcement can stop correlated agent workers after grace");
    } else {
        println!("  safety: notify only; Curb will not stop processes in this mode");
    }
    println!();
    println!("usage policy");
    if cfg.usage.enabled() {
        println!(
            "  warn: {} per turn",
            token_count(cfg.usage.warn_turn_tokens)
        );
        println!(
            "  stop: {} per turn",
            token_count(cfg.usage.kill_turn_tokens)
        );
        println!(
            "  scan: every {}; grace {}",
            short_duration(cfg.usage.scan_interval.as_std()),
            short_duration(cfg.usage.grace_period.as_std())
        );
    } else {
        println!("  disabled");
    }
    println!(
        "  export: {}",
        if cfg.ledger.forward_url.is_empty() {
            "local ledger only".to_string()
        } else {
            format!("forwarding ledger events to {}", cfg.ledger.forward_url)
        }
    );
    println!();
    println!("watched agents");
    println!(
        "  {}",
        cfg.agents
            .iter()
            .map(|agent| agent.label.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );
    println!();
    println!("next");
    println!("  curb app                 open live dashboard");
    println!("  curb watch               run warning/enforcement loop");
    println!("  curb config reasonable   return to notify-only defaults");
    println!("  curb config aggressive   local enforcement test thresholds");
}

fn action_label(mode: Mode) -> &'static str {
    match mode {
        Mode::Visibility => "record only",
        Mode::Alert => "notify only",
        Mode::Enforcement => "warn and stop correlated workers",
        Mode::Unspecified => "not configured",
    }
}

fn token_count(value: i64) -> String {
    if value >= 1_000_000 && value % 1_000_000 == 0 {
        format!("{}M tokens", value / 1_000_000)
    } else if value >= 1_000 && value % 1_000 == 0 {
        format!("{}k tokens", value / 1_000)
    } else {
        format!("{value} tokens")
    }
}

fn short_duration(duration: StdDuration) -> String {
    let seconds = duration.as_secs();
    if seconds != 0 && seconds.is_multiple_of(3600) {
        format!("{}h", seconds / 3600)
    } else if seconds != 0 && seconds.is_multiple_of(60) {
        format!("{}m", seconds / 60)
    } else {
        format!("{seconds}s")
    }
}

fn compact_home(path: &Path) -> String {
    let rendered = path.display().to_string();
    if let Some(home) = default_home_dir() {
        let home = home.display().to_string();
        if let Some(rest) = rendered.strip_prefix(&home) {
            return format!("~{rest}");
        }
    }
    rendered
}

fn attention_sessions(sessions: &[SessionView]) -> Vec<&SessionView> {
    sessions
        .iter()
        .filter(|session| {
            matches!(session.usage_state.as_str(), "warn" | "stop")
                || session.actionable
                || session.can_acknowledge
        })
        .collect()
}

fn session_matches_state(session: &SessionView, state: &str) -> bool {
    match state {
        "attention" => {
            matches!(session.usage_state.as_str(), "warn" | "stop")
                || session.actionable
                || session.can_acknowledge
        }
        "active" => {
            matches!(session.usage_state.as_str(), "spending" | "warn" | "stop")
                || session.process_state == "running"
        }
        "warning" | "warn" => session.usage_state == "warn",
        "stop" => session.usage_state == "stop",
        "acknowledged" | "ack" => session.acknowledged,
        "idle-high" => session.usage_state == "quiet-high",
        other => {
            session.state == other || session.usage_state == other || session.action_state == other
        }
    }
}

fn print_session_table<'a>(
    label: &str,
    sessions: impl IntoIterator<Item = &'a SessionView>,
    limit: usize,
) {
    let sessions = sessions.into_iter().take(limit).collect::<Vec<_>>();
    println!("{label}");
    println!(
        "  {:<12} {:<9} {:<13} {:<12} {:<12} SESSION",
        "PROVIDER", "STATE", "ACTION", "TURN", "WINDOW"
    );
    for session in sessions {
        println!(
            "  {:<12} {:<9} {:<13} {:<12} {:<12} {}",
            session.provider,
            session.usage_state,
            session.action_state,
            token_count(session.latest_turn_tokens),
            token_count(session.window_tokens),
            session.key,
        );
        println!("    {}", session.explanation);
        if let Some(cwd) = &session.cwd {
            println!("    cwd: {}", compact_home(cwd));
        }
        if session.can_acknowledge {
            println!("    next: curb ack {}", session.key);
        }
    }
}

fn state_dir_for_config(path: &Path) -> PathBuf {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from(".curb"))
}

fn user_config_path() -> Option<PathBuf> {
    if let Some(base) = std::env::var_os("XDG_CONFIG_HOME") {
        return Some(PathBuf::from(base).join("curb").join("config.yaml"));
    }
    if let Some(base) = std::env::var_os("APPDATA") {
        return Some(PathBuf::from(base).join("Curb").join("config.yaml"));
    }
    default_home_dir().map(|home| {
        if cfg!(target_os = "macos") {
            home.join("Library")
                .join("Application Support")
                .join("curb")
                .join("config.yaml")
        } else {
            home.join(".config").join("curb").join("config.yaml")
        }
    })
}

fn default_install_prefix() -> PathBuf {
    default_home_dir()
        .map(|home| home.join(".local"))
        .unwrap_or_else(|| PathBuf::from(".local"))
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;

    use super::*;

    struct FakeDoctorPlatform {
        notifications: RefCell<Vec<(String, String)>>,
        notify_error: Option<&'static str>,
    }

    impl DoctorPlatform for FakeDoctorPlatform {
        fn capture_processes(&self) -> Result<usize, PlatformError> {
            Ok(7)
        }

        fn notification_capability(&self) -> NotificationCapability {
            NotificationCapability {
                supported: true,
                status: "available".to_string(),
                message: "test notifications available".to_string(),
            }
        }

        fn notify(&self, title: &str, body: &str) -> Result<(), PlatformError> {
            self.notifications
                .borrow_mut()
                .push((title.to_string(), body.to_string()));
            if let Some(error) = self.notify_error {
                Err(PlatformError::Notify(error.to_string()))
            } else {
                Ok(())
            }
        }

        fn platform_name(&self) -> &'static str {
            "test"
        }
    }

    #[test]
    fn doctor_writes_ledger_and_uses_injected_platform() {
        let dir = tempfile::tempdir().unwrap();
        let config_path = dir.path().join("curb.yaml");
        let cfg = Config::local_default(Mode::Alert, dir.path().join("state"));
        cfg.save(&config_path).unwrap();
        let platform = FakeDoctorPlatform {
            notifications: RefCell::new(Vec::new()),
            notify_error: None,
        };

        doctor_with_platform(config_path, true, &platform).unwrap();

        let events = crate::ledger::read(dir.path().join("state").join("runs.ndjson")).unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].event_type, "doctor");
        assert_eq!(events[0].mode.as_deref(), Some("alert"));
        assert_eq!(
            platform.notifications.into_inner(),
            vec![("Curb doctor".to_string(), "Notification check".to_string())]
        );
    }
}
