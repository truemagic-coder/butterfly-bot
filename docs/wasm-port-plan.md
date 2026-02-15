# WASM Tooling Migration Plan

## Purpose
Port all runtime tools to true WASM implementations (no `host_call` delegation) while preserving current behavior, security guarantees, and UX.

## Current State (as of 2026-02-15)
- Tool modules in `wasm-tool/` are feature-selected wrappers.
- Most tool execution still depends on native host logic via `host_call` payloads.
- Runtime in `src/sandbox/mod.rs` executes WASM modules but does not yet expose a formal host capability ABI for stateful operations (DB, network, secrets, MCP).

## Progress Update (2026-02-15)
- Started migration with **P1 tools first**: `todo`, `tasks`, `reminders`, `planning`, `wakeup`.
- Added tool-specific WASM action normalization + input validation in `wasm-tool/src/lib.rs`.
- Kept P2 tools (`http_call`, `search_internet`, `github`, `mcp`, `coding`) on passthrough delegation for now.
- Added unit coverage in `wasm-tool` for P1 normalization/validation paths.

## Target State
- Every tool executes primary logic inside WASM.
- Host interactions happen only through a constrained, auditable capability ABI.
- `host_call` fallback path is removed.
- Runtime is fail-closed: missing/invalid WASM implementation => hard error.

## Non-Goals
- No broad product redesign.
- No expansion of tool surface area during migration.
- No relaxing of security defaults to speed migration.

## Design Principles
1. Security first: least-privilege capabilities per tool.
2. Determinism: stable request/response contracts and reproducible tests.
3. Incremental migration: one tool group at a time, keep system operable.
4. Fail closed: no silent native fallback.
5. Observability: explicit audit trails for capability calls.

## Phase Plan

### Phase 0 — Freeze and Baseline
- Lock current behavior with golden tests for each tool action.
- Capture current schema/response contracts.
- Add tracking matrix for each tool action and expected outputs.

**Exit criteria**
- Baseline tests pass in CI.
- No unknown behavior branches for existing tool actions.

---

### Phase 1 — Capability ABI Spec
Define host-import ABI between WASM tool modules and host runtime.

Minimum capability families:
- `clock.now_unix()`
- `kv/sqlite.*` for persisted entities (todo/tasks/reminders/plans/wakeup)
- `http.request()` for external calls
- `secrets.get(name)` with strict allowlist
- `mcp.call(server, method, payload)` (optional early, required later)
- `log.emit(level, event)` for diagnostics/audit

Cross-cutting requirements:
- JSON-serializable request/response envelope
- structured error codes (`invalid_args`, `forbidden`, `timeout`, `internal`)
- per-call timeout and byte limits
- per-tool capability allowlist in config

**Exit criteria**
- ABI document reviewed and versioned (`abi_version=1`).
- Runtime validation rejects undeclared capabilities.

---

### Phase 2 — Runtime Host Bridge
Implement host import plumbing in runtime (`src/sandbox/mod.rs` + sandbox policy).

Deliverables:
- Import registration layer.
- Capability dispatcher with allowlist enforcement.
- Auditing for each capability call (tool, capability, args hash, result status, duration).
- Hard limits (input/output bytes, CPU/fuel, wall timeout).

**Exit criteria**
- Capability calls visible in audit logs.
- Disallowed capability call returns deterministic forbidden error.

---

### Phase 3 — Tool Migration (Deterministic First)
Migrate stateful deterministic tools first:
1. `todo`
2. `tasks`
3. `reminders`
4. `planning`
5. `wakeup`

Status:
- In progress. First implementation slice complete in WASM module: canonical action mapping + argument validation for all five P1 tools.

For each tool:
- Port action handlers to WASM module logic.
- Use ABI calls for persistence/time.
- Keep native implementation only for parity comparison until cutover.

**Exit criteria per tool**
- Action-level parity tests pass (native vs WASM output equivalence).
- Performance within agreed envelope.
- No `host_call` path used for migrated tool.

---

### Phase 4 — Network/Provider Tools
Migrate tools with external dependencies:
1. `http_call`
2. `search_internet`
3. `github`
4. `mcp`
5. `coding`

Additional controls:
- network domain allowlist/default-deny preserved
- secret access scoped to explicit names
- response size truncation/sanitization rules

**Exit criteria**
- Provider/network tools execute fully through WASM + capability bridge.
- Security audit checks pass for network and secret usage.

---

### Phase 5 — Cutover and Cleanup
- Remove `host_call` handling from registry.
- Remove legacy placeholder/stub pathways.
- Enforce artifact integrity checks and ABI compatibility checks at startup.
- Update docs and operational runbooks.

**Exit criteria**
- Full suite passes in WASM-only mode.
- No native fallback code path remains for tool execution.

## Tool Migration Matrix
| Tool | Dependencies | Risk | Priority |
|---|---|---:|---:|
| todo | sqlite | Low | P1 |
| tasks | sqlite, scheduler semantics | Medium | P1 |
| reminders | sqlite, time, UI stream semantics | Medium | P1 |
| planning | sqlite/json | Low | P1 |
| wakeup | sqlite/time | Medium | P1 |
| http_call | network policy | Medium | P2 |
| search_internet | provider APIs, secrets, policy | High | P2 |
| github | MCP transport + secrets | High | P2 |
| mcp | dynamic server calls | High | P2 |
| coding | provider API + larger payloads | High | P2 |

## Testing Strategy
1. Unit tests inside WASM tool crate for action logic.
2. Host-runtime integration tests for capability imports.
3. Native-vs-WASM parity tests per tool action.
4. Failure-mode tests: timeout, forbidden capability, malformed output, oversized payload.
5. End-to-end daemon/UI smoke tests to confirm user-visible behavior.

## Security Checklist
- Default deny capabilities per tool.
- Secrets never logged; only secret name references in audit.
- Strict network allowlist enforcement.
- WASM module hash validation during startup.
- ABI version negotiation and hard failure on mismatch.

## Delivery Milestones
- M1: ABI spec + runtime bridge skeleton
- M2: `todo/tasks/reminders/planning/wakeup` fully WASM
- M3: network/provider tools fully WASM
- M4: remove `host_call` fallback + docs/runbooks complete

## Suggested Execution Model
- Work in short tool-by-tool PRs.
- Require parity test + security checks for each PR.
- Defer cross-cutting refactors that are not migration blockers.

## Definition of Done
The migration is complete when:
- all tools run with real WASM logic,
- host interactions are capability-gated imports,
- no `host_call` delegation path exists,
- security and regression suites pass in CI,
- production startup validates module integrity and ABI compatibility.