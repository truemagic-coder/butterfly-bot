# Solana Agent (Rust)

Solana Agent is a Rust-first framework for building multi-agent AI assistants with simple routing, optional memory, and a CLI. This repository contains the Rust rewrite only.

## Features

- Async Rust client and CLI
- JSON-based configuration
- Multi-agent routing by specialization
- In-memory conversation history
- OpenAI Responses API support

## Requirements

- Rust 1.93 or newer
- An OpenAI API key

## Installation

Use the crate as a library:

```bash
cargo add solana-agent
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
solana-agent --config config.json --user-id cli_user --prompt "Hello!"
```

Or start an interactive session:

```bash
solana-agent --config config.json --user-id cli_user
```

## Library Usage

Create a client and stream the response:

```rust
use futures::StreamExt;
use solana_agent::client::SolanaAgent;

#[tokio::main]
async fn main() -> solana_agent::Result<()> {
    let agent = SolanaAgent::from_config_path("config.json").await?;
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
use solana_agent::{
  ImageData, ImageInput, OutputFormat, ProcessOptions, ProcessResult, UserInput,
  SolanaAgent,
};

#[tokio::main]
async fn main() -> solana_agent::Result<()> {
  let agent = SolanaAgent::from_config_path("config.json").await?;

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
use solana_agent::{OutputFormat, ProcessOptions, ProcessResult, UserInput, SolanaAgent};

#[tokio::main]
async fn main() -> solana_agent::Result<()> {
  let agent = SolanaAgent::from_config_path("config.json").await?;

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
use solana_agent::{OutputFormat, ProcessOptions, ProcessResult, UserInput, SolanaAgent};

#[tokio::main]
async fn main() -> solana_agent::Result<()> {
  let agent = SolanaAgent::from_config_path("config.json").await?;

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

  use solana_agent::config::{AgentConfig, Config, OpenAiConfig};
  use solana_agent::client::SolanaAgent;

  #[tokio::main]
  async fn main() -> solana_agent::Result<()> {
    let config = Config {
      openai: Some(OpenAiConfig {
        api_key: env::var("OPENAI_API_KEY").unwrap_or_default(),
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
      mongo: None,
    };

    let agent = SolanaAgent::from_config(config).await?;
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
  "groq": {
    "api_key": "...",
    "model": "openai/gpt-oss-120b",
    "base_url": "https://api.groq.com/openai/v1"
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
  "mongo": {
    "connection_string": "mongodb://localhost:27017",
    "database": "solana_agent",
    "collection": "messages"
  },
  "guardrails": {
    "input": [
      { "class": "solana_agent.guardrails.pii.PII", "config": { "replacement": "[REDACTED]" } }
    ],
    "output": [
      { "class": "solana_agent.guardrails.pii.PII" }
    ]
  }
}
```

Notes:

- `openai.model` defaults to `gpt-5.2` if omitted.
- If `openai` is omitted, `groq` is used as an OpenAI-compatible provider.
- `agents` must contain at least one agent.
- `capture_*` fields are accepted and stored but not yet used by the Rust runtime.
- If `mongo` is omitted, history is kept in-memory only (lost on restart).

## MongoDB Memory (Recommended)

MongoDB provides persistent conversation history. Add a `mongo` block to your config:

```json
{
  "mongo": {
    "connection_string": "mongodb://localhost:27017",
    "database": "solana_agent",
    "collection": "messages"
  }
}
```

## Routing Behavior

The router selects an agent by matching query text against agent names and specialization keywords. If only one agent exists, it is always selected.

You can override routing per request by passing a custom router in `ProcessOptions.router` (implement the `RoutingService` trait).

## Memory Behavior

The default setup stores conversation history in memory only (process lifetime). You can clear history per user with `delete_user_history`.

## Memory Provider Interface

The Rust memory interface mirrors the Python provider shape for MongoDB-backed history and captures:

- `store(user_id, messages)`
- `retrieve(user_id)`
- `delete(user_id)`
- `find(collection, query, sort, limit, skip)`
- `count_documents(collection, query)`
- `save_capture(user_id, capture_name, agent_name, data, schema)`

When MongoDB is configured, messages are stored in the configured collection (default `messages`) and captures are stored in `captures`.

## Guardrails

Guardrails can be configured in the Rust config using class names compatible with the Python naming convention. Currently supported:

- `solana_agent.guardrails.pii.PII` (basic email/phone scrubbing)

```json
{
  "guardrails": {
    "input": [
      { "class": "solana_agent.guardrails.pii.PII", "config": { "replacement": "[REDACTED]" } }
    ],
    "output": [
      { "class": "solana_agent.guardrails.pii.PII" }
    ]
  }
}
```

## Tooling

You can register tools by implementing the `Tool` trait and calling `register_tool` on `SolanaAgent`.

Tool calls are executed automatically when the model requests them. Tool results are fed back into the model until a final response is produced.

## Roadmap

- Tool-calling with structured outputs
- Audio and image inputs
- Persistent memory providers

## License

MIT
