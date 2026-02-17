use std::path::PathBuf;

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
    std::env::var("BUTTERFLY_BOT_APP_ROOT").ok().and_then(|value| {
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
    env_app_root().unwrap_or_else(platform_app_root)
}

pub fn default_db_path() -> String {
    app_root()
        .join("data")
        .join("butterfly-bot.db")
        .to_string_lossy()
        .to_string()
}
