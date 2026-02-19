use crate::error::{ButterflyBotError, Result};
use crate::security::cocoon_store;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::rngs::SysRng;
use rand::TryRng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};

#[cfg(test)]
use std::sync::{OnceLock, RwLock};

#[cfg(all(debug_assertions, not(test)))]
use std::sync::{OnceLock as DebugOnceLock, RwLock as DebugRwLock};

const SERVICE: &str = "butterfly-bot";
const TPM_KEK_NAME: &str = "tpm_kek";
#[cfg(any(target_os = "macos", target_os = "windows"))]
const PLATFORM_BINDING_NAME: &str = "platform_secure_binding";
const POLICY_VERSION: u8 = 1;

#[cfg(all(debug_assertions, not(test)))]
fn debug_available_override_lock() -> &'static DebugRwLock<Option<bool>> {
    static OVERRIDE: DebugOnceLock<DebugRwLock<Option<bool>>> = DebugOnceLock::new();
    OVERRIDE.get_or_init(|| DebugRwLock::new(None))
}

#[cfg(all(debug_assertions, not(test)))]
fn debug_dek_override_lock() -> &'static DebugRwLock<Option<String>> {
    static OVERRIDE: DebugOnceLock<DebugRwLock<Option<String>>> = DebugOnceLock::new();
    OVERRIDE.get_or_init(|| DebugRwLock::new(None))
}

#[cfg(all(debug_assertions, not(test)))]
pub fn set_debug_tpm_available_override(value: Option<bool>) {
    let lock = debug_available_override_lock();
    match lock.write() {
        Ok(mut guard) => *guard = value,
        Err(poisoned) => {
            let mut guard = poisoned.into_inner();
            *guard = value;
        }
    }
}

#[cfg(all(debug_assertions, not(test)))]
pub fn set_debug_dek_passphrase_override(value: Option<String>) {
    let lock = debug_dek_override_lock();
    match lock.write() {
        Ok(mut guard) => *guard = value,
        Err(poisoned) => {
            let mut guard = poisoned.into_inner();
            *guard = value;
        }
    }
}

#[cfg(all(debug_assertions, not(test)))]
fn debug_tpm_available_override() -> Option<bool> {
    let lock = debug_available_override_lock();
    match lock.read() {
        Ok(guard) => *guard,
        Err(poisoned) => *poisoned.into_inner(),
    }
}

#[cfg(all(debug_assertions, not(test)))]
fn debug_dek_override() -> Option<String> {
    let lock = debug_dek_override_lock();
    match lock.read() {
        Ok(guard) => guard.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    }
}

#[cfg(test)]
fn available_override_lock() -> &'static RwLock<Option<bool>> {
    static OVERRIDE: OnceLock<RwLock<Option<bool>>> = OnceLock::new();
    OVERRIDE.get_or_init(|| RwLock::new(None))
}

#[cfg(test)]
fn dek_override_lock() -> &'static RwLock<Option<String>> {
    static OVERRIDE: OnceLock<RwLock<Option<String>>> = OnceLock::new();
    OVERRIDE.get_or_init(|| RwLock::new(None))
}

#[cfg(test)]
pub(crate) fn set_tpm_available_for_tests(value: Option<bool>) {
    let lock = available_override_lock();
    match lock.write() {
        Ok(mut guard) => *guard = value,
        Err(poisoned) => {
            let mut guard = poisoned.into_inner();
            *guard = value;
        }
    }
}

#[cfg(test)]
pub(crate) fn set_dek_passphrase_for_tests(value: Option<String>) {
    let lock = dek_override_lock();
    match lock.write() {
        Ok(mut guard) => *guard = value,
        Err(poisoned) => {
            let mut guard = poisoned.into_inner();
            *guard = value;
        }
    }
}

#[cfg(test)]
fn tpm_available_override() -> Option<bool> {
    let lock = available_override_lock();
    match lock.read() {
        Ok(guard) => *guard,
        Err(poisoned) => *poisoned.into_inner(),
    }
}

#[cfg(test)]
fn dek_override() -> Option<String> {
    let lock = dek_override_lock();
    match lock.read() {
        Ok(guard) => guard.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KeyLifecycleState {
    Provision,
    Seal,
    Unseal,
    Use,
    Rotate,
    Revoke,
    Recover,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct TpmPolicyState {
    version: u8,
    fingerprint: String,
    lifecycle_state: KeyLifecycleState,
}

trait TpmBackend {
    fn is_present(&self) -> bool;
    fn fingerprint(&self) -> Result<String>;
}

trait KekStore {
    fn get_kek(&self) -> Result<Option<String>>;
    fn set_kek(&self, value: &str) -> Result<()>;
    fn clear_kek(&self) -> Result<()>;
}

struct DeviceTpmBackend;

impl DeviceTpmBackend {
    fn active_device_path(&self) -> Option<&'static Path> {
        if Path::new("/dev/tpmrm0").exists() {
            return Some(Path::new("/dev/tpmrm0"));
        }
        if Path::new("/dev/tpm0").exists() {
            return Some(Path::new("/dev/tpm0"));
        }
        None
    }
}

#[cfg(target_os = "macos")]
struct AppleSecureBackend;

#[cfg(target_os = "macos")]
impl AppleSecureBackend {
    fn keychain_entry() -> Result<keyring::Entry> {
        keyring::Entry::new(SERVICE, PLATFORM_BINDING_NAME)
            .map_err(|e| ButterflyBotError::SecurityStorage(e.to_string()))
    }

    fn ensure_binding_id(&self) -> Result<String> {
        let entry = Self::keychain_entry()?;
        match entry.get_password() {
            Ok(value) => {
                let trimmed = value.trim().to_string();
                if trimmed.is_empty() {
                    let generated = TpmRuntime::<DeviceTpmBackend, KeyringKekStore>::random_secret()?;
                    entry
                        .set_password(&generated)
                        .map_err(|e| ButterflyBotError::SecurityStorage(e.to_string()))?;
                    Ok(generated)
                } else {
                    Ok(trimmed)
                }
            }
            Err(keyring::Error::NoEntry) => {
                let generated = TpmRuntime::<DeviceTpmBackend, KeyringKekStore>::random_secret()?;
                entry
                    .set_password(&generated)
                    .map_err(|e| ButterflyBotError::SecurityStorage(e.to_string()))?;
                Ok(generated)
            }
            Err(err) => Err(ButterflyBotError::SecurityStorage(err.to_string())),
        }
    }
}

#[cfg(target_os = "macos")]
impl TpmBackend for AppleSecureBackend {
    fn is_present(&self) -> bool {
        Self::keychain_entry().is_ok()
    }

    fn fingerprint(&self) -> Result<String> {
        let mut hasher = Sha256::new();
        hasher.update(b"darwin-keychain-secure-backend");
        hasher.update(self.ensure_binding_id()?.as_bytes());
        Ok(format!("{:x}", hasher.finalize()))
    }
}

#[cfg(target_os = "windows")]
struct WindowsSecureBackend;

#[cfg(target_os = "windows")]
impl WindowsSecureBackend {
    fn keychain_entry() -> Result<keyring::Entry> {
        keyring::Entry::new(SERVICE, PLATFORM_BINDING_NAME)
            .map_err(|e| ButterflyBotError::SecurityStorage(e.to_string()))
    }

    fn ensure_binding_id(&self) -> Result<String> {
        let entry = Self::keychain_entry()?;
        match entry.get_password() {
            Ok(value) => {
                let trimmed = value.trim().to_string();
                if trimmed.is_empty() {
                    let generated =
                        TpmRuntime::<DeviceTpmBackend, KeyringKekStore>::random_secret()?;
                    entry
                        .set_password(&generated)
                        .map_err(|e| ButterflyBotError::SecurityStorage(e.to_string()))?;
                    Ok(generated)
                } else {
                    Ok(trimmed)
                }
            }
            Err(keyring::Error::NoEntry) => {
                let generated = TpmRuntime::<DeviceTpmBackend, KeyringKekStore>::random_secret()?;
                entry
                    .set_password(&generated)
                    .map_err(|e| ButterflyBotError::SecurityStorage(e.to_string()))?;
                Ok(generated)
            }
            Err(err) => Err(ButterflyBotError::SecurityStorage(err.to_string())),
        }
    }
}

#[cfg(target_os = "windows")]
impl TpmBackend for WindowsSecureBackend {
    fn is_present(&self) -> bool {
        Self::keychain_entry().is_ok()
    }

    fn fingerprint(&self) -> Result<String> {
        let mut hasher = Sha256::new();
        hasher.update(b"windows-keyring-secure-backend");
        hasher.update(self.ensure_binding_id()?.as_bytes());
        Ok(format!("{:x}", hasher.finalize()))
    }
}

impl TpmBackend for DeviceTpmBackend {
    fn is_present(&self) -> bool {
        #[cfg(target_os = "linux")]
        {
            self.active_device_path().is_some()
        }
        #[cfg(not(target_os = "linux"))]
        {
            false
        }
    }

    fn fingerprint(&self) -> Result<String> {
        let path = self.active_device_path().ok_or_else(|| {
            ButterflyBotError::SecurityPolicy(
                "TPM is required in strict mode; no TPM device found".to_string(),
            )
        })?;

        let mut hasher = Sha256::new();
        hasher.update(path.to_string_lossy().as_bytes());

        if let Ok(meta) = std::fs::metadata(path) {
            hasher.update(meta.len().to_le_bytes());
            if let Ok(modified) = meta.modified() {
                if let Ok(duration) = modified.duration_since(std::time::UNIX_EPOCH) {
                    hasher.update(duration.as_secs().to_le_bytes());
                }
            }
        }

        if let Ok(uevent) = std::fs::read("/sys/class/tpm/tpm0/device/uevent") {
            hasher.update(uevent);
        }

        Ok(format!("{:x}", hasher.finalize()))
    }
}

struct KeyringKekStore;

impl KekStore for KeyringKekStore {
    fn get_kek(&self) -> Result<Option<String>> {
        let entry = keyring::Entry::new(SERVICE, TPM_KEK_NAME)
            .map_err(|e| ButterflyBotError::SecurityStorage(e.to_string()))?;
        match entry.get_password() {
            Ok(value) => {
                let trimmed = value.trim().to_string();
                if trimmed.is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(trimmed))
                }
            }
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => Err(ButterflyBotError::SecurityStorage(err.to_string())),
        }
    }

    fn set_kek(&self, value: &str) -> Result<()> {
        let entry = keyring::Entry::new(SERVICE, TPM_KEK_NAME)
            .map_err(|e| ButterflyBotError::SecurityStorage(e.to_string()))?;
        entry
            .set_password(value)
            .map_err(|e| ButterflyBotError::SecurityStorage(e.to_string()))
    }

    fn clear_kek(&self) -> Result<()> {
        let entry = keyring::Entry::new(SERVICE, TPM_KEK_NAME)
            .map_err(|e| ButterflyBotError::SecurityStorage(e.to_string()))?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(ButterflyBotError::SecurityStorage(err.to_string())),
        }
    }
}

struct TpmRuntime<'a, B: TpmBackend, K: KekStore> {
    backend: &'a B,
    kek_store: &'a K,
    security_root: PathBuf,
}

impl<'a, B: TpmBackend, K: KekStore> TpmRuntime<'a, B, K> {
    fn policy_state_path(&self) -> PathBuf {
        self.security_root.join("tpm_policy_state.json")
    }

    fn wrapped_dek_path(&self) -> PathBuf {
        self.security_root.join("wrapped_dek.cocoon")
    }

    fn ensure_root(&self) -> Result<()> {
        std::fs::create_dir_all(&self.security_root).map_err(|e| {
            ButterflyBotError::SecurityStorage(format!(
                "failed to create security root {}: {e}",
                self.security_root.to_string_lossy()
            ))
        })
    }

    fn load_policy_state(&self) -> Result<Option<TpmPolicyState>> {
        let path = self.policy_state_path();
        if !path.exists() {
            return Ok(None);
        }
        let raw = std::fs::read_to_string(&path).map_err(|e| {
            ButterflyBotError::SecurityStorage(format!(
                "failed to read TPM policy state {}: {e}",
                path.to_string_lossy()
            ))
        })?;
        let state: TpmPolicyState = serde_json::from_str(&raw).map_err(|e| {
            ButterflyBotError::SecurityStorage(format!(
                "failed to parse TPM policy state {}: {e}",
                path.to_string_lossy()
            ))
        })?;
        Ok(Some(state))
    }

    fn save_policy_state(&self, state: TpmPolicyState) -> Result<()> {
        self.ensure_root()?;
        let path = self.policy_state_path();
        let payload = serde_json::to_string_pretty(&state)
            .map_err(|e| ButterflyBotError::SecurityStorage(e.to_string()))?;
        std::fs::write(&path, payload).map_err(|e| {
            ButterflyBotError::SecurityStorage(format!(
                "failed to write TPM policy state {}: {e}",
                path.to_string_lossy()
            ))
        })
    }

    fn random_secret() -> Result<String> {
        let mut bytes = [0u8; 32];
        let mut rng = SysRng;
        rng.try_fill_bytes(&mut bytes)
            .map_err(|e| ButterflyBotError::Runtime(e.to_string()))?;
        Ok(URL_SAFE_NO_PAD.encode(bytes))
    }

    fn lockout_like(err: &ButterflyBotError) -> bool {
        let lowered = err.to_string().to_ascii_lowercase();
        lowered.contains("lockout") || lowered.contains("locked") || lowered.contains("auth")
    }

    fn missing_tpm_error() -> ButterflyBotError {
        ButterflyBotError::SecurityPolicy(
            "TPM is required in strict mode; no TPM device found".to_string(),
        )
    }

    fn reset_error(detail: &str) -> ButterflyBotError {
        ButterflyBotError::SecurityPolicy(format!(
            "TPM reset or reprovision detected ({detail}). Recovery runbook: {}",
            recovery_runbook()
        ))
    }

    fn policy_mismatch_error() -> ButterflyBotError {
        ButterflyBotError::SecurityPolicy(format!(
            "TPM policy mismatch detected. Recovery runbook: {}",
            recovery_runbook()
        ))
    }

    fn lockout_error() -> ButterflyBotError {
        ButterflyBotError::SecurityPolicy(format!(
            "TPM lockout/auth failure detected. Recovery runbook: {}",
            recovery_runbook()
        ))
    }

    fn require_present(&self) -> Result<()> {
        if self.backend.is_present() {
            Ok(())
        } else {
            Err(Self::missing_tpm_error())
        }
    }

    fn verify_policy_fingerprint(&self, expected: &str) -> Result<()> {
        let current = self.backend.fingerprint()?;
        if current == expected {
            Ok(())
        } else {
            Err(Self::policy_mismatch_error())
        }
    }

    fn load_unsealed_dek(&self, kek: &str) -> Result<String> {
        let wrapped = self.wrapped_dek_path();
        let decoded = cocoon_store::load_secret(&wrapped, kek)
            .map_err(|_| Self::reset_error("wrapped DEK decryption failed"))?;
        decoded.ok_or_else(|| Self::reset_error("wrapped DEK missing"))
    }

    fn persist_wrapped_dek(&self, kek: &str, dek: &str) -> Result<()> {
        self.ensure_root()?;
        cocoon_store::persist_secret(&self.wrapped_dek_path(), kek, dek)
    }

    fn provision(&self) -> Result<()> {
        self.require_present()?;
        let fingerprint = self.backend.fingerprint()?;
        let state = self.load_policy_state()?;

        if let Some(state) = state {
            self.verify_policy_fingerprint(&state.fingerprint)?;
            let kek = self.kek_store.get_kek().map_err(|e| {
                if Self::lockout_like(&e) {
                    Self::lockout_error()
                } else {
                    e
                }
            })?;
            let kek = kek.ok_or_else(|| Self::reset_error("KEK missing from TPM key store"))?;
            let _ = self.load_unsealed_dek(&kek)?;
            return Ok(());
        }

        let kek = Self::random_secret()?;
        self.kek_store.set_kek(&kek).map_err(|e| {
            if Self::lockout_like(&e) {
                Self::lockout_error()
            } else {
                e
            }
        })?;
        let dek = Self::random_secret()?;
        self.persist_wrapped_dek(&kek, &dek)?;
        self.save_policy_state(TpmPolicyState {
            version: POLICY_VERSION,
            fingerprint,
            lifecycle_state: KeyLifecycleState::Seal,
        })?;
        Ok(())
    }

    fn unseal_dek(&self) -> Result<String> {
        self.require_present()?;
        self.provision()?;

        let state = self
            .load_policy_state()?
            .ok_or_else(|| Self::reset_error("TPM policy state missing"))?;
        self.verify_policy_fingerprint(&state.fingerprint)?;

        let kek = self.kek_store.get_kek().map_err(|e| {
            if Self::lockout_like(&e) {
                Self::lockout_error()
            } else {
                e
            }
        })?;
        let kek = kek.ok_or_else(|| Self::reset_error("KEK missing from TPM key store"))?;
        let dek = self.load_unsealed_dek(&kek)?;

        self.save_policy_state(TpmPolicyState {
            version: POLICY_VERSION,
            fingerprint: state.fingerprint,
            lifecycle_state: KeyLifecycleState::Use,
        })?;

        Ok(dek)
    }

    fn rotate_dek(&self) -> Result<()> {
        self.require_present()?;
        let state = self
            .load_policy_state()?
            .ok_or_else(|| Self::reset_error("TPM policy state missing"))?;
        self.verify_policy_fingerprint(&state.fingerprint)?;
        let kek = self
            .kek_store
            .get_kek()?
            .ok_or_else(|| Self::reset_error("KEK missing from TPM key store"))?;
        let new_dek = Self::random_secret()?;
        self.persist_wrapped_dek(&kek, &new_dek)?;
        self.save_policy_state(TpmPolicyState {
            version: POLICY_VERSION,
            fingerprint: state.fingerprint,
            lifecycle_state: KeyLifecycleState::Rotate,
        })
    }

    fn revoke(&self) -> Result<()> {
        let _ = std::fs::remove_file(self.wrapped_dek_path());
        let _ = std::fs::remove_file(self.policy_state_path());
        self.kek_store.clear_kek()?;
        Ok(())
    }
}

fn runtime_root() -> PathBuf {
    crate::runtime_paths::app_root().join("security")
}

#[cfg(target_os = "linux")]
fn production_runtime() -> TpmRuntime<'static, DeviceTpmBackend, KeyringKekStore> {
    static BACKEND: DeviceTpmBackend = DeviceTpmBackend;
    static STORE: KeyringKekStore = KeyringKekStore;
    TpmRuntime {
        backend: &BACKEND,
        kek_store: &STORE,
        security_root: runtime_root(),
    }
}

#[cfg(target_os = "macos")]
fn production_runtime() -> TpmRuntime<'static, AppleSecureBackend, KeyringKekStore> {
    static BACKEND: AppleSecureBackend = AppleSecureBackend;
    static STORE: KeyringKekStore = KeyringKekStore;
    TpmRuntime {
        backend: &BACKEND,
        kek_store: &STORE,
        security_root: runtime_root(),
    }
}

#[cfg(target_os = "windows")]
fn production_runtime() -> TpmRuntime<'static, WindowsSecureBackend, KeyringKekStore> {
    static BACKEND: WindowsSecureBackend = WindowsSecureBackend;
    static STORE: KeyringKekStore = KeyringKekStore;
    TpmRuntime {
        backend: &BACKEND,
        kek_store: &STORE,
        security_root: runtime_root(),
    }
}

#[cfg(not(any(target_os = "linux", target_os = "macos", target_os = "windows")))]
fn production_runtime() -> TpmRuntime<'static, DeviceTpmBackend, KeyringKekStore> {
    static BACKEND: DeviceTpmBackend = DeviceTpmBackend;
    static STORE: KeyringKekStore = KeyringKekStore;
    TpmRuntime {
        backend: &BACKEND,
        kek_store: &STORE,
        security_root: runtime_root(),
    }
}

pub fn require_tpm() -> Result<()> {
    #[cfg(all(debug_assertions, not(test)))]
    {
        if let Some(value) = debug_tpm_available_override() {
            return if value {
                Ok(())
            } else {
                Err(ButterflyBotError::SecurityPolicy(
                    "TPM is required in strict mode; no TPM device found".to_string(),
                ))
            };
        }
    }

    #[cfg(test)]
    {
        if let Some(value) = tpm_available_override() {
            return if value {
                Ok(())
            } else {
                Err(ButterflyBotError::SecurityPolicy(
                    "TPM is required in strict mode; no TPM device found".to_string(),
                ))
            };
        }
    }

    production_runtime().require_present()
}

pub fn resolve_dek_passphrase() -> Result<String> {
    #[cfg(all(debug_assertions, not(test)))]
    {
        if let Some(value) = debug_dek_override() {
            return Ok(value);
        }
    }

    #[cfg(test)]
    {
        if let Some(value) = dek_override() {
            return Ok(value);
        }
    }

    production_runtime().unseal_dek()
}

pub fn provision_kek_and_dek() -> Result<()> {
    production_runtime().provision()
}

pub fn rotate_dek() -> Result<()> {
    production_runtime().rotate_dek()
}

pub fn revoke_keys() -> Result<()> {
    production_runtime().revoke()
}

pub fn recovery_runbook() -> &'static str {
    "1) stop the daemon and UI, 2) verify TPM device presence and ownership, 3) if TPM was reset/reprovisioned, run migration/recovery path and reprovision keys, 4) restore secrets from trusted backup, 5) restart in strict mode and verify checks"
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Clone)]
    struct MemoryTpmBackend {
        present: bool,
        fingerprint: String,
    }

    impl TpmBackend for MemoryTpmBackend {
        fn is_present(&self) -> bool {
            self.present
        }

        fn fingerprint(&self) -> Result<String> {
            Ok(self.fingerprint.clone())
        }
    }

    #[derive(Default)]
    struct MemoryKekStore {
        value: Mutex<Option<String>>,
    }

    impl KekStore for MemoryKekStore {
        fn get_kek(&self) -> Result<Option<String>> {
            self.value
                .lock()
                .map(|guard| guard.clone())
                .map_err(|_| ButterflyBotError::Runtime("kek lock poisoned".to_string()))
        }

        fn set_kek(&self, value: &str) -> Result<()> {
            let mut guard = self
                .value
                .lock()
                .map_err(|_| ButterflyBotError::Runtime("kek lock poisoned".to_string()))?;
            *guard = Some(value.to_string());
            Ok(())
        }

        fn clear_kek(&self) -> Result<()> {
            let mut guard = self
                .value
                .lock()
                .map_err(|_| ButterflyBotError::Runtime("kek lock poisoned".to_string()))?;
            *guard = None;
            Ok(())
        }
    }

    fn runtime<'a>(
        backend: &'a MemoryTpmBackend,
        store: &'a MemoryKekStore,
        root: &Path,
    ) -> TpmRuntime<'a, MemoryTpmBackend, MemoryKekStore> {
        TpmRuntime {
            backend,
            kek_store: store,
            security_root: root.to_path_buf(),
        }
    }

    #[test]
    fn tpm_present_provision_and_unseal_succeeds() {
        let temp = tempfile::tempdir().unwrap();
        let backend = MemoryTpmBackend {
            present: true,
            fingerprint: "fp-a".to_string(),
        };
        let store = MemoryKekStore::default();
        let runtime = runtime(&backend, &store, temp.path());

        runtime.provision().unwrap();
        let dek = runtime.unseal_dek().unwrap();

        assert!(!dek.trim().is_empty());
    }

    #[test]
    fn tpm_missing_fails_closed() {
        let temp = tempfile::tempdir().unwrap();
        let backend = MemoryTpmBackend {
            present: false,
            fingerprint: "fp-a".to_string(),
        };
        let store = MemoryKekStore::default();
        let runtime = runtime(&backend, &store, temp.path());

        let err = runtime.provision().unwrap_err();
        assert!(format!("{err}").contains("TPM is required"));
    }

    #[test]
    fn tpm_reset_detected_when_kek_missing_after_provision() {
        let temp = tempfile::tempdir().unwrap();
        let backend = MemoryTpmBackend {
            present: true,
            fingerprint: "fp-a".to_string(),
        };
        let store = MemoryKekStore::default();
        let runtime = runtime(&backend, &store, temp.path());

        runtime.provision().unwrap();
        store.clear_kek().unwrap();

        let err = runtime.unseal_dek().unwrap_err();
        assert!(format!("{err}").contains("reset or reprovision"));
    }

    #[test]
    fn tpm_policy_mismatch_fails_closed() {
        let temp = tempfile::tempdir().unwrap();
        let backend_a = MemoryTpmBackend {
            present: true,
            fingerprint: "fp-a".to_string(),
        };
        let store = MemoryKekStore::default();
        let runtime_a = runtime(&backend_a, &store, temp.path());
        runtime_a.provision().unwrap();

        let backend_b = MemoryTpmBackend {
            present: true,
            fingerprint: "fp-b".to_string(),
        };
        let runtime_b = runtime(&backend_b, &store, temp.path());
        let err = runtime_b.unseal_dek().unwrap_err();
        assert!(format!("{err}").contains("policy mismatch"));
    }
}
