#!/usr/bin/env bash
set -euo pipefail

ROOT="${CURB_CHECK_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
cd "$ROOT"

python3 - <<'PY'
from __future__ import annotations

import re
import sys
from pathlib import Path

ROOT = Path.cwd()
ALLOWED_TERMINATION_FILE = Path("curb-core/src/platform/termination.rs")
PRODUCTION_ROOTS = [Path("curb-core/src"), Path("src")]
ALLOWED_TERMINATION_MECHANICS = {
    "soft_terminate",
    "hard_terminate",
    "signal_with_kill",
    "unix_signal_command",
    "windows_taskkill_command",
    "pid_alive",
}
INTEGER_TYPES = (
    "i8",
    "i16",
    "i32",
    "i64",
    "i128",
    "isize",
    "u8",
    "u16",
    "u32",
    "u64",
    "u128",
    "usize",
)
OS_TERMINATION_PATTERNS = [
    ("/bin/kill", r"/bin/kill"),
    ("taskkill", r"taskkill(?:\.exe)?"),
    ("SIGTERM", r"\bSIGTERM\b"),
    ("SIGKILL", r"\bSIGKILL\b"),
    ("Command::new kill", r"Command::new\(\s*['\"](?:/bin/)?kill['\"]\s*\)"),
    ("Command::new taskkill", r"Command::new\(\s*['\"][^'\"]*taskkill(?:\.exe)?['\"]\s*\)"),
    ("kill signal arg", r"\.arg\(\s*['\"]-(?:TERM|KILL)['\"]\s*\)"),
    ("shell kill command", r"\bkill\s+-(?:TERM|KILL)\b"),
    ("libc kill", r"\blibc::kill\b"),
    ("nix signal kill", r"\bnix::sys::signal::kill\b"),
    ("module signal kill", r"\bsignal::kill\b"),
    ("direct kill call", r"\bkill\s*\("),
    ("aliased libc kill import", r"use\s+libc::[^;]*\bkill\b"),
    ("aliased nix kill import", r"use\s+nix::sys::signal::[^;]*\bkill\b"),
    ("aliased libc signal import", r"use\s+libc::[^;]*\bSIG(?:TERM|KILL)\b"),
    ("aliased nix signal import", r"use\s+nix::sys::signal::[^;]*\bSIG(?:TERM|KILL)\b"),
    ("nix signal enum", r"\bSignal::SIG(?:TERM|KILL)\b"),
]


def production_files() -> list[Path]:
    files: list[Path] = []
    for root in PRODUCTION_ROOTS:
        path = ROOT / root
        if not path.exists():
            continue
        for file in path.rglob("*.rs"):
            rel = file.relative_to(ROOT)
            if file.name == "tests.rs" or "tests" in rel.parts:
                continue
            files.append(rel)
    return sorted(files)


def strip_comments(source: str) -> str:
    source = re.sub(r"/\*.*?\*/", "", source, flags=re.DOTALL)
    return re.sub(r"//.*", "", source)


def signature_blocks(source: str) -> list[tuple[str, str]]:
    blocks: list[tuple[str, str]] = []
    pattern = re.compile(
        r"(?m)^\s*(?:pub(?:\([^)]*\))?\s+)?(?:async\s+)?fn\s+([A-Za-z_][A-Za-z0-9_]*)\s*\("
    )
    for match in pattern.finditer(source):
        depth = 0
        end = match.end()
        while end < len(source):
            char = source[end]
            if char == "(":
                depth += 1
            elif char == ")":
                if depth == 0:
                    end += 1
                    break
                depth -= 1
            end += 1
        while end < len(source) and source[end] not in "{;":
            end += 1
        blocks.append((match.group(1), source[match.start() : end]))
    return blocks


def has_pid_like_parameter(signature: str) -> bool:
    normalized = " ".join(signature.split())
    if re.search(r"\bpid\b", normalized, flags=re.IGNORECASE):
        return True
    if re.search(r"[:<,]\s*(?:platform::)?Pid\b", normalized):
        return True
    return False


def has_integer_parameter(signature: str) -> bool:
    normalized = " ".join(signature.split())
    integer_pattern = "|".join(INTEGER_TYPES)
    return bool(re.search(rf"[:<,]\s*(?:{integer_pattern})\b", normalized))


def is_termination_api(name: str) -> bool:
    lowered = name.lower()
    return any(word in lowered for word in ("stop", "kill", "terminate"))


def is_strong_termination_action(name: str) -> bool:
    lowered = name.lower()
    if any(word in lowered for word in ("outcome", "event", "emit", "status", "view")):
        return False
    return lowered in {"stop", "kill", "terminate"} or lowered.startswith(
        ("stop_", "kill_", "terminate_")
    )


def add_failure(failures: list[str], label: str, detail: str) -> None:
    failures.append(f"FAIL: {label}\n{detail}")


failures: list[str] = []
files = production_files()

platform_source = strip_comments((ROOT / "curb-core/src/platform.rs").read_text())
termination_source = strip_comments((ROOT / ALLOWED_TERMINATION_FILE).read_text())

platform_terminate = [
    signature
    for name, signature in signature_blocks(platform_source)
    if name == "terminate" and "TerminationTarget" in signature
]
if not platform_terminate:
    add_failure(
        failures,
        "Platform::terminate must accept only a sealed TerminationTarget",
        "curb-core/src/platform.rs has no terminate signature containing TerminationTarget",
    )

termination_tree = [
    signature
    for name, signature in signature_blocks(termination_source)
    if name == "terminate_tree" and "TerminationTarget" in signature
]
if not termination_tree:
    add_failure(
        failures,
        "platform termination execution must start from a sealed TerminationTarget",
        "curb-core/src/platform/termination.rs has no terminate_tree signature containing TerminationTarget",
    )

for rel in files:
    source = strip_comments((ROOT / rel).read_text())
    for name, signature in signature_blocks(source):
        if rel == ALLOWED_TERMINATION_FILE and name in ALLOWED_TERMINATION_MECHANICS:
            continue
        if is_termination_api(name) and has_pid_like_parameter(signature):
            add_failure(
                failures,
                "public or domain stop/kill/terminate APIs must not accept a bare PID or integer",
                f"{rel}: {signature.strip()}",
            )
        elif is_strong_termination_action(name) and has_integer_parameter(signature):
            add_failure(
                failures,
                "public or domain stop/kill/terminate APIs must not accept a bare PID or integer",
                f"{rel}: {signature.strip()}",
            )
    if rel != ALLOWED_TERMINATION_FILE:
        for label, pattern in OS_TERMINATION_PATTERNS:
            if re.search(pattern, source):
                add_failure(
                    failures,
                    "OS termination commands must remain isolated to platform/termination.rs",
                    f"{rel}: matches {label!r}",
                )

if failures:
    print("\n".join(failures))
    print(
        f"check-termination-boundary: {len(failures)} violation(s); "
        "keep termination behind platform::TerminationTarget"
    )
    sys.exit(1)

print("check-termination-boundary: ok (production termination remains sealed behind TerminationTarget)")
PY
