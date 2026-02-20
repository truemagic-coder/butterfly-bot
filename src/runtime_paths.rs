use std::path::PathBuf;
use std::sync::{OnceLock, RwLock};

#[cfg(target_os = "macos")]
fn home_dir() -> Option<PathBuf> {
    std::env::var("HOME").ok().and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(PathBuf::from(trimmed))
        }
    })
}

fn env_app_root() -> Option<PathBuf> {
    std::env::var("BUTTERFLY_BOT_APP_ROOT")
        .ok()
        .and_then(|value| {
            let trimmed = value.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(PathBuf::from(trimmed))
            }
        })
}

#[cfg(target_os = "macos")]
fn platform_app_root() -> PathBuf {
    home_dir()
        .map(|home| {
            home.join("Library")
                .join("Application Support")
                .join("butterfly-bot")
        })
        .unwrap_or_else(|| std::env::temp_dir().join("butterfly-bot"))
}

#[cfg(not(target_os = "macos"))]
fn platform_app_root() -> PathBuf {
    if let Ok(value) = std::env::var("SNAP_USER_COMMON") {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed).join("butterfly-bot");
        }
    }
    PathBuf::from(".")
}

pub fn app_root() -> PathBuf {
    if let Some(lock) = DEBUG_APP_ROOT_OVERRIDE.get() {
        let override_root = match lock.read() {
            Ok(guard) => guard.clone(),
            Err(poisoned) => poisoned.into_inner().clone(),
        };
        if let Some(path) = override_root {
            return path;
        }
    }

    env_app_root().unwrap_or_else(platform_app_root)
}

pub fn default_db_path() -> String {
    app_root()
        .join("data")
        .join("butterfly-bot.db")
        .to_string_lossy()
        .to_string()
}

pub fn default_wasm_dir_candidates() -> Vec<PathBuf> {
    let mut candidates = Vec::new();

    if let Ok(value) = std::env::var("BUTTERFLY_BOT_WASM_DIR") {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            candidates.push(PathBuf::from(trimmed));
        }
    }

    candidates.push(app_root().join("wasm"));

    if let Ok(current) = std::env::current_dir() {
        candidates.push(current.join("wasm"));
    }

    candidates.push(PathBuf::from("wasm"));

    let mut deduped = Vec::new();
    for candidate in candidates {
        if !deduped.contains(&candidate) {
            deduped.push(candidate);
        }
    }

    deduped
}

static DEBUG_APP_ROOT_OVERRIDE: OnceLock<RwLock<Option<PathBuf>>> = OnceLock::new();

pub fn set_debug_app_root_override(path: Option<PathBuf>) {
    let lock = DEBUG_APP_ROOT_OVERRIDE.get_or_init(|| RwLock::new(None));
    match lock.write() {
        Ok(mut guard) => *guard = path,
        Err(poisoned) => {
            let mut guard = poisoned.into_inner();
            *guard = path;
        }
    }
}

pub fn set_app_root_override_for_tests(path: Option<PathBuf>) {
    set_debug_app_root_override(path);
}
