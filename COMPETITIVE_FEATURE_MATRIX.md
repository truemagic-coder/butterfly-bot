# Competitive Feature Matrix (Butterfly Bot vs OpenClaw, ZeroClaw, IronClaw)

_Last updated: 2026-02-14_

This is a practical, high-level comparison based on publicly visible repository documentation and code structure. It is meant to guide roadmap decisions, not declare an absolute winner.

## Positioning Snapshot

- **Butterfly Bot (this repo):** Practical personal-agent workflows with daemon + UI + planning/todo/tasks/reminders/wakeup + memory.
- **OpenClaw (main competitor):** Full personal-assistant platform with broad channels, mature security guidance/audit tooling, and plugin ecosystem.
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
| Tool/plugin architecture | ✅ | ✅ (plugins/extensions) | ✅ | ✅ |
| Secure tool sandbox model (explicit) | ✅ | ✅ (sandbox + policy/audit flows) | ✅ | ✅ |
| Memory subsystem | ✅ (SQLite + LanceDB paths/config) | ✅ (core memory + LanceDB plugin path) | ✅ (SQLite/Markdown + hybrid search) | ✅ (workspace memory + hybrid search) |
| Planning + todo/task orchestration | ✅ (native modules) | 🟨 | 🟨 | ✅ |
| Scheduled reminders/heartbeat style automation | ✅ | ✅ | ✅ | ✅ (routines/heartbeat) |
| Dynamic tool building | ❌ | 🟨 (plugin/extensibility strong, not builder-centric) | ❌ | ✅ (WASM-oriented builder flow) |
| Zero-step onboarding (no wizard required) | 🟨 | ✅ | ✅ | ✅ |
| Documentation breadth for contributors | 🟨 | ✅ | ✅ | ✅ |
| Explicit security hardening docs/checklists | ✅ | ✅ | ✅ | ✅ |
| Test breadth/visibility | 🟨 | ✅ | ✅ | 🟨 |

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
| Extensibility and integrations | 10 | Determines long-term expansion potential. |
| Documentation and contributor DX | 10 | Impacts community velocity and maintainability. |

### Current Scoring (Estimate)

| Criterion | Weight | Butterfly Bot | OpenClaw | ZeroClaw | IronClaw |
|---|---:|---:|---:|---:|---:|
| Workflow completeness | 20 | 5 | 4 | 3 | 4 |
| Reliability and recovery | 20 | 4 | 4 | 4 | 3 |
| UX and visibility | 15 | 4 | 5 | 3 | 4 |
| Security posture | 15 | 5 | 5 | 5 | 4 |
| Setup/onboarding | 10 | 3 | 5 | 5 | 4 |
| Extensibility/integrations | 10 | 3 | 5 | 5 | 5 |
| Docs/contributor DX | 10 | 4 | 5 | 5 | 4 |
| **Total Weighted (/500)** | **100** | **415** | **455** | **415** | **390** |

### Readout

- **Butterfly Bot strength:** strongest "personal ops loop" fit with materially improved sandboxing and operator diagnostics.
- **Primary competitive gap vs OpenClaw:** onboarding polish and ecosystem breadth.
- **Best leverage:** improve zero-step first-run and integrations without changing product identity.

## Status Update: Issues #3, #4, #5

- **#3 Sandbox architecture:** shipped (closed). Convention-first WASM defaults, runtime routing, and related tests/docs are in place.
- **#4 Doctor diagnostics:** shipped (closed) via daemon diagnostics endpoint + UI diagnostics panel and integration coverage.
- **#5 Security audit:** partially shipped (open issue) via daemon security-audit endpoint + UI posture checks + ranked findings docs; safe auto-fix path remains follow-up work.

## Butterfly Bot Priority Scoreboard (Next 6 Weeks)

Scoring formula: `Impact (1-5) × Urgency (1-5) ÷ Effort (1-5)`

| Initiative | Impact | Urgency | Effort | Priority Score | Priority |
|---|---:|---:|---:|---:|---|
| Secret hygiene + key rotation + CI secret scan | 5 | 3 | 2 | 7.5 | P1 |
| Diagnostics hardening (daemon + UI) | 4 | 3 | 2 | 6.0 | P2 |
| Golden-path eval suite for planning/task/reminder | 5 | 4 | 3 | 6.7 | P1 |
| Zero-step first run autopilot | 4 | 3 | 3 | 4.0 | P2 |
| Public benchmark and capability dashboard | 4 | 3 | 3 | 4.0 | P2 |
| Expanded integration/plugin packs | 3 | 2 | 4 | 1.5 | P3 |

### Priority Interpretation

- **P0:** Do immediately (trust-risk or launch blocker).
- **P1:** Highest ROI roadmap items after P0.
- **P2:** Important but can follow once reliability/trust baseline is fixed.
- **P3:** Nice-to-have until core differentiation is secure.

### Security Note (Current State)

- The project uses OS keychain-backed secret storage (`vault`/`keyring`) for sensitive config paths.
- Local filesystem access still exists for intended app behavior (UI/config/markdown workflows), so security posture is **strong but not absolute isolation**.

## Quick Read: Who Is "Better"?

There is no universal "better". It depends on your target user and product promise:

- If the goal is **broad channel platform + mature operational security controls + docs depth**, OpenClaw currently signals strength.
- If the goal is **small, pluggable infra with broad provider/channel ecosystem**, ZeroClaw currently signals strength.
- If the goal is **sandbox-heavy extensibility + orchestration platform**, IronClaw currently signals strength.
- If the goal is **opinionated personal operations assistant** (planning + tasks + reminders + wakeup + UI), Butterfly Bot has a strong wedge.

## Where Butterfly Bot Can Win Fast

1. **Own the "Personal Ops Agent" category**
   - Make planning → tasks → reminders → completion loop visibly best-in-class.
2. **Reliability as a differentiator**
   - Publish workflow success metrics (not just features).
3. **Security trust baseline**
   - Enforce secret hygiene (env/vault only, no plaintext tokens in config examples).
4. **Onboarding speed**
   - One launch to first useful workflow in <5 minutes.
5. **Operator visibility**
   - Clear status/audit UX: what the bot is doing, why, and what changed.

## Gap Backlog (High Impact)

- Add a **formal threat model** document that complements existing security audit docs.
- Expand diagnostics coverage in daemon/UI flows with machine-readable response output.
- Add **golden-path evaluation suite** for planning/task/reminder workflows.
- Keep **no-wizard** onboarding and improve zero-step first-run defaults.
- Publish a **benchmark + capability page** with reproducible scripts.

## 30-Day Execution Plan (MVP)

### Week 1 — Trust + Setup
- Secret hygiene pass (remove plaintext examples, rotate compromised keys, add scanning in CI).
- Improve zero-step first-run and inline missing-secret messaging.

### Week 2 — Reliability
- Add workflow regression tests for:
  - planning creation
  - task scheduling and polling
  - reminder triggering
  - daemon restart resilience

### Week 3 — Product Edge
- Tighten end-to-end UX for planning → tasks → reminders loop in UI.
- Add explicit execution trace/status timeline.

### Week 4 — Public Proof
- Release a reproducible benchmark matrix:
  - task completion success rate
  - time-to-first-useful-output
  - mean retries per workflow
  - cost/token usage per scenario

## Beat OpenClaw Plan (8 Concrete Initiatives)

Goal: close the highest-value competitive gaps while preserving Butterfly Bot’s personal-ops identity.

| # | Initiative | Mapped Modules | Success Metric |
|---:|---|---|---|
| 1 | **Diagnostics hardening** for config, vault, DB, provider, and daemon health (UI + daemon) | `src/config.rs`, `src/config_store.rs`, `src/db.rs`, `src/vault.rs`, `src/daemon.rs`, `src/ui.rs` | Diagnostics return actionable pass/warn/fail checks in <5s |
| 2 | **Zero-step first run autopilot** (built-in defaults + inline keychain flow, no wizard) | `src/main.rs`, `src/config.rs`, `src/config_store.rs`, `src/vault.rs` | New user reaches first successful message in <5 minutes |
| 3 | **Security audit safe auto-fix follow-up** | `src/guardrails/`, `src/interfaces/guardrails.rs`, `src/config.rs`, `src/daemon.rs`, `src/ui.rs` | Security audit catches exposed-risk configs and offers fix suggestions |
| 4 | **Policy-enforced filesystem/network permissions** per tool profile | `src/tools/mod.rs`, `src/tools/http_call.rs`, `src/guardrails/mod.rs`, `src/interfaces/plugins.rs` | Restricted profile blocks disallowed file/network actions 100% in tests |
| 5 | **Golden-path reliability eval suite** for planning→tasks→reminders | `tests/brain_scheduler.rs`, `tests/query_service.rs`, `tests/agent_service.rs`, `tests/daemon.rs`, `src/planning/`, `src/tasks/`, `src/reminders/` | Stable CI pass rate for core workflows over 30 consecutive runs |
| 6 | **Execution timeline in UI** (what happened, when, and outcome) | `src/ui.rs`, `src/daemon.rs`, `src/tools/`, `src/scheduler/` | User can inspect end-to-end action trace for each run in one screen |
| 7 | **Integration packs v1** (high-value presets for common workflows) | `src/plugins/registry.rs`, `src/tools/`, `src/factories/agent_factory.rs`, docs in `README.md` | At least 3 production-ready packs shipped with setup docs |
| 8 | **Public benchmark + capability report** with reproducible scripts | `benches/`, `README.md`, new `docs/benchmarks.md` (or equivalent) | Published benchmark table with scripts users can run unchanged |

### Sequencing (Recommended)

- **Phase A (Trust + Operations):** #1, #3, #4
- **Phase B (Onboarding + Reliability):** #2, #5
- **Phase C (Differentiation + Proof):** #6, #7, #8

### Why This Beats OpenClaw

- Focuses on your strongest wedge (personal operations loop) rather than feature parity everywhere.
- Improves operator trust and diagnosability where OpenClaw currently appears mature.
- Produces measurable proof (reliability + benchmarks) instead of subjective claims.

## Notes

- This matrix should be updated as features evolve.
- Treat this as a living document for roadmap prioritization and release planning.
