#!/usr/bin/env python3
"""Fail on high-confidence secret material in repo text files."""

from __future__ import annotations

import os
import re
import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parents[1]
MAX_BYTES = 1_000_000

SKIP_DIR_PARTS = {
    ".git",
    "node_modules",
    "target",
    "dist",
    "src-tauri/target",
    "ui/artifacts",
    "demo/006/artifacts",
}

SKIP_SUFFIXES = {
    ".bmp",
    ".gif",
    ".ico",
    ".jpeg",
    ".jpg",
    ".lock",
    ".pdf",
    ".png",
    ".wasm",
    ".webp",
    ".zip",
}

ALLOWLIST_SUBSTRINGS = {
    "Bearer $(cat token-file)",
    "Bearer test-token",
    "curb_token=test-token",
    "token: \"secret\"",
    "secret-session",
    "existing-token",
    "test-token",
}

SECRET_PATTERNS = [
    ("private key block", re.compile(r"-----BEGIN (?:RSA |EC |OPENSSH |DSA |PGP )?PRIVATE KEY-----")),
    ("openai api key", re.compile(r"\bsk-[A-Za-z0-9_-]{32,}\b")),
    ("anthropic api key", re.compile(r"\bsk-ant-[A-Za-z0-9_-]{32,}\b")),
    ("github token", re.compile(r"\bgh[pousr]_[A-Za-z0-9_]{30,}\b")),
    ("aws access key", re.compile(r"\b(?:AKIA|ASIA)[A-Z0-9]{16}\b")),
    ("slack token", re.compile(r"\bxox(?:b|p|o|a|r)-[A-Za-z0-9-]{20,}\b")),
]


def repo_files() -> list[Path]:
    result = subprocess.run(
        ["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"],
        cwd=ROOT,
        check=True,
        stdout=subprocess.PIPE,
    )
    return [ROOT / raw.decode("utf-8") for raw in result.stdout.split(b"\0") if raw]


def should_skip(path: Path) -> bool:
    rel = path.relative_to(ROOT).as_posix()
    if any(part in rel.split("/") for part in SKIP_DIR_PARTS):
        return True
    if any(rel.startswith(f"{part}/") for part in SKIP_DIR_PARTS if "/" in part):
        return True
    if path.suffix.lower() in SKIP_SUFFIXES:
        return True
    try:
        return path.stat().st_size > MAX_BYTES
    except OSError:
        return True


def read_text(path: Path) -> str | None:
    try:
        raw = path.read_bytes()
    except OSError:
        return None
    if b"\0" in raw:
        return None
    try:
        return raw.decode("utf-8")
    except UnicodeDecodeError:
        return None


def line_is_allowed(line: str) -> bool:
    return any(marker in line for marker in ALLOWLIST_SUBSTRINGS)


def scan_file(path: Path) -> list[tuple[int, str, str]]:
    text = read_text(path)
    if text is None:
        return []
    findings: list[tuple[int, str, str]] = []
    for line_number, line in enumerate(text.splitlines(), 1):
        if line_is_allowed(line):
            continue
        for name, pattern in SECRET_PATTERNS:
            if pattern.search(line):
                findings.append((line_number, name, line.strip()[:160]))
    return findings


def main() -> int:
    findings: list[str] = []
    for path in repo_files():
        if should_skip(path):
            continue
        for line_number, name, snippet in scan_file(path):
            rel = path.relative_to(ROOT).as_posix()
            findings.append(f"{rel}:{line_number}: {name}: {snippet}")
    if findings:
        print("check-secrets: high-confidence secret material found", file=sys.stderr)
        for finding in findings:
            print(finding, file=sys.stderr)
        return 1
    print("check-secrets: ok")
    return 0


if __name__ == "__main__":
    os.chdir(ROOT)
    raise SystemExit(main())
