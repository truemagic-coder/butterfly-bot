#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

if ! command -v dx >/dev/null 2>&1; then
  echo "dioxus-cli (dx) is required but not found in PATH." >&2
  echo "Install with: cargo install dioxus-cli" >&2
  exit 1
fi

echo "==> Building WASM tool modules"
./scripts/build_wasm_tools.sh

echo "==> Bundling .deb with Dioxus"
dx bundle --desktop --release --package-types deb "$@"

echo "==> Looking for generated .deb"
DEB_FILE="$(find "$ROOT_DIR" -type f -name '*.deb' \
  \( -path '*/dist/*' -o -path '*/target/dx/*' -o -path '*/target/release/bundle/*' -o -path '*/target/*' \) \
  -printf '%T@ %p\n' 2>/dev/null | sort -nr | head -n1 | cut -d' ' -f2- || true)"

if [[ -z "$DEB_FILE" ]]; then
  echo "No .deb artifact found. Check dx output above for the exact path." >&2
  exit 1
fi

echo "Built: $DEB_FILE"
echo "Install with: sudo dpkg -i "$DEB_FILE""
