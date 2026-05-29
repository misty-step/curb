# Migrate the web UI to a cross-platform desktop app (Tauri)

Priority: P3
Status: pending
Estimate: L

## Goal
Ship Curb as a menu-bar/tray desktop app (Tauri) that keeps the watchdog running with the window closed, while preserving the existing read model and web mode.

## Non-Goals
- Rewriting the UI's fetch layer to Tauri IPC initially — keep the loopback HTTP transport at first.
- Mac App Store / sandboxed distribution (the sandbox blocks process-kill + usage-file reads).
- Doing this before the trust/quality themes (001–008) land. **Icebox.**

## Oracle
- [ ] A `src-tauri/` target links the existing `curb` lib and opens a window over the loopback server; `curb app`/web mode still works.
- [ ] Tray presence + close-to-tray keeps the watch loop running with no window; launch-at-login is available.
- [ ] A signed, notarized macOS build can read usage files and terminate a correlated worker (entitlements / Full Disk Access path documented).
- [ ] CI (001) builds the desktop artifact on macOS + Linux.

## Notes
**Why:** explicit user request. **Dissent (Carmack):** defer — the desktop app effectively already exists via `curb app`, and Tauri adds zero kill-safety; 100% of Curb's value is backend correctness. Hence P3/icebox, below all trust work.

**Migration research (from 2026-05-29 design discussion):**
- The crate is already `[lib] curb` + a thin bin, and the UI is already a decoupled same-origin SPA over loopback `/v1/*` JSON — so this is packaging + lifecycle, not a rewrite.
- **Tauri over Electron, decisively:** backend is Rust (Tauri's native language); ~3–10 MB vs ~100 MB; native `sysinfo`/process-kill vs a Node sidecar.
- **The real crux is lifecycle, not rendering:** a watchdog must watch with the window closed → tray app + close-to-tray + autostart.
- **The long pole is distribution, not app code:** code signing + notarization, and especially **entitlements / Full Disk Access** to send signals to other apps' process trees and read `~/.codex`/Claude usage files — a sandboxed build can't; a Developer-ID build can. Plus auto-update + per-OS kill behavior.
- `build.rs` + `scripts/build-ui.sh` + the gate's `web/dist` check get replaced by Tauri's bundler; the `curb` lib is untouched.
- Effort tiers: webview wrapper ≈ half a day (demoable, not shippable); real tray app ≈ 1–2 weeks; signed/notarized cross-platform ≈ several weeks (mostly signing/entitlements).
