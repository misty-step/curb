# Provider Adapter Research

Backlog 021 asked for the next non-Codex/non-Claude provider adapter to be
chosen from source evidence, not product positioning. Pi was selected and
implemented after backlog 022 split `curb-core/src/usage.rs` into
provider-shaped modules.

This research used metadata-only handling. The local inspection covered
filenames, directory names, table schemas, JSON keys, file counts, and public
documentation. It did not read, quote, summarize, or persist prompt text,
response text, screenshots, keystrokes, or file-content payloads.

## Current Support

Curb currently meters:

- Codex from `~/.codex/archived_sessions` and `~/.codex/sessions`.
- Claude Code from `~/.claude/projects`.
- Pi from `~/.pi/agent/sessions`.

The adapters emit provider-neutral token checkpoints and user-input boundaries.
Provider-specific parsing lives behind `curb-core/src/usage/{codex,claude,pi}.rs`
so content-bearing fields stay outside the shared scan loop.

## Candidate Findings

| Candidate | Local source health | Metadata-only fit | Decision |
| --- | --- | --- | --- |
| Pi | Strong. Public docs define JSONL sessions under `~/.pi/agent/sessions/--<path>--/<timestamp>_<uuid>.jsonl`; this machine has matching files. | Medium. Session headers are metadata-only and assistant messages include `usage`, but session rows also include user, assistant, tool, compaction, and branch-summary content. | Implemented as the next adapter. |
| OpenCode | Strong local footprint. Local SQLite database at `~/.local/share/opencode/opencode.db`; local `storage/` directories also exist. | Medium. Token/cost fields are present, but useful rows live inside a live vendor-owned SQLite database and JSON columns also contain content-bearing fields. | Good second candidate after Pi. |
| Antigravity CLI | Medium. Local roots exist under `~/.gemini/antigravity-cli`, including settings, cache files, protobuf conversation files, brain artifact metadata, and CLI logs. Public docs expose config and status-line metadata such as `conversation_id`. | Weak for token metering. The observed stable public metadata is identity/status/config, not a token usage ledger. Conversation protobufs are not a safe first adapter without an official schema and content filter. | Do not implement yet. Use for process visibility only. |
| GrokBuild / Grok CLI | Weak for token metering. Local roots exist under `~/.grok`, including prompt history, logs, model cache, and session-search SQLite. | Weak. The prompt-history JSONL exposes prompt fields, and the search index contains full session documents. Local logs are operational, not a stable token ledger. | Do not implement yet. |
| Repo-local markers | Weak. `.pi`, `.antigravitycli`, and `.git/opencode` markers identify tool use in some repos. | Weak. Markers can help process/project correlation, not token metering. | Do not treat as usage providers. |

## Evidence

Local metadata inspected:

- `find ~/.pi/agent/sessions -type f -name '*.jsonl'` found 367 local session
  files, and a first-line header key check showed `cwd`, `id`, `timestamp`,
  `type`, and `version`.
- `sqlite3 ~/.local/share/opencode/opencode.db '.tables'` showed `project`,
  `session`, `message`, `part`, `todo`, permission/share tables, and migration
  state.
- `sqlite3 ~/.local/share/opencode/opencode.db "select name, sql from sqlite_master ..."`
  showed stable session columns such as `id`, `project_id`, `parent_id`,
  `directory`, `title`, `time_created`, and `time_updated`.
- JSON keys in OpenCode `message.data` included `model`, `providerID`, `tokens`,
  `cost`, `role`, `time`, and `path`; JSON keys in `part.data` included
  `tokens`, `cost`, `type`, `tool`, `state`, `metadata`, and content-bearing
  keys that an adapter must ignore.
- `find ~/.gemini/antigravity-cli ...` found 117 `.pb` conversation files, CLI
  logs, settings JSON, cache JSON, and artifact metadata JSON.
- `sqlite3 ~/.grok/sessions/session_search.sqlite` showed `session_docs` and an
  FTS index with `title` and `content` columns, which is not a metadata-only
  metering source.

Public documentation checked:

- [OpenCode troubleshooting](https://opencode.ai/docs/troubleshooting/)
  documents local storage at `~/.local/share/opencode/`, with
  project-specific session and message data below project storage.
- [Pi session-format documentation](https://pi.dev/docs/latest/session)
  defines JSONL session files under
  `~/.pi/agent/sessions/--<path>--/<timestamp>_<uuid>.jsonl`, message entries,
  model-change entries, and assistant usage fields.
- [Antigravity CLI documentation](https://www.antigravity.google/docs/cli-using)
  describes `~/.gemini/antigravity-cli/settings.json`, and
  [conversation documentation](https://antigravity.google/docs/cli-conversations)
  describes resumeable conversation histories.

## Integration Result

Pi is registered as provider `pi`, rooted at `~/.pi/agent/sessions`. The adapter
reads JSONL incrementally by line and parses only top-level session metadata,
message role/model, and assistant usage fields. Content-bearing fields such as
user content, assistant text, tool output, compaction summaries, branch
summaries, command text, screenshots, and file snapshots are ignored by the
parser structs.

OpenCode remains the next likely candidate, with a separate SQLite reader design
that handles live WAL files and schema drift explicitly. Until then, OpenCode,
Antigravity, and GrokBuild stay researched candidates rather than supported
usage providers.
