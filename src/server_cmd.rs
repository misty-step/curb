use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::thread;
use std::time::Instant;

use anyhow::{Context, Result, bail};
use chrono::Utc;

use crate::cli::{default_config_path, default_home_dir, load_or_default_config};

pub fn watch_command(config: Option<PathBuf>, home: Option<PathBuf>, once: bool) -> Result<()> {
    let config = config.unwrap_or_else(default_config_path);
    let cfg = load_or_default_config(&config)?;
    let home = home
        .or_else(default_home_dir)
        .context("home directory is required for usage log discovery")?;
    let interval = cfg.usage.scan_interval.as_std();
    let runtime =
        curb_core::runtime::Runtime::new(cfg.clone(), home, curb_core::platform::SystemPlatform)
            .with_config_path(config);
    println!("curb watcher");
    println!("  mode: {}", cfg.mode);
    println!(
        "  usage: warn {} tokens/turn, stop {} tokens/turn, window {}s",
        cfg.usage.warn_turn_tokens,
        cfg.usage.kill_turn_tokens,
        cfg.usage.window.as_std().as_secs()
    );
    println!("  ledger: {}", cfg.ledger.path.display());
    loop {
        let started = Instant::now();
        let report = runtime
            .usage_tick_report(Utc::now())
            .map_err(anyhow::Error::msg)?;
        crate::observability::emit_usage_tick_report(
            &report,
            "watcher_tick",
            Some(started.elapsed()),
        );
        println!(
            "scan: status={} working={} warn={} kill={}",
            report.snapshot.overview.status,
            report.snapshot.overview.working,
            report.snapshot.overview.warn,
            report.snapshot.overview.kill
        );
        if once {
            break;
        }
        thread::sleep(interval);
    }
    Ok(())
}

pub fn serve_dashboard(
    config: PathBuf,
    addr: String,
    home: Option<PathBuf>,
    open_browser: bool,
    headless: bool,
) -> Result<()> {
    if !crate::http::is_loopback_host(&addr) {
        bail!("serve address must be loopback, got {addr:?}");
    }
    let cfg = curb_core::config::Config::load(&config)?;
    crate::observability::emit_config_loaded(&config, &cfg.mode.to_string(), cfg.agents.len());
    let (token, token_path) =
        crate::api::load_or_create_token(&cfg.service.state_dir).map_err(anyhow::Error::msg)?;
    let home = home
        .or_else(default_home_dir)
        .context("home directory is required for usage log discovery")?;
    let runtime = Arc::new(
        curb_core::runtime::Runtime::new(cfg, home, curb_core::platform::SystemPlatform)
            .with_config_path(config),
    );
    let runtime_tasks = Arc::clone(&runtime);
    let mut server = crate::api::Server::new(token, runtime).map_err(anyhow::Error::msg)?;
    if headless {
        server.serve_headless();
    } else {
        server.serve_ui();
    }
    let listener = crate::http::bind_loopback(&addr).map_err(anyhow::Error::msg)?;
    let url = format!("http://{}/", listener.local_addr()?);
    crate::observability::emit_server_started(&url, headless);
    if headless {
        println!("curb headless server");
    } else {
        println!("curb rust app");
    }
    println!("  listening: {url}");
    println!("  token: {}", token_path.display());
    println!("  auth: Authorization: Bearer $(cat token-file)");
    if headless {
        println!("  ui: disabled");
        println!("  live: {url}v1/live");
        println!("  ready: {url}v1/ready");
    }
    println!("  watcher: usage policy scans run in this process");
    if open_browser && let Err(error) = open_dashboard(&url) {
        eprintln!("curb: could not open dashboard: {error}");
    }
    let shutdown = install_shutdown_handler()?;
    run_initial_usage_scan(Arc::clone(&runtime_tasks));
    let _watcher = start_observed_usage_watcher(Arc::clone(&runtime_tasks));
    let result = crate::http::serve_until(listener, &server, || shutdown.load(Ordering::SeqCst))
        .map_err(anyhow::Error::msg);
    crate::observability::emit_shutdown("server", "serve loop exited");
    result
}

fn install_shutdown_handler() -> Result<Arc<AtomicBool>> {
    let shutdown = Arc::new(AtomicBool::new(false));
    let signal = Arc::clone(&shutdown);
    ctrlc::set_handler(move || {
        signal.store(true, Ordering::SeqCst);
    })
    .context("install Ctrl-C handler")?;
    Ok(shutdown)
}

fn run_initial_usage_scan(
    runtime: Arc<curb_core::runtime::Runtime<curb_core::platform::SystemPlatform>>,
) {
    thread::spawn(move || {
        let started = Instant::now();
        match runtime.usage_tick_report(Utc::now()) {
            Ok(report) => crate::observability::emit_usage_tick_report(
                &report,
                "usage_scan",
                Some(started.elapsed()),
            ),
            Err(error) => {
                let message = format!("{error:#}");
                crate::observability::emit_usage_scan_failure(
                    "usage_scan",
                    &message,
                    started.elapsed(),
                );
                eprintln!("curb: initial usage scan failed: {message}");
            }
        }
    });
}

fn start_observed_usage_watcher(
    runtime: Arc<curb_core::runtime::Runtime<curb_core::platform::SystemPlatform>>,
) -> curb_core::runtime::WatcherHandle {
    runtime.start_usage_watcher_with_report_observer(move |result, duration| match result {
        Ok(report) => {
            crate::observability::emit_usage_tick_report(report, "watcher_tick", Some(duration));
        }
        Err(error) => {
            let message = format!("{error:#}");
            crate::observability::emit_usage_scan_failure("watcher_tick", &message, duration);
        }
    })
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
