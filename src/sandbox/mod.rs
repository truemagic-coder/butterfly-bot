use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use wasmtime::{Engine, Linker, Memory, Module, Store};

use crate::error::ButterflyBotError;
use crate::Result;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum SandboxMode {
    Off,
    #[default]
    NonMain,
    All,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum ToolRuntime {
    #[default]
    Wasm,
}

impl ToolRuntime {
    pub fn as_str(&self) -> &'static str {
        "wasm"
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct FilesystemPolicy {
    pub mode: Option<String>,
    #[serde(default)]
    pub allow: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct NetworkPolicy {
    #[serde(default)]
    pub allow: Vec<String>,
    pub default_deny: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CapabilityPolicy {
    pub abi_version: Option<u32>,
    #[serde(default)]
    pub allow: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct WasmToolConfig {
    pub module: Option<String>,
    pub entrypoint: Option<String>,
    pub timeout_ms: Option<u64>,
    pub fuel: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolSandboxConfig {
    #[serde(default)]
    pub wasm: WasmToolConfig,
    #[serde(default)]
    pub filesystem: FilesystemPolicy,
    #[serde(default)]
    pub network: NetworkPolicy,
    #[serde(default)]
    pub capabilities: CapabilityPolicy,
}

impl ToolSandboxConfig {
    pub fn is_capability_allowed(&self, capability: &str) -> bool {
        self.capabilities
            .allow
            .iter()
            .any(|allowed| allowed == capability)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SandboxSettings {
    #[serde(default)]
    pub mode: SandboxMode,
    #[serde(default)]
    pub tools: HashMap<String, ToolSandboxConfig>,
}

impl SandboxSettings {
    pub fn from_root_config(root: &Value) -> Self {
        let candidate = root
            .get("tools")
            .and_then(|v| v.get("settings"))
            .and_then(|v| v.get("sandbox"))
            .cloned();

        match candidate {
            Some(value) => serde_json::from_value(value).unwrap_or_default(),
            None => SandboxSettings::default(),
        }
    }

    pub fn execution_plan(&self, tool_name: &str) -> ExecutionPlan {
        let mut tool_config = self.tools.get(tool_name).cloned().unwrap_or_default();
        if tool_config.capabilities.allow.is_empty() {
            tool_config.capabilities.allow = Self::default_capabilities_for_tool(tool_name);
        }
        let mode_label = match self.mode {
            SandboxMode::Off => "off",
            SandboxMode::All => "all",
            SandboxMode::NonMain => "non_main",
        };
        let reason = format!("wasm_only_policy (configured sandbox.mode={mode_label})");

        ExecutionPlan {
            runtime: ToolRuntime::Wasm,
            reason,
            tool_config,
        }
    }

    fn default_capabilities_for_tool(tool_name: &str) -> Vec<String> {
        match tool_name {
            "todo" => vec![
                "kv.sqlite.todo.create",
                "kv.sqlite.todo.create_many",
                "kv.sqlite.todo.list",
                "kv.sqlite.todo.complete",
                "kv.sqlite.todo.reopen",
                "kv.sqlite.todo.delete",
                "kv.sqlite.todo.reorder",
            ],
            "tasks" => vec![
                "kv.sqlite.tasks.schedule",
                "kv.sqlite.tasks.list",
                "kv.sqlite.tasks.enable",
                "kv.sqlite.tasks.disable",
                "kv.sqlite.tasks.delete",
            ],
            "reminders" => vec![
                "kv.sqlite.reminders.create",
                "kv.sqlite.reminders.list",
                "kv.sqlite.reminders.complete",
                "kv.sqlite.reminders.delete",
                "kv.sqlite.reminders.snooze",
                "kv.sqlite.reminders.clear",
            ],
            "planning" => vec![
                "kv.sqlite.planning.create",
                "kv.sqlite.planning.list",
                "kv.sqlite.planning.get",
                "kv.sqlite.planning.update",
                "kv.sqlite.planning.delete",
            ],
            "wakeup" => vec![
                "kv.sqlite.wakeup.create",
                "kv.sqlite.wakeup.list",
                "kv.sqlite.wakeup.enable",
                "kv.sqlite.wakeup.disable",
                "kv.sqlite.wakeup.delete",
            ],
            "coding" => vec!["coding.generate"],
            "mcp" => vec!["mcp.list_tools", "mcp.call"],
            "http_call" => vec!["http.request"],
            "github" => vec!["github.list_tools", "github.call_tool"],
            "zapier" => vec!["zapier.list_tools", "zapier.call_tool"],
            "search_internet" => vec!["search.internet"],
            _ => Vec::new(),
        }
        .into_iter()
        .map(str::to_string)
        .collect()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use std::fs;

    use super::{SandboxMode, SandboxSettings, ToolRuntime, ToolSandboxConfig, WasmRuntime};

    #[test]
    fn default_mode_is_non_main() {
        let settings = SandboxSettings::default();
        assert_eq!(settings.mode, SandboxMode::NonMain);
    }

    #[test]
    fn wasm_only_policy_routes_all_tools_to_wasm() {
        let settings = SandboxSettings {
            mode: SandboxMode::NonMain,
            ..Default::default()
        };

        assert_eq!(settings.execution_plan("coding").runtime, ToolRuntime::Wasm);
        assert_eq!(settings.execution_plan("mcp").runtime, ToolRuntime::Wasm);
        assert_eq!(
            settings.execution_plan("http_call").runtime,
            ToolRuntime::Wasm
        );
        assert_eq!(settings.execution_plan("github").runtime, ToolRuntime::Wasm);
        assert_eq!(settings.execution_plan("zapier").runtime, ToolRuntime::Wasm);
        assert_eq!(
            settings.execution_plan("planning").runtime,
            ToolRuntime::Wasm
        );
        assert!(settings
            .execution_plan("reminders")
            .tool_config
            .is_capability_allowed("kv.sqlite.reminders.list"));
    }

    #[test]
    fn explicit_capability_allowlist_overrides_defaults() {
        let root = serde_json::json!({
            "tools": {
                "settings": {
                    "sandbox": {
                        "tools": {
                            "reminders": {
                                "capabilities": {
                                    "allow": ["clock.now_unix"]
                                }
                            }
                        }
                    }
                }
            }
        });

        let settings = SandboxSettings::from_root_config(&root);
        let plan = settings.execution_plan("reminders");
        assert!(plan.tool_config.is_capability_allowed("clock.now_unix"));
        assert!(!plan
            .tool_config
            .is_capability_allowed("kv.sqlite.reminders.list"));
    }

    #[test]
    fn explicit_runtime_override_cannot_bypass_wasm_only_policy() {
        let root = serde_json::json!({
            "tools": {
                "settings": {
                    "sandbox": {
                        "mode": "non_main",
                        "tools": {
                            "coding": {
                                "runtime": "native"
                            },
                            "github": {
                                "runtime": "wasm"
                            }
                        }
                    }
                }
            }
        });

        let settings = SandboxSettings::from_root_config(&root);
        assert_eq!(settings.execution_plan("coding").runtime, ToolRuntime::Wasm);
        assert_eq!(settings.execution_plan("github").runtime, ToolRuntime::Wasm);

        let root_all = serde_json::json!({
            "tools": {
                "settings": {
                    "sandbox": {
                        "mode": "all",
                        "tools": {
                            "github": {
                                "runtime": "wasm"
                            }
                        }
                    }
                }
            }
        });
        let settings_all = SandboxSettings::from_root_config(&root_all);
        assert_eq!(
            settings_all.execution_plan("github").runtime,
            ToolRuntime::Wasm
        );

        let root_off = serde_json::json!({
            "tools": {
                "settings": {
                    "sandbox": {
                        "mode": "off"
                    }
                }
            }
        });
        let settings_off = SandboxSettings::from_root_config(&root_off);
        assert_eq!(
            settings_off.execution_plan("coding").runtime,
            ToolRuntime::Wasm
        );
        assert_eq!(
            settings_off.execution_plan("tasks").runtime,
            ToolRuntime::Wasm
        );
    }

    #[test]
    fn wasm_module_path_defaults_to_convention() {
        let cfg = ToolSandboxConfig::default();
        assert_eq!(
            WasmRuntime::resolve_module_path("coding", &cfg),
            "./wasm/coding_tool.wasm"
        );
    }

    #[test]
    fn wasm_module_path_prefers_per_tool_when_generic_configured() {
        let generic = "./wasm/testdata/butterfly_bot_wasm_tool.wasm";
        let tool_name = "__test_reminders";
        let default = "./wasm/__test_reminders_tool.wasm";
        let _ = fs::create_dir_all("./wasm/testdata");
        let _ = fs::write(generic, b"generic");
        let _ = fs::write(default, b"default");

        let mut cfg = ToolSandboxConfig::default();
        cfg.wasm.module = Some(generic.to_string());

        assert_eq!(WasmRuntime::resolve_module_path(tool_name, &cfg), default);

        let _ = fs::remove_file(generic);
        let _ = fs::remove_file(default);
    }

    #[test]
    fn wasm_module_path_falls_back_to_default_when_configured_missing() {
        let tool_name = "__test_todo";
        let default = "./wasm/__test_todo_tool.wasm";
        let _ = fs::create_dir_all("./wasm");
        let _ = fs::write(default, b"default");

        let mut cfg = ToolSandboxConfig::default();
        cfg.wasm.module = Some("./wasm/does_not_exist.wasm".to_string());

        assert_eq!(WasmRuntime::resolve_module_path(tool_name, &cfg), default);

        let _ = fs::remove_file(default);
    }

    #[test]
    fn wasm_module_validation_rejects_non_wasm_bytes() {
        let path = "./wasm/__test_invalid_tool.wasm";
        let _ = fs::create_dir_all("./wasm");
        let _ = fs::write(path, b"default");

        let mut cfg = ToolSandboxConfig::default();
        cfg.wasm.module = Some(path.to_string());

        let err = WasmRuntime::validate_module_binary("__test_invalid", &cfg)
            .expect_err("expected invalid wasm header to fail");
        assert!(
            err.to_string().contains("missing wasm magic header"),
            "unexpected error: {err}"
        );

        let _ = fs::remove_file(path);
    }

    #[test]
    fn wasm_zero_fuel_is_treated_as_unset() {
        let mut cfg = ToolSandboxConfig::default();
        cfg.wasm.fuel = Some(0);
        assert_eq!(WasmRuntime::resolve_fuel_limit(&cfg), None);

        cfg.wasm.fuel = Some(1);
        assert_eq!(WasmRuntime::resolve_fuel_limit(&cfg), Some(1));
    }

    #[test]
    fn wasm_module_validation_rejects_stub_marker() {
        let path = "./wasm/__test_stub_tool.wasm";
        let _ = fs::create_dir_all("./wasm");
        let mut bytes = vec![0x00, 0x61, 0x73, 0x6D];
        bytes.extend_from_slice(b"...stub responses...");
        let _ = fs::write(path, bytes);

        let mut cfg = ToolSandboxConfig::default();
        cfg.wasm.module = Some(path.to_string());

        let err = WasmRuntime::validate_module_binary("__test_stub", &cfg)
            .expect_err("expected stub module to fail");
        assert!(
            err.to_string().contains("placeholder stub"),
            "unexpected error: {err}"
        );

        let _ = fs::remove_file(path);
    }

    #[test]
    fn reminders_wasm_execute_does_not_alloc_trap() {
        let cfg = ToolSandboxConfig::default();
        let params = json!({
            "action": "list",
            "status": "open",
            "user_id": "cli_user",
            "limit": 10
        });

        let result = WasmRuntime::execute_sync("reminders", &cfg, params);
        if let Err(err) = result {
            let msg = err.to_string();
            assert!(
                !msg.contains("WASM alloc failed"),
                "unexpected alloc trap from reminders wasm: {msg}"
            );
        }
    }

    #[test]
    fn capability_allowlist_parses_from_config() {
        let root = serde_json::json!({
            "tools": {
                "settings": {
                    "sandbox": {
                        "tools": {
                            "todo": {
                                "capabilities": {
                                    "abi_version": 1,
                                    "allow": ["kv.sqlite.todo.create", "clock.now_unix"]
                                }
                            }
                        }
                    }
                }
            }
        });

        let settings = SandboxSettings::from_root_config(&root);
        let plan = settings.execution_plan("todo");
        assert_eq!(plan.tool_config.capabilities.abi_version, Some(1));
        assert!(plan
            .tool_config
            .is_capability_allowed("kv.sqlite.todo.create"));
        assert!(!plan
            .tool_config
            .is_capability_allowed("kv.sqlite.todo.delete"));
    }

    #[test]
    fn capability_abi_version_mismatch_rejected() {
        let root = serde_json::json!({
            "tools": {
                "settings": {
                    "sandbox": {
                        "tools": {
                            "todo": {
                                "capabilities": {
                                    "abi_version": 2,
                                    "allow": ["kv.sqlite.todo.create"]
                                }
                            }
                        }
                    }
                }
            }
        });

        let settings = SandboxSettings::from_root_config(&root);
        let plan = settings.execution_plan("todo");
        let err = WasmRuntime::validate_capability_abi("todo", &plan.tool_config)
            .expect_err("expected abi_version mismatch to fail");
        assert!(err
            .to_string()
            .contains("unsupported capability ABI version"));
    }
}

#[derive(Debug, Clone)]
pub struct ExecutionPlan {
    pub runtime: ToolRuntime,
    pub reason: String,
    pub tool_config: ToolSandboxConfig,
}

#[derive(Debug, Default)]
pub struct WasmRuntime;

struct TimeoutCompletion {
    done: Arc<AtomicBool>,
}

impl Drop for TimeoutCompletion {
    fn drop(&mut self) {
        self.done.store(true, Ordering::Relaxed);
    }
}

impl WasmRuntime {
    const MAX_INPUT_BYTES: usize = 256 * 1024;
    const WASM_MAGIC: [u8; 4] = [0x00, 0x61, 0x73, 0x6D];
    pub const SUPPORTED_CAPABILITY_ABI_VERSION: u32 = 1;

    fn default_module_path(tool_name: &str) -> String {
        format!("./wasm/{tool_name}_tool.wasm")
    }

    fn resolve_module_path(tool_name: &str, config: &ToolSandboxConfig) -> String {
        let default_path = Self::default_module_path(tool_name);
        config
            .wasm
            .module
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(|configured| {
                let configured_path = configured.to_string();
                let configured_file = Path::new(configured)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or_default();

                if configured_file == "butterfly_bot_wasm_tool.wasm" {
                    return default_path.clone();
                }

                if Path::new(&configured_path).exists() {
                    configured_path
                } else if Path::new(&default_path).exists() {
                    default_path.clone()
                } else {
                    configured_path
                }
            })
            .unwrap_or(default_path)
    }

    fn resolve_entrypoint(config: &ToolSandboxConfig) -> String {
        config
            .wasm
            .entrypoint
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .unwrap_or("execute")
            .to_string()
    }

    fn resolve_fuel_limit(config: &ToolSandboxConfig) -> Option<u64> {
        config.wasm.fuel.filter(|limit| *limit > 0)
    }

    pub fn validate_module_binary(tool_name: &str, config: &ToolSandboxConfig) -> Result<()> {
        let module_path = Self::resolve_module_path(tool_name, config);
        let path = Path::new(&module_path);

        if !path.exists() {
            return Err(ButterflyBotError::Runtime(format!(
                "WASM module path does not exist for tool '{tool_name}': {module_path}"
            )));
        }

        let mut file = File::open(path)
            .map_err(|e| ButterflyBotError::Runtime(format!("Failed to open wasm module: {e}")))?;
        let mut header = [0u8; 4];
        file.read_exact(&mut header).map_err(|e| {
            ButterflyBotError::Runtime(format!(
                "Failed to read wasm module header for tool '{tool_name}' ({module_path}): {e}"
            ))
        })?;

        if header != Self::WASM_MAGIC {
            return Err(ButterflyBotError::Runtime(format!(
                "Invalid wasm module for tool '{tool_name}' at {module_path}: missing wasm magic header"
            )));
        }

        let mut tail = Vec::new();
        file.read_to_end(&mut tail).map_err(|e| {
            ButterflyBotError::Runtime(format!(
                "Failed to inspect wasm module body for tool '{tool_name}' ({module_path}): {e}"
            ))
        })?;
        if tail
            .windows("stub responses".len())
            .any(|w| w == b"stub responses")
            || tail
                .windows("\"stub\":true".len())
                .any(|w| w == b"\"stub\":true")
        {
            return Err(ButterflyBotError::Runtime(format!(
                "WASM module for tool '{tool_name}' at {module_path} is a placeholder stub. Build/install a real WASM implementation before starting the daemon."
            )));
        }

        Ok(())
    }

    pub fn validate_capability_abi(tool_name: &str, config: &ToolSandboxConfig) -> Result<()> {
        if let Some(version) = config.capabilities.abi_version {
            if version != Self::SUPPORTED_CAPABILITY_ABI_VERSION {
                return Err(ButterflyBotError::Runtime(format!(
                    "Tool '{tool_name}' uses unsupported capability ABI version {version}; supported version is {}",
                    Self::SUPPORTED_CAPABILITY_ABI_VERSION
                )));
            }
        }

        Ok(())
    }

    fn split_ptr_len(packed: i64) -> Result<(i32, i32)> {
        let raw = packed as u64;
        let ptr = (raw >> 32) as u32;
        let len = (raw & 0xFFFF_FFFF) as u32;
        let ptr = i32::try_from(ptr).map_err(|_| {
            ButterflyBotError::Runtime("Invalid output pointer from wasm".to_string())
        })?;
        let len = i32::try_from(len).map_err(|_| {
            ButterflyBotError::Runtime("Invalid output length from wasm".to_string())
        })?;
        Ok((ptr, len))
    }

    fn ensure_range(memory: &Memory, store: &Store<()>, ptr: i32, len: i32) -> Result<()> {
        if ptr < 0 || len < 0 {
            return Err(ButterflyBotError::Runtime(
                "Negative pointer/length from wasm".to_string(),
            ));
        }
        let start = ptr as usize;
        let size = len as usize;
        let end = start
            .checked_add(size)
            .ok_or_else(|| ButterflyBotError::Runtime("WASM pointer overflow".to_string()))?;
        if end > memory.data_size(store) {
            return Err(ButterflyBotError::Runtime(
                "WASM memory range out of bounds".to_string(),
            ));
        }
        Ok(())
    }

    fn execute_sync(tool_name: &str, config: &ToolSandboxConfig, params: Value) -> Result<Value> {
        let module_path = Self::resolve_module_path(tool_name, config);
        tracing::info!(tool = %tool_name, module_path = %module_path, "Executing tool in WASM runtime");

        if !Path::new(&module_path).exists() {
            return Err(ButterflyBotError::Runtime(format!(
                "WASM module path does not exist for tool '{tool_name}': {module_path}"
            )));
        }

        let entrypoint = Self::resolve_entrypoint(config);
        let timeout_ms = config.wasm.timeout_ms.unwrap_or(0);
        let fuel_limit = Self::resolve_fuel_limit(config);

        let mut wasm_config = wasmtime::Config::new();
        if timeout_ms > 0 {
            wasm_config.epoch_interruption(true);
        }
        if fuel_limit.is_some() {
            wasm_config.consume_fuel(true);
        }

        let engine = Engine::new(&wasm_config).map_err(|e| {
            ButterflyBotError::Runtime(format!("Failed to initialize wasm engine: {e}"))
        })?;
        let module = Module::from_file(&engine, &module_path)
            .map_err(|e| ButterflyBotError::Runtime(format!("Failed to load wasm module: {e}")))?;
        let linker = Linker::new(&engine);
        let mut store = Store::new(&engine, ());

        if let Some(limit) = fuel_limit {
            store.set_fuel(limit).map_err(|e| {
                ButterflyBotError::Runtime(format!("Failed to apply wasm fuel limit: {e}"))
            })?;
        }

        let _timeout_guard = if timeout_ms > 0 {
            store.set_epoch_deadline(1);
            let done = Arc::new(AtomicBool::new(false));
            let done_for_thread = done.clone();
            let engine_for_thread = engine.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(timeout_ms));
                if !done_for_thread.load(Ordering::Relaxed) {
                    engine_for_thread.increment_epoch();
                }
            });
            Some(TimeoutCompletion { done })
        } else {
            None
        };

        let instance = linker.instantiate(&mut store, &module).map_err(|e| {
            ButterflyBotError::Runtime(format!("Failed to instantiate wasm module: {e}"))
        })?;

        let memory = instance.get_memory(&mut store, "memory").ok_or_else(|| {
            ButterflyBotError::Runtime("WASM module missing exported memory".to_string())
        })?;

        let alloc = instance
            .get_typed_func::<i32, i32>(&mut store, "alloc")
            .map_err(|_| {
                ButterflyBotError::Runtime(
                    "WASM module missing `alloc(i32)->i32` export".to_string(),
                )
            })?;

        let dealloc = instance
            .get_typed_func::<(i32, i32), ()>(&mut store, "dealloc")
            .map_err(|_| {
                ButterflyBotError::Runtime(
                    "WASM module missing `dealloc(i32,i32)->()` export".to_string(),
                )
            })?;

        let exec = instance
            .get_typed_func::<(i32, i32), i64>(&mut store, &entrypoint)
            .map_err(|_| {
                ButterflyBotError::Runtime(format!(
                    "WASM module missing `{entrypoint}(i32,i32)->i64` export"
                ))
            })?;

        let input = serde_json::to_vec(&params)
            .map_err(|e| ButterflyBotError::Serialization(e.to_string()))?;
        if input.len() > Self::MAX_INPUT_BYTES {
            return Err(ButterflyBotError::Runtime(format!(
                "WASM tool input too large: {} bytes (max {})",
                input.len(),
                Self::MAX_INPUT_BYTES
            )));
        }

        let input_len = i32::try_from(input.len()).map_err(|_| {
            ButterflyBotError::Runtime("WASM input too large to pass as i32 length".to_string())
        })?;

        let input_ptr = alloc
            .call(&mut store, input_len)
            .map_err(|e| ButterflyBotError::Runtime(format!("WASM alloc failed: {e}")))?;

        Self::ensure_range(&memory, &store, input_ptr, input_len)?;
        memory
            .write(&mut store, input_ptr as usize, &input)
            .map_err(|e| ButterflyBotError::Runtime(format!("WASM memory write failed: {e}")))?;

        let packed = exec.call(&mut store, (input_ptr, input_len)).map_err(|e| {
            let msg = e.to_string();
            if timeout_ms > 0 && msg.to_ascii_lowercase().contains("interrupt") {
                ButterflyBotError::Runtime(format!(
                    "WASM tool '{tool_name}' timed out after {timeout_ms}ms"
                ))
            } else {
                ButterflyBotError::Runtime(format!("WASM tool execute failed: {msg}"))
            }
        })?;

        let (output_ptr, output_len) = Self::split_ptr_len(packed)?;
        Self::ensure_range(&memory, &store, output_ptr, output_len)?;

        let mut output = vec![0u8; output_len as usize];
        memory
            .read(&store, output_ptr as usize, &mut output)
            .map_err(|e| ButterflyBotError::Runtime(format!("WASM memory read failed: {e}")))?;

        let _ = dealloc.call(&mut store, (input_ptr, input_len));
        let _ = dealloc.call(&mut store, (output_ptr, output_len));

        if output.is_empty() {
            return Ok(serde_json::json!({}));
        }

        let value: Value = serde_json::from_slice(&output).map_err(|e| {
            ButterflyBotError::Runtime(format!("WASM output must be valid JSON: {e}"))
        })?;
        Ok(value)
    }

    pub async fn execute(
        &self,
        tool_name: &str,
        config: &ToolSandboxConfig,
        params: Value,
    ) -> Result<Value> {
        let tool_name = tool_name.to_string();
        let config = config.clone();
        tokio::task::spawn_blocking(move || Self::execute_sync(&tool_name, &config, params))
            .await
            .map_err(|e| ButterflyBotError::Runtime(format!("WASM task join error: {e}")))?
    }
}
