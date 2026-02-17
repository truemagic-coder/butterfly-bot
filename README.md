## Butterfly Bot

<div style="margin-bottom: "20px">
        <img width="300px" alt="Butterfly-Bot-NTT" src="https://github.com/user-attachments/assets/36eceb35-b517-498a-803f-1c3b5a655ddb" />
</div>

[![CI](https://github.com/truemagic-coder/butterfly-bot/actions/workflows/ci.yml/badge.svg)](https://github.com/truemagic-coder/butterfly-bot/actions/workflows/ci.yml)
[![codecov](https://img.shields.io/codecov/c/github/truemagic-coder/butterfly-bot/main.svg)](https://codecov.io/gh/truemagic-coder/butterfly-bot)
[![Rust](https://img.shields.io/badge/Rust-1.93%2B-orange?logo=rust)](https://www.rust-lang.org/tools/install)
[![fmt](https://github.com/truemagic-coder/butterfly-bot/actions/workflows/fmt.yml/badge.svg)](https://github.com/truemagic-coder/butterfly-bot/actions/workflows/fmt.yml)
[![clippy](https://github.com/truemagic-coder/butterfly-bot/actions/workflows/clippy.yml/badge.svg)](https://github.com/truemagic-coder/butterfly-bot/actions/workflows/clippy.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](LICENSE)

Butterfly Bot is an opinionated personal-ops AI assistant built for people who want results, not setup overhead.

It is built to be **Zapier-first**: most real-world integrations run through Zapier MCP, so the fastest path is simply adding your Zapier MCP token in the Config screen.

Get your Zapier key here: [https://zapier.com/mcp](https://zapier.com/mcp) and setup your integrations there.

<img width="809" height="797" alt="Screenshot from 2026-02-15 09-31-23" src="https://github.com/user-attachments/assets/eee0cc6d-90f1-44c1-9c25-7866ef972960" />

### Install

#### Ubuntu

Download the `deb` file for Ubuntu from the latest [GitHub Release](https://github.com/truemagic-coder/butterfly-bot/releases)

#### Mac

Download the `app` file for Mac from the latest [GitHub Release](https://github.com/truemagic-coder/butterfly-bot/releases)

#### Other

`cargo install butterfly-bot`


### Why users pick it:

- **Fast value:** works out-of-the-box with default settings.
- **Unlimited tokens:** designed to support Ollama to run privately on your computer with unlimited use.
- **UI-first:** polished desktop app with chat, AI activity, and settings.
- **Automation:** full toolset provided for your always-on agent.
- **Integrations:** Zapier-first MCP integration model for connecting your existing SaaS stack in minutes.
- **Security:** WASM-only execution for tools plus OS keychain-backed secrets - no plaintext secrets or insecure tools.
- **Memory:** best-in-class memory that remembers the facts and when they happened.

### Zapier-first setup (60 seconds)

If you only configure one thing, configure Zapier:

1. Open the app and go to `Config`.
2. Paste your **Zapier MCP token** and save the config.

That single token unlocks most production workflows because Butterfly Bot can route actions through Zapier's connected apps (email, calendar, tasks, CRM, docs, alerts, and more).

Want a fast start? Use the ready-made templates in [examples/](examples/) and paste a `context.md` + `heartbeat.md` pair into the app.

### Top 5 real-world use cases (Butterfly Bot + Zapier)

These are the most commonly adopted autonomous-agent outcomes and how to implement them with Butterfly Bot.

1. **Autonomous email / inbox management**
    - Use Butterfly Bot planning + tasks + reminders to run recurring inbox cleanup cycles.
    - Route actions through Zapier to Gmail/Outlook for labeling, drafting, replying, archiving, and escalation.
    - Keep only high-risk decisions human-in-the-loop (for example: send approval for high-priority drafts).

2. **Daily morning briefings and proactive digests**
    - Schedule a wakeup/task that runs each morning before your workday.
    - Pull weather, calendar, headlines, watchlists, or team metrics through Zapier app integrations.
    - Deliver one consolidated digest in your preferred channel (email, Slack, Telegram, etc.) via Zapier.

3. **Calendar, scheduling, and task management**
    - Let Butterfly Bot parse intent from chat/notes and generate next actions.
    - Use Zapier to sync Google Calendar/Outlook + Todoist/Linear/Jira/Asana from one workflow.
    - Enforce conflict checks and reminder policies with Butterfly Bot reminders and recurring tasks.

4. **Personal/family/executive assistance**
    - Capture requests in natural language, then let Butterfly Bot break them into executable steps.
    - Use Zapier actions for shopping lists, booking flows, subscription tracking, and travel coordination.
    - Keep continuity with Butterfly memory so follow-ups reflect previous preferences and decisions.

5. **Research, summarization, and monitoring**
    - Run scheduled monitoring tasks for competitors, markets, topics, or project signals.
    - Use Zapier integrations to collect source data and trigger downstream reports.
    - Have Butterfly Bot summarize findings, create tasks, and notify only when thresholds are met.

### Why this pairing works

- **Butterfly Bot** provides autonomy, planning, memory, reminders, and always-on execution.
- **Zapier MCP** provides broad app connectivity without custom per-app engineering.
- Together, you get local-first agent orchestration plus cloud app automation from one simple setup path.

### Examples (Context + Heartbeat templates)

Use these templates by opening a pair, copying the markdown, and pasting into the app:

1. Paste `context.md` into the `Context` tab.
2. Paste `heartbeat.md` into the `Heartbeat` tab.
3. Save and start Heartbeat.

- **Autonomous inbox management**
    - [examples/01-autonomous-inbox-management/context.md](examples/01-autonomous-inbox-management/context.md)
    - [examples/01-autonomous-inbox-management/heartbeat.md](examples/01-autonomous-inbox-management/heartbeat.md)
- **Morning briefings and digests**
    - [examples/02-morning-briefings-digests/context.md](examples/02-morning-briefings-digests/context.md)
    - [examples/02-morning-briefings-digests/heartbeat.md](examples/02-morning-briefings-digests/heartbeat.md)
- **Calendar, scheduling, and tasks**
    - [examples/03-calendar-scheduling-task-management/context.md](examples/03-calendar-scheduling-task-management/context.md)
    - [examples/03-calendar-scheduling-task-management/heartbeat.md](examples/03-calendar-scheduling-task-management/heartbeat.md)
- **Personal/family/executive assistance**
    - [examples/04-personal-family-executive-assistance/context.md](examples/04-personal-family-executive-assistance/context.md)
    - [examples/04-personal-family-executive-assistance/heartbeat.md](examples/04-personal-family-executive-assistance/heartbeat.md)
- **Research, summarization, and monitoring**
    - [examples/05-research-summarization-monitoring/context.md](examples/05-research-summarization-monitoring/context.md)
    - [examples/05-research-summarization-monitoring/heartbeat.md](examples/05-research-summarization-monitoring/heartbeat.md)

### How it compares:

| Criterion | Weight | Butterfly Bot | OpenClaw | ZeroClaw | IronClaw |
|---|---:|---:|---:|---:|---:|
| Workflow completeness | 20 | 5 | 4 | 3 | 4 |
| Reliability and recovery | 20 | 5 | 3 | 4 | 3 |
| UX and visibility | 15 | 5 | 4 | 3 | 4 |
| Security posture | 15 | 5 | 1 | 5 | 4 |
| Setup/onboarding | 10 | 5 | 4 | 5 | 4 |
| Integration leverage/extensibility | 10 | 4 | 5 | 5 | 5 |
| Docs/contributor DX | 10 | 5 | 4 | 5 | 4 |
| **Total Weighted (/500)** | **100** | **490** | **345** | **415** | **390** |

### Tools

Built-in tools included in Butterfly Bot:

- `mcp` — Connects to external MCP servers over streamable HTTP.
- `github` — GitHub MCP wrapper for GitHub workflows.
- `zapier` — Zapier MCP wrapper for connected app automations.
- `coding` — Dedicated coding tool/model for implementation tasks.
- `search_internet` — Web search tool (OpenAI, Grok, or Perplexity providers).
- `http_call` — Generic HTTP client for external API calls.
- `planning` — Structured plans (goals + steps).
- `todo` — Ordered checklist-style todos.
- `tasks` — Scheduled one-off and recurring tasks.
- `reminders` — Reminder creation and lifecycle operations.
- `wakeup` — Interval-based wakeup/autonomy task loop.

Tool configuration is convention-first and managed through app defaults plus minimal Config tab controls.


### Architecture (Daemon + UI + Always-On Agent)

```
        ┌──────────────────────────────────────┐
        │           Desktop UI (Dioxus)        │
        │  - chat, activity, simple settings   │
        │  - streams tool + agent events       │
        └───────────────┬──────────────────────┘
                │ IPC / local client
                v
            ┌──────────────────────────────┐
            │      butterfly-botd          │
            │        (daemon)              │
            │  - always-on scheduler       │
            │  - tools + wakeups           │
            │  - memory + planning         │
            └──────────────┬───────────────┘
                           │
         ┌─────────────────┼─────────────────┐
         v                 v                 v
    ┌────────────────┐  ┌───────────────┐  ┌──────────────────┐
    │  Memory System │  │ Tooling Layer │  │  Model Provider  │
    │ (SQLCipher +   │  │ (MCP, HTTP,   │  │     (Ollama)     │
    │  sqlite-vec)   │  │ reminders,    │  │                  │
    │                │  │ tasks, etc.)  │  │                  │
    │                │  │ + WASM sandbox│  │                  │
    │                │  │   runtime     │  │                  │
    └────────────────┘  └───────────────┘  └──────────────────┘
```

### Memory System (Diagram + Rationale)

```
                    ┌───────────────────────────────┐
                    │         Conversation          │
                    │  (raw turns + metadata)       │
                    └───────────────┬───────────────┘
                                    │
                                    v
                   ┌────────────────────────────────┐
                   │     Event + Signal Extractor   │
                   │ (facts, prefs, tasks, entities)│
                   └───────────────┬────────────────┘
                                   │
                     ┌─────────────┴─────────────┐
                     │                           │
                     v                           v
        ┌──────────────────────────┐   ┌──────────────────────────┐
        │  Temporal SQLCipher DB   │   │    sqlite-vec Vectors    │
        │  (structured memories)   │   │ (embeddings + rerank)    │
        └─────────────┬────────────┘   └─────────────┬────────────┘
                      │                              │
                      v                              v
        ┌──────────────────────────┐   ┌──────────────────────────┐
        │   Memory Summarizer      │   │  Semantic Recall + Rank  │
        │ (compression + pruning)  │   │ (query-time retrieval)   │
        └─────────────┬────────────┘   └─────────────┬────────────┘
                      └──────────────┬───────────────┘
                                     v
                        ┌────────────────────────┐
                        │   Context Assembler    │
                        │ (chat + tools + agent) │
                        └────────────────────────┘
```

### Temporal knowledge graph (what “temporal” means here)

Memory entries are stored as time-ordered events and entities in the SQLCipher database. Each fact, preference, reminder, and decision is recorded with timestamps and relationships, so recall can answer questions like “when did we decide this?” or “what changed since last week?” without relying on lossy summaries. This timeline-first structure is what makes the memory system a temporal knowledge graph rather than a static summary.

### Why this beats “just summarization” or QMD

- Summaries alone lose details. The system stores structured facts in SQLCipher and semantic traces in sqlite-vec so exact preferences, dates, and decisions remain queryable even after summarization.
- QMD-style recall can miss context. Dual storage (structured + vectors) plus reranking yields higher recall and fewer false positives.
- Temporal memory matters. The DB keeps time-ordered events so the assistant can answer “when did we decide X?” without relying on brittle summary phrasing.
- Safer pruning. Summarization is used for compression, not replacement, so older context is condensed while retaining anchors for precise retrieval.
- Faster, cheaper queries. Quick structured lookups handle facts and tasks; semantic search handles fuzzy recall, keeping prompts smaller and more relevant.

### Privacy & Security & Always On

- Run locally with Ollama to keep requests and model inference private on your machine.
- Designed for always-on use with unlimited token use (local inference) and customized wakeup and task intervals.
- Conversation data and memory are only stored locally.
- Config JSON is stored in the OS keychain.
- SQLite data is encrypted at rest via SQLCipher when a DB key is set.

### Prerequisites

- Rust (via rustup): https://rustup.rs (only if `cargo` installing)
- Ollama is auto-installed on Linux at first run (via `curl -fsSL https://ollama.com/install.sh | sh`) when local Ollama is configured.

### Requirements

#### Ubuntu
- Rust 1.93+
- 16GB+ RAM with 8GB+ VRAM (for Ubuntu)
- Certain system libraries for Ubuntu (only if using `cargo` install)
- 16GB+ RAM with M2 Pro (for Mac)

#### Models Used

- ministral-3:14b (assistant + summaries)
- embeddinggemma:latest (embedding)
- qllama/bge-reranker-v2-m3 (reranking)

Models auto-download and install if not already installed.

#### Test Systems

- AMD Threadripper 2950X with 128GB DDR4 with AMD 7900XTX on Ubuntu 24.04.3 (instant response)
- MSI Raider GE68-HX-14V on Ubuntu 24.04.3 (instant response)
- M2 Pro Mac Mini with 16GB RAM (~10 second responses)

### Build

```bash
cargo build --release
```

### Test

```bash
cargo test
```

Coverage (llvm-cov):

```bash
rustup component add llvm-tools-preview
cargo install cargo-llvm-cov
cargo llvm-cov --workspace --tests --lcov --output-path lcov.info
```

If your environment prompts for keychain/keyring access during tests, disable keyring usage for that run:

```bash
BUTTERFLY_BOT_DISABLE_KEYRING=1 cargo test
```

## Run

```bash
cargo run --release --bin butterfly-bot
```

### Debian package via Dioxus (`.deb`)

If you want to avoid Snap for local testing, build a Debian package directly:

```bash
./scripts/build-deb.sh
```

Install the generated package:

```bash
sudo dpkg -i /path/to/generated/butterfly-bot*.deb
```

If `dx` is missing:

```bash
cargo install dioxus-cli
```

Run the packaged commands:

```bash
butterfly-bot
snap run butterfly-bot.butterfly-bot-ui
snap run butterfly-bot.butterfly-botd
```

Daemon service (optional) is shipped disabled by default and can be managed with:

```bash
sudo snap start butterfly-bot.butterfly-botd
sudo snap stop butterfly-bot.butterfly-botd
```

Notes:

- Snap launchers set a writable app root under `$SNAP_USER_COMMON/butterfly-bot`.
- The default DB path is `$SNAP_USER_COMMON/butterfly-bot/data/butterfly-bot.db`.
- Bundled modules are mounted at `./wasm/<tool>_tool.wasm` inside the app runtime directory.
- `BUTTERFLY_BOT_DISABLE_KEYRING=1` is enabled by default in the snap launcher (override if your snap environment provides a working keyring backend).

### macOS app bundle via Dioxus (`.app`)

Build the macOS app bundle and a zipped release artifact:

```bash
./scripts/build-macos-app.sh
```

Open the generated app:

```bash
open /path/to/generated/ButterflyBot.app
```

If `dx` is missing:

```bash
cargo install dioxus-cli
```

Release artifact output:

- `dist/ButterflyBot.app`
- `dist/ButterflyBot_<version>_<arch>.app.zip`



### How To (Context, Heartbeat, Config)

Use this quick sequence for best results with minimal setup:

1. Set your Context first
    - Open the `Context` tab and paste your operating context (goals, constraints, preferences, project notes).
    - Keep it short and actionable; update only when your priorities change.

2. Start Heartbeat for always-on automation
    - Open `Heartbeat` and start the loop so the daemon can run wakeups, tasks, reminders, and tool actions continuously.
    - If you want fewer background checks, lower activity by increasing the wakeup interval in Config.

3. Use Config only when needed
    - Open `Config` to set required secrets and connectivity:
    - Zapier token (primary; most integrations rely on this)
    - GitHub token
      - Coding OpenAI API key
      - Search provider + search API key
      - MCP servers
      - Network allow list
    - If something fails with a tool call, check provider/key/allowlist first before changing anything else.
    - Config is stored in the OS keychain for top security and safety.

### Diagnostics & Security Audit

- Health diagnostics and security audit capabilities remain available at the daemon/API layer.
- Security posture guidance and limits are documented in [docs/security-audit.md](docs/security-audit.md).

### Threat Model (Important)

- Butterfly Bot now has a formal attacker model and trust-boundary definition.
- This covers UI↔daemon, daemon↔tool runtime, daemon↔provider, and daemon↔storage boundaries.
- Primary threats covered: plaintext secret leakage, tool capability escalation, over-permissive network egress, and daemon auth misuse.
- Baseline controls include OS keychain-backed secrets, WASM-only execution for built-in tools, daemon auth checks, and `default_deny` network posture guidance.
- See [docs/threat-model.md](docs/threat-model.md) for full assumptions, residual risks, and hardening priorities.

### Memory LLM Configuration

- `memory.openai` lets memory operations (embeddings, summarization, reranking) use a different OpenAI-compatible provider than the main agent.
- This is useful for running the agent on a remote provider (e.g., Cerebras) while keeping memory on a local Ollama instance.

### Convention Mode (WASM-only tools)

- Tool execution is WASM-only for all built-in tools.
- Startup now validates WASM module integrity (magic header) for registered tools and fails fast on invalid/corrupted binaries.
- Per-tool `runtime` config is ignored; tool execution is WASM-only.
- Per-tool `wasm.module` is optional and defaults to `./wasm/<tool>_tool.wasm`.
- Zero-config path: place modules at `./wasm/<tool>_tool.wasm` for each tool you run.
- Convention defaults include a deny-by-default network posture with allowlisted domains including `mcp.zapier.com`.

Build all default tool modules:

```bash
./scripts/build_wasm_tools.sh
```

This generates:

- `./wasm/coding_tool.wasm`
- `./wasm/mcp_tool.wasm`
- `./wasm/http_call_tool.wasm`
- `./wasm/github_tool.wasm`
- `./wasm/zapier_tool.wasm`
- `./wasm/planning_tool.wasm`
- `./wasm/reminders_tool.wasm`
- `./wasm/search_internet_tool.wasm`
- `./wasm/tasks_tool.wasm`
- `./wasm/todo_tool.wasm`
- `./wasm/wakeup_tool.wasm`

```
tool call
   │
   v
┌───────────────────────────────┐
│ Sandbox planner               │
│ (WASM-only invariant)         │
└───────────────┬───────────────┘
                │
      ┌─────────v─────────┐
      │ WASM runtime      │
      │ ./wasm/<tool>_tool│
      │ .wasm             │
      └───────────────────┘
```

## SQLCipher (encrypted storage)

Butterfly Bot uses SQLCipher-backed SQLite when you provide a DB key.

Set the environment variable before running:

```bash
export BUTTERFLY_BOT_DB_KEY="your-strong-passphrase"
```

If no key is set, storage falls back to plaintext SQLite.

## Competitive Feature Matrix (Butterfly Bot vs OpenClaw, ZeroClaw, IronClaw)

### Positioning Snapshot

- **Butterfly Bot (this repo):** Practical personal-agent workflows with daemon + UI + planning/todo/tasks/reminders/wakeup + memory.
- **OpenClaw (main competitor):** Full personal-assistant platform with broad channels and plugin ecosystem, but currently high operational security risk for typical deployments.
- **ZeroClaw:** Lean, pluggable Rust agent framework with strong onboarding story and broad provider/channel coverage.
- **IronClaw:** Platform-style architecture emphasizing sandboxed extensibility (WASM), orchestration, routines, and gateway capabilities.

### Feature Matrix

Legend: **✅ strong**, **🟨 partial/limited**, **❌ not evident**

| Area | Butterfly Bot | OpenClaw | ZeroClaw | IronClaw |
|---|---:|---:|---:|---:|
| Rust core implementation | ✅ | ❌ (TypeScript-first) | ✅ | ✅ |
| Interactive UI included | ✅ (Dioxus UI) | ✅ (Control UI + WebChat) | 🟨 (CLI-first) | ✅ (TUI/Web gateway) |
| Local daemon/service model | ✅ | ✅ | ✅ | ✅ |
| Config persistence + reload path | ✅ | ✅ | ✅ | ✅ |
| Provider abstraction | ✅ | ✅ | ✅ | ✅ |
| Broad multi-provider catalog | ✅ (relies on Zapier) | ✅ | ✅ | 🟨 (focused provider path + adapters) |
| Agent extension architecture | ✅ (Rust-native modules + MCP integrations; maintainer-curated) | ✅ (plugins/extensions) | ✅ | ✅ |
| Secure tool sandbox model (explicit) | ✅ | 🟨 (sandbox/policy flows exist, but high-risk defaults and misconfiguration exposure remain common) | ✅ | ✅ |
| Memory subsystem | ✅ (SQLite + sqlite-vec hybrid search) | ✅ (core memory + LanceDB plugin path) | ✅ (SQLite/Markdown + hybrid search) | ✅ (workspace memory + hybrid search) |
| Planning + todo/task orchestration | ✅ (native modules) | 🟨 | 🟨 | ✅ |
| Scheduled reminders/heartbeat style automation | ✅ | ✅ | ✅ | ✅ (routines/heartbeat) |
| End-user dynamic plugin building | ❌ (intentional: convention-over-configuration) | 🟨 (plugin/extensibility strong, not builder-centric) | ❌ | ✅ (WASM-oriented builder flow) |
| Zero-step onboarding (no wizard required) | ✅ | ✅ | ✅ | ✅ |
| Documentation breadth for contributors | ✅ | ✅ | ✅ | ✅ |
| Explicit security hardening docs/checklists | ✅ | ✅ | ✅ | ✅ |
| Test breadth/visibility | ✅ | ✅ | ✅ | 🟨 |

### Weighted Scorecard (Personal Ops Agent Lens)

Scoring model:
- Score each criterion from **1 to 5** (5 = strongest).
- Weight reflects importance for a **personal operations assistant** product.
- Weighted score per row = `score × weight`.
- Total possible = **500** (if all criteria scored 5).

#### Criteria and Weights

| Criterion | Weight (%) | Why it matters |
|---|---:|---|
| Workflow completeness (plan→task→reminder→done) | 20 | Core product value for daily execution. |
| Reliability and failure recovery | 20 | Users trust consistency more than raw feature count. |
| UX and operator visibility | 15 | Faster adoption and better day-2 usability. |
| Security posture and secret hygiene | 15 | Critical for real-world deployment and trust. |
| Setup/onboarding speed | 10 | Strong determinant of conversion and retention. |
| Integration leverage and extensibility | 10 | Measures practical capability breadth, including MCP partner surfaces (e.g., Zapier) and native agent extension velocity. |
| Documentation and contributor DX | 10 | Impacts community velocity and maintainability. |

### Current Scoring (Post-Ship Estimate)

Scoring reflects current shipped state for Butterfly Bot after landing local golden-path reliability checks, execution-trace sanity coverage, and trace redaction hardening. It should still be revised as competitors evolve.

| Criterion | Weight | Butterfly Bot | OpenClaw | ZeroClaw | IronClaw |
|---|---:|---:|---:|---:|---:|
| Workflow completeness | 20 | 5 | 4 | 3 | 4 |
| Reliability and recovery | 20 | 5 | 3 | 4 | 3 |
| UX and visibility | 15 | 5 | 4 | 3 | 4 |
| Security posture | 15 | 5 | 1 | 5 | 4 |
| Setup/onboarding | 10 | 5 | 4 | 5 | 4 |
| Integration leverage/extensibility | 10 | 4 | 5 | 5 | 5 |
| Docs/contributor DX | 10 | 5 | 4 | 5 | 4 |
| **Total Weighted (/500)** | **100** | **490** | **345** | **415** | **390** |

### Path to 500 (Integration Leverage 4→5)

To reach **500/500**, the remaining criterion is **Integration leverage/extensibility** (currently 4/5, weight 10).

Definition of **5/5** for Integration leverage/extensibility:

- Ship **at least 5** operator-ready integration playbooks (for example MCP/Zapier/provider workflows) with reproducible steps.
- Each playbook includes: prerequisites, exact configuration, expected outputs, failure modes, and recovery/rollback steps.
- Add a tested compatibility table (integration surface + version/provider assumptions + last validation date).
- Add repeatable verification checks (local smoke tests or scripted validation) for every published playbook.
- Publish reliability evidence for those integration paths (success rate + retry behavior over repeated runs).

Exit rule for score update:

- Keep Integration leverage/extensibility at **4/5** until all criteria above are met and documented.
- Move Integration leverage/extensibility to **5/5** only after evidence is published; total then becomes **500/500**.

### License

MIT
