# Butterfly Bot "Everyone" Plan

## Objective

Make Butterfly Bot run reliably on Windows, macOS, and Linux with graceful degradation, lower hardware friction, and explicit provider choice (Ollama vs OpenAI).

## Product Principles

1. **Works everywhere first**: install and first run must succeed on mainstream machines.
2. **Graceful degradation**: missing TPM/GPU/VRAM never blocks core chat + automation.
3. **Clear provider UX**: one explicit model provider selector for assistant/runtime.
4. **Daemon independence**: UI lifecycle and daemon lifecycle are separate concerns.
5. **Safe-by-default**: security features auto-enable when available and visible.

## Scope for This Plan

- Cross-platform parity for launcher/daemon behavior.
- Config UI support for runtime provider selection: `Ollama` or `OpenAI`.
- Config UI support for a first-class Doctor panel (status + checks + actions).
- OpenAI hosted defaults for:
  - completions/chat model,
  - reranking model,
  - small embedding model.
- TPM capability detection + feature gating (non-fatal when unavailable).
- Explicit daemon start/stop semantics independent of window close.

## Workstream A — Provider UX + Config Model

### A1. Add explicit provider selection

Add a runtime provider field in config:

- `provider.runtime = "ollama" | "openai"`
- Keep OpenAI-compatible structure for base URL/API key where needed.

### A2. UI fields

In Config UI, add:

- Runtime Provider dropdown: `Ollama` / `OpenAI`.
- If `Ollama`:
  - show local base URL and local model fields.
- If `OpenAI`:
  - hide local install hints,
  - show API key field,
  - show fixed default model set with optional advanced override.

### A3. OpenAI hosted defaults

Define app defaults (owner-selected) for:

- primary chat/completions model,
- reranker,
- embedding-small model.

### A4. Validation behavior

- Validate provider-specific required fields.
- If provider is `OpenAI`, skip local Ollama install/model pull checks.
- If provider is `Ollama`, keep current local bootstrap behavior.

### A5. Acceptance criteria

- User can switch provider in UI and save successfully.
- Restart preserves provider state.
- Health check reports provider-specific readiness.
- No forced Ollama install when OpenAI is selected.

## Workstream B — TPM Graceful Degradation

### B1. Capability modes

Introduce explicit security capability mode:

- `strict` (current fail-closed behavior),
- `compatible` (default for broad install success),
- `auto` (strict features enabled when TPM secure path is available).

### B2. Feature gating

If TPM unavailable or broken:

- core bot still runs,
- mark TPM-bound features unavailable,
- Solana signing/wallet-seed custody is disabled unless a secure key backend is available,
- present actionable status in Doctor/Security UI,
- do not fail app startup.

### B3. Secret handling fallback policy

- Keep encrypted-at-rest baseline.
- If strict TPM operations are unavailable, use non-TPM secure backend path where allowed by policy.
- Record selected protection level in diagnostics.

### B4. Acceptance criteria

- Machine without TPM starts and runs core workflows.
- Machine with TPM automatically enables stricter mode.
- Diagnostics clearly display active security level and reasons.

## Workstream C — Daemon/UI Lifecycle Decoupling

### C1. Behavioral contract

- Closing UI window does **not** stop daemon.
- Daemon stops only when user clicks `Stop` in UI (or explicit daemon command is issued).

### C2. Implementation direction

- Prefer separate daemon process ownership over in-UI thread ownership.
- UI should connect/disconnect from daemon without affecting daemon process.
- Keep explicit `Start`/`Stop` controls and status polling.

### C3. Acceptance criteria

- Close/reopen UI while daemon continues serving tasks/reminders.
- `Stop` from UI terminates daemon only when UI initiated/connected to that daemon instance.
- No orphaned zombie processes after repeated start/stop cycles.

## Workstream D — Cross-Platform Reliability

### D1. Packaging parity

- Ship and verify install artifacts for Linux/macOS/Windows.
- Ensure first-run flow and runtime paths are consistent.

### D2. Compatibility matrix

Test matrix by:

- OS (Win/macOS/Linux),
- provider (Ollama/OpenAI),
- TPM state (available/unavailable),
- hardware tier (low/mid/high memory).

### D3. Acceptance criteria

- All matrix lanes complete onboarding and send a first message.
- Provider switch works in all OS lanes.
- Non-TPM lanes pass with degraded security features indicated, not blocked.

## Workstream E — Observability + UX Confidence

### E0. Doctor UI in Config tab

Add a dedicated Doctor section in Config with:

- `Run Doctor` action,
- per-check status cards (pass/warn/fail),
- actionable remediation text and quick links,
- security mode + TPM capability summary,
- provider readiness summary (Ollama/OpenAI specific),
- daemon lifecycle summary (running, owned, detached).

### E1. Doctor upgrades

Add checks for:

- provider config completeness,
- daemon ownership/lifecycle state,
- TPM capability and active security mode,
- model reachability for selected provider.

### E2. Onboarding copy

- Make provider choice explicit in first-run guidance.
- Explain security mode in plain language.

### E3. Acceptance criteria

- User can self-diagnose setup issues from in-app Doctor output.
- Error messages provide next action, not only failure text.
- Doctor UI is discoverable in Config without terminal usage.

## Delivery Phasing

### Phase 1 (Fastest user impact)

- Provider dropdown in Config UI.
- OpenAI hosted defaults wired.
- Skip Ollama bootstrap when OpenAI selected.

### Phase 2

- TPM `auto/compatible/strict` mode with non-fatal startup on non-TPM machines.
- Security/Doctor visibility for gated features.

### Phase 3

- Full daemon/UI decoupling with persistent daemon process behavior.
- Lifecycle ownership semantics finalized.

### Phase 4

- Cross-platform test matrix automation and release hardening.

## Suggested Technical Touchpoints

- Config schema/defaults: `src/config.rs`
- UI settings and provider controls: `src/ui.rs`
- Launcher bootstrap logic (Ollama install/model pulls): `src/main.rs`
- TPM capability + policy behavior: `src/security/tpm_provider.rs`
- Secret migration strictness assumptions: `src/security/migration.rs`
- Daemon startup/shutdown semantics in UI: `src/ui.rs`

## Non-Goals (for this plan)

- Replacing current memory architecture.
- Broad plugin-system redesign.
- Removing strict security mode entirely.

## Definition of Done

1. New users on Windows/macOS/Linux can complete first-run without hardware blockers.
2. Provider can be switched between Ollama and OpenAI from UI in < 30 seconds.
3. Missing TPM no longer blocks core functionality; gated features are clearly labeled.
4. Closing UI does not stop daemon; explicit stop does.
5. CI includes coverage for provider + TPM + OS compatibility lanes.
