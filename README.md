# SensusAI Harness

SensusAI Harness is a terminal-first local agent harness for Codex CLI and Claude CLI.

This repository is being rebuilt around a Rust workspace. The active product path now lives under `crates/`, and the previous Web-first implementation is preserved under `legacy/` as reference material only.

## Workspace Layout

- `crates/sah-cli`: terminal entrypoint
- `crates/sah-domain`: shared run and event models
- `crates/sah-store`: local filesystem run store
- `crates/sah-provider`: provider trait and probe helpers
- `crates/provider-codex`: Codex CLI adapter
- `crates/provider-claude`: Claude CLI adapter
- `crates/sah-runtime`: process execution and event persistence
- `legacy/`: previous backend/frontend/web-core implementation for reference only

## Current Status

- Active rebuild, not a stable public release yet
- Terminal-first Rust path is the only active direction
- Web-first code has been removed from the main runtime path

## Quick Start

```bash
cargo run -p sah-cli -- doctor
cargo run -p sah-cli -- providers list
cargo run -p sah-cli -- run --provider codex --approval auto --cwd . "Summarize this repository"
```

Runs are stored under `SAH_HOME` if set, otherwise under `~/.sah/`.

## Development

Phase 1 commands in the new CLI:

- `config show [--json]`
- `config set [--provider codex|claude] [--approval auto|confirm] [--default-sah-home PATH]`
- `continue <provider:session-id> [--approval auto|confirm] [prompt]`
- `doctor`
- `doctor --json`
- `delete <run-id> [--force]`
- `export <run-id> [--output PATH]`
- `list [--limit N] [--provider codex|claude] [--status running|completed|failed] [--json]`
- `inspect <run-id> [--json]`
- `providers list`
- `providers list --json`
- `run --approval auto|confirm`
- `sessions list [--limit N] [--provider codex|claude] [--json]`
- `sessions inspect <provider:session-id> [--json]`
- `watch <run-id> [--follow]`
- `resume <run-id> [--approval auto|confirm] [prompt]`

Persistent config lives at `~/.config/sah/config.json` by default. You can override the file with `--config PATH` or `SAH_CONFIG`.

Example:

```bash
cargo run -p sah-cli -- config set --provider codex --approval auto --default-sah-home ~/.sah
cargo run -p sah-cli -- config show --json
```

Resolution order for runtime defaults is:

```text
CLI flags > environment variables > config file > built-in defaults
```

Supported environment variables:

- `SAH_CONFIG`
- `SAH_PROVIDER`
- `SAH_APPROVAL`
- `SAH_HOME`

Approval modes are now CLI-managed:

- `auto`: execute immediately through the provider's automatic mode
- `confirm`: `sah` prompts once before launch or resume, then runs the provider in automatic mode if approved

Confirmed runs also record a `system` event with `approval confirmed by sah` at the start of the transcript.

Session-oriented browsing and continuation are available for runs that expose a provider session id:

- `sessions list` shows resumable conversations grouped by provider session
- `sessions inspect <provider:session-id>` shows the run history inside a session
- `continue <provider:session-id>` resumes the latest run in that session without looking up a raw run id

Each run now keeps:

- normalized event transcripts
- command records and stdout artifacts
- workspace snapshots before and after execution when `cwd` is inside a Git repository

Normalized event kinds now include:

- `message`
- `file_change`
- `command_started`
- `command_finished`
- `usage`
- `system`
- `completed`
- `failed`

`list` and `inspect` now surface lightweight summaries from those artifacts:

- command counts
- workspace changed-file and diff presence stats
- final message previews

Validation:

```bash
cargo check
cargo test -p sah-runtime
```

## Repository Notes

- Use GitHub issues and pull requests as the default backlog and review system.
- Generated artifacts, local databases, and dependency directories are intentionally ignored.
- `legacy/` is not part of the new runtime path.

## Near-Term Scope

- Stabilize provider abstraction for Codex CLI and Claude CLI
- Harden the event protocol and transcript persistence
- Add approval flows, diff views, and richer terminal UX
- Decide which legacy assets should be ported into Rust and which should be dropped
