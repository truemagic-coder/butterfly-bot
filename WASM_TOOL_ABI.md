# WASM Tool ABI (MVP)

This documents the initial Butterfly Bot WASM sandbox ABI implemented in `src/sandbox/mod.rs`.

## Runtime Behavior

When a tool executes, Butterfly Bot:

1. Loads the module from `tools.settings.sandbox.tools.<tool>.wasm.module`
  - If omitted, uses `./wasm/<tool>_tool.wasm` by convention.
2. Instantiates it with **no host imports**
3. Calls `alloc` to pass JSON input bytes
4. Calls the entrypoint (default `execute`)
5. Reads JSON output bytes from guest memory
6. Calls `dealloc` for input/output buffers

Because no host imports are provided, this is a strict host-isolated execution path by default.

## Capability ABI (v1 scaffold)

The runtime now recognizes a capability envelope returned by WASM modules:

```json
{
  "status": "capability_call",
  "abi_version": 1,
  "capability_call": {
    "name": "kv.sqlite.todo.create",
    "args": {"user_id":"cli_user","title":"buy milk"}
  }
}
```

Current state (scaffold only):

- `abi_version` must be `1` when present.
- Capability name must be explicitly allowlisted in per-tool sandbox config.
- Undeclared capability names are rejected with deterministic `forbidden` error.
- Implemented handlers:
  - `clock.now_unix`
  - `log.emit`
  - `coding.generate`
  - `http.request`
  - `mcp.list_tools`
  - `mcp.call`
  - `github.list_tools`
  - `github.call_tool`
  - `search.internet`
  - `secrets.get` (supports strict scoped allowlist entries like `secrets.get.github_pat`)
  - `kv.sqlite.todo.{create,list}`
  - `kv.sqlite.tasks.{schedule,list,enable,disable,delete}`
  - `kv.sqlite.reminders.{create,list,complete,delete,snooze,clear}`
  - `kv.sqlite.planning.{create,list,get,update,delete}`
  - `kv.sqlite.wakeup.{create,list,enable,disable,delete}`
- Other declared capability names currently return deterministic `internal` until additional host bridge handlers land.
- Runtime rejects deprecated `host_call` fallback for all tools.

## Required Exports

Your `.wasm` tool must export:

- `memory` (linear memory)
- `alloc(i32) -> i32`
- `dealloc(i32, i32) -> ()`
- `execute(i32, i32) -> i64` (or custom entrypoint name)

### `execute` return format

`execute` returns a packed `i64`:

- High 32 bits: output pointer
- Low 32 bits: output length

Output bytes must be UTF-8 JSON.

## Tool Input/Output Contract

- Input: JSON object containing tool params.
- Output: JSON value (object preferred) consumed as tool result.

`host_call` is deprecated and rejected by runtime.

## Configuration Example (minimal)

```json
{
  "tools": {
    "settings": {
      "sandbox": {
        "mode": "all",
        "tools": {
          "coding": {
            "wasm": {
              "module": "./wasm/coding_tool.wasm",
              "entrypoint": "execute",
              "timeout_ms": 3000,
              "fuel": 5000000
            },
            "capabilities": {
              "abi_version": 1,
              "allow": [
                "log.emit"
              ]
            }
          }
        }
      }
    }
  }
}
```

## Notes

- Tool runtime is WASM-only for all built-in tools.
- `sandbox.mode` values are accepted for compatibility but do not bypass WASM execution.
- Per-tool `runtime` is ignored; tool execution remains WASM-only.
- Per-tool `wasm.module` is optional. If omitted, module path defaults to `./wasm/<tool>_tool.wasm`.
- `timeout_ms` interrupts long-running WASM execution by epoch deadline.
- `fuel` sets a deterministic instruction budget for guest execution.
- `capabilities.abi_version` validates ABI compatibility at startup (`1` supported).
- `capabilities.allow` is a per-tool allowlist for `capability_call.name`.
- Sandbox decisions are audit-logged through `ToolRegistry`.

## Important: Placeholder module caveat

The repository contains a helper crate at `wasm-tool/` that emits a **placeholder** WASM module for ABI smoke testing.

- It returns stub JSON (`"stub": true`) and does not execute real tool logic.
- It must **not** be copied to `./wasm/*_tool.wasm` for production/dev usage.
- Real per-tool modules are required for functional reminders/todo/tasks/planning behavior.

## Zero-Config Convention

If you place modules at:

- `./wasm/coding_tool.wasm`
- `./wasm/mcp_tool.wasm`
- `./wasm/http_call_tool.wasm`

then those tools run in WASM with no explicit sandbox config required.

For the top-level overview, see `README.md` → `Convention Mode (WASM-only tools)`.
