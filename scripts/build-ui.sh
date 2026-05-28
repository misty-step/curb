#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
UI_DIR="$ROOT/ui"
EMBED_DIR="$ROOT/internal/web/dist"

usage() {
  cat <<'EOF'
usage: scripts/build-ui.sh [--check]

Build the React dashboard and sync it into the committed embedded asset tree.

  --check   fail if internal/web/dist differs from a fresh UI build
EOF
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

if [[ "${1:-}" == "--check" ]]; then
  if [[ ! -d "$EMBED_DIR" ]]; then
    echo "ui embed check failed: run scripts/build-ui.sh" >&2
    exit 1
  fi
  TMP_DIR="$(mktemp -d)"
  cleanup() {
    rm -rf "$TMP_DIR"
  }
  trap cleanup EXIT
  TMP_DIST="$TMP_DIR/dist"
  (cd "$UI_DIR" && npm run build -- --outDir "$TMP_DIST" --emptyOutDir)
  if ! diff -qr "$TMP_DIST" "$EMBED_DIR" >/dev/null; then
    echo "ui embed check failed: internal/web/dist is stale; run scripts/build-ui.sh" >&2
    diff -qr "$TMP_DIST" "$EMBED_DIR" >&2 || true
    exit 1
  fi
  echo "ui embed check ok"
  exit 0
fi

if [[ $# -gt 0 ]]; then
  usage >&2
  exit 2
fi

(cd "$UI_DIR" && npm run build)
rm -rf "$EMBED_DIR"
mkdir -p "$EMBED_DIR"
cp -R "$UI_DIR/dist/." "$EMBED_DIR/"
echo "synced ui/dist -> internal/web/dist"
