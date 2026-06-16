# Release Evidence Index

Date: 2026-06-16

This is the cold-start map for Curb release proof. Use it before broad scans of
`evidence/dogfood/` or `.harness-kit/traces/`.

Latest behavior-proof baseline:

- Branch: `master`
- Commit: `ad5e1b194a790e89819c3668711099f196740da9`
- Merge source: `Refresh long sidecar dogfood proof`
- Post-merge hosted proof: GitHub Actions run `27565453296`
- Local parity proof: `git rev-list --left-right --count master...origin/master`
  returned `0 0` at the `ad5e1b1` baseline.

## Canonical Current Proof

| Claim | Canonical proof | Rerun command or live source | Notes |
|---|---|---|---|
| Full pre-merge gate | `scripts/validate.sh`; hosted run `27565453296` passed full Ubuntu, full macOS, fast feedback, Windows smoke, coverage, and dependency audit on `ad5e1b1`. | `scripts/validate.sh`; `gh run view 27565453296 --json conclusion,status,url` | This is the latest behavior-bearing hosted proof after the refreshed long-sidecar packet merge. The CI workflow uses Node 24-compatible official action majors (`actions/checkout@v5`, `actions/setup-node@v6`, and `actions/upload-artifact@v6`). |
| Browser-backed operator flow | `evidence/dogfood/2026-06-12-live-dashboard-qa/` | `bash scripts/qa-live-dashboard.sh evidence/dogfood/$(date +%F)-live-dashboard-qa` | Real `curb serve`, scratch state, synthetic metadata-only Codex usage, Playwright desktop/narrow screenshots, ack, settings save/revert, notification test, stale stop rejection, confirmed synthetic stop, API failure recovery, console capture, overflow checks, parser acceptance, and redaction check. Advisory until repeated runs justify making it mandatory. |
| Long-running headless sidecar | `evidence/dogfood/2026-06-15-long-sidecar-refresh/` | `CURB_LONG_DOGFOOD_SECONDS=7200 CURB_LONG_DOGFOOD_SNAPSHOT_SECONDS=300 bash scripts/dogfood-long-sidecar.sh evidence/dogfood/$(date +%F)-long-sidecar`; `python3 scripts/verify-long-sidecar-evidence.py evidence/dogfood/2026-06-15-long-sidecar-refresh --duration-seconds 7200` | Fresh two-hour release sidecar generated against `090cfcf2` and shipped in `ad5e1b1` with private state outside the repo, final ready HTTP 200, 25/25 periodic readiness samples at `200 ready`, all sampled live/health/overview probes HTTP 200, parser acceptance, NDJSON/path/session redaction checks, 1,441 watcher ticks against a 1,080 minimum, max RSS 53,764 KB, max sampled probe latency 4.633927s, and 16 sanitized Codex source-health error events. |
| Successful safe stop enforcement | `evidence/dogfood/2026-06-04-headless-enforcement/` | `bash scripts/dogfood-headless-enforcement.sh evidence/dogfood/$(date +%F)-headless-enforcement` | Release headless server stopped a uniquely marked synthetic worker through the protected API. Retain as the canonical positive enforcement proof. |
| Stop rejection safety | `evidence/dogfood/2026-06-04-stop-rejection/` | Manual rerun from the packet README and protected stop API | Proves unsafe stop requests return `409 Conflict`, emit `stop_rejection`, and avoid token, reason, or raw session-key leakage. |
| Headless runbook and loopback API | `evidence/dogfood/2026-06-04-runbook-headless/` | `docs/runbooks/headless-sidecar.md` | Proves release headless sidecar startup, public live/ready probes, protected health auth, root behavior, parsed NDJSON, and redaction checks. |
| Timed observability | `evidence/dogfood/2026-06-04-headless-observability-3min/` | `CURB_DOGFOOD_SECONDS=180 bash scripts/dogfood-headless-observability.sh evidence/dogfood/$(date +%F)-headless-observability-3min` | Canonical short timed observability proof. The 20-second and 30-second packets are retained as historical/oracle-hardening context. |
| Active provider metadata | `evidence/dogfood/2026-06-03-active-agent/` | `curb usage --since 24h`; release headless sidecar probes | Proves non-zero Codex, Claude, and Pi provider metadata without prompt/response capture and preserves the fixed default-home discovery bug proof. |
| Dependency audit | Hosted run `27565453296`; `scripts/check-dependency-audit.sh` | `scripts/check-dependency-audit.sh --offline`; `bash scripts/check-dependency-audit.sh --online` | Online mode is hosted and explicit local proof. Offline mode avoids making the default local gate registry-dependent. |

## Historical Evidence To Keep

| Path | Classification | Why it stays |
|---|---|---|
| `.harness-kit/traces/` | Historical review and provider-lane receipts | Preserves readiness-tranche dispatch evidence, including failed/timed-out critic attempts that must not be misrepresented as successful review. Not a current release proof source. |
| `docs/readiness-tranche-closeout.md` | Historical June 4 closeout map | Useful for understanding the large readiness tranche and artifact disposition. Current release status lives in this file instead. |
| `docs/readiness-tranche-commit-plan.md` | Historical staging plan | Records the intended semantic commit breakdown for the old tranche. Not current staging guidance. |
| `evidence/dogfood/2026-06-03-local-release/` | Historical local release evidence | Early local release proof retained for lineage. Superseded by current gate and dogfood packets for release claims. |
| `evidence/dogfood/2026-06-04-headless-observability/` | Historical short observability run | Superseded by the 3-minute timed run for current short observability claims. |
| `evidence/dogfood/2026-06-04-headless-observability-30s-oracle/` | Historical oracle-hardening run | Retained because it documents dogfood-script hardening, but the 3-minute packet is the canonical timed proof. |

## Disposition

No evidence is deleted by backlog 038. The bulky packets are classified here so
future cleanup can archive or compress them with a reviewable rationale. Do not
delete evidence that remains an acceptance source for an open ticket, especially
`evidence/dogfood/2026-06-12-long-sidecar/` because it records the old
readiness/source-health failure that the refreshed June 15 packet supersedes.

## Quick Current Verification

```sh
git switch master
git pull --ff-only
git rev-list --left-right --count master...origin/master
scripts/validate.sh
gh run view 27565453296 --json conclusion,status,url
```

For browser-backed operator evidence:

```sh
bash scripts/qa-live-dashboard.sh evidence/dogfood/$(date +%F)-live-dashboard-qa
```

## Residual Risks

- The refreshed long sidecar replaced the old post-first-snapshot readiness
  blocker: all 25 periodic readiness samples were `200 ready`. Startup
  readiness is still strict and can return `503 degraded` before the first
  snapshot exists.
- The refreshed long sidecar still found 16 sanitized Codex source-health error
  events and sampled overview latency up to 4.633927s. Provider source-health
  errors now route through classified operator recovery guidance; treat the
  remaining latency as performance evidence, not readiness failure.
- The live browser QA script is intentionally advisory until it proves stable
  enough for the default gate.
