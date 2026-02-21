use std::mem;

use serde_json::{json, Map, Value};

fn tool_name() -> &'static str {
    if cfg!(feature = "tool_coding") {
        "coding"
    } else if cfg!(feature = "tool_mcp") {
        "mcp"
    } else if cfg!(feature = "tool_http_call") {
        "http_call"
    } else if cfg!(feature = "tool_github") {
        "github"
    } else if cfg!(feature = "tool_zapier") {
        "zapier"
    } else if cfg!(feature = "tool_planning") {
        "planning"
    } else if cfg!(feature = "tool_reminders") {
        "reminders"
    } else if cfg!(feature = "tool_search_internet") {
        "search_internet"
    } else if cfg!(feature = "tool_solana") {
        "solana"
    } else if cfg!(feature = "tool_tasks") {
        "tasks"
    } else if cfg!(feature = "tool_todo") {
        "todo"
    } else if cfg!(feature = "tool_wakeup") {
        "wakeup"
    } else {
        "unknown"
    }
}

fn execute_for_tool(tool: &str, input: &Value) -> Value {
    match tool {
        "todo" => execute_todo(input),
        "tasks" => execute_tasks(input),
        "reminders" => execute_reminders(input),
        "planning" => execute_planning(input),
        "wakeup" => execute_wakeup(input),
        "coding" => execute_coding(input),
        "mcp" => execute_mcp(input),
        "http_call" => execute_http_call(input),
        "github" => execute_github(input),
        "zapier" => execute_zapier(input),
        "search_internet" => execute_search_internet(input),
        "solana" => execute_solana(input),
        _ => json!({
            "status": "error",
            "code": "internal",
            "error": format!("tool '{}' wasm implementation not yet complete", tool)
        }),
    }
}

fn capability_call(name: &str, args: Value) -> Value {
    json!({
        "status": "capability_call",
        "abi_version": 1,
        "capability_call": {
            "name": name,
            "args": args
        }
    })
}

fn invalid_args(message: &str) -> Value {
    json!({
        "status": "error",
        "code": "invalid_args",
        "error": message,
    })
}

fn input_object(input: &Value) -> Result<Map<String, Value>, Value> {
    input
        .as_object()
        .cloned()
        .ok_or_else(|| invalid_args("Input must be a JSON object"))
}

fn require_string(args: &Map<String, Value>, key: &str) -> Result<(), Value> {
    if args.get(key).and_then(|value| value.as_str()).is_some() {
        Ok(())
    } else {
        Err(invalid_args(&format!("Missing {}", key)))
    }
}

fn require_i64(args: &Map<String, Value>, key: &str) -> Result<(), Value> {
    if args.get(key).and_then(|value| value.as_i64()).is_some() {
        Ok(())
    } else {
        Err(invalid_args(&format!("Missing {}", key)))
    }
}

fn require_u64(args: &Map<String, Value>, key: &str) -> Result<(), Value> {
    if args.get(key).and_then(|value| value.as_u64()).is_some() {
        Ok(())
    } else {
        Err(invalid_args(&format!("Missing {}", key)))
    }
}

fn execute_todo(input: &Value) -> Value {
    let mut args = match input_object(input) {
        Ok(args) => args,
        Err(err) => return err,
    };

    if let Err(err) = require_string(&args, "user_id") {
        return err;
    }

    let raw_action = args
        .get("action")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    let action = match raw_action.as_str() {
        "add" | "new" => "create",
        "create_list" | "create_many" | "add_many" | "bulk_create" | "create_items" => {
            "create_many"
        }
        "clear_all" | "delete_all" | "remove_all" | "wipe" | "clean" => "clear",
        other => other,
    };
    args.insert("action".to_string(), Value::String(action.to_string()));

    let valid = match action {
        "create" => require_string(&args, "title"),
        "create_many" => {
            let has_items = args
                .get("items")
                .and_then(|value| value.as_array())
                .map(|items| !items.is_empty())
                .unwrap_or(false);
            if has_items {
                Ok(())
            } else {
                Err(invalid_args("Missing items"))
            }
        }
        "complete" | "reopen" | "delete" => require_i64(&args, "id"),
        "reorder" => {
            let has_ids = args
                .get("ordered_ids")
                .and_then(|value| value.as_array())
                .map(|ids| !ids.is_empty())
                .unwrap_or(false);
            if has_ids {
                Ok(())
            } else {
                Err(invalid_args("Missing ordered_ids"))
            }
        }
        "list" | "clear" => Ok(()),
        _ => Err(invalid_args("Unsupported action")),
    };

    if let Err(err) = valid {
        return err;
    }

    let capability = match action {
        "create" => "kv.sqlite.todo.create",
        "create_many" => "kv.sqlite.todo.create_many",
        "list" => "kv.sqlite.todo.list",
        "complete" => "kv.sqlite.todo.complete",
        "reopen" => "kv.sqlite.todo.reopen",
        "delete" => "kv.sqlite.todo.delete",
        "clear" => "kv.sqlite.todo.clear",
        "reorder" => "kv.sqlite.todo.reorder",
        _ => return invalid_args("Unsupported action"),
    };

    capability_call(capability, Value::Object(args))
}

fn execute_tasks(input: &Value) -> Value {
    let mut args = match input_object(input) {
        Ok(args) => args,
        Err(err) => return err,
    };

    if let Err(err) = require_string(&args, "user_id") {
        return err;
    }

    let action = args
        .get("action")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    let action = match action.as_str() {
        "cancel" => "disable".to_string(),
        "clear_all" | "delete_all" | "remove_all" | "wipe" | "clean" => "clear".to_string(),
        other => other.to_string(),
    };
    args.insert("action".to_string(), Value::String(action.clone()));

    let valid = match action.as_str() {
        "schedule" => {
            require_string(&args, "name")
                .and_then(|_| require_string(&args, "prompt"))
                .and_then(|_| require_i64(&args, "run_at"))
        }
        "cancel" | "disable" | "enable" | "delete" => require_i64(&args, "id"),
        "list" | "clear" => Ok(()),
        _ => Err(invalid_args("Unsupported action")),
    };

    if let Err(err) = valid {
        return err;
    }

    let capability = match action.as_str() {
        "schedule" => "kv.sqlite.tasks.schedule",
        "list" => "kv.sqlite.tasks.list",
        "enable" => "kv.sqlite.tasks.enable",
        "disable" => "kv.sqlite.tasks.disable",
        "delete" => "kv.sqlite.tasks.delete",
        "clear" => "kv.sqlite.tasks.clear",
        _ => return invalid_args("Unsupported action"),
    };

    capability_call(capability, Value::Object(args))
}

fn execute_reminders(input: &Value) -> Value {
    let mut args = match input_object(input) {
        Ok(args) => args,
        Err(err) => return err,
    };

    if let Err(err) = require_string(&args, "user_id") {
        return err;
    }

    let raw_action = args
        .get("action")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    let action = match raw_action.as_str() {
        "set" | "add" | "remind" | "schedule" | "create_reminder" => "create",
        "show" | "list_reminders" => "list",
        "done" | "finish" => "complete",
        "remove" | "erase" => "delete",
        "clear" | "clear_all" | "clear_reminders" => "clear",
        other => other,
    };
    args.insert("action".to_string(), Value::String(action.to_string()));

    let valid = match action {
        "create" => require_string(&args, "title"),
        "complete" | "delete" => require_i64(&args, "id"),
        "snooze" => {
            require_i64(&args, "id").and_then(|_| {
                let has_due = args
                    .get("delay_seconds")
                    .and_then(|value| value.as_i64())
                    .is_some()
                    || args
                        .get("in_seconds")
                        .and_then(|value| value.as_i64())
                        .is_some()
                    || args
                        .get("due_at")
                        .and_then(|value| value.as_i64())
                        .is_some();
                if has_due {
                    Ok(())
                } else {
                    Err(invalid_args("Missing due_at or delay_seconds"))
                }
            })
        }
        "list" | "clear" => Ok(()),
        _ => Err(invalid_args("Unsupported action")),
    };

    if let Err(err) = valid {
        return err;
    }

    let capability = match action {
        "create" => "kv.sqlite.reminders.create",
        "list" => "kv.sqlite.reminders.list",
        "complete" => "kv.sqlite.reminders.complete",
        "delete" => "kv.sqlite.reminders.delete",
        "snooze" => "kv.sqlite.reminders.snooze",
        "clear" => "kv.sqlite.reminders.clear",
        _ => return invalid_args("Unsupported action"),
    };

    capability_call(capability, Value::Object(args))
}

fn execute_planning(input: &Value) -> Value {
    let args = match input_object(input) {
        Ok(args) => args,
        Err(err) => return err,
    };

    if let Err(err) = require_string(&args, "user_id") {
        return err;
    }

    let raw_action = args
        .get("action")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();
    let action = match raw_action.as_str() {
        "clear_all" | "delete_all" | "remove_all" | "wipe" | "clean" => "clear".to_string(),
        other => other.to_string(),
    };
    let mut args = args;
    args.insert("action".to_string(), Value::String(action.clone()));

    let valid = match action.as_str() {
        "create" => require_string(&args, "title").and_then(|_| require_string(&args, "goal")),
        "get" | "update" | "delete" => require_i64(&args, "id"),
        "list" | "clear" => Ok(()),
        _ => Err(invalid_args("Unsupported action")),
    };

    if let Err(err) = valid {
        return err;
    }

    let capability = match action.as_str() {
        "create" => "kv.sqlite.planning.create",
        "list" => "kv.sqlite.planning.list",
        "get" => "kv.sqlite.planning.get",
        "update" => "kv.sqlite.planning.update",
        "delete" => "kv.sqlite.planning.delete",
        "clear" => "kv.sqlite.planning.clear",
        _ => return invalid_args("Unsupported action"),
    };

    capability_call(capability, Value::Object(args))
}

fn execute_wakeup(input: &Value) -> Value {
    let args = match input_object(input) {
        Ok(args) => args,
        Err(err) => return err,
    };

    if let Err(err) = require_string(&args, "user_id") {
        return err;
    }

    let action = args
        .get("action")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();

    let valid = match action.as_str() {
        "create" => {
            require_string(&args, "name")
                .and_then(|_| require_string(&args, "prompt"))
                .and_then(|_| require_i64(&args, "interval_minutes"))
        }
        "enable" | "disable" | "delete" => require_i64(&args, "id"),
        "list" => Ok(()),
        _ => Err(invalid_args("Unsupported action")),
    };

    if let Err(err) = valid {
        return err;
    }

    let capability = match action.as_str() {
        "create" => "kv.sqlite.wakeup.create",
        "list" => "kv.sqlite.wakeup.list",
        "enable" => "kv.sqlite.wakeup.enable",
        "disable" => "kv.sqlite.wakeup.disable",
        "delete" => "kv.sqlite.wakeup.delete",
        _ => return invalid_args("Unsupported action"),
    };

    capability_call(capability, Value::Object(args))
}

fn execute_coding(input: &Value) -> Value {
    let args = match input_object(input) {
        Ok(args) => args,
        Err(err) => return err,
    };

    if let Err(err) = require_string(&args, "prompt") {
        return err;
    }

    capability_call("coding.generate", Value::Object(args))
}

fn execute_http_call(input: &Value) -> Value {
    let args = match input_object(input) {
        Ok(args) => args,
        Err(err) => return err,
    };

    if let Err(err) = require_string(&args, "method") {
        return err;
    }

    capability_call("http.request", Value::Object(args))
}

fn execute_mcp(input: &Value) -> Value {
    let args = match input_object(input) {
        Ok(args) => args,
        Err(err) => return err,
    };

    let action = args
        .get("action")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();

    let capability = match action.as_str() {
        "list_tools" => "mcp.list_tools",
        "call_tool" => {
            if let Err(err) = require_string(&args, "tool") {
                return err;
            }
            "mcp.call"
        }
        _ => return invalid_args("Unsupported action"),
    };

    capability_call(capability, Value::Object(args))
}

fn execute_github(input: &Value) -> Value {
    let args = match input_object(input) {
        Ok(args) => args,
        Err(err) => return err,
    };

    let action = args
        .get("action")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();

    let capability = match action.as_str() {
        "list_tools" => "github.list_tools",
        "call_tool" => {
            if let Err(err) = require_string(&args, "tool") {
                return err;
            }
            "github.call_tool"
        }
        _ => return invalid_args("Unsupported action"),
    };

    capability_call(capability, Value::Object(args))
}

fn execute_zapier(input: &Value) -> Value {
    let args = match input_object(input) {
        Ok(args) => args,
        Err(err) => return err,
    };

    let action = args
        .get("action")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string();

    let capability = match action.as_str() {
        "list_tools" => "zapier.list_tools",
        "call_tool" => {
            if let Err(err) = require_string(&args, "tool") {
                return err;
            }
            "zapier.call_tool"
        }
        _ => return invalid_args("Unsupported action"),
    };

    capability_call(capability, Value::Object(args))
}

fn execute_search_internet(input: &Value) -> Value {
    let args = match input_object(input) {
        Ok(args) => args,
        Err(err) => return err,
    };

    if let Err(err) = require_string(&args, "query") {
        return err;
    }

    capability_call("search.internet", Value::Object(args))
}

fn sol_to_lamports(amount_sol: f64) -> Option<u64> {
    if !amount_sol.is_finite() || amount_sol < 0.0 {
        return None;
    }
    let lamports = (amount_sol * 1_000_000_000f64).round();
    if !lamports.is_finite() || lamports < 0.0 {
        return None;
    }
    Some(lamports as u64)
}

fn parse_amount_string_to_lamports(raw: &str) -> Option<u64> {
    let normalized = raw.trim().to_ascii_lowercase();
    if normalized.is_empty() {
        return None;
    }

    let compact = normalized.replace(' ', "");
    if let Some(value) = compact.strip_suffix("lamports") {
        return value.parse::<u64>().ok();
    }
    if let Some(value) = compact.strip_suffix("lamport") {
        return value.parse::<u64>().ok();
    }
    if let Some(value) = compact.strip_suffix("sol") {
        return value.parse::<f64>().ok().and_then(sol_to_lamports);
    }

    compact
        .parse::<f64>()
        .ok()
        .and_then(sol_to_lamports)
}

fn execute_solana(input: &Value) -> Value {
    let mut args = match input_object(input) {
        Ok(args) => args,
        Err(err) => return err,
    };

    if args
        .get("to")
        .and_then(|value| value.as_str())
        .is_none()
    {
        let to_alias = args
            .get("recipient")
            .or_else(|| args.get("address_to"))
            .or_else(|| args.get("to_address"))
            .or_else(|| args.get("destination"))
            .cloned();
        if let Some(to_value) = to_alias {
            args.insert("to".to_string(), to_value);
        }
    }

    if args
        .get("address")
        .and_then(|value| value.as_str())
        .is_none()
    {
        if let Some(address) = args.get("wallet_address").cloned() {
            args.insert("address".to_string(), address);
        }
    }

    if args
        .get("mint")
        .and_then(|value| value.as_str())
        .is_none()
    {
        if let Some(mint) = args
            .get("token_mint")
            .or_else(|| args.get("asset"))
            .or_else(|| args.get("asset_id"))
            .cloned()
        {
            args.insert("mint".to_string(), mint);
        }
    }

    if args
        .get("amount_atomic")
        .and_then(|value| value.as_u64())
        .is_none()
    {
        if let Some(amount_atomic) = args
            .get("token_amount_atomic")
            .or_else(|| args.get("amount_atomic_units"))
            .cloned()
        {
            args.insert("amount_atomic".to_string(), amount_atomic);
        } else if args
            .get("mint")
            .and_then(|value| value.as_str())
            .map(|value| !value.trim().is_empty())
            .unwrap_or(false)
        {
            if let Some(amount) = args.get("amount") {
                match amount {
                    Value::Number(number) if number.as_u64().is_some() => {
                        args.insert("amount_atomic".to_string(), amount.clone());
                    }
                    Value::String(text) => {
                        if let Ok(parsed) = text.trim().parse::<u64>() {
                            args.insert("amount_atomic".to_string(), Value::from(parsed));
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    let amount_sol_alias = args
        .get("amount_sol")
        .or_else(|| args.get("sol"))
        .or_else(|| args.get("amount_in_sol"));

    let mut normalized_lamports = amount_sol_alias.and_then(|value| match value {
        Value::Number(number) => number.as_f64().and_then(sol_to_lamports),
        Value::String(text) => parse_amount_string_to_lamports(text),
        _ => None,
    });

    if normalized_lamports.is_none() {
        normalized_lamports = args.get("amount").and_then(|value| match value {
            Value::Number(number) => {
                if let Some(value_u64) = number.as_u64() {
                    Some(value_u64)
                } else {
                    number.as_f64().and_then(sol_to_lamports)
                }
            }
            Value::String(text) => parse_amount_string_to_lamports(text),
            _ => None,
        });
    }

    if args.get("lamports").and_then(|value| value.as_u64()).is_none() {
        if let Some(lamports) = normalized_lamports {
            args.insert("lamports".to_string(), Value::from(lamports));
        }
    } else if amount_sol_alias.is_some() {
        if let Some(lamports) = normalized_lamports {
            args.insert("lamports".to_string(), Value::from(lamports));
        }
    }

    if args
        .get("user_id")
        .and_then(|value| value.as_str())
        .is_none()
    {
        args.insert("user_id".to_string(), Value::String("user".to_string()));
    }

    let raw_action = args
        .get("action")
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let action = match raw_action.as_str() {
        "address"
        | "get_wallet"
        | "inspect_wallet"
        | "check_wallet"
        | "wallet_address"
        | "get_wallet_address" => "wallet",
        "get_balance" | "inspect_balance" | "check_balance" | "wallet_balance" => "balance",
        "send"
        | "send_transfer"
        | "transact"
        | "transaction"
        | "pay"
        | "send_token"
        | "payment"
        | "execute_payment"
        | "submit_payment"
        | "x402_payment" => "transfer",
        "simulate" | "dry_run" | "simulate_payment" | "preview_payment" | "x402_preview" => {
            "simulate_transfer"
        }
        "simulate_transaction" | "simulate_tx" => "simulate_transfer",
        "status" | "signature_status" | "txstatus" | "check_tx" | "transaction_status" => {
            "tx_status"
        }
        "history" => "tx_history",
        "" => {
            if args
                .get("to")
                .and_then(|value| value.as_str())
                .map(|value| !value.trim().is_empty())
                .unwrap_or(false)
            {
                "transfer"
            } else {
                ""
            }
        }
        other => other,
    }
    .to_string();
    args.insert("action".to_string(), Value::String(action.clone()));

    let valid = match action.as_str() {
        "wallet" => require_string(&args, "user_id"),
        "balance" => {
            let has_address = args
                .get("address")
                .and_then(|value| value.as_str())
                .map(|value| !value.trim().is_empty())
                .unwrap_or(false);
            let has_user = args
                .get("user_id")
                .and_then(|value| value.as_str())
                .map(|value| !value.trim().is_empty())
                .unwrap_or(false);
            if has_address || has_user {
                Ok(())
            } else {
                Err(invalid_args("Missing address or user_id"))
            }
        }
        "transfer" | "simulate_transfer" => {
            require_string(&args, "user_id")
                .and_then(|_| require_string(&args, "to"))
                .and_then(|_| {
                    let has_lamports = args
                        .get("lamports")
                        .and_then(|value| value.as_u64())
                        .is_some();
                    let has_token_args = args
                        .get("mint")
                        .and_then(|value| value.as_str())
                        .map(|value| !value.trim().is_empty())
                        .unwrap_or(false)
                        && args
                            .get("amount_atomic")
                            .and_then(|value| value.as_u64())
                            .is_some();
                    if has_lamports || has_token_args {
                        Ok(())
                    } else {
                        Err(invalid_args(
                            "Missing lamports for SOL transfer or mint+amount_atomic for SPL transfer",
                        ))
                    }
                })
        }
        "tx_status" => require_string(&args, "signature"),
        "tx_history" => {
            let has_address = args
                .get("address")
                .and_then(|value| value.as_str())
                .map(|value| !value.trim().is_empty())
                .unwrap_or(false);
            let has_user = args
                .get("user_id")
                .and_then(|value| value.as_str())
                .map(|value| !value.trim().is_empty())
                .unwrap_or(false);
            if has_address || has_user {
                Ok(())
            } else {
                Err(invalid_args("Missing address or user_id"))
            }
        }
        _ => Err(invalid_args("Unsupported action")),
    };

    if let Err(err) = valid {
        return err;
    }

    let capability = match action.as_str() {
        "wallet" => "solana.wallet",
        "balance" => "solana.balance",
        "transfer" => "solana.transfer",
        "simulate_transfer" => "solana.simulate_transfer",
        "tx_status" => "solana.tx_status",
        "tx_history" => "solana.tx_history",
        _ => return invalid_args("Unsupported action"),
    };

    capability_call(capability, Value::Object(args))
}

#[no_mangle]
pub extern "C" fn alloc(len: i32) -> i32 {
    if len <= 0 {
        return 0;
    }
    let mut buf = vec![0u8; len as usize];
    let ptr = buf.as_mut_ptr();
    mem::forget(buf);
    ptr as i32
}

#[no_mangle]
pub extern "C" fn dealloc(ptr: i32, len: i32) {
    if ptr == 0 || len <= 0 {
        return;
    }
    unsafe {
        let _ = Vec::from_raw_parts(ptr as *mut u8, len as usize, len as usize);
    }
}

#[no_mangle]
pub extern "C" fn execute(input_ptr: i32, input_len: i32) -> i64 {
    let input = if input_ptr == 0 || input_len <= 0 {
        Value::Null
    } else {
        let bytes = unsafe {
            std::slice::from_raw_parts(input_ptr as *const u8, input_len as usize)
        };
        serde_json::from_slice(bytes).unwrap_or(Value::Null)
    };

    let output = execute_for_tool(tool_name(), &input);

    let mut bytes = serde_json::to_vec(&output).unwrap_or_else(|_| b"{}".to_vec());
    bytes.shrink_to_fit();
    let len = bytes.len() as u32;
    let ptr = bytes.as_mut_ptr() as u32;
    mem::forget(bytes);

    ((ptr as u64) << 32 | len as u64) as i64
}

#[cfg(test)]
mod tests {
    use super::execute_for_tool;
    use serde_json::json;

    #[test]
    fn todo_alias_is_normalized() {
        let output = execute_for_tool(
            "todo",
            &json!({"action":"add_many","user_id":"u1","items":["a"]}),
        );

        assert_eq!(output["status"].as_str(), Some("capability_call"));
        assert_eq!(
            output["capability_call"]["args"]["action"].as_str(),
            Some("create_many")
        );
    }

    #[test]
    fn todo_create_uses_capability_call() {
        let output = execute_for_tool(
            "todo",
            &json!({"action":"create","user_id":"u1","title":"t"}),
        );
        assert_eq!(output["status"].as_str(), Some("capability_call"));
        assert_eq!(
            output["capability_call"]["name"].as_str(),
            Some("kv.sqlite.todo.create")
        );
    }

    #[test]
    fn solana_transfer_uses_capability_call() {
        let output = execute_for_tool(
            "solana",
            &json!({"action":"transfer","user_id":"u1","to":"11111111111111111111111111111111","lamports":1}),
        );
        assert_eq!(output["status"].as_str(), Some("capability_call"));
        assert_eq!(
            output["capability_call"]["name"].as_str(),
            Some("solana.transfer")
        );
    }

    #[test]
    fn solana_transact_alias_with_address_to_and_amount_normalizes() {
        let output = execute_for_tool(
            "solana",
            &json!({
                "action":"transact",
                "address_to":"11111111111111111111111111111111",
                "amount": 20000000
            }),
        );
        assert_eq!(output["status"].as_str(), Some("capability_call"));
        assert_eq!(
            output["capability_call"]["name"].as_str(),
            Some("solana.transfer")
        );
        assert_eq!(
            output["capability_call"]["args"]["lamports"].as_u64(),
            Some(20000000)
        );
    }

    #[test]
    fn solana_execute_payment_alias_normalizes() {
        let output = execute_for_tool(
            "solana",
            &json!({
                "action":"execute_payment",
                "recipient":"11111111111111111111111111111111",
                "amount": 0.01
            }),
        );
        assert_eq!(output["status"].as_str(), Some("capability_call"));
        assert_eq!(
            output["capability_call"]["name"].as_str(),
            Some("solana.transfer")
        );
        assert_eq!(
            output["capability_call"]["args"]["lamports"].as_u64(),
            Some(10_000_000)
        );
    }

    #[test]
    fn solana_missing_action_with_recipient_and_sol_amount_normalizes() {
        let output = execute_for_tool(
            "solana",
            &json!({
                "recipient":"11111111111111111111111111111111",
                "amount": 0.02,
                "kind":"transfer",
                "type":"SolanaTransaction"
            }),
        );
        assert_eq!(output["status"].as_str(), Some("capability_call"));
        assert_eq!(
            output["capability_call"]["name"].as_str(),
            Some("solana.transfer")
        );
        assert_eq!(
            output["capability_call"]["args"]["lamports"].as_u64(),
            Some(20_000_000)
        );
        assert_eq!(
            output["capability_call"]["args"]["user_id"].as_str(),
            Some("user")
        );
    }

    #[test]
    fn todo_missing_user_id_is_invalid_args() {
        let output = execute_for_tool("todo", &json!({"action":"list"}));
        assert_eq!(output["status"].as_str(), Some("error"));
        assert_eq!(output["code"].as_str(), Some("invalid_args"));
    }

    #[test]
    fn tasks_schedule_requires_run_at() {
        let output = execute_for_tool(
            "tasks",
            &json!({"action":"schedule","user_id":"u1","name":"n","prompt":"p"}),
        );
        assert_eq!(output["status"].as_str(), Some("error"));
        assert_eq!(output["code"].as_str(), Some("invalid_args"));
    }

    #[test]
    fn reminders_alias_and_snooze_validation() {
        let output = execute_for_tool(
            "reminders",
            &json!({"action":"schedule","user_id":"u1","title":"t"}),
        );
        assert_eq!(
            output["capability_call"]["args"]["action"].as_str(),
            Some("create")
        );

        let invalid = execute_for_tool("reminders", &json!({"action":"snooze","user_id":"u1","id":1}));
        assert_eq!(invalid["status"].as_str(), Some("error"));
    }

    #[test]
    fn planning_create_requires_goal() {
        let output = execute_for_tool(
            "planning",
            &json!({"action":"create","user_id":"u1","title":"t"}),
        );
        assert_eq!(output["status"].as_str(), Some("error"));
    }

    #[test]
    fn wakeup_create_requires_interval() {
        let output = execute_for_tool(
            "wakeup",
            &json!({"action":"create","user_id":"u1","name":"n","prompt":"p"}),
        );
        assert_eq!(output["status"].as_str(), Some("error"));
    }

    #[test]
    fn p2_tools_use_capability_calls() {
        let coding = execute_for_tool("coding", &json!({"prompt":"hi"}));
        assert_eq!(coding["status"].as_str(), Some("capability_call"));
        assert_eq!(coding["capability_call"]["name"], "coding.generate");

        let github = execute_for_tool("github", &json!({"action":"list_tools"}));
        assert_eq!(github["status"].as_str(), Some("capability_call"));
        assert_eq!(github["capability_call"]["name"], "github.list_tools");

        let zapier = execute_for_tool("zapier", &json!({"action":"list_tools"}));
        assert_eq!(zapier["status"].as_str(), Some("capability_call"));
        assert_eq!(zapier["capability_call"]["name"], "zapier.list_tools");

        let search = execute_for_tool("search_internet", &json!({"query":"rust"}));
        assert_eq!(search["status"].as_str(), Some("capability_call"));
        assert_eq!(search["capability_call"]["name"], "search.internet");
    }

    #[test]
    fn p2_invalid_args_are_rejected() {
        let mcp_missing_tool = execute_for_tool("mcp", &json!({"action":"call_tool"}));
        assert_eq!(mcp_missing_tool["status"].as_str(), Some("error"));
        assert_eq!(mcp_missing_tool["code"].as_str(), Some("invalid_args"));

        let github_unsupported = execute_for_tool("github", &json!({"action":"search"}));
        assert_eq!(github_unsupported["status"].as_str(), Some("error"));
        assert_eq!(github_unsupported["code"].as_str(), Some("invalid_args"));

        let zapier_unsupported = execute_for_tool("zapier", &json!({"action":"search"}));
        assert_eq!(zapier_unsupported["status"].as_str(), Some("error"));
        assert_eq!(zapier_unsupported["code"].as_str(), Some("invalid_args"));

        let http_missing_method = execute_for_tool("http_call", &json!({"url":"https://example.com"}));
        assert_eq!(http_missing_method["status"].as_str(), Some("error"));
        assert_eq!(http_missing_method["code"].as_str(), Some("invalid_args"));
    }

    #[test]
    fn solana_amount_string_with_sol_unit_normalizes() {
        let output = execute_for_tool(
            "solana",
            &json!({
                "action":"transfer",
                "user_id":"u1",
                "recipient":"11111111111111111111111111111111",
                "amount":"0.02 SOL"
            }),
        );
        assert_eq!(
            output["capability_call"]["name"].as_str(),
            Some("solana.transfer")
        );
        assert_eq!(
            output["capability_call"]["args"]["lamports"].as_u64(),
            Some(20_000_000)
        );
    }

    #[test]
    fn solana_amount_sol_overrides_bad_lamports() {
        let output = execute_for_tool(
            "solana",
            &json!({
                "action":"transfer",
                "user_id":"u1",
                "to":"11111111111111111111111111111111",
                "amount_sol":0.02,
                "lamports":2_000_000
            }),
        );
        assert_eq!(
            output["capability_call"]["name"].as_str(),
            Some("solana.transfer")
        );
        assert_eq!(
            output["capability_call"]["args"]["lamports"].as_u64(),
            Some(20_000_000)
        );
    }

    #[test]
    fn no_tool_returns_host_call_envelope() {
        let cases = [
            ("todo", json!({"action":"list","user_id":"u1"})),
            ("tasks", json!({"action":"list","user_id":"u1"})),
            ("reminders", json!({"action":"list","user_id":"u1"})),
            ("planning", json!({"action":"list","user_id":"u1"})),
            ("wakeup", json!({"action":"list","user_id":"u1"})),
            ("coding", json!({"prompt":"hello"})),
            ("mcp", json!({"action":"list_tools"})),
            ("http_call", json!({"method":"GET"})),
            ("github", json!({"action":"list_tools"})),
            ("zapier", json!({"action":"list_tools"})),
            ("search_internet", json!({"query":"rust"})),
        ];

        for (tool, input) in cases {
            let output = execute_for_tool(tool, &input);
            assert_ne!(
                output["status"].as_str(),
                Some("host_call"),
                "tool '{}' unexpectedly emitted host_call envelope",
                tool
            );
        }
    }
}
