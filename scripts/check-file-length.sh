#!/usr/bin/env bash
set -euo pipefail

# RATCHET: maximum lines per Rust source file in src/*.rs and curb-core/src/*.rs.
#
# When this trips, the remedy is to EXTRACT A MODULE (split the offending
# file into smaller, focused files) -- NOT to raise the cap. The cap exists
# to prevent regression toward god-objects.
#
# As files shrink, the cap should be ratcheted DOWN over time so the ceiling
# tracks the codebase's actual maximum and keeps holding the line.

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

MAX_LINES=2200

offenders=0
for f in src/*.rs curb-core/src/*.rs; do
  count=$(wc -l < "$f" | tr -d '[:space:]')
  if [ "$count" -gt "$MAX_LINES" ]; then
    echo "FAIL: $f has $count lines (cap $MAX_LINES)"
    offenders=$((offenders + 1))
  fi
done

if [ "$offenders" -gt 0 ]; then
  echo "check-file-length: $offenders file(s) exceed the $MAX_LINES-line cap; extract a module to fix"
  exit 1
fi

echo "check-file-length: ok (all src/*.rs and curb-core/src/*.rs within $MAX_LINES lines)"
