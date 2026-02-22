#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

if ! command -v dpkg-deb >/dev/null 2>&1; then
  echo "dpkg-deb is required but not found in PATH." >&2
  exit 1
fi

if [[ ! -f "$ROOT_DIR/assets/icon.png" ]]; then
  echo "assets/icon.png is missing." >&2
  exit 1
fi

ICON_SIZES=(16 24 32 48 64 128 256 512)
MISSING_ICONS=0

for size in "${ICON_SIZES[@]}"; do
  icon_path="$ROOT_DIR/assets/icons/hicolor/${size}x${size}/apps/butterfly-bot.png"
  if [[ ! -f "$icon_path" ]]; then
    MISSING_ICONS=1
    break
  fi
done

if [[ "$MISSING_ICONS" -eq 0 ]]; then
  echo "==> Using pre-generated Linux icon sizes from assets/icons/hicolor"
else
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
fi

echo "==> Building WASM tool modules"
./scripts/build_wasm_tools.sh

echo "==> Building release binaries"
cargo build --release --bin butterfly-bot --bin butterfly-botd "$@"

CARGO_VERSION="$(awk -F'"' '
  /^\[package\]$/ { in_pkg=1; next }
  /^\[/ && $0 != "[package]" { in_pkg=0 }
  in_pkg && $1 ~ /^version[[:space:]]*=[[:space:]]*$/ { print $2; exit }
' "$ROOT_DIR/Cargo.toml")"

if [[ -z "$CARGO_VERSION" ]]; then
  echo "Could not determine package version from Cargo.toml [package].version." >&2
  exit 1
fi

DEB_ARCH="$(dpkg --print-architecture)"
STAGE_DIR="$ROOT_DIR/dist/deb-root"
DEB_FILE="$ROOT_DIR/dist/butterfly-bot_${CARGO_VERSION}_${DEB_ARCH}.deb"

echo "==> Staging Debian filesystem"
rm -rf "$STAGE_DIR"
mkdir -p "$STAGE_DIR/DEBIAN"
mkdir -p "$STAGE_DIR/usr/bin"
mkdir -p "$STAGE_DIR/usr/lib/butterfly-bot/wasm"
mkdir -p "$STAGE_DIR/usr/share/icons/hicolor"

cat > "$STAGE_DIR/DEBIAN/control" <<EOF
Package: butterfly-bot
Version: $CARGO_VERSION
Section: utils
Priority: optional
Architecture: $DEB_ARCH
Maintainer: True Magic Coder
Description: Butterfly Bot desktop UI + daemon
EOF

install -m 0755 "$ROOT_DIR/target/release/butterfly-bot" "$STAGE_DIR/usr/bin/butterfly-bot"
install -m 0755 "$ROOT_DIR/target/release/butterfly-botd" "$STAGE_DIR/usr/bin/butterfly-botd"

cp "$ROOT_DIR/wasm/"*_tool.wasm "$STAGE_DIR/usr/lib/butterfly-bot/wasm/"
cp -r "$ROOT_DIR/assets/icons/hicolor"/* "$STAGE_DIR/usr/share/icons/hicolor/"

echo "==> Building Debian package"
mkdir -p "$ROOT_DIR/dist"
rm -f "$DEB_FILE"
dpkg-deb --build "$STAGE_DIR" "$DEB_FILE"

echo "Built: $DEB_FILE"
echo "Install with: sudo dpkg -i \"$DEB_FILE\""
