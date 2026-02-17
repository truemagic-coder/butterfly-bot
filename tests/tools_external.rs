use butterfly_bot::error::ButterflyBotError;
use butterfly_bot::interfaces::plugins::Tool;
use butterfly_bot::tools::coding::CodingTool;
use butterfly_bot::tools::github::GitHubTool;
use butterfly_bot::tools::http_call::HttpCallTool;
use butterfly_bot::tools::mcp::McpTool;
use butterfly_bot::tools::search_internet::SearchInternetTool;
use butterfly_bot::tools::zapier::ZapierTool;
use httpmock::Method::{GET, POST};
use httpmock::MockServer;
use serde_json::json;

fn assert_runtime_err_contains(err: ButterflyBotError, expected: &str) {
    match err {
        ButterflyBotError::Runtime(message) => assert!(
            message.contains(expected),
            "expected runtime error containing '{expected}', got '{message}'"
        ),
        other => panic!("expected runtime error, got {other}"),
    }
}

#[tokio::test]
async fn http_call_tool_uses_base_url_headers_query_and_parses_json() {
    let server = MockServer::start_async().await;
    let request_mock = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/v1/ping")
                .header("x-default", "alpha")
                .header("x-extra", "beta")
                .query_param("q", "rust");
            then.status(200)
                .header("content-type", "application/json")
                .json_body(json!({"ok": true, "source": "mock"}));
        })
        .await;

    let tool = HttpCallTool::new();
    tool.configure(&json!({
        "tools": {
            "http_call": {
                "base_urls": [format!("{}/v1", server.base_url())],
                "custom_headers": {"x-default": "alpha"},
                "timeout_seconds": 2
            }
        }
    }))
    .expect("configure http_call");

    let result = tool
        .execute(json!({
            "method": "get",
            "endpoint": "ping",
            "headers": {"x-extra": "beta"},
            "query": {"q": "rust"}
        }))
        .await
        .expect("execute http_call");

    assert_eq!(result["status"], json!("ok"));
    assert_eq!(result["http_status"], json!(200));
    assert_eq!(result["json"]["ok"], json!(true));
    request_mock.assert_calls(1);
}

#[tokio::test]
async fn http_call_tool_infers_json_from_string_body() {
    let server = MockServer::start_async().await;
    let request_mock = server
        .mock_async(|when, then| {
            when.method(POST)
                .path("/submit")
                .header("content-type", "application/json");
            then.status(200)
                .header("content-type", "application/json")
                .json_body(json!({"accepted": true}));
        })
        .await;

    let tool = HttpCallTool::new();

    let result = tool
        .execute(json!({
            "method": "POST",
            "url": format!("{}/submit", server.base_url()),
            "body": "{\"hello\":\"world\"}"
        }))
        .await
        .expect("execute http_call post");

    assert_eq!(result["status"], json!("ok"));
    assert_eq!(result["json"]["accepted"], json!(true));
    request_mock.assert_calls(1);
}

#[tokio::test]
async fn http_call_tool_requires_server_when_multiple_configured() {
    let tool = HttpCallTool::new();
    tool.configure(&json!({
        "tools": {
            "http_call": {
                "servers": [
                    {"name": "a", "url": "http://localhost:3001"},
                    {"name": "b", "url": "http://localhost:3002"}
                ]
            }
        }
    }))
    .expect("configure http_call servers");

    let err = tool
        .execute(json!({
            "method": "GET",
            "endpoint": "ping"
        }))
        .await
        .expect_err("multiple servers should require server name");

    assert_runtime_err_contains(err, "Multiple HTTP call servers configured");
}

#[tokio::test]
async fn http_call_tool_uses_named_server_config() {
    let server = MockServer::start_async().await;
    let request_mock = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/v1/ping")
                .header("x-default", "alpha")
                .query_param("q", "rust");
            then.status(200)
                .header("content-type", "application/json")
                .json_body(json!({"ok": true}));
        })
        .await;

    let tool = HttpCallTool::new();
    tool.configure(&json!({
        "tools": {
            "http_call": {
                "servers": [
                    {
                        "name": "primary",
                        "url": format!("{}/v1", server.base_url()),
                        "headers": {"x-default": "alpha"}
                    }
                ]
            }
        }
    }))
    .expect("configure http_call servers");

    let result = tool
        .execute(json!({
            "method": "GET",
            "server": "primary",
            "endpoint": "ping",
            "query": {"q": "rust"}
        }))
        .await
        .expect("execute http_call with named server");

    assert_eq!(result["status"], json!("ok"));
    assert_eq!(result["server"], json!("primary"));
    assert_eq!(result["http_status"], json!(200));
    request_mock.assert_calls(1);
}

#[tokio::test]
async fn coding_tool_execute_fails_without_api_key() {
    let tool = CodingTool::new();
    tool.configure(&json!({"tools": {"coding": {}}}))
        .expect("configure coding tool");

    let err = tool
        .execute(json!({"prompt": "write rust"}))
        .await
        .expect_err("missing key should fail");
    assert_runtime_err_contains(err, "Missing coding tool api_key");
}

#[tokio::test]
async fn mcp_tool_validates_config_and_reports_routing_errors() {
    let tool = McpTool::new();

    let configure_err = tool
        .configure(&json!({
            "tools": {"mcp": {"servers": [{"name": "demo"}]}}
        }))
        .expect_err("missing url should fail");
    match configure_err {
        ButterflyBotError::Config(message) => {
            assert!(message.contains("requires name and url"));
        }
        other => panic!("expected config error, got {other}"),
    }

    tool.configure(&json!({
        "tools": {
            "mcp": {
                "servers": [
                    {"name": "a", "url": "http://localhost:3001", "type": "http"},
                    {"name": "b", "url": "http://localhost:3002", "type": "http"}
                ]
            }
        }
    }))
    .expect("configure multiple servers");

    let no_server_err = tool
        .execute(json!({"action": "list_tools"}))
        .await
        .expect_err("missing server should fail with multiple configured");
    assert_runtime_err_contains(no_server_err, "Multiple MCP servers configured");
}

#[tokio::test]
async fn github_tool_secret_requirement_and_missing_pat_error() {
    let tool = GitHubTool::new();
    let needed = tool.required_secrets_for_config(&json!({"tools": {"github": {}}}));
    assert_eq!(needed.len(), 1);
    assert_eq!(needed[0].name, "github_pat");

    tool.configure(&json!({"tools": {"github": {}}}))
        .expect("configure github tool");
    let err = tool
        .execute(json!({"action": "list_tools"}))
        .await
        .expect_err("missing PAT should fail");
    assert_runtime_err_contains(err, "Missing GitHub PAT");
}

#[tokio::test]
async fn zapier_tool_secret_requirement_and_missing_token_error() {
    let tool = ZapierTool::new();
    let needed = tool.required_secrets_for_config(&json!({
        "tools": {"zapier": {"url": "https://mcp.zapier.com/api/v1/connect"}}
    }));
    assert_eq!(needed.len(), 1);
    assert_eq!(needed[0].name, "zapier_token");

    tool.configure(&json!({
        "tools": {"zapier": {"url": "https://mcp.zapier.com/api/v1/connect"}}
    }))
    .expect("configure zapier tool");
    let err = tool
        .execute(json!({"action": "list_tools"}))
        .await
        .expect_err("missing token should fail");
    assert_runtime_err_contains(err, "Missing Zapier token");
}

#[tokio::test]
async fn zapier_tool_placeholder_token_is_rejected() {
    let tool = ZapierTool::new();
    let needed = tool.required_secrets_for_config(&json!({
        "tools": {"zapier": {"url": "https://mcp.zapier.com/api/v1/connect?token=my_token"}}
    }));
    assert_eq!(needed.len(), 1);
    assert_eq!(needed[0].name, "zapier_token");

    tool.configure(&json!({
        "tools": {"zapier": {"url": "https://mcp.zapier.com/api/v1/connect?token=my_token"}}
    }))
    .expect("configure zapier tool");

    let err = tool
        .execute(json!({"action": "list_tools"}))
        .await
        .expect_err("placeholder token should fail fast");
    assert_runtime_err_contains(err, "Missing Zapier token");
}

#[tokio::test]
async fn search_internet_tool_honors_network_policy_and_query_validation() {
    let tool = SearchInternetTool::new();
    tool.configure(&json!({
        "tools": {
            "settings": {
                "permissions": {
                    "default_deny": true,
                    "network_allow": []
                }
            },
            "search_internet": {
                "provider": "openai"
            }
        }
    }))
    .expect("configure search_internet");

    let missing_query = tool
        .execute(json!({}))
        .await
        .expect("missing query response");
    assert_eq!(missing_query["status"], json!("error"));
    assert_eq!(missing_query["message"], json!("query is required"));

    let denied = tool
        .execute(json!({"query": "latest rust release"}))
        .await
        .expect("denied response");
    assert_eq!(denied["status"], json!("error"));
    assert!(denied["message"]
        .as_str()
        .expect("message")
        .contains("Network access denied"));
}
