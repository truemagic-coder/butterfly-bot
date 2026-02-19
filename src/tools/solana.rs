use std::time::{SystemTime, UNIX_EPOCH};

use async_trait::async_trait;
use serde_json::{json, Value};
use tokio::sync::RwLock;

use crate::error::{ButterflyBotError, Result};
use crate::interfaces::plugins::Tool;
use crate::security::solana_rpc_policy::{SolanaRpcExecutionPolicy, SolanaRpcProvider};

pub struct SolanaTool {
    policy: RwLock<SolanaRpcExecutionPolicy>,
}

impl Default for SolanaTool {
    fn default() -> Self {
        Self::new()
    }
}

impl SolanaTool {
    pub fn new() -> Self {
        Self {
            policy: RwLock::new(SolanaRpcExecutionPolicy::default_for_provider(
                SolanaRpcProvider::QuickNode,
            )),
        }
    }

    async fn configured_policy(&self) -> SolanaRpcExecutionPolicy {
        self.policy.read().await.clone()
    }

    fn require_endpoint(policy: &SolanaRpcExecutionPolicy) -> Result<String> {
        let endpoint = policy.endpoint.as_deref().unwrap_or("").trim();
        if endpoint.is_empty() {
            return Err(ButterflyBotError::Config(
                "tools.settings.solana.rpc.endpoint must be configured for Solana RPC".to_string(),
            ));
        }
        Ok(endpoint.to_string())
    }

    fn resolve_query_or_wallet_address(
        address: Option<&str>,
        user_id: Option<&str>,
        actor: Option<&str>,
        default_actor: &str,
    ) -> Result<String> {
        if let Some(address) = address.map(str::trim).filter(|v| !v.is_empty()) {
            return Ok(address.to_string());
        }

        let user_id = user_id
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .ok_or_else(|| {
                ButterflyBotError::Runtime(
                    "user_id is required when address is not provided".to_string(),
                )
            })?;
        let actor = actor
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .unwrap_or(default_actor);
        crate::security::solana_signer::wallet_address(user_id, actor)
    }

    fn now_ts() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64
    }

    fn sol_to_lamports(sol: f64) -> Option<u64> {
        if !sol.is_finite() || sol < 0.0 {
            return None;
        }
        let lamports = (sol * 1_000_000_000f64).round();
        if !lamports.is_finite() || lamports < 0.0 {
            return None;
        }
        Some(lamports as u64)
    }

    fn parse_string_amount(raw: &str) -> Option<u64> {
        let compact = raw.trim().to_ascii_lowercase().replace(' ', "");
        if compact.is_empty() {
            return None;
        }
        if let Some(value) = compact.strip_suffix("lamports") {
            return value.parse::<u64>().ok();
        }
        if let Some(value) = compact.strip_suffix("lamport") {
            return value.parse::<u64>().ok();
        }
        if let Some(value) = compact.strip_suffix("sol") {
            return value.parse::<f64>().ok().and_then(Self::sol_to_lamports);
        }
        compact.parse::<f64>().ok().and_then(Self::sol_to_lamports)
    }

    fn resolve_lamports(params: &Value) -> Option<u64> {
        if let Some(sol_alias) = params
            .get("amount_sol")
            .or_else(|| params.get("sol"))
            .or_else(|| params.get("amount_in_sol"))
        {
            match sol_alias {
                Value::Number(number) => return number.as_f64().and_then(Self::sol_to_lamports),
                Value::String(text) => return Self::parse_string_amount(text),
                _ => {}
            }
        }

        if let Some(lamports) = params.get("lamports").and_then(|v| v.as_u64()) {
            return Some(lamports);
        }

        if let Some(amount) = params.get("amount") {
            return match amount {
                Value::Number(number) => {
                    if let Some(as_u64) = number.as_u64() {
                        Some(as_u64)
                    } else {
                        number.as_f64().and_then(Self::sol_to_lamports)
                    }
                }
                Value::String(text) => Self::parse_string_amount(text),
                _ => None,
            };
        }

        None
    }
}

#[async_trait]
impl Tool for SolanaTool {
    fn name(&self) -> &str {
        "solana"
    }

    fn description(&self) -> &str {
        "Solana wallet operations: get wallet address, get balance, simulate transfers, submit transfers, and inspect transaction status/history."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": ["wallet", "balance", "transfer", "tx_status", "tx_history"]
                },
                "request_id": { "type": "string" },
                "user_id": { "type": "string" },
                "actor": { "type": "string", "description": "Defaults to agent" },
                "address": { "type": "string" },
                "to": { "type": "string", "description": "Destination Solana address" },
                "lamports": { "type": "integer" },
                "amount_sol": { "type": "number", "description": "SOL amount (preferred over lamports when provided)" },
                "amount": { "description": "Amount alias; integer = lamports, decimal/string = SOL" },
                "signature": { "type": "string" },
                "limit": { "type": "integer" }
            },
            "required": ["action"]
        })
    }

    fn configure(&self, config: &Value) -> Result<()> {
        let tools = config.get("tools").cloned().unwrap_or_else(|| json!({}));
        let policy = SolanaRpcExecutionPolicy::from_tools(&tools)?;
        let mut guard = self
            .policy
            .try_write()
            .map_err(|_| ButterflyBotError::Runtime("Solana tool lock busy".to_string()))?;
        *guard = policy;
        Ok(())
    }

    async fn execute(&self, params: Value) -> Result<Value> {
        let action = params
            .get("action")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let action = match action.as_str() {
            "address" | "get_wallet" => "wallet",
            "get_balance" => "balance",
            "send" | "send_transfer" => "transfer",
            "simulate" | "dry_run" => "simulate_transfer",
            "status" | "signature_status" => "tx_status",
            "history" => "tx_history",
            other => other,
        };

        let policy = self.configured_policy().await;

        match action {
            "wallet" => {
                let user_id = params
                    .get("user_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ButterflyBotError::Runtime("Missing user_id".to_string()))?;
                let actor = params
                    .get("actor")
                    .and_then(|v| v.as_str())
                    .filter(|v| !v.trim().is_empty())
                    .unwrap_or("agent");
                let address = crate::security::solana_signer::wallet_address(user_id, actor)?;
                Ok(json!({
                    "status": "ok",
                    "user_id": user_id,
                    "actor": actor,
                    "address": address
                }))
            }
            "balance" => {
                let endpoint = Self::require_endpoint(&policy)?;
                let address = Self::resolve_query_or_wallet_address(
                    params.get("address").and_then(|v| v.as_str()),
                    params.get("user_id").and_then(|v| v.as_str()),
                    params.get("actor").and_then(|v| v.as_str()),
                    "agent",
                )?;

                let lamports =
                    crate::solana_rpc::get_balance(&endpoint, &address, &policy.commitment).await?;
                Ok(json!({
                    "status": "ok",
                    "address": address,
                    "lamports": lamports,
                    "sol": lamports as f64 / 1_000_000_000f64
                }))
            }
            "transfer" | "simulate_transfer" => {
                let endpoint = Self::require_endpoint(&policy)?;
                let user_id = params
                    .get("user_id")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ButterflyBotError::Runtime("Missing user_id".to_string()))?;
                let actor = params
                    .get("actor")
                    .and_then(|v| v.as_str())
                    .filter(|v| !v.trim().is_empty())
                    .unwrap_or("agent");
                let to = params
                    .get("to")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ButterflyBotError::Runtime("Missing to".to_string()))?;
                let lamports = Self::resolve_lamports(&params)
                    .ok_or_else(|| ButterflyBotError::Runtime("Missing lamports".to_string()))?;

                let request_id = params
                    .get("request_id")
                    .and_then(|v| v.as_str())
                    .map(|v| v.to_string())
                    .unwrap_or_else(|| format!("sol-{}", Self::now_ts()));

                let from_seed = crate::security::solana_signer::signing_seed(user_id, actor)?;
                let latest_blockhash =
                    crate::solana_rpc::get_latest_blockhash(&endpoint, &policy.commitment).await?;

                let simulate_only = action == "simulate_transfer"
                    || params
                        .get("simulate_only")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false);

                let (tx_base64, wallet_address, simulation_result) = if policy.simulation.enabled {
                    let probe_unit_limit = crate::solana_rpc::probe_compute_unit_limit(&policy);
                    let (probe_tx_base64, wallet_address) =
                        crate::solana_rpc::build_transfer_transaction_base64_with_unit_limit(
                            &from_seed,
                            to,
                            lamports,
                            &latest_blockhash,
                            &policy,
                            probe_unit_limit,
                        )?;

                    let simulation = crate::solana_rpc::simulate_transaction(
                        &endpoint,
                        &probe_tx_base64,
                        &policy,
                    )
                    .await?;
                    let adjusted_unit_limit = crate::solana_rpc::recommended_compute_unit_limit(
                        &simulation,
                        policy.compute_budget.unit_limit,
                    );

                    let tx_base64 = if adjusted_unit_limit == probe_unit_limit {
                        probe_tx_base64
                    } else {
                        crate::solana_rpc::build_transfer_transaction_base64_with_unit_limit(
                            &from_seed,
                            to,
                            lamports,
                            &latest_blockhash,
                            &policy,
                            adjusted_unit_limit,
                        )?
                        .0
                    };
                    (tx_base64, wallet_address, Some(simulation))
                } else {
                    let (tx_base64, wallet_address) =
                        crate::solana_rpc::build_transfer_transaction_base64(
                            &from_seed,
                            to,
                            lamports,
                            &latest_blockhash,
                            &policy,
                        )?;
                    (tx_base64, wallet_address, None)
                };

                if simulate_only {
                    return Ok(json!({
                        "status": "simulated",
                        "request_id": request_id,
                        "wallet_address": wallet_address,
                        "simulation": simulation_result,
                        "signature": Value::Null
                    }));
                }

                let signature =
                    crate::solana_rpc::send_transaction(&endpoint, &tx_base64, &policy).await?;

                Ok(json!({
                    "status": "submitted",
                    "request_id": request_id,
                    "wallet_address": wallet_address,
                    "simulated_before_send": simulation_result.is_some(),
                    "signature": signature
                }))
            }
            "tx_status" => {
                let endpoint = Self::require_endpoint(&policy)?;
                let signature = params
                    .get("signature")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| ButterflyBotError::Runtime("Missing signature".to_string()))?;
                let value = crate::solana_rpc::get_signature_status(&endpoint, signature).await?;
                Ok(json!({
                    "status": "ok",
                    "signature": signature,
                    "value": value
                }))
            }
            "tx_history" => {
                let endpoint = Self::require_endpoint(&policy)?;
                let address = Self::resolve_query_or_wallet_address(
                    params.get("address").and_then(|v| v.as_str()),
                    params.get("user_id").and_then(|v| v.as_str()),
                    params.get("actor").and_then(|v| v.as_str()),
                    "agent",
                )?;
                let limit = params.get("limit").and_then(|v| v.as_u64()).unwrap_or(20) as usize;
                let entries =
                    crate::solana_rpc::get_signatures_for_address(&endpoint, &address, limit)
                        .await?;
                Ok(json!({
                    "status": "ok",
                    "address": address,
                    "entries": entries
                }))
            }
            _ => Err(ButterflyBotError::Runtime("Unsupported action".to_string())),
        }
    }
}
