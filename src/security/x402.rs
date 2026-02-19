use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{ButterflyBotError, Result};
use crate::security::policy::SigningIntent;

pub const DENY_INVALID_X402_INTENT: &str = "DENY_INVALID_X402_INTENT";
pub const DENY_UNAPPROVED_SCHEME: &str = "DENY_UNAPPROVED_SCHEME";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalX402Intent {
    pub scheme_id: String,
    pub chain_id: String,
    pub asset_id: String,
    pub amount_atomic: u64,
    pub payee: String,
    pub payment_authority: String,
    pub request_expiry: u64,
    pub idempotency_key: String,
    pub context_requires_approval: bool,
}

fn deny(code: &str, detail: &str) -> ButterflyBotError {
    ButterflyBotError::SecurityPolicy(format!("{code}: {detail}"))
}

fn parse_u64_atomic(value: &str) -> Result<u64> {
    value
        .trim()
        .parse::<u64>()
        .map_err(|_| deny(DENY_INVALID_X402_INTENT, "amount_atomic parse failure"))
}

fn extract_payment_authority(extra: Option<&Value>, merchant_origin: Option<&str>) -> String {
    let from_extra = extra.and_then(|value| {
        value
            .get("paymentAuthority")
            .or_else(|| value.get("payment_authority"))
            .or_else(|| value.get("facilitator"))
            .or_else(|| value.get("facilitatorUrl"))
            .or_else(|| value.get("settlementEndpoint"))
            .and_then(|candidate| candidate.as_str())
            .map(|text| text.trim().to_string())
            .filter(|text| !text.is_empty())
    });

    from_extra
        .or_else(|| {
            merchant_origin
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
        })
        .unwrap_or_else(|| "unknown_authority".to_string())
}

pub fn canonicalize_payment_required(
    request_id: &str,
    actor: &str,
    user_id: &str,
    payment_required_json: &Value,
    merchant_origin: Option<&str>,
    context_requires_approval: bool,
    idempotency_key: Option<&str>,
) -> Result<(CanonicalX402Intent, SigningIntent)> {
    let x402_version = payment_required_json
        .get("x402Version")
        .and_then(|value| value.as_u64())
        .ok_or_else(|| deny(DENY_INVALID_X402_INTENT, "missing x402Version"))?;

    let idempotency_key = idempotency_key
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| request_id.to_string());

    if x402_version == 1 {
        let payment_required: x402_rs::proto::v1::PaymentRequired =
            serde_json::from_value(payment_required_json.clone())
                .map_err(|_| deny(DENY_INVALID_X402_INTENT, "invalid v1 payment_required"))?;

        let requirement = payment_required
            .accepts
            .into_iter()
            .find(|entry| entry.scheme == "exact")
            .ok_or_else(|| deny(DENY_UNAPPROVED_SCHEME, "no supported v1 scheme"))?;

        let chain_id = x402_rs::chain::ChainId::from_network_name(&requirement.network)
            .ok_or_else(|| deny(DENY_INVALID_X402_INTENT, "unknown v1 network"))?;
        if chain_id.namespace() != "solana" {
            return Err(deny(
                DENY_UNAPPROVED_SCHEME,
                "only Solana chain is allowed",
            ));
        }

        let amount_atomic = parse_u64_atomic(&requirement.max_amount_required)?;
        let request_expiry = now_ts().saturating_add(requirement.max_timeout_seconds);
        let payment_authority =
            extract_payment_authority(requirement.extra.as_ref(), merchant_origin);

        let canonical = CanonicalX402Intent {
            scheme_id: "v1-solana-exact".to_string(),
            chain_id: chain_id.to_string(),
            asset_id: requirement.asset.clone(),
            amount_atomic,
            payee: requirement.pay_to.clone(),
            payment_authority,
            request_expiry,
            idempotency_key,
            context_requires_approval,
        };

        let intent = SigningIntent {
            request_id: request_id.to_string(),
            actor: actor.to_string(),
            user_id: user_id.to_string(),
            action_type: "x402_payment".to_string(),
            amount_atomic,
            payee: requirement.pay_to,
            context_requires_approval,
            scheme_id: Some("v1-solana-exact".to_string()),
            chain_id: Some(chain_id.to_string()),
            payment_authority: Some(canonical.payment_authority.clone()),
            idempotency_key: Some(canonical.idempotency_key.clone()),
        };

        return Ok((canonical, intent));
    }

    if x402_version == 2 {
        let payment_required: x402_rs::proto::v2::PaymentRequired =
            serde_json::from_value(payment_required_json.clone())
                .map_err(|_| deny(DENY_INVALID_X402_INTENT, "invalid v2 payment_required"))?;

        let requirement = payment_required
            .accepts
            .into_iter()
            .find(|entry| entry.scheme == "exact")
            .ok_or_else(|| deny(DENY_UNAPPROVED_SCHEME, "no supported v2 scheme"))?;

        if requirement.network.namespace() != "solana" {
            return Err(deny(
                DENY_UNAPPROVED_SCHEME,
                "only Solana chain is allowed",
            ));
        }

        let amount_atomic = parse_u64_atomic(&requirement.amount)?;
        let request_expiry = now_ts().saturating_add(requirement.max_timeout_seconds);
        let payment_authority =
            extract_payment_authority(requirement.extra.as_ref(), merchant_origin);

        let canonical = CanonicalX402Intent {
            scheme_id: "v2-solana-exact".to_string(),
            chain_id: requirement.network.to_string(),
            asset_id: requirement.asset.clone(),
            amount_atomic,
            payee: requirement.pay_to.clone(),
            payment_authority,
            request_expiry,
            idempotency_key,
            context_requires_approval,
        };

        let intent = SigningIntent {
            request_id: request_id.to_string(),
            actor: actor.to_string(),
            user_id: user_id.to_string(),
            action_type: "x402_payment".to_string(),
            amount_atomic,
            payee: requirement.pay_to,
            context_requires_approval,
            scheme_id: Some("v2-solana-exact".to_string()),
            chain_id: Some(requirement.network.to_string()),
            payment_authority: Some(canonical.payment_authority.clone()),
            idempotency_key: Some(canonical.idempotency_key.clone()),
        };

        return Ok((canonical, intent));
    }

    Err(deny(
        DENY_UNAPPROVED_SCHEME,
        "unsupported x402 version for Solana-only mode",
    ))
}

fn now_ts() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonicalize_v2_solana_exact_success() {
        let challenge = serde_json::json!({
            "x402Version": 2,
            "resource": {
                "description": "pay",
                "mimeType": "application/json",
                "url": "https://merchant.local/pay"
            },
            "accepts": [{
                "scheme": "exact",
                "network": "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp",
                "amount": "1000",
                "payTo": "merchant.local",
                "maxTimeoutSeconds": 300,
                "asset": "USDC",
                "extra": {"paymentAuthority": "https://facilitator.local"}
            }]
        });

        let (canonical, intent) = canonicalize_payment_required(
            "req-1",
            "agent",
            "user",
            &challenge,
            Some("https://merchant.local"),
            false,
            Some("idem-1"),
        )
        .unwrap();

        assert_eq!(canonical.scheme_id, "v2-solana-exact");
        assert_eq!(canonical.chain_id, "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp");
        assert_eq!(canonical.amount_atomic, 1000);
        assert_eq!(canonical.idempotency_key, "idem-1");
        assert_eq!(intent.amount_atomic, 1000);
        assert_eq!(intent.payee, "merchant.local");
    }

    #[test]
    fn canonicalize_rejects_non_solana_chain() {
        let challenge = serde_json::json!({
            "x402Version": 2,
            "resource": {
                "description": "pay",
                "mimeType": "application/json",
                "url": "https://merchant.local/pay"
            },
            "accepts": [{
                "scheme": "exact",
                "network": "eip155:8453",
                "amount": "1000",
                "payTo": "merchant.local",
                "maxTimeoutSeconds": 300,
                "asset": "USDC"
            }]
        });

        let err = canonicalize_payment_required(
            "req-2",
            "agent",
            "user",
            &challenge,
            Some("https://merchant.local"),
            false,
            None,
        )
        .unwrap_err();

        assert!(format!("{err}").contains(DENY_UNAPPROVED_SCHEME));
    }

    #[test]
    fn canonicalize_rejects_unsupported_scheme() {
        let challenge = serde_json::json!({
            "x402Version": 2,
            "resource": {
                "description": "pay",
                "mimeType": "application/json",
                "url": "https://merchant.local/pay"
            },
            "accepts": [{
                "scheme": "streaming",
                "network": "solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp",
                "amount": "1000",
                "payTo": "merchant.local",
                "maxTimeoutSeconds": 300,
                "asset": "USDC"
            }]
        });

        let err = canonicalize_payment_required(
            "req-3",
            "agent",
            "user",
            &challenge,
            None,
            false,
            None,
        )
        .unwrap_err();

        assert!(format!("{err}").contains(DENY_UNAPPROVED_SCHEME));
    }

    #[test]
    fn canonicalize_v1_solana_exact_success() {
        let challenge = serde_json::json!({
            "x402Version": 1,
            "accepts": [{
                "scheme": "exact",
                "network": "solana",
                "maxAmountRequired": "2000",
                "resource": "https://merchant.local/pay",
                "description": "pay",
                "mimeType": "application/json",
                "payTo": "merchant.local",
                "maxTimeoutSeconds": 60,
                "asset": "USDC"
            }]
        });

        let (canonical, _intent) = canonicalize_payment_required(
            "req-4",
            "agent",
            "user",
            &challenge,
            Some("https://merchant.local"),
            true,
            None,
        )
        .unwrap();

        assert_eq!(canonical.scheme_id, "v1-solana-exact");
        assert_eq!(canonical.amount_atomic, 2000);
        assert!(canonical.context_requires_approval);
    }
}
