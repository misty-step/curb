# Source-Health Recovery

Source-health errors mean a provider metadata source could not be fully read.
They are operator recovery items, not readiness failures. Curb must keep serving
the dashboard from the metadata it can read while telling the operator what to
try next.

Use the recovery item first. It classifies the sanitized error and should never
show raw provider paths, prompt text, response text, screenshots, keystrokes, or
file contents.

Common next steps:

- `invalid utf-8` or `invalid json`: run `curb usage --since 24h`. If the same
  provider still reports the error, rotate or archive the malformed provider log
  from that provider app, then rerun the command.
- `oversized metadata line`: rotate or archive the oversized provider log. Curb
  intentionally refuses huge single lines because they are more likely to carry
  content-bearing payload than useful metadata.
- `permission denied`: restore read permission for the provider metadata
  directory, then rerun `curb usage --since 24h`.
- `refused symlink` or `outside trusted root`: remove the symlink or move the
  metadata back under the provider's trusted root.

Do not make Curb mutate provider logs automatically. Recovery stays advisory so
the operator can preserve evidence before moving or truncating a provider file.
