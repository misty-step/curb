#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use std::{
    env,
    net::{SocketAddr, TcpStream},
    thread,
};

use tauri::menu::{Menu, MenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_autostart::{MacosLauncher, ManagerExt};

const DEFAULT_ADDR: &str = "127.0.0.1:8765";

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
            let child = ensure_curb_server(&addr)?;
            app.manage(CurbServer {
                child: Mutex::new(child),
            });

            let url = format!("http://{addr}/");
            WebviewWindowBuilder::new(app, "main", WebviewUrl::External(url.parse()?))
                .title("Curb")
                .inner_size(1180.0, 820.0)
                .min_inner_size(920.0, 640.0)
                .build()?;

            install_tray(app)?;
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

fn install_tray(app: &tauri::App) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "Show", true, None::<&str>)?;
    let hide = MenuItem::with_id(app, "hide", "Hide", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &hide, &quit])?;

    TrayIconBuilder::new()
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
    Ok(())
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

fn ensure_curb_server(addr: &SocketAddr) -> Result<Option<Child>, Box<dyn std::error::Error>> {
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
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if let Ok(config) = env::var("CURB_DESKTOP_CONFIG") {
        command.arg("--config").arg(config);
    }
    if let Ok(home) = env::var("CURB_DESKTOP_HOME") {
        command.arg("--home").arg(home);
    }

    let mut child = command.spawn()?;
    wait_for_server(addr, &mut child)?;
    Ok(Some(child))
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
