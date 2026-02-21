## Butterfly Bot

<div style="margin-bottom: "20px">
        <img width="300px" alt="Butterfly-Bot-NTT" src="https://github.com/user-attachments/assets/36eceb35-b517-498a-803f-1c3b5a655ddb" />
</div>

[![CI](https://github.com/truemagic-coder/butterfly-bot/actions/workflows/ci.yml/badge.svg)](https://github.com/truemagic-coder/butterfly-bot/actions/workflows/ci.yml)
[![codecov](https://img.shields.io/codecov/c/github/truemagic-coder/butterfly-bot/main.svg)](https://codecov.io/gh/truemagic-coder/butterfly-bot)
[![Crates.io](https://img.shields.io/crates/v/butterfly-bot.svg)](https://crates.io/crates/butterfly-bot)
[![Rust](https://img.shields.io/badge/Rust-1.93%2B-orange?logo=rust)](https://www.rust-lang.org/tools/install)
[![fmt](https://github.com/truemagic-coder/butterfly-bot/actions/workflows/fmt.yml/badge.svg)](https://github.com/truemagic-coder/butterfly-bot/actions/workflows/fmt.yml)
[![clippy](https://github.com/truemagic-coder/butterfly-bot/actions/workflows/clippy.yml/badge.svg)](https://github.com/truemagic-coder/butterfly-bot/actions/workflows/clippy.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

Butterfly Bot is an opinionated personal-ops AI assistant built for people who want results, not setup overhead.

It is built to be **Zapier-first**: most real-world integrations run through Zapier MCP, so the fastest path is simply adding your Zapier MCP token in the Config screen.

Get your Zapier key here: [https://zapier.com/mcp](https://zapier.com/mcp) and setup your integrations there.

<img width="723" height="772" alt="Screenshot from 2026-02-17 10-40-54" src="https://github.com/user-attachments/assets/24de535a-94ad-41da-afd5-f6282665c0a9" />

### Install

#### Ubuntu

Download the `deb` file for Ubuntu from the latest [GitHub Release](https://github.com/truemagic-coder/butterfly-bot/releases)

#### Other

`cargo install butterfly-bot`

`butterfly-botd` now auto-provisions bundled WASM tool modules and smart-refreshes them on upgrades (to `$XDG_DATA_HOME/butterfly-bot/wasm` or `~/.local/share/butterfly-bot/wasm` on Linux, `~/Library/Application Support/butterfly-bot/wasm` on macOS, and `%APPDATA%/butterfly-bot/wasm` on Windows).


### Why users pick it:

- **Fast value:** works out-of-the-box with default settings.
- **OpenAI-first:** optimized for OpenAI routing and coding with simple key-based setup.
- **UI-first:** polished desktop app with chat, AI activity, and settings.
- **Automation:** full toolset provided for your always-on agent.
- **Integrations:** Zapier-first MCP integration model for connecting your existing SaaS stack in minutes.
- **Security:** WASM-only execution for tools plus OS keychain-backed secrets - no plaintext secrets or insecure tools.
- **Memory:** best-in-class memory that remembers the facts and when they happened.

### OpenAI setup (recommended)

1. Open the app and go to `Config`.
2. Paste your **OpenAI API key**.
3. Save the config and start chatting.

Notes:

- Router model defaults to **`gpt-4.1-mini`** on `api.openai.com`.
- Memory models run on OpenAI defaults.
- **Grok API key** is optional and only used for internet search.

### Zapier-first setup (60 seconds)

If you only configure one thing, configure Zapier:

1. Open the app and go to `Config`.
2. Paste your **Zapier MCP token** and save the config.

That single token unlocks most production workflows because Butterfly Bot can route actions through Zapier's connected apps (email, calendar, tasks, CRM, docs, alerts, and more).

Want a fast start? Use the ready-made templates in [examples/](examples/) and paste a `context.md` + `heartbeat.md` pair into the app.

## Built for the money layer: Agentic Wallet + x402 Payments

Butterfly Bot is built around a **first-of-its-kind hardware-encrypted agentic wallet** so your AI can act, pay, and execute with strong built-in protection.

What this means for non-technical users:

- **Your wallet is protected by hardware-grade encryption by default.**
- **Your assistant can run agentic payments with x402** for real autonomous commerce flows.
- **You stay in control** with approval checkpoints for sensitive actions.

Simple story: your assistant is not just smart - it can also be an economic actor.

Read the deeper details: [docs/solana-x402-economic-actor.md](docs/solana-x402-economic-actor.md)

## Documentation map

| I need… | Go here |
|---|---|
| Product and architecture deep dive | [docs/product-deep-dive.md](docs/product-deep-dive.md) |
| Security audit behavior and guidance | [docs/security-audit.md](docs/security-audit.md) |
| Threat model and trust boundaries | [docs/threat-model.md](docs/threat-model.md) |
| TPM/custody hardening plan | [docs/tpm-cocoon-security-plan.md](docs/tpm-cocoon-security-plan.md) |
| Cross-platform reliability plan | [docs/everyone-plan.md](docs/everyone-plan.md) |
| Coverage to 100% plan | [docs/coverage-100-plan.md](docs/coverage-100-plan.md) |
| Daily blackbox report template | [docs/blackbox-daily-report-template.md](docs/blackbox-daily-report-template.md) |
| Security evidence requirements | [docs/security-evidence-manifest.md](docs/security-evidence-manifest.md) |

## Examples (Context + Heartbeat)

- [examples/01-autonomous-inbox-management/context.md](examples/01-autonomous-inbox-management/context.md)
- [examples/01-autonomous-inbox-management/heartbeat.md](examples/01-autonomous-inbox-management/heartbeat.md)
- [examples/02-morning-briefings-digests/context.md](examples/02-morning-briefings-digests/context.md)
- [examples/02-morning-briefings-digests/heartbeat.md](examples/02-morning-briefings-digests/heartbeat.md)
- [examples/03-calendar-scheduling-task-management/context.md](examples/03-calendar-scheduling-task-management/context.md)
- [examples/03-calendar-scheduling-task-management/heartbeat.md](examples/03-calendar-scheduling-task-management/heartbeat.md)
- [examples/04-personal-family-executive-assistance/context.md](examples/04-personal-family-executive-assistance/context.md)
- [examples/04-personal-family-executive-assistance/heartbeat.md](examples/04-personal-family-executive-assistance/heartbeat.md)
- [examples/05-research-summarization-monitoring/context.md](examples/05-research-summarization-monitoring/context.md)
- [examples/05-research-summarization-monitoring/heartbeat.md](examples/05-research-summarization-monitoring/heartbeat.md)

## Developer quick commands

- Build: `cargo build --release`
- Test: `cargo test --all`
- Strict lint: `cargo clippy --all-targets --all-features -- -D warnings`
- Coverage: `cargo llvm-cov --workspace --tests --lcov --output-path lcov.info`

### License

MIT
