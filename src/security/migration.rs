use crate::error::{ButterflyBotError, Result};

const SERVICE: &str = "butterfly-bot";

const LEGACY_SECRET_NAMES: &[&str] = &[
    "daemon_auth_token",
    "db_encryption_key",
    "openai_api_key",
    "memory_openai_api_key",
    "app_config_json",
    "github_pat",
    "zapier_token",
    "coding_openai_api_key",
    "search_internet_openai_api_key",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MigrationMode {
    DryRun,
    Apply,
}

#[derive(Debug, Clone)]
pub struct MigrationItem {
    pub name: String,
    pub status: String,
    pub detail: String,
}

#[derive(Debug, Clone)]
pub struct MigrationReport {
    pub mode: MigrationMode,
    pub checked: usize,
    pub migrated: usize,
    pub skipped: usize,
    pub errors: usize,
    pub items: Vec<MigrationItem>,
}

trait LegacySecretStore {
    fn get_secret(&self, name: &str) -> Result<Option<String>>;
    fn delete_secret(&self, name: &str) -> Result<()>;
}

trait TargetSecretStore {
    fn get_secret(&self, name: &str) -> Result<Option<String>>;
    fn set_secret_required(&self, name: &str, value: &str) -> Result<()>;
}

struct KeyringLegacySecretStore;

impl LegacySecretStore for KeyringLegacySecretStore {
    fn get_secret(&self, name: &str) -> Result<Option<String>> {
        let entry = keyring::Entry::new(SERVICE, name)
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

    fn delete_secret(&self, name: &str) -> Result<()> {
        let entry = keyring::Entry::new(SERVICE, name)
            .map_err(|e| ButterflyBotError::SecurityStorage(e.to_string()))?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(ButterflyBotError::SecurityStorage(err.to_string())),
        }
    }
}

struct VaultTargetSecretStore;

impl TargetSecretStore for VaultTargetSecretStore {
    fn get_secret(&self, name: &str) -> Result<Option<String>> {
        crate::vault::get_secret(name)
    }

    fn set_secret_required(&self, name: &str, value: &str) -> Result<()> {
        crate::vault::set_secret_required(name, value)
    }
}

fn migrate_with_stores(
    mode: MigrationMode,
    names: &[&str],
    legacy: &dyn LegacySecretStore,
    target: &dyn TargetSecretStore,
) -> MigrationReport {
    let mut report = MigrationReport {
        mode,
        checked: 0,
        migrated: 0,
        skipped: 0,
        errors: 0,
        items: Vec::new(),
    };

    for name in names {
        report.checked += 1;

        let target_value = match target.get_secret(name) {
            Ok(value) => value,
            Err(err) => {
                report.errors += 1;
                report.items.push(MigrationItem {
                    name: (*name).to_string(),
                    status: "error".to_string(),
                    detail: format!("target_read_failed: {err}"),
                });
                continue;
            }
        };

        if target_value
            .as_ref()
            .is_some_and(|value| !value.trim().is_empty())
        {
            report.skipped += 1;
            report.items.push(MigrationItem {
                name: (*name).to_string(),
                status: "skipped".to_string(),
                detail: "already_migrated".to_string(),
            });
            continue;
        }

        let legacy_value = match legacy.get_secret(name) {
            Ok(value) => value,
            Err(err) => {
                report.errors += 1;
                report.items.push(MigrationItem {
                    name: (*name).to_string(),
                    status: "error".to_string(),
                    detail: format!("legacy_read_failed: {err}"),
                });
                continue;
            }
        };

        let Some(legacy_value) = legacy_value else {
            report.skipped += 1;
            report.items.push(MigrationItem {
                name: (*name).to_string(),
                status: "skipped".to_string(),
                detail: "legacy_missing".to_string(),
            });
            continue;
        };

        if legacy_value.trim().is_empty() {
            report.errors += 1;
            report.items.push(MigrationItem {
                name: (*name).to_string(),
                status: "error".to_string(),
                detail: "legacy_empty_value".to_string(),
            });
            continue;
        }

        if mode == MigrationMode::DryRun {
            report.migrated += 1;
            report.items.push(MigrationItem {
                name: (*name).to_string(),
                status: "planned".to_string(),
                detail: "ready_to_migrate".to_string(),
            });
            continue;
        }

        if let Err(err) = target.set_secret_required(name, &legacy_value) {
            report.errors += 1;
            report.items.push(MigrationItem {
                name: (*name).to_string(),
                status: "error".to_string(),
                detail: format!("target_write_failed: {err}"),
            });
            continue;
        }

        let detail = match legacy.delete_secret(name) {
            Ok(()) => "migrated_and_legacy_deleted".to_string(),
            Err(err) => format!("migrated_legacy_delete_failed: {err}"),
        };

        report.migrated += 1;
        report.items.push(MigrationItem {
            name: (*name).to_string(),
            status: "migrated".to_string(),
            detail,
        });
    }

    report
}

pub fn run_legacy_secret_migration(mode: MigrationMode) -> Result<MigrationReport> {
    crate::security::tpm_provider::require_tpm()?;

    let legacy = KeyringLegacySecretStore;
    let target = VaultTargetSecretStore;
    Ok(migrate_with_stores(
        mode,
        LEGACY_SECRET_NAMES,
        &legacy,
        &target,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::Mutex;

    #[derive(Default)]
    struct MemoryLegacyStore {
        values: Mutex<HashMap<String, String>>,
    }

    impl MemoryLegacyStore {
        fn with(values: &[(&str, &str)]) -> Self {
            let mut map = HashMap::new();
            for (k, v) in values {
                map.insert((*k).to_string(), (*v).to_string());
            }
            Self {
                values: Mutex::new(map),
            }
        }
    }

    impl LegacySecretStore for MemoryLegacyStore {
        fn get_secret(&self, name: &str) -> Result<Option<String>> {
            let guard = self
                .values
                .lock()
                .map_err(|_| ButterflyBotError::Runtime("legacy lock poisoned".to_string()))?;
            Ok(guard.get(name).cloned())
        }

        fn delete_secret(&self, name: &str) -> Result<()> {
            let mut guard = self
                .values
                .lock()
                .map_err(|_| ButterflyBotError::Runtime("legacy lock poisoned".to_string()))?;
            guard.remove(name);
            Ok(())
        }
    }

    #[derive(Default)]
    struct MemoryTargetStore {
        values: Mutex<HashMap<String, String>>,
    }

    impl MemoryTargetStore {
        fn with(values: &[(&str, &str)]) -> Self {
            let mut map = HashMap::new();
            for (k, v) in values {
                map.insert((*k).to_string(), (*v).to_string());
            }
            Self {
                values: Mutex::new(map),
            }
        }

        fn value(&self, name: &str) -> Option<String> {
            self.values
                .lock()
                .ok()
                .and_then(|guard| guard.get(name).cloned())
        }
    }

    impl TargetSecretStore for MemoryTargetStore {
        fn get_secret(&self, name: &str) -> Result<Option<String>> {
            let guard = self
                .values
                .lock()
                .map_err(|_| ButterflyBotError::Runtime("target lock poisoned".to_string()))?;
            Ok(guard.get(name).cloned())
        }

        fn set_secret_required(&self, name: &str, value: &str) -> Result<()> {
            let mut guard = self
                .values
                .lock()
                .map_err(|_| ButterflyBotError::Runtime("target lock poisoned".to_string()))?;
            guard.insert(name.to_string(), value.to_string());
            Ok(())
        }
    }

    #[test]
    fn dry_run_reports_without_writing() {
        let legacy = MemoryLegacyStore::with(&[("openai_api_key", "abc")]);
        let target = MemoryTargetStore::default();

        let report =
            migrate_with_stores(MigrationMode::DryRun, &["openai_api_key"], &legacy, &target);

        assert_eq!(report.migrated, 1);
        assert_eq!(report.errors, 0);
        assert!(target.value("openai_api_key").is_none());
    }

    #[test]
    fn apply_migrates_and_is_idempotent_when_repeated() {
        let legacy = MemoryLegacyStore::with(&[("openai_api_key", "abc")]);
        let target = MemoryTargetStore::default();

        let first =
            migrate_with_stores(MigrationMode::Apply, &["openai_api_key"], &legacy, &target);
        assert_eq!(first.migrated, 1);
        assert_eq!(first.errors, 0);
        assert_eq!(target.value("openai_api_key").as_deref(), Some("abc"));

        let second =
            migrate_with_stores(MigrationMode::Apply, &["openai_api_key"], &legacy, &target);
        assert_eq!(second.migrated, 0);
        assert_eq!(second.skipped, 1);
    }

    #[test]
    fn apply_skips_when_target_already_populated() {
        let legacy = MemoryLegacyStore::with(&[("openai_api_key", "legacy")]);
        let target = MemoryTargetStore::with(&[("openai_api_key", "new")]);

        let report =
            migrate_with_stores(MigrationMode::Apply, &["openai_api_key"], &legacy, &target);

        assert_eq!(report.migrated, 0);
        assert_eq!(report.skipped, 1);
        assert_eq!(target.value("openai_api_key").as_deref(), Some("new"));
    }
}
