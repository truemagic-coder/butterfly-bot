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

    fn resolve_token_mint(params: &Value) -> Option<String> {
        params
            .get("mint")
            .or_else(|| params.get("token_mint"))
            .or_else(|| params.get("asset"))
            .or_else(|| params.get("asset_id"))
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.to_string())
    }

    fn resolve_amount_atomic(params: &Value) -> Option<u64> {
        if let Some(value) = params
            .get("amount_atomic")
            .or_else(|| params.get("token_amount_atomic"))
        {
            return match value {
                Value::Number(number) => number.as_u64(),
                Value::String(text) => text.trim().parse::<u64>().ok(),
                _ => None,
            };
        }

        if let Some(value) = params.get("amount") {
            return match value {
                Value::Number(number) => number.as_u64(),
                Value::String(text) => text.trim().parse::<u64>().ok(),
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
        "Solana wallet operations: get wallet address, SOL balance, SPL-token balance by mint, simulate/submit SOL transfers (lamports), simulate/submit SPL-token transfers by mint+amount_atomic, and inspect transaction status/history."
    }

    fn parameters(&self) -> Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": [
                        "wallet",
                        "address",
                        "get_wallet",
                        "balance",
                        "get_balance",
                        "transfer",
                        "send",
                        "send_transfer",
                        "simulate_transfer",
                        "simulate",
                        "dry_run",
                        "tx_status",
                        "status",
                        "signature_status",
                        "tx_history",
                        "history"
                    ]
                },
                "request_id": { "type": "string" },
                "user_id": { "type": "string" },
                "actor": { "type": "string", "description": "Defaults to agent" },
                "address": { "type": "string" },
                "to": { "type": "string", "description": "Destination Solana address" },
                "mint": { "type": "string", "description": "SPL token mint address (required for token balance/transfer)" },
                "from_token_account": { "type": "string" },
                "to_token_account": { "type": "string" },
                "amount_atomic": { "type": "integer", "description": "Atomic units for SPL-token transfer" },
                "decimals": { "type": "integer", "description": "Optional mint decimals override" },
                "lamports": { "type": "integer" },
                "amount_sol": { "type": "number", "description": "SOL amount (preferred over lamports when provided)" },
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
            .to_ascii_lowercase();
        let action = match action.as_str() {
            "address" | "get_wallet" | "inspect_wallet" | "check_wallet" | "wallet_address"
            | "get_wallet_address" => "wallet",
            "get_balance" | "inspect_balance" | "check_balance" | "wallet_balance" => "balance",
            "send" | "send_transfer" | "send_token" | "pay" | "payment" | "execute_payment"
            | "submit_payment" | "x402_payment" => "transfer",
            "simulate" | "dry_run" | "simulate_payment" | "preview_payment" | "x402_preview" => {
                "simulate_transfer"
            }
            "status" | "signature_status" | "txstatus" | "check_tx" | "transaction_status" => {
                "tx_status"
            }
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

                if let Some(mint) = Self::resolve_token_mint(&params) {
                    let token_account = crate::solana_rpc::find_token_account_by_owner_and_mint(
                        &endpoint,
                        &address,
                        &mint,
                        &policy.commitment,
                    )
                    .await?;

                    let Some(token_account) = token_account else {
                        return Ok(json!({
                            "status": "ok",
                            "address": address,
                            "mint": mint,
                            "token_account": Value::Null,
                            "amount_atomic": "0",
                            "decimals": Value::Null,
                            "ui_amount_string": "0"
                        }));
                    };

                    let token_balance = crate::solana_rpc::get_token_account_balance(
                        &endpoint,
                        &token_account,
                        &policy.commitment,
                    )
                    .await?;

                    return Ok(json!({
                        "status": "ok",
                        "address": address,
                        "mint": mint,
                        "token_account": token_account,
                        "amount_atomic": token_balance.get("value").and_then(|v| v.get("amount")).cloned().unwrap_or(Value::String("0".to_string())),
                        "decimals": token_balance.get("value").and_then(|v| v.get("decimals")).cloned().unwrap_or(Value::Null),
                        "ui_amount_string": token_balance.get("value").and_then(|v| v.get("uiAmountString")).cloned().unwrap_or(Value::String("0".to_string()))
                    }));
                }

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
                let mint = Self::resolve_token_mint(&params);
                let mint_for_response = mint.clone();
                let mut token_decimals_for_response: Option<u8> = None;
                let lamports = if mint.is_none() {
                    Some(Self::resolve_lamports(&params).ok_or_else(|| {
                        ButterflyBotError::Runtime("Missing lamports".to_string())
                    })?)
                } else {
                    None
                };
                let amount_atomic = if mint.is_some() {
                    Some(Self::resolve_amount_atomic(&params).ok_or_else(|| {
                        ButterflyBotError::Runtime(
                            "Missing amount_atomic for token transfer".to_string(),
                        )
                    })?)
                } else {
                    None
                };

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

                let (tx_base64, wallet_address, simulation_result) = if let Some(mint) =
                    mint.as_ref()
                {
                    let owner_address =
                        crate::security::solana_signer::wallet_address(user_id, actor)?;
                    let source_token_account = params
                        .get("from_token_account")
                        .and_then(|v| v.as_str())
                        .map(|v| v.to_string())
                        .or(crate::solana_rpc::find_token_account_by_owner_and_mint(
                            &endpoint,
                            &owner_address,
                            mint,
                            &policy.commitment,
                        )
                        .await?)
                        .ok_or_else(|| {
                            ButterflyBotError::Runtime(
                                "Missing source token account for owner+mint".to_string(),
                            )
                        })?;

                    let destination_token_account = params
                        .get("to_token_account")
                        .and_then(|v| v.as_str())
                        .map(|v| v.to_string())
                        .or(crate::solana_rpc::find_token_account_by_owner_and_mint(
                            &endpoint,
                            to,
                            mint,
                            &policy.commitment,
                        )
                        .await?);

                    let (destination_token_account, create_destination_ata) =
                        match destination_token_account {
                            Some(account) => (account, false),
                            None => (
                                crate::solana_rpc::derive_associated_token_address(to, mint)?,
                                true,
                            ),
                        };

                    let decimals = params
                        .get("decimals")
                        .and_then(|v| v.as_u64())
                        .map(|v| v as u8)
                        .unwrap_or(
                            crate::solana_rpc::get_token_decimals(
                                &endpoint,
                                mint,
                                &policy.commitment,
                            )
                            .await?,
                        );
                    token_decimals_for_response = Some(decimals);

                    if policy.simulation.enabled {
                        let probe_unit_limit = crate::solana_rpc::probe_compute_unit_limit(&policy);
                        let (probe_tx_base64, wallet_address) = crate::solana_rpc::build_spl_transfer_transaction_base64_with_unit_limit(
                            crate::solana_rpc::SplTransferTransactionBuildArgs {
                                from_seed: &from_seed,
                                source_token_account: &source_token_account,
                                mint,
                                destination_token_account: &destination_token_account,
                                destination_owner: Some(to),
                                create_destination_ata,
                                amount_atomic: amount_atomic.unwrap_or_default(),
                                decimals,
                                latest_blockhash: &latest_blockhash,
                                policy: &policy,
                                unit_limit: probe_unit_limit,
                            },
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
                            crate::solana_rpc::build_spl_transfer_transaction_base64_with_unit_limit(
                                crate::solana_rpc::SplTransferTransactionBuildArgs {
                                    from_seed: &from_seed,
                                    source_token_account: &source_token_account,
                                    mint,
                                    destination_token_account: &destination_token_account,
                                    destination_owner: Some(to),
                                    create_destination_ata,
                                    amount_atomic: amount_atomic.unwrap_or_default(),
                                    decimals,
                                    latest_blockhash: &latest_blockhash,
                                    policy: &policy,
                                    unit_limit: adjusted_unit_limit,
                                },
                            )?
                            .0
                        };
                        (tx_base64, wallet_address, Some(simulation))
                    } else {
                        let (tx_base64, wallet_address) =
                            crate::solana_rpc::build_spl_transfer_transaction_base64_with_unit_limit(
                                crate::solana_rpc::SplTransferTransactionBuildArgs {
                                    from_seed: &from_seed,
                                    source_token_account: &source_token_account,
                                    mint,
                                    destination_token_account: &destination_token_account,
                                    destination_owner: Some(to),
                                    create_destination_ata,
                                    amount_atomic: amount_atomic.unwrap_or_default(),
                                    decimals,
                                    latest_blockhash: &latest_blockhash,
                                    policy: &policy,
                                    unit_limit: policy.compute_budget.unit_limit,
                                },
                            )?;
                        (tx_base64, wallet_address, None)
                    }
                } else if policy.simulation.enabled {
                    let probe_unit_limit = crate::solana_rpc::probe_compute_unit_limit(&policy);
                    let (probe_tx_base64, wallet_address) =
                        crate::solana_rpc::build_transfer_transaction_base64_with_unit_limit(
                            &from_seed,
                            to,
                            lamports.unwrap_or_default(),
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
                            lamports.unwrap_or_default(),
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
                            lamports.unwrap_or_default(),
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
                        "mint": mint_for_response,
                        "amount_atomic": amount_atomic,
                        "decimals": token_decimals_for_response,
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
                    "mint": mint_for_response,
                    "amount_atomic": amount_atomic,
                    "decimals": token_decimals_for_response,
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
