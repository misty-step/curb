# Curb Launch Specification

Date: 2026-05-18
Status: draft for launch implementation

## Summary

Curb is a local AI-agent visibility and watchdog tool for developer machines.
It detects configured agent applications and processes, reads metadata-only
agent usage logs where providers expose them, creates usage and runtime records,
and later warns or enforces policy from those records.

The strategic pivot is that wall-clock runtime is not the primary spend signal.
An agent process can sit idle for hours without consuming tokens, while a short
agent loop can burn a large model budget quickly. Curb should therefore build
usage visibility first and treat duration enforcement as a secondary
stuck-process control.

The launch target is cross-platform: macOS, Windows, and Linux. macOS has the
first verified desktop-agent evidence in this spec, but the architecture must
ship as a portable core with OS-specific adapters, not as a macOS-only tool.

First-class agent support:

- Codex Desktop, including agents launched from the Codex desktop application.
- Codex CLI.
- Claude Desktop.
- Claude Code.
- Additional desktop or CLI agents through configuration.

Curb must not depend on wrapping `codex`, `claude`, or any other single binary.
Desktop-launched agents are a primary requirement, not a later edge case.

## Product Goal

Give an operator or managed-device owner a reliable local control surface for
agent activity:

1. See what local AI agents are running.
2. See which sessions are consuming tokens, which models they use, and how that
   usage changes over time.
3. Drill into usage by provider, session, turn/request, model, and local working
   directory without capturing prompt or response content.
4. Warn the user before usage policy is violated.
5. Allow explicit acknowledgement and limited extensions.
6. Kill agent runs that exceed configured policy in enforcement mode, after the
   usage primitives are reliable enough to justify enforcement.
7. Preserve an append-only evidence ledger without capturing prompt content.

## Non-Goals

- No prompt, response, screenshot, keystroke, or file-content capture by default.
- No attempt to perfectly estimate vendor billing in v1. Local token metadata is
  a visibility primitive, not an invoice.
- No dependency on OpenAI Enterprise, Anthropic Enterprise, or vendor admin APIs.
- No browser-tab deep inspection in v1.
- No hidden termination in visibility or alert mode.
- No guarantee that unmanaged users cannot evade policy by renaming binaries,
  disabling Curb, or using browser-only agents.

## Operating Modes

### Visibility

Visibility mode is the default for local experimentation.

Behavior:

- Discover matching agent processes and applications.
- Create run records.
- Track elapsed runtime, process trees, and coarse activity.
- Write the local ledger.
- Print status through the CLI.
- Never terminate a process.

### Alert

Alert mode is the default for early team rollout.

Behavior:

- Everything in visibility mode.
- Send local notifications at warning thresholds.
- Support `curb ack` to grant configured extensions.
- Optionally forward events to webhooks or Slack.
- Never terminate a process.

### Enforcement

Enforcement mode is for company-managed devices.

Behavior:

- Everything in alert mode.
- Start a grace period after a kill threshold is crossed.
- Re-check the process tree after grace.
- Terminate the configured process tree.
- Record termination evidence and failures.
- Optionally block immediate relaunch for a configured cooldown window.

## Launch Architecture

### Components

Curb ships as one local endpoint agent. There is no required central control
plane, gateway, or cloud-side enforcement authority in the launch product.
Remote systems may receive metadata-only events, but the endpoint owns policy,
correlation, warnings, and termination.

- `src/main.rs`: CLI commands and user-facing command composition.
- `src/api.rs`: token-gated loopback HTTP adapter for UI and CLI clients.
- `src/config.rs`: strict YAML loading, defaults, validation, and policy
  merging.
- `src/runtime.rs`: daemon orchestration, durable machine identity, config
  persistence, snapshot cache, UI/API read models, usagewatch ownership,
  session actions, and optional metadata-only ledger export.
- `src/service.rs`: service-owned read models, session/process correlation,
  actionability, acknowledgement projection, and stop-session revalidation.
- `src/usage.rs`: provider-specific metadata readers and provider-neutral
  usage events for tokens, model, session, turn/request, cwd, and timestamp.
- `src/usagewatch.rs`: usage policy evaluation, session/process correlation,
  acknowledgement, warning, grace, and usage-based enforcement.
- `src/platform.rs`: macOS, Windows, and Linux process discovery,
  notification, and sealed termination-target construction.
- `src/ledger.rs`: append-only NDJSON event journal with hash chaining.

Future native shells, tray apps, service installers, or managed-device wrappers
must remain clients or deployment packaging around the same local service.

### Usage Model

Curb normalizes provider logs into one metadata-only event shape:

- provider and source.
- local source path or cursor.
- session id.
- turn id or request id when available.
- timestamp.
- working directory.
- model id when available.
- input, cached input, cache creation, output, reasoning, total, and cumulative
  token counters when available.

The dashboard and policy engine distinguish two token concepts:

- **checkpoint tokens**: the provider-reported context or request total for the
  latest observed usage row. This explains how large the current context is, but
  it is not the same as newly consumed work.
- **spent tokens**: uncached input plus cache creation, output, and reasoning
  tokens for the observed checkpoint. Cached/read tokens remain visible for
  diagnostics, but they do not drive the primary fresh-checkpoint headline or
  warning/stop thresholds.

Codex emits local `token_count` checkpoints with `last_token_usage` and
`total_token_usage`; Curb treats `last_token_usage.total_tokens` as the
checkpoint size and derives spent tokens by excluding cached input. Claude Code
emits request usage rows; Curb treats each request row as a checkpoint and
excludes `cache_read_input_tokens` from spent tokens. Cumulative counters stay
available for reconciliation and future velocity work, but they are not used as
the primary operator-facing spend value.

These local logs are completion/checkpoint ledgers. They prove that work
finished and what it cost; they do not prove that tokens are being consumed at
this exact instant. Curb therefore keeps liveness and usage as separate facts:
process correlation says whether a worker is alive, while `data_recency` and
`activity_basis` say whether a fresh, recent, historical, or unmatched usage
checkpoint was observed. UI rows should stay session-first and stable as a run
moves between fresh and quiet states; state changes should update badges and
metrics in place instead of moving the run between unrelated process lists.

Provider adapters are allowed to be uneven. Codex and Claude currently expose
useful local metadata. Gemini/Antigravity and OpenCode/Pi may require additional
adapter research, a launcher mode, provider-side usage APIs, or explicit
configuration before they can expose the same fidelity.

The first-class visibility commands are:

- `curb dashboard` - show live agent workers and recent usage in one view.
- `curb usage` - summarize recent local usage.
- `curb usage --since 24h` - summarize a specific lookback window.
- `curb usage --json` - emit provider-neutral usage summaries for scripts.
- `curb tail` - stream new usage events as agents work.

Future commands should build on the same primitives:

- `curb inspect <session>` - show a session timeline by turn/request.
- `curb top` - rank current sessions by token velocity and expensive model use.

Usage watch evaluates only recent activity. A historical high-token session with
zero tokens in the current rolling window is visible in the dashboard but should
not trigger automatic warning or enforcement.

### Process Model

Curb runs as one long-lived local service plus a CLI:

- macOS: user `LaunchAgent` for visibility/alert mode; system `LaunchDaemon`
  for managed enforcement mode.
- Windows: per-user startup/background service for visibility/alert mode;
  Windows Service for managed enforcement mode.
- Linux: user `systemd --user` service for visibility/alert mode; system
  `systemd` service for managed enforcement mode.
- One service instance per machine scope, guarded by a lock file or local socket.
- CLI commands communicate with the service over a local IPC channel:
  Unix domain socket on macOS/Linux and named pipe on Windows.

Recommended macOS launch paths:

- User agent plist: `~/Library/LaunchAgents/com.curb.watchdog.plist`
- System daemon plist: `/Library/LaunchDaemons/com.curb.watchdog.plist`
- User state: `~/Library/Application Support/Curb`
- Managed state: `/Library/Application Support/Curb`
- Managed config: `/Library/Managed Preferences/com.curb.watchdog.yaml`

Recommended Windows launch paths:

- User config: `%LOCALAPPDATA%\\Curb\\config.yaml`
- User state: `%LOCALAPPDATA%\\Curb`
- Managed config: `%PROGRAMDATA%\\Curb\\config.yaml`
- Managed state: `%PROGRAMDATA%\\Curb`
- Service name: `CurbWatchdog`

Recommended Linux launch paths:

- User service: `~/.config/systemd/user/curb-watchdog.service`
- System service: `/etc/systemd/system/curb-watchdog.service`
- User config: `~/.config/curb/config.yaml`
- User state: `~/.local/state/curb`
- Managed config: `/etc/curb/config.yaml`
- Managed state: `/var/lib/curb`

### Loop Cadence

Initial defaults:

- Full process snapshot: every 15 seconds.
- Policy evaluation: every 5 seconds.
- Ledger heartbeat: every 60 seconds per active run.
- Config reload: every 60 seconds or on file-change signal where supported.

The service must use monotonic time for elapsed runtime calculations so
wall-clock changes do not shorten or extend policy windows. It should record
wall-clock timestamps for audit events.

## Detection Design

### Required Sources

Every platform adapter must expose a normalized process snapshot:

- PID.
- Parent PID.
- Process start time.
- Owner uid/user SID.
- Session id or logon session.
- Executable path.
- Command line where permitted.
- Current working directory where permitted.
- Process name.
- CPU and memory counters where permitted.

The macOS implementation should combine these sources:

- `NSWorkspace` / `NSRunningApplication` for running app metadata such as bundle
  identifier, executable URL, launch date, active state, and process identifier.
- `libproc` or equivalent process-table access for PID, PPID, executable path,
  command line, uid, start time, and elapsed runtime.
- Code-signature inspection for high-confidence desktop app identity when
  available.
- Filesystem path anchoring for known app bundle and helper executable paths.

The Windows implementation should combine these sources:

- Process table access for PID, parent PID, command line, executable path,
  owner SID, session id, and creation time.
- Installed application metadata where available.
- Authenticode signature publisher/thumbprint checks for high-confidence
  desktop app identity.
- Windows Service Control Manager integration for managed service deployment.
- Windows Job Objects or explicit descendant traversal for enforcement scope.

The Linux implementation should combine these sources:

- `/proc` process table data for PID, PPID, command line, executable path, uid,
  start time, cwd, and process state.
- Desktop file metadata where available for graphical desktop apps.
- systemd user/system service metadata where Curb itself is installed through
  systemd.
- cgroups/process groups where available for enforcement scope.

### Optional Sources

Optional sources can improve confidence but must not be required for the launch:

- Accessibility APIs for window title or foreground-window signals.
- Network endpoint/process correlation.
- Known app log locations.
- Endpoint Security Framework on managed enterprise devices.
- Windows ETW or Windows Event Log signals on managed devices.
- Linux auditd, journald, or eBPF-derived process telemetry on managed devices.

Accessibility, endpoint telemetry, ETW, auditd, journald, and eBPF signals must
degrade cleanly. If permissions are missing, Curb should record
`capability_unavailable` and continue with process/app detection.

### Matcher Precedence

Matchers are evaluated in this order:

1. Bundle identifier.
2. Code-signature identifier and Team ID.
3. Executable path or app bundle path.
4. Process tree anchored to a matched app root.
5. Process name.
6. Command-line regex.
7. Parent process name.
8. Window title regex.
9. Network endpoint/process correlation.

Bundle ID, code signature, and app-root process lineage are high-confidence
signals. Process names, command lines, window titles, and network domains are
secondary signals because they are more likely to drift or collide.

### Verified Local Defaults

The following defaults were verified on the local macOS machine used for this
spec on 2026-05-18. They are defaults, not hard-coded truth.

| Agent | Bundle ID | Executable | Team ID |
| --- | --- | --- | --- |
| Codex Desktop | `com.openai.codex` | `Codex` | `2DC432GLL2` |
| Claude Desktop | `com.anthropic.claudefordesktop` | `Claude` | `Q6L2SF6YDW` |

Observed Codex Desktop agent workers include descendants under:

- `/Applications/Codex.app/Contents/MacOS/Codex`
- `/Applications/Codex.app/Contents/Resources/codex app-server --analytics-default-enabled`
- `/Applications/Codex.app/Contents/Resources/codex app-server --listen stdio://`
- `/Applications/Codex.app/Contents/Resources/node_repl`

The spec must treat those as evidence for current implementation, not as the
only supported shape. Codex Desktop can launch multiple concurrent agent worker
trees, so Curb must track descendant workers separately under the Codex app root
when possible.

Windows and Linux desktop defaults must be verified during implementation on
representative machines. Until verified, those platforms should rely on
configurable executable paths, process names, command-line regexes, signature or
package metadata where available, and process lineage.

### Initial Agent Matchers

The launch config should ship with defaults equivalent to:

```yaml
agents:
  - id: codex-desktop
    label: Codex Desktop
    family: codex
    match:
      bundle_ids:
        - com.openai.codex
      code_signatures:
        - identifier: com.openai.codex
          team_id: 2DC432GLL2
      app_paths:
        - /Applications/Codex.app
      windows_paths: []
      linux_paths: []
      process_names:
        - Codex
        - codex
      command_regex:
        - "\\bcodex\\b"

  - id: codex-cli
    label: Codex CLI
    family: codex
    match:
      process_names:
        - codex
      command_regex:
        - "/codex(\\s|$)"
        - "@openai/codex"

  - id: claude-desktop
    label: Claude Desktop
    family: claude
    match:
      bundle_ids:
        - com.anthropic.claudefordesktop
      code_signatures:
        - identifier: com.anthropic.claudefordesktop
          team_id: Q6L2SF6YDW
      app_paths:
        - /Applications/Claude.app
      windows_paths: []
      linux_paths: []
      process_names:
        - Claude

  - id: claude-code
    label: Claude Code
    family: claude
    match:
      process_names:
        - claude
        - claude-code
      command_regex:
        - "\\bclaude\\b"
        - "\\bclaude-code\\b"
```

## Run Model

### Run

A run is a policy-tracked agent activity window.

Required fields:

- `run_id`
- `agent_id`
- `agent_family`
- `root_pid`
- `root_start_time`
- `owner_uid`
- `session_id`
- `hostname`
- `workspace_label`, if discoverable or configured
- `cwd`, if discoverable
- `match_evidence`
- `confidence`
- `started_at`
- `last_seen_at`
- `last_activity_at`
- `policy_profile`
- `state`
- `extension_count`
- `termination_reason`, if any

### Desktop Runs

For desktop applications, the app process lifetime and the agent work lifetime
are not always identical. Curb should use a two-level model:

- App root: the long-lived desktop app process.
- Agent workers: descendant worker trees, app-server processes, terminal helper
  processes, MCP servers, or other processes anchored to the app root.

If a distinct worker root is visible, Curb should track and enforce against the
worker tree first. If no worker root can be isolated and the app is the only
trackable unit, enforcement mode may terminate the whole app when policy
explicitly allows `kill_app_root: true`.

### CLI Runs

For CLI agents, the run root is the matched CLI process. Descendants are part of
the run while they remain under that root's PID/PPID tree and uid/session.

### Run Boundaries

Start a run when:

- A configured matcher reaches the run-start confidence threshold.
- The matched process survives `min_lifetime_seconds`.
- The same PID/start-time pair is not already attached to an active run.

Refresh a run when:

- The root process is still alive.
- A tracked descendant is alive.
- A configured activity signal is fresh.

End a run when:

- The root and tracked descendants are gone.
- No matching replacement process appears inside `max_run_gap_seconds`.
- The service has written a `run_stopped` event.

Sleep/wake behavior:

- Runtime should count real elapsed monotonic time by default.
- A future policy may subtract system sleep time, but the launch policy should
  assume an unattended sleeping laptop can wake and continue work.

## Activity Signals

Launch activity signals:

- Process existence.
- Process start time and elapsed time.
- CPU above configured threshold.
- Descendant process churn.
- File writes by matched process tree where available without content capture.
- Foreground app state from `NSRunningApplication`.

Optional activity signals:

- Window title changes through Accessibility.
- Network activity to configured vendor domains.
- New app log lines in known directories.

Activity is used for confidence and reporting. The kill threshold is based on
continuous run duration unless policy explicitly uses idle/runtime hybrid rules.

## Policy Model

### Core Policy Fields

```yaml
version: 1
profile: contractor-default
mode: visibility

defaults:
  warn_after: 90m
  kill_after: 120m
  ack_extension: 30m
  max_extensions: 2
  kill_grace_period: 60s
  cooldown_after_kill: 15m
  min_lifetime: 10s
  max_run_gap: 20s
  allow_app_root_kill: false

agents:
  - id: codex-desktop
    policy:
      warn_after: 90m
      kill_after: 120m
      ack_extension: 30m
      max_extensions: 1
      allow_app_root_kill: true

alerts:
  local_notifications: true
  webhook_url: null
  slack_webhook_url: null

ledger:
  path: /Library/Application Support/Curb/runs.ndjson
  include_prompt_content: false
```

### Duration Rules

- `warn_after` starts the warning lifecycle.
- `kill_after` starts enforcement only in enforcement mode.
- `ack_extension` extends `kill_after` and the next warning deadline.
- `max_extensions` caps user-driven extensions.
- `kill_grace_period` is the delay between final warning and termination.
- `cooldown_after_kill` blocks or flags immediate relaunches where the service
  can enforce or detect them.

Validation rules:

- `warn_after` must be less than `kill_after`.
- `kill_grace_period` must be positive in enforcement mode.
- Agent-level policy inherits from `defaults`.
- Unknown top-level keys should fail validation.
- Regex matchers must compile at config load.
- A policy that enables app-root termination must name the affected agents
  explicitly.

## State Machine

States:

- `observed`
- `active`
- `warned`
- `acknowledged`
- `grace`
- `terminating`
- `terminated`
- `stopped`
- `error`

Transitions:

1. `observed -> active`: matcher confidence is high enough and min lifetime
   passes.
2. `active -> warned`: elapsed runtime crosses `warn_after`.
3. `warned -> acknowledged`: user acknowledges within policy limits.
4. `acknowledged -> active`: extension is granted and deadlines are updated.
5. `warned -> grace`: elapsed runtime crosses `kill_after` in enforcement mode.
6. `grace -> terminating`: grace period expires and run is still alive.
7. `terminating -> terminated`: all targeted processes exit.
8. `terminating -> error`: termination fails or safety guard rejects the kill.
9. Any live state -> `stopped`: process tree exits naturally.

Visibility and alert modes never enter `terminating`; they emit
`would_terminate` events instead.

## Warning And Acknowledgement UX

Warnings must include:

- Agent label.
- Run ID.
- Elapsed runtime.
- Warning threshold.
- Kill threshold, when configured.
- Extension budget remaining.
- Exact CLI acknowledgement command.

Example:

```text
Codex Desktop run run_01JZ... has been active for 90m.
It will be terminated at 120m unless acknowledged.
Run: curb ack run_01JZ... --extend 30m
```

Acknowledgement command:

```sh
curb ack <run-id> --extend 30m --reason "still supervising"
```

Rules:

- Acknowledgement is only valid for active, warned, or grace states.
- Extension duration cannot exceed the configured `ack_extension`.
- Extension count cannot exceed `max_extensions`.
- Every acknowledgement writes an `ack_received` ledger event.
- Enforcement should re-check current state after acknowledgement to avoid
  racing with termination.

## Termination Semantics

### Scope Invariants

Curb must never kill outside the run's allowed scope.

Before terminating, the service must verify:

- PID and process start time still match the tracked process.
- Process uid matches the run owner unless running as managed root daemon.
- Process is still in the tracked descendant closure or approved app root.
- Process executable or ancestry still matches the agent evidence.
- PID is not Curb itself, launchd, a system daemon, or another denylisted
  process.

### Preferred Termination Order

For desktop app runs with an identifiable worker tree:

1. Warn.
2. Wait grace.
3. Send graceful termination to worker descendants, leaves first.
4. Re-check after `kill_grace_period`.
5. Send hard kill only to surviving tracked descendants.
6. Kill app root only if `allow_app_root_kill` is true and no smaller safe
   scope exists.

For CLI runs:

1. Warn.
2. Wait grace.
3. Send platform graceful termination to descendants leaves first, then root.
4. Re-check.
5. Send platform hard termination to remaining tracked processes.

For macOS app roots, the graceful operation should prefer app termination APIs
where possible before raw signals.

For Windows app roots, graceful termination should prefer window close or
application-specific shutdown where available, then terminate the tracked
process tree through the Windows adapter. Job Objects should be used when Curb
itself launches or owns a process tree; for externally launched desktop apps,
the adapter must rely on descendant traversal plus PID/start-time safety checks.

For Linux app roots, graceful termination should prefer `SIGTERM` to tracked
process groups, cgroups, or descendant sets, followed by `SIGKILL` only after
the configured grace period. If a desktop app cannot be isolated from unrelated
work, enforcement must fail closed to `would_terminate` unless policy explicitly
allows app-root termination.

### Relaunch Cooldown

In enforcement mode, if a terminated run relaunches inside
`cooldown_after_kill`, Curb should:

- Create a new run record.
- Emit `cooldown_violation`.
- Warn immediately.
- Terminate after grace if policy requires it.

Blocking relaunch is best-effort in v1; reliable blocking likely requires MDM,
endpoint security, or a supervised daemon.

## Ledger

The ledger is append-only NDJSON.

Required event types:

- `service_started`
- `config_loaded`
- `capability_unavailable`
- `run_started`
- `run_heartbeat`
- `policy_warning`
- `ack_received`
- `policy_violation`
- `grace_started`
- `termination_started`
- `termination_signal_sent`
- `termination_completed`
- `termination_failed`
- `would_terminate`
- `run_stopped`

Example:

```json
{
  "type": "policy_warning",
  "seq": 42,
  "ts": "2026-05-18T22:00:00Z",
  "run_id": "run_01JZ...",
  "agent_id": "codex-desktop",
  "elapsed_seconds": 5400,
  "warn_after_seconds": 5400,
  "kill_after_seconds": 7200,
  "mode": "alert",
  "action": "notify"
}
```

Ledger privacy rules:

- Do not store prompt text.
- Do not store response text.
- Do not store screenshots.
- Do not store file contents.
- Store process metadata, timing, policy, and enforcement evidence only.

Integrity hardening:

- Include a monotonically increasing `seq`.
- Include optional `prev_hash` and `event_hash`.
- Rotate on size or age.
- Write with owner-only permissions.

## CLI

Launch commands:

```sh
curb status
curb dashboard
curb usage [--since 24h] [--json]
curb runs
curb runs --active
curb inspect <run-id>
curb ack <run-id> --extend 30m --reason "still supervised"
curb validate-config [path]
curb policy [--json]
curb doctor
curb tail
curb stop <run-id> --reason "manual operator stop"
```

`curb stop` is allowed only when the current user owns the run or the CLI is
authorized for managed enforcement.

`curb doctor` should report daemon health, active config path, notification
availability, ledger writeability, optional Accessibility status, launchd
registration, and whether enforcement is capable of signaling a same-user test
process.

## Platform Permissions

Required for launch on all platforms:

- Process listing for same-user processes.
- Local notification permission for alert mode, where the OS requires consent.
- Termination permission for same-user CLI and desktop descendants.
- A writable state directory and ledger path.

macOS optional or managed permissions:

- Accessibility permission for window title or frontmost-window metadata.
- Root LaunchDaemon authorization for managed enforcement.
- MDM-delivered immutable config for enterprise rollout.
- Endpoint Security entitlement for deeper telemetry later.

Windows optional or managed permissions:

- Windows Service installation and management rights for managed enforcement.
- Administrator rights or endpoint-management deployment for cross-user
  enforcement.
- Event Log source registration for durable service events.
- Optional ETW collection rights for deeper telemetry later.

Linux optional or managed permissions:

- systemd user or system unit installation.
- Root privileges or policy-managed capabilities for cross-user enforcement.
- journald integration for service logs.
- Optional auditd/eBPF privileges for deeper telemetry later.

If optional permissions are missing, Curb should continue operating and write
capability events.

## Rollout Plan

### Phase 1: Local Visibility

Deliver:

- Cross-platform service skeleton with macOS, Windows, and Linux adapter
  interfaces.
- Working macOS, Windows, and Linux service implementations for visibility mode.
- Config loader and validator.
- Codex Desktop, Codex CLI, Claude Desktop, and Claude Code matchers.
- Active run detection.
- NDJSON ledger.
- `curb status`, `curb runs`, and `curb inspect`.

Acceptance:

- Detect a running Codex Desktop app by bundle ID.
- Detect Codex Desktop worker descendants launched by the app.
- Detect a CLI `codex` process.
- Detect Claude Desktop by bundle ID.
- Detect a CLI `claude` process.
- Run the same policy/state-machine tests against macOS, Windows, and Linux
  fake process snapshots.
- Run a real harmless synthetic long-running process on macOS, Windows, and
  Linux and track it through the same ledger lifecycle.
- Record run start, heartbeat, and stop events without prompt content.

### Phase 2: Alert And Acknowledgement

Deliver:

- Local notification warnings.
- `curb ack`.
- Extension budget enforcement.
- `would_terminate` events in alert mode.
- Webhook adapter behind config.

Acceptance:

- Warn at `warn_after`.
- Extend once when acknowledgement is valid.
- Refuse extension after `max_extensions`.
- Continue without Accessibility permission.
- Emit complete ledger evidence for each warning and ack.

### Phase 3: Enforcement

Deliver:

- Process-tree termination.
- Grace period.
- Platform-specific graceful termination followed by hard termination.
- App-root termination only when explicitly allowed.
- Relaunch cooldown detection.
- System LaunchDaemon, Windows Service, and systemd packaging notes.

Acceptance:

- Kill a synthetic CLI run after `kill_after`.
- Kill only tracked descendants, not unrelated sibling processes.
- Refuse to kill when PID/start-time safety checks fail.
- In visibility/alert mode, never kill and emit `would_terminate`.
- In enforcement mode, write termination proof or failure evidence.

## Launch Oracle

The launch implementation is ready when all of these are true:

- `curb validate-config` passes for the documented example config.
- A fake Codex Desktop process snapshot creates a run, warning, violation, and
  complete ledger trail.
- A real running Codex Desktop app is detected by bundle ID on macOS.
- At least one Codex Desktop descendant worker tree is attached to the Codex app
  root when desktop-launched agents are active.
- A fake Claude Desktop process snapshot is detected by bundle ID.
- Windows and Linux fake snapshots exercise the same matcher, policy, warning,
  ack, and enforcement state-machine behavior.
- Visibility and alert modes cannot call the termination adapter in tests.
- Enforcement mode calls the termination adapter only after violation plus
  grace.
- Termination safety checks reject stale PID/start-time pairs.
- `curb runs --json` can reconstruct active and completed runs from the ledger.
- No launch event contains prompt text, response text, screenshots, keystrokes,
  or file contents by default.

## Test Plan

Unit tests:

- Duration parser.
- Config validation.
- Matcher scoring and precedence.
- Policy inheritance.
- Run state transitions.
- Ack extension accounting.
- Termination target filtering.

Integration tests:

- Synthetic process tree detection.
- PID reuse safety.
- Worker descendant churn.
- Sleep/resume monotonic time handling.
- Multiple concurrent runs for the same agent family.
- Missing optional permissions.
- Ledger append and rotation.

Manual macOS smoke tests:

- Codex Desktop running with multiple desktop-launched agents.
- Codex CLI running from a terminal.
- Claude Desktop running.
- Claude Code running from a terminal.
- Local notification delivery.
- Enforcement against a harmless synthetic long-running process.

Manual Windows smoke tests:

- Curb runs as a foreground process and as a Windows Service.
- Codex/Claude CLI-style synthetic process snapshots are detected.
- Toast or fallback notification delivery works.
- Enforcement kills only a harmless synthetic descendant tree.
- Ledger writes to `%PROGRAMDATA%\\Curb` in managed mode.

Manual Linux smoke tests:

- Curb runs as a foreground process and as a systemd user service.
- Codex/Claude CLI-style synthetic process snapshots are detected through
  `/proc`.
- Desktop notification or CLI fallback warning works.
- Enforcement kills only a harmless synthetic process group.
- Ledger writes to `/var/lib/curb` in managed mode.

## Open Questions

- Should Codex Desktop enforcement default to worker-tree-only, app-root kill,
  or alert-only until worker isolation is proven on more machines?
- What should the default launch policy be: warn at 90 minutes and kill at
  120 minutes, or a stricter contractor profile?
- Should system sleep time count toward runtime in all managed profiles?
- Which enterprise collector should be first: Slack, generic webhook, SIEM, or a
  local export file?
- How should browser-only agent sessions be represented without intrusive
  browser inspection?
- Should Curb expose a menubar app in launch, or keep launch focused on service
  plus CLI?
- Which Windows desktop app identifiers and Authenticode publishers should ship
  as verified defaults for Codex and Claude?
- Which Linux packaging formats should launch first: Homebrew/Linuxbrew, deb,
  rpm, tarball, or distro package?

## Research Anchors

- Apple [`NSRunningApplication`](https://developer.apple.com/documentation/appkit/nsrunningapplication)
  / `NSWorkspace` APIs provide bundle ID, executable URL, launch date, active
  state, process identifier, and termination methods for running applications.
- Apple's
  [launchd job documentation](https://developer.apple.com/library/archive/documentation/MacOSX/Conceptual/BPSystemStartup/Chapters/CreatingLaunchdJobs.html)
  covers user LaunchAgents and system LaunchDaemons configured through property
  lists with keys such as `Label`, `ProgramArguments`, and `KeepAlive`.
- Microsoft
  [Windows Service documentation](https://learn.microsoft.com/en-us/windows/win32/services/about-services)
  describes the Service Control Manager as the Windows control plane for
  installed services.
- Microsoft
  [.NET Worker Service documentation](https://learn.microsoft.com/en-us/dotnet/core/extensions/windows-service)
  documents one practical path for publishing a long-running worker as a
  Windows Service.
- freedesktop.org
  [`systemd.service`](https://www.freedesktop.org/software/systemd/man/latest/systemd.service.html)
  documents service units, `ExecStart`, restart behavior, and service lifecycle
  controls for Linux deployments.
- OpenAI
  [Codex governance](https://developers.openai.com/codex/enterprise/governance)
  exposes analytics and compliance exports, but usage analytics can lag and
  should not be the only real-time stop for runaway local work.
- OpenAI
  [Codex admin setup](https://developers.openai.com/codex/enterprise/admin-setup)
  says managed policies apply across local Codex surfaces for ChatGPT-signed-in
  users, including the Codex app, CLI, and IDE extension.
- Anthropic
  [Claude Code settings](https://docs.anthropic.com/en/docs/claude-code/settings)
  and [hooks](https://docs.anthropic.com/en/docs/claude-code/hooks) support
  settings, hooks, permissions, and enterprise managed policy files; those are
  useful companion controls but do not replace local process watchdog behavior.
