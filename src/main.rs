use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result, bail};
use chrono::{Duration, Utc};
use clap::{CommandFactory, Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "curb")]
#[command(about = "Local AI-agent visibility and watchdog tool")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
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
    /// Serve the Rust local API on loopback.
    Serve {
        /// Config file to use.
        #[arg(long, default_value = "configs/curb.example.yaml")]
        config: PathBuf,
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
        #[arg(long, default_value = "configs/curb.example.yaml")]
        config: PathBuf,
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
        Some(Command::Serve { config, addr, home }) => {
            if !curb::http::is_loopback_host(&addr) {
                bail!("serve address must be loopback, got {addr:?}");
            }
            let cfg = curb::config::Config::load(&config)?;
            let (token, token_path) = curb::api::load_or_create_token(&cfg.service.state_dir)
                .map_err(anyhow::Error::msg)?;
            let home = home
                .or_else(default_home_dir)
                .context("home directory is required for usage log discovery")?;
            let runtime = Arc::new(
                curb::runtime::Runtime::new(cfg, home, curb::platform::SystemPlatform)
                    .with_config_path(config),
            );
            runtime.usage_tick(Utc::now()).map_err(anyhow::Error::msg)?;
            let _watcher = Arc::clone(&runtime).start_usage_watcher();
            let server = curb::api::Server::new(token, runtime).map_err(anyhow::Error::msg)?;
            let listener = curb::http::bind_loopback(&addr).map_err(anyhow::Error::msg)?;
            println!("curb rust api");
            println!("  listening: http://{}", listener.local_addr()?);
            println!("  token: {}", token_path.display());
            println!("  auth: Authorization: Bearer $(cat token-file)");
            println!("  watcher: usage policy scans run in this process");
            curb::http::serve_blocking(listener, &server).map_err(anyhow::Error::msg)?;
        }
        Some(Command::Watch { config, home, once }) => {
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

fn default_home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(PathBuf::from))
}
