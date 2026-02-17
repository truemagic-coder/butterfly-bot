use std::path::{Path, PathBuf};

use crate::error::{ButterflyBotError, Result};

const BUNDLED_WASM_MODULES: [(&str, &[u8]); 11] = [
    (
        "coding_tool.wasm",
        include_bytes!("../wasm/coding_tool.wasm"),
    ),
    ("mcp_tool.wasm", include_bytes!("../wasm/mcp_tool.wasm")),
    (
        "http_call_tool.wasm",
        include_bytes!("../wasm/http_call_tool.wasm"),
    ),
    (
        "github_tool.wasm",
        include_bytes!("../wasm/github_tool.wasm"),
    ),
    (
        "zapier_tool.wasm",
        include_bytes!("../wasm/zapier_tool.wasm"),
    ),
    (
        "planning_tool.wasm",
        include_bytes!("../wasm/planning_tool.wasm"),
    ),
    (
        "reminders_tool.wasm",
        include_bytes!("../wasm/reminders_tool.wasm"),
    ),
    (
        "search_internet_tool.wasm",
        include_bytes!("../wasm/search_internet_tool.wasm"),
    ),
    ("tasks_tool.wasm", include_bytes!("../wasm/tasks_tool.wasm")),
    ("todo_tool.wasm", include_bytes!("../wasm/todo_tool.wasm")),
    (
        "wakeup_tool.wasm",
        include_bytes!("../wasm/wakeup_tool.wasm"),
    ),
];

fn env_wasm_dir() -> Option<PathBuf> {
    std::env::var("BUTTERFLY_BOT_WASM_DIR")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

#[cfg(target_os = "macos")]
fn default_wasm_dir_candidates() -> Vec<PathBuf> {
    let mut roots = Vec::new();

    if let Ok(home) = std::env::var("HOME") {
        let trimmed = home.trim();
        if !trimmed.is_empty() {
            roots.push(
                PathBuf::from(trimmed)
                    .join("Library")
                    .join("Application Support")
                    .join("butterfly-bot")
                    .join("wasm"),
            );
        }
    }

    roots.push(std::env::temp_dir().join("butterfly-bot").join("wasm"));
    roots.push(PathBuf::from(".").join("wasm"));
    roots
}

#[cfg(not(target_os = "macos"))]
fn default_wasm_dir_candidates() -> Vec<PathBuf> {
    let mut roots = Vec::new();

    if let Ok(value) = std::env::var("XDG_DATA_HOME") {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            roots.push(PathBuf::from(trimmed).join("butterfly-bot").join("wasm"));
        }
    }

    if let Ok(home) = std::env::var("HOME") {
        let trimmed = home.trim();
        if !trimmed.is_empty() {
            roots.push(
                PathBuf::from(trimmed)
                    .join(".local")
                    .join("share")
                    .join("butterfly-bot")
                    .join("wasm"),
            );
        }
    }

    let app_root = crate::runtime_paths::app_root();
    if !app_root.as_os_str().is_empty() {
        roots.push(app_root.join("wasm"));
    }

    roots.push(std::env::temp_dir().join("butterfly-bot").join("wasm"));
    roots.push(PathBuf::from(".").join("wasm"));
    roots
}

fn write_module_if_needed(root: &Path, file_name: &str, content: &[u8]) -> Result<()> {
    let path = root.join(file_name);

    if path.exists() {
        let existing = std::fs::read(&path).map_err(|e| {
            ButterflyBotError::Runtime(format!(
                "Failed to read bundled WASM module {}: {}",
                path.to_string_lossy(),
                e
            ))
        })?;

        if existing.as_slice() == content {
            return Ok(());
        }
    }

    let tmp_path = path.with_extension("wasm.tmp");
    std::fs::write(&tmp_path, content).map_err(|e| {
        ButterflyBotError::Runtime(format!(
            "Failed to write bundled WASM module {}: {}",
            tmp_path.to_string_lossy(),
            e
        ))
    })?;

    std::fs::rename(&tmp_path, &path).map_err(|e| {
        ButterflyBotError::Runtime(format!(
            "Failed to replace bundled WASM module {}: {}",
            path.to_string_lossy(),
            e
        ))
    })
}

fn provision_into_dir(wasm_dir: &Path) -> Result<()> {
    std::fs::create_dir_all(wasm_dir).map_err(|e| {
        ButterflyBotError::Runtime(format!(
            "Failed to create WASM tools directory {}: {}",
            wasm_dir.to_string_lossy(),
            e
        ))
    })?;

    for (file_name, content) in BUNDLED_WASM_MODULES {
        write_module_if_needed(wasm_dir, file_name, content)?;
    }

    Ok(())
}

pub fn ensure_bundled_wasm_tools() -> Result<PathBuf> {
    if let Some(wasm_dir) = env_wasm_dir() {
        provision_into_dir(&wasm_dir)?;
        std::env::set_var("BUTTERFLY_BOT_WASM_DIR", &wasm_dir);
        return Ok(wasm_dir);
    }

    let mut tried = Vec::new();
    let mut last_err = None;
    let mut candidates = default_wasm_dir_candidates();
    candidates.dedup();

    for wasm_dir in candidates {
        tried.push(wasm_dir.to_string_lossy().to_string());
        match provision_into_dir(&wasm_dir) {
            Ok(()) => {
                std::env::set_var("BUTTERFLY_BOT_WASM_DIR", &wasm_dir);
                return Ok(wasm_dir);
            }
            Err(err) => {
                last_err = Some(err);
            }
        }
    }

    let detail = last_err
        .map(|e| e.to_string())
        .unwrap_or_else(|| "no candidate directories available".to_string());
    Err(ButterflyBotError::Runtime(format!(
        "Could not provision bundled WASM tool modules. Tried: [{}]. Last error: {}",
        tried.join(", "),
        detail
    )))
}
