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
- `command_started`
- `command_finished`
- `usage`
- `system`
- `completed`
- `failed`

Phase 1 also exposes a provider-independent approval policy:

- `auto`: let the provider execute commands automatically
- `confirm`: keep provider-side approval behavior enabled

The CLI adds one guardrail on top:

- `confirm` mode requires explicit opt-in through `--allow-interactive-provider`

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

## Legacy Policy

`legacy/` remains available as a reference pool. New runtime features should be built in the Rust workspace rather than threaded back into the old Web stack.
