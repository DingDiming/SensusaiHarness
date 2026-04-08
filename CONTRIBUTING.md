# Contributing

This repository is being rebuilt into a terminal-first Rust application. Keep changes narrow and reviewable.

## Expectations

- Prefer small, focused pull requests
- Do not commit generated files, local databases, caches, or dependency directories
- Keep runtime outputs under ignored local paths
- Preserve the separation between active Rust code and `legacy/` reference code

## Validation

Run the relevant checks for the area you touched:

```bash
cargo check
```

## Security

- Do not introduce default credentials
- Do not commit real API keys, auth tokens, or personal workspace state
- Prefer environment variables for local bootstrap configuration
- Do not make `legacy/` part of the default runtime again unless the design is explicitly revisited
