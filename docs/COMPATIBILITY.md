# Compatibility Rules

This document defines the current compatibility rules for transcript events, local store layout, and exported run bundles.

## Current Versions

- transcript schema: `1`
- store layout: `1`
- bundle schema: `1`

You can inspect the current versions through:

```bash
sah doctor --json
```

Exported bundles also record these values in `bundle.json`.

## Transcript Schema Rules

Transcript events are stored in `events.jsonl`.

Rules for schema `1`:

- each line is one JSON-encoded `RunEvent`
- additive fields are allowed within schema `1`
- removing fields, renaming fields, or changing field meaning requires a transcript schema bump
- readers should ignore unknown fields when possible

If transcript schema `2` is ever introduced, `sah` should add an explicit migration or a compatibility reader before writing mixed data into the same store.

## Store Layout Rules

Local runs live under:

```text
$SAH_HOME/runs/<run-id>/
```

Rules for store layout `1`:

- `run.json`, `events.jsonl`, and `artifacts/` are the stable top-level entries for a run
- additive files under `artifacts/` are allowed within layout `1`
- moving or renaming the top-level run files requires a store layout bump
- deleting previously stable artifact paths requires a store layout bump

Operational rule:

- older stores should continue to read as long as the layout version is unchanged
- new optional files must not make older runs unreadable

## Bundle Schema Rules

Portable bundles are exported directories containing:

- `run.json`
- `events.jsonl`
- `artifacts/...`
- `bundle.json`

Rules for bundle schema `1`:

- `bundle.json` is the authoritative manifest for the bundle
- additive manifest fields are allowed within schema `1`
- removing or renaming manifest keys requires a bundle schema bump
- `file_index` must stay bundle-relative so bundles remain portable across machines

`bundle.json` also records:

- `transcript_schema_version`
- `store_layout_version`
- `schema_version` for the bundle itself

This makes it possible to reason about replay and inspection compatibility without guessing from the directory tree alone.

## Change Policy

When compatibility changes:

1. bump the relevant version constant in `sah-domain`
2. document the change here
3. update tests and bundle expectations
4. add migration or compatibility handling before making the new format the default
