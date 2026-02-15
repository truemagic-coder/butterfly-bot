use std::collections::HashMap;
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
pub struct WasmToolConfig {
    pub module: Option<String>,
    pub entrypoint: Option<String>,
    pub timeout_ms: Option<u64>,
    pub fuel: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSandboxConfig {
    #[serde(default)]
    pub wasm: WasmToolConfig,
    #[serde(default)]
    pub filesystem: FilesystemPolicy,
    #[serde(default)]
    pub network: NetworkPolicy,
}

impl Default for ToolSandboxConfig {
    fn default() -> Self {
        Self {
            wasm: WasmToolConfig::default(),
            filesystem: FilesystemPolicy::default(),
            network: NetworkPolicy::default(),
        }
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
        let tool_config = self.tools.get(tool_name).cloned().unwrap_or_default();
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
}

#[cfg(test)]
mod tests {
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
        assert_eq!(settings.execution_plan("http_call").runtime, ToolRuntime::Wasm);
        assert_eq!(settings.execution_plan("github").runtime, ToolRuntime::Wasm);
        assert_eq!(settings.execution_plan("planning").runtime, ToolRuntime::Wasm);
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
        assert_eq!(settings_all.execution_plan("github").runtime, ToolRuntime::Wasm);

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
        assert_eq!(settings_off.execution_plan("coding").runtime, ToolRuntime::Wasm);
        assert_eq!(settings_off.execution_plan("tasks").runtime, ToolRuntime::Wasm);
    }

    #[test]
    fn wasm_module_path_defaults_to_convention() {
        let cfg = ToolSandboxConfig::default();
        assert_eq!(
            WasmRuntime::resolve_module_path("coding", &cfg),
            "./wasm/coding_tool.wasm"
        );
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
    fn default_module_path(tool_name: &str) -> String {
        format!("./wasm/{tool_name}_tool.wasm")
    }

    fn resolve_module_path(tool_name: &str, config: &ToolSandboxConfig) -> String {
        config
            .wasm
            .module
            .as_deref()
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(ToString::to_string)
            .unwrap_or_else(|| Self::default_module_path(tool_name))
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

    fn split_ptr_len(packed: i64) -> Result<(i32, i32)> {
        let raw = packed as u64;
        let ptr = (raw >> 32) as u32;
        let len = (raw & 0xFFFF_FFFF) as u32;
        let ptr = i32::try_from(ptr)
            .map_err(|_| ButterflyBotError::Runtime("Invalid output pointer from wasm".to_string()))?;
        let len = i32::try_from(len)
            .map_err(|_| ButterflyBotError::Runtime("Invalid output length from wasm".to_string()))?;
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

        if !Path::new(&module_path).exists() {
            return Err(ButterflyBotError::Runtime(format!(
                "WASM module path does not exist for tool '{tool_name}': {module_path}"
            )));
        }

        let entrypoint = Self::resolve_entrypoint(config);
        let timeout_ms = config.wasm.timeout_ms.unwrap_or(0);
        let fuel_limit = config.wasm.fuel;

        let mut wasm_config = wasmtime::Config::new();
        wasm_config.epoch_interruption(true);
        if fuel_limit.is_some() {
            wasm_config.consume_fuel(true);
        }

        let engine = Engine::new(&wasm_config)
            .map_err(|e| ButterflyBotError::Runtime(format!("Failed to initialize wasm engine: {e}")))?;
        let module = Module::from_file(&engine, &module_path)
            .map_err(|e| ButterflyBotError::Runtime(format!("Failed to load wasm module: {e}")))?;
        let linker = Linker::new(&engine);
        let mut store = Store::new(&engine, ());

        if let Some(limit) = fuel_limit {
            store
                .set_fuel(limit)
                .map_err(|e| ButterflyBotError::Runtime(format!("Failed to apply wasm fuel limit: {e}")))?;
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

        let instance = linker
            .instantiate(&mut store, &module)
            .map_err(|e| ButterflyBotError::Runtime(format!("Failed to instantiate wasm module: {e}")))?;

        let memory = instance
            .get_memory(&mut store, "memory")
            .ok_or_else(|| ButterflyBotError::Runtime("WASM module missing exported memory".to_string()))?;

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

        let value: Value = serde_json::from_slice(&output)
            .map_err(|e| ButterflyBotError::Runtime(format!("WASM output must be valid JSON: {e}")))?;
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
