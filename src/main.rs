use std::path::PathBuf;

use anyhow::Result;
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
        None => {
            Cli::command().print_help()?;
            println!();
        }
    }
    Ok(())
}
