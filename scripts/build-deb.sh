#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

if ! command -v dx >/dev/null 2>&1; then
  echo "dioxus-cli (dx) is required but not found in PATH." >&2
  echo "Install with: cargo install dioxus-cli" >&2
  exit 1
fi

if [[ ! -f "$ROOT_DIR/Dioxus.toml" ]]; then
  echo "Dioxus.toml is missing. Create it before running dx bundle." >&2
  exit 1
fi

if [[ ! -f "$ROOT_DIR/assets/icon.png" ]]; then
  echo "assets/icon.png is missing. Dioxus bundler may fail without an icon." >&2
  exit 1
fi

if [[ -x "$ROOT_DIR/.venv/bin/python" ]]; then
  PYTHON_BIN="$ROOT_DIR/.venv/bin/python"
elif command -v python3 >/dev/null 2>&1; then
  PYTHON_BIN="python3"
else
  echo "Python 3 is required to generate Debian icon sizes." >&2
  exit 1
fi

echo "==> Generating Linux icon sizes with Pillow"
if ! "$PYTHON_BIN" -c "import PIL" >/dev/null 2>&1; then
  echo "Pillow is required in the selected Python environment ($PYTHON_BIN)." >&2
  echo "Install with: $PYTHON_BIN -m pip install pillow" >&2
  exit 1
fi
"$PYTHON_BIN" "$ROOT_DIR/scripts/generate_icons.py"

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
