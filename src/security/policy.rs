use crate::error::ButterflyBotError;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SigningIntent {
    pub request_id: String,
    pub actor: String,
    pub user_id: String,
    pub action_type: String,
    pub amount_atomic: u64,
    pub payee: String,
    pub context_requires_approval: bool,
    #[serde(default)]
    pub scheme_id: Option<String>,
    #[serde(default)]
    pub chain_id: Option<String>,
    #[serde(default)]
    pub payment_authority: Option<String>,
    #[serde(default)]
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PolicyLimits {
    pub per_tx_limit: u64,
    pub daily_limit: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PolicyDecision {
    AutoApproved { reason_code: &'static str },
    NeedsApproval { reason_code: &'static str },
    Denied { reason_code: &'static str },
}

pub const ALLOW_AUTO_POLICY_OK: &str = "ALLOW_AUTO_POLICY_OK";
pub const PROMPT_CONTEXT_REQUIRED: &str = "PROMPT_CONTEXT_REQUIRED";
pub const PROMPT_USER_LIMIT_EXCEEDED: &str = "PROMPT_USER_LIMIT_EXCEEDED";
pub const DENY_GLOBAL_LIMIT: &str = "DENY_GLOBAL_LIMIT";
pub const DENY_UNTRUSTED_FACILITATOR_OR_PAYEE: &str = "DENY_UNTRUSTED_FACILITATOR_OR_PAYEE";
pub const DENY_UNAPPROVED_SCHEME: &str = "DENY_UNAPPROVED_SCHEME";
pub const DENY_INVALID_X402_INTENT: &str = "DENY_INVALID_X402_INTENT";

#[derive(Debug, Clone)]
pub struct PolicyEngine {
    pub user_limits: PolicyLimits,
    pub global_limits: PolicyLimits,
    pub trusted_payees: Vec<String>,
    pub trusted_payment_authorities: Vec<String>,
    pub allowed_x402_schemes: Vec<String>,
}

impl PolicyEngine {
    pub fn evaluate(&self, intent: &SigningIntent, user_daily_spend: u64) -> PolicyDecision {
        if intent.action_type == "x402_payment" {
            let Some(scheme_id) = intent.scheme_id.as_deref() else {
                return PolicyDecision::Denied {
                    reason_code: DENY_INVALID_X402_INTENT,
                };
            };

            if !self
                .allowed_x402_schemes
                .iter()
                .any(|scheme| scheme.eq_ignore_ascii_case(scheme_id))
            {
                return PolicyDecision::Denied {
                    reason_code: DENY_UNAPPROVED_SCHEME,
                };
            }

            let Some(chain_id_value) = intent.chain_id.as_deref() else {
                return PolicyDecision::Denied {
                    reason_code: DENY_INVALID_X402_INTENT,
                };
            };

            let chain_id = match x402_rs::chain::ChainId::from_str(chain_id_value) {
                Ok(value) => value,
                Err(_) => {
                    return PolicyDecision::Denied {
                        reason_code: DENY_INVALID_X402_INTENT,
                    }
                }
            };

            if chain_id.namespace() != "solana" {
                return PolicyDecision::Denied {
                    reason_code: DENY_UNAPPROVED_SCHEME,
                };
            }

            let Some(authority) = intent.payment_authority.as_deref() else {
                return PolicyDecision::Denied {
                    reason_code: DENY_UNTRUSTED_FACILITATOR_OR_PAYEE,
                };
            };

            if !self
                .trusted_payment_authorities
                .iter()
                .any(|candidate| candidate == authority)
            {
                return PolicyDecision::Denied {
                    reason_code: DENY_UNTRUSTED_FACILITATOR_OR_PAYEE,
                };
            }
        }

        if intent.context_requires_approval {
            return PolicyDecision::NeedsApproval {
                reason_code: PROMPT_CONTEXT_REQUIRED,
            };
        }

        if !self
            .trusted_payees
            .iter()
            .any(|payee| payee == &intent.payee)
        {
            return PolicyDecision::Denied {
                reason_code: DENY_UNTRUSTED_FACILITATOR_OR_PAYEE,
            };
        }

        if intent.amount_atomic > self.global_limits.per_tx_limit
            || user_daily_spend.saturating_add(intent.amount_atomic)
                > self.global_limits.daily_limit
        {
            return PolicyDecision::Denied {
                reason_code: DENY_GLOBAL_LIMIT,
            };
        }

        if intent.amount_atomic > self.user_limits.per_tx_limit
            || user_daily_spend.saturating_add(intent.amount_atomic) > self.user_limits.daily_limit
        {
            return PolicyDecision::NeedsApproval {
                reason_code: PROMPT_USER_LIMIT_EXCEEDED,
            };
        }

        PolicyDecision::AutoApproved {
            reason_code: ALLOW_AUTO_POLICY_OK,
        }
    }
}

impl PolicyEngine {
    pub fn from_json(value: &serde_json::Value) -> Result<Self, ButterflyBotError> {
        let object = value.as_object().ok_or_else(|| {
            ButterflyBotError::SecurityPolicy("policy config must be an object".to_string())
        })?;

        let user = object
            .get("user_limits")
            .and_then(|v| v.as_object())
            .ok_or_else(|| {
                ButterflyBotError::SecurityPolicy("policy config missing user_limits".to_string())
            })?;
        let global = object
            .get("global_limits")
            .and_then(|v| v.as_object())
            .ok_or_else(|| {
                ButterflyBotError::SecurityPolicy("policy config missing global_limits".to_string())
            })?;
        let trusted_payees = object
            .get("trusted_payees")
            .and_then(|v| v.as_array())
            .ok_or_else(|| {
                ButterflyBotError::SecurityPolicy(
                    "policy config missing trusted_payees".to_string(),
                )
            })?;
        let trusted_payment_authorities = object
            .get("trusted_payment_authorities")
            .and_then(|v| v.as_array())
            .ok_or_else(|| {
                ButterflyBotError::SecurityPolicy(
                    "policy config missing trusted_payment_authorities".to_string(),
                )
            })?;
        let allowed_x402_schemes = object
            .get("allowed_x402_schemes")
            .and_then(|v| v.as_array())
            .ok_or_else(|| {
                ButterflyBotError::SecurityPolicy(
                    "policy config missing allowed_x402_schemes".to_string(),
                )
            })?;

        let parse_limits = |entry: &serde_json::Map<String, serde_json::Value>| -> Result<PolicyLimits, ButterflyBotError> {
            let per_tx_limit = entry
                .get("per_tx_limit")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| {
                    ButterflyBotError::SecurityPolicy(
                        "policy limits require per_tx_limit".to_string(),
                    )
                })?;
            let daily_limit = entry
                .get("daily_limit")
                .and_then(|v| v.as_u64())
                .ok_or_else(|| {
                    ButterflyBotError::SecurityPolicy(
                        "policy limits require daily_limit".to_string(),
                    )
                })?;

            if per_tx_limit == 0 || daily_limit == 0 || per_tx_limit > daily_limit {
                return Err(ButterflyBotError::SecurityPolicy(
                    "policy limits are invalid".to_string(),
                ));
            }

            Ok(PolicyLimits {
                per_tx_limit,
                daily_limit,
            })
        };

        let mut payees = Vec::new();
        for value in trusted_payees {
            let payee = value.as_str().ok_or_else(|| {
                ButterflyBotError::SecurityPolicy("trusted_payees must be strings".to_string())
            })?;
            if payee.trim().is_empty() {
                return Err(ButterflyBotError::SecurityPolicy(
                    "trusted_payees cannot be empty".to_string(),
                ));
            }
            payees.push(payee.to_string());
        }

        let mut authorities = Vec::new();
        for value in trusted_payment_authorities {
            let authority = value.as_str().ok_or_else(|| {
                ButterflyBotError::SecurityPolicy(
                    "trusted_payment_authorities must be strings".to_string(),
                )
            })?;
            if authority.trim().is_empty() {
                return Err(ButterflyBotError::SecurityPolicy(
                    "trusted_payment_authorities cannot be empty".to_string(),
                ));
            }
            authorities.push(authority.to_string());
        }

        let mut schemes = Vec::new();
        for value in allowed_x402_schemes {
            let scheme = value.as_str().ok_or_else(|| {
                ButterflyBotError::SecurityPolicy(
                    "allowed_x402_schemes must be strings".to_string(),
                )
            })?;
            if scheme.trim().is_empty() {
                return Err(ButterflyBotError::SecurityPolicy(
                    "allowed_x402_schemes cannot be empty".to_string(),
                ));
            }
            schemes.push(scheme.to_string());
        }

        Ok(Self {
            user_limits: parse_limits(user)?,
            global_limits: parse_limits(global)?,
            trusted_payees: payees,
            trusted_payment_authorities: authorities,
            allowed_x402_schemes: schemes,
        })
    }
}

pub fn default_policy_engine() -> PolicyEngine {
    PolicyEngine {
        user_limits: PolicyLimits {
            per_tx_limit: 100_000,
            daily_limit: 500_000,
        },
        global_limits: PolicyLimits {
            per_tx_limit: 1_000_000,
            daily_limit: 5_000_000,
        },
        trusted_payees: vec!["merchant.local".to_string()],
        trusted_payment_authorities: vec!["https://merchant.local".to_string()],
        allowed_x402_schemes: vec!["v1-solana-exact".to_string(), "v2-solana-exact".to_string()],
    }
}

pub fn ensure_policy_allows(decision: &PolicyDecision) -> Result<(), ButterflyBotError> {
    match decision {
        PolicyDecision::Denied { reason_code } => {
            Err(ButterflyBotError::SecurityPolicy(reason_code.to_string()))
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base_intent() -> SigningIntent {
        SigningIntent {
            request_id: "req-1".to_string(),
            actor: "agent".to_string(),
            user_id: "user".to_string(),
            action_type: "x402_payment".to_string(),
            amount_atomic: 10_000,
            payee: "merchant.local".to_string(),
            context_requires_approval: false,
            scheme_id: Some("v2-solana-exact".to_string()),
            chain_id: Some("solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp".to_string()),
            payment_authority: Some("https://merchant.local".to_string()),
            idempotency_key: Some("idem-1".to_string()),
        }
    }

    #[test]
    fn context_requires_prompt() {
        let engine = default_policy_engine();
        let mut intent = base_intent();
        intent.context_requires_approval = true;

        let decision = engine.evaluate(&intent, 0);
        assert_eq!(
            decision,
            PolicyDecision::NeedsApproval {
                reason_code: PROMPT_CONTEXT_REQUIRED
            }
        );
    }

    #[test]
    fn untrusted_payee_denied() {
        let engine = default_policy_engine();
        let mut intent = base_intent();
        intent.payee = "evil.local".to_string();

        let decision = engine.evaluate(&intent, 0);
        assert_eq!(
            decision,
            PolicyDecision::Denied {
                reason_code: DENY_UNTRUSTED_FACILITATOR_OR_PAYEE
            }
        );
    }

    #[test]
    fn global_limit_denied() {
        let engine = default_policy_engine();
        let mut intent = base_intent();
        intent.amount_atomic = 2_000_000;

        let decision = engine.evaluate(&intent, 0);
        assert_eq!(
            decision,
            PolicyDecision::Denied {
                reason_code: DENY_GLOBAL_LIMIT
            }
        );
    }

    #[test]
    fn from_json_validates_schema() {
        let config = serde_json::json!({
            "user_limits": {"per_tx_limit": 100, "daily_limit": 500},
            "global_limits": {"per_tx_limit": 1000, "daily_limit": 5000},
            "trusted_payees": ["merchant.local"],
            "trusted_payment_authorities": ["https://merchant.local"],
            "allowed_x402_schemes": ["v1-solana-exact", "v2-solana-exact"]
        });

        let policy = PolicyEngine::from_json(&config).unwrap();
        assert_eq!(policy.user_limits.per_tx_limit, 100);
        assert_eq!(policy.global_limits.daily_limit, 5000);
        assert_eq!(policy.trusted_payees.len(), 1);
        assert_eq!(policy.trusted_payment_authorities.len(), 1);
        assert_eq!(policy.allowed_x402_schemes.len(), 2);
    }

    #[test]
    fn from_json_rejects_invalid_limits() {
        let config = serde_json::json!({
            "user_limits": {"per_tx_limit": 1000, "daily_limit": 500},
            "global_limits": {"per_tx_limit": 1000, "daily_limit": 5000},
            "trusted_payees": ["merchant.local"],
            "trusted_payment_authorities": ["https://merchant.local"],
            "allowed_x402_schemes": ["v1-solana-exact", "v2-solana-exact"]
        });

        let err = PolicyEngine::from_json(&config).unwrap_err();
        assert!(format!("{err}").contains("policy limits are invalid"));
    }

    #[test]
    fn unapproved_x402_scheme_is_denied() {
        let engine = default_policy_engine();
        let mut intent = base_intent();
        intent.scheme_id = Some("v3-solana-exact".to_string());

        let decision = engine.evaluate(&intent, 0);
        assert_eq!(
            decision,
            PolicyDecision::Denied {
                reason_code: DENY_UNAPPROVED_SCHEME
            }
        );
    }

    #[test]
    fn non_solana_chain_is_denied() {
        let engine = default_policy_engine();
        let mut intent = base_intent();
        intent.chain_id = Some("eip155:8453".to_string());

        let decision = engine.evaluate(&intent, 0);
        assert_eq!(
            decision,
            PolicyDecision::Denied {
                reason_code: DENY_UNAPPROVED_SCHEME
            }
        );
    }

    #[test]
    fn untrusted_payment_authority_is_denied() {
        let engine = default_policy_engine();
        let mut intent = base_intent();
        intent.payment_authority = Some("https://evil.local".to_string());

        let decision = engine.evaluate(&intent, 0);
        assert_eq!(
            decision,
            PolicyDecision::Denied {
                reason_code: DENY_UNTRUSTED_FACILITATOR_OR_PAYEE
            }
        );
    }
}
