use crate::config::Config;
use crate::error::ButterflyBotError;
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SolanaRpcProvider {
    QuickNode,
    Helius,
    Custom,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ComputeBudgetPolicy {
    pub unit_limit: u32,
    pub unit_price_microlamports: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SimulationPolicy {
    pub enabled: bool,
    pub commitment: String,
    pub replace_recent_blockhash: bool,
    pub sig_verify: bool,
    pub min_context_slot: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendPolicy {
    pub skip_preflight: bool,
    pub preflight_commitment: String,
    pub max_retries: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SolanaRpcExecutionPolicy {
    pub provider: SolanaRpcProvider,
    pub endpoint: Option<String>,
    pub commitment: String,
    pub compute_budget: ComputeBudgetPolicy,
    pub simulation: SimulationPolicy,
    pub send: SendPolicy,
}

impl SolanaRpcExecutionPolicy {
    pub fn default_for_provider(provider: SolanaRpcProvider) -> Self {
        match provider {
            SolanaRpcProvider::QuickNode => Self {
                provider,
                endpoint: None,
                commitment: "confirmed".to_string(),
                compute_budget: ComputeBudgetPolicy {
                    unit_limit: 300_000,
                    unit_price_microlamports: 2_500,
                },
                simulation: SimulationPolicy {
                    enabled: true,
                    commitment: "processed".to_string(),
                    replace_recent_blockhash: true,
                    sig_verify: false,
                    min_context_slot: None,
                },
                send: SendPolicy {
                    skip_preflight: false,
                    preflight_commitment: "confirmed".to_string(),
                    max_retries: 5,
                },
            },
            SolanaRpcProvider::Helius => Self {
                provider,
                endpoint: None,
                commitment: "confirmed".to_string(),
                compute_budget: ComputeBudgetPolicy {
                    unit_limit: 350_000,
                    unit_price_microlamports: 5_000,
                },
                simulation: SimulationPolicy {
                    enabled: true,
                    commitment: "processed".to_string(),
                    replace_recent_blockhash: false,
                    sig_verify: false,
                    min_context_slot: None,
                },
                send: SendPolicy {
                    skip_preflight: false,
                    preflight_commitment: "confirmed".to_string(),
                    max_retries: 8,
                },
            },
            SolanaRpcProvider::Custom => Self {
                provider,
                endpoint: None,
                commitment: "confirmed".to_string(),
                compute_budget: ComputeBudgetPolicy {
                    unit_limit: 300_000,
                    unit_price_microlamports: 3_000,
                },
                simulation: SimulationPolicy {
                    enabled: true,
                    commitment: "processed".to_string(),
                    replace_recent_blockhash: true,
                    sig_verify: false,
                    min_context_slot: None,
                },
                send: SendPolicy {
                    skip_preflight: false,
                    preflight_commitment: "confirmed".to_string(),
                    max_retries: 5,
                },
            },
        }
    }

    pub fn from_config(config: &Config) -> Result<Self, ButterflyBotError> {
        let Some(tools) = config.tools.as_ref() else {
            return Ok(Self::default_for_provider(SolanaRpcProvider::QuickNode));
        };
        Self::from_tools(tools)
    }

    pub fn from_tools(tools: &Value) -> Result<Self, ButterflyBotError> {
        let rpc_value = tools
            .get("settings")
            .and_then(|settings| settings.get("solana"))
            .and_then(|solana| solana.get("rpc"));

        let Some(rpc) = rpc_value else {
            return Ok(Self::default_for_provider(SolanaRpcProvider::QuickNode));
        };

        let provider = parse_provider(rpc.get("provider"))?;
        let mut policy = Self::default_for_provider(provider);

        if let Some(endpoint) = rpc.get("endpoint") {
            let endpoint_value = endpoint.as_str().ok_or_else(|| {
                ButterflyBotError::Config("tools.settings.solana.rpc.endpoint must be a string".to_string())
            })?;
            if !endpoint_value.trim().is_empty() {
                policy.endpoint = Some(endpoint_value.to_string());
            }
        }

        if let Some(commitment) = rpc.get("commitment") {
            policy.commitment = normalize_commitment(commitment)?;
        }

        if let Some(compute_budget) = rpc.get("compute_budget") {
            let compute_budget_obj = compute_budget.as_object().ok_or_else(|| {
                ButterflyBotError::Config(
                    "tools.settings.solana.rpc.compute_budget must be an object".to_string(),
                )
            })?;

            if let Some(unit_limit) = compute_budget_obj.get("unit_limit") {
                let unit_limit_value = unit_limit.as_u64().ok_or_else(|| {
                    ButterflyBotError::Config(
                        "tools.settings.solana.rpc.compute_budget.unit_limit must be an integer"
                            .to_string(),
                    )
                })?;
                policy.compute_budget.unit_limit = normalize_unit_limit(unit_limit_value)?;
            }

            if let Some(unit_price) = compute_budget_obj.get("unit_price_microlamports") {
                let unit_price_value = unit_price.as_u64().ok_or_else(|| {
                    ButterflyBotError::Config("tools.settings.solana.rpc.compute_budget.unit_price_microlamports must be an integer".to_string())
                })?;
                policy.compute_budget.unit_price_microlamports =
                    normalize_unit_price_microlamports(unit_price_value)?;
            }
        }

        if let Some(simulation) = rpc.get("simulation") {
            let simulation_obj = simulation.as_object().ok_or_else(|| {
                ButterflyBotError::Config(
                    "tools.settings.solana.rpc.simulation must be an object".to_string(),
                )
            })?;

            if let Some(enabled) = simulation_obj.get("enabled") {
                policy.simulation.enabled = enabled.as_bool().ok_or_else(|| {
                    ButterflyBotError::Config(
                        "tools.settings.solana.rpc.simulation.enabled must be a boolean"
                            .to_string(),
                    )
                })?;
            }

            if let Some(commitment) = simulation_obj.get("commitment") {
                policy.simulation.commitment = normalize_commitment(commitment)?;
            }

            if let Some(replace_recent_blockhash) = simulation_obj.get("replace_recent_blockhash") {
                policy.simulation.replace_recent_blockhash = replace_recent_blockhash
                    .as_bool()
                    .ok_or_else(|| {
                        ButterflyBotError::Config(
                            "tools.settings.solana.rpc.simulation.replace_recent_blockhash must be a boolean".to_string(),
                        )
                    })?;
            }

            if let Some(sig_verify) = simulation_obj.get("sig_verify") {
                policy.simulation.sig_verify = sig_verify.as_bool().ok_or_else(|| {
                    ButterflyBotError::Config(
                        "tools.settings.solana.rpc.simulation.sig_verify must be a boolean"
                            .to_string(),
                    )
                })?;
            }

            if let Some(min_context_slot) = simulation_obj.get("min_context_slot") {
                if min_context_slot.is_null() {
                    policy.simulation.min_context_slot = None;
                } else {
                    policy.simulation.min_context_slot =
                        Some(min_context_slot.as_u64().ok_or_else(|| {
                            ButterflyBotError::Config("tools.settings.solana.rpc.simulation.min_context_slot must be an integer".to_string())
                        })?);
                }
            }
        }

        if let Some(send) = rpc.get("send") {
            let send_obj = send.as_object().ok_or_else(|| {
                ButterflyBotError::Config("tools.settings.solana.rpc.send must be an object".to_string())
            })?;

            if let Some(skip_preflight) = send_obj.get("skip_preflight") {
                policy.send.skip_preflight = skip_preflight.as_bool().ok_or_else(|| {
                    ButterflyBotError::Config(
                        "tools.settings.solana.rpc.send.skip_preflight must be a boolean"
                            .to_string(),
                    )
                })?;
            }

            if let Some(preflight_commitment) = send_obj.get("preflight_commitment") {
                policy.send.preflight_commitment = normalize_commitment(preflight_commitment)?;
            }

            if let Some(max_retries) = send_obj.get("max_retries") {
                let max_retries_value = max_retries.as_u64().ok_or_else(|| {
                    ButterflyBotError::Config(
                        "tools.settings.solana.rpc.send.max_retries must be an integer".to_string(),
                    )
                })?;
                policy.send.max_retries = normalize_max_retries(max_retries_value)?;
            }
        }

        Ok(policy)
    }
}

fn parse_provider(value: Option<&Value>) -> Result<SolanaRpcProvider, ButterflyBotError> {
    let provider_value = value
        .and_then(|entry| entry.as_str())
        .unwrap_or("quicknode")
        .to_ascii_lowercase();

    match provider_value.as_str() {
        "quicknode" => Ok(SolanaRpcProvider::QuickNode),
        "helius" => Ok(SolanaRpcProvider::Helius),
        "custom" => Ok(SolanaRpcProvider::Custom),
        _ => Err(ButterflyBotError::Config(
            "tools.settings.solana.rpc.provider must be one of quicknode, helius, custom"
                .to_string(),
        )),
    }
}

fn normalize_commitment(value: &Value) -> Result<String, ButterflyBotError> {
    let commitment = value.as_str().ok_or_else(|| {
        ButterflyBotError::Config("commitment must be a string".to_string())
    })?;

    let normalized = commitment.to_ascii_lowercase();
    match normalized.as_str() {
        "processed" | "confirmed" | "finalized" => Ok(normalized),
        _ => Err(ButterflyBotError::Config(
            "commitment must be one of processed, confirmed, finalized".to_string(),
        )),
    }
}

fn normalize_unit_limit(value: u64) -> Result<u32, ButterflyBotError> {
    if value == 0 {
        return Err(ButterflyBotError::Config(
            "compute_budget.unit_limit must be greater than zero".to_string(),
        ));
    }
    let clamped = value.clamp(200_000, 1_400_000);
    Ok(clamped as u32)
}

fn normalize_unit_price_microlamports(value: u64) -> Result<u64, ButterflyBotError> {
    if value == 0 {
        return Err(ButterflyBotError::Config(
            "compute_budget.unit_price_microlamports must be greater than zero".to_string(),
        ));
    }
    Ok(value.clamp(1, 1_000_000))
}

fn normalize_max_retries(value: u64) -> Result<usize, ButterflyBotError> {
    if value > 100 {
        return Err(ButterflyBotError::Config(
            "send.max_retries must be less than or equal to 100".to_string(),
        ));
    }
    Ok(value as usize)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn quicknode_and_helius_have_distinct_defaults() {
        let quicknode = SolanaRpcExecutionPolicy::default_for_provider(SolanaRpcProvider::QuickNode);
        let helius = SolanaRpcExecutionPolicy::default_for_provider(SolanaRpcProvider::Helius);

        assert_ne!(
            quicknode.compute_budget.unit_price_microlamports,
            helius.compute_budget.unit_price_microlamports
        );
        assert_ne!(
            quicknode.simulation.replace_recent_blockhash,
            helius.simulation.replace_recent_blockhash
        );
        assert_ne!(quicknode.send.max_retries, helius.send.max_retries);
    }

    #[test]
    fn from_tools_applies_overrides_with_normalization() {
        let tools = json!({
            "settings": {
                "solana": {
                    "rpc": {
                        "provider": "helius",
                        "endpoint": "https://example.invalid/rpc",
                        "commitment": "Finalized",
                        "compute_budget": {
                            "unit_limit": 5_000_000,
                            "unit_price_microlamports": 9_999_999
                        },
                        "simulation": {
                            "enabled": true,
                            "commitment": "Processed",
                            "replace_recent_blockhash": true,
                            "sig_verify": false,
                            "min_context_slot": 42
                        },
                        "send": {
                            "skip_preflight": false,
                            "preflight_commitment": "confirmed",
                            "max_retries": 20
                        }
                    }
                }
            }
        });

        let policy = SolanaRpcExecutionPolicy::from_tools(&tools).unwrap();
        assert_eq!(policy.provider, SolanaRpcProvider::Helius);
        assert_eq!(policy.endpoint.as_deref(), Some("https://example.invalid/rpc"));
        assert_eq!(policy.commitment, "finalized");
        assert_eq!(policy.compute_budget.unit_limit, 1_400_000);
        assert_eq!(policy.compute_budget.unit_price_microlamports, 1_000_000);
        assert_eq!(policy.simulation.commitment, "processed");
        assert_eq!(policy.simulation.min_context_slot, Some(42));
        assert_eq!(policy.send.max_retries, 20);
    }

    #[test]
    fn from_tools_rejects_invalid_commitment() {
        let tools = json!({
            "settings": {
                "solana": {
                    "rpc": {
                        "commitment": "recent"
                    }
                }
            }
        });

        let err = SolanaRpcExecutionPolicy::from_tools(&tools).unwrap_err();
        assert!(
            err.to_string()
                .contains("commitment must be one of processed, confirmed, finalized")
        );
    }

    #[test]
    fn defaults_to_quicknode_when_missing_config() {
        let tools = json!({"settings": {"permissions": {"default_deny": true}}});
        let policy = SolanaRpcExecutionPolicy::from_tools(&tools).unwrap();
        assert_eq!(policy.provider, SolanaRpcProvider::QuickNode);
    }
}