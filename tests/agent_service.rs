mod common;

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;
use tokio::sync::Mutex as AsyncMutex;

use butterfly_bot::brain::manager::BrainManager;
use butterfly_bot::domains::agent::AIAgent;
use butterfly_bot::error::Result;
use butterfly_bot::interfaces::brain::{BrainContext, BrainEvent, BrainPlugin};
use butterfly_bot::interfaces::plugins::Tool;
use butterfly_bot::interfaces::providers::{ImageData, ImageInput, LlmResponse, ToolCall};
use butterfly_bot::services::agent::AgentService;

use common::{DummyTool, QueueLlmProvider};
use std::sync::Mutex;

struct RecordingTool {
    name: String,
    calls: Arc<AsyncMutex<Vec<serde_json::Value>>>,
}

struct FixedResultTool {
    name: String,
    result: serde_json::Value,
}

struct RecordingFixedResultTool {
    name: String,
    result: serde_json::Value,
    calls: Arc<AsyncMutex<Vec<serde_json::Value>>>,
}

struct FailingHttpCallTool {
    result: serde_json::Value,
    calls: Arc<AsyncMutex<Vec<serde_json::Value>>>,
}

impl FailingHttpCallTool {
    fn new(result: serde_json::Value) -> Self {
        Self {
            result,
            calls: Arc::new(AsyncMutex::new(Vec::new())),
        }
    }
}

impl FixedResultTool {
    fn new(name: &str, result: serde_json::Value) -> Self {
        Self {
            name: name.to_string(),
            result,
        }
    }
}

impl RecordingFixedResultTool {
    fn new(name: &str, result: serde_json::Value) -> Self {
        Self {
            name: name.to_string(),
            result,
            calls: Arc::new(AsyncMutex::new(Vec::new())),
        }
    }
}

impl RecordingTool {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
            calls: Arc::new(AsyncMutex::new(Vec::new())),
        }
    }
}

#[async_trait]
impl Tool for RecordingTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        "recording"
    }

    fn parameters(&self) -> serde_json::Value {
        json!({"type":"object","properties":{}})
    }

    async fn execute(&self, params: serde_json::Value) -> Result<serde_json::Value> {
        self.calls.lock().await.push(params);
        Ok(json!({"status":"ok"}))
    }
}

#[async_trait]
impl Tool for FixedResultTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        "fixed-result"
    }

    fn parameters(&self) -> serde_json::Value {
        json!({"type":"object","properties":{}})
    }

    async fn execute(&self, _params: serde_json::Value) -> Result<serde_json::Value> {
        Ok(self.result.clone())
    }
}

#[async_trait]
impl Tool for RecordingFixedResultTool {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        "recording-fixed-result"
    }

    fn parameters(&self) -> serde_json::Value {
        json!({"type":"object","properties":{}})
    }

    async fn execute(&self, params: serde_json::Value) -> Result<serde_json::Value> {
        self.calls.lock().await.push(params);
        Ok(self.result.clone())
    }
}

#[async_trait]
impl Tool for FailingHttpCallTool {
    fn name(&self) -> &str {
        "http_call"
    }

    fn description(&self) -> &str {
        "failing-http-call"
    }

    fn parameters(&self) -> serde_json::Value {
        json!({"type":"object","properties":{}})
    }

    async fn execute(&self, params: serde_json::Value) -> Result<serde_json::Value> {
        self.calls.lock().await.push(params.clone());
        let url = params
            .get("url")
            .and_then(|v| v.as_str())
            .unwrap_or_default();
        if url.starts_with("solana:") {
            return Err(butterfly_bot::error::ButterflyBotError::Runtime(
                "http error: builder error for url (solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp)"
                    .to_string(),
            ));
        }
        Ok(self.result.clone())
    }
}

#[tokio::test]
async fn routing_and_agent_service() {
    let brain_manager = Arc::new(BrainManager::new(json!({})));
    let llm = Arc::new(QueueLlmProvider::new(vec![
        LlmResponse {
            text: "tool response".to_string(),
            tool_calls: vec![
                ToolCall {
                    name: "tool1".to_string(),
                    arguments: json!({"value": 1}),
                },
                ToolCall {
                    name: "missing".to_string(),
                    arguments: json!({}),
                },
            ],
        },
        LlmResponse {
            text: "done".to_string(),
            tool_calls: Vec::new(),
        },
    ]));

    let agent = AIAgent {
        name: "agent1".to_string(),
        instructions: "inst".to_string(),
        specialization: "spec".to_string(),
    };

    let service = AgentService::new(llm, agent, None, None, None, None, brain_manager, None);

    let system = service.get_agent_system_prompt().await.unwrap();
    assert!(system.contains("inst"));

    let registry = service.tool_registry.clone();
    let tool = Arc::new(DummyTool::new("tool1"));
    assert!(registry.register_tool(tool).await);
    assert!(
        registry
            .assign_tool_to_agent(service.agent_name(), "tool1")
            .await
    );

    let response = service
        .generate_response("u1", "query", "history", Some("prompt"))
        .await
        .unwrap();
    assert_eq!(response, "done");

    let response = service
        .generate_response_with_images(
            "u1",
            "query",
            vec![ImageInput {
                data: ImageData::Url("http://example.com".to_string()),
            }],
            "",
            None,
            "auto",
        )
        .await
        .unwrap();
    assert_eq!(response, "image response");

    let response = service
        .generate_response_with_images(
            "u1",
            "query",
            vec![ImageInput {
                data: ImageData::Url("http://example.com".to_string()),
            }],
            "",
            Some("extra"),
            "auto",
        )
        .await
        .unwrap();
    assert_eq!(response, "image response");

    let structured = service
        .generate_structured_response("u1", "query", "", None, json!({"type":"object"}))
        .await
        .unwrap();
    assert_eq!(structured, json!({"ok": true}));

    let transcript = service
        .transcribe_audio(vec![1, 2, 3], "wav")
        .await
        .unwrap();
    assert_eq!(transcript, "transcribed");

    let audio = service
        .synthesize_audio("hi", "alloy", "mp3")
        .await
        .unwrap();
    assert_eq!(audio, b"audio".to_vec());

    let mut responses = Vec::new();
    for idx in 0..5 {
        responses.push(LlmResponse {
            text: format!("step {idx}"),
            tool_calls: vec![ToolCall {
                name: "tool1".to_string(),
                arguments: json!({"value": idx}),
            }],
        });
    }

    let looping_llm = Arc::new(QueueLlmProvider::new(responses));
    let looping_brain = Arc::new(BrainManager::new(json!({})));
    let looping_agent = AIAgent {
        name: "agent-loop".to_string(),
        instructions: "inst".to_string(),
        specialization: "spec".to_string(),
    };
    let looping_service = AgentService::new(
        looping_llm,
        looping_agent,
        None,
        None,
        None,
        None,
        looping_brain,
        None,
    );
    let registry = looping_service.tool_registry.clone();
    let tool = Arc::new(DummyTool::new("tool1"));
    assert!(registry.register_tool(tool).await);
    assert!(
        registry
            .assign_tool_to_agent(looping_service.agent_name(), "tool1")
            .await
    );

    let response = looping_service
        .generate_response("u1", "query", "", None)
        .await
        .unwrap();
    assert_eq!(response, "mock text");
}

struct RecordingBrain {
    name: String,
    events: Arc<Mutex<Vec<String>>>,
}

#[async_trait::async_trait]
impl BrainPlugin for RecordingBrain {
    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        "recording"
    }

    async fn on_event(&self, event: BrainEvent, _ctx: &BrainContext) -> butterfly_bot::Result<()> {
        let label = match event {
            BrainEvent::Start => "start",
            BrainEvent::Tick => "tick",
            BrainEvent::UserMessage { .. } => "user",
            BrainEvent::AssistantResponse { .. } => "assistant",
        };
        let mut guard = self.events.lock().unwrap();
        guard.push(label.to_string());
        Ok(())
    }
}

#[tokio::test]
async fn agent_service_dispatches_brain_events() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut brain = BrainManager::new(json!({"brains": ["record"]}));
    let events_factory = events.clone();
    brain.register_factory("record", move |_| {
        Arc::new(RecordingBrain {
            name: "record".to_string(),
            events: events_factory.clone(),
        })
    });
    brain.load_plugins();
    let brain = Arc::new(brain);

    let llm = Arc::new(QueueLlmProvider::new(vec![]));
    let agent = AIAgent {
        name: "agent".to_string(),
        instructions: "inst".to_string(),
        specialization: "spec".to_string(),
    };
    let service = AgentService::new(llm, agent, None, None, None, None, brain, None);

    let response = service
        .generate_response("u1", "hello", "", None)
        .await
        .unwrap();
    assert_eq!(response, "mock text");

    let guard = events.lock().unwrap();
    assert_eq!(guard.as_slice(), ["start", "user", "assistant"]);
}

#[tokio::test]
async fn agent_service_brain_tick_dispatches() {
    let events = Arc::new(Mutex::new(Vec::new()));
    let mut brain = BrainManager::new(json!({"brains": ["record"]}));
    let events_factory = events.clone();
    brain.register_factory("record", move |_| {
        Arc::new(RecordingBrain {
            name: "record".to_string(),
            events: events_factory.clone(),
        })
    });
    brain.load_plugins();
    let brain = Arc::new(brain);

    let llm = Arc::new(QueueLlmProvider::new(vec![]));
    let agent = AIAgent {
        name: "agent".to_string(),
        instructions: "inst".to_string(),
        specialization: "spec".to_string(),
    };
    let service = AgentService::new(llm, agent, None, None, None, None, brain, None);

    service.dispatch_brain_tick().await;

    let guard = events.lock().unwrap();
    assert_eq!(guard.as_slice(), ["tick"]);
}

#[tokio::test]
async fn x402_flow_normalizes_malformed_solana_tool_name() {
    let brain_manager = Arc::new(BrainManager::new(json!({})));
    let llm = Arc::new(QueueLlmProvider::new(vec![
        LlmResponse {
            text: "".to_string(),
            tool_calls: vec![ToolCall {
                name: "solana」\n{\n\"action\":\"simulate_transfer\"\n}\n[TOOL_CALLS]solana"
                    .to_string(),
                arguments: json!({
                    "action": "simulate_transfer",
                    "to": "H32YnqbzL62YkHMSCzfKcLry9yuipwwx1EMztiCSPhjb",
                    "amount": "10000",
                    "asset": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
                }),
            }],
        },
        LlmResponse {
            text: "done".to_string(),
            tool_calls: Vec::new(),
        },
    ]));

    let agent = AIAgent {
        name: "agent-x402".to_string(),
        instructions: "inst".to_string(),
        specialization: "spec".to_string(),
    };
    let service = AgentService::new(llm, agent, None, None, None, None, brain_manager, None);

    let solana = Arc::new(RecordingTool::new("solana"));
    let registry = service.tool_registry.clone();
    assert!(registry.register_tool(solana.clone()).await);
    assert!(
        registry
            .assign_tool_to_agent(service.agent_name(), "solana")
            .await
    );

    let response = service
        .generate_response(
            "u1",
            "Pay x402 - https://x402.payai.network/api/solana/paid-content",
            "",
            None,
        )
        .await
        .unwrap();
    assert_eq!(response, "done");

    let calls = solana.calls.lock().await;
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0].get("action").and_then(|v| v.as_str()),
        Some("simulate_transfer")
    );
}

#[tokio::test]
async fn reminders_tool_still_available_after_x402_turn() {
    let brain_manager = Arc::new(BrainManager::new(json!({})));
    let llm = Arc::new(QueueLlmProvider::new(vec![
        LlmResponse {
            text: "".to_string(),
            tool_calls: vec![ToolCall {
                name: "solana".to_string(),
                arguments: json!({"action": "wallet"}),
            }],
        },
        LlmResponse {
            text: "first done".to_string(),
            tool_calls: Vec::new(),
        },
        LlmResponse {
            text: "".to_string(),
            tool_calls: vec![ToolCall {
                name: "reminders".to_string(),
                arguments: json!({
                    "action": "create",
                    "title": "feed cats",
                    "in_seconds": 30
                }),
            }],
        },
        LlmResponse {
            text: "second done".to_string(),
            tool_calls: Vec::new(),
        },
    ]));

    let agent = AIAgent {
        name: "agent-tools".to_string(),
        instructions: "inst".to_string(),
        specialization: "spec".to_string(),
    };
    let service = AgentService::new(llm, agent, None, None, None, None, brain_manager, None);

    let solana = Arc::new(RecordingTool::new("solana"));
    let http_call = Arc::new(RecordingTool::new("http_call"));
    let reminders = Arc::new(RecordingTool::new("reminders"));
    let registry = service.tool_registry.clone();
    assert!(registry.register_tool(solana.clone()).await);
    assert!(registry.register_tool(http_call.clone()).await);
    assert!(registry.register_tool(reminders.clone()).await);
    assert!(
        registry
            .assign_tool_to_agent(service.agent_name(), "solana")
            .await
    );
    assert!(
        registry
            .assign_tool_to_agent(service.agent_name(), "http_call")
            .await
    );
    assert!(
        registry
            .assign_tool_to_agent(service.agent_name(), "reminders")
            .await
    );

    let response = service
        .generate_response(
            "u1",
            "Pay x402 - https://x402.payai.network/api/solana/paid-content",
            "",
            None,
        )
        .await
        .unwrap();
    assert_eq!(response, "first done");

    let response = service
        .generate_response("u1", "set a reminder in 30 seconds to feed cats", "", None)
        .await
        .unwrap();
    assert_eq!(response, "second done");

    let reminder_calls = reminders.calls.lock().await;
    assert_eq!(reminder_calls.len(), 1);
    assert_eq!(
        reminder_calls[0].get("action").and_then(|v| v.as_str()),
        Some("create")
    );
}

#[tokio::test]
async fn todo_request_forces_tool_grounding_after_deferred_listing_text() {
    let brain_manager = Arc::new(BrainManager::new(json!({})));
    let llm = Arc::new(QueueLlmProvider::new(vec![
        LlmResponse {
            text: "Listing existing todos to avoid duplicates, then creating a new one for your requested pet care list.".to_string(),
            tool_calls: Vec::new(),
        },
        LlmResponse {
            text: "".to_string(),
            tool_calls: vec![
                ToolCall {
                    name: "todo".to_string(),
                    arguments: json!({"action": "list"}),
                },
                ToolCall {
                    name: "todo".to_string(),
                    arguments: json!({
                        "action": "create_many",
                        "items": ["feed cats", "feed dogs", "feed fish"]
                    }),
                },
            ],
        },
        LlmResponse {
            text: "done".to_string(),
            tool_calls: Vec::new(),
        },
    ]));

    let agent = AIAgent {
        name: "agent-todo-grounding".to_string(),
        instructions: "inst".to_string(),
        specialization: "spec".to_string(),
    };
    let service = AgentService::new(llm, agent, None, None, None, None, brain_manager, None);

    let todo = Arc::new(RecordingTool::new("todo"));
    let registry = service.tool_registry.clone();
    assert!(registry.register_tool(todo.clone()).await);
    assert!(
        registry
            .assign_tool_to_agent(service.agent_name(), "todo")
            .await
    );

    let response = service
        .generate_response(
            "u1",
            "create a todo list - 1) feed cats 2) feed dogs 3) feed fish",
            "",
            None,
        )
        .await
        .unwrap();
    assert_eq!(response, "done");

    let calls = todo.calls.lock().await;
    assert_eq!(calls.len(), 2);
    assert_eq!(
        calls[0].get("action").and_then(|v| v.as_str()),
        Some("list")
    );
    assert_eq!(
        calls[1].get("action").and_then(|v| v.as_str()),
        Some("create_many")
    );
}

#[tokio::test]
async fn tasks_request_forces_tool_grounding_after_deferred_listing_text() {
    let brain_manager = Arc::new(BrainManager::new(json!({})));
    let llm = Arc::new(QueueLlmProvider::new(vec![
        LlmResponse {
            text: "Listing existing tasks to avoid duplicates, then scheduling a new one."
                .to_string(),
            tool_calls: Vec::new(),
        },
        LlmResponse {
            text: "".to_string(),
            tool_calls: vec![
                ToolCall {
                    name: "tasks".to_string(),
                    arguments: json!({"action": "list"}),
                },
                ToolCall {
                    name: "tasks".to_string(),
                    arguments: json!({
                        "action": "schedule",
                        "name": "feed cats",
                        "prompt": "feed cats",
                        "run_at": 1_800_000_000
                    }),
                },
            ],
        },
        LlmResponse {
            text: "done".to_string(),
            tool_calls: Vec::new(),
        },
    ]));

    let agent = AIAgent {
        name: "agent-tasks-grounding".to_string(),
        instructions: "inst".to_string(),
        specialization: "spec".to_string(),
    };
    let service = AgentService::new(llm, agent, None, None, None, None, brain_manager, None);

    let tasks = Arc::new(RecordingTool::new("tasks"));
    let registry = service.tool_registry.clone();
    assert!(registry.register_tool(tasks.clone()).await);
    assert!(
        registry
            .assign_tool_to_agent(service.agent_name(), "tasks")
            .await
    );

    let response = service
        .generate_response("u1", "create a task to feed the cats", "", None)
        .await
        .unwrap();
    assert_eq!(response, "done");

    let calls = tasks.calls.lock().await;
    assert_eq!(calls.len(), 2);
    assert_eq!(
        calls[0].get("action").and_then(|v| v.as_str()),
        Some("list")
    );
    assert_eq!(
        calls[1].get("action").and_then(|v| v.as_str()),
        Some("schedule")
    );
}

#[tokio::test]
async fn todo_request_rejects_promise_style_preaction_text() {
    let brain_manager = Arc::new(BrainManager::new(json!({})));
    let llm = Arc::new(QueueLlmProvider::new(vec![
        LlmResponse {
            text: "Here’s your clean todo list for feeding cats. I'll create it now.".to_string(),
            tool_calls: Vec::new(),
        },
        LlmResponse {
            text: "".to_string(),
            tool_calls: vec![
                ToolCall {
                    name: "todo".to_string(),
                    arguments: json!({"action": "list"}),
                },
                ToolCall {
                    name: "todo".to_string(),
                    arguments: json!({
                        "action": "create_many",
                        "items": ["feed cats"]
                    }),
                },
            ],
        },
        LlmResponse {
            text: "done".to_string(),
            tool_calls: Vec::new(),
        },
    ]));

    let agent = AIAgent {
        name: "agent-todo-preaction-promise".to_string(),
        instructions: "inst".to_string(),
        specialization: "spec".to_string(),
    };
    let service = AgentService::new(llm, agent, None, None, None, None, brain_manager, None);

    let todo = Arc::new(RecordingTool::new("todo"));
    let registry = service.tool_registry.clone();
    assert!(registry.register_tool(todo.clone()).await);
    assert!(
        registry
            .assign_tool_to_agent(service.agent_name(), "todo")
            .await
    );

    let response = service
        .generate_response("u1", "create a todo list about feeding cats", "", None)
        .await
        .unwrap();
    assert_eq!(response, "done");

    let calls = todo.calls.lock().await;
    assert_eq!(calls.len(), 2);
    assert_eq!(
        calls[0].get("action").and_then(|v| v.as_str()),
        Some("list")
    );
    assert_eq!(
        calls[1].get("action").and_then(|v| v.as_str()),
        Some("create_many")
    );
}

#[tokio::test]
async fn solana_balance_request_forces_tool_grounding() {
    let brain_manager = Arc::new(BrainManager::new(json!({})));
    let llm = Arc::new(QueueLlmProvider::new(vec![
        LlmResponse {
            text: "Your Solana balance is 0.048989 SOL (~48,989 lamports).".to_string(),
            tool_calls: Vec::new(),
        },
        LlmResponse {
            text: "".to_string(),
            tool_calls: vec![ToolCall {
                name: "solana".to_string(),
                arguments: json!({"action": "balance"}),
            }],
        },
        LlmResponse {
            text: "done".to_string(),
            tool_calls: Vec::new(),
        },
    ]));

    let agent = AIAgent {
        name: "agent-solana-balance".to_string(),
        instructions: "inst".to_string(),
        specialization: "spec".to_string(),
    };
    let service = AgentService::new(llm, agent, None, None, None, None, brain_manager, None);

    let solana = Arc::new(RecordingTool::new("solana"));
    let registry = service.tool_registry.clone();
    assert!(registry.register_tool(solana.clone()).await);
    assert!(
        registry
            .assign_tool_to_agent(service.agent_name(), "solana")
            .await
    );

    let response = service
        .generate_response("u1", "what is my solana balance?", "", None)
        .await
        .unwrap();
    assert_eq!(response, "done");

    let calls = solana.calls.lock().await;
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0].get("action").and_then(|v| v.as_str()),
        Some("balance")
    );
}

#[tokio::test]
async fn solana_underscore_alias_tool_name_maps_to_balance_action() {
    let brain_manager = Arc::new(BrainManager::new(json!({})));
    let llm = Arc::new(QueueLlmProvider::new(vec![
        LlmResponse {
            text: "".to_string(),
            tool_calls: vec![ToolCall {
                name: "solana_get_balance".to_string(),
                arguments: json!({}),
            }],
        },
        LlmResponse {
            text: "done".to_string(),
            tool_calls: Vec::new(),
        },
    ]));

    let agent = AIAgent {
        name: "agent-solana-underscore-alias".to_string(),
        instructions: "inst".to_string(),
        specialization: "spec".to_string(),
    };
    let service = AgentService::new(llm, agent, None, None, None, None, brain_manager, None);

    let solana = Arc::new(RecordingTool::new("solana"));
    let registry = service.tool_registry.clone();
    assert!(registry.register_tool(solana.clone()).await);
    assert!(
        registry
            .assign_tool_to_agent(service.agent_name(), "solana")
            .await
    );

    let response = service
        .generate_response("u1", "check my wallet balance", "", None)
        .await
        .unwrap();
    assert_eq!(response, "done");

    let calls = solana.calls.lock().await;
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0].get("action").and_then(|v| v.as_str()),
        Some("balance")
    );
}

#[tokio::test]
async fn solana_balance_reply_uses_tool_result_values() {
    let brain_manager = Arc::new(BrainManager::new(json!({})));
    let llm = Arc::new(QueueLlmProvider::new(vec![
        LlmResponse {
            text: "".to_string(),
            tool_calls: vec![ToolCall {
                name: "solana".to_string(),
                arguments: json!({"action": "balance"}),
            }],
        },
        LlmResponse {
            text: "Your balance is 999 SOL".to_string(),
            tool_calls: Vec::new(),
        },
    ]));

    let agent = AIAgent {
        name: "agent-solana-grounded-reply".to_string(),
        instructions: "inst".to_string(),
        specialization: "spec".to_string(),
    };
    let service = AgentService::new(llm, agent, None, None, None, None, brain_manager, None);

    let solana = Arc::new(FixedResultTool::new(
        "solana",
        json!({
            "status": "ok",
            "address": "H32YnqbzL62YkHMSCzfKcLry9yuipwwx1EMztiCSPhjb",
            "lamports": 48989,
            "sol": 0.000048989
        }),
    ));
    let registry = service.tool_registry.clone();
    assert!(registry.register_tool(solana).await);
    assert!(
        registry
            .assign_tool_to_agent(service.agent_name(), "solana")
            .await
    );

    let response = service
        .generate_response("u1", "what is my solana balance?", "", None)
        .await
        .unwrap();
    assert_eq!(
        response,
        "Your Solana balance is 0.000048989 SOL (48989 lamports)."
    );
}

#[tokio::test]
async fn x402_flow_overrides_url_destination_with_canonical_payto() {
    let brain_manager = Arc::new(BrainManager::new(json!({})));
    let llm = Arc::new(QueueLlmProvider::new(vec![
        LlmResponse {
            text: "".to_string(),
            tool_calls: vec![ToolCall {
                name: "http_call".to_string(),
                arguments: json!({
                    "method": "GET",
                    "url": "https://x402.payai.network/api/solana/paid-content"
                }),
            }],
        },
        LlmResponse {
            text: "".to_string(),
            tool_calls: vec![ToolCall {
                name: "solana".to_string(),
                arguments: json!({
                    "action": "simulate_transfer",
                    "to": "https://x402.payai.network/api/solana/paid-content"
                }),
            }],
        },
        LlmResponse {
            text: "done".to_string(),
            tool_calls: Vec::new(),
        },
    ]));

    let agent = AIAgent {
        name: "agent-x402-deterministic".to_string(),
        instructions: "inst".to_string(),
        specialization: "spec".to_string(),
    };
    let service = AgentService::new(llm, agent, None, None, None, None, brain_manager, None);

    let http_call = Arc::new(FixedResultTool::new(
        "http_call",
        json!({
            "status": "ok",
            "http_status": 402,
            "json": {
                "x402Version": 2,
                "resource": {
                    "description": "paid",
                    "mimeType": "application/json",
                    "url": "https://x402.payai.network/api/solana/paid-content"
                },
                "accepts": [{
                    "scheme": "exact",
                    "network": "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp",
                    "amount": "10000",
                    "payTo": "H32YnqbzL62YkHMSCzfKcLry9yuipwwx1EMztiCSPhjb",
                    "maxTimeoutSeconds": 300,
                    "asset": "SOL"
                }]
            }
        }),
    ));
    let solana = Arc::new(RecordingTool::new("solana"));
    let registry = service.tool_registry.clone();
    assert!(registry.register_tool(http_call).await);
    assert!(registry.register_tool(solana.clone()).await);
    assert!(
        registry
            .assign_tool_to_agent(service.agent_name(), "http_call")
            .await
    );
    assert!(
        registry
            .assign_tool_to_agent(service.agent_name(), "solana")
            .await
    );

    let response = service
        .generate_response(
            "u1",
            "Pay with x402 - https://x402.payai.network/api/solana/paid-content",
            "",
            None,
        )
        .await
        .unwrap();
    assert_eq!(response, "done");

    let calls = solana.calls.lock().await;
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0].get("to").and_then(|v| v.as_str()),
        Some("H32YnqbzL62YkHMSCzfKcLry9yuipwwx1EMztiCSPhjb")
    );
    assert_eq!(
        calls[0].get("lamports").and_then(|v| v.as_u64()),
        Some(10000)
    );
}

#[tokio::test]
async fn x402_request_requires_http_call_grounding_without_solana_tool() {
    let brain_manager = Arc::new(BrainManager::new(json!({})));
    let llm = Arc::new(QueueLlmProvider::new(vec![
        LlmResponse {
            text: "Let me check your current Solana balance to confirm you can cover this payment. Executing: Check wallet balance".to_string(),
            tool_calls: Vec::new(),
        },
        LlmResponse {
            text: "".to_string(),
            tool_calls: vec![ToolCall {
                name: "http_call".to_string(),
                arguments: json!({
                    "method": "GET",
                    "url": "https://x402.payai.network/api/solana/paid-content"
                }),
            }],
        },
        LlmResponse {
            text: "done".to_string(),
            tool_calls: Vec::new(),
        },
    ]));

    let agent = AIAgent {
        name: "agent-x402-http-grounding".to_string(),
        instructions: "inst".to_string(),
        specialization: "spec".to_string(),
    };
    let service = AgentService::new(llm, agent, None, None, None, None, brain_manager, None);

    let http_call = Arc::new(RecordingTool::new("http_call"));
    let registry = service.tool_registry.clone();
    assert!(registry.register_tool(http_call.clone()).await);
    assert!(
        registry
            .assign_tool_to_agent(service.agent_name(), "http_call")
            .await
    );

    let response = service
        .generate_response(
            "u1",
            "Pay with x402 - https://x402.payai.network/api/solana/paid-content",
            "",
            None,
        )
        .await
        .unwrap();
    assert_eq!(response, "done");

    let calls = http_call.calls.lock().await;
    assert!(!calls.is_empty());
    assert_eq!(calls[0].get("method").and_then(|v| v.as_str()), Some("GET"));
}

#[tokio::test]
async fn x402_rejects_staged_countdown_narration() {
    let brain_manager = Arc::new(BrainManager::new(json!({})));
    let llm = Arc::new(QueueLlmProvider::new(vec![
        LlmResponse {
            text: "Solana Payment Flow for x402 (1/2). To proceed, I will:\n- Check your balance\nConfirm if you'd like me to proceed.\n(Will execute solana tool in 3... 2... 1...)\nAwaiting balance/tool-response.\n— Tools activated: —".to_string(),
            tool_calls: Vec::new(),
        },
        LlmResponse {
            text: "".to_string(),
            tool_calls: vec![ToolCall {
                name: "http_call".to_string(),
                arguments: json!({
                    "method": "GET",
                    "url": "https://x402.payai.network/api/solana/paid-content"
                }),
            }],
        },
        LlmResponse {
            text: "done".to_string(),
            tool_calls: Vec::new(),
        },
    ]));

    let agent = AIAgent {
        name: "agent-x402-staged-guard".to_string(),
        instructions: "inst".to_string(),
        specialization: "spec".to_string(),
    };
    let service = AgentService::new(llm, agent, None, None, None, None, brain_manager, None);

    let http_call = Arc::new(RecordingTool::new("http_call"));
    let registry = service.tool_registry.clone();
    assert!(registry.register_tool(http_call.clone()).await);
    assert!(
        registry
            .assign_tool_to_agent(service.agent_name(), "http_call")
            .await
    );

    let response = service
        .generate_response(
            "u1",
            "Pay with x402 - https://x402.payai.network/api/solana/paid-content",
            "",
            None,
        )
        .await
        .unwrap();
    assert_eq!(response, "done");

    let calls = http_call.calls.lock().await;
    assert!(!calls.is_empty());
}

#[tokio::test]
async fn x402_non_sol_asset_maps_into_token_transfer_args() {
    let brain_manager = Arc::new(BrainManager::new(json!({})));
    let llm = Arc::new(QueueLlmProvider::new(vec![
        LlmResponse {
            text: "".to_string(),
            tool_calls: vec![ToolCall {
                name: "http_call".to_string(),
                arguments: json!({
                    "method": "GET",
                    "url": "https://x402.payai.network/api/solana/paid-content"
                }),
            }],
        },
        LlmResponse {
            text: "".to_string(),
            tool_calls: vec![ToolCall {
                name: "solana".to_string(),
                arguments: json!({
                    "action": "simulate_transfer",
                    "to": "H32YnqbzL62YkHMSCzfKcLry9yuipwwx1EMztiCSPhjb"
                }),
            }],
        },
        LlmResponse {
            text: "done".to_string(),
            tool_calls: Vec::new(),
        },
    ]));

    let agent = AIAgent {
        name: "agent-x402-non-sol-guard".to_string(),
        instructions: "inst".to_string(),
        specialization: "spec".to_string(),
    };
    let service = AgentService::new(llm, agent, None, None, None, None, brain_manager, None);

    let http_call = Arc::new(FixedResultTool::new(
        "http_call",
        json!({
            "status": "ok",
            "http_status": 402,
            "json": {
                "x402Version": 2,
                "resource": {
                    "description": "paid",
                    "mimeType": "application/json",
                    "url": "https://x402.payai.network/api/solana/paid-content"
                },
                "accepts": [{
                    "scheme": "exact",
                    "network": "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp",
                    "amount": "10000",
                    "payTo": "H32YnqbzL62YkHMSCzfKcLry9yuipwwx1EMztiCSPhjb",
                    "maxTimeoutSeconds": 300,
                    "asset": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
                }]
            }
        }),
    ));
    let solana = Arc::new(RecordingTool::new("solana"));
    let registry = service.tool_registry.clone();
    assert!(registry.register_tool(http_call).await);
    assert!(registry.register_tool(solana.clone()).await);
    assert!(
        registry
            .assign_tool_to_agent(service.agent_name(), "http_call")
            .await
    );
    assert!(
        registry
            .assign_tool_to_agent(service.agent_name(), "solana")
            .await
    );

    let response = service
        .generate_response(
            "u1",
            "Pay with x402 - https://x402.payai.network/api/solana/paid-content",
            "",
            None,
        )
        .await
        .unwrap();
    assert_eq!(response, "done");

    let calls = solana.calls.lock().await;
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0].get("mint").and_then(|v| v.as_str()),
        Some("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v")
    );
    assert_eq!(
        calls[0].get("amount_atomic").and_then(|v| v.as_u64()),
        Some(10000)
    );
    assert_eq!(calls[0].get("lamports"), None);
}

#[tokio::test]
async fn solana_nested_parameters_and_inspect_balance_alias_are_normalized() {
    let brain_manager = Arc::new(BrainManager::new(json!({})));
    let llm = Arc::new(QueueLlmProvider::new(vec![
        LlmResponse {
            text: "".to_string(),
            tool_calls: vec![ToolCall {
                name: "solana".to_string(),
                arguments: json!({
                    "action": "inspect_balance",
                    "parameters": {
                        "wallet_address": "CvkK9CeYhhh1Vtkw6WZQkS8wGmmZsmZMcaXssD8pKZts",
                        "mint": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
                    }
                }),
            }],
        },
        LlmResponse {
            text: "done".to_string(),
            tool_calls: Vec::new(),
        },
    ]));

    let agent = AIAgent {
        name: "agent-solana-normalize-nested".to_string(),
        instructions: "inst".to_string(),
        specialization: "spec".to_string(),
    };
    let service = AgentService::new(llm, agent, None, None, None, None, brain_manager, None);

    let solana = Arc::new(RecordingTool::new("solana"));
    let registry = service.tool_registry.clone();
    assert!(registry.register_tool(solana.clone()).await);
    assert!(
        registry
            .assign_tool_to_agent(service.agent_name(), "solana")
            .await
    );

    let response = service
        .generate_response(
            "u1",
            "Pay with x402 - https://x402.payai.network/api/solana/paid-content",
            "",
            None,
        )
        .await
        .unwrap();
    assert_eq!(response, "done");

    let calls = solana.calls.lock().await;
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0].get("action").and_then(|v| v.as_str()),
        Some("balance")
    );
    assert_eq!(
        calls[0].get("address").and_then(|v| v.as_str()),
        Some("CvkK9CeYhhh1Vtkw6WZQkS8wGmmZsmZMcaXssD8pKZts")
    );
}

#[tokio::test]
async fn x402_prefetch_fills_missing_transfer_destination() {
    let brain_manager = Arc::new(BrainManager::new(json!({})));
    let llm = Arc::new(QueueLlmProvider::new(vec![
        LlmResponse {
            text: "".to_string(),
            tool_calls: vec![ToolCall {
                name: "solana".to_string(),
                arguments: json!({
                    "action": "simulate_transfer"
                }),
            }],
        },
        LlmResponse {
            text: "done".to_string(),
            tool_calls: Vec::new(),
        },
    ]));

    let agent = AIAgent {
        name: "agent-x402-prefetch".to_string(),
        instructions: "inst".to_string(),
        specialization: "spec".to_string(),
    };
    let service = AgentService::new(llm, agent, None, None, None, None, brain_manager, None);

    let http_call = Arc::new(RecordingFixedResultTool::new(
        "http_call",
        json!({
            "status": "ok",
            "http_status": 402,
            "json": {
                "x402Version": 2,
                "resource": {
                    "description": "paid",
                    "mimeType": "application/json",
                    "url": "https://x402.payai.network/api/solana/paid-content"
                },
                "accepts": [{
                    "scheme": "exact",
                    "network": "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp",
                    "amount": "10000",
                    "payTo": "H32YnqbzL62YkHMSCzfKcLry9yuipwwx1EMztiCSPhjb",
                    "maxTimeoutSeconds": 300,
                    "asset": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
                }]
            }
        }),
    ));
    let solana = Arc::new(RecordingTool::new("solana"));
    let registry = service.tool_registry.clone();
    assert!(registry.register_tool(http_call.clone()).await);
    assert!(registry.register_tool(solana.clone()).await);
    assert!(
        registry
            .assign_tool_to_agent(service.agent_name(), "http_call")
            .await
    );
    assert!(
        registry
            .assign_tool_to_agent(service.agent_name(), "solana")
            .await
    );

    let response = service
        .generate_response(
            "u1",
            "Pay with x402 - https://x402.payai.network/api/solana/paid-content",
            "",
            None,
        )
        .await
        .unwrap();
    assert_eq!(response, "done");

    let http_calls = http_call.calls.lock().await;
    assert_eq!(http_calls.len(), 1);
    assert_eq!(
        http_calls[0].get("method").and_then(|v| v.as_str()),
        Some("GET")
    );

    let sol_calls = solana.calls.lock().await;
    assert_eq!(sol_calls.len(), 1);
    assert_eq!(
        sol_calls[0].get("to").and_then(|v| v.as_str()),
        Some("H32YnqbzL62YkHMSCzfKcLry9yuipwwx1EMztiCSPhjb")
    );
    assert_eq!(
        sol_calls[0].get("mint").and_then(|v| v.as_str()),
        Some("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v")
    );
    assert_eq!(
        sol_calls[0].get("amount_atomic").and_then(|v| v.as_u64()),
        Some(10000)
    );
}

#[tokio::test]
async fn x402_submission_message_is_grounded_to_canonical_asset() {
    let brain_manager = Arc::new(BrainManager::new(json!({})));
    let llm = Arc::new(QueueLlmProvider::new(vec![
        LlmResponse {
            text: "".to_string(),
            tool_calls: vec![ToolCall {
                name: "solana".to_string(),
                arguments: json!({
                    "action": "transfer"
                }),
            }],
        },
        LlmResponse {
            text: "Paid 0.01 SOL successfully".to_string(),
            tool_calls: Vec::new(),
        },
    ]));

    let agent = AIAgent {
        name: "agent-x402-grounded-submit".to_string(),
        instructions: "inst".to_string(),
        specialization: "spec".to_string(),
    };
    let service = AgentService::new(llm, agent, None, None, None, None, brain_manager, None);

    let http_call = Arc::new(RecordingFixedResultTool::new(
        "http_call",
        json!({
            "status": "ok",
            "http_status": 402,
            "json": {
                "x402Version": 2,
                "resource": {
                    "description": "paid",
                    "mimeType": "application/json",
                    "url": "https://x402.payai.network/api/solana/paid-content"
                },
                "accepts": [{
                    "scheme": "exact",
                    "network": "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp",
                    "amount": "10000",
                    "payTo": "H32YnqbzL62YkHMSCzfKcLry9yuipwwx1EMztiCSPhjb",
                    "maxTimeoutSeconds": 300,
                    "asset": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
                }]
            }
        }),
    ));
    let solana = Arc::new(FixedResultTool::new(
        "solana",
        json!({
            "status": "submitted",
            "signature": "sig-abc",
            "decimals": 6
        }),
    ));

    let registry = service.tool_registry.clone();
    assert!(registry.register_tool(http_call).await);
    assert!(registry.register_tool(solana).await);
    assert!(
        registry
            .assign_tool_to_agent(service.agent_name(), "http_call")
            .await
    );
    assert!(
        registry
            .assign_tool_to_agent(service.agent_name(), "solana")
            .await
    );

    let response = service
        .generate_response(
            "u1",
            "Pay with x402 - https://x402.payai.network/api/solana/paid-content",
            "",
            None,
        )
        .await
        .unwrap();

    assert!(response.contains("x402 payment submitted successfully"));
    assert!(response.contains("Amount: 0.01 USDC"));
    assert!(response.contains("H32YnqbzL62YkHMSCzfKcLry9yuipwwx1EMztiCSPhjb"));
    assert!(response.contains("sig-abc"));
}

#[tokio::test]
async fn x402_submission_prefers_mint_symbol_over_ambiguous_asset_label() {
    let brain_manager = Arc::new(BrainManager::new(json!({})));
    let llm = Arc::new(QueueLlmProvider::new(vec![
        LlmResponse {
            text: "".to_string(),
            tool_calls: vec![ToolCall {
                name: "solana".to_string(),
                arguments: json!({ "action": "transfer" }),
            }],
        },
        LlmResponse {
            text: "done".to_string(),
            tool_calls: Vec::new(),
        },
    ]));

    let agent = AIAgent {
        name: "agent-x402-usdc-symbol".to_string(),
        instructions: "inst".to_string(),
        specialization: "spec".to_string(),
    };
    let service = AgentService::new(llm, agent, None, None, None, None, brain_manager, None);

    let http_call = Arc::new(RecordingFixedResultTool::new(
        "http_call",
        json!({
            "status": "ok",
            "http_status": 402,
            "json": {
                "x402Version": 2,
                "resource": {
                    "description": "paid",
                    "mimeType": "application/json",
                    "url": "https://x402.payai.network/api/solana/paid-content"
                },
                "accepts": [{
                    "scheme": "exact",
                    "network": "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp",
                    "amount": "10000",
                    "payTo": "H32YnqbzL62YkHMSCzfKcLry9yuipwwx1EMztiCSPhjb",
                    "maxTimeoutSeconds": 300,
                    "asset": "ai token"
                }]
            }
        }),
    ));
    let solana = Arc::new(FixedResultTool::new(
        "solana",
        json!({
            "status": "submitted",
            "signature": "sig-usdc",
            "mint": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v",
            "amount_atomic": 10000,
            "decimals": 6
        }),
    ));

    let registry = service.tool_registry.clone();
    assert!(registry.register_tool(http_call).await);
    assert!(registry.register_tool(solana).await);
    assert!(
        registry
            .assign_tool_to_agent(service.agent_name(), "http_call")
            .await
    );
    assert!(
        registry
            .assign_tool_to_agent(service.agent_name(), "solana")
            .await
    );

    let response = service
        .generate_response(
            "u1",
            "Pay with x402 - https://x402.payai.network/api/solana/paid-content",
            "",
            None,
        )
        .await
        .unwrap();

    assert!(response.contains("0.01 USDC"));
    assert!(!response.contains("ai token"));
}

#[tokio::test]
async fn x402_minimal_body_without_resource_still_maps_token_args() {
    let brain_manager = Arc::new(BrainManager::new(json!({})));
    let llm = Arc::new(QueueLlmProvider::new(vec![
        LlmResponse {
            text: "".to_string(),
            tool_calls: vec![ToolCall {
                name: "solana".to_string(),
                arguments: json!({
                    "action": "simulate_transfer",
                    "to": "H32YnqbzL62YkHMSCzfKcLry9yuipwwx1EMztiCSPhjb",
                    "lamports": 10_000_000
                }),
            }],
        },
        LlmResponse {
            text: "done".to_string(),
            tool_calls: Vec::new(),
        },
    ]));

    let agent = AIAgent {
        name: "agent-x402-minimal-body".to_string(),
        instructions: "inst".to_string(),
        specialization: "spec".to_string(),
    };
    let service = AgentService::new(llm, agent, None, None, None, None, brain_manager, None);

    let http_call = Arc::new(RecordingFixedResultTool::new(
        "http_call",
        json!({
            "status": "ok",
            "http_status": 402,
            "json": {
                "x402Version": 2,
                "error": "PAYMENT-SIGNATURE header is required",
                "accepts": [{
                    "scheme": "exact",
                    "network": "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp",
                    "amount": "10000",
                    "payTo": "H32YnqbzL62YkHMSCzfKcLry9yuipwwx1EMztiCSPhjb",
                    "maxTimeoutSeconds": 60,
                    "asset": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
                }]
            }
        }),
    ));
    let solana = Arc::new(RecordingTool::new("solana"));

    let registry = service.tool_registry.clone();
    assert!(registry.register_tool(http_call).await);
    assert!(registry.register_tool(solana.clone()).await);
    assert!(
        registry
            .assign_tool_to_agent(service.agent_name(), "http_call")
            .await
    );
    assert!(
        registry
            .assign_tool_to_agent(service.agent_name(), "solana")
            .await
    );

    let _ = service
        .generate_response(
            "u1",
            "Pay with x402 - https://x402.payai.network/api/solana/paid-content",
            "",
            None,
        )
        .await
        .unwrap();

    let calls = solana.calls.lock().await;
    assert_eq!(calls.len(), 1);
    assert_eq!(
        calls[0].get("mint").and_then(|v| v.as_str()),
        Some("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v")
    );
    assert_eq!(
        calls[0].get("amount_atomic").and_then(|v| v.as_u64()),
        Some(10000)
    );
    assert_eq!(calls[0].get("lamports"), None);
}

#[tokio::test]
async fn x402_bad_http_call_url_is_skipped_not_fatal() {
    let brain_manager = Arc::new(BrainManager::new(json!({})));
    let llm = Arc::new(QueueLlmProvider::new(vec![
        LlmResponse {
            text: "".to_string(),
            tool_calls: vec![
                ToolCall {
                    name: "http_call".to_string(),
                    arguments: json!({
                        "method": "GET",
                        "url": "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp"
                    }),
                },
                ToolCall {
                    name: "solana".to_string(),
                    arguments: json!({
                        "action": "simulate_transfer"
                    }),
                },
            ],
        },
        LlmResponse {
            text: "done".to_string(),
            tool_calls: Vec::new(),
        },
    ]));

    let agent = AIAgent {
        name: "agent-x402-skip-bad-http".to_string(),
        instructions: "inst".to_string(),
        specialization: "spec".to_string(),
    };
    let service = AgentService::new(llm, agent, None, None, None, None, brain_manager, None);

    let http_call = Arc::new(FailingHttpCallTool::new(json!({
        "status": "ok",
        "http_status": 402,
        "json": {
            "x402Version": 2,
            "accepts": [{
                "scheme": "exact",
                "network": "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp",
                "amount": "10000",
                "payTo": "H32YnqbzL62YkHMSCzfKcLry9yuipwwx1EMztiCSPhjb",
                "maxTimeoutSeconds": 60,
                "asset": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
            }]
        }
    })));
    let solana = Arc::new(RecordingTool::new("solana"));
    let registry = service.tool_registry.clone();
    assert!(registry.register_tool(http_call).await);
    assert!(registry.register_tool(solana.clone()).await);
    assert!(
        registry
            .assign_tool_to_agent(service.agent_name(), "http_call")
            .await
    );
    assert!(
        registry
            .assign_tool_to_agent(service.agent_name(), "solana")
            .await
    );

    let response = service
        .generate_response(
            "u1",
            "Pay x402 - https://x402.payai.network/api/solana/paid-content",
            "",
            None,
        )
        .await
        .unwrap();

    assert_eq!(response, "done");
    let calls = solana.calls.lock().await;
    assert_eq!(calls.len(), 1);
}

#[tokio::test]
async fn x402_transfer_blocked_without_canonical_intent() {
    let brain_manager = Arc::new(BrainManager::new(json!({})));
    let llm = Arc::new(QueueLlmProvider::new(vec![LlmResponse {
        text: "".to_string(),
        tool_calls: vec![ToolCall {
            name: "solana".to_string(),
            arguments: json!({
                "action": "transfer",
                "to": "H32YnqbzL62YkHMSCzfKcLry9yuipwwx1EMztiCSPhjb",
                "lamports": 10600000
            }),
        }],
    }]));

    let agent = AIAgent {
        name: "agent-x402-block-without-intent".to_string(),
        instructions: "inst".to_string(),
        specialization: "spec".to_string(),
    };
    let service = AgentService::new(llm, agent, None, None, None, None, brain_manager, None);

    let solana = Arc::new(RecordingTool::new("solana"));
    let http_call = Arc::new(FixedResultTool::new(
        "http_call",
        json!({
            "status": "ok",
            "http_status": 200,
            "json": {"ok": true}
        }),
    ));
    let registry = service.tool_registry.clone();
    assert!(registry.register_tool(solana.clone()).await);
    assert!(registry.register_tool(http_call).await);
    assert!(
        registry
            .assign_tool_to_agent(service.agent_name(), "solana")
            .await
    );
    assert!(
        registry
            .assign_tool_to_agent(service.agent_name(), "http_call")
            .await
    );

    let err = service
        .generate_response(
            "u1",
            "Pay x402 - https://x402.payai.network/api/solana/paid-content",
            "",
            None,
        )
        .await
        .unwrap_err();
    assert!(err
        .to_string()
        .contains("x402 payment requirement not resolved yet"));

    let calls = solana.calls.lock().await;
    assert_eq!(calls.len(), 0);
}
