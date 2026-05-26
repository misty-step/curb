# Curb Local Agent Watchdog Spec

Date: 2026-05-18
Status: draft

## Summary

Curb is a configurable local watchdog for AI agent applications running on
developer machines. It starts as a visibility tool and grows into an enforcement
agent for managed enterprise environments.

The first production target is macOS laptops where engineers run official
desktop applications such as Codex, Claude, Quadcode, Pi, OpenCode, Droid, and
other local agent clients. Because many sessions run through desktop apps rather
than CLIs, Curb must not depend on wrapping `codex` or any single binary.
It should observe local processes and app activity, maintain a run ledger, alert
on risky behavior, and optionally terminate agent processes that violate policy.

## Naming

Working repo and product name: `curb`.

Why this name:

- Short and operational.
- Fits existing local naming patterns such as `canary`, `cadence`, `gradient`,
  `cerberus`, `apollo`, and `olympus`.
- Not tied to Codex specifically.
- Suitable for both an open core and an enterprise-managed wrapper.

## Problem

AI agent desktop apps can continue working long after the operator stops paying
attention. In the incident that motivated this spec, a Friday Codex goal kept
running until Monday, roughly 40 hours later. The issue is systemic: if an
agent can run unattended and unbounded, the organization needs a control layer.

Human reminders are not adequate controls. Curb should make long-running
agent work visible by default and mechanically enforce limits where the
organization owns the device or has endpoint-management authority.

## Goals

- Detect configured local agent applications and processes.
- Track runtime, idle time, user presence, process lineage, and basic activity.
- Maintain a local append-only run ledger.
- Support visibility-only mode for evaluation.
- Support alerting mode for early rollout.
- Support enforcement mode that terminates processes after policy violations.
- Support centrally managed configuration on company-managed machines.
- Avoid vendor lock-in to Codex.
- Provide a small, auditable core that can later be wrapped for enterprise use.

## Non-Goals

- Do not implement a full DLP or SIEM product.
- Do not inspect private prompt content by default.
- Do not rely on OpenAI Enterprise admin access.
- Do not require CLI wrappers for desktop-app monitoring.
- Do not try to perfectly estimate token spend in v1.
- Do not silently kill processes in visibility-only mode.

## Target Platforms

Initial:

- macOS 14+.
- Company-managed laptops via LaunchAgent/LaunchDaemon and MDM-delivered config.

Later:

- Windows service.
- Linux systemd user service.

## Agent Inventory

Curb must be configured from a list of agent matchers. Example initial
targets:

- Codex desktop app.
- Codex CLI.
- Claude desktop app.
- Claude Code.
- Quadcode.
- Pi.
- OpenCode.
- Droid.
- Factory.
- Cursor/agent modes, if configured.
- Any custom process names, bundle identifiers, paths, or network domains added
  by policy.

Matchers should support:

- Process name.
- Executable path.
- macOS bundle identifier.
- Parent process name.
- Command-line substring or regex.
- Open window title regex, where accessibility permissions allow it.
- Network endpoint/process correlation, where endpoint tooling supports it.

## Runtime Modes

### Visibility Mode

Default for local experimentation.

Behavior:

- Detect matching agent processes.
- Create run records.
- Track elapsed runtime.
- Track activity signals.
- Write local logs.
- Print or display warnings.
- Never kill processes.

### Alert Mode

Default for early team rollout.

Behavior:

- Everything in visibility mode.
- Send local notifications.
- Optionally send Slack/email/webhook alerts.
- Escalate after missed acknowledgement.
- Still does not kill unless explicitly configured.

### Enforcement Mode

Default for mature managed-device rollout.

Behavior:

- Everything in alert mode.
- Terminate matching agent process trees when policy is violated.
- Record termination reason and evidence.
- Optionally block relaunch for a cooldown window.

## Core Policy Concepts

### Session

A session is a contiguous period where a configured agent process appears active.
For desktop apps, this may be an app process plus inferred active work. For CLI
agents, this may be a process tree rooted at a known binary.

### Run

A run is a policy-tracked session with metadata:

- Run ID.
- Agent family.
- Process IDs.
- Bundle ID / executable path.
- Start time.
- Last observed activity.
- User.
- Hostname.
- CWD where available.
- Client/workspace label where configured.
- Policy profile.
- Current state.
- Termination reason, if any.

### Activity

Activity is any signal that suggests the agent or user is doing work:

- Process CPU above threshold.
- Network activity to configured domains.
- File writes in watched directories.
- New log lines in known app log locations.
- Foreground window activity.
- User keyboard/mouse activity.
- CLI stdout/stderr activity where process-owned output is observable.

### Human Acknowledgement

Some policies should warn before killing:

- "Codex has been active for 90 minutes. Acknowledge within 15 minutes to extend
  by 30 minutes."

Acknowledgement mechanisms:

- Local menubar app.
- CLI command: `curb ack <run-id> --extend 30m`.
- Local notification action.
- Webhook callback, later.

## Default Policy Profile

Initial recommended profile:

```yaml
profile: contractor-default
mode: visibility
max_continuous_runtime: 2h
max_unacknowledged_runtime: 90m
ack_extension: 30m
max_extensions: 2
idle_user_threshold: 30m
after_hours_policy: alert
weekend_policy: alert
kill_grace_period: 60s
cooldown_after_kill: 15m
```

Enterprise enforcement profile:

```yaml
profile: brant-managed
mode: enforcement
max_continuous_runtime: 2h
max_unacknowledged_runtime: 90m
ack_extension: 30m
max_extensions: 1
idle_user_threshold: 30m
after_hours_policy: kill_after_grace
weekend_policy: kill_after_grace
kill_grace_period: 120s
cooldown_after_kill: 30m
```

## Configuration Shape

Example:

```yaml
version: 1
device_policy_id: brant-managed-v1
mode: enforcement

agents:
  - id: codex-desktop
    label: Codex Desktop
    match:
      bundle_ids:
        - com.openai.codex
      process_names:
        - Codex
        - codex
      command_regex:
        - "\\bcodex\\b"

  - id: claude
    label: Claude
    match:
      process_names:
        - Claude
        - claude
        - claude-code

  - id: opencode
    label: OpenCode
    match:
      process_names:
        - opencode

policies:
  default:
    max_continuous_runtime: 2h
    max_unacknowledged_runtime: 90m
    idle_user_threshold: 30m
    after_hours:
      timezone: America/Chicago
      windows:
        - "Mon-Fri 08:00-18:00"
      outside_window: alert
    weekend: alert
    enforcement:
      grace_period: 60s
      signal_order:
        - TERM
        - KILL

alerts:
  local_notifications: true
  webhook_url: null
  slack_webhook_url: null

ledger:
  path: /Library/Application Support/Curb/runs.ndjson
  include_prompt_content: false
```

## Enforcement Semantics

Curb should use progressive enforcement:

1. Detect policy violation.
2. Record violation in ledger.
3. Warn user locally.
4. Wait grace period if configured.
5. Re-check process state.
6. Terminate process tree with graceful signal.
7. Escalate to hard kill if needed.
8. Record final state.
9. Optionally block relaunch for cooldown.

Desktop app roots are watch-only by default. Enforcement is for agent worker
processes such as Codex CLI and Claude Code; closing an entire desktop app is
too blunt for the local launch profile.

## Observability

Curb should emit local NDJSON records:

```json
{
  "type": "run_started",
  "run_id": "run_...",
  "agent_id": "codex-desktop",
  "pid": 12345,
  "bundle_id": "com.openai.codex",
  "started_at": "2026-05-18T20:00:00Z",
  "policy_profile": "contractor-default"
}
```

```json
{
  "type": "policy_violation",
  "run_id": "run_...",
  "violation": "max_continuous_runtime",
  "observed_runtime_seconds": 7201,
  "limit_seconds": 7200,
  "action": "alert"
}
```

The ledger should be append-only by default. Enterprise deployments can forward
events to SIEM, Slack, or a managed collector.

## Architecture

Suggested modules:

- `curb-core`: policy evaluation, run state machine, config schema.
- `curb-macos`: process discovery, LaunchAgent integration, notification
  adapter, termination adapter.
- `curb-ledger`: append-only local ledger and export.
- `curb-alerts`: local notification, webhook, Slack.
- `curb-cli`: inspect runs, acknowledge warnings, validate config.
- `curb-enterprise`: future opinionated packaging, signing, MDM profile
  examples, and default policies.

## macOS Implementation Notes

Process discovery options:

- `NSWorkspace` running applications for bundle identifiers and app metadata.
- `ps` / libproc for PID, parent PID, executable, command-line, and runtime.
- Endpoint Security Framework later for stronger enterprise telemetry.
- Accessibility APIs only when explicitly enabled for window title/activity.

Deployment options:

- User LaunchAgent for visibility/alert mode.
- System LaunchDaemon for stronger enforcement on managed devices.
- Signed/notarized binary for enterprise deployment.
- MDM-managed configuration profile or root-owned config file.

## Security And Privacy

Default privacy posture:

- Do not capture prompt or response content.
- Do not capture file contents.
- Do not capture screenshots.
- Store only process, timing, policy, and enforcement metadata.
- Make content capture an explicit enterprise opt-in, not a default.

Threat model:

- Users may try to rename binaries.
- Users may launch agents from unexpected paths.
- Users may use unmanaged machines.
- Users may use browser-based agents that do not expose distinct local process
  names.
- Vendor app process names and bundle IDs may change.

Mitigations:

- Multiple matcher types.
- Config update channel.
- Signed managed config.
- Root-owned config in enterprise mode.
- Browser-domain/network matching as a later enhancement.

## MVP

MVP should deliver:

- macOS daemon/agent that scans every 30 seconds.
- YAML config with process-name and bundle-ID matchers.
- Visibility mode.
- NDJSON run ledger.
- Local CLI:
  - `curb status`
  - `curb runs`
  - `curb validate-config`
- Alert mode with macOS notifications.
- Enforcement mode with process-tree termination.
- Basic tests for policy evaluation.

## Future Work

- Menubar app.
- MDM configuration examples.
- SIEM/webhook collector.
- Browser tab/domain detection.
- Endpoint Security integration.
- User acknowledgement workflow.
- Per-client/team policy profiles.
- Internal usage dashboard.
- Optional integration with OpenAI/Codex analytics exports.

## Open Questions

- Which exact process names and bundle identifiers do Codex desktop, Quadcode,
  Pi, OpenCode, Droid, and related tools expose on macOS today?
- Should enforcement kill the entire desktop app or only active helper
  processes when helper processes are distinguishable?
- What is the right default max runtime: 90 minutes, 2 hours, or 4 hours?
- Should after-hours work always be killed or only require explicit
  acknowledgement?
- How should Curb handle browser-based agent sessions?
- What central collector should enterprise deployments use first: Slack,
  webhook, SIEM, or a simple internal API?
