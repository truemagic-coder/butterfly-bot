# ButterFly Bot (Rust)

ButterFly Bot is a Rust-first framework for building multi-agent AI assistants with simple routing, optional memory, and a CLI. This repository contains the Rust rewrite only.

## Features

- Async Rust client and CLI
- JSON-based configuration
- Multi-agent routing by specialization
- In-memory conversation history
- OpenAI Responses API support

## Requirements

- Rust 1.93 or newer
- An OpenAI API key, or an OpenAI-compatible endpoint (e.g., Ollama)

## Installation

Use the crate as a library:

```bash
cargo add butterfly-bot
```

Or build the CLI from this repository:

```bash
cargo build --release
```

## Quickstart (CLI)

Create a config file (config.json):

```json
{
  "openai": {
    "api_key": "your-openai-api-key",
    "model": "gpt-5.2"
  },
  "agents": [
    {
      "name": "default_agent",
      "instructions": "You are a helpful AI assistant.",
      "specialization": "general"
    }
  ]
}
```

Run a single prompt:

```bash
butterfly-bot --config config.json --user-id cli_user --prompt "Hello!"
```

Or start an interactive session:

```bash
butterfly-bot --config config.json --user-id cli_user
```

## Ollama (OpenAI-Compatible)

Ollama exposes an OpenAI-compatible API. Point the `base_url` to your local Ollama server and set a model that Ollama has pulled.

```json
{
  "openai": {
    "base_url": "http://localhost:11434/v1",
    "model": "gpt-oss:20b"
  },
  "agents": [
    {
      "name": "default_agent",
      "instructions": "You are a helpful AI assistant.",
      "specialization": "general"
    }
  ]
}
```

Note: when `base_url` is set, the API key is optional.

## Library Usage

Create a client and stream the response:

```rust
use futures::StreamExt;
use butterfly_bot::client::ButterflyBot;

#[tokio::main]
async fn main() -> butterfly_bot::Result<()> {
  let agent = ButterflyBot::from_config_path("config.json").await?;
    let mut stream = agent.process_text_stream("user123", "Hello!", None);

    while let Some(chunk) = stream.next().await {
        let text = chunk?;
        print!("{}", text);
    }

    Ok(())
}
```

## Unified Process API

Use the unified `process` API for audio, images, and structured output:

```rust
use butterfly_bot::{
  ImageData, ImageInput, OutputFormat, ProcessOptions, ProcessResult, UserInput,
  ButterflyBot,
};

#[tokio::main]
async fn main() -> butterfly_bot::Result<()> {
  let agent = ButterflyBot::from_config_path("config.json").await?;

  let options = ProcessOptions {
    prompt: None,
    images: vec![ImageInput { data: ImageData::Url("https://example.com/cat.png".to_string()) }],
    output_format: OutputFormat::Text,
    image_detail: "auto".to_string(),
    json_schema: None,
    router: None,
  };

  let result = agent
    .process(
      "user123",
      UserInput::Text("Describe the image".to_string()),
      options,
    )
    .await?;

  if let ProcessResult::Text(text) = result {
    println!("{}", text);
  }

  Ok(())
}
```

### Audio Input and Output

```rust
use butterfly_bot::{OutputFormat, ProcessOptions, ProcessResult, UserInput, ButterflyBot};

#[tokio::main]
async fn main() -> butterfly_bot::Result<()> {
  let agent = ButterflyBot::from_config_path("config.json").await?;

  let options = ProcessOptions {
    prompt: None,
    images: vec![],
    output_format: OutputFormat::Audio { voice: "nova".to_string(), format: "aac".to_string() },
    image_detail: "auto".to_string(),
    json_schema: None,
    router: None,
  };

  let audio_input = std::fs::read("./input.wav")?;
  let result = agent
    .process(
      "user123",
      UserInput::Audio { bytes: audio_input, input_format: "wav".to_string() },
      options,
    )
    .await?;

  if let ProcessResult::Audio(bytes) = result {
    std::fs::write("./output.aac", bytes)?;
  }

  Ok(())
}
```

### Structured Output

```rust
use serde_json::json;
use butterfly_bot::{OutputFormat, ProcessOptions, ProcessResult, UserInput, ButterflyBot};

#[tokio::main]
async fn main() -> butterfly_bot::Result<()> {
  let agent = ButterflyBot::from_config_path("config.json").await?;

  let schema = json!({
    "name": "extract",
    "schema": {
      "type": "object",
      "properties": {
        "topic": { "type": "string" },
        "summary": { "type": "string" }
      },
      "required": ["topic", "summary"]
    }
  });

  let options = ProcessOptions {
    prompt: None,
    images: vec![],
    output_format: OutputFormat::Text,
    image_detail: "auto".to_string(),
    json_schema: Some(schema),
    router: None,
  };

  let result = agent
    .process(
      "user123",
      UserInput::Text("Summarize AI news".to_string()),
      options,
    )
    .await?;

  if let ProcessResult::Structured(value) = result {
    println!("{}", value);
  }

  Ok(())
}
```

  ## Configuration in Code (Preferred)

  Most apps build config in code using environment variables. The file-based config is intended for the CLI.

  ```rust
  use std::env;

  use butterfly_bot::config::{AgentConfig, Config, OpenAiConfig};
  use butterfly_bot::client::ButterflyBot;

  #[tokio::main]
  async fn main() -> butterfly_bot::Result<()> {
    let config = Config {
      openai: Some(OpenAiConfig {
        api_key: env::var("OPENAI_API_KEY").ok(),
        model: Some("gpt-5.2".to_string()),
        base_url: None,
      }),
      agents: vec![AgentConfig {
        name: "default_agent".to_string(),
        instructions: "You are a helpful AI assistant.".to_string(),
        specialization: "general".to_string(),
        description: None,
        capture_name: None,
        capture_schema: None,
      }],
      business: None,
      memory: None,
      guardrails: None,
      tools: None,
    };

    let agent = ButterflyBot::from_config(config).await?;
    let mut stream = agent.process_text_stream("user123", "Hello!", None);
    while let Some(chunk) = stream.next().await {
      let text = chunk?;
      print!("{}", text);
    }
    Ok(())
  }
  ```

## Configuration Reference

Top-level schema:

```json
{
  "openai": {
    "api_key": "...",
    "model": "gpt-5.2",
    "base_url": "https://api.openai.com/v1"
  },
  "agents": [
    {
      "name": "...",
      "instructions": "...",
      "specialization": "...",
      "description": "...",
      "capture_name": "...",
      "capture_schema": {}
    }
  ],
  "business": {
    "mission": "...",
    "voice": "...",
    "values": [
      { "name": "...", "description": "..." }
    ],
    "goals": ["..."]
  },
  "memory": {
    "enabled": true,
    "sqlite_path": "./data/butterfly-bot.db",
    "lancedb_path": "./data/lancedb",
    "embedding_model": "ollama:qwen3-embedding",
    "rerank_model": "ollama:qwen3-reranker"
  },
  "guardrails": {
    "input": [
      { "class": "butterfly_bot.guardrails.pii.PII", "config": { "replacement": "[REDACTED]" } }
    ],
    "output": [
      { "class": "butterfly_bot.guardrails.pii.PII" }
    ]
  }
}
```

Notes:

- `openai.api_key` is optional when using an OpenAI-compatible `base_url` (e.g., Ollama).

- `openai.model` defaults to `gpt-5.2` if omitted.
- `agents` must contain at least one agent.
- `capture_*` fields are accepted and stored but not yet used by the Rust runtime.
- If `memory` is omitted, history is kept in-memory only (lost on restart).
- Use `butterfly-bot config export --path <file>` to export a redacted config.

## Local Memory (SQLite + LanceDB)

Local persistent memory uses embedded SQLite for transcripts and LanceDB for vectors. Add a `memory` block to your config:

```json
{
  "memory": {
    "enabled": true,
    "sqlite_path": "./data/butterfly-bot.db",
    "lancedb_path": "./data/lancedb"
  }
}
```

## Routing Behavior

The router selects an agent by matching query text against agent names and specialization keywords. If only one agent exists, it is always selected.

You can override routing per request by passing a custom router in `ProcessOptions.router` (implement the `RoutingService` trait).

## Memory Behavior

The default setup stores conversation history in memory only (process lifetime). You can clear history per user with `delete_user_history`.

## Memory Provider Interface

The Rust memory interface mirrors the Python provider shape for history and captures:

- `store(user_id, messages)`
- `retrieve(user_id)`
- `delete(user_id)`
- `find(collection, query, sort, limit, skip)`
- `count_documents(collection, query)`
- `save_capture(user_id, capture_name, agent_name, data, schema)`

With local memory enabled, messages are stored in SQLite and captures are stored in the local `captures` table.

## Guardrails

Guardrails can be configured in the Rust config using class names compatible with the Python naming convention. Currently supported:

- `butterfly_bot.guardrails.pii.PII` (basic email/phone scrubbing)

```json
{
  "guardrails": {
    "input": [
      { "class": "butterfly_bot.guardrails.pii.PII", "config": { "replacement": "[REDACTED]" } }
    ],
    "output": [
      { "class": "butterfly_bot.guardrails.pii.PII" }
    ]
  }
}
```

## Tooling

You can register tools by implementing the `Tool` trait and calling `register_tool` on `ButterflyBot`.

Tool calls are executed automatically when the model requests them. Tool results are fed back into the model until a final response is produced.

Tool safety is driven by config settings:

- `tools.settings.permissions.default_deny` (bool)
- `tools.settings.permissions.network_allow` (list of domains)
- `tools.settings.audit_log_path` (path, defaults to `./data/tool_audit.log`)

Tool-specific overrides can be set in `tools.<tool_name>.permissions.network_allow`.

Brain settings:

- `brains.settings.tick_seconds` (u64, default `60`)

## Roadmap

- Tool-calling with structured outputs
- Audio and image inputs
- Persistent memory providers

## License

MIT
