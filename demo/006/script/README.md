# Curb Backlog 006 Demo

This demo is a controlled walkthrough for alert-mode and enforcement behavior.
It is intentionally built around synthetic Codex token metadata and Curb's
configured `codex-synthetic-sleep` worker instead of real Codex, Claude,
Gemini, or desktop app processes.

Preview the demo without launching anything:

```sh
bash demo/006/script/run-backlog-006-demo.sh --dry-run
```

Run the live synthetic demo:

```sh
bash demo/006/script/run-backlog-006-demo.sh --mode all
```

The script creates an isolated `HOME`, isolated state directory, synthetic
Codex usage log, and a harmless `sleep` process. Alert mode must produce a
`usage_would_terminate` event while leaving the worker alive. Enforcement mode
must produce `usage_termination_completed` and stop only the synthetic worker.
Artifacts are written under `demo/006/artifacts/live-*`, with
`demo/006/artifacts/latest` pointing at the latest run.

The storyboard in `demo/006/storyboard.md` is the buyer-facing artifact. It
references the evidence ledger and calls out Curb's privacy boundary: Curb
records process identity, token metadata, policy events, and ledger hashes; it
does not capture prompts, responses, screenshots, keystrokes, or file contents.
