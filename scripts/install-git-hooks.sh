#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

usage() {
  cat <<'EOF'
usage: scripts/install-git-hooks.sh [--force]

Installs Curb's repo-managed pre-commit hook into this Git checkout.
Use --force to replace an existing non-Curb pre-commit hook.
EOF
}

force=0
case "${1:-}" in
  "")
    ;;
  --force)
    force=1
    ;;
  -h|--help)
    usage
    exit 0
    ;;
  *)
    usage >&2
    exit 2
    ;;
esac

src="$ROOT/scripts/git-hooks/pre-commit"
dst="$(git rev-parse --git-path hooks/pre-commit)"
dst_dir="$(dirname "$dst")"

if [ ! -f "$src" ]; then
  echo "missing hook template: $src" >&2
  exit 1
fi

mkdir -p "$dst_dir"

if [ -e "$dst" ] && ! grep -q "curb pre-commit" "$dst"; then
  if [ "$force" -ne 1 ]; then
    echo "refusing to replace existing non-Curb hook: $dst" >&2
    echo "rerun with --force after preserving anything you still need" >&2
    exit 1
  fi
fi

cp "$src" "$dst"
chmod +x "$dst"

echo "installed Curb pre-commit hook: $dst"
