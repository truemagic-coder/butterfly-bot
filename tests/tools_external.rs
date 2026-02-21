use butterfly_bot::error::ButterflyBotError;
use butterfly_bot::interfaces::plugins::Tool;
use butterfly_bot::tools::coding::CodingTool;
use butterfly_bot::tools::github::GitHubTool;
use butterfly_bot::tools::http_call::HttpCallTool;
use butterfly_bot::tools::mcp::McpTool;
use butterfly_bot::tools::search_internet::SearchInternetTool;
use butterfly_bot::tools::solana::SolanaTool;
use butterfly_bot::tools::zapier::ZapierTool;
use httpmock::Method::{GET, POST};
use httpmock::MockServer;
use serde_json::json;
use std::sync::{Once, OnceLock};

fn setup_security_env() {
    static ROOT: OnceLock<std::path::PathBuf> = OnceLock::new();
    let root = ROOT
        .get_or_init(|| {
            let unique = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path =
                std::env::temp_dir().join(format!("butterfly-tools-external-tests-root-{unique}"));
            std::fs::create_dir_all(&path).unwrap();
            path
        })
        .clone();

    butterfly_bot::runtime_paths::set_debug_app_root_override(Some(root));
    butterfly_bot::security::tpm_provider::set_debug_tpm_available_override(Some(true));
    butterfly_bot::security::tpm_provider::set_debug_dek_passphrase_override(Some(
        "tools-external-test-dek".to_string(),
    ));
    butterfly_bot::vault::set_secret("db_encryption_key", "tools-external-test-sqlcipher-key")
        .expect("set deterministic external tools db key");
}

fn disable_keyring_for_test_process() {
    static ONCE: Once = Once::new();
    ONCE.call_once(|| {
        std::env::set_var("BUTTERFLY_BOT_DISABLE_KEYRING", "1");
    });
}

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
    setup_security_env();
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
    setup_security_env();
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
async fn http_call_tool_accepts_absolute_endpoint_without_server_config() {
    setup_security_env();
    let server = MockServer::start_async().await;
    let request_mock = server
        .mock_async(|when, then| {
            when.method(GET).path("/healthz");
            then.status(200)
                .header("content-type", "application/json")
                .json_body(json!({"ok": true}));
        })
        .await;

    let tool = HttpCallTool::new();
    let result = tool
        .execute(json!({
            "method": "GET",
            "endpoint": format!("{}/healthz", server.base_url())
        }))
        .await
        .expect("execute http_call with absolute endpoint");

    assert_eq!(result["status"], json!("ok"));
    assert_eq!(result["http_status"], json!(200));
    assert_eq!(result["json"]["ok"], json!(true));
    request_mock.assert_calls(1);
}

#[tokio::test]
async fn http_call_tool_requires_server_when_multiple_configured() {
    setup_security_env();
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
    setup_security_env();
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
async fn http_call_tool_supports_multiple_server_headers() {
    setup_security_env();
    let server = MockServer::start_async().await;
    let request_mock = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/v1/ping")
                .header("x-default", "alpha")
                .header("x-auth", "token-123")
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
                        "headers": {
                            "x-default": "alpha",
                            "x-auth": "token-123"
                        }
                    }
                ]
            }
        }
    }))
    .expect("configure http_call multi-headers");

    let result = tool
        .execute(json!({
            "method": "GET",
            "server": "primary",
            "endpoint": "ping",
            "query": {"q": "rust"}
        }))
        .await
        .expect("execute http_call with multiple server headers");

    assert_eq!(result["status"], json!("ok"));
    assert_eq!(result["http_status"], json!(200));
    request_mock.assert_calls(1);
}

#[tokio::test]
async fn http_call_tool_merges_multiple_legacy_headers() {
    setup_security_env();
    let server = MockServer::start_async().await;
    let request_mock = server
        .mock_async(|when, then| {
            when.method(GET)
                .path("/v1/ping")
                .header("x-custom", "alpha")
                .header("x-default", "beta")
                .query_param("q", "legacy");
            then.status(200)
                .header("content-type", "application/json")
                .json_body(json!({"ok": true}));
        })
        .await;

    let tool = HttpCallTool::new();
    tool.configure(&json!({
        "tools": {
            "http_call": {
                "base_urls": [format!("{}/v1", server.base_url())],
                "custom_headers": {
                    "x-custom": "alpha"
                },
                "default_headers": {
                    "x-default": "beta"
                }
            }
        }
    }))
    .expect("configure http_call legacy headers");

    let result = tool
        .execute(json!({
            "method": "GET",
            "endpoint": "ping",
            "query": {"q": "legacy"}
        }))
        .await
        .expect("execute http_call legacy headers");

    assert_eq!(result["status"], json!("ok"));
    assert_eq!(result["http_status"], json!(200));
    request_mock.assert_calls(1);
}

#[tokio::test]
async fn coding_tool_execute_fails_without_api_key() {
    setup_security_env();
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
async fn coding_tool_requires_prompt_before_provider_call() {
    setup_security_env();
    let tool = CodingTool::new();
    tool.configure(&json!({"tools": {"coding": {"api_key": "sk-test"}}}))
        .expect("configure coding tool with api key");

    let err = tool
        .execute(json!({}))
        .await
        .expect_err("missing prompt should fail early");
    assert_runtime_err_contains(err, "Missing prompt");
}

#[tokio::test]
async fn mcp_tool_validates_config_and_reports_routing_errors() {
    setup_security_env();
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
async fn mcp_tool_surfaces_configuration_and_action_errors() {
    setup_security_env();
    let tool = McpTool::new();

    let empty_err = tool
        .execute(json!({"action": "list_tools"}))
        .await
        .expect_err("no configured MCP servers should fail");
    assert_runtime_err_contains(empty_err, "No MCP servers configured");

    tool.configure(&json!({
        "tools": {
            "mcp": {
                "servers": [
                    {"name": "primary", "url": "http://localhost:3001"}
                ]
            }
        }
    }))
    .expect("configure single mcp server");

    let unknown_server_err = tool
        .execute(json!({"action": "list_tools", "server": "missing"}))
        .await
        .expect_err("unknown server should fail");
    assert_runtime_err_contains(unknown_server_err, "Unknown MCP server");

    let missing_tool_err = tool
        .execute(json!({"action": "call_tool", "server": "primary"}))
        .await
        .expect_err("call_tool requires tool field");
    assert_runtime_err_contains(missing_tool_err, "Missing tool name");

    let unsupported_action_err = tool
        .execute(json!({"action": "not_real", "server": "primary"}))
        .await
        .expect_err("unsupported actions should fail");
    assert_runtime_err_contains(unsupported_action_err, "Unsupported action");

    tool.configure(&json!({
        "tools": {
            "mcp": {
                "servers": [
                    {
                        "name": "primary",
                        "url": "http://localhost:3001",
                        "headers": {"bad header": "value"}
                    }
                ]
            }
        }
    }))
    .expect("configure mcp with invalid header for runtime validation");

    let invalid_header_err = tool
        .execute(json!({"action": "list_tools", "server": "primary"}))
        .await
        .expect_err("invalid header should fail before transport");
    match invalid_header_err {
        ButterflyBotError::Config(message) => {
            assert!(message.contains("Invalid MCP header name"));
        }
        other => panic!("expected config error, got {other}"),
    }
}

#[tokio::test]
async fn github_tool_secret_requirement_and_missing_pat_error() {
    setup_security_env();
    disable_keyring_for_test_process();
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
async fn github_tool_respects_authorization_header_and_validates_actions() {
    setup_security_env();
    disable_keyring_for_test_process();
    let tool = GitHubTool::new();

    let needed = tool.required_secrets_for_config(&json!({
        "tools": {
            "github": {
                "headers": {
                    "Authorization": "Bearer test-token"
                }
            }
        }
    }));
    assert!(needed.is_empty());

    tool.configure(&json!({
        "tools": {
            "github": {
                "headers": {
                    "Authorization": "Bearer test-token"
                }
            }
        }
    }))
    .expect("configure github tool with auth header");

    let unsupported_err = tool
        .execute(json!({"action": "nope"}))
        .await
        .expect_err("unsupported action should fail");
    assert_runtime_err_contains(unsupported_err, "Unsupported action");

    let missing_tool_err = tool
        .execute(json!({"action": "call_tool"}))
        .await
        .expect_err("call_tool requires tool name");
    assert_runtime_err_contains(missing_tool_err, "Missing tool name");
}

#[tokio::test]
async fn zapier_tool_secret_requirement_and_missing_token_error() {
    setup_security_env();
    disable_keyring_for_test_process();
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
    setup_security_env();
    disable_keyring_for_test_process();
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
async fn zapier_tool_header_auth_and_action_validation() {
    setup_security_env();
    disable_keyring_for_test_process();
    let tool = ZapierTool::new();

    let needed = tool.required_secrets_for_config(&json!({
        "tools": {
            "zapier": {
                "headers": {
                    "Authorization": "Bearer token-123"
                }
            }
        }
    }));
    assert!(needed.is_empty());

    tool.configure(&json!({
        "tools": {
            "zapier": {
                "headers": {
                    "Authorization": "Bearer token-123"
                }
            }
        }
    }))
    .expect("configure zapier with auth header");

    let unsupported_err = tool
        .execute(json!({"action": "invalid"}))
        .await
        .expect_err("unsupported action should fail");
    assert_runtime_err_contains(unsupported_err, "Unsupported action");

    let missing_tool_err = tool
        .execute(json!({"action": "call_tool"}))
        .await
        .expect_err("call_tool requires tool name");
    assert_runtime_err_contains(missing_tool_err, "Missing tool name");
}

#[tokio::test]
async fn search_internet_tool_honors_network_policy_and_query_validation() {
    setup_security_env();
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

#[tokio::test]
async fn solana_tool_requires_rpc_endpoint_for_network_actions() {
    setup_security_env();
    let tool = SolanaTool::new();

    let err = tool
        .execute(json!({
            "action": "get_balance",
            "address": "11111111111111111111111111111111"
        }))
        .await
        .expect_err("missing endpoint should fail");

    match err {
        ButterflyBotError::Config(message) => {
            assert!(message.contains("tools.settings.solana.rpc.endpoint"));
        }
        other => panic!("expected config error, got {other}"),
    }
}

#[tokio::test]
async fn solana_tool_wallet_balance_transfer_status_and_history_workflow() {
    setup_security_env();
    let rpc = MockServer::start_async().await;

    let get_balance = rpc
        .mock_async(|when, then| {
            when.method(POST)
                .path("/")
                .body_includes("\"method\":\"getBalance\"");
            then.status(200).json_body(
                json!({"jsonrpc":"2.0","id":1,"result":{"context":{"slot":1},"value":42}}),
            );
        })
        .await;

    let get_latest_blockhash = rpc
        .mock_async(|when, then| {
            when.method(POST)
                .path("/")
                .body_includes("\"method\":\"getLatestBlockhash\"");
            then.status(200).json_body(json!({
                "jsonrpc":"2.0",
                "id":1,
                "result":{"context":{"slot":1},"value":{"blockhash":"11111111111111111111111111111111","lastValidBlockHeight":100}}
            }));
        })
        .await;

    let simulate_transaction = rpc
        .mock_async(|when, then| {
            when.method(POST)
                .path("/")
                .body_includes("\"method\":\"simulateTransaction\"");
            then.status(200).json_body(
                json!({"jsonrpc":"2.0","id":1,"result":{"context":{"slot":1},"value":{"err":null,"unitsConsumed":1000000}}}),
            );
        })
        .await;

    let send_transaction = rpc
        .mock_async(|when, then| {
            when.method(POST)
                .path("/")
                .body_includes("\"method\":\"sendTransaction\"");
            then.status(200)
                .json_body(json!({"jsonrpc":"2.0","id":1,"result":"sig-tool-123"}));
        })
        .await;

    let signature_status = rpc
        .mock_async(|when, then| {
            when.method(POST)
                .path("/")
                .body_includes("\"method\":\"getSignatureStatuses\"");
            then.status(200).json_body(
                json!({"jsonrpc":"2.0","id":1,"result":{"context":{"slot":1},"value":[{"confirmationStatus":"confirmed","err":null}]}}),
            );
        })
        .await;

    let history = rpc
        .mock_async(|when, then| {
            when.method(POST)
                .path("/")
                .body_includes("\"method\":\"getSignaturesForAddress\"");
            then.status(200).json_body(json!({
                "jsonrpc":"2.0",
                "id":1,
                "result":[{"signature":"sig-tool-123","slot":1,"err":null}]
            }));
        })
        .await;

    let tool = SolanaTool::new();
    tool.configure(&json!({
        "tools": {
            "settings": {
                "solana": {
                    "rpc": {
                        "provider": "custom",
                        "endpoint": rpc.base_url(),
                        "simulation": {
                            "enabled": true,
                            "replace_recent_blockhash": true,
                            "sig_verify": false
                        },
                        "send": {
                            "skip_preflight": false,
                            "max_retries": 0
                        }
                    }
                }
            }
        }
    }))
    .expect("configure solana tool");

    let wallet = tool
        .execute(json!({"action":"address","user_id":"u1"}))
        .await
        .expect("wallet action alias should work");
    assert_eq!(wallet["status"], json!("ok"));
    let address = wallet["address"]
        .as_str()
        .expect("wallet address")
        .to_string();

    let balance = tool
        .execute(json!({"action":"get_balance","address":address}))
        .await
        .expect("balance action alias should work");
    assert_eq!(balance["status"], json!("ok"));
    assert_eq!(balance["lamports"], json!(42));

    let simulated = tool
        .execute(json!({
            "action": "dry_run",
            "user_id": "u1",
            "to": "11111111111111111111111111111111",
            "amount": "0.000001 sol"
        }))
        .await
        .expect("simulate alias should work");
    assert_eq!(simulated["status"], json!("simulated"));
    assert!(simulated["signature"].is_null());

    let submitted = tool
        .execute(json!({
            "action": "send",
            "user_id": "u1",
            "to": "11111111111111111111111111111111",
            "lamports": 1000
        }))
        .await
        .expect("transfer submit should work");
    assert_eq!(submitted["status"], json!("submitted"));
    assert_eq!(submitted["signature"], json!("sig-tool-123"));

    let status = tool
        .execute(json!({"action":"status","signature":"sig-tool-123"}))
        .await
        .expect("status alias should work");
    assert_eq!(status["status"], json!("ok"));

    let tx_history = tool
        .execute(json!({"action":"history","address":"11111111111111111111111111111111","limit":5}))
        .await
        .expect("history alias should work");
    assert_eq!(tx_history["status"], json!("ok"));
    assert_eq!(tx_history["entries"][0]["signature"], json!("sig-tool-123"));

    get_balance.assert_calls(1);
    get_latest_blockhash.assert_calls(2);
    simulate_transaction.assert_calls(2);
    send_transaction.assert_calls(1);
    signature_status.assert_calls(1);
    history.assert_calls(1);
}

#[tokio::test]
async fn solana_tool_balance_with_mint_uses_spl_token_rpc() {
    setup_security_env();
    let rpc = MockServer::start_async().await;

    let get_token_accounts = rpc
        .mock_async(|when, then| {
            when.method(POST)
                .path("/")
                .body_includes("\"method\":\"getTokenAccountsByOwner\"")
                .body_includes("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v");
            then.status(200).json_body(json!({
                "jsonrpc":"2.0",
                "id":1,
                "result":{
                    "context":{"slot":1},
                    "value":[{
                        "pubkey":"9f1MFK8nQ7kkh2YkSK36D6cvn18PkEhGj4N8rn4vQ6iX",
                        "account":{
                            "data":{
                                "program":"spl-token",
                                "parsed":{},
                                "space":165
                            },
                            "executable":false,
                            "lamports":2039280,
                            "owner":"TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA",
                            "rentEpoch":1
                        }
                    }]
                }
            }));
        })
        .await;

    let get_token_balance = rpc
        .mock_async(|when, then| {
            when.method(POST)
                .path("/")
                .body_includes("\"method\":\"getTokenAccountBalance\"")
                .body_includes("9f1MFK8nQ7kkh2YkSK36D6cvn18PkEhGj4N8rn4vQ6iX");
            then.status(200).json_body(json!({
                "jsonrpc":"2.0",
                "id":1,
                "result":{
                    "context":{"slot":1},
                    "value":{
                        "amount":"10000",
                        "decimals":6,
                        "uiAmountString":"0.01"
                    }
                }
            }));
        })
        .await;

    let tool = SolanaTool::new();
    tool.configure(&json!({
        "tools": {
            "settings": {
                "solana": {
                    "rpc": {
                        "provider": "custom",
                        "endpoint": rpc.base_url()
                    }
                }
            }
        }
    }))
    .expect("configure solana tool");

    let balance = tool
        .execute(json!({
            "action": "balance",
            "address": "CvkK9CeYhhh1Vtkw6WZQkS8wGmmZsmZMcaXssD8pKZts",
            "mint": "EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v"
        }))
        .await
        .expect("token balance should work");

    assert_eq!(balance["status"], json!("ok"));
    assert_eq!(
        balance["mint"],
        json!("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v")
    );
    assert_eq!(balance["amount_atomic"], json!("10000"));
    assert_eq!(balance["decimals"], json!(6));
    assert_eq!(balance["ui_amount_string"], json!("0.01"));

    get_token_accounts.assert_calls(1);
    get_token_balance.assert_calls(1);
}
