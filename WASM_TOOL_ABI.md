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
- Sandbox decisions are audit-logged through `ToolRegistry`.

## Zero-Config Convention

If you place modules at:

- `./wasm/coding_tool.wasm`
- `./wasm/mcp_tool.wasm`
- `./wasm/http_call_tool.wasm`

then those tools run in WASM with no explicit sandbox config required.

For the top-level overview, see `README.md` → `Convention Mode (WASM-only tools)`.
