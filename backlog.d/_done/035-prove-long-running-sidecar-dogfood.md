# Prove long-running sidecar dogfood outside the worktree

Priority: P0
Status: done
Estimate: L

## Goal
Prove Curb can run as a local headless sidecar for a realistic operator window,
not just short worktree-bound dogfood runs.

## Oracle
- [x] Run a release-built `curb serve --headless` sidecar for at least one
      multi-hour real operator session using state outside the repo worktree.
- [x] Capture validated NDJSON, `/v1/live`, `/v1/ready`, protected API probes,
      source-health summaries, watcher-tick counts, and redaction checks in a
      new `evidence/dogfood/YYYY-MM-DD-<slug>/` packet.
- [x] Record startup, degraded-readiness transitions, provider roots discovered,
      notification capability, false positives, false negatives,
      process-correlation surprises, and resource/latency drift over the window.
- [x] Update `docs/agent-readiness-roadmap.md` so its CI and dogfood claims
      reflect current `master` truth, including any red hosted runs.
- [x] Produce a follow-up ranking from the evidence and explicitly decide
      whether the dogfood scripts remain enough or a repo-local QA/dogfood skill
      is now justified.

## Children
1. Harden the dogfood script or add a wrapper for longer windows, log rotation,
   and periodic summary snapshots.
2. Run the long sidecar session in visibility mode first; only run enforcement
   against a harmless synthetic worker.
3. Compare the long-run evidence with the June 4 short runs and update readiness
   status without overstating deployment proof.
4. File or close follow-ups based on real source-health and operator notes.

## Notes
**Why:** Operator/product and harness perspectives. Before this ticket, the
repo had strong June 4 local evidence, including a 180-second headless run, but
`docs/agent-readiness-roadmap.md` still listed longer real deployment dogfood as
an L4 blocker and older hosted `master` CI runs were red.

Keep prompt, response, screenshot, keystroke, and file-content capture absent.
