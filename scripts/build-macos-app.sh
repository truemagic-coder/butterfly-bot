#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "This script must be run on macOS to produce a .app bundle." >&2
  exit 1
fi

if [[ ! -f "$ROOT_DIR/assets/icon.png" ]]; then
  echo "assets/icon.png is missing." >&2
  exit 1
fi

CARGO_VERSION="$(awk -F'"' '
  /^\[package\]$/ { in_pkg=1; next }
  /^\[/ && $0 != "[package]" { in_pkg=0 }
  in_pkg && $1 ~ /^version[[:space:]]*=[[:space:]]*$/ { print $2; exit }
' "$ROOT_DIR/Cargo.toml")"

if [[ -z "$CARGO_VERSION" ]]; then
  echo "Could not determine package version from Cargo.toml [package].version." >&2
  exit 1
fi

echo "==> Building WASM tool modules"
./scripts/build_wasm_tools.sh

echo "==> Building release UI binary"
cargo build --release --bin butterfly-bot "$@"

APP_NAME="Butterfly Bot"
APP_BUNDLE="$ROOT_DIR/dist/${APP_NAME}.app"
rm -rf "$APP_BUNDLE"
mkdir -p "$APP_BUNDLE/Contents/MacOS"
mkdir -p "$APP_BUNDLE/Contents/Resources"

cp "$ROOT_DIR/target/release/butterfly-bot" "$APP_BUNDLE/Contents/MacOS/butterfly-bot"
chmod 0755 "$APP_BUNDLE/Contents/MacOS/butterfly-bot"

PLIST_PATH="$APP_BUNDLE/Contents/Info.plist"
cat > "$PLIST_PATH" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Butterfly Bot</string>
  <key>CFBundleDisplayName</key><string>Butterfly Bot</string>
  <key>CFBundleIdentifier</key><string>com.truemagic-coder.butterfly-bot</string>
  <key>CFBundleVersion</key><string>$CARGO_VERSION</string>
  <key>CFBundleShortVersionString</key><string>$CARGO_VERSION</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleExecutable</key><string>butterfly-bot</string>
</dict>
</plist>
EOF

WASM_SOURCE_DIR="$ROOT_DIR/wasm"
WASM_BUNDLE_DIR="$APP_BUNDLE/Contents/Resources/wasm"

if [[ ! -d "$WASM_SOURCE_DIR" ]]; then
  echo "WASM output directory not found at $WASM_SOURCE_DIR" >&2
  exit 1
fi

echo "==> Embedding WASM modules into app bundle"
mkdir -p "$WASM_BUNDLE_DIR"
cp "$WASM_SOURCE_DIR"/*_tool.wasm "$WASM_BUNDLE_DIR/"

if [[ -n "${APPLE_SIGN_IDENTITY:-}" ]]; then
  echo "==> Signing macOS bundle with identity: $APPLE_SIGN_IDENTITY"
  codesign --force --deep --options runtime --timestamp --sign "$APPLE_SIGN_IDENTITY" "$APP_BUNDLE"
else
  echo "==> Signing macOS bundle with ad-hoc identity"
  codesign --force --deep --sign - "$APP_BUNDLE"
fi

echo "==> Verifying code signature"
codesign --verify --deep --strict --verbose=2 "$APP_BUNDLE"

ARCH="$(uname -m)"
case "$ARCH" in
  arm64) ARCH_TAG="aarch64" ;;
  x86_64) ARCH_TAG="x64" ;;
  *) ARCH_TAG="$ARCH" ;;
esac

mkdir -p "$ROOT_DIR/dist"
EXPECTED_APP="$ROOT_DIR/dist/${APP_NAME}.app"
EXPECTED_ZIP="$ROOT_DIR/dist/${APP_NAME}_${CARGO_VERSION}_${ARCH_TAG}.app.zip"

rm -f "$EXPECTED_ZIP"
ditto -c -k --sequesterRsrc --keepParent "$EXPECTED_APP" "$EXPECTED_ZIP"

echo "Built app: $EXPECTED_APP"
echo "Built zip: $EXPECTED_ZIP"
echo "Open app with: open \"$EXPECTED_APP\""
