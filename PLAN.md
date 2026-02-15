# ButterFly Bot (Rust) — Local‑First Roadmap

## Goals
- Local‑first, Ollama‑primary assistant with strong privacy defaults.
- Robust local memory and search with Diesel + SQLite.
- Convention‑first tools with optional config overrides and guarded execution.
- Polished desktop UI UX with zero‑step onboarding and safe defaults.

## Non‑Goals (for now)
- Cloud‑hosted multi‑tenant services.
- Public‑facing gateways or web UIs.
- Full multi‑channel messaging network parity with Moltbot.

## Recent Completions (2026-02-14)
- [x] Sandbox architecture shipped with convention-first WASM defaults and runtime routing (Issue #3).
- [x] Diagnostics shipped via daemon endpoint + UI diagnostics flow (Issue #4).
- [ ] Follow-up: security audit safe auto-fix path remains (Issue #5).

## Phase 0 — Foundations (now)
- [x] Ollama OpenAI‑compat config support.
- [ ] Local‑first model defaults (Ollama + GPT‑OSS:20b preset).
- [x] Streaming desktop UI chat rendering.
- [x] Tool scaffolding + JSON config hooks.
- [ ] Doc: Local‑first architecture + threat model summary.
- [ ] Replace config.json with DB + OS keyring.

## Phase 1 — Storage & Memory (Pluggable, Local‑First)
**Objective:** Add durable local memory with search and retention controls.

1. Dual storage backend (local‑first)
   - SQLite for metadata + transcripts (embedded, zero‑install).
   - LanceDB for vector memory + semantic retrieval (embedded/local‑only).
   - Provide a `Storage` trait with a pluggable backend, but run both by default.

2. Memory behavior
   - Append all turns to `messages`.
   - Summarize when token budget exceeds threshold.
   - Store summaries in `memories` with tags + timestamps.

3. Query & retrieval
   - SQLite FTS5 for keyword + recency.
   - LanceDB embeddings + ANN for semantic recall.
   - Merge & rerank (recency + similarity + tag overlap).

4. Config
   - `memory.enabled` (bool)
   - `memory.db_path` (string)
   - `memory.retention_days` (u32)
   - `memory.summary_model` (string)

## Phase 2 — Local Daemon + Desktop Client
**Objective:** A local background service with a thin desktop UI client.

1. Daemon
   - Run on loopback only by default.
   - Unix socket or local TCP port with token auth.
   - Process model requests and tool calls.

2. Desktop UI Client
   - Chat view for interactive conversations.
   - Config view for settings and provider management.
   - Diagnostics/security views for health and posture checks.

3. First‑run autopilot (no wizard, no required config.json)
   - App launch starts with built‑in defaults for provider/model/storage/tooling.
   - Required secrets are requested only when execution is blocked, then stored in OS keyring (macOS Keychain / libsecret).
   - Optional customization can be done later via Config UI.
   - Safe defaults: loopback‑only, no remote exposure.
   - Never write secrets to disk or logs.

4. Config storage model
   - **SQLite** stores all non‑secret config (models, tools, routing, memory).
   - **Keyring** stores all secrets (API keys, private keys, tokens).
   - Config loading path: DB first, then environment overrides (non‑secret only).
   - Provide redacted config visibility in the UI.

## Phase 3 — Tools Platform (Local‑First)
**Objective:** Powerful but safe tools with shareable, secret‑free configs.

1. Config model (DB‑first, shareable JSON)
   - Add config export/import in desktop UI (redacted secrets).
   - Optional per-session config profile override in desktop UI.
   - JSON may reference vault keys by name (no secrets in files).

2. Simple tool spec + permissions
   - Tool definition lives in config: name, description, endpoint, permissions.
   - Permissions: fs_read/fs_write allowlists and network domains.
   - Default deny for dangerous capabilities.

3. Minimal tool management
   - UI: tools list, enable/disable, and details.
   - Per‑agent allowlists in config/DB.

4. Internal‑only tools (until traction)
   - Tools are first‑party and shipped in the repo.
   - PR‑reviewed only; no dynamic loading, no external installs.
   - No bundles, no registry, no crates.io discovery.

5. Safety controls
   - Global “safe mode” toggle.
   - Explicit approval prompts for new capabilities.
   - Local audit log of tool usage.

6. QA + docs
   - Integration tests for permissions + enable/disable.
   - Doc: simple tool authoring and local install.

## Phase 4 — Brain Plugins + Background Scheduler
**Objective:** Add a pluggable “brain” system and cron‑like background tasks.

1. Brain plugin system
   - Plugin manager with discovery, load/unload, enable/disable.
   - Clear plugin manifest (name, version, deps, permissions).
   - Execution order + timeouts; safe failure isolation.

2. Background scheduler
   - First‑party scheduler service (cron‑like intervals).
   - Jobs can run tools or plugins on a schedule.
   - Task registry for built‑ins + user‑installed plugins.

3. Default recurring jobs
   - Conversation auto‑completion after inactivity.
   - Memory cleanup / retention enforcement.
   - Optional health checks and model availability checks.

4. UI control
   - Scheduler list/start/stop/run in desktop controls.
   - Plugin list/enable/disable/reload in desktop controls.

5. Safety
   - Per‑plugin permissions + explicit approval.
   - Scheduler can be disabled globally.

## Phase 5 — Security Hardening
**Objective:** Make local default safe and predictable.

- Strict allowlists for execution tools.
- Redaction of secrets in logs.
- Environment isolation for execution tools.
- Clear warnings for risky config.

## Phase 6 — UX Polishing
**Objective:** Improve adoption and day‑to‑day use.

- Themed desktop UI + rich rendered output (done).
- Usage footer toggle.
- Session export/import.
- Auto‑patching for upgrades (Patchify): https://github.com/danwilliams/patchify

## Phase 6 — Optional Extensions (Future)
- Web UI or TUI dashboard.
- Multi‑channel adapters (if needed).
- Node actions (camera/screen) with local consent.

## Phase 7 — Marketplace + Payments (x402 + $AGENT)
**Objective:** Enable a prompt-context marketplace with on‑chain purchase and local installs.

- x402 payment flow for prompt-context purchases.
- $AGENT token support for marketplace pricing.
- Signed prompt-context bundles + local verification.
- Clear refund/rollback path for failed installs.

## Storage Implementation Notes
- Keep `Storage` trait stable across backends.
- SQLite: embedded migrations + FTS5 for logs/summaries.
- LanceDB: embeddings + metadata; store raw transcripts in SQLite.
- Use `tokio::task::spawn_blocking` for any sync DB work.

## Memory Design Inspiration (Graphiti)
- Use Graph‑style memory links (entities ↔ events ↔ facts) as a local embedded pattern.
- Implement a lightweight relationship table in SQLite to model edges.
- Provide a “memory graph” view for explainability.
- Keep everything embedded/local‑only (no external DB services).

## Deliverables Checklist
- [ ] Storage trait + SQLite impl
- [ ] Migration setup
- [ ] Memory ingest + summarize
- [ ] Search + retrieval
- [ ] Daemon + client
- [ ] Tool safety model
- [ ] Zero‑step onboarding autopilot
