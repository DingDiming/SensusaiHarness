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

## Install

For a local checkout:

```bash
cargo install --path crates/sah-cli
```

This installs the terminal binary as `sah`.

Tagged releases also publish prebuilt binaries through [`.github/workflows/release.yml`](/Users/ddm/Documents/GitHub/SensusaiHarness/.github/workflows/release.yml).

## Development

Phase 1 commands in the new CLI:

- `chat [--session PROVIDER:SESSION_ID] [--provider codex|claude] [--approval auto|confirm] [--cwd PATH] [--prompt-file PATH]`
- `completion <bash|elvish|fish|powershell|zsh>`
- `config provider show <provider> [--json]`
- `config provider set <provider> [--binary PATH] [--model MODEL] [--arg ARG]... [--json]`
- `config show [--json]`
- `config set [--provider codex|claude] [--approval auto|confirm] [--default-sah-home PATH]`
- `continue <provider:session-id> [--approval auto|confirm] [--prompt-file PATH] [prompt]`
- `archive <run-id> [--output PATH] [--delete-source]`
- `browse [--limit N] [--provider codex|claude] [--status running|completed|failed]`
- `doctor`
- `doctor --json`
- `delete <run-id> [--force]`
- `export <run-id> [--output PATH]`
- `import <bundle-path>`
- `list [--limit N] [--provider codex|claude] [--status running|completed|failed] [--json]`
- `inspect <run-id> [--json]`
- `man [--output-dir PATH]`
- `providers list`
- `providers list --json`
- `prune --keep N [--provider codex|claude] [--status running|completed|failed] [--archive-root PATH] [--dry-run]`
- `run [--provider codex|claude] [--approval auto|confirm] [--cwd PATH] [--prompt-file PATH] [prompt]`
- `sessions list [--limit N] [--provider codex|claude] [--json]`
- `sessions inspect <provider:session-id> [--json]`
- `verify-bundle <bundle-path> [--json]`
- `watch <run-id> [--follow]`
- `resume <run-id> [--approval auto|confirm] [--prompt-file PATH] [prompt]`

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

Per-provider launch config is persisted alongside the main defaults:

- `config provider set codex --binary /path/to/codex-wrapper --model gpt-5 --arg=--profile --arg=test`
- `config provider show codex --json`

Provider launch config currently supports:

- custom binary paths
- a default `--model` value per provider
- additional provider CLI args appended to launch commands

For extra args that begin with `-`, prefer the `--arg=VALUE` form so clap does not parse them as `sah` flags.

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
- `chat --session <provider:session-id>` re-enters an existing conversation and keeps prompting inside one terminal loop

`chat` provides a persistent terminal conversation loop. It uses `run` for the first prompt, `resume` for later prompts in the same provider session, and supports lightweight built-ins such as `:help`, `:session`, and `:exit`.

Prompt sources are now scriptable across one-shot and interactive flows:

- `run`, `continue`, and `resume` accept a positional prompt, `--prompt-file PATH`, or piped stdin
- `chat --prompt-file PATH` treats each non-empty line in the file as one prompt turn
- `chat` still accepts piped stdin line-by-line for non-interactive batch conversations

Terminal integration helpers are now built into the CLI:

- `sah completion zsh` prints a shell completion script to stdout
- `sah man --output-dir ./output/man` writes `sah.1` plus subcommand man pages such as `sah-run.1` and `sah-config-provider-set.1`

`browse` provides a lightweight interactive terminal browser for recent runs. It lets you pick a run and switch between overview, transcript, commands, workspace, and artifact views from the terminal.

Retention controls now cover both local run history and archived bundles:

- `archive <run-id> --delete-source` exports one run bundle to an archive path and optionally removes the local source run
- `prune --keep N` keeps only the most recent matching runs in local storage
- `prune --archive-root PATH` archives pruned runs before removing them from the local store

Exported bundles now include a machine-readable `bundle.json` manifest at the bundle root. It captures the run record, summary counts, and a relative file index so bundles can be inspected or replayed consistently outside the original store.

Bundle portability is now a full round-trip:

- `verify-bundle <bundle-path>` validates schema versions, file index integrity, and bundled run metadata before restore
- `import <bundle-path>` restores a previously exported run into the local store so `inspect`, `watch`, and other store-backed commands work again without manual copying

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

CI now runs on GitHub Actions through [`.github/workflows/ci.yml`](/Users/ddm/Documents/GitHub/SensusaiHarness/.github/workflows/ci.yml) and covers:

- `cargo fmt --check`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo test`
- basic CLI smoke commands for `doctor`, `providers list`, and `list`

## Repository Notes

- Operations/setup guide: [`docs/OPERATIONS.md`](/Users/ddm/Documents/GitHub/SensusaiHarness/docs/OPERATIONS.md)
- Compatibility/versioning guide: [`docs/COMPATIBILITY.md`](/Users/ddm/Documents/GitHub/SensusaiHarness/docs/COMPATIBILITY.md)
- Legacy audit: [`docs/LEGACY_AUDIT.md`](/Users/ddm/Documents/GitHub/SensusaiHarness/docs/LEGACY_AUDIT.md)
- Legacy policy: [`docs/LEGACY_POLICY.md`](/Users/ddm/Documents/GitHub/SensusaiHarness/docs/LEGACY_POLICY.md)
- License: MIT, with the full text in `LICENSE`
- Use GitHub issues and pull requests as the default backlog and review system.
- Generated artifacts, local databases, and dependency directories are intentionally ignored.
- `legacy/` is not part of the new runtime path.

## Near-Term Scope

- Stabilize provider abstraction for Codex CLI and Claude CLI
- Harden the event protocol and transcript persistence
- Add approval flows, diff views, and richer terminal UX
- Decide which legacy assets should be ported into Rust and which should be dropped
