use std::sync::OnceLock;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use diesel::sqlite::SqliteConnection;
use diesel_async::sync_connection_wrapper::SyncConnectionWrapper;
use rand::rngs::SysRng;
use rand::TryRng;

use crate::error::{ButterflyBotError, Result};

const DB_KEY_NAME: &str = "db_encryption_key";

fn tune_sqlcipher_log_level_sync(conn: &mut SqliteConnection) {
    if let Err(err) =
        diesel::RunQueryDsl::execute(diesel::sql_query("PRAGMA cipher_log_level = ERROR"), conn)
    {
        tracing::debug!("Unable to set SQLCipher log level (sync): {}", err);
    }
}

async fn tune_sqlcipher_log_level_async(conn: &mut SyncConnectionWrapper<SqliteConnection>) {
    if let Err(err) = diesel_async::RunQueryDsl::execute(
        diesel::sql_query("PRAGMA cipher_log_level = ERROR"),
        conn,
    )
    .await
    {
        tracing::debug!("Unable to set SQLCipher log level (async): {}", err);
    }
}

fn log_sqlcipher_key_source_once(source: &str) {
    static LOGGED: OnceLock<()> = OnceLock::new();
    if LOGGED.set(()).is_ok() {
        tracing::info!(db_key_source = source, "Resolved SQLCipher key source");
    }
}

fn generated_db_key() -> Result<String> {
    let mut bytes = [0u8; 32];
    let mut rng = SysRng;
    rng.try_fill_bytes(&mut bytes)
        .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

pub fn get_sqlcipher_key() -> Result<String> {
    if let Some(value) = crate::vault::get_secret(DB_KEY_NAME)? {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            log_sqlcipher_key_source_once("keychain");
            return Ok(trimmed.to_string());
        }
    }

    let generated = generated_db_key()?;
    crate::vault::set_secret_required(DB_KEY_NAME, &generated)?;
    log_sqlcipher_key_source_once("generated_keychain");
    Ok(generated)
}

pub fn apply_sqlcipher_key_sync(conn: &mut SqliteConnection) -> Result<()> {
    diesel::RunQueryDsl::execute(diesel::sql_query("PRAGMA busy_timeout = 5000"), conn)
        .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
    let key = get_sqlcipher_key()?;
    let escaped_key = key.replace('\'', "''");
    diesel::RunQueryDsl::execute(
        diesel::sql_query(format!("PRAGMA key = '{escaped_key}'")),
        conn,
    )
    .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
    tune_sqlcipher_log_level_sync(conn);
    Ok(())
}

pub async fn apply_sqlcipher_key_async(
    conn: &mut SyncConnectionWrapper<SqliteConnection>,
) -> Result<()> {
    diesel_async::RunQueryDsl::execute(diesel::sql_query("PRAGMA busy_timeout = 5000"), conn)
        .await
        .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
    let key = get_sqlcipher_key()?;
    let escaped_key = key.replace('\'', "''");
    diesel_async::RunQueryDsl::execute(
        diesel::sql_query(format!("PRAGMA key = '{escaped_key}'")),
        conn,
    )
    .await
    .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
    tune_sqlcipher_log_level_async(conn).await;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn env_test_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn clear_env() {
        crate::runtime_paths::set_app_root_override_for_tests(None);
    }

    #[test]
    fn generates_and_reloads_db_key_from_secure_store() {
        let _guard = env_test_lock().lock().expect("test env lock poisoned");
        let temp = tempfile::tempdir().unwrap();

        clear_env();
        crate::runtime_paths::set_app_root_override_for_tests(Some(temp.path().to_path_buf()));

        let first = get_sqlcipher_key().unwrap();
        let second = get_sqlcipher_key().unwrap();

        assert_eq!(first, second);
        assert!(!first.trim().is_empty());

        clear_env();
    }
}
