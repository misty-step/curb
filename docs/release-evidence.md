# Release Evidence Index

Date: 2026-06-12

This is the cold-start map for Curb release proof. Use it before broad scans of
`evidence/dogfood/` or `.harness-kit/traces/`.

Latest behavior-proof baseline:

- Branch: `master`
- Commit: `33889504eca97e7951557ba21319afbd9053b8c1`
- Merge source: PR #5, `deliver/037-live-operator-browser-qa`
- Post-merge hosted proof: GitHub Actions run `27435399495`
- Local parity proof: `git rev-list --left-right --count master...origin/master`
  returned `0 0` after PR #5 landed.

## Canonical Current Proof

| Claim | Canonical proof | Rerun command or live source | Notes |
|---|---|---|---|
| Full pre-merge gate | `scripts/validate.sh`; hosted run `27435399495` passed full Ubuntu, full macOS, fast feedback, Windows smoke, coverage, and dependency audit after PR #5 landed. | `scripts/validate.sh`; `gh run view 27435399495 --json conclusion,status,url` | This is the latest behavior-bearing hosted proof. Docs-only release commits can land after it without changing the product proof baseline. Hosted jobs still emit a GitHub Actions Node 20 deprecation warning that should be handled before the June 16, 2026 Node 24 default switch. |
| Browser-backed operator flow | `evidence/dogfood/2026-06-12-live-dashboard-qa/` | `bash scripts/qa-live-dashboard.sh evidence/dogfood/$(date +%F)-live-dashboard-qa` | Real `curb serve`, scratch state, synthetic metadata-only Codex usage, Playwright desktop/narrow screenshots, ack, settings save/revert, notification test, stale stop rejection, confirmed synthetic stop, API failure recovery, console capture, overflow checks, parser acceptance, and redaction check. Advisory until repeated runs justify making it mandatory. |
| Long-running headless sidecar | `evidence/dogfood/2026-06-12-long-sidecar/` | `CURB_LONG_DOGFOOD_SECONDS=7200 CURB_LONG_DOGFOOD_SNAPSHOT_SECONDS=300 bash scripts/dogfood-long-sidecar.sh evidence/dogfood/$(date +%F)-long-sidecar` | Two-hour release sidecar with private state outside the repo, final ready HTTP 200, protected health HTTP 200, parser acceptance, redaction check, and documented source-health/readiness degradation. Still an acceptance source for the next refactor/readiness ticket. |
| Successful safe stop enforcement | `evidence/dogfood/2026-06-04-headless-enforcement/` | `bash scripts/dogfood-headless-enforcement.sh evidence/dogfood/$(date +%F)-headless-enforcement` | Release headless server stopped a uniquely marked synthetic worker through the protected API. Retain as the canonical positive enforcement proof. |
| Stop rejection safety | `evidence/dogfood/2026-06-04-stop-rejection/` | Manual rerun from the packet README and protected stop API | Proves unsafe stop requests return `409 Conflict`, emit `stop_rejection`, and avoid token, reason, or raw session-key leakage. |
| Headless runbook and loopback API | `evidence/dogfood/2026-06-04-runbook-headless/` | `docs/runbooks/headless-sidecar.md` | Proves release headless sidecar startup, public live/ready probes, protected health auth, root behavior, parsed NDJSON, and redaction checks. |
| Timed observability | `evidence/dogfood/2026-06-04-headless-observability-3min/` | `CURB_DOGFOOD_SECONDS=180 bash scripts/dogfood-headless-observability.sh evidence/dogfood/$(date +%F)-headless-observability-3min` | Canonical short timed observability proof. The 20-second and 30-second packets are retained as historical/oracle-hardening context. |
| Active provider metadata | `evidence/dogfood/2026-06-03-active-agent/` | `curb usage --since 24h`; release headless sidecar probes | Proves non-zero Codex, Claude, and Pi provider metadata without prompt/response capture and preserves the fixed default-home discovery bug proof. |
| Dependency audit | Hosted run `27435399495`; `scripts/check-dependency-audit.sh` | `scripts/check-dependency-audit.sh --offline`; `bash scripts/check-dependency-audit.sh --online` | Online mode is hosted and explicit local proof. Offline mode avoids making the default local gate registry-dependent. |

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
gh run view 27435399495 --json conclusion,status,url
```

For browser-backed operator evidence:

```sh
bash scripts/qa-live-dashboard.sh evidence/dogfood/$(date +%F)-live-dashboard-qa
```

## Residual Risks

- GitHub Actions emitted the Node 20 deprecation warning on current green runs.
- The long sidecar proof found source-health errors and transient
  `watcher_runtime: cache busy` readiness degradation while live and health
  stayed available.
- The live browser QA script is intentionally advisory until it proves stable
  enough for the default gate.
