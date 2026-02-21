use crate::error::{ButterflyBotError, Result};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::rngs::SysRng;
use rand::TryRng;
use std::path::PathBuf;
#[cfg(target_os = "macos")]
use std::process::Command;

const SERVICE: &str = "butterfly-bot";
const DAEMON_TOKEN_FILE: &str = "daemon_auth_token";

fn daemon_auth_token_file() -> PathBuf {
    crate::runtime_paths::app_root()
        .join("secrets")
        .join(DAEMON_TOKEN_FILE)
}

fn secret_fallback_file(name: &str) -> PathBuf {
    let encoded = URL_SAFE_NO_PAD.encode(name.as_bytes());
    crate::runtime_paths::app_root()
        .join("secrets")
        .join("fallback")
        .join(encoded)
}

fn read_secret_fallback_file(name: &str) -> Option<String> {
    let path = secret_fallback_file(name);
    let raw = std::fs::read_to_string(path).ok()?;
    let trimmed = raw.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn write_secret_fallback_file(name: &str, value: &str) {
    let path = secret_fallback_file(name);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, value);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
}

fn read_daemon_auth_token_file() -> Option<String> {
    let path = daemon_auth_token_file();
    let raw = std::fs::read_to_string(path).ok()?;
    let trimmed = raw.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn write_daemon_auth_token_file(token: &str) {
    let path = daemon_auth_token_file();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, token);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
}

fn keyring_backend_unavailable(message: &str) -> bool {
    let message = message.to_ascii_lowercase();
    message.contains("dbus")
        || message.contains("secret service")
        || message.contains("keyring")
        || message.contains("message recipient disconnected")
        || message.contains("no such interface")
        || message.contains("service unknown")
        || message.contains("backend not available")
        || message.contains("platform secure storage failure")
        || message.contains("keychain")
        || message.contains("user interaction is not allowed")
}

fn env_token() -> Option<String> {
    std::env::var("BUTTERFLY_BOT_TOKEN").ok().and_then(|token| {
        let trimmed = token.trim().to_string();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed)
        }
    })
}

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

#[cfg(target_os = "macos")]
fn macos_keychain_set_required(name: &str, value: &str) -> Result<()> {
    let output = Command::new("security")
        .arg("add-generic-password")
        .arg("-U")
        .arg("-s")
        .arg(SERVICE)
        .arg("-a")
        .arg(name)
        .arg("-w")
        .arg(value)
        .output()
        .map_err(|err| ButterflyBotError::SecurityStorage(err.to_string()))?;

    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let message = if stderr.is_empty() {
        format!("security add-generic-password failed with status {}", output.status)
    } else {
        stderr
    };
    Err(ButterflyBotError::SecurityStorage(message))
}

#[cfg(target_os = "macos")]
fn macos_keychain_get_required(name: &str) -> Result<Option<String>> {
    let output = Command::new("security")
        .arg("find-generic-password")
        .arg("-s")
        .arg(SERVICE)
        .arg("-a")
        .arg(name)
        .arg("-w")
        .output()
        .map_err(|err| ButterflyBotError::SecurityStorage(err.to_string()))?;

    if output.status.success() {
        let value = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if value.is_empty() {
            return Ok(None);
        }
        return Ok(Some(value));
    }

    if output.status.code() == Some(44) {
        return Ok(None);
    }

    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    let message = if stderr.is_empty() {
        format!(
            "security find-generic-password failed with status {}",
            output.status
        )
    } else {
        stderr
    };
    Err(ButterflyBotError::SecurityStorage(message))
}

pub fn set_secret(name: &str, value: &str) -> Result<()> {
    if keyring_disabled() {
        write_secret_fallback_file(name, value);
        return Ok(());
    }
    let entry = keyring::Entry::new(SERVICE, name)
        .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
    if let Err(err) = entry.set_password(value) {
        if keyring_backend_unavailable(&err.to_string()) {
            write_secret_fallback_file(name, value);
            return Ok(());
        }
        return Err(ButterflyBotError::Runtime(err.to_string()));
    }
    Ok(())
}

pub fn set_secret_required(name: &str, value: &str) -> Result<()> {
    if keyring_disabled() {
        return Err(ButterflyBotError::SecurityStorage(
            "Keyring is disabled (BUTTERFLY_BOT_DISABLE_KEYRING); cannot store required secret"
                .to_string(),
        ));
    }

    #[cfg(target_os = "macos")]
    {
        return macos_keychain_set_required(name, value);
    }

    #[cfg(not(target_os = "macos"))]
    {
    let entry = keyring::Entry::new(SERVICE, name)
        .map_err(|e| ButterflyBotError::SecurityStorage(e.to_string()))?;
    entry
        .set_password(value)
        .map_err(|err| ButterflyBotError::SecurityStorage(err.to_string()))?;
    Ok(())
    }
}

pub fn get_secret_required(name: &str) -> Result<Option<String>> {
    if keyring_disabled() {
        return Err(ButterflyBotError::SecurityStorage(
            "Keyring is disabled (BUTTERFLY_BOT_DISABLE_KEYRING); cannot read required secret"
                .to_string(),
        ));
    }

    #[cfg(target_os = "macos")]
    {
        return macos_keychain_get_required(name);
    }

    #[cfg(not(target_os = "macos"))]
    {
    let entry = keyring::Entry::new(SERVICE, name)
        .map_err(|e| ButterflyBotError::SecurityStorage(e.to_string()))?;
    match entry.get_password() {
        Ok(value) => {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                Ok(None)
            } else {
                Ok(Some(trimmed.to_string()))
            }
        }
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(err) => Err(ButterflyBotError::SecurityStorage(err.to_string())),
    }
    }
}

pub fn get_secret(name: &str) -> Result<Option<String>> {
    if keyring_disabled() {
        return Ok(read_secret_fallback_file(name));
    }
    let entry = keyring::Entry::new(SERVICE, name)
        .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
    match entry.get_password() {
        Ok(value) => Ok(Some(value)),
        Err(keyring::Error::NoEntry) => Ok(read_secret_fallback_file(name)),
        Err(err) => {
            if keyring_backend_unavailable(&err.to_string()) {
                return Ok(read_secret_fallback_file(name));
            }
            Err(ButterflyBotError::Runtime(err.to_string()))
        }
    }
}

pub fn ensure_daemon_auth_token() -> Result<String> {
    if let Some(token) = env_token() {
        return Ok(token);
    }

    if let Some(token) = get_secret("daemon_auth_token")? {
        let trimmed = token.trim().to_string();
        if !trimmed.is_empty() {
            std::env::set_var("BUTTERFLY_BOT_TOKEN", &trimmed);
            return Ok(trimmed);
        }
    }

    if let Some(token) = read_daemon_auth_token_file() {
        std::env::set_var("BUTTERFLY_BOT_TOKEN", &token);
        return Ok(token);
    }

    let mut bytes = [0u8; 32];
    let mut rng = SysRng;
    rng.try_fill_bytes(&mut bytes)
        .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
    let generated = URL_SAFE_NO_PAD.encode(bytes);
    let _ = set_secret("daemon_auth_token", &generated);
    write_daemon_auth_token_file(&generated);
    std::env::set_var("BUTTERFLY_BOT_TOKEN", &generated);
    Ok(generated)
}
