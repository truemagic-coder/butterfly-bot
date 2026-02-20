# Security Evidence Manifest (Phase H)

This manifest defines the minimum reproducible artifacts required for audit-ready releases.

## Required artifacts

- `environment.txt` — toolchain, commit, and workspace metadata.
- `cargo-check.log` — compile status under production profile defaults.
- `test-daemon.log` — daemon integration results.
- `test-security-hardening.log` — strict-profile hardening test results.
- `mutation-check.log` — critical-path mutation testing gate output.
- `coverage-summary.log` — coverage gate summary for security-sensitive paths.
- `SHA256SUMS` — digest index for all evidence artifacts.

## Generation procedure

Run:

`./scripts/generate_security_evidence_bundle.sh`

The script writes bundles under `artifacts/security-evidence/<stamp>/`.

## Reproducibility requirements

- Set `SOURCE_DATE_EPOCH` in CI for deterministic bundle naming.
- Run in clean workspace state (`git status --porcelain` empty).
- Preserve generated bundle and `SHA256SUMS` as immutable release evidence.
