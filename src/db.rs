use std::sync::{OnceLock, RwLock};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use diesel::Connection;
use diesel::sqlite::SqliteConnection;
use diesel_async::sync_connection_wrapper::SyncConnectionWrapper;
#[cfg(not(test))]
use rand::rngs::SysRng;
#[cfg(not(test))]
use rand::TryRng;

use crate::error::{ButterflyBotError, Result};

#[cfg(not(test))]
const DB_KEY_NAME: &str = "db_encryption_key";

fn sqlcipher_key_cache() -> &'static RwLock<Option<(String, String)>> {
    static CACHE: OnceLock<RwLock<Option<(String, String)>>> = OnceLock::new();
    CACHE.get_or_init(|| RwLock::new(None))
}

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

#[cfg(not(test))]
fn log_sqlcipher_key_source_once(source: &str) {
    static LOGGED: OnceLock<()> = OnceLock::new();
    if LOGGED.set(()).is_ok() {
        tracing::info!(db_key_source = source, "Resolved SQLCipher key source");
    }
}

#[cfg(not(test))]
fn generated_db_key() -> Result<String> {
    let mut bytes = [0u8; 32];
    let mut rng = SysRng;
    rng.try_fill_bytes(&mut bytes)
        .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

#[cfg(test)]
pub fn get_sqlcipher_key() -> Result<String> {
    let root = crate::runtime_paths::app_root()
        .to_string_lossy()
        .to_string();

    let lock = sqlcipher_key_cache();
    let cached = match lock.read() {
        Ok(guard) => guard.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    };
    if let Some((cached_root, cached_key)) = cached {
        if cached_root == root {
            return Ok(cached_key);
        }
    }

    let resolved = format!(
        "test-sqlcipher-key-{}",
        URL_SAFE_NO_PAD.encode(root.as_bytes())
    );
    match lock.write() {
        Ok(mut guard) => *guard = Some((root, resolved.clone())),
        Err(poisoned) => {
            let mut guard = poisoned.into_inner();
            *guard = Some((root, resolved.clone()));
        }
    }
    Ok(resolved)
}

#[cfg(not(test))]
pub fn get_sqlcipher_key() -> Result<String> {
    let root = crate::runtime_paths::app_root()
        .to_string_lossy()
        .to_string();

    let lock = sqlcipher_key_cache();
    let cached = match lock.read() {
        Ok(guard) => guard.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    };
    if let Some((cached_root, cached_key)) = cached {
        if cached_root == root {
            return Ok(cached_key);
        }
    }

    let resolved = if let Some(value) = crate::vault::get_secret(DB_KEY_NAME)? {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            log_sqlcipher_key_source_once("keychain");
            trimmed.to_string()
        } else {
            let generated = generated_db_key()?;
            crate::vault::set_secret_required(DB_KEY_NAME, &generated)?;
            log_sqlcipher_key_source_once("generated_keychain");
            generated
        }
    } else {
        let generated = generated_db_key()?;
        crate::vault::set_secret_required(DB_KEY_NAME, &generated)?;
        log_sqlcipher_key_source_once("generated_keychain");
        generated
    };

    match lock.write() {
        Ok(mut guard) => *guard = Some((root, resolved.clone())),
        Err(poisoned) => {
            let mut guard = poisoned.into_inner();
            *guard = Some((root, resolved.clone()));
        }
    }

    Ok(resolved)
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

fn sqlcipher_not_a_database(err: &ButterflyBotError) -> bool {
    let lowered = err.to_string().to_ascii_lowercase();
    lowered.contains("file is not a database")
        || lowered.contains("file is encrypted or is not a database")
}

fn validate_sqlcipher_connection_sync(conn: &mut SqliteConnection) -> Result<()> {
    diesel::connection::SimpleConnection::batch_execute(conn, "SELECT count(*) FROM sqlite_master;")
        .map_err(|e| ButterflyBotError::Runtime(e.to_string()))
}

fn archive_unreadable_db_file(database_url: &str) -> Result<()> {
    let path = std::path::Path::new(database_url);
    if !path.exists() {
        return Ok(());
    }

    let file_name = match path.file_name().and_then(|name| name.to_str()) {
        Some(name) if !name.trim().is_empty() => name,
        _ => return Ok(()),
    };

    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|dur| dur.as_secs())
        .unwrap_or(0);
    let backup_name = format!("{file_name}.recovery-{stamp}.bak");
    let backup_path = match path.parent() {
        Some(parent) => parent.join(backup_name),
        None => return Ok(()),
    };

    std::fs::rename(path, &backup_path).map_err(|e| {
        ButterflyBotError::Runtime(format!(
            "failed to archive unreadable database {} -> {}: {e}",
            path.to_string_lossy(),
            backup_path.to_string_lossy()
        ))
    })?;

    tracing::warn!(
        db_path = %path.to_string_lossy(),
        backup_path = %backup_path.to_string_lossy(),
        "Archived unreadable SQLCipher database; a new encrypted database will be created"
    );
    Ok(())
}

pub fn open_sqlcipher_connection_sync(database_url: &str) -> Result<SqliteConnection> {
    let mut conn = SqliteConnection::establish(database_url)
        .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;

    if let Err(err) = apply_sqlcipher_key_sync(&mut conn) {
        if sqlcipher_not_a_database(&err) {
            archive_unreadable_db_file(database_url)?;
            let mut rebuilt = SqliteConnection::establish(database_url)
                .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
            apply_sqlcipher_key_sync(&mut rebuilt)?;
            validate_sqlcipher_connection_sync(&mut rebuilt)?;
            return Ok(rebuilt);
        }
        return Err(err);
    }

    if let Err(err) = validate_sqlcipher_connection_sync(&mut conn) {
        if sqlcipher_not_a_database(&err) {
            archive_unreadable_db_file(database_url)?;
            let mut rebuilt = SqliteConnection::establish(database_url)
                .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
            apply_sqlcipher_key_sync(&mut rebuilt)?;
            validate_sqlcipher_connection_sync(&mut rebuilt)?;
            return Ok(rebuilt);
        }
        return Err(err);
    }

    Ok(conn)
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
