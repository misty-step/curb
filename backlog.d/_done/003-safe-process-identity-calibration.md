---
id: 003-safe-process-identity-calibration
title: Calibrate safe process identity against live Codex and Claude workers
status: done
lifecycle_stage: Policy/Eval
acceptance:
    - Curb distinguishes desktop app roots from killable worker or CLI process trees for Codex Desktop, Codex CLI, Claude Desktop, and Claude Code.
    - Matchers use a confidence tuple that includes executable path, command line shape, parent lineage, user, cwd/worktree where available, PID, and process start time.
    - Visibility and alert modes cannot terminate processes, even under aggressive presets.
    - Enforcement tests cover desktop roots, worker children, renamed/versioned Claude Code binaries, and ambiguous low-confidence matches.
evidence_required:
    - go test ./...
    - go test -race ./...
    - go build -o /tmp/curb-darwin ./cmd/curb
    - /tmp/curb-darwin scan --config configs/curb.example.yaml
---

## Problem

Recent live testing showed the dangerous edge clearly: Curb must control agent
work without killing the host desktop app. Process names alone are insufficient.
Codex Desktop workers can appear as `/Applications/Codex.app/.../codex
app-server --listen stdio://`; Claude Code may appear under a versioned
executable path while the command line starts with `claude`.

## Desired Behavior

- Desktop app roots are watch-only or ignored unless a managed profile
  explicitly says otherwise.
- Worker subprocesses and CLI sessions can be enforcement targets when identity
  confidence is high enough.
- Ambiguous matches warn and record evidence, but do not terminate.
- The matcher explains why it matched and why it is killable or not killable.

## Implementation Notes

- Preserve PID plus process start time as the termination identity boundary.
- Add fixture snapshots for real process shapes observed on macOS.
- Treat command-line and process-name signals as secondary.
- Prefer explicit match reasons in scan output so operators can validate the
  rule before enabling enforcement.

## What Was Built

- Added match evidence for PID, parent lineage, executable path, cwd, user, and
  process start time, and exposed that evidence in human `curb scan` output.
- Tightened command-regex scoring so environment variables or incidental text
  mentioning `codex` or `claude-code` do not become agent matches.
- Redacted process command lines from `curb scan --json` and watchdog
  `run_started` ledger events so process-table inspection does not leak
  inherited secrets.
- Added fixture coverage for Codex Desktop roots, Codex Desktop workers, Claude
  Desktop roots, versioned Claude Code executables, ambiguous mention-only
  matches, Cursor helper false positives, and alert-mode non-termination.

## Acceptance Evidence

- `go test ./...`
- `go test -race ./...`
- `go build -o /tmp/curb-darwin ./cmd/curb`
- `/tmp/curb-darwin scan --config configs/curb.example.yaml`
