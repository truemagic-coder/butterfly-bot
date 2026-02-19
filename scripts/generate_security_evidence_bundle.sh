#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUTPUT_BASE="${ROOT_DIR}/artifacts/security-evidence"
STAMP="${SOURCE_DATE_EPOCH:-$(date -u +%Y%m%dT%H%M%SZ)}"
BUNDLE_DIR="${OUTPUT_BASE}/${STAMP}"

mkdir -p "${BUNDLE_DIR}"

{
  echo "stamp=${STAMP}"
  echo "workspace=${ROOT_DIR}"
  echo "git_commit=$(git -C "${ROOT_DIR}" rev-parse HEAD 2>/dev/null || echo unknown)"
  echo "git_status=$(git -C "${ROOT_DIR}" status --porcelain | wc -l | tr -d ' ')"
  cargo --version
  rustc --version
} > "${BUNDLE_DIR}/environment.txt"

(cd "${ROOT_DIR}" && cargo check) > "${BUNDLE_DIR}/cargo-check.log" 2>&1
(cd "${ROOT_DIR}" && cargo test --test daemon) > "${BUNDLE_DIR}/test-daemon.log" 2>&1
(cd "${ROOT_DIR}" && cargo test --test security_hardening) > "${BUNDLE_DIR}/test-security-hardening.log" 2>&1

if command -v cargo-mutants >/dev/null 2>&1; then
  (
    cd "${ROOT_DIR}" && cargo mutants --check --timeout 120 \
      --file 'src/security/**/*.rs' \
      --file 'src/daemon.rs' \
      --file 'src/vault.rs' \
      --file 'src/db.rs' \
      --file 'src/error.rs'
  ) > "${BUNDLE_DIR}/mutation-check.log" 2>&1
else
  {
    echo "cargo-mutants is required for critical-path mutation gate"
    echo "install with: cargo install cargo-mutants"
  } > "${BUNDLE_DIR}/mutation-check.log"
  exit 1
fi

if command -v cargo-llvm-cov >/dev/null 2>&1; then
  (cd "${ROOT_DIR}" && cargo llvm-cov --summary-only) > "${BUNDLE_DIR}/coverage-summary.log" 2>&1
else
  {
    echo "cargo-llvm-cov is required for critical-path coverage gate"
    echo "install with: cargo install cargo-llvm-cov"
  } > "${BUNDLE_DIR}/coverage-summary.log"
  exit 1
fi

sha256sum "${BUNDLE_DIR}"/* > "${BUNDLE_DIR}/SHA256SUMS"

echo "security evidence bundle generated at: ${BUNDLE_DIR}"
