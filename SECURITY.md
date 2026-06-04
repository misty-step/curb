# Security Policy

Curb is a local endpoint watchdog. Security issues are primarily about privacy,
process-safety, and preserving local enforcement boundaries.

## Report A Vulnerability

Do not post suspected secrets, prompt content, response content, screenshots,
keystrokes, or private file contents in public issues. Report enough metadata
to reproduce the issue without exposing sensitive local data:

- affected command, route, or configuration file,
- expected behavior,
- observed behavior,
- operating system and Curb version or commit,
- minimal synthetic repro data when possible.

## Security Invariants

- Prompt, response, screenshot, keystroke, and file-content capture are rejected
  by default.
- Visibility and alert modes must never terminate processes.
- Production termination APIs must not accept bare PIDs.
- Termination authority requires PID, start time, owner, executable or app
  identity, and a fresh platform revalidation.
- Desktop app roots are not enforcement targets; only correlated worker or CLI
  processes may be stopped.
- Remote systems may receive metadata-only advisory events, but local endpoint
  policy remains authoritative for enforcement.

## Local Security Gates

Run the normal pre-merge gate:

```sh
scripts/validate.sh
```

The fast gate includes an offline high-confidence secret scan:

```sh
python3 scripts/check-secrets.py
```

The scanner checks tracked and untracked non-ignored text files for private key
blocks and common API-token formats. Synthetic test values such as
`test-token` are allowed; real credentials, raw provider logs, prompt content,
response content, screenshots, keystrokes, and private file contents must stay
out of the repository.

Dependency-advisory auditing runs in CI and can be reproduced locally:

```sh
scripts/check-dependency-audit.sh --online
```

That command refreshes RustSec advisory data through `cargo audit` and checks
the UI lockfile with `npm audit --audit-level=high --package-lock-only`.
For a cache-only Rust check that avoids registry/network dependency during local
inner loops, run:

```sh
scripts/check-dependency-audit.sh --offline
```

Dependency changes must preserve existing lockfiles, pass `scripts/validate.sh`,
and pass the online advisory audit before merge.

Advisory waivers are intentionally narrow. If a RustSec or npm advisory is not
actionable, record the advisory id, affected package, reason, owner, expiry
date, and compensating control in the backlog item or PR that keeps the
dependency. Expired waivers must be removed or renewed before merge.
