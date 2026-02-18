use std::env;
use std::path::PathBuf;
use std::sync::OnceLock;

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use diesel::sqlite::SqliteConnection;
use diesel_async::sync_connection_wrapper::SyncConnectionWrapper;
use rand::rngs::SysRng;
use rand::TryRng;

use crate::error::{ButterflyBotError, Result};

const DB_KEY_NAME: &str = "db_encryption_key";
const DB_KEY_FILE_NAME: &str = "db_encryption_key";

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

fn db_key_file_path() -> PathBuf {
    crate::runtime_paths::app_root()
        .join("secrets")
        .join(DB_KEY_FILE_NAME)
}

fn load_sqlcipher_key_from_file() -> Result<Option<String>> {
    let path = db_key_file_path();
    if !path.exists() {
        return Ok(None);
    }

    let raw = std::fs::read_to_string(&path).map_err(|e| {
        ButterflyBotError::Runtime(format!(
            "Failed to read SQLCipher key file {}: {e}",
            path.to_string_lossy()
        ))
    })?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(ButterflyBotError::Runtime(format!(
            "SQLCipher key file {} is empty",
            path.to_string_lossy()
        )));
    }
    Ok(Some(trimmed.to_string()))
}

fn persist_sqlcipher_key_to_file(key: &str) -> Result<()> {
    let path = db_key_file_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            ButterflyBotError::Runtime(format!(
                "Failed to create SQLCipher key directory {}: {e}",
                parent.to_string_lossy()
            ))
        })?;

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
        }
    }

    std::fs::write(&path, key).map_err(|e| {
        ButterflyBotError::Runtime(format!(
            "Failed to write SQLCipher key file {}: {e}",
            path.to_string_lossy()
        ))
    })?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }

    Ok(())
}

fn generated_db_key() -> Result<String> {
    let mut bytes = [0u8; 32];
    let mut rng = SysRng;
    rng.try_fill_bytes(&mut bytes)
        .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

pub fn get_sqlcipher_key() -> Result<String> {
    if let Ok(value) = env::var("BUTTERFLY_BOT_DB_KEY") {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            log_sqlcipher_key_source_once("env");
            return Ok(trimmed.to_string());
        }
    }

    if let Some(value) = crate::vault::get_secret(DB_KEY_NAME)? {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            std::env::set_var("BUTTERFLY_BOT_DB_KEY", trimmed);
            log_sqlcipher_key_source_once("keychain");
            return Ok(trimmed.to_string());
        }
    }

    if let Some(value) = load_sqlcipher_key_from_file()? {
        std::env::set_var("BUTTERFLY_BOT_DB_KEY", &value);
        log_sqlcipher_key_source_once("file_fallback");
        return Ok(value);
    }

    let generated = generated_db_key()?;

    if crate::vault::set_secret_required(DB_KEY_NAME, &generated).is_err() {
        persist_sqlcipher_key_to_file(&generated)?;
        log_sqlcipher_key_source_once("generated_file_fallback");
    } else {
        log_sqlcipher_key_source_once("generated_keychain");
    }

    std::env::set_var("BUTTERFLY_BOT_DB_KEY", &generated);
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
