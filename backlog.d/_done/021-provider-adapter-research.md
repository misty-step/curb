# Add the next provider adapter from evidence, not aspiration

Priority: P2
Status: done
Estimate: M

## Goal
Choose and implement the first non-Codex/non-Claude provider adapter whose local
metadata source is real enough for Curb's privacy-preserving model.

## Non-Goals
- Capturing prompt or response text.
- Supporting every advertised provider in one change.
- Proxying all LLM traffic through Curb.

## Oracle
- [x] A short repo doc compares Antigravity, GrokBuild, Pi, OpenCode, and any
      current local-agent log sources that grooming research identifies, using
      concrete local files/APIs rather than marketing claims.
- [x] One provider is selected with a metadata-only contract, fixture files, and
      parser tests; unsupported candidates get explicit source-health messages.
- [x] README/config docs stop implying broad provider coverage before adapters
      exist.
- [x] `scripts/validate.sh` passes.

## Notes
Closeout: research landed in `2f6c6c8`; the Pi provider adapter and parser tests
landed in `018e8a0` after `022` created the provider-module boundary.

Current public copy positions Curb as a local agent watchdog, but live support is
Codex/Claude-shaped. Grooming research also found nearby projects using local
proxies, OTLP, and provider-specific JSONL, so provider expansion should start
with source truth rather than a generic plugin layer.
