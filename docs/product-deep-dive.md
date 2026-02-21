# Butterfly Bot Product Deep Dive

This page holds the detailed material moved out of README to keep onboarding fast.

## Architecture

Butterfly Bot runs as:

- Desktop UI (iced)
- Local daemon (`butterfly-botd`)
- Tooling/runtime layer (WASM-first tools + policy)
- Memory layer (SQLCipher + sqlite-vec)
- Provider layer (Ollama/OpenAI-compatible)

See also:

- [docs/threat-model.md](threat-model.md)
- [docs/security-audit.md](security-audit.md)

## Built-in Tools

- `mcp`
- `github`
- `zapier`
- `coding`
- `search_internet`
- `http_call`
- `planning`
- `todo`
- `tasks`
- `reminders`
- `wakeup`

Tool execution is WASM-only for built-in tools.

## Memory Model

- Structured temporal memory in encrypted SQLite (SQLCipher)
- Semantic retrieval using sqlite-vec embeddings
- Summarization/compression without replacing critical facts

## Packaging and Runtime Notes

- Linux: release `.deb` supported
- macOS: `.app` bundle workflow supported
- Windows: build-check lane present in CI
- Bundled WASM modules are auto-provisioned/refreshed by daemon

## Developer Commands

Build:

- `cargo build --release`

Test:

- `cargo test --all`

Coverage:

- `rustup component add llvm-tools-preview`
- `cargo install cargo-llvm-cov`
- `cargo llvm-cov --workspace --tests --lcov --output-path lcov.info`

Strict lint:

- `cargo clippy --all-targets --all-features -- -D warnings`

## Examples

- [examples/01-autonomous-inbox-management/context.md](../examples/01-autonomous-inbox-management/context.md)
- [examples/01-autonomous-inbox-management/heartbeat.md](../examples/01-autonomous-inbox-management/heartbeat.md)
- [examples/02-morning-briefings-digests/context.md](../examples/02-morning-briefings-digests/context.md)
- [examples/02-morning-briefings-digests/heartbeat.md](../examples/02-morning-briefings-digests/heartbeat.md)
- [examples/03-calendar-scheduling-task-management/context.md](../examples/03-calendar-scheduling-task-management/context.md)
- [examples/03-calendar-scheduling-task-management/heartbeat.md](../examples/03-calendar-scheduling-task-management/heartbeat.md)
- [examples/04-personal-family-executive-assistance/context.md](../examples/04-personal-family-executive-assistance/context.md)
- [examples/04-personal-family-executive-assistance/heartbeat.md](../examples/04-personal-family-executive-assistance/heartbeat.md)
- [examples/05-research-summarization-monitoring/context.md](../examples/05-research-summarization-monitoring/context.md)
- [examples/05-research-summarization-monitoring/heartbeat.md](../examples/05-research-summarization-monitoring/heartbeat.md)
