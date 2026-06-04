# Dogfood Evidence

Use one directory per real release-build dogfood run:

```text
evidence/dogfood/YYYY-MM-DD-<short-slug>/
```

Each run should record:

- build SHA, OS, command, config path, state path, and mode;
- provider roots detected and source-health baseline;
- notification health and startup behavior;
- UI clarity notes when the app is used;
- false positives, false negatives, and process-correlation surprises;
- explicit confirmation that prompt, response, screenshot, keystroke, and
  file-content capture stayed absent;
- ranked backlog implications with links back to the evidence.

Dogfood evidence is the acceptance source for future post-closeout backlog
ranking. Do not open speculative feature tranches when the evidence is missing.

Start each run from `evidence/dogfood/TEMPLATE.md`, keep command output
summaries short, and link to larger logs only when needed.
