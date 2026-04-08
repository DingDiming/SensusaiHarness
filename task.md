# SensusAI Harness Mainline Roadmap

## Phase 5: Interactive Product Flow

- [x] Add a persistent `chat` command for multi-turn, session-backed terminal conversations
- [x] Add prompt input sources such as `--prompt-file` and stdin capture for repeatable scripted runs and chats
- [x] Add provider launch config for binary overrides and per-provider default args or model selection
- [ ] Add shell completions and man pages for `sah`
- [ ] Add bundle import or verify flows so exported runs can be reopened without manual store surgery

## Phase 6: Mainline Cleanup

- [ ] Remove the `drop` subset from `legacy/` and shrink the frozen reference tree
- [ ] Open a separate archive destination for the remaining historical legacy tree
