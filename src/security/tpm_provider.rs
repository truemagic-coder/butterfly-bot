use crate::error::{ButterflyBotError, Result};
use crate::security::cocoon_store;
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine as _};
use rand::rngs::SysRng;
use rand::TryRng;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

#[cfg(test)]
use std::path::Path;

#[cfg(test)]
use std::cell::RefCell;

#[cfg(test)]
use std::thread_local;

#[cfg(all(debug_assertions, not(test)))]
use std::sync::{OnceLock as DebugOnceLock, RwLock as DebugRwLock};

const SERVICE: &str = "butterfly-bot";
const TPM_KEK_NAME: &str = "tpm_kek";
#[cfg(not(test))]
const COMPAT_KEK_NAME: &str = "compat_kek";
const TPM_MODE_ENV: &str = "BUTTERFLY_TPM_MODE";
#[cfg(any(target_os = "macos", target_os = "windows", not(test)))]
const PLATFORM_BINDING_NAME: &str = "platform_secure_binding";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TpmMode {
    Strict,
    Auto,
    Compatible,
}

impl TpmMode {
    fn as_str(self) -> &'static str {
        match self {
            Self::Strict => "strict",
            Self::Auto => "auto",
            Self::Compatible => "compatible",
        }
    }
}

fn parse_tpm_mode(value: Option<&str>) -> TpmMode {
    match value.unwrap_or("auto").trim().to_ascii_lowercase().as_str() {
        "strict" => TpmMode::Strict,
        "compatible" => TpmMode::Compatible,
        _ => TpmMode::Auto,
    }
}

fn configured_tpm_mode() -> TpmMode {
    parse_tpm_mode(std::env::var(TPM_MODE_ENV).ok().as_deref())
}

#[cfg(not(test))]
fn is_missing_tpm_policy(err: &ButterflyBotError) -> bool {
    let lowered = err.to_string().to_ascii_lowercase();
    lowered.contains("tpm is required") || lowered.contains("no tpm")
}

fn should_fallback_to_compatible(err: &ButterflyBotError) -> bool {
    match err {
        ButterflyBotError::SecurityPolicy(_) | ButterflyBotError::SecurityStorage(_) => true,
        ButterflyBotError::Runtime(_) => {
            let lowered = err.to_string().to_ascii_lowercase();
            lowered.contains("tpm") || lowered.contains("keyring") || lowered.contains("secure")
        }
        _ => false,
    }
}

#[cfg(not(test))]
struct CompatibleBackend;

#[cfg(not(test))]
impl CompatibleBackend {
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

#[cfg(not(test))]
impl TpmBackend for CompatibleBackend {
    fn is_present(&self) -> bool {
        Self::keychain_entry().is_ok()
    }

    fn fingerprint(&self) -> Result<String> {
        let mut hasher = Sha256::new();
        hasher.update(b"compatible-keyring-backend");
        hasher.update(self.ensure_binding_id()?.as_bytes());
        Ok(format!("{:x}", hasher.finalize()))
    }
}

const POLICY_VERSION: u8 = 2;

fn missing_tpm_policy_message() -> String {
    let base = "TPM is required in strict mode; no TPM device found";

    #[cfg(target_os = "linux")]
    {
        let dev_nodes = list_tpm_nodes_under(PathBuf::from("/dev"));
        let sys_nodes = list_tpm_nodes_under(PathBuf::from("/sys/class/tpm"));

        if !dev_nodes.is_empty() {
            return format!(
                "{base}; detected TPM-like nodes in /dev: {}",
                dev_nodes.join(", ")
            );
        }

        if !sys_nodes.is_empty() {
            return format!(
                "{base}; sysfs reports TPM entries ({}) but /dev has none. Check kernel TPM modules (e.g. tpm_crb/tpm_tis) and udev permissions.",
                sys_nodes.join(", ")
            );
        }

        format!(
            "{base}; probe found no /dev/tpm* or /sys/class/tpm/tpm*. Verify TPM/fTPM is enabled in BIOS/UEFI and Linux TPM drivers are loaded."
        )
    }

    #[cfg(not(target_os = "linux"))]
    {
        base.to_string()
    }
}

#[cfg(target_os = "linux")]
fn list_tpm_nodes_under(root: PathBuf) -> Vec<String> {
    let mut nodes: Vec<(u32, String)> = Vec::new();
    if let Ok(entries) = std::fs::read_dir(root) {
        for entry in entries.flatten() {
            let name = match entry.file_name().into_string() {
                Ok(value) => value,
                Err(_) => continue,
            };

            if let Some(index) = DeviceTpmBackend::parse_tpm_index(&name, "tpmrm") {
                nodes.push((index, name));
                continue;
            }

            if let Some(index) = DeviceTpmBackend::parse_tpm_index(&name, "tpm") {
                nodes.push((index, name));
            }
        }
    }

    nodes.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
    nodes.into_iter().map(|(_, name)| name).collect()
}

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
thread_local! {
    static TEST_TPM_AVAILABLE_OVERRIDE: RefCell<Option<bool>> = const { RefCell::new(None) };
    static TEST_DEK_OVERRIDE: RefCell<Option<String>> = const { RefCell::new(None) };
}

#[cfg(test)]
pub(crate) fn set_tpm_available_for_tests(value: Option<bool>) {
    TEST_TPM_AVAILABLE_OVERRIDE.with(|cell| {
        *cell.borrow_mut() = value;
    });
}

#[cfg(test)]
pub(crate) fn set_dek_passphrase_for_tests(value: Option<String>) {
    TEST_DEK_OVERRIDE.with(|cell| {
        *cell.borrow_mut() = value;
    });
}

#[cfg(test)]
fn tpm_available_override() -> Option<bool> {
    TEST_TPM_AVAILABLE_OVERRIDE.with(|cell| *cell.borrow())
}

#[cfg(test)]
fn dek_override() -> Option<String> {
    TEST_DEK_OVERRIDE.with(|cell| cell.borrow().clone())
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
    fn parse_tpm_index(name: &str, prefix: &str) -> Option<u32> {
        let suffix = name.strip_prefix(prefix)?;
        if suffix.is_empty() || !suffix.chars().all(|ch| ch.is_ascii_digit()) {
            return None;
        }
        suffix.parse::<u32>().ok()
    }

    #[cfg(target_os = "linux")]
    fn active_device_path(&self) -> Option<PathBuf> {
        let mut rm_nodes: Vec<(u32, PathBuf)> = Vec::new();
        let mut direct_nodes: Vec<(u32, PathBuf)> = Vec::new();

        if let Ok(entries) = std::fs::read_dir("/dev") {
            for entry in entries.flatten() {
                let name = match entry.file_name().into_string() {
                    Ok(value) => value,
                    Err(_) => continue,
                };
                if let Some(index) = Self::parse_tpm_index(&name, "tpmrm") {
                    rm_nodes.push((index, entry.path()));
                    continue;
                }
                if let Some(index) = Self::parse_tpm_index(&name, "tpm") {
                    direct_nodes.push((index, entry.path()));
                }
            }
        }

        rm_nodes.sort_by_key(|(index, _)| *index);
        if let Some((_, path)) = rm_nodes.into_iter().next() {
            return Some(path);
        }

        direct_nodes.sort_by_key(|(index, _)| *index);
        direct_nodes.into_iter().next().map(|(_, path)| path)
    }

    #[cfg(not(target_os = "linux"))]
    fn active_device_path(&self) -> Option<PathBuf> {
        None
    }

    #[cfg(target_os = "linux")]
    fn sysfs_tpm_paths(&self) -> Vec<PathBuf> {
        let mut paths = Vec::new();
        if let Ok(entries) = std::fs::read_dir("/sys/class/tpm") {
            for entry in entries.flatten() {
                let name = match entry.file_name().into_string() {
                    Ok(value) => value,
                    Err(_) => continue,
                };
                if Self::parse_tpm_index(&name, "tpm").is_some() {
                    paths.push(entry.path());
                }
            }
        }
        paths.sort();
        paths
    }

    #[cfg(target_os = "linux")]
    fn sysfs_has_tpm(&self) -> bool {
        !self.sysfs_tpm_paths().is_empty()
    }

    #[cfg(target_os = "linux")]
    fn sysfs_uevent_bytes(&self) -> Option<Vec<u8>> {
        self.sysfs_tpm_paths().into_iter().find_map(|path| {
            let uevent = path.join("device").join("uevent");
            std::fs::read(uevent).ok()
        })
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
            self.active_device_path().is_some() || self.sysfs_has_tpm()
        }
        #[cfg(not(target_os = "linux"))]
        {
            false
        }
    }

    fn fingerprint(&self) -> Result<String> {
        let mut hasher = Sha256::new();

        #[cfg(target_os = "linux")]
        {
            // Prefer sysfs hardware identity because /dev node metadata and selected
            // frontend device path (/dev/tpmrmN vs /dev/tpmN) can change across boots.
            if let Some(uevent) = self.sysfs_uevent_bytes() {
                hasher.update(b"linux-tpm-uevent-v1");
                hasher.update(uevent);
                return Ok(format!("{:x}", hasher.finalize()));
            }

            if let Some(path) = self.active_device_path() {
                if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
                    if let Some(index) = Self::parse_tpm_index(name, "tpmrm") {
                        hasher.update(format!("linux-tpm-index:{index}").as_bytes());
                        return Ok(format!("{:x}", hasher.finalize()));
                    }
                    if let Some(index) = Self::parse_tpm_index(name, "tpm") {
                        hasher.update(format!("linux-tpm-index:{index}").as_bytes());
                        return Ok(format!("{:x}", hasher.finalize()));
                    }
                }
                hasher.update(path.to_string_lossy().as_bytes());
                return Ok(format!("{:x}", hasher.finalize()));
            }

            if self.sysfs_has_tpm() {
                hasher.update(b"linux-tpm-sysfs-present");
                return Ok(format!("{:x}", hasher.finalize()));
            }

            Err(ButterflyBotError::SecurityPolicy(
                missing_tpm_policy_message(),
            ))
        }

        #[cfg(not(target_os = "linux"))]
        {
            let path = if let Some(path) = self.active_device_path() {
                path
            } else {
                return Err(ButterflyBotError::SecurityPolicy(
                    missing_tpm_policy_message(),
                ));
            };

            if path.as_os_str().is_empty() {
                return Err(ButterflyBotError::SecurityPolicy(
                    missing_tpm_policy_message(),
                ));
            }

            hasher.update(path.to_string_lossy().as_bytes());
            return Ok(format!("{:x}", hasher.finalize()));
        }
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

#[cfg(not(test))]
struct KeyringCompatibleKekStore;

#[cfg(not(test))]
impl KekStore for KeyringCompatibleKekStore {
    fn get_kek(&self) -> Result<Option<String>> {
        let entry = keyring::Entry::new(SERVICE, COMPAT_KEK_NAME)
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
        let entry = keyring::Entry::new(SERVICE, COMPAT_KEK_NAME)
            .map_err(|e| ButterflyBotError::SecurityStorage(e.to_string()))?;
        entry
            .set_password(value)
            .map_err(|e| ButterflyBotError::SecurityStorage(e.to_string()))
    }

    fn clear_kek(&self) -> Result<()> {
        let entry = keyring::Entry::new(SERVICE, COMPAT_KEK_NAME)
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

    fn fallback_kek_path(&self) -> PathBuf {
        self.security_root.join("kek_fallback.txt")
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

    fn reset_like(err: &ButterflyBotError) -> bool {
        let lowered = err.to_string().to_ascii_lowercase();
        lowered.contains("tpm reset or reprovision detected")
    }

    fn missing_tpm_error() -> ButterflyBotError {
        ButterflyBotError::SecurityPolicy(missing_tpm_policy_message())
    }

    fn reset_error(detail: &str) -> ButterflyBotError {
        ButterflyBotError::SecurityPolicy(format!(
            "TPM reset or reprovision detected ({detail}). Recovery runbook: {}",
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

    fn load_kek_mapped(&self) -> Result<String> {
        match self.kek_store.get_kek() {
            Ok(Some(value)) => return Ok(value),
            Ok(None) => {}
            Err(err) => {
                if Self::lockout_like(&err) {
                    return Err(Self::lockout_error());
                }
            }
        }

        let fallback = self.fallback_kek_path();
        if let Ok(raw) = std::fs::read_to_string(&fallback) {
            let trimmed = raw.trim().to_string();
            if !trimmed.is_empty() {
                return Ok(trimmed);
            }
        }

        Err(Self::reset_error("KEK missing from TPM key store"))
    }

    fn persist_kek_with_fallback(&self, value: &str) -> Result<()> {
        if let Err(err) = self.kek_store.set_kek(value) {
            if Self::lockout_like(&err) {
                return Err(Self::lockout_error());
            }
        }

        self.ensure_root()?;
        let path = self.fallback_kek_path();
        std::fs::write(&path, value).map_err(|e| {
            ButterflyBotError::SecurityStorage(format!(
                "failed to write fallback KEK {}: {e}",
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

    fn verify_or_migrate_policy_fingerprint(
        &self,
        state: &TpmPolicyState,
        current_fingerprint: &str,
    ) -> Result<()> {
        if state.fingerprint == current_fingerprint {
            return Ok(());
        }

        // Prove continuity by unwrapping with existing KEK before accepting a fingerprint change.
        // This preserves fail-closed behavior for true reset/reprovision events while healing
        // benign platform fingerprint drift for existing installations.
        let kek = self.load_kek_mapped()?;
        let _ = self.load_unsealed_dek(&kek)?;
        self.save_policy_state(TpmPolicyState {
            version: POLICY_VERSION,
            fingerprint: current_fingerprint.to_string(),
            lifecycle_state: KeyLifecycleState::Recover,
        })?;
        Ok(())
    }

    fn load_unsealed_dek(&self, kek: &str) -> Result<String> {
        let wrapped = self.wrapped_dek_path();
        let decoded = cocoon_store::load_secret(&wrapped, kek)
            .map_err(|_| Self::reset_error("wrapped DEK decryption failed"))?;
        decoded.ok_or_else(|| Self::reset_error("wrapped DEK missing"))
    }

    fn maybe_archive_stale_artifacts(&self) {
        let policy = self.policy_state_path();
        let wrapped = self.wrapped_dek_path();
        if !policy.exists() && !wrapped.exists() {
            return;
        }

        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|dur| dur.as_secs())
            .unwrap_or(0);
        let backup_root = self.security_root.join(format!("recovery_backup_{stamp}"));

        if std::fs::create_dir_all(&backup_root).is_err() {
            return;
        }

        if policy.exists() {
            let _ = std::fs::copy(&policy, backup_root.join("tpm_policy_state.json"));
        }
        if wrapped.exists() {
            let _ = std::fs::copy(&wrapped, backup_root.join("wrapped_dek.cocoon"));
        }
    }

    fn reprovision_after_reset(&self, fingerprint: &str) -> Result<()> {
        self.maybe_archive_stale_artifacts();
        self.revoke()?;

        let kek = Self::random_secret()?;
        self.persist_kek_with_fallback(&kek)?;
        let dek = Self::random_secret()?;
        self.persist_wrapped_dek(&kek, &dek)?;
        self.save_policy_state(TpmPolicyState {
            version: POLICY_VERSION,
            fingerprint: fingerprint.to_string(),
            lifecycle_state: KeyLifecycleState::Recover,
        })
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
            if let Err(err) = self.verify_or_migrate_policy_fingerprint(&state, &fingerprint) {
                if Self::reset_like(&err) {
                    self.reprovision_after_reset(&fingerprint)?;
                    return Ok(());
                }
                return Err(err);
            }
            let kek = match self.load_kek_mapped() {
                Ok(value) => value,
                Err(err) if Self::reset_like(&err) => {
                    self.reprovision_after_reset(&fingerprint)?;
                    return Ok(());
                }
                Err(err) => return Err(err),
            };
            if let Err(err) = self.load_unsealed_dek(&kek) {
                if Self::reset_like(&err) {
                    self.reprovision_after_reset(&fingerprint)?;
                    return Ok(());
                }
                return Err(err);
            }
            return Ok(());
        }

        let kek = Self::random_secret()?;
        self.persist_kek_with_fallback(&kek)?;
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
        let mut recovered_once = false;

        loop {
            self.provision()?;

            let state = self
                .load_policy_state()?
                .ok_or_else(|| Self::reset_error("TPM policy state missing"))?;
            let fingerprint = self.backend.fingerprint()?;
            if let Err(err) = self.verify_or_migrate_policy_fingerprint(&state, &fingerprint) {
                if Self::reset_like(&err) && !recovered_once {
                    self.reprovision_after_reset(&fingerprint)?;
                    recovered_once = true;
                    continue;
                }
                return Err(err);
            }

            let kek = match self.load_kek_mapped() {
                Ok(value) => value,
                Err(err) if Self::reset_like(&err) && !recovered_once => {
                    self.reprovision_after_reset(&fingerprint)?;
                    recovered_once = true;
                    continue;
                }
                Err(err) => return Err(err),
            };
            let dek = match self.load_unsealed_dek(&kek) {
                Ok(value) => value,
                Err(err) if Self::reset_like(&err) && !recovered_once => {
                    self.reprovision_after_reset(&fingerprint)?;
                    recovered_once = true;
                    continue;
                }
                Err(err) => return Err(err),
            };

            self.save_policy_state(TpmPolicyState {
                version: POLICY_VERSION,
                fingerprint,
                lifecycle_state: KeyLifecycleState::Use,
            })?;

            return Ok(dek);
        }
    }

    fn rotate_dek(&self) -> Result<()> {
        self.require_present()?;
        let state = self
            .load_policy_state()?
            .ok_or_else(|| Self::reset_error("TPM policy state missing"))?;
        let fingerprint = self.backend.fingerprint()?;
        if let Err(err) = self.verify_or_migrate_policy_fingerprint(&state, &fingerprint) {
            if Self::reset_like(&err) {
                self.reprovision_after_reset(&fingerprint)?;
                return Ok(());
            }
            return Err(err);
        }
        let kek = match self.load_kek_mapped() {
            Ok(value) => value,
            Err(err) if Self::reset_like(&err) => {
                self.reprovision_after_reset(&fingerprint)?;
                return Ok(());
            }
            Err(err) => return Err(err),
        };
        let new_dek = Self::random_secret()?;
        self.persist_wrapped_dek(&kek, &new_dek)?;
        self.save_policy_state(TpmPolicyState {
            version: POLICY_VERSION,
            fingerprint,
            lifecycle_state: KeyLifecycleState::Rotate,
        })
    }

    fn revoke(&self) -> Result<()> {
        let _ = std::fs::remove_file(self.wrapped_dek_path());
        let _ = std::fs::remove_file(self.policy_state_path());
        let _ = std::fs::remove_file(self.fallback_kek_path());
        self.kek_store.clear_kek()?;
        Ok(())
    }
}

fn runtime_root() -> PathBuf {
    crate::runtime_paths::app_root().join("security")
}

#[cfg(not(test))]
fn compatible_runtime_root() -> PathBuf {
    crate::runtime_paths::app_root().join("security_compatible")
}

#[cfg(not(test))]
fn production_runtime_compatible(
) -> TpmRuntime<'static, CompatibleBackend, KeyringCompatibleKekStore> {
    static BACKEND: CompatibleBackend = CompatibleBackend;
    static STORE: KeyringCompatibleKekStore = KeyringCompatibleKekStore;
    TpmRuntime {
        backend: &BACKEND,
        kek_store: &STORE,
        security_root: compatible_runtime_root(),
    }
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
                    missing_tpm_policy_message(),
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
                    missing_tpm_policy_message(),
                ))
            };
        }
    }

    production_runtime().require_present()
}

pub fn tpm_mode() -> &'static str {
    configured_tpm_mode().as_str()
}

pub fn tpm_available() -> bool {
    require_tpm().is_ok()
}

pub fn resolve_dek_passphrase() -> Result<String> {
    #[cfg(all(debug_assertions, not(test)))]
    {
        if let Some(value) = debug_tpm_available_override() {
            if !value {
                return Err(ButterflyBotError::SecurityPolicy(
                    missing_tpm_policy_message(),
                ));
            }
        }
        if let Some(value) = debug_dek_override() {
            return Ok(value);
        }
    }

    #[cfg(test)]
    {
        if let Some(value) = tpm_available_override() {
            if !value {
                return Err(ButterflyBotError::SecurityPolicy(
                    missing_tpm_policy_message(),
                ));
            }
        }
        if let Some(value) = dek_override() {
            return Ok(value);
        }

        // Keep unit tests deterministic and isolated from host keyring/TPM state.
        Ok("butterfly-bot-test-dek-passphrase".to_string())
    }

    #[cfg(not(test))]
    {
        match configured_tpm_mode() {
            TpmMode::Strict => production_runtime().unseal_dek(),
            TpmMode::Compatible => production_runtime_compatible().unseal_dek(),
            TpmMode::Auto => match production_runtime().unseal_dek() {
                Ok(value) => Ok(value),
                Err(err) => {
                    if is_missing_tpm_policy(&err) || should_fallback_to_compatible(&err) {
                        return production_runtime_compatible().unseal_dek();
                    }
                    Err(err)
                }
            },
        }
    }
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
    fn tpm_reset_missing_kek_auto_reprovisions() {
        let temp = tempfile::tempdir().unwrap();
        let backend = MemoryTpmBackend {
            present: true,
            fingerprint: "fp-a".to_string(),
        };
        let store = MemoryKekStore::default();
        let runtime = runtime(&backend, &store, temp.path());

        runtime.provision().unwrap();
        let original_dek = runtime.unseal_dek().unwrap();
        store.clear_kek().unwrap();

        let recovered_dek = runtime.unseal_dek().unwrap();
        assert!(!recovered_dek.trim().is_empty());
        assert_eq!(recovered_dek, original_dek);

        let state = runtime.load_policy_state().unwrap().unwrap();
        assert_eq!(state.version, POLICY_VERSION);
        assert_eq!(state.fingerprint, "fp-a");
        assert_eq!(state.lifecycle_state, KeyLifecycleState::Use);
    }

    #[test]
    fn tpm_policy_mismatch_auto_recovers_with_continuity() {
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

        let dek = runtime_b.unseal_dek().unwrap();
        assert!(!dek.trim().is_empty());

        let migrated = runtime_b.load_policy_state().unwrap().unwrap();
        assert_eq!(migrated.version, POLICY_VERSION);
        assert_eq!(migrated.fingerprint, "fp-b");
        assert_eq!(migrated.lifecycle_state, KeyLifecycleState::Use);
    }

    #[test]
    fn tpm_policy_mismatch_with_missing_kek_auto_reprovisions() {
        let temp = tempfile::tempdir().unwrap();
        let backend_a = MemoryTpmBackend {
            present: true,
            fingerprint: "fp-a".to_string(),
        };
        let store = MemoryKekStore::default();
        let runtime_a = runtime(&backend_a, &store, temp.path());
        runtime_a.provision().unwrap();
        store.clear_kek().unwrap();

        let backend_b = MemoryTpmBackend {
            present: true,
            fingerprint: "fp-b".to_string(),
        };
        let runtime_b = runtime(&backend_b, &store, temp.path());

        let dek = runtime_b.unseal_dek().unwrap();
        assert!(!dek.trim().is_empty());

        let state = runtime_b.load_policy_state().unwrap().unwrap();
        assert_eq!(state.version, POLICY_VERSION);
        assert_eq!(state.fingerprint, "fp-b");
        assert_eq!(state.lifecycle_state, KeyLifecycleState::Use);
    }

    #[test]
    fn legacy_policy_mismatch_auto_migrates() {
        let temp = tempfile::tempdir().unwrap();
        let backend_a = MemoryTpmBackend {
            present: true,
            fingerprint: "fp-a".to_string(),
        };
        let store = MemoryKekStore::default();
        let runtime_a = runtime(&backend_a, &store, temp.path());
        runtime_a.provision().unwrap();

        runtime_a
            .save_policy_state(TpmPolicyState {
                version: 1,
                fingerprint: "fp-a".to_string(),
                lifecycle_state: KeyLifecycleState::Seal,
            })
            .unwrap();

        let backend_b = MemoryTpmBackend {
            present: true,
            fingerprint: "fp-b".to_string(),
        };
        let runtime_b = runtime(&backend_b, &store, temp.path());

        let dek = runtime_b.unseal_dek().unwrap();
        assert!(!dek.trim().is_empty());

        let migrated = runtime_b.load_policy_state().unwrap().unwrap();
        assert_eq!(migrated.version, POLICY_VERSION);
        assert_eq!(migrated.fingerprint, "fp-b");
    }

    #[test]
    fn parse_tpm_index_accepts_numeric_suffixes() {
        assert_eq!(
            DeviceTpmBackend::parse_tpm_index("tpmrm0", "tpmrm"),
            Some(0)
        );
        assert_eq!(DeviceTpmBackend::parse_tpm_index("tpm12", "tpm"), Some(12));
    }

    #[test]
    fn parse_tpm_index_rejects_invalid_suffixes() {
        assert_eq!(DeviceTpmBackend::parse_tpm_index("tpm", "tpm"), None);
        assert_eq!(DeviceTpmBackend::parse_tpm_index("tpmrmx", "tpmrm"), None);
        assert_eq!(DeviceTpmBackend::parse_tpm_index("atpm0", "tpm"), None);
    }

    #[test]
    fn configured_tpm_mode_defaults_to_auto() {
        assert_eq!(parse_tpm_mode(None), TpmMode::Auto);
    }

    #[test]
    fn configured_tpm_mode_parses_strict_and_compatible() {
        assert_eq!(parse_tpm_mode(Some("strict")), TpmMode::Strict);
        assert_eq!(parse_tpm_mode(Some("compatible")), TpmMode::Compatible);
        assert_eq!(parse_tpm_mode(Some("AUTO")), TpmMode::Auto);
    }

    #[test]
    fn should_fallback_to_compatible_for_security_errors() {
        assert!(should_fallback_to_compatible(
            &ButterflyBotError::SecurityPolicy("TPM not present".to_string())
        ));
        assert!(should_fallback_to_compatible(
            &ButterflyBotError::SecurityStorage("Keyring unavailable".to_string())
        ));
    }

    #[test]
    fn should_fallback_to_compatible_for_runtime_secure_backend_errors() {
        assert!(should_fallback_to_compatible(&ButterflyBotError::Runtime(
            "TPM backend unavailable".to_string()
        )));
        assert!(!should_fallback_to_compatible(&ButterflyBotError::Runtime(
            "network timeout".to_string()
        )));
    }
}
