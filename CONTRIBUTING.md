# Contributing

This repository is still being reshaped into an open-source project, so keep changes narrow and reviewable.

## Expectations

- Prefer small, focused pull requests
- Do not commit generated files, local databases, caches, or dependency directories
- Keep runtime outputs under ignored local paths
- Preserve the separation between product code and local experiment artifacts

## Validation

Run the relevant checks for the area you touched:

```bash
cd backend && uv run pytest -q
cd core && cargo check
cd frontend && npm run build
```

## Security

- Do not introduce default credentials
- Do not commit real API keys, auth tokens, or personal workspace state
- Prefer environment variables for local bootstrap configuration
