# Threat Model (Butterfly Bot)

_Last updated: 2026-02-15_

This document defines the explicit attacker model, trust boundaries, assets, and mitigations for Butterfly Bot as a local-first personal operations assistant.

It complements:

- `docs/security-audit.md` (runtime posture checks and operator guidance)
- `README.md` (product and operational overview)

## Security Objectives

1. Protect secrets (API keys, tokens) from accidental disclosure and unsafe storage paths.
2. Constrain built-in tool execution so agent capabilities remain intentionally bounded.
3. Preserve integrity of plans/tasks/reminders/memory across restarts and normal operation.
4. Keep local operator control explicit and observable (audit and diagnostics flows).
5. Favor fail-closed defaults for network/tool permissions where practical.

## System Scope

Primary components in scope:

- Desktop UI (`butterfly-bot-ui`)
- Local daemon (`butterfly-botd`)
- Tool execution layer (`src/tools/`)
- Guardrails and policy logic (`src/guardrails/`)
- Local data stores (SQLite/SQLCipher and sqlite-vec vectors)
- Secret handling and vault integration (`src/vault.rs`)
- Model provider integrations (`src/providers/`)

Out of scope:

- Host OS kernel compromise
- Physical device compromise while unlocked
- Supply-chain compromise in third-party binaries or model artifacts
- User-approved destructive actions initiated intentionally by the local operator

## Assets to Protect

High value:

- Provider/API secrets and daemon auth token
- Local memory and conversation history
- Planning/task/reminder state and schedules
- Tool permission configuration and policy intent

Moderate value:

- Diagnostic and audit outputs
- Runtime metadata and execution traces

## Trust Boundaries

1. **UI ↔ Daemon boundary**
   - IPC/local client calls cross from user-facing UI to privileged runtime operations.
   - Risk: unauthorized or malformed control actions.

2. **Daemon ↔ Tool runtime boundary**
   - Tool calls may touch filesystem, network, scheduling, and external systems.
   - Risk: capability escalation or policy bypass.

3. **Daemon ↔ External provider boundary**
   - Model/provider requests leave the local host.
   - Risk: data exfiltration and credential misuse.

4. **Daemon ↔ Local storage boundary**
   - Persistent memory/tasks/config references are loaded and written.
   - Risk: integrity corruption, plaintext secret leakage.

5. **Config intent ↔ Runtime behavior boundary**
   - User policy configuration must map to enforced runtime controls.
   - Risk: drift between configured and actual behavior.

## Attacker Model

### A1: Opportunistic local malware (same user context)

Capabilities:

- Reads accessible files and process environment under current user privileges.
- Attempts to locate plaintext secrets or insecure config defaults.

Primary goals:

- Exfiltrate API keys, tokens, and conversation/memory data.

### A2: Malicious or risky tool/prompt flow

Capabilities:

- Influences model/tool behavior through prompt/tool input patterns.
- Attempts to trigger high-risk local actions.

Primary goals:

- Cause unintended file/network/system operations.

### A3: Network-adjacent adversary on provider path

Capabilities:

- Observes or manipulates traffic if transport/channel assumptions fail.

Primary goals:

- Access prompt/context payloads or abuse leaked credentials.

### A4: Misconfiguration / operator error (non-malicious)

Capabilities:

- Weakens posture through permissive settings or unsafe secret placement.

Primary goals:

- Not malicious; accidental exposure or reliability degradation.

## Key Threats and Current Mitigations

### T1: Plaintext secret leakage

- Threat: keys/tokens stored inline in config files or logs.
- Mitigations:
  - OS keychain-backed vault paths for secret storage and resolution.
  - Security audit checks for inline secret hygiene.
  - Documentation guidance to avoid plaintext config secrets.
- Residual risk: host-level compromise under same user context can still access active session material.

### T2: Tool capability escalation

- Threat: built-in tool actions execute outside intended sandbox constraints.
- Mitigations:
  - Tool runtime planner enforces WASM-only execution for built-in tools.
  - Security audit validates the WASM-only runtime invariant across built-in tools.
  - Daemon startup validates tool WASM modules (magic header and non-stub checks) before serving requests.
  - Guardrails/policy checks enforce explicit execution intent.
- Residual risk: intended local automation still has meaningful capabilities by design.

### T3: Over-permissive network egress

- Threat: unrestricted outbound calls increase data exfiltration surface.
- Mitigations:
  - `default_deny` permission posture with explicit allowlists.
  - Security audit finding coverage for network policy posture.
- Residual risk: allowlist quality depends on operator configuration discipline.

### T4: Unauthorized daemon control path use

- Threat: daemon endpoints used without intended authorization control.
- Mitigations:
  - Daemon auth token requirement surfaced and audited.
  - Diagnostics and security audit expose auth posture issues.
- Residual risk: weak local host controls can still undermine token-based boundaries.

### T5: Data integrity loss in planning/memory workflows

- Threat: corruption or unintended mutation of reminders/tasks/memory state.
- Mitigations:
  - Local persistence model with schema-backed modules.
  - Existing workflow tests and scheduler coverage.
  - Diagnostics read/write probes for storage health.
- Residual risk: reliability proof should continue improving via published golden-path metrics.

## Assumptions

- Host OS and user account are reasonably trusted and maintained.
- Local filesystem permissions are not intentionally bypassed by the operator.
- Operator understands this is hardened local automation, not absolute isolation.
- External provider trust and transport security are inherited from configured endpoints.

## Non-Goals

- This model does not claim resistance to full host compromise.
- This model does not claim perfect containment against all prompt-injection variants.
- This model does not claim formal verification of every runtime path.

## Validation and Evidence Sources

- UI/daemon diagnostics flow (health checks)
- UI/daemon security audit flow (risk-ranked posture findings)
- Integration and workflow tests under `tests/`
- Guardrails and tool runtime enforcement paths in `src/`

## Ongoing Hardening Priorities

1. Maintain and continuously test WASM-only invariants for all built-in tools.
2. Expand reproducible golden-path reliability metrics in CI.
3. Improve operator-visible execution timeline and forensic trace depth.
4. Add machine-readable security audit artifact export for trend analysis.

## Change Control

Update this document when any of the following change:

- Tool runtime/sandbox policy behavior
- Secret storage or resolution semantics
- Daemon auth model
- Network permission policy defaults
- Memory/task/reminder persistence architecture