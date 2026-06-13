#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

"$ROOT/scripts/build-ui.sh" --check
cargo fmt --all -- --check
cargo clippy --all-targets --workspace -- -D warnings
bash "$ROOT/scripts/check-file-length.sh"
bash "$ROOT/scripts/check-termination-boundary.sh"
python3 "$ROOT/scripts/check-secrets.py"
bash "$ROOT/scripts/check-product-principles.sh"
cargo test --workspace
(cd ui && npm run typecheck)
(cd ui && npm run lint)
(cd ui && npm test -- --run)
(cd ui && npm run smoke)
