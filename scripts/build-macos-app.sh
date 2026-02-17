#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "This script must be run on macOS to produce a .app bundle." >&2
  exit 1
fi

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

echo "==> Bundling macOS .app with Dioxus"
dx bundle --desktop --release --package-types macos "$@"

APP_BUNDLE="$(find "$ROOT_DIR/target/dx" -type d -name '*.app' -path '*/release/macos/*' -print0 2>/dev/null | xargs -0 ls -td 2>/dev/null | head -n1 || true)"
if [[ -z "$APP_BUNDLE" ]]; then
  echo "No macOS .app bundle found under target/dx after dx bundle." >&2
  exit 1
fi

APP_NAME="$(basename "$APP_BUNDLE" .app)"
PLIST_PATH="$APP_BUNDLE/Contents/Info.plist"

if [[ ! -f "$PLIST_PATH" ]]; then
  echo "Info.plist not found at $PLIST_PATH" >&2
  exit 1
fi

if /usr/libexec/PlistBuddy -c "Print :CFBundleShortVersionString" "$PLIST_PATH" >/dev/null 2>&1; then
  /usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString $CARGO_VERSION" "$PLIST_PATH"
else
  /usr/libexec/PlistBuddy -c "Add :CFBundleShortVersionString string $CARGO_VERSION" "$PLIST_PATH"
fi

if /usr/libexec/PlistBuddy -c "Print :CFBundleVersion" "$PLIST_PATH" >/dev/null 2>&1; then
  /usr/libexec/PlistBuddy -c "Set :CFBundleVersion $CARGO_VERSION" "$PLIST_PATH"
else
  /usr/libexec/PlistBuddy -c "Add :CFBundleVersion string $CARGO_VERSION" "$PLIST_PATH"
fi

ARCH="$(uname -m)"
case "$ARCH" in
  arm64) ARCH_TAG="aarch64" ;;
  x86_64) ARCH_TAG="x64" ;;
  *) ARCH_TAG="$ARCH" ;;
esac

mkdir -p "$ROOT_DIR/dist"
EXPECTED_APP="$ROOT_DIR/dist/${APP_NAME}.app"
EXPECTED_ZIP="$ROOT_DIR/dist/${APP_NAME}_${CARGO_VERSION}_${ARCH_TAG}.app.zip"

rm -rf "$EXPECTED_APP"
cp -R "$APP_BUNDLE" "$EXPECTED_APP"

rm -f "$EXPECTED_ZIP"
ditto -c -k --sequesterRsrc --keepParent "$EXPECTED_APP" "$EXPECTED_ZIP"

echo "Built app: $EXPECTED_APP"
echo "Built zip: $EXPECTED_ZIP"
echo "Open app with: open \"$EXPECTED_APP\""
