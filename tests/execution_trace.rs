mod common;

use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use tokio::sync::broadcast;

use butterfly_bot::brain::manager::BrainManager;
use butterfly_bot::domains::agent::AIAgent;
use butterfly_bot::interfaces::providers::{LlmResponse, ToolCall};
use butterfly_bot::services::agent::{AgentService, UiEvent};

use common::{DummyTool, QueueLlmProvider};

async fn recv_event(rx: &mut broadcast::Receiver<UiEvent>) -> UiEvent {
    tokio::time::timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("timed out waiting for ui event")
        .expect("ui event channel closed")
}

#[tokio::test]
async fn execution_trace_redacts_tool_event_payload() {
    let llm = Arc::new(QueueLlmProvider::new(vec![
        LlmResponse {
            text: "Action: call trace_tool for secret work".to_string(),
            tool_calls: vec![ToolCall {
                name: "trace_tool".to_string(),
                arguments: json!({
                    "api_key": "sk-abc123",
                    "authorization": "Bearer raw-token",
                    "note": "github_pat_abcdef12345"
                }),
            }],
        },
        LlmResponse {
            text: "Summary: complete".to_string(),
            tool_calls: Vec::new(),
        },
    ]));
    let brain = Arc::new(BrainManager::new(json!({})));
    let (tx, mut rx) = broadcast::channel(16);

    let agent = AIAgent {
        name: "trace-agent".to_string(),
        instructions: "trace".to_string(),
        specialization: "ops".to_string(),
    };
    let service = AgentService::new(llm, agent, None, None, None, None, brain, Some(tx));

    let tool = Arc::new(DummyTool::new("dummy_tool"));
    assert!(service.tool_registry.register_tool(tool).await);
    assert!(
        service
            .tool_registry
            .assign_tool_to_agent(service.agent_name(), "dummy_tool")
            .await
    );

    let _ = service
        .generate_response("u1", "run trace", "", None)
        .await
        .unwrap();

    let event = recv_event(&mut rx).await;
    assert_eq!(event.event_type, "tool");
    assert_eq!(event.user_id, "u1");
    assert_eq!(event.tool, "trace_tool");
    assert_eq!(event.status, "not_found");
    assert!(event.timestamp > 0);

    let args = event.payload.get("args").expect("args payload");
    assert_eq!(args.get("api_key").and_then(|v| v.as_str()), Some("[REDACTED]"));
    assert_eq!(
        args.get("authorization").and_then(|v| v.as_str()),
        Some("[REDACTED]")
    );
    assert_eq!(
        args.get("note").and_then(|v| v.as_str()),
        Some("github_pat_[REDACTED]")
    );

    assert_eq!(
        event.payload.get("message").and_then(|v| v.as_str()),
        Some("Tool not found")
    );
}

#[tokio::test]
async fn execution_trace_emits_events_in_call_order() {
    let llm = Arc::new(QueueLlmProvider::new(vec![
        LlmResponse {
            text: "Action: call trace_tool first".to_string(),
            tool_calls: vec![ToolCall {
                name: "trace_tool".to_string(),
                arguments: json!({"step": 1}),
            }],
        },
        LlmResponse {
            text: "Action: call trace_tool second".to_string(),
            tool_calls: vec![ToolCall {
                name: "trace_tool".to_string(),
                arguments: json!({"step": 2}),
            }],
        },
        LlmResponse {
            text: "Summary: done".to_string(),
            tool_calls: Vec::new(),
        },
    ]));
    let brain = Arc::new(BrainManager::new(json!({})));
    let (tx, mut rx) = broadcast::channel(16);

    let agent = AIAgent {
        name: "trace-agent".to_string(),
        instructions: "trace".to_string(),
        specialization: "ops".to_string(),
    };
    let service = AgentService::new(llm, agent, None, None, None, None, brain, Some(tx));

    let tool = Arc::new(DummyTool::new("dummy_tool"));
    assert!(service.tool_registry.register_tool(tool).await);
    assert!(
        service
            .tool_registry
            .assign_tool_to_agent(service.agent_name(), "dummy_tool")
            .await
    );

    let _ = service
        .generate_response("u2", "run two steps", "", None)
        .await
        .unwrap();

    let first = recv_event(&mut rx).await;
    let second = recv_event(&mut rx).await;

    assert_eq!(first.event_type, "tool");
    assert_eq!(second.event_type, "tool");
    assert_eq!(first.user_id, "u2");
    assert_eq!(second.user_id, "u2");
    assert_eq!(first.tool, "trace_tool");
    assert_eq!(second.tool, "trace_tool");
    assert_eq!(first.status, "not_found");
    assert_eq!(second.status, "not_found");

    let first_step = first
        .payload
        .get("args")
        .and_then(|v| v.get("step"))
        .and_then(|v| v.as_i64());
    let second_step = second
        .payload
        .get("args")
        .and_then(|v| v.get("step"))
        .and_then(|v| v.as_i64());
    assert_eq!(first_step, Some(1));
    assert_eq!(second_step, Some(2));
    assert!(second.timestamp >= first.timestamp);
}