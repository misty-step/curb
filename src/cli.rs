use std::fs;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration as StdDuration;

use anyhow::{Context, Result, bail};
use chrono::Utc;

use crate::config::{Config, Mode, Preset};
use crate::platform::SystemPlatform;
use crate::runtime::Runtime;

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

pub fn load_or_default_config(path: &Path) -> Result<Config> {
    if path.exists() {
        return Config::load(path).map_err(anyhow::Error::from);
    }
    let cfg = Config::local_default(Mode::Visibility, state_dir_for_config(path));
    cfg.save(path)?;
    Ok(cfg)
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
