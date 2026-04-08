# Legacy Audit

This audit classifies the current `legacy/` contents into three buckets:

- `port`: worth migrating into the Rust mainline
- `archive`: keep only as historical/reference material
- `drop`: generated or conflicting material that should not survive long-term

## Top-Level Summary

| Path | Status | Reason | Next action |
| --- | --- | --- | --- |
| `legacy/backend/migrations` | `port` | still relevant to run/thread/artifact data modeling | mine schema ideas into Rust docs/tests, then archive the originals |
| `legacy/backend/src/harness/artifacts.py` | `port` | artifact layout and persistence semantics still map to the Rust store | port remaining useful logic into `sah-store` |
| `legacy/backend/src/harness/event_emitter.py` | `port` | event emission model aligns with transcript persistence | port only the event-shape ideas, not the Python runtime shell |
| `legacy/backend/src/harness/runner.py` | `port` | lifecycle model is still directly relevant to the terminal harness | port remaining lifecycle semantics into `sah-runtime` |
| `legacy/backend/src/harness/workspace.py` | `port` | workspace snapshot ideas remain relevant | port only the reusable snapshot semantics |
| `legacy/backend/tests/test_artifacts.py` | `port` | still useful as behavior reference for artifact expectations | port assertions into Rust tests where missing |
| `legacy/backend/tests/test_runner_lifecycle.py` | `port` | still useful as behavior reference for runtime transitions | port assertions into Rust tests where missing |
| `legacy/docs/HARNESS_RUNTIME.md` | `port` | still describes the useful runtime concepts better than some code comments | distill into current Rust docs, then archive |
| `legacy/docs/SQLITE_SCHEMA.md` | `port` | schema rationale is still relevant | merge useful pieces into current compatibility and ops docs |
| `legacy/docs/sqlite_schema_v1.sql` | `port` | concrete schema snapshot is still useful reference material | keep as reference until equivalent Rust docs cover it |
| `legacy/backend/src/harness/planner.py` | `archive` | potentially interesting orchestration reference, but not on the current runtime path | keep as reference only |
| `legacy/backend/src/harness/generator.py` | `archive` | same as planner, but tied to the old Python flow | keep as reference only |
| `legacy/backend/src/harness/evaluator.py` | `archive` | evaluation logic is not on the current critical path | keep as reference only |
| `legacy/backend/src/harness/llm_client.py` | `archive` | old provider shell, useful only as historical reference | keep as reference only |
| `legacy/backend/src/harness/progress.py` | `archive` | legacy progress model, superseded by Rust CLI behavior | keep as reference only |
| `legacy/backend/src/harness/role_registry.py` | `archive` | role registry is not part of the current terminal-first product | keep as reference only |
| `legacy/backend/src/routes` | `archive` | Web API surface is no longer a product path | keep only as historical reference |
| `legacy/backend/src/auth.py` | `archive` | JWT/auth shell belongs to the abandoned Web stack | archive |
| `legacy/backend/src/config.py` | `archive` | only partially relevant, but mostly tied to the old backend | archive |
| `legacy/backend/src/db.py` | `archive` | DB glue is tied to the old Python app shell | archive |
| `legacy/backend/src/deps.py` | `archive` | FastAPI dependency wiring is obsolete | archive |
| `legacy/backend/src/main.py` | `archive` | old backend entrypoint, no longer a runtime path | archive |
| `legacy/backend/src/schemas.py` | `archive` | API schema shell for the old backend | archive |
| `legacy/backend/tests/test_execution_modules.py` | `archive` | useful only if old Python execution modules return | archive |
| `legacy/backend/tests/test_runs_serialization.py` | `archive` | historical reference, lower value than current Rust tests | archive |
| `legacy/docs/API.md` | `archive` | documents a legacy Web API that should not guide new work | archive |
| `legacy/docs/ARCHITECTURE.md` | `archive` | conflicts with the terminal-first architecture | archive |
| `legacy/docs/PHASE2_PLAN.md` | `archive` | obsolete project planning context | archive |
| `legacy/docs/RUNBOOK.md` | `archive` | tied to the legacy backend/frontend stack | archive |
| `legacy/REWRITE_PLAN.md` | `archive` | historical planning artifact now superseded by `task.md` and current docs | archive |
| `legacy/frontend` | `archive` | previous Next.js UI, no longer on the product path | keep only as frozen reference |
| `legacy/web-core` | `archive` | previous Rust Web gateway, not part of the terminal runtime | keep only as frozen reference |
| `legacy/docker-compose.yml` | `archive` | only serves the old Web stack | archive |
| `legacy/package.json` | `archive` | root Web toolchain metadata for the old stack | archive |
| `legacy/package-lock.json` | `archive` | same as above | archive |
| `legacy/backend/.venv` | `drop` | generated virtualenv, almost 100MB of dead weight | remove from repo |
| `legacy/backend/__pycache__` | `drop` | generated cache | remove from repo |
| `legacy/backend/src/__pycache__` | `drop` | generated cache | remove from repo |
| `legacy/backend/tests/__pycache__` | `drop` | generated cache | remove from repo |
| `legacy/backend/.pytest_cache` | `drop` | generated cache | remove from repo |
| `legacy/backend/.ruff_cache` | `drop` | generated cache | remove from repo |
| `legacy/frontend/node_modules` | `drop` | generated dependency tree, over 600MB | remove from repo |
| `legacy/frontend/.next` | `drop` | generated build output | remove from repo |
| `legacy/web-core/target` | `drop` | generated Rust build output, nearly 1GB | remove from repo |
| `legacy/frontend/.env.local` | `drop` | local environment file should not remain in a long-term archived tree | remove or redact |

## Immediate Conclusions

1. The only legacy areas with strong port value are schema/runtime/artifact references from the old Python backend and selected legacy docs.
2. The old Web surfaces (`legacy/frontend`, `legacy/web-core`, `legacy/docs/API.md`, `legacy/docs/ARCHITECTURE.md`) should not influence new product design.
3. The generated content inside `legacy/` is large enough to distort repository hygiene and should be removed or quarantined first.

## Port Progress

Already ported into the Rust mainline:

- reusable workspace directory creation from `legacy/backend/src/harness/workspace.py`
- traversal-safe artifact path resolution from `legacy/backend/src/harness/workspace.py`
- reusable artifact writers from `legacy/backend/src/harness/artifacts.py`
- legacy artifact/workspace behavior assertions from `legacy/backend/tests/test_artifacts.py`

## Cleanup Progress

Removed from the working tree on 2026-04-08:

- `legacy/backend/.venv`
- `legacy/backend/__pycache__`
- `legacy/backend/src/__pycache__`
- `legacy/backend/src/harness/__pycache__`
- `legacy/backend/src/routes/__pycache__`
- `legacy/backend/tests/__pycache__`
- `legacy/backend/.pytest_cache`
- `legacy/backend/.ruff_cache`
- `legacy/frontend/node_modules`
- `legacy/frontend/.next`
- `legacy/frontend/.env.local`
- `legacy/web-core/target`

After this cleanup, the frozen legacy tree is down to a few hundred kilobytes instead of multiple gigabytes.

## Recommended Order

1. Remove generated `drop` content from `legacy/`.
2. Quarantine or clearly mark conflicting archived docs.
3. Port the small `port` subset into Rust docs/tests.
4. Re-evaluate whether the remaining archived tree still belongs in this repository at all.
