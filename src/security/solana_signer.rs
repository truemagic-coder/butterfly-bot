use crate::error::{ButterflyBotError, Result};
use crate::security::policy::SigningIntent;
use ed25519_dalek::{Signer, SigningKey};
use rand::rngs::SysRng;
use rand::TryRng;

fn wallet_secret_name(user_id: &str, actor: &str) -> String {
    format!("solana_wallet_seed_{}_{}", user_id, actor)
}

fn decode_seed(secret: &str) -> Result<[u8; 32]> {
    let bytes = bs58::decode(secret).into_vec().map_err(|_| {
        ButterflyBotError::SecurityStorage("invalid base58 wallet seed".to_string())
    })?;
    if bytes.len() != 32 {
        return Err(ButterflyBotError::SecurityStorage(
            "wallet seed must be 32 bytes".to_string(),
        ));
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&bytes);
    Ok(out)
}

fn encode_seed(seed: &[u8; 32]) -> String {
    bs58::encode(seed).into_string()
}

fn ensure_signing_key(user_id: &str, actor: &str) -> Result<SigningKey> {
    let secret_name = wallet_secret_name(user_id, actor);

    if let Some(existing) = crate::vault::get_secret_required(&secret_name)? {
        let seed = decode_seed(existing.trim())?;
        return Ok(SigningKey::from_bytes(&seed));
    }

    let mut seed = [0u8; 32];
    let mut rng = SysRng;
    rng.try_fill_bytes(&mut seed)
        .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;

    crate::vault::set_secret_required(&secret_name, &encode_seed(&seed))?;
    Ok(SigningKey::from_bytes(&seed))
}

fn signing_message(intent: &SigningIntent) -> Vec<u8> {
    let payload = serde_json::json!({
        "request_id": intent.request_id,
        "actor": intent.actor,
        "user_id": intent.user_id,
        "action_type": intent.action_type,
        "amount_atomic": intent.amount_atomic,
        "payee": intent.payee,
        "scheme_id": intent.scheme_id,
        "chain_id": intent.chain_id,
        "payment_authority": intent.payment_authority,
        "idempotency_key": intent.idempotency_key,
    });

    serde_json::to_vec(&payload).unwrap_or_default()
}

pub fn sign_intent(intent: &SigningIntent) -> Result<String> {
    let key = ensure_signing_key(&intent.user_id, &intent.actor)?;
    let signature = key.sign(&signing_message(intent));
    Ok(bs58::encode(signature.to_bytes()).into_string())
}

pub fn wallet_address(user_id: &str, actor: &str) -> Result<String> {
    let key = ensure_signing_key(user_id, actor)?;
    Ok(bs58::encode(key.verifying_key().to_bytes()).into_string())
}

pub(crate) fn signing_seed(user_id: &str, actor: &str) -> Result<[u8; 32]> {
    let key = ensure_signing_key(user_id, actor)?;
    Ok(key.to_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn intent() -> SigningIntent {
        SigningIntent {
            request_id: "req-solana-sign".to_string(),
            actor: "agent".to_string(),
            user_id: "user".to_string(),
            action_type: "x402_payment".to_string(),
            amount_atomic: 10,
            payee: "merchant.local".to_string(),
            context_requires_approval: false,
            scheme_id: Some("v2-solana-exact".to_string()),
            chain_id: Some("solana:5eykt4UsFv8P8NJdTREpY1vzqKqZKvdp".to_string()),
            payment_authority: Some("https://merchant.local".to_string()),
            idempotency_key: Some("idem-1".to_string()),
        }
    }

    #[test]
    fn signing_is_stable_for_same_intent() {
        crate::security::tpm_provider::set_tpm_available_for_tests(Some(true));
        crate::security::tpm_provider::set_dek_passphrase_for_tests(Some(
            "solana-signer-test-dek".to_string(),
        ));

        let temp = tempfile::tempdir().unwrap();
        crate::runtime_paths::set_app_root_override_for_tests(Some(temp.path().to_path_buf()));

        let first = sign_intent(&intent()).unwrap();
        let second = sign_intent(&intent()).unwrap();
        assert_eq!(first, second);

        crate::runtime_paths::set_app_root_override_for_tests(None);
        crate::security::tpm_provider::set_dek_passphrase_for_tests(None);
        crate::security::tpm_provider::set_tpm_available_for_tests(None);
    }
}
