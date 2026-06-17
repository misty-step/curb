#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use std::{
    env,
    net::{Shutdown, SocketAddr, TcpStream},
    thread,
};

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{TrayIcon, TrayIconBuilder};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder, Wry};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

const DEFAULT_ADDR: &str = "127.0.0.1:8765";

/// How often the tray polls the local API for the live agent summary.
const TRAY_POLL: Duration = Duration::from_secs(2);

/// The bearer token for the local API, published once `curb serve` prints its
/// startup banner. Shared between the stdout reader and the tray poller, so the
/// tray never has to re-derive the (config/home/XDG-dependent) state dir.
type SharedToken = Arc<Mutex<Option<String>>>;

struct CurbServer {
    child: Mutex<Option<Child>>,
}

fn main() {
    tauri::Builder::default()
        .setup(|app| {
            app.handle().plugin(tauri_plugin_autostart::init(
                MacosLauncher::LaunchAgent,
                None,
            ))?;
            sync_autostart(app);

            let addr = env::var("CURB_DESKTOP_ADDR").unwrap_or_else(|_| DEFAULT_ADDR.to_string());
            let addr = parse_loopback_addr(&addr)?;
            let token: SharedToken = Arc::new(Mutex::new(None));
            let child = ensure_curb_server(&addr, Arc::clone(&token))?;
            app.manage(CurbServer {
                child: Mutex::new(child),
            });

            let url = format!("http://{addr}/");
            WebviewWindowBuilder::new(app, "main", WebviewUrl::External(url.parse()?))
                .title("Curb")
                // Compact by default: a small pane the menu-bar app pops open. The
                // window shrinks to the agent table; expand grows it back.
                .inner_size(460.0, 720.0)
                .min_inner_size(360.0, 380.0)
                .build()?;

            let (tray, status_item) = install_tray(app)?;
            let tray_handle = app.handle().clone();
            thread::spawn(move || tray_poll_loop(tray_handle, tray, status_item, addr, token));
            Ok(())
        })
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running Curb desktop shell");
}

fn install_tray(app: &tauri::App) -> tauri::Result<(TrayIcon, MenuItem<Wry>)> {
    // A disabled header row that the poller keeps current with the live summary,
    // so clicking the tray shows "1 over the kill line" without opening the window.
    let status = MenuItem::with_id(app, "status", "Curb — connecting…", false, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let show = MenuItem::with_id(app, "show", "Show Curb", true, None::<&str>)?;
    let hide = MenuItem::with_id(app, "hide", "Hide", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&status, &separator, &show, &hide, &quit])?;

    let tray = TrayIconBuilder::new()
        // Without an explicit icon the macOS status item renders zero-width and
        // is effectively invisible in the menu bar. Embed the app icon at
        // compile time so the tray entry actually shows up.
        .icon(tauri::include_image!("icons/icon.png"))
        .tooltip("Curb")
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main_window(app),
            "hide" => {
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.hide();
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;
    Ok((tray, status))
}

// Polls the local API for the live summary and reflects it in the menu bar: a
// terse count+severity title next to the icon, the headline as the tooltip, and
// the same headline as the status row at the top of the tray menu. Runs until
// the process exits; a missing token or unreachable API just shows "connecting".
fn tray_poll_loop(
    app: AppHandle,
    tray: TrayIcon,
    status: MenuItem<Wry>,
    addr: SocketAddr,
    token: SharedToken,
) {
    loop {
        let summary = current_token(&token)
            .and_then(|token| fetch_overview(&addr, &token))
            .map(TraySummary::from_overview)
            .unwrap_or_else(TraySummary::connecting);
        let tray = tray.clone();
        let status = status.clone();
        let _ = app.run_on_main_thread(move || {
            let _ = tray.set_title(summary.title);
            let _ = tray.set_tooltip(Some(summary.tooltip));
            let _ = status.set_text(summary.status_text);
        });
        thread::sleep(TRAY_POLL);
    }
}

fn current_token(token: &SharedToken) -> Option<String> {
    token.lock().ok().and_then(|guard| guard.clone())
}

struct OverviewCounts {
    message: String,
    working: u64,
    warn: u64,
    kill: u64,
}

// One blocking loopback GET /v1/overview with bearer auth. The curb server frames
// every response with Content-Length + Connection: close and serves one request
// per stream, so read-to-end then split on the header terminator is sufficient;
// no HTTP client dependency is warranted for this single tiny call.
fn fetch_overview(addr: &SocketAddr, token: &str) -> Option<OverviewCounts> {
    let mut stream = TcpStream::connect_timeout(addr, Duration::from_secs(1)).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(2))).ok()?;
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .ok()?;
    let request = format!(
        "GET /v1/overview HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {token}\r\nAccept: application/json\r\nConnection: close\r\n\r\n"
    );
    stream.write_all(request.as_bytes()).ok()?;
    let _ = stream.shutdown(Shutdown::Write);
    // /v1/overview is tiny; cap the read so neither a slow-dribbling nor an
    // oversized response can balloon memory or wedge the 2s poll cadence.
    let mut raw = Vec::new();
    stream.take(64 * 1024).read_to_end(&mut raw).ok()?;
    let separator = raw.windows(4).position(|window| window == b"\r\n\r\n")?;
    if !raw[..separator].starts_with(b"HTTP/1.1 200") {
        return None;
    }
    let value: serde_json::Value = serde_json::from_slice(&raw[separator + 4..]).ok()?;
    let count = |key| {
        value
            .get(key)
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0)
    };
    Some(OverviewCounts {
        message: value
            .get("message")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("")
            .to_string(),
        working: count("working"),
        warn: count("warn"),
        kill: count("kill"),
    })
}

struct TraySummary {
    title: Option<String>,
    tooltip: String,
    status_text: String,
}

impl TraySummary {
    fn from_overview(overview: OverviewCounts) -> Self {
        // The menu-bar title stays terse: the over-a-line count wins (kill before
        // warn), else the working count, else nothing — a calm icon at rest.
        let title = if overview.kill > 0 {
            Some(format!("{}\u{2715}", overview.kill))
        } else if overview.warn > 0 {
            Some(format!("{}!", overview.warn))
        } else if overview.working > 0 {
            Some(overview.working.to_string())
        } else {
            None
        };
        let headline = if overview.message.is_empty() {
            "Nothing spending".to_string()
        } else {
            overview.message
        };
        Self {
            title,
            tooltip: format!("Curb — {headline}"),
            status_text: headline,
        }
    }

    fn connecting() -> Self {
        Self {
            title: None,
            tooltip: "Curb".to_string(),
            status_text: "Curb — connecting…".to_string(),
        }
    }
}

fn show_main_window(app: &tauri::AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.unminimize();
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn sync_autostart(app: &tauri::App) {
    let autostart = app.autolaunch();
    match env::var("CURB_DESKTOP_AUTOSTART").as_deref() {
        Ok("1" | "true" | "yes" | "on") => {
            let _ = autostart.enable();
        }
        Ok("0" | "false" | "no" | "off") => {
            let _ = autostart.disable();
        }
        _ => {}
    }
}

fn ensure_curb_server(
    addr: &SocketAddr,
    token: SharedToken,
) -> Result<Option<Child>, Box<dyn std::error::Error>> {
    if TcpStream::connect(addr).is_ok() {
        return Err(
            format!("refusing to attach desktop shell to existing listener on {addr}").into(),
        );
    }

    let mut command = Command::new(curb_bin());
    command
        .arg("serve")
        .arg("--addr")
        .arg(addr.to_string())
        // Pipe stdout so the reader thread can lift the token path from the
        // startup banner; stderr stays silenced.
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    if let Ok(config) = env::var("CURB_DESKTOP_CONFIG") {
        command.arg("--config").arg(config);
    }
    if let Ok(home) = env::var("CURB_DESKTOP_HOME") {
        command.arg("--home").arg(home);
    }

    let mut child = command.spawn()?;
    if let Some(stdout) = child.stdout.take() {
        spawn_token_reader(stdout, token);
    }
    wait_for_server(addr, &mut child)?;
    Ok(Some(child))
}

// `curb serve` prints a startup banner that includes `  token: <path>`. Parse
// that line, load the bearer token from the file, and publish it for the tray
// poller; then keep reading (and discarding) so the child never blocks writing
// to a full stdout pipe. This is authoritative, unlike re-deriving the state dir.
fn spawn_token_reader(stdout: ChildStdout, token: SharedToken) {
    thread::spawn(move || {
        let mut published = false;
        for line in BufReader::new(stdout).lines() {
            let Ok(line) = line else { break };
            if published {
                continue;
            }
            let Some((_, path)) = line.split_once("token:") else {
                continue;
            };
            // The banner prints `token:` exactly once, so a transient read miss
            // would otherwise strand the tray on "connecting…" for the whole
            // session. Retry briefly before giving up on this (only) line.
            if let Some(value) = read_token(path.trim()) {
                if let Ok(mut guard) = token.lock() {
                    *guard = Some(value);
                }
                published = true;
            }
        }
    });
}

fn read_token(path: &str) -> Option<String> {
    for attempt in 0..5 {
        if let Ok(contents) = std::fs::read_to_string(path) {
            let value = contents.trim().to_string();
            if !value.is_empty() {
                return Some(value);
            }
        }
        if attempt < 4 {
            thread::sleep(Duration::from_millis(100));
        }
    }
    None
}

fn wait_for_server(addr: &SocketAddr, child: &mut Child) -> Result<(), Box<dyn std::error::Error>> {
    let deadline = Instant::now() + Duration::from_secs(8);
    while Instant::now() < deadline {
        if TcpStream::connect(addr).is_ok() {
            return Ok(());
        }
        if let Some(status) = child.try_wait()? {
            return Err(format!("curb serve exited before desktop window opened: {status}").into());
        }
        thread::sleep(Duration::from_millis(100));
    }
    Err(format!("curb serve did not listen on {addr} within 8s").into())
}

fn parse_loopback_addr(addr: &str) -> Result<SocketAddr, Box<dyn std::error::Error>> {
    let parsed: SocketAddr = addr.parse()?;
    if !parsed.ip().is_loopback() {
        return Err(format!("desktop server address must be loopback, got {addr}").into());
    }
    Ok(parsed)
}

fn curb_bin() -> PathBuf {
    curb_bin_from(
        env::var("CURB_DESKTOP_CURB_BIN").ok(),
        env::current_dir().expect("current directory is required"),
    )
}

fn curb_bin_from(override_path: Option<String>, cwd: PathBuf) -> PathBuf {
    if let Some(path) = override_path {
        let path = PathBuf::from(path);
        assert!(
            path.is_absolute(),
            "CURB_DESKTOP_CURB_BIN must be an absolute path"
        );
        return path;
    }

    let mut path = cwd;
    path.extend([
        "target",
        "debug",
        if cfg!(windows) { "curb.exe" } else { "curb" },
    ]);
    if !path.exists() {
        panic!("set CURB_DESKTOP_CURB_BIN to an absolute curb binary path");
    }
    path
}

impl Drop for CurbServer {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.child.lock()
            && let Some(mut child) = guard.take()
        {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn counts(working: u64, warn: u64, kill: u64, message: &str) -> OverviewCounts {
        OverviewCounts {
            message: message.to_string(),
            working,
            warn,
            kill,
        }
    }

    #[test]
    fn tray_title_ranks_kill_over_warn_over_working() {
        // kill wins, with the stop glyph and the kill count
        let kill = TraySummary::from_overview(counts(3, 1, 1, "1 over the kill line"));
        assert_eq!(kill.title.as_deref(), Some("1\u{2715}"));
        assert_eq!(kill.tooltip, "Curb — 1 over the kill line");
        assert_eq!(kill.status_text, "1 over the kill line");

        // warn shows the warn count with the attention mark
        let warn = TraySummary::from_overview(counts(3, 2, 0, "2 over the warn line"));
        assert_eq!(warn.title.as_deref(), Some("2!"));

        // in limits: just the running count, no severity mark
        let working = TraySummary::from_overview(counts(3, 0, 0, "3 agents working"));
        assert_eq!(working.title.as_deref(), Some("3"));
    }

    #[test]
    fn tray_is_a_bare_icon_at_rest() {
        // nothing spending: no title (calm icon), and a plain headline
        let idle = TraySummary::from_overview(counts(0, 0, 0, ""));
        assert_eq!(idle.title, None);
        assert_eq!(idle.status_text, "Nothing spending");
    }

    #[test]
    fn parse_loopback_addr_accepts_loopback_only() {
        assert_eq!(
            parse_loopback_addr("127.0.0.1:8765").unwrap(),
            "127.0.0.1:8765".parse::<SocketAddr>().unwrap()
        );
        assert_eq!(
            parse_loopback_addr("[::1]:8765").unwrap(),
            "[::1]:8765".parse::<SocketAddr>().unwrap()
        );

        assert!(parse_loopback_addr("0.0.0.0:8765").is_err());
        assert!(parse_loopback_addr("example.com:8765").is_err());
    }

    #[test]
    fn curb_bin_override_must_be_absolute() {
        let result = std::panic::catch_unwind(|| {
            curb_bin_from(Some("curb".to_string()), PathBuf::from("/repo"));
        });

        assert!(result.is_err());
    }

    #[test]
    fn curb_bin_defaults_to_repo_debug_binary() {
        let cwd = unique_temp_dir();
        let expected =
            cwd.join("target")
                .join("debug")
                .join(if cfg!(windows) { "curb.exe" } else { "curb" });
        std::fs::create_dir_all(expected.parent().unwrap()).unwrap();
        std::fs::write(&expected, b"curb").unwrap();

        let path = curb_bin_from(None, cwd.clone());

        assert!(path.is_absolute());
        assert_eq!(path, expected);
        std::fs::remove_dir_all(cwd).unwrap();
    }

    fn unique_temp_dir() -> PathBuf {
        let mut path = env::temp_dir();
        path.push(format!(
            "curb-desktop-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        path
    }
}
