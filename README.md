## Butterfly Bot

Butterfly Bot is an opinionated personal-ops AI assistant built for people who want results, not setup overhead.

Open the app, provide the prompt, and watch your always-on agent run with memory, tools, and full visibility.

Why users pick it:

- **Fast to first value:** works out-of-the-box with default settings.
- **UI-first operator experience:** polished desktop chat, streaming responses, diagnostics, security audit, and live execution events.
- **Real automation, not just chat:** native planning/todo/tasks/reminders/wakeup modules for always-on personal operations.
- **Integration leverage:** MCP-based tooling (including Zapier) plus built-in tools for web, coding, scheduling, and reminders.
- **Security-focused local posture:** keychain-backed secrets, local memory/storage paths, and WASM-only tool runtime policy.

## Highlights

- Liquid Glass cross-platform desktop app with streaming chat.
- Plan → task → reminder workflow support with native modules.
- Reminders in chat and OS notifications.
- Long-term agentic memory stored locally.
- Optional prompt-context/heartbeat Markdown overrides.
- Agent tools including MCP (Zapier-compatible via MCP server setup).
- Config stored in the OS keychain for maximum security.
- UI launch auto-bootstraps default config and starts the local daemon automatically.

## Architecture (Daemon + UI + Always-On Agent)

```
        ┌──────────────────────────────────────┐
        │           Desktop UI (Dioxus)        │
        │  - chat, config, notifications       │
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
    │ (SQLCipher +   │  │ (MCP, HTTP,   │  │ (Ollama/OpenAI)  │
    │  LanceDB)      │  │ reminders,    │  │                  │
    │                │  │ tasks, etc.)  │  │                  │
    │                │  │ + WASM sandbox│  │                  │
    │                │  │   runtime     │  │                  │
    └────────────────┘  └───────────────┘  └──────────────────┘
```

## Memory System (Diagram + Rationale)

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
        │  Temporal SQLCipher DB   │   │      LanceDB Vectors     │
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

- Summaries alone lose details. The system stores structured facts in SQLCipher and semantic traces in LanceDB so exact preferences, dates, and decisions remain queryable even after summarization.
- QMD-style recall can miss context. Dual storage (structured + vectors) plus reranking yields higher recall and fewer false positives.
- Temporal memory matters. The DB keeps time-ordered events so the assistant can answer “when did we decide X?” without relying on brittle summary phrasing.
- Safer pruning. Summarization is used for compression, not replacement, so older context is condensed while retaining anchors for precise retrieval.
- Faster, cheaper queries. Quick structured lookups handle facts and tasks; semantic search handles fuzzy recall, keeping prompts smaller and more relevant.

## Privacy & Security & Always On

- Run locally with Ollama to keep requests and model inference private on your machine.
- Designed for always-on use with unlimited token use (local inference) and customized wakeup and task intervals.
- Conversation data and memory are only stored locally.
- Config JSON is stored in the OS keychain.
- SQLite data is encrypted at rest via SQLCipher when a DB key is set.

## Install Prerequisites

- Rust (via rustup): https://rustup.rs
- Ollama (platform-specific installers): https://ollama.com/download

## Ollama

### Requirements

### Linux or Windows (WSL)
- Rust 1.93+
- Ubuntu recommended
- 16GB+ RAM with 8GB+ VRAM
- Certain system libraries for Linux

### Mac
- Rust 1.93+
- Mac Tahoe
- 16GB+ RAM
- M2 Pro+

### Models Used

- ministral-3:14b (assistant + summaries)
- embeddinggemma:latest (embedding)
- qllama/bge-reranker-v2-m3 (reranking)

### Model Notes
- When using a local Ollama base URL, `butterfly-bot` will automatically pull missing models on startup and let you know while they load.
- Ollama models can be overriden and other models can be used rather than the default ones.

### Test Systems

- AMD Threadripper 2950X with 128GB DDR4 with AMD 7900XTX on Ubuntu 24.04.3 (instant response)
- MSI Raider GE68-HX-14V on Ubuntu 24.04.3 (instant response)
- Mac M2 Pro Mini with 16GB RAM (10-20 seconds per response)
- Not tested on Windows (WSL)

## OpenAI 

### Requirements

- Rust 1.93+
- Certain system libraries for the host OS
- Mac or Linux or Windows (WSL)

### Model Recommendations

- No recommendations at this time as no testing of OpenAI has been done

## Build

```bash
cargo build --release
```

## Run

```bash
cargo run --release --bin butterfly-bot
```

## Config

Butterfly Bot uses convention-first defaults. You can run without editing config files, then override settings only when needed.

- Default-first behavior: provider/model/storage/tool defaults are preselected.
- Inline blocking prompts only: if a required secret is missing, the app requests it when needed.
- Optional overrides: use the Config tab for advanced customization.
- Factory reset: use **Config → Factory Reset** to restore convention defaults.
- Zero-step startup: launching `butterfly-bot` or `butterfly-bot-ui` writes convention defaults when missing and attempts to start the daemon automatically.

Config is stored in the OS keychain for top security and safety.

### Diagnostics (Doctor)

- Use **Config → Diagnostics → Run Diagnostics** to run health checks in the app UX.
- Checks include config load/parse, vault resolution, DB read/write probe, provider reachability, and daemon auth token status.
- Results are returned as pass/warn/fail with actionable fix hints so bug reports are easier to reproduce and resolve.

### Security Audit (UI)

- Use **Config → Security Audit → Run Security Audit** to run local security posture checks.
- Findings are risk-ranked as **critical/high/medium/low** and include fix guidance.
- The audit is intentionally **read-only**: no automatic config mutations are applied.
- `auto_fixable` is currently informational and defaults to `false` because automatic hardening can create unintended downtime.
- If a finding cannot be safely auto-remediated, guidance is presented as explicit manual steps.
- See [docs/security-audit.md](docs/security-audit.md) for operating recommendations and limits.

### Threat Model (Important)

- Butterfly Bot now has a formal attacker model and trust-boundary definition.
- This covers UI↔daemon, daemon↔tool runtime, daemon↔provider, and daemon↔storage boundaries.
- Primary threats covered: plaintext secret leakage, tool capability escalation, over-permissive network egress, and daemon auth misuse.
- Baseline controls include OS keychain-backed secrets, WASM-first high-risk tool runtime defaults, daemon auth checks, and `default_deny` network posture guidance.
- See [docs/threat-model.md](docs/threat-model.md) for full assumptions, residual risks, and hardening priorities.

### Prompt Context & Heartbeat

- `prompt_source` is optional Markdown (URL or inline database markdown) for custom assistant identity/style/rules.
- `heartbeat_file` is optional Markdown (local path or URL) appended for periodic guidance.
- `prompt_file` is optional Markdown (local path or URL) for extra instructions.
- If these files are omitted, built-in defaults are used.
- The heartbeat file is reloaded on every wakeup tick (using `tools.wakeup.poll_seconds`) so changes take effect without a restart.
- Boot preload uses a **fast path**: context/prompt/heartbeat warmup is time-bounded and long operations continue in deferred background hydration.
- This keeps startup responsive while preserving lazy/on-demand context import for first real work.

### Memory LLM Configuration

- `memory.openai` lets memory operations (embeddings, summarization, reranking) use a different OpenAI-compatible provider than the main agent.
- This is useful for running the agent on a remote provider (e.g., Cerebras) while keeping memory on a local Ollama instance.

### Advanced: config.json override example

This is optional and intended for advanced customization.

```json
{
  "openai": {
    "api_key": null,
    "model": "ministral-3:14b",
    "base_url": "http://localhost:11434/v1"
  },
  "heartbeat_source": {
    "type": "database",
    "markdown": "# Heartbeat\n\nStay proactive, grounded, and transparent. Prefer clear next steps and avoid over-claiming."
  },
  "prompt_source": {
    "type": "database",
    "markdown": "# Prompt\n\nAnswer directly, include concrete actions, and keep responses practical."
  },
  "memory": {
    "enabled": true,
    "sqlite_path": "./data/butterfly-bot.db",
    "lancedb_path": "./data/lancedb",
    "summary_model": "ministral-3:14b",
    "embedding_model": "embeddinggemma:latest",
    "rerank_model": "qllama/bge-reranker-v2-m3",
    "openai": null,
    "context_embed_enabled": false,
    "summary_threshold": null,
    "retention_days": null
  },
  "tools": null,
  "brains": null
}
```

### Convention Mode (WASM-only tools)

- Tool execution is WASM-only for all built-in tools.
- `tools.settings.sandbox.mode` remains accepted as config but does not bypass WASM-only execution.
- Per-tool `runtime` config is ignored; tool execution is WASM-only.
- Per-tool `wasm.module` is optional and defaults to `./wasm/<tool>_tool.wasm`.
- Zero-config path: place modules at `./wasm/<tool>_tool.wasm` for each tool you run.

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

## Tools

### MCP Tool

The MCP tool supports connection type, custom headers, and multiple servers at once.

There are many high-quality MCP server providers like: 

* [Zapier](https://zapier.com/mcp) - 7,000+ app connections via MCP

* [VAPI.AI](https://vapi.ai) - Voice Agent Telephony

Config fields:
- `tools.mcp.servers` (required to use MCP)
    - `name` (required)
    - `url` (required)
    - `type` (optional, defaults to `sse`; supports `sse`, `http`, or `streamable-http`)
    - `headers` (optional)

```json
{
    "tools": {
        "mcp": {
            "servers": [
                {
                    "name": "local",
                    "type": "sse",
                    "url": "http://127.0.0.1:3001/sse",
                    "headers": {
                        "Authorization": "Bearer my-token"
                    }
                }
            ]
        }
    }
}
```

HTTP (streamable) example:

```json
{
    "tools": {
        "mcp": {
            "servers": [
                {
                    "name": "github",
                    "type": "http",
                    "url": "https://api.githubcopilot.com/mcp/",
                    "headers": {
                        "Authorization": "Bearer YOUR_TOKEN"
                    }
                }
            ]
        }
    }
}
```

### GitHub Tool (MCP wrapper)

Use the built-in GitHub tool to call GitHub MCP tools with a single PAT. This tool uses MCP under the hood, so you don't need to define MCP servers directly if you don't want to.

Config fields:
- `tools.github.pat` (optional; can also come from vault secret `github_pat`)
- `tools.github.url` (optional; defaults to `https://api.githubcopilot.com/mcp/`)
- `tools.github.type` (optional; defaults to `http`)
- `tools.github.headers` (optional; additional headers)

```json
{
        "tools": {
        "github": {
            "pat": "YOUR_GITHUB_PAT",
            "url": "https://api.githubcopilot.com/mcp/",
            "type": "http"
        }
    }
}
```

### Coding Tool (Codex)

Use a dedicated coding model for Solana backend and Solana smart contract work without changing the main runtime model.

Config fields:
- `tools.coding.api_key` (optional; can also come from vault secret `coding_openai_api_key`)
- `tools.coding.model` (optional; defaults to `gpt-5.2-codex`)
- `tools.coding.base_url` (optional; defaults to `https://api.openai.com/v1`)
- `tools.coding.system_prompt` (optional; overrides default coding system prompt)

```json
{
    "tools": {
        "coding": {
            "api_key": "YOUR_OPENAI_KEY",
            "model": "gpt-5.2-codex",
            "base_url": "https://api.openai.com/v1"
        }
    }
}
```

### Internet Search Tool

The Internet Search tool supports 3 different providers: `openai`, `grok`, and `perplexity`.

Configure the internet search tool under `tools.search_internet`:

Config fields:
- `api_key` (optional; can also come from vault secrets or `openai.api_key`)
- `provider` (optional; defaults to `openai`)
- `model` (optional; defaults by provider)
- `citations` (optional; defaults to `true`)
- `grok_web_search` (optional; defaults to `true`)
- `grok_x_search` (optional; defaults to `true`)
- `grok_timeout` (optional; defaults to `90`)
- `permissions.network_allow` (optional allowlist for outbound domains)
- `permissions.default_deny` (optional; defaults to `true` from code-level convention defaults)
- `tools.settings.permissions.*` (optional global defaults; tool-level `permissions` can override `network_allow`)

```json
{
    "tools": {
        "search_internet": {
            "api_key": "YOUR_API_KEY",
            "provider": "openai",
            "model": "gpt-4o-mini-search-preview",
            "citations": true,
            "grok_web_search": true,
            "grok_x_search": true,
            "grok_timeout": 90,
            "permissions": {
                "network_allow": ["api.openai.com"],
                "default_deny": true
            }
        }
    }
}
```

### Wakeup Tool

The wakeup tool runs scheduled tasks on an interval.

Wakeup runs are also streamed to the UI event feed as tool messages.

Create recurring agent tasks with `tools.wakeup`, control polling, and log runs to an audit file:

Config fields:
- `poll_seconds` (optional; defaults to `60`)
- `sqlite_path` (optional; defaults to `./data/butterfly-bot.db`)
- `audit_log_path` (optional; defaults to `./data/wakeup_audit.log`)

```json
{
    "tools": {
        "wakeup": {
            "poll_seconds": 60,
            "audit_log_path": "./data/wakeup_audit.log"
        }
    }
}
```

### HTTP Call Tool

HTTP Call tool can call any public endpoint and private endpoint (if base url and authorization is provided).

Endpoints can be discovered by the agent or provided in the system/user prompts.

Call external APIs with arbitrary HTTP requests and custom headers. Configure defaults under `tools.http_call`:

Config fields:
- `base_url` (optional)
- `default_headers` (optional)
- `timeout_seconds` (optional; defaults to `60`)

```json
{
    "tools": {
        "http_call": {
            "base_url": "https://api.example.com",
            "default_headers": {
                "Authorization": "Bearer YOUR_TOKEN"
            },
            "timeout_seconds": 60
        }
    }
}
```

### Todo Tool

Ordered todo list backed by SQLite for the agent to created todo lists:

Config fields:
- `sqlite_path` (optional; defaults to `./data/butterfly-bot.db`)

```json
{
    "tools": {
        "todo": {
            "sqlite_path": "./data/butterfly-bot.db"
        }
    }
}
```

### Planning Tool

Structured plans with goals and steps for the agent to create plans:

Config fields:
- `sqlite_path` (optional; defaults to `./data/butterfly-bot.db`)

```json
{
    "tools": {
        "planning": {
            "sqlite_path": "./data/butterfly-bot.db"
        }
    }
}
```

### Tasks Tool

Schedule one-off or recurring tasks with cancellation support for the agent to create tasks:

Config fields:
- `poll_seconds` (optional; defaults to `60`)
- `audit_log_path` (optional; defaults to `./data/tasks_audit.log`)
- `sqlite_path` (optional; defaults to `./data/butterfly-bot.db`)

```json
{
    "tools": {
        "tasks": {
            "poll_seconds": 60,
            "audit_log_path": "./data/tasks_audit.log",
            "sqlite_path": "./data/butterfly-bot.db"
        }
    }
}
```

### Reminders Tool

The reminders tool is for users to create reminders for themselves or for the agent to create reminders for the user.

Create, list, complete, delete, and snooze reminders. Configure storage under `tools.reminders` (falls back to `memory.sqlite_path` if omitted):

Config fields:
- `sqlite_path` (optional; defaults to `./data/butterfly-bot.db` and falls back to `memory.sqlite_path` when set)

```json
{
    "tools": {
        "reminders": {
            "sqlite_path": "./data/butterfly-bot.db"
        }
    }
}
```

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
| Memory subsystem | ✅ (SQLite + LanceDB paths/config) | ✅ (core memory + LanceDB plugin path) | ✅ (SQLite/Markdown + hybrid search) | ✅ (workspace memory + hybrid search) |
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

## License

MIT
