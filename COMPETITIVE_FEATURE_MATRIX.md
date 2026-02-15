# Competitive Feature Matrix (Butterfly Bot vs OpenClaw, ZeroClaw, IronClaw)

_Last updated: 2026-02-14_

This is a practical, high-level comparison based on publicly visible repository documentation and code structure. It is meant to guide roadmap decisions, not declare an absolute winner.

## Positioning Snapshot

- **Butterfly Bot (this repo):** Practical personal-agent workflows with daemon + UI + planning/todo/tasks/reminders/wakeup + memory.
- **OpenClaw (main competitor):** Full personal-assistant platform with broad channels and plugin ecosystem, but currently high operational security risk for typical deployments.
- **ZeroClaw:** Lean, pluggable Rust agent framework with strong onboarding story and broad provider/channel coverage.
- **IronClaw:** Platform-style architecture emphasizing sandboxed extensibility (WASM), orchestration, routines, and gateway capabilities.

## Feature Matrix

Legend: **✅ strong**, **🟨 partial/limited**, **❌ not evident**

| Area | Butterfly Bot | OpenClaw | ZeroClaw | IronClaw |
|---|---:|---:|---:|---:|
| Rust core implementation | ✅ | ❌ (TypeScript-first) | ✅ | ✅ |
| Interactive UI included | ✅ (Dioxus UI) | ✅ (Control UI + WebChat) | 🟨 (CLI-first) | ✅ (TUI/Web gateway) |
| Local daemon/service model | ✅ | ✅ | ✅ | ✅ |
| Config persistence + reload path | ✅ | ✅ | ✅ | ✅ |
| Provider abstraction | ✅ | ✅ | ✅ | ✅ |
| Broad multi-provider catalog | 🟨 | ✅ | ✅ | 🟨 (focused provider path + adapters) |
| Agent extension architecture | ✅ (Rust-native modules + MCP integrations; maintainer-curated) | ✅ (plugins/extensions) | ✅ | ✅ |
| Secure tool sandbox model (explicit) | ✅ | 🟨 (sandbox/policy flows exist, but high-risk defaults and misconfiguration exposure remain common) | ✅ | ✅ |
| Memory subsystem | ✅ (SQLite + LanceDB paths/config) | ✅ (core memory + LanceDB plugin path) | ✅ (SQLite/Markdown + hybrid search) | ✅ (workspace memory + hybrid search) |
| Planning + todo/task orchestration | ✅ (native modules) | 🟨 | 🟨 | ✅ |
| Scheduled reminders/heartbeat style automation | ✅ | ✅ | ✅ | ✅ (routines/heartbeat) |
| End-user dynamic plugin building | ❌ (intentional: convention-over-configuration) | 🟨 (plugin/extensibility strong, not builder-centric) | ❌ | ✅ (WASM-oriented builder flow) |
| Zero-step onboarding (no wizard required) | ✅ | ✅ | ✅ | ✅ |
| Documentation breadth for contributors | ✅ | ✅ | ✅ | ✅ |
| Explicit security hardening docs/checklists | ✅ | ✅ | ✅ | ✅ |
| Test breadth/visibility | ✅ | ✅ | ✅ | 🟨 |

## Weighted Scorecard (Personal Ops Agent Lens)

Scoring model:
- Score each criterion from **1 to 5** (5 = strongest).
- Weight reflects importance for a **personal operations assistant** product.
- Weighted score per row = `score × weight`.
- Total possible = **500** (if all criteria scored 5).

### Criteria and Weights

| Criterion | Weight (%) | Why it matters |
|---|---:|---|
| Workflow completeness (plan→task→reminder→done) | 20 | Core product value for daily execution. |
| Reliability and failure recovery | 20 | Users trust consistency more than raw feature count. |
| UX and operator visibility | 15 | Faster adoption and better day-2 usability. |
| Security posture and secret hygiene | 15 | Critical for real-world deployment and trust. |
| Setup/onboarding speed | 10 | Strong determinant of conversion and retention. |
| Integration leverage and extensibility | 10 | Measures practical capability breadth, including MCP partner surfaces (e.g., Zapier) and native agent extension velocity. |
| Documentation and contributor DX | 10 | Impacts community velocity and maintainability. |

### Current Scoring (Post-Ship Estimate)

Scoring reflects current shipped state for Butterfly Bot after landing local golden-path reliability checks, execution-trace sanity coverage, and trace redaction hardening. It should still be revised as competitors evolve.

| Criterion | Weight | Butterfly Bot | OpenClaw | ZeroClaw | IronClaw |
|---|---:|---:|---:|---:|---:|
| Workflow completeness | 20 | 5 | 4 | 3 | 4 |
| Reliability and recovery | 20 | 5 | 3 | 4 | 3 |
| UX and visibility | 15 | 5 | 4 | 3 | 4 |
| Security posture | 15 | 5 | 1 | 5 | 4 |
| Setup/onboarding | 10 | 5 | 4 | 5 | 4 |
| Integration leverage/extensibility | 10 | 4 | 5 | 5 | 5 |
| Docs/contributor DX | 10 | 5 | 4 | 5 | 4 |
| **Total Weighted (/500)** | **100** | **490** | **345** | **415** | **390** |

### Readout

- **Butterfly Bot strength:** strongest personal-ops loop fit, with shipped WASM defaults, diagnostics, keychain-backed secrets, and now local reproducible reliability/trace sanity coverage.
- **Primary competitive gap vs OpenClaw:** connector breadth/packaging/community scale, not trust/reliability baseline.
- **Score impact from shipped items:** **445 → 490 (+45)** via Reliability (4→5), UX/Visibility (4→5), and Docs/DX (4→5).

## Capability Reality Check (Current)

### Shipped Baseline

- **WASM-first sandbox posture:** convention-mode defaults route high-risk tools (`coding`, `mcp`, `http_call`) to WASM runtime.
- **Diagnostics (Doctor):** daemon endpoint + UI flow with actionable health checks.
- **Security audit (UI-first):** ranked findings with remediation guidance.
- **OS keychain secret handling:** config/API secrets resolved through keychain-backed vault paths.
- **Golden-path reliability sanity suite (local):** deterministic planning→tasks→reminders + restart checks are now in test coverage.
- **Execution trace sanity suite (local):** trace schema/order + payload redaction checks are now covered in tests.
- **WASM-only runtime invariant:** runtime planning/execution stays WASM-only for tool execution paths.

### Security Note (Current State)

- Secrets are stored/resolved via OS keychain-backed vault paths (`vault`/`keyring`) instead of plaintext config defaults.
- High-risk tool execution is designed for WASM runtime by default in convention mode.
- Security audit + diagnostics are built-in and actionable in UI/daemon flows.
- Residual risk remains: local file/system capabilities are still part of intended agent behavior, so this is **hardened local automation**, not perfect isolation.

## Quick Read: Who Is "Better"?

There is no universal "better". It depends on your target user and product promise:

- If the goal is **broad channel platform + rapid ecosystem scale**, OpenClaw currently signals strength, but with significant security tradeoffs.
- If the goal is **small, pluggable infra with broad provider/channel ecosystem**, ZeroClaw currently signals strength.
- If the goal is **sandbox-heavy extensibility + orchestration platform**, IronClaw currently signals strength.
- If the goal is **opinionated personal operations assistant** (planning + tasks + reminders + wakeup + UI), Butterfly Bot has a strong wedge.

## Notes

- This matrix should be updated as features evolve.
- Treat this as a living document for roadmap prioritization and release planning.
