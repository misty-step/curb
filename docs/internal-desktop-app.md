# Internal Desktop App

Curb's desktop app is an internal Tauri shell around the same loopback API used
by `curb app`. It does not replace the CLI, daemon, API server, or web UI.

## Run Locally

Build the Curb CLI first:

```bash
cargo build --bin curb
```

Then run the Tauri shell:

```bash
CURB_DESKTOP_CURB_BIN="$PWD/target/debug/curb" cargo run --manifest-path src-tauri/Cargo.toml
```

The shell starts `curb serve --addr 127.0.0.1:8765` unless something is already
listening on that address. It refuses to attach to an existing listener, opens a
native window to the server it started, and keeps that server alive when the
window closes. The tray menu exposes Show, Hide, and Quit.

The shell intentionally does not attach to a pre-existing listener. That keeps
the Tauri window from granting desktop-app privileges to arbitrary local HTTP
content that won the port race.

The loopback server must stay responsive while the native webview keeps idle or
long-lived sockets open during page load. `src/http.rs` handles accepted streams
concurrently so a stalled webview connection cannot block `/v1/live`, the
embedded dashboard, or later API calls.

## Configuration

- `CURB_DESKTOP_CURB_BIN`: absolute path to the `curb` binary. If omitted, the
  shell looks for `target/debug/curb` from the current working directory.
- `CURB_DESKTOP_ADDR`: loopback address. Defaults to `127.0.0.1:8765`; non-loopback
  addresses are rejected.
- `CURB_DESKTOP_CONFIG`: optional path passed to `curb serve --config`.
- `CURB_DESKTOP_HOME`: optional path passed to `curb serve --home`.
- `CURB_DESKTOP_AUTOSTART`: `1`/`true` enables launch-at-login, `0`/`false`
  disables it, omitted leaves the current setting unchanged.

## Internal Distribution

For team use, ship source or a checked-out repo and run the cargo command above.
No Developer ID signing, notarization, app-store sandboxing, installer,
auto-update, or public release channel is required for this phase.

Cross-platform CI builds the desktop executable on macOS and Linux through
`scripts/check-desktop.sh`. Windows desktop packaging remains a manual
team-run path, while `.github/workflows/ci.yml` includes a focused Windows smoke
for the Rust binary, example config validation, notification capability
contract, and Windows termination-command construction.
