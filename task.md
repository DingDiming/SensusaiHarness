# SensusAI Harness Mainline Roadmap

## Phase 1.5: CLI Beta Hardening

- [x] Add persistent CLI config support for defaults such as provider, approval mode, and `SAH_HOME`
- [x] Add true live streaming for `watch` or `run --follow`, instead of replay-only transcript viewing
- [x] Improve `inspect` and `list` summaries with workspace diff stats, command counts, and final message previews
- [x] Normalize Codex and Claude events into a stricter internal schema for message, command, usage, file change, completed, and failed states
- [ ] Add fixture-based end-to-end tests for provider parsing, transcript persistence, resume, export, and delete flows

## Phase 2: Native Terminal Workflow

- [ ] Add a native `sah` approval flow so confirmation is managed by the CLI instead of provider-specific interactive prompts
- [ ] Add session-oriented commands for browsing and continuing prior conversations without depending on raw run IDs alone
- [ ] Add an interactive terminal run browser for recent runs, transcripts, commands, and artifacts
- [ ] Add run retention controls such as prune or archive for old local history and exported bundles
- [ ] Add machine-readable export metadata so a run bundle can be inspected or replayed consistently across environments

## Phase 3: Release and OSS Readiness

- [ ] Add a `LICENSE` and finalize the public open-source licensing choice
- [ ] Add CI for `cargo fmt --check`, `cargo clippy`, `cargo test`, and basic CLI smoke coverage
- [ ] Add release packaging for prebuilt binaries and a documented `cargo install` path
- [ ] Expand docs for provider setup, authentication expectations, common failures, and local data layout
- [ ] Add versioning and compatibility rules for transcript schema, store layout, and exported bundles

## Phase 4: Legacy Disposition

- [ ] Audit everything under `legacy/` and mark each area as port, archive, or drop
- [ ] Port only reusable non-Web assets from `legacy/` into the Rust mainline crates
- [ ] Remove or quarantine legacy docs that conflict with the terminal-first architecture
- [ ] Decide whether `legacy/` remains in this repository long-term or moves to a separate archive repository
