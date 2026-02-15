use crate::error::{ButterflyBotError, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::rngs::SysRng;
use rand::TryRng;

const SERVICE: &str = "butterfly-bot";

fn keyring_disabled() -> bool {
    std::env::var("BUTTERFLY_BOT_DISABLE_KEYRING")
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(false)
}

pub fn set_secret(name: &str, value: &str) -> Result<()> {
    if keyring_disabled() {
        return Ok(());
    }
    let entry = keyring::Entry::new(SERVICE, name)
        .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
    entry
        .set_password(value)
        .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
    Ok(())
}

pub fn get_secret(name: &str) -> Result<Option<String>> {
    if keyring_disabled() {
        return Ok(None);
    }
    let entry = keyring::Entry::new(SERVICE, name)
        .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
    match entry.get_password() {
        Ok(value) => Ok(Some(value)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(err) => {
            let message = err.to_string().to_ascii_lowercase();
            let backend_unavailable = message.contains("dbus")
                || message.contains("secret service")
                || message.contains("keyring")
                || message.contains("message recipient disconnected")
                || message.contains("no such interface")
                || message.contains("service unknown")
                || message.contains("backend not available")
                || message.contains("platform secure storage failure");
            if backend_unavailable {
                return Ok(None);
            }
            Err(ButterflyBotError::Runtime(err.to_string()))
        }
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
    let mut rng = SysRng;
    rng.try_fill_bytes(&mut bytes)
        .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
    let generated = URL_SAFE_NO_PAD.encode(bytes);
    set_secret("daemon_auth_token", &generated)?;
    std::env::set_var("BUTTERFLY_BOT_TOKEN", &generated);
    Ok(generated)
}
