use crate::error::{ButterflyBotError, Result};

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
