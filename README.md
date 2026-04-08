# SensusAI Harness

SensusAI Harness is an experimental local agent harness for software delivery workflows.

The current repository contains three layers:

- `core/`: Rust HTTP/SSE core
- `backend/`: Python harness runtime and orchestration
- `frontend/`: Next.js monitoring UI

The project is being cleaned up toward an open-source, terminal-first direction where Rust becomes the primary user-facing entrypoint and Codex CLI / Claude CLI are treated as provider adapters.

## Current Status

- Active development repository, not a stable public release yet
- Existing Web stack remains usable for local experimentation
- Terminal-first open-source packaging is planned, but not complete

## Local Development

### Backend

```bash
cd backend
uv run pytest -q
uv run uvicorn backend.src.main:app --host 0.0.0.0 --port 8000
```

Optional bootstrap admin credentials can be provided through environment variables:

```bash
export BOOTSTRAP_ADMIN_USERNAME=admin
export BOOTSTRAP_ADMIN_PASSWORD='change-me'
```

### Rust Core

```bash
cd core
cargo check
cargo run
```

### Frontend

```bash
cd frontend
npm install
npm run build
npm run dev
```

## Repository Notes

- Generated artifacts, local databases, and dependency directories are intentionally ignored.
- Runtime outputs belong under local `data/` or `output/` paths and should not be committed.
- There is currently no GitHub remote configured for this clone, so backlog and issue tracking still need to be moved to GitHub before the open-source workflow is complete.

## Near-Term Cleanup

- Stabilize provider abstraction for Codex CLI and Claude CLI
- Add a Rust terminal entrypoint
- Reduce Web-first assumptions in the repository layout and docs
- Finish open-source hygiene: docs, license choice, release packaging
