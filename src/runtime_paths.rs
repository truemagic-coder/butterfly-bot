use directories::{BaseDirs, ProjectDirs};
use std::path::PathBuf;
use std::sync::{OnceLock, RwLock};

fn app_root_override_lock() -> &'static RwLock<Option<PathBuf>> {
    static OVERRIDE: OnceLock<RwLock<Option<PathBuf>>> = OnceLock::new();
    OVERRIDE.get_or_init(|| RwLock::new(None))
}

fn app_root_override() -> Option<PathBuf> {
    let lock = app_root_override_lock();
    match lock.read() {
        Ok(guard) => guard.clone(),
        Err(poisoned) => poisoned.into_inner().clone(),
    }
}

#[cfg(test)]
pub(crate) fn set_app_root_override_for_tests(path: Option<PathBuf>) {
    let lock = app_root_override_lock();
    match lock.write() {
        Ok(mut guard) => *guard = path,
        Err(poisoned) => {
            let mut guard = poisoned.into_inner();
            *guard = path;
        }
    }
}

fn platform_app_root() -> PathBuf {
    if let Some(project_dirs) = ProjectDirs::from("", "", "butterfly-bot") {
        return project_dirs.data_dir().to_path_buf();
    }

    if let Some(base_dirs) = BaseDirs::new() {
        return base_dirs.data_local_dir().join("butterfly-bot");
    }

    std::env::temp_dir().join("butterfly-bot")
}

pub fn app_root() -> PathBuf {
    app_root_override().unwrap_or_else(platform_app_root)
}

pub fn default_db_path() -> String {
    app_root()
        .join("data")
        .join("butterfly-bot.db")
        .to_string_lossy()
        .to_string()
}

pub fn default_wasm_dir_candidates() -> Vec<PathBuf> {
    let mut roots = Vec::new();

    if let Some(project_dirs) = ProjectDirs::from("", "", "butterfly-bot") {
        roots.push(project_dirs.data_dir().join("wasm"));
    }

    if let Some(base_dirs) = BaseDirs::new() {
        roots.push(base_dirs.data_local_dir().join("butterfly-bot").join("wasm"));
    }

    let app_root = app_root();
    if !app_root.as_os_str().is_empty() {
        roots.push(app_root.join("wasm"));
    }

    roots.push(std::env::temp_dir().join("butterfly-bot").join("wasm"));
    roots.push(PathBuf::from(".").join("wasm"));

    roots.dedup();
    roots
}
