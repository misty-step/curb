#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

"$ROOT/scripts/build-ui.sh" --check
go test ./...
go vet ./...
(cd ui && npm run typecheck)
(cd ui && npm run lint)
(cd ui && npm test -- --run)
