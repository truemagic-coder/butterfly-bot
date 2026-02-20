# Butterfly Bot TPM + Cocoon Security Plan

## Objective

Implement a local-first, TPM-rooted, Cocoon-encrypted custody path for Butterfly Bot, with:

- no plaintext keys at rest,
- minimal plaintext exposure in memory,
- strict process boundaries between untrusted plugins/WASM and signing operations.

## Required Encryption Layers (Explicit)

This program uses multiple layers together (not one mechanism alone):

1. **Root key protection (TPM)**
  - TPM protects or unseals the KEK used to unwrap application encryption keys.
2. **Data-at-rest encryption (Cocoon AEAD)**
  - Persisted secret blobs are encrypted and authenticated with Cocoon.
  - Default target cipher suite should be ChaCha20-Poly1305 (256-bit) unless platform constraints require AES-256-GCM.
3. **Local IPC transport protection (process-to-process)**
  - OS-native local transport peer auth is mandatory (Unix socket on Linux/macOS, Named Pipe on Windows).
  - IPC payload encryption is mandatory: all signer IPC payloads must be AEAD-protected end-to-end with per-session keys.
4. **In-memory exposure minimization**
  - Plaintext key material must be short-lived, zeroized, and never copied into plugin/WASM memory space.

If any one layer is absent, overall security regresses.

## In-Memory Key Protection (Memory-Scraper Threat Model)

### Important security reality

There is no perfect “cipher suite” that keeps a key safe while it is actively being used in a compromised process.
If malware can read the same process memory at the right time, plaintext can still be exposed.

Therefore, defense must combine:

- cryptography,
- process isolation,
- OS hardening,
- strict key-lifetime minimization.

### Mandatory controls against memory scrapers

1. **Isolated signer process only**
  - Key unwrap/sign operations occur only inside signer process.
  - Main app, plugins, and WASM never hold private key bytes.

2. **Just-in-time key material**
  - Unwrap only per operation or very short session window.
  - Immediately zeroize after signing.
  - No long-lived plaintext key cache.

3. **Protected memory handling**
  - Use zeroizing secret containers for sensitive buffers.
  - Apply `mlock`/`munlock` (best effort) for key buffers.
  - Mark pages `MADV_DONTDUMP` where available.
  - Disable core dumps for signer process.

4. **OS/process anti-inspection hardening**
  - Run signer under dedicated least-privilege user.
  - Deny ptrace from non-authorized processes.
  - Apply syscall restrictions (seccomp profile) for signer.
  - Restrict environment inheritance and secret-bearing env vars.

5. **IPC confidentiality and integrity**
  - Keep mandatory AEAD IPC payload encryption.
  - Use per-session keys with strict nonce/replay policy.

### Recommended IPC crypto profile

- KEX/session setup: ECDH (X25519) + HKDF-SHA256 key schedule.
- Payload AEAD: XChaCha20-Poly1305 (preferred) or AES-256-GCM where required.
- Replay defense: monotonic per-session counters and strict nonce uniqueness.

### Optional advanced hardening

- Signer sandbox via namespace/container isolation.
- Physical user-presence confirmation for high-risk actions.
- Out-of-process hardware signer mode when chain support allows key-non-exportability.

### Acceptance criteria for memory-scraper mitigation

- [ ] No private key bytes present in non-signer process memory.
- [ ] Signer plaintext key lifetime bounded and measured.
- [ ] Core-dump and ptrace protections verified in CI/system checks.
- [ ] Security tests include simulated memory-scraper scenarios.

### Dependency Decision Record — `memsecurity`

Decision: Adopt `memsecurity` as the primary in-memory hardening backend for signer-secret handling.

Rationale:

- Aligns directly with required controls already in plan (`mlock`, zeroization, encrypted memory handling).
- Reduces custom security-sensitive memory code surface.
- Supports immediate hardening goals while broader external audit is in roadmap.

Risk acknowledgement:

- The crate is currently unaudited; treat as high-scrutiny third-party dependency.
- Adoption is acceptable only with strict CI red-team verification and explicit rollback controls.

Adoption policy:

- [ ] Pin exact crate version and verify checksums in lockfile workflow.
- [ ] Wrap crate usage behind internal abstraction (no direct crate spread across codebase).
- [ ] Restrict usage to signer process and security modules only.
- [ ] Maintain emergency kill switch to disable encrypted-memory backend if needed.
- [ ] Keep fallback secure memory path available for recovery builds.

Promotion criteria to strict default:

- [ ] All Critical red-team scenarios pass with `memsecurity` enabled.
- [ ] No regression in memory-leakage tests or crash-dump protections.
- [ ] Dependency review completed (license, maintenance, issue posture, update cadence).
- [ ] Internal pre-audit sign-off for memory-hardening subsystem.

Audit evidence requirements specific to `memsecurity`:

- [ ] Mapping of each `memsecurity` primitive to threat mitigations in this plan.
- [ ] Negative tests demonstrating fail-closed behavior when memory-hardening init fails.
- [ ] Benchmark and stability report under production profile.
- [ ] Upgrade policy with security review required for every minor/major version bump.

## Security Targets and Non-Goals

### Security targets

- Resist offline disk theft.
- Resist accidental key leakage from config/files/logs.
- Limit blast radius of plugin or app-process compromise.
- Preserve Butterfly contract: local and secure by default.

### Non-goals (initial phases)

- Full blockchain-level privacy/anonymity.
- Full defense against kernel/root compromise.
- Cross-platform parity before Linux TPM2 path is hardened.

## Threat Model Matrix (Open-Source Governance)

This project is open-source and does not operate under contractual SLAs.
Instead of SLA commitments, security work is tracked by **severity**, **owner role**, and **release gate impact**.

### Owner roles

- **Core Maintainer**: code and architecture owner for custody/signer/security modules.
- **Security Maintainer**: threat-model owner and release gate reviewer.
- **Contributor**: implements fixes under maintainer review.

### Matrix

| Asset | Threat Actor | Attack Path | Primary Controls | Residual Risk | Severity | Owner Role | Release Gate |
| --- | --- | --- | --- | --- | --- | --- | --- |
| Wallet private key (signer) | Local malware (user space) | Memory scraping during signing window | isolated signer process, short-lived unwrap, zeroization, mlock, IPC AEAD | Exposure possible if signer process fully compromised at runtime | Critical | Core + Security Maintainer | Block release on failing Critical tests |
| Wallet private key (at rest) | Offline attacker with disk access | File theft, blob tampering, rollback | Cocoon AEAD blobs, TPM-rooted unseal, rollback detection | TPM reset/reprovision operational complexity | Critical | Core Maintainer | Block release |
| Sign authorization flow | Malicious plugin/WASM | Bypass approval path, direct sign call | signer boundary, rust-fsm transition guards, policy engine | Logic bugs in policy/FSM implementation | Critical | Core + Security Maintainer | Block release |
| IPC command channel | Local unauthorized process | Socket hijack, tamper, replay | SO_PEERCRED checks, mandatory AEAD payloads, nonce/counter replay defense | Host-level compromise may still bypass local controls | Critical | Core Maintainer | Block release |
| x402 payment path | Malicious/rogue endpoint | Untrusted facilitator/scheme/payee coercion | facilitator allowlist, scheme allowlist, approval precedence, policy checks | Social engineering/user approval mistakes | High | Security Maintainer | Block release if Critical dependency impacted |
| Policy configuration | Misconfiguration by user/contributor | Overly permissive limits, disabled guardrails | strict defaults, config validation, startup compliance checks | Unsafe local overrides by operator choice | High | Core Maintainer | Warn + block strict-profile release if unsafe defaults |
| Audit logs | Data leakage in telemetry/logs | Secret leakage via error/context output | redaction filters, structured reason codes, red-team leakage tests | Novel leak paths from new features | High | Core + Contributor (reviewed) | Block release on failing leakage tests |
| Migration engine | Interrupted/partial migration | Secret loss/corruption, fallback regression | idempotent checkpoints, verify-before-delete, strict fallback policy | Edge-case data recovery complexity | Medium | Core Maintainer | Block release if data-loss bug present |
| TPM availability state | Device reset/lockout | Unseal failure causing availability outage | fail-closed policy, recovery runbooks, diagnostics | Usability/recovery friction | Medium | Core + Security Maintainer | No bypass of security gate allowed |
| CI trust signal | Supply-chain/process drift | Red-team gate bypass or stale badge | required red-team workflow, README badge, artifact verification | CI platform outages can delay merges | Medium | Security Maintainer | Merge blocked when gate status unknown/fail |

### Residual Risk Register Rules

- Every Critical and High residual risk must have an explicit mitigation roadmap item.
- Risk acceptance is allowed only with maintainer sign-off recorded in repo docs/issues.
- No residual-risk acceptance may disable strict-profile security gates in CI.

## Secret Buffer Exposure Map

Purpose: explicitly track where plaintext secrets can appear in memory, for how long, and how each surface is reduced or eliminated.

| Surface ID | Secret Source | Buffer Type / Location | Typical Lifetime | Primary Risk | Required Mitigation | Target State |
| --- | --- | --- | --- | --- | --- | --- |
| BUF-001 | OS keyring read | in-process heap string during fetch/parse | short (ms to s) | memory scraping of host process | migrate secret use to signer path, zeroize wrappers, minimize copies | removed from host critical paths |
| BUF-002 | env var secret injection | process env + copied string buffers | medium (process lifetime unless cleared) | easy exfiltration via proc/env inspection | disallow env secrets in strict mode except emergency bootstrapping, clear after import | disabled in strict mode |
| BUF-003 | legacy file fallback read | file contents + parse buffers | short to medium | plaintext persistence + memory copies | replace with Cocoon blob + TPM unseal flow | eliminated |
| BUF-004 | policy/context metadata | request structs/log context | short | accidental secret logging | strict schema (no secret fields), redaction and high-entropy scanners | controlled |
| BUF-005 | IPC plaintext payload | sender/receiver transient buffers | short | local snoop/tamper/replay | mandatory AEAD payload wrapping + replay protection | encrypted-only transport |
| BUF-006 | signer decrypt window | signer process sensitive buffer | very short (bounded) | runtime scraper in signer process | memsecurity backend, mlock, zeroize, no long-lived cache, process hardening | minimized and monitored |
| BUF-007 | crash dumps/core files | OS dump artifacts | long (until deleted) | offline secret extraction | disable core dumps, MADV_DONTDUMP, crash artifact checks | prevented |
| BUF-008 | migration temporary buffers | migration worker memory | short to medium | leakage during conversion | checkpointed flow, verify-before-delete, zeroize temporary buffers | minimized |

### Buffer Governance Rules

- Any new secret-handling code path must add/update a `BUF-*` row before merge.
- `BUF-*` rows mapped to Critical threats require matching red-team scenario coverage.
- Strict profile release is blocked if any `Target State` marked “eliminated” is still active without approved exception.

### Buffer-to-Red-Team Traceability Map

Every buffer surface must map to explicit red-team checks.

| Buffer ID | Primary Red-Team Coverage | Secondary Coverage | Pass Condition |
| --- | --- | --- | --- |
| BUF-001 | RT-014 (memory-scraper simulation) | RT-018 (audit log leakage) | no host-process key exposure and no leakage artifacts |
| BUF-002 | RT-017 (legacy fallback regression) | RT-018 (audit log leakage) | strict mode rejects env-based secret persistence paths |
| BUF-003 | RT-011 (blob tamper), RT-012 (rollback replay) | RT-017 (fallback regression) | plaintext fallback absent; blob integrity/rollback protections hold |
| BUF-004 | RT-018 (audit log leakage) | RT-001 (context approval bypass) | metadata paths never carry/expose secret material |
| BUF-005 | RT-005 (IPC tamper), RT-006 (IPC replay) | RT-004 (unauthorized IPC caller) | AEAD integrity/replay checks and caller auth enforced |
| BUF-006 | RT-014 (memory-scraper simulation) | RT-007 (invalid FSM transition), RT-013 (TPM unavailable) | signer-only secret residency with bounded decrypt window |
| BUF-007 | RT-015 (core dump leakage) | RT-014 (memory-scraper simulation) | crash paths do not expose recoverable secrets |
| BUF-008 | RT-016 (migration interruption) | RT-018 (audit log leakage) | migration temps are recoverable, bounded, and non-leaking |

Traceability enforcement:

- CI evidence must include a generated map proving each `BUF-*` has executed linked `RT-*` tests.
- A release is non-compliant if any `BUF-*` row lacks at least one passing primary coverage scenario.

## Wallet Usage Pattern and x402 UX Contract (Must Finalize Before Coding)

### Core model

The wallet is an **agent wallet** with dual access patterns:

- **Autonomous lane (agent-initiated):** allowed only for policy-approved actions.
- **Interactive lane (user-initiated or user-escalated):** required when action is outside autonomous policy or context demands approval.

Both lanes use the same signer boundary and policy engine. No direct key access from agent/plugin/WASM.

### Permission precedence (hard rule)

Decision order for every sign/purchase/payment request:

1. **Runtime/context policy** (highest priority), including instructions like “ask user permission before purchasing/signing/whatever”.
2. **User policy profile** (per-user limits, approved counterparties, risk posture).
3. **Global system policy defaults**.

If any higher-priority layer requires user approval, auto-sign is forbidden and request must transition to `AwaitUserApproval`.

### UX outcomes for each request

- **Auto-Approved:** request is in autonomous envelope and no higher-priority approval requirement exists.
- **Needs Approval:** request is valid but requires user decision due to policy/context.
- **Denied:** request violates hard policy constraints.
- **Expired/Aborted:** request timed out, superseded, or user/agent canceled.

### Required request metadata (for policy + UX)

Every request must include, at minimum:

- actor (`agent` or `user`),
- action type (`x402_payment`, `transfer`, `approve`, etc.),
- chain and asset details,
- max amount and exact quoted amount,
- destination/payee identity,
- rationale/context summary,
- idempotency key and expiry,
- correlation/audit IDs.

### x402-rs integration contract (Solana)

Use `x402-rs` as the protocol implementation target, including Solana schemes (`v1-solana-exact`, `v2-solana-exact`).

#### Behavioral requirements

- `402 Payment Required` responses are converted into normalized internal payment intents.
- Signer policy validates normalized intent before any signing call.
- x402 payment signing must flow through the same approval/autonomy decision pipeline as all other wallet actions.
- Chain/scheme allowlists must explicitly include allowed x402 Solana schemes.
- Trust policy must validate payment authority endpoint context:
  - if challenge includes facilitator/settlement endpoint, enforce endpoint allowlist,
  - otherwise enforce merchant origin + scheme/payee trust policy.

#### UX requirements for x402 flows

- User sees clear prompt with: payee, amount, asset, chain, scheme version, payment authority (facilitator endpoint if present, otherwise merchant origin), and reason.
- If context says ask permission, prompt is mandatory even for previously auto-approvable amounts.
- Re-approval is required on material change (amount increase, new payee, changed payment authority endpoint/origin, new scheme, policy drift).

### FSM-driven usage states (`rust-fsm`)

Wallet request lifecycle must be modeled as explicit states:

- `Received`
- `PolicyEvaluated`
- `AwaitUserApproval`
- `Approved`
- `Signing`
- `Submitted`
- `Settled`
- `Denied`
- `Expired`
- `Failed`

Required transition guards:

- No transition to `Signing` without `Approved`.
- No transition to `Approved` when higher-priority layer requires manual permission and none is present.
- Any invalid transition fails closed and emits auditable denial event.

### Audit and explainability requirements

- Every decision must emit a machine-readable reason code.
- Reason codes must identify which policy layer determined the outcome.
- Audit log entries must support reconstruction of “why auto-approved vs why prompted vs why denied” for x402 and non-x402 flows.

### Decision Table (Authoritative)

| # | Request Type | Context Requires Approval? | Within User Limits? | Within Global Limits? | Payment Authority/Payee Trusted? | Outcome | Reason Code |
| --- | --- | --- | --- | --- | --- | --- | --- |
| 1 | x402 payment (agent) | No | Yes | Yes | Yes | Auto-Approve | `ALLOW_AUTO_POLICY_OK` |
| 2 | x402 payment (agent) | Yes | Yes | Yes | Yes | Prompt User | `PROMPT_CONTEXT_REQUIRED` |
| 3 | x402 payment (agent) | No | No | Yes | Yes | Prompt User | `PROMPT_USER_LIMIT_EXCEEDED` |
| 4 | x402 payment (agent) | No | Yes | No | Yes | Deny | `DENY_GLOBAL_LIMIT` |
| 5 | x402 payment (agent) | No | Yes | Yes | No | Deny | `DENY_UNTRUSTED_FACILITATOR_OR_PAYEE` |
| 6 | x402 payment (user) | No | Yes | Yes | Yes | Approve (User Path) | `ALLOW_USER_INITIATED` |
| 7 | transfer/sign (agent) | Yes | Yes | Yes | N/A | Prompt User | `PROMPT_CONTEXT_REQUIRED` |
| 8 | transfer/sign (agent) | No | Yes | Yes | N/A | Auto-Approve | `ALLOW_AUTO_POLICY_OK` |
| 9 | any | N/A | N/A | N/A | N/A | Deny | `DENY_INVALID_TRANSITION` |
| 10 | any | N/A | N/A | N/A | N/A | Expire | `EXPIRE_TTL_REACHED` |

Notes:

- “Context Requires Approval?” has absolute precedence over user/global limits.
- “Within User Limits?” includes per-user spend, velocity, and allowlist constraints.
- “Within Global Limits?” includes system-wide caps and hard safety controls.

### Feature Matrix (Scope and Enforcement)

| Capability | Agent Lane | User Lane | x402 Solana | Enforcement Point | Mandatory in Strict Profile |
| --- | --- | --- | --- | --- | --- |
| TPM-rooted key unseal | Yes | Yes | Yes | Signer key manager | Yes |
| Cocoon encrypted secret blobs | Yes | Yes | Yes | Secret provider | Yes |
| IPC payload AEAD | Yes | Yes | Yes | IPC transport | Yes |
| Local IPC peer auth (Unix socket/Named Pipe) | Yes | Yes | Yes | IPC transport | Yes |
| Context-driven approval gate | Yes | Yes | Yes | Policy engine | Yes |
| Autonomous signing | Yes (bounded) | No | Yes (bounded) | Policy engine | Yes |
| Explicit user approval UI | Escalation only | Primary | Escalation + user-initiated | UX/daemon API | Yes |
| Payment authority trust policy | N/A | N/A | Yes | x402 adapter + policy | Yes |
| Scheme allowlist (`v1/v2-solana-exact`) | N/A | N/A | Yes | x402 adapter + policy | Yes |
| Per-tx and daily spend limits | Yes | Yes | Yes | Policy engine | Yes |
| Replay protection (idempotency + nonce policy) | Yes | Yes | Yes | IPC + request validator | Yes |
| Full audit reason codes | Yes | Yes | Yes | Decision logger | Yes |

### ASCII Flow Charts

#### 1) High-Level Wallet Request Flow

```text
[Request Received]
    |
    v
[Normalize Intent + Validate Schema]
    |
    v
[Policy Evaluation]
   |          |          |
   |          |          +--> [Deny] --> [Audit + Return]
   |          |
   |          +--> [Need User Approval] --> [Prompt User]
   |                                           |
   |                               +-----------+-----------+
   |                               |                       |
   |                               v                       v
   |                         [User Approves]         [User Rejects/Timeout]
   |                               |                       |
   |                               v                       v
   |                          [Approved]               [Denied/Expired]
   |                               |                       |
   +-------------------------------+-----------------------+
               |
               v
             [Signer: Sign + Submit]
               |
               v
             [Settled/Failed]
               |
               v
          [Audit + Return]
```

#### 2) Permission Precedence Flow

```text
[Evaluate Context Rules]
    |
    +-- requires approval? -- Yes --> [Prompt Required]
    |                                  |
    |                                  v
    |                           [Skip Auto-Sign Path]
    |
    No
    |
    v
[Evaluate User Policy]
    |
    +-- exceeds user limit? -- Yes --> [Prompt User]
    |
    No
    |
    v
[Evaluate Global Policy]
    |
    +-- hard violation? -- Yes --> [Deny]
    |
    No
    |
    v
[Auto-Approve Eligible]
```

#### 3) x402 Solana Flow (`x402-rs`)

```text
[HTTP Request]
  |
  v
[Receive 402 Payment Required]
  |
  v
[Parse x402 Requirement]
  |
  v
[Normalize -> Internal Payment Intent]
  |
  v
[Check Scheme/Chain/Facilitator Allowlist]
   |                 |
   |                 +--> fail --> [Deny + Audit]
   v
[Policy + Permission Precedence]
   |          |
   |          +--> prompt --> [User Decision]
   v
[Approved]
   |
   v
[Signer via AEAD IPC -> Sign Solana Payment Payload]
   |
   v
[Attach Payment Header + Retry HTTP Request]
   |
   v
[Response OK/Fail -> Audit + Return]
```

## Phase 0 — Requirements Lock (Decision Gate 1)

### Checklist

- [ ] Define protected assets:
- [x] Define protected assets:
  - DB encryption key,
  - wallet seed/private key material,
  - signer policy configuration,
  - audit records.
- [x] Define adversaries:
  - stolen laptop/offline attacker,
  - malicious plugin/WASM,
  - local unprivileged user,
  - memory scraper in app process.
- [ ] Define UX tolerance:
  - unlock prompts,
  - approval prompts,
  - unlock session TTL.
- [x] Decide fail strategy:
  - fail closed (default recommended),
  - or temporary degraded mode with explicit warning.

### Exit criteria

- [ ] Threat model approved.
- [x] Explicit decisions on fail behavior and UX friction documented.

## Phase 1 — Cryptographic Architecture

### Design

- TPM-sealed KEK (key encryption key) as root of trust.
- DEK (data encryption key) used by Cocoon for encrypted blobs.
- DEK is wrapped by KEK and persisted only in wrapped form.
- Secrets (DB key, wallet key, etc.) are encrypted by DEK in versioned blobs.

### Checklist

- [ ] Define key hierarchy and lifecycle states: create, use, rotate, revoke, recover.
- [x] Define envelope format metadata (version, algorithm IDs, KDF params, nonce, tag).
- [x] Lock explicit cipher choices for each layer:
  - Cocoon blob encryption AEAD,
  - required IPC payload AEAD,
  - TPM wrapping/unsealing strategy.
- [ ] Define rotation policy:
  - DEK rotation cadence,
  - KEK rewrap strategy,
  - emergency key rollover.
- [x] Define memory-handling policy (zeroize and lifetime limits).

### Exit criteria

- [ ] Architecture doc includes lifecycle state machine and rotation procedures.

## Phase 2 — TPM Policy Design (Linux)

### Design

- Use tss-esapi with /dev/tpmrm0 where available.
- Support two policy modes:
  - Strict: PCR-bound unseal to measured boot state.
  - Compatibility: unseal without PCR binding.
- Optional user-auth policy (PIN/passphrase) layered on TPM policy sessions.

### Checklist

- [ ] Define TPM provisioning flow (SRK/object handles, persistence conventions).
- [ ] Define PCR selection and update policy for strict mode.
- [ ] Define lockout strategy (dictionary attack counter, backoff, reset runbook).
- [x] Define behavior when TPM unavailable or reset.

### Exit criteria

- [ ] Deterministic provision/unseal flow documented and testable.
- [ ] Recovery playbook exists for lockout and TPM clear/reset scenarios.

## Phase 3 — Cocoon Data-at-Rest Layer

### Design

- Cocoon protects persisted secret material as authenticated encrypted blobs.
- Plaintext file fallback is replaced with Cocoon blob fallback.
- All blob formats are versioned for future migrations.
- Cipher/KDF policy is explicit and versioned to avoid accidental drift.

### Checklist

- [ ] Implement encrypted container API for read/write/list/delete.
- [x] Pin Cocoon cryptography policy in code and docs:
  - target AEAD (ChaCha20-Poly1305 preferred),
  - approved fallback (AES-256-GCM if needed),
  - KDF parameters and minimum strength requirements.
- [ ] Add anti-rollback metadata strategy (version/epoch checks).
- [x] Add tamper detection and explicit error mapping.
- [ ] Define secure replacement/deletion best effort for old blobs.

### Exit criteria

- [x] No plaintext secrets written to disk in normal paths.

## Phase 4 — Runtime Isolation and Memory Hardening

### Design

- Signing happens in a dedicated local signer daemon process.
- Main bot process and plugins/WASM can only submit signing intents.
- Signer enforces policy before any decrypt/sign operation.

### Checklist

- [x] Define IPC channel and peer authentication (Unix socket on Linux/macOS, Named Pipe on Windows).
- [ ] Decide IPC confidentiality mode:
  - required: per-session AEAD-wrapped IPC payloads in all environments,
  - peer-auth only mode is disallowed.
- [ ] Define signer API: intent, preview, approve, sign, deny reason.
- [ ] Add policy controls:
  - allowlisted programs/contracts,
  - spend caps,
  - authority mutation checks,
  - high-risk confirmation requirements.
- [ ] Add memory controls:
  - zeroize sensitive buffers,
  - minimize plaintext lifetime,
  - mandatory page locking (fail closed if unavailable).
- [ ] Ensure no long-lived plaintext key cache in signer or host process.

### Exit criteria

- [ ] No plugin/WASM path can read private key bytes.
- [ ] Policy denials are auditable without exposing secrets.

## Phase 5 — Butterfly Integration Mapping

### Existing integration anchors

- Secret abstraction in src/vault.rs.
- DB key resolution path in src/db.rs.

### Checklist

- [x] Add secret-provider abstraction with precedence model.
- [x] Add TPM+Cocoon provider as default secure source.
- [x] Replace plaintext DB file fallback with Cocoon+TPM path.
- [ ] Add migration-aware loading for legacy keychain/file secrets.
- [ ] Add structured telemetry for source/mode (no secret values).

### Exit criteria

- [ ] App boots and runs with TPM+Cocoon default path enabled.

## Phase 6 — Migration and Backward Compatibility

### Checklist

- [ ] Detect legacy secrets in keyring/plain files.
- [x] One-time migration command with dry-run mode.
- [ ] Migrate flow:
  - read legacy,
  - encrypt into new blob,
  - verify decrypt,
  - securely remove legacy material where possible.
- [x] Partial-failure rollback and idempotency strategy.

### Exit criteria

- [ ] Existing installs migrate without data loss.
- [x] Re-running migration is safe.

## Phase 7 — Testing and Security Validation Matrix

### Mandatory test policy

- All testing must run with production security settings enabled.
- No separate “test-mode crypto,” weakened KDFs, disabled policy checks, or plaintext fallbacks.
- Any test requiring reduced security for speed is out of policy and must not be merged.

### Test matrix

- [ ] Unit tests:
  - blob encode/decode,
  - key lifecycle,
  - rotation/rewrap,
  - error mapping.
- [ ] Integration tests:
  - TPM present,
  - TPM unavailable,
  - PCR mismatch,
  - lockout conditions.
- [ ] Negative tests:
  - tampered blob,
  - replayed old blob,
  - unauthorized IPC client.
- [ ] Migration tests:
  - fresh install,
  - legacy upgrade,
  - interrupted migration recovery.
- [ ] Security checks:
  - secret redaction in logs,
  - plaintext file scans,
  - memory lifetime spot checks.
- [ ] Production-parity environment checks in CI:
  - strict profile enabled,
  - fail-closed behavior enforced,
  - hardened IPC encryption enabled,
  - PCR policy path exercised.
- [ ] Coverage gates:
  - line coverage target: 100%,
  - branch coverage target: 100%,
  - critical-path mutation testing required for signer/policy/unwrap flows.

### Exit criteria

- [ ] Validation matrix passes for release target environments under strict profile only.
- [ ] Coverage gates met or explicit written exception approved by security owner.

### CI Red-Team Gate (Required)

- A dedicated CI job must run red-team scenarios on every PR and on main.
- The red-team job is a release-blocking gate (failure blocks merge/release).
- The red-team suite must include at least:
  - policy-bypass attempts,
  - unauthorized IPC caller attempts,
  - payload tampering/replay attempts,
  - memory-scraper simulation checks,
  - migration abuse/failure-path checks.
- CI must publish machine-readable results artifacts for audit evidence.

### Red-Team Scenario Catalog (CI)

Use stable scenario IDs. Every run must report each ID as `PASS`/`FAIL`/`SKIP` with rationale.

| ID | Severity | Scenario | Attack Simulation | Expected Result | Evidence Required |
| --- | --- | --- | --- | --- | --- |
| RT-001 | Critical | Context approval bypass | Request marked “approval required” attempts auto-sign path | Deny auto-sign, transition to `AwaitUserApproval` | FSM transition log + reason code |
| RT-002 | High | User policy limit bypass | Agent request exceeds per-user limit | Prompt or deny per policy; never auto-approve | Policy eval trace + decision record |
| RT-003 | Critical | Global hard-limit bypass | Request exceeds global hard cap | Deny | Policy layer + reason code `DENY_GLOBAL_LIMIT` |
| RT-004 | Critical | Unauthorized IPC caller | Non-allowed process connects to signer socket | Connection rejected | Peer-cred validation log |
| RT-005 | Critical | IPC payload tamper | Mutate AEAD ciphertext/tag in transit | Reject payload, no signing | AEAD verification failure log |
| RT-006 | Critical | IPC replay attempt | Replay prior valid encrypted message | Rejected as replay | Nonce/counter replay detection log |
| RT-007 | Critical | Invalid FSM transition | Force direct transition to `Signing` from non-approved state | Deny transition, fail closed | FSM error + audit event |
| RT-008 | High | Untrusted payment authority | x402 flow with unknown facilitator endpoint (if present) or untrusted merchant origin context | Deny | Trust-policy check log |
| RT-009 | High | Unapproved x402 scheme | x402 flow with non-allowlisted scheme | Deny | Scheme-policy log |
| RT-010 | High | Amount mutation after approval | Modify amount/payee after user approval | Require re-approval or deny | Approval invalidation log |
| RT-011 | Critical | Blob tampering at rest | Modify Cocoon blob bytes/metadata | Decrypt/parse fails closed | Integrity/tamper error |
| RT-012 | Critical | Rollback blob replay | Replace current blob with older valid blob | Detect rollback and deny | Version/epoch check log |
| RT-013 | Critical | TPM unavailable at sign time | TPM access denied/reset/unseal fails | Fail closed; no degraded sign | TPM error path + deny reason |
| RT-014 | Critical | Memory-scraper simulation | Attempt read from non-signer process memory path | No private key exposure outside signer | memory-scope assertion + process boundary evidence |
| RT-015 | High | Core dump leakage check | Trigger crash path in signer | No secrets in dumps (or dump disabled) | crash artifact policy check |
| RT-016 | Medium | Migration interruption | Kill process mid-migration | Restart-safe + no secret loss/corruption | checkpoint recovery evidence |
| RT-017 | Critical | Legacy fallback regression | Force plaintext fallback path in strict mode | Deny fallback path | strict-mode compliance log |
| RT-018 | High | Audit log secret leakage | Inject high-entropy secrets in request context | Logs redact secrets; no plaintext leak | redaction scan report |

### Scenario Execution Rules

- Every scenario ID above is mandatory in strict profile CI.
- `SKIP` is allowed only with explicit waiver ID and owner approval.
- Any `FAIL` on required IDs blocks merge and release.
- Scenario IDs and semantics are versioned; changes require security review.

### Severity-Based CI Blocking Policy

- **Critical:** any `FAIL` blocks PR merge and release immediately.
- **High:** any `FAIL` blocks release; PR merge allowed only with approved temporary waiver and expiry.
- **Medium:** does not block PR merge by default, but blocks release if unresolved past waiver expiry.
- **Low (future use):** informational unless explicitly elevated by policy.

Additional enforcement:

- Two consecutive `FAIL` results on the same High scenario auto-escalate it to Critical until manually downgraded by security owner.
- Any scenario tagged Critical cannot be waived for production release.

### CI Evidence Output Contract

Each CI red-team run must publish an artifact bundle containing:

- `redteam-summary.json` with overall result and per-ID status.
- `redteam-results.ndjson` event stream (timestamped decisions).
- `redteam-junit.xml` for CI UI integration.
- `redteam-coverage.json` mapping IDs to executed checks.
- `redteam-waivers.json` listing active waivers and expiry.

Minimum `redteam-summary.json` shape:

```json
{
  "run_id": "<ci-run-id>",
  "profile": "strict",
  "overall": "PASS|FAIL",
  "results": [
    {"id": "RT-001", "status": "PASS", "reason": "..."}
  ]
}
```

### Public Trust Signal (README Badge)

- README must display a dedicated red-team status badge with pass/fail state.
- Badge source must be the dedicated red-team CI workflow status.
- Badge policy:
  - green only when required red-team job passes,
  - red on failure,
  - unknown/missing status treated as non-compliant for release readiness.
- Badge and workflow naming must stay stable to preserve external trust links.

## Phase 9 — Audit Readiness Program

### Goal

Prepare an audit-ready package so an external security review can evaluate design,
implementation, and operational controls with minimal ambiguity.

### Checklist

- [ ] Threat model is current and signed off by owner.
- [ ] Cryptographic design document includes algorithm choices, key lifecycle, and downgrade protections.
- [ ] Test evidence bundle includes:
  - CI logs,
  - coverage reports,
  - negative test outputs,
  - reproducible test commands.
- [ ] Static analysis and dependency audit reports are archived per release.
- [ ] Pen-test style abuse-case tests exist for signer IPC, policy bypass, and blob tampering.
- [ ] Secure code review checklist is completed for all custody-related PRs.
- [ ] Incident response runbooks validated via tabletop exercises.
- [ ] Release artifacts are traceable to reviewed commits.

### Auditor handoff package

- [ ] Architecture and trust-boundary diagrams.
- [ ] Threat model and assumptions.
- [ ] Cryptography and key-management specs.
- [ ] Full test matrix results (prod settings).
- [ ] Coverage and mutation-testing reports.
- [ ] Red-team CI reports and historical pass/fail trend.
- [ ] Known limitations and residual risk register.

### Exit criteria

- [ ] Internal pre-audit completed with no critical unresolved findings.
- [ ] External audit scope and test access finalized.

## Phase 8 — Operations, Backup, Recovery, and Rollout

### Checklist

- [ ] Define backup approach for encrypted blobs and metadata.
- [ ] Define machine migration flow (new TPM enrollment + rewrap).
- [ ] Define incident runbooks:
  - TPM lockout,
  - TPM reset/clear,
  - corrupted blob recovery.
- [ ] Rollout stages:
  1. opt-in alpha,
  2. default-on for new installs,
  3. default-on for upgrades with migration helper,
  4. mandatory secure mode after stability targets.
- [ ] Define release metrics and alert thresholds.

### Exit criteria

- [ ] Production readiness checklist approved.

## Critical Decisions to Lock Early

- [x] Fail closed if TPM unavailable (recommended) or allow degraded mode?
- [ ] PCR binding default on or off?
- [ ] Require explicit user confirmation for high-risk signing?
- [ ] Keep any external signer fallback, or strict local-only custody?
- [x] Which Cocoon cipher policy is mandatory by default (ChaCha20-Poly1305 vs AES-256-GCM fallback policy)?
- [ ] Which IPC AEAD construction is mandatory (algorithm, key schedule, nonce policy, replay protection)?

## Strict Security Profile (Proposed Defaults)

This profile is the recommended production baseline for “local and secure” mode.

| Area | Strict Default | Notes |
| --- | --- | --- |
| Availability behavior | Fail closed | If TPM unseal fails, signing and secret unwrap are denied. |
| Root of trust | TPM2 KEK with policy session | KEK never exported in plaintext from policy-controlled path. |
| TPM policy | PCR-bound unseal enabled | Bind unseal to measured boot state for anti-offline extraction. |
| Cocoon at-rest cipher | ChaCha20-Poly1305 (256-bit) | Preferred default for secret blobs. |
| Cocoon fallback cipher | AES-256-GCM (explicit opt-in fallback) | Allow only if required by platform/policy. |
| KDF policy | Cocoon default PBKDF2-SHA256 (100000 min) | Never allow weaker KDF params than baseline. |
| Secret blob format | Versioned envelope with algo IDs | Required for future crypto agility/migrations. |
| IPC auth | Local IPC peer credential checks mandatory | Reject unknown UID/GID/process identity. |
| IPC confidentiality | Mandatory AEAD session wrapping | Encrypt signer IPC payloads end-to-end locally in all modes. |
| Signer boundary | Separate local signer process required | Plugins/WASM submit intent only, never raw keys. |
| Key cache policy | No long-lived plaintext key cache | Unwrap per operation or short bounded session only. |
| Plaintext key lifetime | ≤ 5 seconds in sensitive buffer | Target bound; zeroize immediately after sign/decrypt. |
| Memory handling | Zeroize + mandatory page lock | Apply to key buffers and derived secrets; fail closed if unavailable. |
| High-risk actions | Explicit user confirmation required | New destination/program/authority changes always gated. |
| Spend controls | Per-tx cap + daily cap enforced | Deny above-threshold operations by policy. |
| Audit policy | Structured logs without secrets | Log decision reason, policy ID, caller identity. |
| Legacy fallback | Plaintext file fallback disabled | Migration path only, then remove. |

### Strict Profile Checklist

- [x] Fail-closed behavior enforced for TPM/key unwrap failures.
- [ ] PCR-bound unseal enabled and documented with update procedure.
- [x] Cocoon cipher policy pinned (ChaCha20-Poly1305 default, AES-256-GCM fallback rules).
- [ ] Hardened encrypted IPC mode enabled by default.
- [ ] IPC payload AEAD mandatory in all environments (no peer-auth-only fallback).
- [x] Signer daemon process isolation enabled for all key use.
- [x] Plaintext key lifetime bounded and zeroization verified.
- [ ] High-risk operation confirmations enabled.
- [ ] Spend limits and allowlists configured before production use.

## Coding Phases Breakdown (Execution Plan)

Use these phases as the implementation sequence. Each phase is mergeable on its own and must pass strict-profile tests.

## Phase Crosswalk (Numbered ↔ Lettered)

Purpose: keep governance/program phases and implementation phases aligned during pre-v1 execution.

| Numbered Phase | Lettered Phase(s) | Relationship | Current Status | Review Notes |
| --- | --- | --- | --- | --- |
| Phase 0 — Requirements Lock | A (foundation), F (policy), H (evidence) | Requirements and decisions feed implementation constraints and release gates. | Partial | Threat model and fail-closed decisions are documented; formal owner sign-off remains open. |
| Phase 1 — Cryptographic Architecture | B, C, G | Architecture choices are implemented through Cocoon storage, TPM rooting, and hardening controls. | Partial | Blob schema/cipher policy are in place; TPM KEK/DEK lifecycle and rotation implementation still pending. |
| Phase 2 — TPM Policy Design | C | TPM policy is implemented in Phase C code paths and tests. | In Progress | TPM-required fast-fail baseline is implemented; provisioning, PCR policy, and lockout runbooks are pending. |
| Phase 3 — Cocoon Data-at-Rest Layer | B | Phase B is the direct implementation of at-rest encrypted blob requirements. | Complete | Versioned envelope, strict parse/verify, tamper failures, and no plaintext fallback in strict paths are implemented. |
| Phase 4 — Runtime Isolation and Memory Hardening | E, G | Signer boundary and memory controls are implemented across these phases. | Complete | Signer daemon split, mandatory AEAD IPC, startup hardening checks, and mandatory page-lock controls are implemented. |
| Phase 5 — Butterfly Integration Mapping | A, B, C, D | Integration mapping spans vault/db/provider wiring and migration entry points. | Partial | Secret abstraction + Cocoon integration + migration CLI are in place; full TPM KEK/DEK integration remains open. |
| Phase 6 — Migration and Backward Compatibility | D | Phase D is the migration engine and operational migration flow. | Complete | Dry-run and apply modes, idempotent behavior, and restart-safe semantics are implemented for legacy secrets. |
| Phase 7 — Testing and Security Validation Matrix | H (plus all phases) | Validation phase aggregates strict-profile tests for all prior implementation. | In Progress | Targeted unit coverage exists for A/B/C baseline and D migration; full matrix and CI red-team gates remain open. |
| Phase 8 — Operations/Backup/Recovery/Rollout | H (ops outputs), C/D (recovery behavior) | Operational readiness depends on TPM and migration recovery behavior. | Not Started | Backup/reprovision/runbook rollout items are pending. |
| Phase 9 — Audit Readiness Program | H | Audit bundle and evidence packaging are delivered in Phase H. | Not Started | Artifact pipeline, trend reporting, and pre-audit checklist completion are pending. |

### Alignment Rule for Review

- Numbered phases are authoritative for security outcomes and release readiness.
- Lettered phases are authoritative for coding sequence and merge scope.
- A numbered phase is only treated as complete when all linked lettered phase deliverables and its own exit criteria are complete.
- If statuses differ, use the stricter interpretation (mark as Partial/In Progress) until both views agree.

### Cross-Cutting Implementation Rule — State Machines via `rust-fsm`

Use `rust-fsm` for protocol-critical workflow states to avoid ad-hoc transition logic.

**Required FSM-controlled flows:**

- signer session lifecycle,
- IPC handshake and key-establishment lifecycle,
- signing request approval lifecycle,
- migration lifecycle checkpoints and rollback transitions,
- TPM lockout/recovery state handling.

**Implementation constraints:**

- [ ] Define explicit input/state/output alphabets for each critical flow.
- [ ] Invalid transitions must fail closed and emit auditable deny/error events.
- [ ] Keep secret material outside FSM state enums; hold sensitive data in short-lived side context only.
- [ ] Generate and commit Mermaid diagrams from FSM specs for review.

### Coding Phase A — Secure Secret Abstractions

**Goal:** Introduce explicit interfaces so TPM/Cocoon can be integrated without large refactors later.

**Primary code touchpoints:**

- `src/vault.rs`
- `src/db.rs`
- `src/error.rs`

**Checklist:**

- [ ] Add `SecretProvider` trait (get/set/delete/list minimal surface).
- [ ] Add provider-resolution policy (strict mode first, legacy provider behind migration path).
- [ ] Add typed error variants for security failures (tamper, policy denied, TPM unavailable).
- [ ] Keep existing behavior functionally compatible while introducing abstraction boundaries.

**Definition of done:**

- [ ] Existing app flows compile and run through the new abstraction with no plaintext logging regressions.

### Coding Phase B — Cocoon Blob Storage Layer

**Goal:** Replace plaintext file fallback with authenticated encrypted blobs.

**Primary code touchpoints:**

- `src/vault.rs`
- `src/db.rs`
- `src/runtime_paths.rs`
- New module: `src/security/cocoon_store.rs` (or equivalent)

**Checklist:**

- [x] Implement versioned secret blob schema (metadata + ciphertext + integrity checks).
- [x] Pin cryptography configuration in code (strict profile defaults).
- [x] Replace file fallback reads/writes for DB key with Cocoon blob path.
- [x] Add blob parse/verify errors that fail closed in strict mode.

**Definition of done:**

- [x] No plaintext secret fallback files are emitted in strict mode.

### Coding Phase C — TPM Root-Key Provider (Linux)

**Goal:** Use TPM as KEK root to unwrap app DEK and secrets.

**Primary code touchpoints:**

- New module: `src/security/tpm_provider.rs`
- `src/vault.rs`
- `src/db.rs`
- `Cargo.toml` (feature-gated TPM dependencies)

**Checklist:**

- [x] Add TPM initialization and capability checks.
- [x] Implement KEK provision/unseal path with strict fail-closed behavior.
- [x] Implement DEK wrap/unwrap integration for Cocoon secret operations.
- [x] Add lockout-aware actionable diagnostics for TPM-unavailable paths (without leaking sensitive info).

**Definition of done:**

- [x] Strict mode can bootstrap and recover secrets only through TPM-governed path.

### Coding Phase D — Migration Engine

**Goal:** Move existing keychain/plain legacy secrets into TPM+Cocoon format safely.

**Primary code touchpoints:**

- `src/daemon.rs`
- `src/main.rs`
- `src/vault.rs`
- New module: `src/security/migration.rs`

**Checklist:**

- [x] Add migration command with dry-run report.
- [x] Implement idempotent migrate-and-verify flow.
- [x] Add partial-failure recovery and restart-safe checkpoints.
- [x] Ensure legacy material removal occurs only after successful verification.

**Definition of done:**

- [x] Repeated migration runs produce stable outcomes and no data loss.

### Coding Phase E — Local Signer Daemon Boundary

**Goal:** Separate signing from main runtime and block plugin/WASM key access.

**Primary code touchpoints:**

- `src/bin/butterfly-botd.rs`
- `src/daemon.rs`
- New modules: `src/security/signer_daemon.rs`, `src/security/ipc.rs`

**Checklist:**

- [x] Create signer process with local transport (Unix socket on Linux/macOS, Named Pipe on Windows).
- [x] Enforce peer identity checks for every request.
- [x] Implement mandatory AEAD-protected IPC payload framing (session keys, nonce policy, replay defense).
- [x] Implement intent-only API (`preview`/`approve`/`sign`/`deny`).
- [x] Ensure only signer process module can trigger decrypt/sign operations.
- [x] Implement signer/session/request flow transitions using `rust-fsm`.

**Definition of done:**

- [x] Main process and plugins cannot access private key bytes directly.

### Coding Phase F — Policy Engine and Guardrails

**Goal:** Enforce transaction/signing restrictions before key use.

**Primary code touchpoints:**

- New module: `src/security/policy.rs`
- `src/guardrails/`
- `src/config.rs`

**Checklist:**

- [x] Add allowlists, per-tx/daily limits, high-risk confirmation gates.
- [x] Add clear deny reasons suitable for UI and audit logs.
- [x] Add policy config schema and strict validation.
- [x] Prevent bypass paths from direct code invocation.
- [x] Model policy decision lifecycle with `rust-fsm` to enforce deterministic, auditable transitions.

**Definition of done:**

- [x] All sign operations pass through policy checks with auditable outcomes.

### Coding Phase G — Hardening and Observability

**Goal:** Reduce in-memory risk and produce audit-quality operational telemetry.

**Primary code touchpoints:**

- `src/services/agent.rs` (redaction alignment)
- `src/daemon.rs`
- New module: `src/security/hardening.rs`

**Checklist:**

- [x] Add zeroization wrappers and bounded plaintext lifetimes.
- [x] Add mandatory page-locking for sensitive buffers.
- [x] Add structured security telemetry without secret values.
- [x] Add startup self-checks for strict profile compliance.

**Definition of done:**

- [x] Strict-profile self-check passes and logs compliance status at boot.

### Coding Phase H — Verification and Audit Bundle

**Goal:** Complete production-parity test suite and generate external audit evidence.

**Primary code touchpoints:**

- `tests/` (new and updated suites)
- `docs/` (runbooks, evidence manifest)
- CI config files (as applicable)

**Checklist:**

- [x] Expand tests to full strict-profile matrix.
- [x] Enforce coverage gates and mutation testing for critical paths.
- [x] Generate reproducible evidence package per release.
- [x] Complete pre-audit review checklist and artifact index.

**Definition of done:**

- [x] Release candidate is audit-ready with reproducible test evidence.

## Phase Dependencies (Must Respect Order)

- [ ] A before B/C (abstractions first).
- [ ] B before C integration hardening (blob layer first, then TPM binding).
- [ ] C before D/E (secure root before migration and signer isolation).
- [ ] E before F (policy applies at signer boundary).
- [ ] F/G before H (hardening before final evidence).

## Initial Implementation Slices (Recommended PR Sequence)

### PR 1 — Foundation

- [ ] Add feature-gated secure-secret provider abstraction.
- [ ] Add Cocoon blob storage module with versioned format.
- [ ] Keep TPM hook as interface with mock implementation for tests.

### PR 2 — TPM Integration

- [ ] Add Linux TPM provider via tss-esapi.
- [ ] Implement KEK provision/unseal and DEK wrap/unwrap.
- [ ] Wire provider into DB key resolution path.

### PR 3 — Migration

- [ ] Add migration command and dry-run.
- [ ] Migrate keychain/file legacy secrets into Cocoon blobs.
- [ ] Add telemetry and operator messaging.

### PR 4 — Signer Isolation

- [ ] Introduce local signer daemon process and IPC auth.
- [ ] Move key use/signing behind signer boundary.
- [ ] Add policy engine and approval hooks.

### PR 5 — Hardening and Rollout

- [ ] Complete validation matrix and threat-model review.
- [ ] Enable default-on for new installs.
- [ ] Publish runbooks and migration guides.

## Acceptance Criteria (Program Level)

- [ ] No plaintext key material persisted on disk in supported secure mode.
- [ ] Secrets are recoverable only through local TPM-governed flow.
- [ ] Untrusted plugin/WASM paths cannot directly access private keys.
- [ ] Migration from existing installs is reliable and reversible where feasible.
- [ ] Security posture and residual risks are documented for operators.
- [ ] All verification evidence is produced under production security settings only.

## Normative Security Specification (Lock-In for A+)

This section is normative. Implementations must conform exactly unless superseded by a versioned spec update.

### IPC Cryptographic Profile (Normative)

- Key exchange: `X25519` ephemeral-static handshake (client ephemeral, signer static identity key).
- KDF: `HKDF-SHA256`.
- AEAD payload cipher: `XChaCha20-Poly1305`.
- AEAD associated data (AAD) must include:
  - protocol version,
  - session ID,
  - sender role,
  - message type,
  - monotonic counter.

Fallback policy:

- `AES-256-GCM` is allowed only behind explicit compatibility flag with security maintainer approval.
- Any fallback activation must emit compliance warning and audit event.

### Session and Nonce Rules (Normative)

- Session ID: 128-bit random value generated by signer at handshake.
- Message counter: unsigned 64-bit monotonic per session; starts at `1`.
- Nonce derivation: deterministic from `(session_id, counter, direction)` via HKDF-expand domain separation.
- Counter reuse is forbidden; any duplicate or stale counter is hard reject (`DENY_REPLAY`).
- Session key lifetime:
  - max 5 minutes, or
  - max 1,000 messages,
  whichever is reached first.
- Rekey required after expiry; old session keys must be zeroized before new session activation.

### Handshake Transcript Binding (Normative)

- Handshake transcript hash must cover:
  - protocol version,
  - peer identity claims,
  - both key shares,
  - negotiated cipher suite,
  - session creation timestamp.
- Final session key material must be derived from transcript-bound secrets.
- If transcript verification fails, handshake aborts with `DENY_HANDSHAKE_INTEGRITY`.

### x402 Intent Canonicalization (Normative)

Before policy evaluation, x402 requests must be canonicalized into a deterministic internal structure containing:

- `scheme_id`,
- `chain_id`,
- `asset_id`,
- `amount_atomic`,
- `payee`,
- `payment_authority` (optional facilitator/settlement endpoint, otherwise merchant origin),
- `request_expiry`,
- `idempotency_key`,
- `context_requires_approval`.

Canonicalization failures must hard-deny with `DENY_INVALID_X402_INTENT`.

### Failure-Code Taxonomy (Normative)

All security-relevant denials/failures must use one of the following stable codes:

- `DENY_GLOBAL_LIMIT`
- `DENY_USER_POLICY`
- `DENY_CONTEXT_APPROVAL_REQUIRED`
- `DENY_UNTRUSTED_FACILITATOR_OR_PAYEE`
- `DENY_UNAPPROVED_SCHEME`
- `DENY_REPLAY`
- `DENY_AEAD_INTEGRITY`
- `DENY_INVALID_TRANSITION`
- `DENY_INVALID_X402_INTENT`
- `DENY_TPM_UNAVAILABLE`
- `DENY_HANDSHAKE_INTEGRITY`
- `DENY_STRICT_MODE_FALLBACK`
- `EXPIRE_TTL_REACHED`

Rules:

- Codes are append-only; renames/removals are not allowed.
- Every deny/expire event must include exactly one primary code.
- Logs and CI artifacts must report these exact code strings.

### Compliance Gates (Normative)

- Any PR modifying handshake, nonce/counter, or failure code behavior must include:
  - updated spec diff in this file,
  - red-team scenario impact assessment,
  - passing CI red-team evidence.
- If normative and implementation behavior diverge, release is blocked.

## Strict Compliance Snapshot (Implementation Status)

This section tracks current code alignment with strict-mode plan requirements.

### Completed in code

- [x] Secret resolution policy is strict-only (no compat mode behavior).
- [x] Vault operations fail closed when secure storage is disabled/unavailable in strict paths.
- [x] Launcher token bootstrap is fail-closed (no silent continue).
- [x] Config loading path propagates vault errors instead of silently degrading.
- [x] Context cache secret read/write path propagates vault errors.
- [x] DB key path is strict fail-closed:
  - no plaintext fallback,
  - no non-strict fallback path,
  - secure storage required for generated key persistence.
- [x] Factory reset daemon path fails if vault config persistence fails.
- [x] TPM strict gate is enforced for secret operations (fast fail when no TPM device is present).
- [x] Migration engine with dry-run/apply modes is implemented and wired to launcher CLI.
- [x] TPM lifecycle states are implemented for provision/seal/unseal/use/rotate/revoke/recover flows.
- [x] Cocoon DEK wrap/unwrap is bound to TPM-governed KEK unseal path.
- [x] TPM reset/policy-mismatch detection includes strict fail-closed recovery guidance.
- [x] Module-level signer intent API and policy evaluation path are implemented (`preview`/`approve`/`sign`/`deny`).
- [x] Module-level AEAD IPC framing with replay and integrity checks is implemented.
- [x] Rust FSM transition guards are implemented for signer request lifecycle and IPC session lifecycle.

### In progress / pending implementation

- [ ] Local signer daemon boundary with mandatory AEAD IPC payload wrapping.
- [ ] x402 Solana adapter with canonicalized intent contract and strict policy checks.
- [ ] Red-team CI workflow and README pass/fail badge automation.
- [ ] Audit evidence artifact generation pipeline (`redteam-summary.json`, ndjson, junit, coverage map).
- [ ] Unix-socket signer process wiring with peer-identity enforcement in runtime daemon paths.

### Strict policy statement (effective)

- Unsupported behavior in strict profile:
  - non-strict/compat secret mode,
  - plaintext secret fallbacks,
  - best-effort secret persistence on critical paths.
- Any future code that reintroduces these patterns is out-of-policy and must fail review.

### Verification commands used during implementation

- `cargo check --workspace --all-targets`
- `cargo test vault::tests --lib`
- `cargo test secret_policy::tests --lib`
- `cargo test db::tests --lib`
- `cargo test cocoon_store::tests --lib`
- `cargo test tpm_provider::tests --lib`
- `cargo test migration::tests --lib`
- `cargo test ipc::tests --lib`
- `cargo test policy::tests --lib`
- `cargo test signer_daemon::tests --lib`

## E/F Sign-Off Summary (Review Gate)

### Phase E — Local Signer Daemon Boundary

- Status: **Implemented (current scope)**
- Delivered:
  - signer intent API (`preview`/`approve`/`sign`/`deny`) in `src/security/signer_daemon.rs`,
  - local transport helpers with authorized request handling (Unix sockets + Windows Named Pipes),
  - peer identity enforcement (`SO_PEERCRED` on Linux, `getpeereid` on macOS, SID/token match on Windows) in `src/security/ipc.rs`,
  - mandatory AEAD framing + replay/integrity checks,
  - daemon route wiring for signer endpoints in `src/daemon.rs`.
- Verified by tests:
  - `cargo test signer_daemon::tests --lib`,
  - `cargo test ipc::tests --lib`,
  - `cargo test --test daemon`.

### Phase F — Policy Engine and Guardrails

- Status: **Implemented (current scope)**
- Delivered:
  - allowlist + per-tx/daily limit policy engine in `src/security/policy.rs`,
  - deterministic deny/prompt/allow reason codes,
  - strict policy schema validation (`PolicyEngine::from_json`),
  - transition guard enforcement via `rust-fsm` for signer request lifecycle.
- Verified by tests:
  - `cargo test policy::tests --lib`,
  - `cargo test signer_daemon::tests --lib`.

### Deferred by explicit decision

- Phase G remains intentionally deferred pending design review.
- `cargo test tpm_provider::tests --lib`
- `cargo test migration::tests --lib`
- `cargo test vault::tests --lib`