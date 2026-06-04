#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

MODE="${1:---offline}"

case "$MODE" in
  --offline)
    if ! command -v cargo-audit >/dev/null 2>&1; then
      echo "dependency-audit: cargo-audit is not installed; skipping offline Rust advisory check" >&2
      echo "dependency-audit: run scripts/check-dependency-audit.sh --online for the mandatory CI audit" >&2
      exit 0
    fi
    cargo audit --no-fetch --stale --deny warnings
    echo "dependency-audit: offline Rust advisory check ok"
    echo "dependency-audit: npm audit is registry-backed; run --online for npm advisory proof"
    ;;
  --online)
    if ! command -v cargo-audit >/dev/null 2>&1; then
      echo "dependency-audit: missing cargo-audit; install it before running the online audit" >&2
      exit 1
    fi
    cargo audit --deny warnings
    (cd ui && npm audit --audit-level=high --package-lock-only)
    echo "dependency-audit: online Rust and npm advisory checks ok"
    ;;
  *)
    echo "usage: scripts/check-dependency-audit.sh [--offline|--online]" >&2
    exit 2
    ;;
esac
