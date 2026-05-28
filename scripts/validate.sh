#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

"$ROOT/scripts/build-ui.sh" --check
cargo fmt --all -- --check
cargo clippy --all-targets -- -D warnings
cargo test
bash demo/006/script/run-backlog-006-demo.sh --dry-run
go test ./...
go vet ./...
(cd ui && npm run typecheck)
(cd ui && npm run lint)
(cd ui && npm test -- --run)
