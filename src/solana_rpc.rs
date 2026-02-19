use std::str::FromStr;

use base64::{engine::general_purpose::STANDARD, Engine as _};
use serde_json::{json, Value};
use solana_compute_budget_interface::ComputeBudgetInstruction;
use solana_sdk::{
    hash::Hash,
    message::Message,
    pubkey::Pubkey,
    signature::{Keypair, Signer},
    transaction::Transaction,
};
use solana_system_interface::instruction as system_instruction;

use crate::error::{ButterflyBotError, Result};
use crate::security::solana_rpc_policy::SolanaRpcExecutionPolicy;

fn normalize_rpc_result(value: Value, method: &str) -> Result<Value> {
    if let Some(error) = value.get("error") {
        return Err(ButterflyBotError::Runtime(format!(
            "solana rpc {method} error: {error}"
        )));
    }

    value
        .get("result")
        .cloned()
        .ok_or_else(|| ButterflyBotError::Runtime(format!("solana rpc {method} missing result")))
}

pub async fn rpc_call(endpoint: &str, method: &str, params: Value) -> Result<Value> {
    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": method,
        "params": params,
    });

    let response = reqwest::Client::new()
        .post(endpoint)
        .json(&request)
        .send()
        .await
        .map_err(|e| ButterflyBotError::Runtime(format!("solana rpc transport failure: {e}")))?;

    let status = response.status();
    let body: Value = response
        .json()
        .await
        .map_err(|e| ButterflyBotError::Runtime(format!("solana rpc decode failure: {e}")))?;

    if !status.is_success() {
        return Err(ButterflyBotError::Runtime(format!(
            "solana rpc http {}: {body}",
            status
        )));
    }

    normalize_rpc_result(body, method)
}

pub async fn get_balance(endpoint: &str, address: &str, commitment: &str) -> Result<u64> {
    let result = rpc_call(
        endpoint,
        "getBalance",
        json!([address, {"commitment": commitment}]),
    )
    .await?;

    result
        .get("value")
        .and_then(|value| value.as_u64())
        .ok_or_else(|| ButterflyBotError::Runtime("solana rpc getBalance missing value".to_string()))
}

pub async fn get_latest_blockhash(endpoint: &str, commitment: &str) -> Result<String> {
    let result = rpc_call(
        endpoint,
        "getLatestBlockhash",
        json!([{ "commitment": commitment }]),
    )
    .await?;

    result
        .get("value")
        .and_then(|value| value.get("blockhash"))
        .and_then(|value| value.as_str())
        .map(|value| value.to_string())
        .ok_or_else(|| {
            ButterflyBotError::Runtime("solana rpc getLatestBlockhash missing blockhash".to_string())
        })
}

pub fn build_transfer_transaction_base64(
    from_seed: &[u8; 32],
    to_address: &str,
    lamports: u64,
    latest_blockhash: &str,
    policy: &SolanaRpcExecutionPolicy,
) -> Result<(String, String)> {
    build_transfer_transaction_base64_with_unit_limit(
        from_seed,
        to_address,
        lamports,
        latest_blockhash,
        policy,
        policy.compute_budget.unit_limit,
    )
}

pub fn build_transfer_transaction_base64_with_unit_limit(
    from_seed: &[u8; 32],
    to_address: &str,
    lamports: u64,
    latest_blockhash: &str,
    policy: &SolanaRpcExecutionPolicy,
    unit_limit: u32,
) -> Result<(String, String)> {
    let signer = Keypair::new_from_array(*from_seed);

    let destination = Pubkey::from_str(to_address)
        .map_err(|e| ButterflyBotError::Runtime(format!("invalid destination pubkey: {e}")))?;

    let recent_blockhash = Hash::from_str(latest_blockhash)
        .map_err(|e| ButterflyBotError::Runtime(format!("invalid blockhash: {e}")))?;

    let from_address = signer.pubkey().to_string();

    let instructions = vec![
        ComputeBudgetInstruction::set_compute_unit_limit(unit_limit),
        ComputeBudgetInstruction::set_compute_unit_price(
            policy.compute_budget.unit_price_microlamports,
        ),
        system_instruction::transfer(&signer.pubkey(), &destination, lamports),
    ];

    let message = Message::new(&instructions, Some(&signer.pubkey()));
    let tx = Transaction::new(&[&signer], message, recent_blockhash);

    let bytes = wincode::serialize(&tx)
        .map_err(|e| ButterflyBotError::Runtime(format!("failed to serialize tx: {e}")))?;

    Ok((STANDARD.encode(bytes), from_address))
}

pub fn probe_compute_unit_limit(policy: &SolanaRpcExecutionPolicy) -> u32 {
    policy.compute_budget.unit_limit.max(1_000_000).min(1_400_000)
}

pub fn recommended_compute_unit_limit(simulation_result: &Value, fallback: u32) -> u32 {
    let consumed = simulation_result
        .get("value")
        .and_then(|value| value.get("unitsConsumed"))
        .and_then(|value| value.as_u64());

    let Some(consumed) = consumed else {
        return fallback;
    };

    let padded = consumed
        .saturating_mul(12)
        .saturating_div(10)
        .saturating_add(25_000);

    padded.clamp(200_000, 1_400_000) as u32
}

pub async fn simulate_transaction(
    endpoint: &str,
    tx_base64: &str,
    policy: &SolanaRpcExecutionPolicy,
) -> Result<Value> {
    let mut options = json!({
        "encoding": "base64",
        "commitment": policy.simulation.commitment,
        "replaceRecentBlockhash": policy.simulation.replace_recent_blockhash,
        "sigVerify": policy.simulation.sig_verify,
    });

    if let Some(min_context_slot) = policy.simulation.min_context_slot {
        options["minContextSlot"] = json!(min_context_slot);
    }

    rpc_call(endpoint, "simulateTransaction", json!([tx_base64, options])).await
}

pub async fn send_transaction(
    endpoint: &str,
    tx_base64: &str,
    policy: &SolanaRpcExecutionPolicy,
) -> Result<String> {
    let result = rpc_call(
        endpoint,
        "sendTransaction",
        json!([
            tx_base64,
            {
                "encoding": "base64",
                "skipPreflight": policy.send.skip_preflight,
                "preflightCommitment": policy.send.preflight_commitment,
                "maxRetries": policy.send.max_retries,
            }
        ]),
    )
    .await?;

    result
        .as_str()
        .map(|value| value.to_string())
        .ok_or_else(|| ButterflyBotError::Runtime("solana rpc sendTransaction missing signature".to_string()))
}

pub async fn get_signature_status(endpoint: &str, signature: &str) -> Result<Value> {
    rpc_call(
        endpoint,
        "getSignatureStatuses",
        json!([[signature], {"searchTransactionHistory": true}]),
    )
    .await
}

pub async fn get_signatures_for_address(endpoint: &str, address: &str, limit: usize) -> Result<Value> {
    rpc_call(
        endpoint,
        "getSignaturesForAddress",
        json!([address, {"limit": limit.max(1).min(100)}]),
    )
    .await
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security::solana_rpc_policy::SolanaRpcProvider;

    fn decode_tx(encoded: &str) -> Transaction {
        let bytes = STANDARD.decode(encoded).unwrap();
        wincode::deserialize::<Transaction>(&bytes).unwrap()
    }

    #[test]
    fn rebuild_uses_simulation_recommended_compute_limit() {
        let policy = SolanaRpcExecutionPolicy::default_for_provider(SolanaRpcProvider::QuickNode);
        let from_seed = [7u8; 32];
        let to_address = "11111111111111111111111111111111";
        let latest_blockhash = "11111111111111111111111111111111";
        let lamports = 25_000;

        let probe_limit = probe_compute_unit_limit(&policy);
        let (probe_encoded, _) = build_transfer_transaction_base64_with_unit_limit(
            &from_seed,
            to_address,
            lamports,
            latest_blockhash,
            &policy,
            probe_limit,
        )
        .unwrap();

        let simulation = json!({"value": {"unitsConsumed": 500_000}});
        let adjusted_limit = recommended_compute_unit_limit(&simulation, policy.compute_budget.unit_limit);
        assert_ne!(adjusted_limit, probe_limit);

        let (final_encoded, _) = build_transfer_transaction_base64_with_unit_limit(
            &from_seed,
            to_address,
            lamports,
            latest_blockhash,
            &policy,
            adjusted_limit,
        )
        .unwrap();

        let probe_tx = decode_tx(&probe_encoded);
        let final_tx = decode_tx(&final_encoded);

        let expected_probe_ix = ComputeBudgetInstruction::set_compute_unit_limit(probe_limit);
        let expected_final_ix = ComputeBudgetInstruction::set_compute_unit_limit(adjusted_limit);

        assert_eq!(probe_tx.message.instructions[0].data, expected_probe_ix.data);
        assert_eq!(final_tx.message.instructions[0].data, expected_final_ix.data);
    }
}
