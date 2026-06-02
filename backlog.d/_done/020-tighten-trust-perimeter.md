# Tighten Curb's trust perimeter

Priority: P1
Status: done
Estimate: M

## Goal
Make Curb's privacy and enforcement boundaries match its documented promises:
no misleading egress knobs, no PATH-resolved termination primitive, and bounded
local usage-file ingestion.

## Non-Goals
- Prompt, response, screenshot, keystroke, or file-content capture.
- Building a remote control plane.
- Changing the PID + process-start-time termination identity boundary.

## Oracle
- [x] `ledger.forward_url` and alert webhook fields are either implemented with
      explicit privacy docs/tests, or removed from config/docs/UI so Curb does
      not advertise egress paths that do not exist.
- [x] Unix and Windows termination no longer resolves `kill` or `taskkill`
      through the caller's `PATH`; tests cover the boundary used by
      `SystemPlatform::terminate`.
- [x] Usage-file parsers enforce per-file and per-line caps and handle symlinks
      or root escapes as source-health failures rather than unbounded reads.
- [x] `scripts/validate.sh` passes.

## Notes
Closeout: implemented in `2f6e974`; the integrated branch also preserves the
provider-module split from `022`.

The security/privacy lane found that Curb's defaults are good, but its trust
surface has drift: egress config exists without HTTP forwarding code, process
actions shell out through ambient `PATH`, and historical file parsers are less
bounded than the live Codex tail path.
