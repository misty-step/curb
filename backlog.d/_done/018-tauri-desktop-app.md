# Migrate the web UI to a cross-platform desktop app (Tauri)

Priority: P3
Status: done
Estimate: L

## Goal
Ship Curb as a menu-bar/tray desktop app (Tauri) that keeps the watchdog running with the window closed, while preserving the existing read model and web mode.

## Non-Goals
- Rewriting the UI's fetch layer to Tauri IPC initially — keep the loopback HTTP transport at first.
- Mac App Store / sandboxed distribution (the sandbox blocks process-kill + usage-file reads).
- Developer ID signing, notarization, auto-update, or public release packaging. This is an internal team app.

## Oracle
- [x] A `src-tauri/` target opens a window over the existing loopback server; `curb app`/web mode still works.
- [x] Tray presence + close-to-tray keeps the watch loop running with no window; launch-at-login is available through the Tauri autostart plugin and `CURB_DESKTOP_AUTOSTART`.
- [x] Unsigned internal distribution path is documented for macOS, Windows, and Linux.
- [x] CI builds the desktop artifact on macOS + Linux.

## Notes
**Grooming status (2026-06-01):** blocked/icebox. Keep this as a preserved
option, not active next work, until the daemon lifecycle/read-model/config
surface is stable and the user explicitly chooses native packaging. A desktop
wrapper still does not improve Curb's termination safety by itself.

**Closeout (2026-06-02):** user narrowed the distribution requirement to an
internal unsigned, cross-platform Tauri shell. Implemented `src-tauri/` as a
thin lifecycle wrapper around `curb serve`: it starts or reuses the loopback
server, opens a desktop window to the existing UI, keeps the server alive when
the window closes, exposes tray show/hide/quit, and supports opt-in autostart.
Public signing/notarization remains deliberately out of scope.
`docs/internal-desktop-app.md` documents the cargo-based team distribution path.

**Why:** explicit user request. **Dissent (Carmack):** defer — the desktop app effectively already exists via `curb app`, and Tauri adds zero kill-safety; 100% of Curb's value is backend correctness. Hence P3/icebox, below all trust work.

**Migration research (from 2026-05-29 design discussion):**
- The crate is already `[lib] curb` + a thin bin, and the UI is already a decoupled same-origin SPA over loopback `/v1/*` JSON — so this is packaging + lifecycle, not a rewrite.
- **Tauri over Electron, decisively:** backend is Rust (Tauri's native language); ~3–10 MB vs ~100 MB; native `sysinfo`/process-kill vs a Node sidecar.
- **The real crux is lifecycle, not rendering:** a watchdog must watch with the window closed → tray app + close-to-tray + autostart.
- **The long pole is distribution, not app code:** code signing + notarization, and especially **entitlements / Full Disk Access** to send signals to other apps' process trees and read `~/.codex`/Claude usage files — a sandboxed build can't; a Developer-ID build can. Plus auto-update + per-OS kill behavior.
- `build.rs` + `scripts/build-ui.sh` + the gate's `web/dist` check get replaced by Tauri's bundler; the `curb` lib is untouched.
- Effort tiers: webview wrapper ≈ half a day (demoable, not shippable); real tray app ≈ 1–2 weeks; signed/notarized cross-platform ≈ several weeks (mostly signing/entitlements).
