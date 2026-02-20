use crate::error::{ButterflyBotError, Result};
use crate::security::cocoon_store;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::rngs::SysRng;
use rand::TryRng;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock, RwLock};

#[cfg(test)]
use std::cell::RefCell;

pub trait SecretProvider: Send + Sync {
    fn set_secret(&self, name: &str, value: &str, allow_backend_unavailable: bool) -> Result<()>;
    fn get_secret(&self, name: &str) -> Result<Option<String>>;
}

struct CocoonFileSecretProvider;

impl CocoonFileSecretProvider {
    fn passphrase(&self) -> Result<String> {
        crate::security::tpm_provider::resolve_dek_passphrase()
    }

    fn secret_path(&self, name: &str) -> PathBuf {
        let sanitized = name
            .chars()
            .map(|ch| {
                if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.') {
                    ch
                } else {
                    '_'
                }
            })
            .collect::<String>();
        crate::runtime_paths::app_root()
            .join("secrets")
            .join(format!("{sanitized}.cocoon"))
    }
}

impl SecretProvider for CocoonFileSecretProvider {
    fn set_secret(&self, name: &str, value: &str, _allow_backend_unavailable: bool) -> Result<()> {
        let path = self.secret_path(name);
        if value.trim().is_empty() {
            match std::fs::remove_file(&path) {
                Ok(()) => return Ok(()),
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(()),
                Err(e) => {
                    return Err(ButterflyBotError::SecurityStorage(format!(
                        "failed to clear encrypted secret {}: {e}",
                        path.to_string_lossy()
                    )));
                }
            }
        }

        let passphrase = self.passphrase()?;
        crate::security::hardening::with_sensitive_string(passphrase, |sensitive_passphrase| {
            cocoon_store::persist_secret(&path, sensitive_passphrase, value)
        })
    }

    fn get_secret(&self, name: &str) -> Result<Option<String>> {
        let path = self.secret_path(name);
        let passphrase = self.passphrase()?;
        match crate::security::hardening::with_sensitive_string(passphrase, |sensitive_passphrase| {
            cocoon_store::load_secret(&path, sensitive_passphrase)
        }) {
            Ok(value) => Ok(value),
            Err(ButterflyBotError::SecurityStorage(message))
                if message.contains(" is empty") =>
            {
                let _ = std::fs::remove_file(&path);
                Ok(None)
            }
            Err(err) => Err(err),
        }
    }
}

fn build_default_provider() -> Arc<dyn SecretProvider> {
    Arc::new(CocoonFileSecretProvider)
}

static SECRET_PROVIDER: OnceLock<RwLock<Arc<dyn SecretProvider>>> = OnceLock::new();

fn provider_lock() -> &'static RwLock<Arc<dyn SecretProvider>> {
    SECRET_PROVIDER.get_or_init(|| RwLock::new(build_default_provider()))
}

fn active_provider() -> Arc<dyn SecretProvider> {
    #[cfg(test)]
    {
        let provider = TEST_SECRET_PROVIDER.with(|cell| cell.borrow().clone());
        if let Some(provider) = provider {
            return provider;
        }
    }

    match provider_lock().read() {
        Ok(guard) => Arc::clone(&guard),
        Err(poisoned) => Arc::clone(poisoned.get_ref()),
    }
}

#[cfg(test)]
thread_local! {
    static TEST_SECRET_PROVIDER: RefCell<Option<Arc<dyn SecretProvider>>> = RefCell::new(None);
}

#[cfg(test)]
fn set_secret_provider_for_tests(provider: Arc<dyn SecretProvider>) {
    TEST_SECRET_PROVIDER.with(|cell| {
        *cell.borrow_mut() = Some(provider);
    });
}

#[cfg(test)]
fn reset_secret_provider_for_tests() {
    TEST_SECRET_PROVIDER.with(|cell| {
        *cell.borrow_mut() = None;
    });
}

pub fn set_secret(name: &str, value: &str) -> Result<()> {
    set_secret_internal(name, value, true)
}

pub fn set_secret_required(name: &str, value: &str) -> Result<()> {
    set_secret_internal(name, value, false)
}

fn set_secret_internal(name: &str, value: &str, allow_backend_unavailable: bool) -> Result<()> {
    active_provider().set_secret(name, value, allow_backend_unavailable)
}

pub fn get_secret(name: &str) -> Result<Option<String>> {
    active_provider().get_secret(name)
}

pub fn ensure_daemon_auth_token() -> Result<String> {
    if let Some(token) = get_secret("daemon_auth_token")? {
        let trimmed = token.trim().to_string();
        if !trimmed.is_empty() {
            return Ok(trimmed);
        }
    }

    let mut bytes = [0u8; 32];
    let mut rng = SysRng;
    rng.try_fill_bytes(&mut bytes)
        .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
    let generated = URL_SAFE_NO_PAD.encode(bytes);
    set_secret_required("daemon_auth_token", &generated)?;
    Ok(generated)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Mutex, OnceLock};

    fn test_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    struct MockSecretProvider {
        values: Mutex<HashMap<String, String>>,
        set_calls: AtomicUsize,
    }

    impl MockSecretProvider {
        fn new() -> Self {
            Self {
                values: Mutex::new(HashMap::new()),
                set_calls: AtomicUsize::new(0),
            }
        }

        fn set_call_count(&self) -> usize {
            self.set_calls.load(Ordering::Relaxed)
        }
    }

    impl SecretProvider for MockSecretProvider {
        fn set_secret(
            &self,
            name: &str,
            value: &str,
            _allow_backend_unavailable: bool,
        ) -> Result<()> {
            self.set_calls.fetch_add(1, Ordering::Relaxed);
            let mut guard = self
                .values
                .lock()
                .map_err(|_| ButterflyBotError::Runtime("mock mutex poisoned".to_string()))?;
            guard.insert(name.to_string(), value.to_string());
            Ok(())
        }

        fn get_secret(&self, name: &str) -> Result<Option<String>> {
            let guard = self
                .values
                .lock()
                .map_err(|_| ButterflyBotError::Runtime("mock mutex poisoned".to_string()))?;
            Ok(guard.get(name).cloned())
        }
    }

    #[test]
    fn secret_provider_roundtrip_through_public_api() {
        let _guard = test_lock().lock().expect("test lock poisoned");
        let mock = Arc::new(MockSecretProvider::new());
        set_secret_provider_for_tests(mock.clone());

        set_secret("phase_a_test", "value-123").unwrap();
        let loaded = get_secret("phase_a_test").unwrap();

        assert_eq!(loaded.as_deref(), Some("value-123"));
        assert_eq!(mock.set_call_count(), 1);

        reset_secret_provider_for_tests();
    }

    #[test]
    fn ensure_daemon_auth_token_uses_provider_and_env_cache() {
        let _guard = test_lock().lock().expect("test lock poisoned");
        crate::security::tpm_provider::set_tpm_available_for_tests(Some(true));
        let mock = Arc::new(MockSecretProvider::new());
        set_secret_provider_for_tests(mock.clone());

        let first = ensure_daemon_auth_token().unwrap();
        let second = ensure_daemon_auth_token().unwrap();

        assert_eq!(first, second);
        assert!(first.len() >= 40);
        assert_eq!(mock.set_call_count(), 1);

        reset_secret_provider_for_tests();
        crate::security::tpm_provider::set_tpm_available_for_tests(None);
    }

    #[test]
    fn cocoon_provider_roundtrip() {
        let _guard = test_lock().lock().expect("test lock poisoned");
        crate::security::tpm_provider::set_tpm_available_for_tests(Some(true));
        crate::security::tpm_provider::set_dek_passphrase_for_tests(Some(
            "vault-test-dek-passphrase".to_string(),
        ));
        let temp = tempfile::tempdir().unwrap();

        crate::runtime_paths::set_app_root_override_for_tests(Some(temp.path().to_path_buf()));
        set_secret_provider_for_tests(Arc::new(CocoonFileSecretProvider));

        set_secret("cocoon_secret", "encrypted-value").unwrap();
        let loaded = get_secret("cocoon_secret").unwrap();

        assert_eq!(loaded.as_deref(), Some("encrypted-value"));
        assert!(temp
            .path()
            .join("secrets")
            .join("cocoon_secret.cocoon")
            .exists());

        reset_secret_provider_for_tests();
        crate::runtime_paths::set_app_root_override_for_tests(None);
        crate::security::tpm_provider::set_dek_passphrase_for_tests(None);
        crate::security::tpm_provider::set_tpm_available_for_tests(None);
    }

    #[test]
    fn cocoon_provider_generates_master_key_when_missing() {
        let _guard = test_lock().lock().expect("test lock poisoned");
        crate::security::tpm_provider::set_tpm_available_for_tests(Some(true));
        crate::security::tpm_provider::set_dek_passphrase_for_tests(Some(
            "vault-test-dek-passphrase-2".to_string(),
        ));
        let temp = tempfile::tempdir().unwrap();

        crate::runtime_paths::set_app_root_override_for_tests(Some(temp.path().to_path_buf()));
        set_secret_provider_for_tests(Arc::new(CocoonFileSecretProvider));

        set_secret("cocoon_secret_master_key", "value").unwrap();
        let loaded = get_secret("cocoon_secret_master_key").unwrap();
        assert_eq!(loaded.as_deref(), Some("value"));

        reset_secret_provider_for_tests();
        crate::runtime_paths::set_app_root_override_for_tests(None);
        crate::security::tpm_provider::set_dek_passphrase_for_tests(None);
        crate::security::tpm_provider::set_tpm_available_for_tests(None);
    }

    #[test]
    fn cocoon_provider_empty_set_clears_secret_file() {
        let _guard = test_lock().lock().expect("test lock poisoned");
        crate::security::tpm_provider::set_tpm_available_for_tests(Some(true));
        crate::security::tpm_provider::set_dek_passphrase_for_tests(Some(
            "vault-test-dek-passphrase-empty-clear".to_string(),
        ));
        let temp = tempfile::tempdir().unwrap();

        crate::runtime_paths::set_app_root_override_for_tests(Some(temp.path().to_path_buf()));
        set_secret_provider_for_tests(Arc::new(CocoonFileSecretProvider));

        set_secret("github_pat", "token-value").unwrap();
        let path = temp.path().join("secrets").join("github_pat.cocoon");
        assert!(path.exists());

        set_secret("github_pat", "").unwrap();
        assert!(!path.exists());
        assert!(get_secret("github_pat").unwrap().is_none());

        reset_secret_provider_for_tests();
        crate::runtime_paths::set_app_root_override_for_tests(None);
        crate::security::tpm_provider::set_dek_passphrase_for_tests(None);
        crate::security::tpm_provider::set_tpm_available_for_tests(None);
    }

    #[test]
    fn cocoon_provider_self_heals_legacy_empty_encrypted_secret() {
        let _guard = test_lock().lock().expect("test lock poisoned");
        crate::security::tpm_provider::set_tpm_available_for_tests(Some(true));
        crate::security::tpm_provider::set_dek_passphrase_for_tests(Some(
            "vault-test-dek-passphrase-empty-heal".to_string(),
        ));
        let temp = tempfile::tempdir().unwrap();

        crate::runtime_paths::set_app_root_override_for_tests(Some(temp.path().to_path_buf()));
        set_secret_provider_for_tests(Arc::new(CocoonFileSecretProvider));

        let path = temp.path().join("secrets").join("github_pat.cocoon");
        crate::security::hardening::with_sensitive_string(
            "vault-test-dek-passphrase-empty-heal".to_string(),
            |sensitive| crate::security::cocoon_store::persist_secret(&path, sensitive, ""),
        )
        .unwrap();
        assert!(path.exists());

        let loaded = get_secret("github_pat").unwrap();
        assert!(loaded.is_none());
        assert!(!path.exists());

        reset_secret_provider_for_tests();
        crate::runtime_paths::set_app_root_override_for_tests(None);
        crate::security::tpm_provider::set_dek_passphrase_for_tests(None);
        crate::security::tpm_provider::set_tpm_available_for_tests(None);
    }

    #[test]
    fn cocoon_provider_fails_fast_without_tpm() {
        let _guard = test_lock().lock().expect("test lock poisoned");
        crate::security::tpm_provider::set_tpm_available_for_tests(Some(false));
        let temp = tempfile::tempdir().unwrap();

        crate::runtime_paths::set_app_root_override_for_tests(Some(temp.path().to_path_buf()));
        set_secret_provider_for_tests(Arc::new(CocoonFileSecretProvider));

        let err = set_secret("no_tpm", "value").unwrap_err();
        assert!(format!("{err}").contains("TPM is required"));

        reset_secret_provider_for_tests();
        crate::runtime_paths::set_app_root_override_for_tests(None);
        crate::security::tpm_provider::set_tpm_available_for_tests(None);
    }
}
