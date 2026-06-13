# Release Evidence Index

Date: 2026-06-12

This is the cold-start map for Curb release proof. Use it before broad scans of
`evidence/dogfood/` or `.harness-kit/traces/`.

Latest behavior-proof baseline:

- Branch: `master`
- Commit: `ed7e2fb83cfbd52d54358e1bfed27df908e4a334`
- Merge source: PR #9, `deliver/040-ci-dogfood-doctrine-next`
- Post-merge hosted proof: GitHub Actions run `27470901295`
- Local parity proof: `git rev-list --left-right --count master...origin/master`
  returned `0 0` after PR #9 landed.

## Canonical Current Proof

| Claim | Canonical proof | Rerun command or live source | Notes |
|---|---|---|---|
| Full pre-merge gate | `scripts/validate.sh`; hosted run `27470901295` passed full Ubuntu, full macOS, fast feedback, Windows smoke, coverage, and dependency audit after PR #9 landed. | `scripts/validate.sh`; `gh run view 27470901295 --json conclusion,status,url` | This is the latest behavior-bearing hosted proof. The CI workflow now uses Node 24-compatible official action majors (`actions/checkout@v5`, `actions/setup-node@v6`, and `actions/upload-artifact@v6`); run `27470901295` had no old Node.js 20 action-runtime warning. |
| Browser-backed operator flow | `evidence/dogfood/2026-06-12-live-dashboard-qa/` | `bash scripts/qa-live-dashboard.sh evidence/dogfood/$(date +%F)-live-dashboard-qa` | Real `curb serve`, scratch state, synthetic metadata-only Codex usage, Playwright desktop/narrow screenshots, ack, settings save/revert, notification test, stale stop rejection, confirmed synthetic stop, API failure recovery, console capture, overflow checks, parser acceptance, and redaction check. Advisory until repeated runs justify making it mandatory. |
| Long-running headless sidecar | `evidence/dogfood/2026-06-12-long-sidecar-refresh/` | `CURB_LONG_DOGFOOD_SECONDS=7200 CURB_LONG_DOGFOOD_SNAPSHOT_SECONDS=300 bash scripts/dogfood-long-sidecar.sh evidence/dogfood/$(date +%F)-long-sidecar`; `python3 scripts/verify-long-sidecar-evidence.py evidence/dogfood/2026-06-12-long-sidecar-refresh --duration-seconds 7200` | Fresh two-hour release sidecar against current branch with private state outside the repo, final ready HTTP 200, all sampled live/health/overview probes HTTP 200, parser acceptance, redaction check, 1,110 watcher ticks against a 1,080 minimum, and documented source-health/readiness degradation. Use the same harness to prove the bounded-readiness/source-recovery fix with a new packet. |
| Successful safe stop enforcement | `evidence/dogfood/2026-06-04-headless-enforcement/` | `bash scripts/dogfood-headless-enforcement.sh evidence/dogfood/$(date +%F)-headless-enforcement` | Release headless server stopped a uniquely marked synthetic worker through the protected API. Retain as the canonical positive enforcement proof. |
| Stop rejection safety | `evidence/dogfood/2026-06-04-stop-rejection/` | Manual rerun from the packet README and protected stop API | Proves unsafe stop requests return `409 Conflict`, emit `stop_rejection`, and avoid token, reason, or raw session-key leakage. |
| Headless runbook and loopback API | `evidence/dogfood/2026-06-04-runbook-headless/` | `docs/runbooks/headless-sidecar.md` | Proves release headless sidecar startup, public live/ready probes, protected health auth, root behavior, parsed NDJSON, and redaction checks. |
| Timed observability | `evidence/dogfood/2026-06-04-headless-observability-3min/` | `CURB_DOGFOOD_SECONDS=180 bash scripts/dogfood-headless-observability.sh evidence/dogfood/$(date +%F)-headless-observability-3min` | Canonical short timed observability proof. The 20-second and 30-second packets are retained as historical/oracle-hardening context. |
| Active provider metadata | `evidence/dogfood/2026-06-03-active-agent/` | `curb usage --since 24h`; release headless sidecar probes | Proves non-zero Codex, Claude, and Pi provider metadata without prompt/response capture and preserves the fixed default-home discovery bug proof. |
| Dependency audit | Hosted run `27470901295`; `scripts/check-dependency-audit.sh` | `scripts/check-dependency-audit.sh --offline`; `bash scripts/check-dependency-audit.sh --online` | Online mode is hosted and explicit local proof. Offline mode avoids making the default local gate registry-dependent. |

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
`evidence/dogfood/2026-06-12-long-sidecar/` while the readiness/facade follow-up
is still pending.

## Quick Current Verification

```sh
git switch master
git pull --ff-only
git rev-list --left-right --count master...origin/master
scripts/validate.sh
gh run view 27470901295 --json conclusion,status,url
```

For browser-backed operator evidence:

```sh
bash scripts/qa-live-dashboard.sh evidence/dogfood/$(date +%F)-live-dashboard-qa
```

## Residual Risks

- Hosted run `27470901295` still emits unrelated Node `punycode` deprecation
  warnings from the cache action path; it does not emit the old Node.js 20
  action-runtime warning.
- The old long sidecar proof found source-health errors and transient
  `watcher_runtime: cache busy` readiness degradation while live and health
  stayed available. Focused tests now cover cached readiness and sanitized
  source recovery; a fresh long sidecar should replace the old packet before a
  release claim.
- The live browser QA script is intentionally advisory until it proves stable
  enough for the default gate.
