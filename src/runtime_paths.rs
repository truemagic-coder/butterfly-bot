use std::path::PathBuf;

fn env_trimmed(name: &str) -> Option<String> {
    std::env::var(name).ok().and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

#[cfg(target_os = "macos")]
fn home_dir() -> Option<PathBuf> {
    env_trimmed("HOME").map(PathBuf::from)
}

fn env_app_root() -> Option<PathBuf> {
    env_trimmed("BUTTERFLY_BOT_APP_ROOT").map(PathBuf::from)
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
    if let Some(value) = env_trimmed("SNAP_USER_COMMON") {
        return PathBuf::from(value).join("butterfly-bot");
    }

    if let Some(value) = env_trimmed("XDG_DATA_HOME") {
        return PathBuf::from(value).join("butterfly-bot");
    }

    if let Some(home) = env_trimmed("HOME") {
        return PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("butterfly-bot");
    }

    if let Some(value) = env_trimmed("APPDATA") {
        return PathBuf::from(value).join("butterfly-bot");
    }

    if let Some(home) = env_trimmed("USERPROFILE") {
        return PathBuf::from(home)
            .join("AppData")
            .join("Roaming")
            .join("butterfly-bot");
    }

    std::env::temp_dir().join("butterfly-bot")
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

pub fn default_wasm_dir_candidates() -> Vec<PathBuf> {
    let mut roots = Vec::new();

    if let Some(value) = env_trimmed("BUTTERFLY_BOT_WASM_DIR") {
        roots.push(PathBuf::from(value));
    }

    #[cfg(target_os = "macos")]
    {
        if let Some(home) = home_dir() {
            roots.push(
                home.join("Library")
                    .join("Application Support")
                    .join("butterfly-bot")
                    .join("wasm"),
            );
        }
    }

    #[cfg(not(target_os = "macos"))]
    {
        if let Some(value) = env_trimmed("XDG_DATA_HOME") {
            roots.push(PathBuf::from(value).join("butterfly-bot").join("wasm"));
        }

        if let Some(home) = env_trimmed("HOME") {
            roots.push(
                PathBuf::from(home)
                    .join(".local")
                    .join("share")
                    .join("butterfly-bot")
                    .join("wasm"),
            );
        }

        if let Some(value) = env_trimmed("APPDATA") {
            roots.push(PathBuf::from(value).join("butterfly-bot").join("wasm"));
        }

        if let Some(home) = env_trimmed("USERPROFILE") {
            roots.push(
                PathBuf::from(home)
                    .join("AppData")
                    .join("Roaming")
                    .join("butterfly-bot")
                    .join("wasm"),
            );
        }
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
