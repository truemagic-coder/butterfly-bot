use std::path::{Path, PathBuf};

use crate::error::{ButterflyBotError, Result};

const BUNDLED_WASM_MODULES: [(&str, &[u8]); 11] = [
    ("coding_tool.wasm", include_bytes!("../wasm/coding_tool.wasm")),
    ("mcp_tool.wasm", include_bytes!("../wasm/mcp_tool.wasm")),
    (
        "http_call_tool.wasm",
        include_bytes!("../wasm/http_call_tool.wasm"),
    ),
    ("github_tool.wasm", include_bytes!("../wasm/github_tool.wasm")),
    ("zapier_tool.wasm", include_bytes!("../wasm/zapier_tool.wasm")),
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
    ("wakeup_tool.wasm", include_bytes!("../wasm/wakeup_tool.wasm")),
];

fn env_wasm_dir() -> Option<PathBuf> {
    std::env::var("BUTTERFLY_BOT_WASM_DIR")
        .ok()
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .map(PathBuf::from)
}

#[cfg(target_os = "macos")]
fn default_wasm_dir() -> PathBuf {
    std::env::var("HOME")
        .ok()
        .map(|home| home.trim().to_string())
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
        .map(|home| {
            home.join("Library")
                .join("Application Support")
                .join("butterfly-bot")
                .join("wasm")
        })
        .unwrap_or_else(|| PathBuf::from(".").join("wasm"))
}

#[cfg(not(target_os = "macos"))]
fn default_wasm_dir() -> PathBuf {
    if let Ok(value) = std::env::var("XDG_DATA_HOME") {
        let trimmed = value.trim();
        if !trimmed.is_empty() {
            return PathBuf::from(trimmed).join("butterfly-bot").join("wasm");
        }
    }

    std::env::var("HOME")
        .ok()
        .map(|home| home.trim().to_string())
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
        .map(|home| {
            home.join(".local")
                .join("share")
                .join("butterfly-bot")
                .join("wasm")
        })
        .unwrap_or_else(|| PathBuf::from(".").join("wasm"))
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

pub fn ensure_bundled_wasm_tools() -> Result<PathBuf> {
    let wasm_dir = env_wasm_dir().unwrap_or_else(default_wasm_dir);

    std::fs::create_dir_all(&wasm_dir).map_err(|e| {
        ButterflyBotError::Runtime(format!(
            "Failed to create WASM tools directory {}: {}",
            wasm_dir.to_string_lossy(),
            e
        ))
    })?;

    for (file_name, content) in BUNDLED_WASM_MODULES {
        write_module_if_needed(&wasm_dir, file_name, content)?;
    }

    std::env::set_var("BUTTERFLY_BOT_WASM_DIR", &wasm_dir);
    Ok(wasm_dir)
}
