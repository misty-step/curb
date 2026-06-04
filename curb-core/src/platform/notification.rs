use std::process::Command;

use super::{CommandSpec, NotificationCapability, PlatformError};

pub(super) fn capability_for(os: &str, exists: impl Fn(&str) -> bool) -> NotificationCapability {
    match os {
        "macos" => {
            if exists("osascript") {
                NotificationCapability {
                    supported: true,
                    status: "available".to_string(),
                    message: "macOS user notifications available through osascript".to_string(),
                }
            } else {
                NotificationCapability {
                    supported: false,
                    status: "unavailable".to_string(),
                    message: "osascript not found".to_string(),
                }
            }
        }
        "linux" => {
            if exists("notify-send") {
                NotificationCapability {
                    supported: true,
                    status: "available".to_string(),
                    message: "Desktop notification command found".to_string(),
                }
            } else {
                NotificationCapability {
                    supported: false,
                    status: "unavailable".to_string(),
                    message: "notify-send not found".to_string(),
                }
            }
        }
        "windows" => NotificationCapability {
            supported: false,
            status: "unsupported".to_string(),
            message: "Windows toast notifications are not implemented".to_string(),
        },
        other => NotificationCapability {
            supported: false,
            status: "unsupported".to_string(),
            message: format!("notifications unsupported on {other}"),
        },
    }
}

pub(super) fn command(os: &str, title: &str, body: &str) -> Result<CommandSpec, PlatformError> {
    match os {
        "macos" => Ok(CommandSpec {
            program: "osascript".to_string(),
            args: vec![
                "-e".to_string(),
                format!(
                    "display notification {} with title {}",
                    apple_script_string(body),
                    apple_script_string(title)
                ),
            ],
        }),
        "linux" => Ok(CommandSpec {
            program: "notify-send".to_string(),
            args: vec![title.to_string(), body.to_string()],
        }),
        "windows" => Err(PlatformError::Notify(
            "Windows toast notifications are not implemented".to_string(),
        )),
        other => Err(PlatformError::Notify(format!(
            "notifications unsupported on {other}"
        ))),
    }
}

pub(super) fn run(spec: CommandSpec) -> Result<(), PlatformError> {
    let status = Command::new(&spec.program)
        .args(&spec.args)
        .status()
        .map_err(|source| PlatformError::Notify(source.to_string()))?;
    if status.success() {
        Ok(())
    } else {
        Err(PlatformError::Notify(format!(
            "{} exited with {status}",
            spec.program
        )))
    }
}

pub(super) fn command_exists(program: &str) -> bool {
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };
    std::env::split_paths(&paths).any(|dir| dir.join(program).is_file())
}

fn apple_script_string(value: &str) -> String {
    let escaped = value
        .chars()
        .flat_map(|ch| match ch {
            '"' | '\\' => vec!['\\', ch],
            _ => vec![ch],
        })
        .collect::<String>();
    format!("\"{escaped}\"")
}
