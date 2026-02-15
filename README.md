## Butterfly Bot

`Butterfly Bot` is your personal AI assistant accessible via a native desktop app. It includes memory, tool integrations, convention-first defaults, and streaming responses in a polished UI. The codebase still exposes a Rust library for building bots, but the primary focus is the app experience.

## Highlights

- Liquid Glass cross-platform desktop app with streaming chat.
- Reminders and notifications both in chat and OS notifications.
- Long-term agentic memory stored locally.
- Optional prompt-context/heartbeat Markdown overrides.
- Agent tools including MCP.
- Config stored in the OS keychain for maximum security.
- No first-run wizard required for the default path.

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

### How this enables an always-on agent

- The agent is always-on only while the daemon is running. The daemon owns the scheduler, wakeups, and tool execution.
- If the UI shuts down and the daemon is also stopped, the agent will pause until the daemon is started again.
- Persistent memory and task queues live in the daemon’s storage, preserving context across restarts and long idle periods.

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

Launches the desktop app.

## Config

Butterfly Bot uses convention-first defaults. You can run without editing config files, then override settings only when needed.

- Default-first behavior: provider/model/storage/tool defaults are preselected.
- Inline blocking prompts only: if a required secret is missing, the app requests it when needed.
- Optional overrides: use the Config tab for advanced customization.
- Factory reset: use **Config → Factory Reset** to restore convention defaults.

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

### Prompt Context & Heartbeat

- `prompt_source` is optional Markdown (URL or inline database markdown) for custom assistant identity/style/rules.
- `heartbeat_file` is optional Markdown (local path or URL) appended for periodic guidance.
- `prompt_file` is optional Markdown (local path or URL) for extra instructions.
- If these files are omitted, built-in defaults are used.
- The heartbeat file is reloaded on every wakeup tick (using `tools.wakeup.poll_seconds`) so changes take effect without a restart.

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
    "prompt_source": {"type": "url", "url": "https://example.com/prompt.md"},
    "heartbeat_source": {"type": "url", "url": "https://example.com/heartbeat.md"},
    "memory": {
        "enabled": true,
        "sqlite_path": "./data/butterfly-bot.db",
        "lancedb_path": "./data/lancedb",
        "summary_model": "ministral-3:14b",
        "embedding_model": "embeddinggemma:latest",
        "rerank_model": "qllama/bge-reranker-v2-m3",
        "openai": {
            "api_key": null,
            "model": "ministral-3:14b",
            "base_url": "http://localhost:11434/v1"
        },
        "summary_threshold": null,
        "retention_days": null
    },
    "tools": {
        "settings": {
            "audit_log_path": "./data/tool_audit.log"
        },
        "reminders": {
            "sqlite_path": "./data/butterfly-bot.db"
        },
        "wakeup": {
            "poll_seconds": 60,
            "sqlite_path": "./data/butterfly-bot.db",
            "audit_log_path": "./data/wakeup_audit.log"
        },
        "todo": {
            "sqlite_path": "./data/butterfly-bot.db"
        },
        "planning": {
            "sqlite_path": "./data/butterfly-bot.db"
        },
        "tasks": {
            "poll_seconds": 60,
            "audit_log_path": "./data/tasks_audit.log",
            "sqlite_path": "./data/butterfly-bot.db"
        }
    },
    "brains": {
        "settings": {
            "tick_seconds": 60
        }
    }
}
```

### Convention Mode (WASM sandbox defaults)

- Butterfly Bot defaults to `tools.settings.sandbox.mode = non_main`.
- High-risk tools (`coding`, `mcp`, `http_call`) run in WASM by default.
- Per-tool `runtime` is optional.
- Per-tool `wasm.module` is optional and defaults to `./wasm/<tool>_tool.wasm`.
- Zero-config path: place modules at `./wasm/coding_tool.wasm`, `./wasm/mcp_tool.wasm`, and `./wasm/http_call_tool.wasm`.
- Set `tools.settings.sandbox.mode = off` only when you explicitly want native runtime.

```
tool call
   │
   v
┌───────────────────────────────┐
│ Sandbox planner (default mode │
│ is non_main)                  │
└───────────────┬───────────────┘
                │
        ┌───────┴────────┐
        │ high-risk tool?│
        │ coding/mcp/http│
        └───────┬────────┘
            yes │ no
                │
      ┌─────────v─────────┐      ┌───────────────────┐
      │ WASM runtime      │      │ Native runtime    │
      │ ./wasm/<tool>_tool│      │ tool.execute(...) │
      │ .wasm             │      └───────────────────┘
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

* [GitHub](https://github.com) - Coding

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
- `permissions.default_deny` (optional; defaults to `false`)
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
                "default_deny": false
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

## Library Usage (Minimal)

If you still want to embed Butterfly Bot, the Rust API is available:

```rust
use futures::StreamExt;
use butterfly_bot::client::ButterflyBot;

#[tokio::main]
async fn main() -> butterfly_bot::Result<()> {
    let agent = ButterflyBot::from_config_path("config.json").await?;
    let mut stream = agent.process_text_stream("user123", "Hello!", None);
    while let Some(chunk) = stream.next().await {
        print!("{}", chunk?);
    }
    Ok(())
}
```

## License

MIT
