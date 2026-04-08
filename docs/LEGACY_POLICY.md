# Legacy Policy

## Decision

`legacy/` should not remain in this repository long-term.

Short-term:

- keep `legacy/` in this repository as a frozen reference tree while Phase 4 cleanup is still in progress
- do not add new feature work under `legacy/`
- continue porting only the small non-Web subset that has clear reuse value

Long-term:

- move the remaining archived tree to a separate archive repository once the `drop` set is removed and the `port` set has been harvested

## Why

The current product is terminal-first and Rust-first. Keeping the full historical Web stack in the same repository permanently creates three problems:

1. it blurs the active product boundary
2. it keeps almost a gigabyte of historical/runtime noise nearby
3. it makes architecture discovery worse for both humans and agents

## Exit Criteria For Moving `legacy/`

Move `legacy/` out of this repository once all of the following are true:

1. generated `drop` content has been removed
2. the remaining `port` subset has been migrated or intentionally abandoned
3. conflicting archived docs have been quarantined
4. the remaining tree is only historical reference material

## Until Then

- treat `legacy/` as read-only reference material
- prefer the root `/docs` directory for all current guidance
- prefer the Rust crates under `/crates` for all new implementation work
