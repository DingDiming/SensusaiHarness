# Operations Guide

This guide covers provider setup, authentication expectations, common failures, and the local data layout for `sah`.

## Provider Setup

`sah` is only a local harness. It does not bundle model access itself. The provider CLIs must already be installed and runnable on the host machine.

Current providers:

- `codex`: OpenAI Codex CLI
- `claude`: Anthropic Claude Code

The quickest health check is:

```bash
sah doctor --json
```

That reports:

- the resolved config path
- the resolved `SAH_HOME`
- whether each provider binary is available
- the provider version strings that `sah` can see

## Authentication Expectations

`sah` does not manage provider login flows. It expects each provider CLI to already be authenticated.

Observed behavior on this machine:

- `codex` is installed and authenticated, so `sah run --provider codex ...` succeeds normally
- `claude` is installed but not authenticated; direct invocations currently return `Not logged in · Please run /login`

If a provider is not logged in, `sah` will still launch it, but the run will fail with the provider's own auth error.

## Common Failures

### Provider binary missing

Symptoms:

- `sah run` fails before launch
- `sah doctor` shows `available=false`

Typical message:

```text
provider <name> is unavailable: binary=<binary> detail=<os error>
```

Fix:

- install the missing provider CLI
- make sure it is on `PATH`
- rerun `sah doctor`

### Provider not authenticated

Symptoms:

- the binary exists
- the run starts, then fails almost immediately with an auth error

Known Claude example:

```text
Not logged in · Please run /login
```

Fix:

- authenticate the provider CLI directly
- verify the provider works outside `sah`
- rerun `sah`

### Confirmation cancelled by the user

In `approval=confirm` mode, `sah` prompts once before launch or resume. Any response other than `y` or `yes` cancels the run.

Typical message:

```text
approval cancelled by user
```

### Delete or archive blocked by a running run

`delete` and archive-with-source-removal keep the same safety rule: they do not remove runs that are still marked `running`.

Fix:

- wait for the run to finish, or
- use a different retention target

### Resume or continue unavailable

`resume` and `continue` depend on a stored provider session id. If the original run never captured one, the run is not resumable.

Fix:

- inspect the run with `sah inspect <run-id>`
- use a different run or session that has `provider_session_id`

## Local Data Layout

### Config

Default config path comes from the OS config directory:

- macOS example: `~/Library/Application Support/sah/config.json`
- Linux example: `~/.config/sah/config.json`

Overrides:

- `--config PATH`
- `SAH_CONFIG`

### Store Root

Default store root:

- `~/.sah`

Overrides:

- `--sah-home PATH`
- `SAH_HOME`
- config file `sah_home`

### Run Directory

Each run lives under:

```text
$SAH_HOME/runs/<run-id>/
```

Typical contents:

- `run.json`
- `events.jsonl`
- `artifacts/final-message.txt`
- `artifacts/commands/*.json`
- `artifacts/commands/*.stdout.txt`
- `artifacts/workspace/*.json`
- `artifacts/workspace/*.status.txt`
- `artifacts/workspace/*.diff.patch`

### Exported Bundle

`sah export` and `sah archive` produce a portable run bundle directory.

Bundle root contents:

- `run.json`
- `events.jsonl`
- `artifacts/...`
- `bundle.json`

`bundle.json` is the machine-readable manifest. It includes:

- `schema_version`
- the exported run record
- transcript, command, and workspace counts
- `final_message_preview`
- a `file_index` of bundle-relative files

## Useful Commands

```bash
sah doctor --json
sah providers list --json
sah list --limit 20
sah sessions list
sah browse
```
