# Curb Backlog 006 Demo

This demo is a controlled walkthrough for observe, warn, acknowledge, and
enforcement behavior. It is intentionally built around Curb's configured
`synthetic-sleep` worker instead of real Codex, Claude, Gemini, or desktop app
processes.

The default path is a dry run:

```sh
bash demo/006/script/run-backlog-006-demo.sh --dry-run
```

The script prints the exact commands and artifact paths for a live capture. A
live capture must use an isolated `HOME`, an isolated `.curb` state directory,
and a synthetic `sleep` process. Do not use live model sessions for this demo.

The storyboard in `demo/006/storyboard.md` is the buyer-facing artifact. It
references the evidence ledger and calls out Curb's privacy boundary: Curb
records process identity, token metadata, policy events, and ledger hashes; it
does not capture prompts, responses, screenshots, keystrokes, or file contents.
