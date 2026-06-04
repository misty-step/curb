#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

need_command() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "missing required command: $1" >&2
    exit 1
  fi
}

need_file() {
  if [ ! -f "$1" ]; then
    echo "missing required file: $1" >&2
    exit 1
  fi
}

need_dir() {
  if [ ! -d "$1" ]; then
    echo "missing required directory: $1" >&2
    exit 1
  fi
}

need_command cargo
need_command rustc
need_command npm
need_command node

need_file Cargo.lock
need_file ui/package-lock.json
need_file configs/curb.example.yaml
need_file web/dist/index.html
need_dir ui/node_modules

node -e 'const major = Number(process.versions.node.split(".")[0]); if (major < 22) { console.error(`node ${process.versions.node} is too old; expected >=22`); process.exit(1); }'
cargo metadata --format-version 1 --no-deps >/dev/null
(cd ui && npm exec -- tsc --version >/dev/null)
bash -n "$ROOT/scripts/install-git-hooks.sh"
bash -n "$ROOT/scripts/git-hooks/pre-commit"
cargo run -- validate-config configs/curb.example.yaml >/dev/null
"$ROOT/scripts/build-ui.sh" --check

echo "setup check ok"
