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
cargo run -p sah-cli -- run --provider codex --cwd . "Summarize this repository"
```

Runs are stored under `SAH_HOME` if set, otherwise under `~/.sah/`.

## Development

Phase 1 commands in the new CLI:

- `doctor`
- `providers list`
- `run`
- `watch`
- `resume`

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
