use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration as StdDuration;

use anyhow::{Context, Result, bail};
use chrono::{Duration, Utc};
use clap::{CommandFactory, Parser, Subcommand};
use curb::cli::{
    config_command, dashboard_command, default_config_path, default_home_dir, doctor_command,
    init_config, install_binary,
};

#[derive(Debug, Parser)]
#[command(name = "curb")]
#[command(about = "Local AI-agent visibility and watchdog tool")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Create a user config.
    Init {
        /// Config file to create.
        #[arg(long)]
        config: Option<PathBuf>,
        /// Overwrite an existing config.
        #[arg(long)]
        force: bool,
        /// Initial mode: visibility, alert, or enforcement.
        #[arg(long, default_value = "visibility")]
        mode: String,
    },
    /// Install this binary to a prefix.
    Install {
        /// Install prefix. The binary is copied into <prefix>/bin.
        #[arg(long)]
        prefix: Option<PathBuf>,
    },
    /// Show or update the user config.
    Config {
        /// show, path, aggressive, reasonable, or observe.
        action: Option<String>,
    },
    /// Show live agents and usage from the Rust read model.
    #[command(alias = "dash")]
    Dashboard {
        /// Config file to use.
        #[arg(long)]
        config: Option<PathBuf>,
        /// Home directory containing provider log roots.
        #[arg(long)]
        home: Option<PathBuf>,
        /// Maximum sessions to print.
        #[arg(long, default_value_t = 10)]
        limit: usize,
        /// Print JSON.
        #[arg(long)]
        json: bool,
    },
    /// Check local Curb configuration and platform capabilities.
    Doctor {
        /// Config file to use.
        #[arg(long)]
        config: Option<PathBuf>,
        /// Send a real local notification as part of the check.
        #[arg(long)]
        test_notification: bool,
    },
    /// Validate a Curb YAML config.
    ValidateConfig {
        /// Config file to validate.
        #[arg(default_value = "configs/curb.example.yaml")]
        path: PathBuf,
    },
    /// Summarize local provider metadata usage.
    Usage {
        /// Home directory containing provider log roots.
        #[arg(long)]
        home: Option<PathBuf>,
        /// Print JSON.
        #[arg(long)]
        json: bool,
        /// Lookback window such as 168h, 24h, or 15m.
        #[arg(long, default_value = "168h")]
        since: String,
        /// Scan all known local logs.
        #[arg(long)]
        all: bool,
    },
    /// Stream new local provider usage events.
    Tail {
        /// Home directory containing provider log roots.
        #[arg(long)]
        home: Option<PathBuf>,
        /// Initial and rolling lookback window such as 5m, 1h, or 30s.
        #[arg(long, default_value = "5m")]
        since: String,
        /// Poll interval such as 2s or 500ms.
        #[arg(long, default_value = "2s")]
        interval: String,
        /// Run one scan and exit.
        #[arg(long)]
        once: bool,
    },
    /// Serve the Rust local API on loopback.
    Serve {
        /// Config file to use.
        #[arg(long)]
        config: Option<PathBuf>,
        /// Loopback address to bind.
        #[arg(long, default_value = "127.0.0.1:8765")]
        addr: String,
        /// Home directory containing provider log roots.
        #[arg(long)]
        home: Option<PathBuf>,
    },
    /// Serve the Rust dashboard and open it in the browser.
    App {
        /// Config file to use.
        #[arg(long)]
        config: Option<PathBuf>,
        /// Loopback address to bind.
        #[arg(long, default_value = "127.0.0.1:8765")]
        addr: String,
        /// Home directory containing provider log roots.
        #[arg(long)]
        home: Option<PathBuf>,
    },
    /// Run the Rust usage watcher loop.
    Watch {
        /// Config file to use.
        #[arg(long)]
        config: Option<PathBuf>,
        /// Home directory containing provider log roots.
        #[arg(long)]
        home: Option<PathBuf>,
        /// Run one scan and exit.
        #[arg(long)]
        once: bool,
    },
}

fn main() {
    if let Err(err) = run() {
        eprintln!("curb: {err:#}");
        std::process::exit(1);
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Some(Command::ValidateConfig { path }) => {
            let cfg = curb::config::Config::load(&path)?;
            println!(
                "ok config={} mode={} agents={} ledger={}",
                path.display(),
                cfg.mode,
                cfg.agents.len(),
                cfg.ledger.path.display()
            );
        }
        Some(Command::Init {
            config,
            force,
            mode,
        }) => init_config(config.unwrap_or_else(default_config_path), force, &mode)?,
        Some(Command::Install { prefix }) => install_binary(prefix)?,
        Some(Command::Config { action }) => config_command(action)?,
        Some(Command::Dashboard {
            config,
            home,
            limit,
            json,
        }) => {
            let home = home
                .or_else(default_home_dir)
                .context("home directory is required for usage log discovery")?;
            dashboard_command(
                config.unwrap_or_else(default_config_path),
                home,
                limit,
                json,
            )?
        }
        Some(Command::Doctor {
            config,
            test_notification,
        }) => doctor_command(
            config.unwrap_or_else(default_config_path),
            test_notification,
        )?,
        Some(Command::Usage {
            home,
            json,
            since,
            all,
        }) => {
            let home = home.unwrap_or(std::env::current_dir()?);
            let since = if all {
                None
            } else {
                let duration =
                    curb::config::parse_duration_for_cli(&since).map_err(anyhow::Error::msg)?;
                Some(Utc::now() - Duration::from_std(duration)?)
            };
            let report = curb::usage::Reader::new(home).report_since(since)?;
            if json {
                serde_json::to_writer_pretty(std::io::stdout(), &report)?;
                println!();
            } else {
                println!("curb usage");
                println!("  sources: {}", report.source_line());
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
        }
        Some(Command::Tail {
            home,
            since,
            interval,
            once,
        }) => {
            let home = home
                .or_else(default_home_dir)
                .context("home directory is required for usage log discovery")?;
            let since = curb::config::parse_duration_for_cli(&since).map_err(anyhow::Error::msg)?;
            let interval =
                curb::config::parse_duration_for_cli(&interval).map_err(anyhow::Error::msg)?;
            tail_command(home, since, interval, once)?;
        }
        Some(Command::Serve { config, addr, home }) => serve_dashboard(
            config.unwrap_or_else(default_config_path),
            addr,
            home,
            false,
        )?,
        Some(Command::App { config, addr, home }) => {
            serve_dashboard(config.unwrap_or_else(default_config_path), addr, home, true)?
        }
        Some(Command::Watch { config, home, once }) => {
            let config = config.unwrap_or_else(default_config_path);
            let cfg = curb::config::Config::load(&config)?;
            let home = home
                .or_else(default_home_dir)
                .context("home directory is required for usage log discovery")?;
            let interval = cfg.usage.scan_interval.as_std();
            let runtime =
                curb::runtime::Runtime::new(cfg.clone(), home, curb::platform::SystemPlatform)
                    .with_config_path(config);
            println!("curb rust watcher");
            println!("  mode: {}", cfg.mode);
            println!(
                "  usage: warn {} tokens/turn, stop {} tokens/turn, window {}s",
                cfg.usage.warn_turn_tokens,
                cfg.usage.kill_turn_tokens,
                cfg.usage.window.as_std().as_secs()
            );
            println!("  ledger: {}", cfg.ledger.path.display());
            loop {
                let snapshot = runtime.usage_tick(Utc::now()).map_err(anyhow::Error::msg)?;
                println!(
                    "scan: status={} active={} warn={} stop={}",
                    snapshot.overview.status,
                    snapshot.overview.active_sessions,
                    snapshot.overview.warning_sessions,
                    snapshot.overview.stop_sessions
                );
                if once {
                    break;
                }
                std::thread::sleep(interval);
            }
        }
        None => {
            Cli::command().print_help()?;
            println!();
        }
    }
    Ok(())
}

fn tail_command(
    home: PathBuf,
    since: StdDuration,
    interval: StdDuration,
    once: bool,
) -> Result<()> {
    let reader = curb::usage::Reader::new(home);
    let mut state = curb::tail::TailState::default();
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
        let scan = curb::tail::scan_once(&reader, &mut state, std::io::stdout(), since_at, now)?;
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

fn serve_dashboard(
    config: PathBuf,
    addr: String,
    home: Option<PathBuf>,
    open_browser: bool,
) -> Result<()> {
    if !curb::http::is_loopback_host(&addr) {
        bail!("serve address must be loopback, got {addr:?}");
    }
    let cfg = curb::config::Config::load(&config)?;
    let (token, token_path) =
        curb::api::load_or_create_token(&cfg.service.state_dir).map_err(anyhow::Error::msg)?;
    let home = home
        .or_else(default_home_dir)
        .context("home directory is required for usage log discovery")?;
    let runtime = Arc::new(
        curb::runtime::Runtime::new(cfg, home, curb::platform::SystemPlatform)
            .with_config_path(config),
    );
    runtime.usage_tick(Utc::now()).map_err(anyhow::Error::msg)?;
    let _watcher = Arc::clone(&runtime).start_usage_watcher();
    let mut server = curb::api::Server::new(token, runtime).map_err(anyhow::Error::msg)?;
    server.serve_ui();
    let listener = curb::http::bind_loopback(&addr).map_err(anyhow::Error::msg)?;
    let url = format!("http://{}/", listener.local_addr()?);
    println!("curb rust app");
    println!("  listening: {url}");
    println!("  token: {}", token_path.display());
    println!("  auth: Authorization: Bearer $(cat token-file)");
    println!("  watcher: usage policy scans run in this process");
    if open_browser && let Err(error) = open_dashboard(&url) {
        eprintln!("curb: could not open dashboard: {error}");
    }
    curb::http::serve_blocking(listener, &server).map_err(anyhow::Error::msg)
}

fn open_dashboard(url: &str) -> Result<()> {
    let status = if cfg!(target_os = "macos") {
        std::process::Command::new("open").arg(url).status()
    } else if cfg!(target_os = "windows") {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", url])
            .status()
    } else {
        std::process::Command::new("xdg-open").arg(url).status()
    };
    match status {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => bail!("open dashboard command exited with {status}"),
        Err(error) => bail!("open dashboard: {error}"),
    }
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
