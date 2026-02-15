use crate::error::{ButterflyBotError, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::rngs::OsRng;
use rand::RngCore;

const SERVICE: &str = "butterfly-bot";

pub fn set_secret(name: &str, value: &str) -> Result<()> {
    let entry = keyring::Entry::new(SERVICE, name)
        .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
    entry
        .set_password(value)
        .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
    Ok(())
}

pub fn get_secret(name: &str) -> Result<Option<String>> {
    let entry = keyring::Entry::new(SERVICE, name)
        .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
    match entry.get_password() {
        Ok(value) => Ok(Some(value)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(err) => Err(ButterflyBotError::Runtime(err.to_string())),
    }
}

pub fn ensure_daemon_auth_token() -> Result<String> {
    if let Some(token) = get_secret("daemon_auth_token")? {
        let trimmed = token.trim().to_string();
        if !trimmed.is_empty() {
            std::env::set_var("BUTTERFLY_BOT_TOKEN", &trimmed);
            return Ok(trimmed);
        }
    }

    let mut bytes = [0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    let generated = URL_SAFE_NO_PAD.encode(bytes);
    set_secret("daemon_auth_token", &generated)?;
    std::env::set_var("BUTTERFLY_BOT_TOKEN", &generated);
    Ok(generated)
}
