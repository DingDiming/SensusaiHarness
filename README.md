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
- `doctor`
- `doctor --json`
- `delete <run-id> [--force]`
- `export <run-id> [--output PATH]`
- `list [--limit N] [--provider codex|claude] [--status running|completed|failed] [--json]`
- `inspect <run-id> [--json]`
- `providers list`
- `providers list --json`
- `run --approval auto|confirm`
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

When using `approval=confirm`, pass `--allow-interactive-provider` explicitly. This keeps the CLI from silently dropping into provider-managed confirmation prompts.

Each run now keeps:

- normalized event transcripts
- command records and stdout artifacts
- workspace snapshots before and after execution when `cwd` is inside a Git repository

Validation:

```bash
cargo check
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
