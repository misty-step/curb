#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

require_text() {
  local path="$1"
  local text="$2"
  if ! grep -Fq "$text" "$path"; then
    echo "product-principles: missing required text in ${path}: ${text}" >&2
    exit 1
  fi
}

require_text "docs/product-principles.md" "Privacy, termination safety, and local endpoint authority are not demotable by"
require_text "docs/product-principles.md" "ADR. If a future feature clarifies lower-priority tradeoffs"
require_text "docs/product-principles.md" "local policy and local process authority remain endpoint-owned."
require_text "docs/product-principles.md" "Prompt"
require_text "docs/product-principles.md" "response text, screenshots, keystrokes, and file contents"
require_text "docs/product-principles.md" "outside the"
require_text "AGENTS.md" 'Product doctrine: `docs/product-principles.md`.'
require_text "README.md" "[docs/product-principles.md](docs/product-principles.md)"
require_text "docs/contributor-guide.md" "Read [product-principles.md](product-principles.md) before changing behavior"
require_text ".github/pull_request_template.md" "Product Doctrine Check"
require_text ".github/pull_request_template.md" "privacy, termination safety, and local endpoint"

echo "product-principles: ok"
