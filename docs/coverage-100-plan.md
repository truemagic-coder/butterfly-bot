# Coverage 100% Plan

Date: 2026-02-19

## Target

Increase automated coverage from ~55% to 100% while preserving signal quality.

## Rules

1. No coverage regressions on PRs.
2. Critical paths must reach 100% branch coverage first.
3. New features require tests in the same PR.

## Priority Order

1. `src/security/**`
2. `src/daemon.rs`
3. `src/services/**`
4. `src/providers/**`
5. `src/ui.rs` (logic paths, not visual-only concerns)

## Workstream

- Generate baseline per-file coverage report.
- Tag uncovered paths by type:
  - happy path missing
  - error path missing
  - retry/recovery missing
  - timeout/cancellation missing
  - policy-deny path missing
- Add table-driven tests for branch-heavy modules.
- Add integration tests for daemon lifecycle and provider switching.
- Add blackbox end-to-end scripts for Linux/macOS/Windows.

## CI Gates

- `cargo test --all`
- `cargo clippy --all-targets --all-features -- -D warnings`
- `cargo llvm-cov --workspace --tests --summary-only`
- Fail if coverage decreases.
- Fail if protected modules are <100% branch coverage.

## Cross-Platform Blackbox Matrix

Run daily for next few days:

- OS: Linux, macOS, Windows
- Provider: Ollama, OpenAI
- Security mode: strict, auto, compatible

Scenarios:

1. Fresh install + first message
2. Provider switch + restart
3. Daemon stop/start/reconnect from UI
4. Reminder/task execution while UI reconnects
5. Security doctor + security audit run
6. Recovery after forced daemon restart

## Exit Criteria

- 100% target reached on protected branches.
- Three consecutive daily blackbox matrix runs green.
- No open P0 reliability defects.
