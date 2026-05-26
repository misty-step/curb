# Curb Demo Storyboard

Audience: engineering leader or AI-ops buyer who needs local visibility and
safe enforcement without exposing private work.

## 1. Observe

The operator starts Curb against an isolated demo config. Curb shows the
configured policy, platform capability, and an empty ledger.

Evidence:

- `curb validate-config demo/006/artifacts/curb.demo.yaml`
- `curb scan --config demo/006/artifacts/curb.demo.yaml --json`
- Ledger path: `demo/006/artifacts/runs.ndjson`

Voiceover: Curb begins as a local visibility tool. It samples process metadata
and usage metadata, then writes append-only ledger events.

## 2. Synthetic Worker Appears

A controlled `sleep 120` process appears as `Synthetic Sleep`. The dashboard
labels it as a worker, not as a desktop application.

Voiceover: The demo never launches Codex, Claude, Gemini, or any expensive
model session. The target is a disposable process whose PID can be verified.

## 3. Warning

Alert mode crosses the configured warning threshold. Curb writes a warning
event and sends local notification surfaces when enabled.

Voiceover: Alert mode warns but never terminates. That invariant is visible in
the policy line and in the ledger event type.

## 4. Acknowledge

The operator acknowledges the run and extends the deadline. The evidence ledger
records the acknowledgement without capturing prompt or response content.

Voiceover: Acknowledgement is explicit, bounded, and auditable.

## 5. Enforce

The demo switches to enforcement against a fresh `sleep` worker. Curb
revalidates PID and start-time identity, waits through grace, and terminates
only the controlled process tree.

Voiceover: Enforcement is based on fresh process identity, not a stale name
match. Desktop app roots are watch-only unless explicitly configured otherwise.

## 6. Evidence And Privacy

The final frame shows the ledger summary, dashboard policy, and privacy
boundary.

Curb records:

- process identity metadata;
- token and model usage metadata when provider logs expose it;
- warnings, acknowledgements, would-stop events, and completed stop events;
- append-only ledger hashes.

Curb does not record:

- prompts or response text;
- screenshots;
- keystrokes;
- file contents.

## Remotion Notes

The Remotion source in `demo/remotion` maps this storyboard to a short,
buyer-facing walkthrough. The first version can render the storyboard text and
selected ledger excerpts; live screen recordings can be layered in later after
QA signs off on the synthetic process flow.
