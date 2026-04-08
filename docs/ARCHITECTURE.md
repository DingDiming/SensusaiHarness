# Terminal-First Architecture

SensusAI Harness is being rebuilt as a local terminal-first agent harness with Rust as the primary runtime.

## Phase 1 Boundary

Phase 1 is intentionally narrow:

- execute Codex CLI and Claude CLI through a shared provider abstraction
- persist runs and event transcripts locally
- expose a small CLI surface for doctor, provider discovery, run, and replay

The following are out of scope for Phase 1:

- browser UI
- HTTP gateway
- JWT/auth flows
- web session management

## Crate Responsibilities

- `sah-cli`: user-facing CLI and command routing
- `sah-domain`: core types for runs, statuses, and events
- `sah-store`: local run metadata and transcript persistence
- `sah-provider`: provider trait, command spec, and probe helpers
- `provider-codex`: Codex CLI adapter
- `provider-claude`: Claude CLI adapter
- `sah-runtime`: execution loop that streams provider output into stored events

The Rust mainline now also owns the reusable non-Web workspace/artifact helpers that were worth carrying forward from the legacy backend:

- standard run workspace directory creation
- traversal-safe workspace-relative artifact paths
- typed artifact writers for product specs, sprint contracts, QA reports, handoffs, approvals, checkpoint metadata, and run summaries

## Persistence Model

Each run is stored under a local run directory:

- `run.json`: current run metadata
- `events.jsonl`: append-only event transcript
- `artifacts/final-message.txt`: latest assistant message
- `artifacts/commands/*.json`: normalized command records
- `artifacts/commands/*.stdout.txt`: captured command stdout when available
- `artifacts/workspace/*.json`: workspace snapshot metadata
- `artifacts/workspace/*.status.txt`: git status snapshots before and after the run
- `artifacts/workspace/*.diff.patch`: git diff against `HEAD` when changes exist

Phase 1 normalizes provider output into a small internal event set:

- `message`
- `file_change`
- `command_started`
- `command_finished`
- `usage`
- `system`
- `completed`
- `failed`

Phase 1 also exposes a provider-independent approval policy:

- `auto`: let the provider execute commands automatically
- `confirm`: require a one-time confirmation inside `sah`, then run the provider in automatic mode

When a confirmed run proceeds, the runtime records a `system` event with `approval confirmed by sah` before the provider launch event.

Phase 1.5 adds persistent CLI defaults:

- config file defaults live at `~/.config/sah/config.json` unless `SAH_CONFIG` overrides the path
- supported persisted defaults are provider, approval mode, and store root
- runtime precedence is `CLI flags > environment variables > config file > built-in defaults`

The effective store root is:

- `--sah-home` when set
- otherwise `SAH_HOME` when set
- otherwise the config file `sah_home` value
- otherwise `~/.sah`

Transcript inspection now has two modes:

- `watch <run-id>` replays the stored transcript that already exists on disk
- `watch <run-id> --follow` polls the append-only transcript and waits for the terminal event before exiting

CLI summaries are derived from persisted artifacts rather than a separate cache:

- command counts come from `artifacts/commands/*.json`
- final message previews come from `artifacts/final-message.txt`
- workspace changed-file and diff presence stats come from `artifacts/workspace/*.json`

Session-oriented CLI commands aggregate runs by `provider + provider_session_id`:

- `sessions list` shows resumable conversations without exposing only raw run ids
- `sessions inspect` shows the ordered run history within one provider session
- `continue` resumes the latest run in a session by session ref instead of by run id

`browse` adds a lightweight interactive terminal browser on top of the same persisted store. It reuses run summaries and transcript/artifact readers rather than introducing a separate cache or UI backend.

Retention controls also stay store-backed and filesystem-local:

- `archive` reuses bundle export semantics and can remove the active local copy after export
- `prune` operates on the sorted local run list, skipping active running runs
- `prune --archive-root` combines both behaviors by archiving older runs before local deletion

Exported bundles carry a stable `bundle.json` manifest:

- `schema_version` currently starts at `1`
- the manifest embeds the exported `run.json` record plus event, command, and workspace counts
- `file_index` lists bundle-relative files so downstream tooling can inspect the bundle without scanning the filesystem ad hoc

End-to-end regression coverage now lives under `crates/sah-runtime/tests/` and replays fixture provider stdout through the real runtime, store, and parser stack.

## Legacy Policy

`legacy/` remains available as a reference pool. New runtime features should be built in the Rust workspace rather than threaded back into the old Web stack.
