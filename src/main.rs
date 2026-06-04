//! Curb transport/presentation shell.
//!
//! This binary crate owns the HTTP API, the embedded web UI, the CLI, and the
//! terminal dashboard. It is one consumer of `curb-core`; the engine never
//! depends on anything here.

mod api;
mod cli;
mod dashboard;
mod http;
mod observability;
mod server_cmd;
mod usage_cli;
mod web;

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand};

use crate::cli::{
    ack_command, config_command, config_set_command, dashboard_command, default_config_path,
    default_home_dir, doctor_command, init_config, install_binary, runs_command, scan_command,
    status_command,
};
use crate::server_cmd::{serve_dashboard, watch_command};
use crate::usage_cli::{tail_command, usage_command};

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
        /// show, path, aggressive, reasonable, observe, or set.
        action: Option<String>,
        /// Arguments for `curb config set`.
        #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
        args: Vec<String>,
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
    /// Show current Curb status from the Rust read model.
    Status {
        /// Config file to use.
        #[arg(long)]
        config: Option<PathBuf>,
        /// Home directory containing provider log roots.
        #[arg(long)]
        home: Option<PathBuf>,
        /// Print JSON.
        #[arg(long)]
        json: bool,
    },
    /// Print current configured process matches once.
    Scan {
        /// Config file to use.
        #[arg(long)]
        config: Option<PathBuf>,
        /// Home directory containing provider log roots.
        #[arg(long)]
        home: Option<PathBuf>,
        /// Print JSON.
        #[arg(long)]
        json: bool,
    },
    /// Show sessions tracked by local provider metadata.
    #[command(alias = "sessions")]
    Runs {
        /// Config file to use.
        #[arg(long)]
        config: Option<PathBuf>,
        /// Home directory containing provider log roots.
        #[arg(long)]
        home: Option<PathBuf>,
        /// Show only active or live sessions.
        #[arg(long)]
        active: bool,
        /// Maximum sessions to print.
        #[arg(long, default_value_t = 12)]
        limit: usize,
        /// Filter by session state: all, attention, active, warning, stop, acknowledged.
        #[arg(long, default_value = "all")]
        state: String,
        /// Filter by provider, such as codex or claude.
        #[arg(long)]
        provider: Option<String>,
        /// Print JSON.
        #[arg(long)]
        json: bool,
    },
    /// Acknowledge and extend a warning session.
    Ack {
        /// Session key, such as codex:session-id.
        key: String,
        /// Config file to use.
        #[arg(long)]
        config: Option<PathBuf>,
        /// Home directory containing provider log roots.
        #[arg(long)]
        home: Option<PathBuf>,
        /// Extension duration. Curb clamps this to the configured max.
        #[arg(long, default_value = "30m")]
        extend: String,
        /// Optional acknowledgement reason.
        #[arg(long, default_value = "")]
        reason: String,
    },
    /// Serve the Rust local API on loopback.
    #[command(aliases = ["daemon", "api"])]
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
        /// Serve the API/runtime without the embedded web UI.
        #[arg(long)]
        headless: bool,
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
    #[command(aliases = ["run", "start", "curb"])]
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
    let args = std::env::args().collect::<Vec<_>>();
    if args.get(1).map(String::as_str) == Some("help")
        && args.get(2).map(String::as_str) == Some("advanced")
    {
        print_advanced_help();
        return Ok(());
    }
    let cli = Cli::parse();
    match cli.command {
        Some(Command::ValidateConfig { path }) => {
            let cfg = curb_core::config::Config::load(&path)?;
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
        Some(Command::Config { action, args }) => {
            if action.as_deref() == Some("set") {
                config_set_command(args)?;
            } else if !args.is_empty() {
                bail!("unexpected config arguments after {action:?}");
            } else {
                config_command(action)?;
            }
        }
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
            )?;
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
            let home = home
                .or_else(default_home_dir)
                .context("home directory is required for usage log discovery")?;
            usage_command(home, json, since, all)?;
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
            let since =
                curb_core::config::parse_duration_for_cli(&since).map_err(anyhow::Error::msg)?;
            let interval =
                curb_core::config::parse_duration_for_cli(&interval).map_err(anyhow::Error::msg)?;
            tail_command(home, since, interval, once)?;
        }
        Some(Command::Status { config, home, json }) => {
            let home = home
                .or_else(default_home_dir)
                .context("home directory is required for usage log discovery")?;
            status_command(config.unwrap_or_else(default_config_path), home, json)?;
        }
        Some(Command::Scan { config, home, json }) => {
            let home = home
                .or_else(default_home_dir)
                .context("home directory is required for process and usage correlation")?;
            scan_command(config.unwrap_or_else(default_config_path), home, json)?;
        }
        Some(Command::Runs {
            config,
            home,
            active,
            limit,
            state,
            provider,
            json,
        }) => {
            let home = home
                .or_else(default_home_dir)
                .context("home directory is required for usage log discovery")?;
            runs_command(
                config.unwrap_or_else(default_config_path),
                home,
                active,
                &state,
                provider.as_deref(),
                json,
                limit,
            )?;
        }
        Some(Command::Ack {
            key,
            config,
            home,
            extend,
            reason,
        }) => {
            let home = home
                .or_else(default_home_dir)
                .context("home directory is required for usage log discovery")?;
            ack_command(
                config.unwrap_or_else(default_config_path),
                home,
                key,
                &extend,
                reason,
            )?;
        }
        Some(Command::Serve {
            config,
            addr,
            home,
            headless,
        }) => serve_dashboard(
            config.unwrap_or_else(default_config_path),
            addr,
            home,
            false,
            headless,
        )?,
        Some(Command::App { config, addr, home }) => {
            serve_dashboard(
                config.unwrap_or_else(default_config_path),
                addr,
                home,
                true,
                false,
            )?;
        }
        Some(Command::Watch { config, home, once }) => watch_command(config, home, once)?,
        None => watch_command(None, None, false)?,
    }
    Ok(())
}

fn print_advanced_help() {
    println!("curb advanced commands:");
    println!("  init              create a user config");
    println!("  install           install this binary to ~/.local/bin/curb");
    println!("  config            show or update config");
    println!("  config set        update first-class policy fields");
    println!("  dashboard         show live agents plus recent usage");
    println!("  app               serve and open the local dashboard");
    println!("  serve|daemon|api  serve token-gated local API; use --headless for API-only mode");
    println!("  usage             summarize local Codex and Claude usage logs");
    println!("  tail              stream new usage events");
    println!("  run|start|watch   run the usage policy loop");
    println!("  scan              print current process matches once");
    println!("  validate-config   validate config");
    println!("  status            print config and session status");
    println!("  runs|sessions     summarize usage sessions");
    println!("  ack               acknowledge a usage session");
    println!("  doctor            check local capabilities");
}
