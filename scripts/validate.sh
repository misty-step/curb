#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

bash "$ROOT/scripts/check-fast.sh"
bash "$ROOT/scripts/check-desktop.sh"
bash demo/006/script/run-backlog-006-demo.sh --dry-run
