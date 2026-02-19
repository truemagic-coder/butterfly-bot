# Security Pre-Audit Checklist (Phase H)

- [ ] Strict-profile startup self-check passes at daemon boot.
- [ ] TPM strict fail-closed behavior verified in current release candidate.
- [ ] Signer boundary and policy lifecycle tests pass (`tests/daemon.rs`).
- [ ] Hardening tests pass (`tests/security_hardening.rs`).
- [ ] Evidence bundle generated and archived.
- [ ] Evidence manifest reviewed and complete.
- [ ] Threat model and residual-risk sections reviewed for current release.
- [ ] Security plan checklist statuses match implementation reality.
