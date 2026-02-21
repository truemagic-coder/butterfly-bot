# Memory profiling playbook (UI + daemon)

This project now includes a practical profiling path inspired by the ChainSafe memory-analysis workflow.

## Quick start

Use the helper script:

- `./scripts/profile_ui_memory.sh smaps`
- `./scripts/profile_ui_memory.sh heaptrack`
- `./scripts/profile_ui_memory.sh massif`

Artifacts are written under:

- `artifacts/memory-profiles/<UTC timestamp>/`

## What each mode gives you

### `smaps` (lowest friction)
- Runs `butterfly-botd` + `butterfly-bot` with profiling symbols.
- Samples `/proc/<pid>/smaps_rollup` every 2s.
- Helps separate RSS vs mapped/cached behavior.

Useful files:
- `smaps-rollup-*.txt`
- `status-*.txt`
- `ui.log`
- `daemon.log`

### `heaptrack` (allocation callstacks)
- Requires `heaptrack` installed.
- Produces `heaptrack.ui.gz` with allocation/deallocation call paths.
- Higher overhead than `smaps`.

### `massif` (heap growth shape)
- Requires `valgrind` (`ms_print` optional for text report).
- Produces `massif.ui.out` and optionally `massif.ui.txt`.
- Can be slow.

## Build profile used for profiling

`Cargo.toml` now includes:

- `[profile.profiling]`
  - `inherits = "dev"`
  - `opt-level = 1`
  - `debug = true`

This keeps debug symbols while preserving some optimization.

## Recommended workflow for this crash pattern

1. Run `smaps` mode first and reproduce kill/hang.
2. Compare `VmRSS` vs `smaps_rollup` and cgroup counters.
3. If memory seems allocator-driven, run `heaptrack`.
4. If growth shape is unclear, run `massif`.
5. Correlate with UI/daemon logs to match events with allocation bursts.

## Notes

- Rust can still hit OOM or pressure kills through unbounded growth/caches.
- Memory-mapped files may not show clearly in allocator-only tools; `smaps` helps close that gap.
- For longer runs, keep artifact sets by timestamp and diff across sessions.
