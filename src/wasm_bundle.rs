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
    let mut tried = Vec::new();
    let mut last_err = None;
    let candidates = crate::runtime_paths::default_wasm_dir_candidates();

    for wasm_dir in candidates {
        tried.push(wasm_dir.to_string_lossy().to_string());
        match provision_into_dir(&wasm_dir) {
            Ok(()) => {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provision_into_dir_writes_all_bundled_modules() {
        let dir = tempfile::tempdir().expect("tempdir");
        provision_into_dir(dir.path()).expect("provision bundled modules");

        for (file_name, content) in BUNDLED_WASM_MODULES {
            let path = dir.path().join(file_name);
            assert!(path.exists(), "expected bundled module {file_name}");
            let bytes = std::fs::read(&path).expect("read provisioned module");
            assert_eq!(
                bytes.as_slice(),
                content,
                "content mismatch for {file_name}"
            );
        }
    }

    #[test]
    fn write_module_if_needed_replaces_stale_content() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (file_name, content) = BUNDLED_WASM_MODULES[0];
        let target = dir.path().join(file_name);

        std::fs::write(&target, b"stale module bytes").expect("seed stale module");
        write_module_if_needed(dir.path(), file_name, content).expect("rewrite stale module");

        let bytes = std::fs::read(&target).expect("read rewritten module");
        assert_eq!(bytes.as_slice(), content);
    }

    #[test]
    fn write_module_if_needed_is_idempotent_when_unchanged() {
        let dir = tempfile::tempdir().expect("tempdir");
        let (file_name, content) = BUNDLED_WASM_MODULES[1];
        let target = dir.path().join(file_name);

        write_module_if_needed(dir.path(), file_name, content).expect("initial write");
        let first = std::fs::read(&target).expect("read first write");

        write_module_if_needed(dir.path(), file_name, content).expect("idempotent rewrite");
        let second = std::fs::read(&target).expect("read second write");

        assert_eq!(first, second);
        assert_eq!(second.as_slice(), content);
    }
}
