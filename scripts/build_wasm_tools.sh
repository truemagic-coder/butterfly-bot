#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
CRATE_DIR="$ROOT_DIR/wasm-tool"
OUT_DIR="$ROOT_DIR/wasm"

TOOLS=(
  coding
  mcp
  http_call
  github
  planning
  reminders
  search_internet
  tasks
  todo
  wakeup
)

rustup target add wasm32-unknown-unknown >/dev/null
mkdir -p "$OUT_DIR"

for tool in "${TOOLS[@]}"; do
  feature="tool_${tool}"
  cargo build \
    --manifest-path "$CRATE_DIR/Cargo.toml" \
    --target wasm32-unknown-unknown \
    --release \
    --no-default-features \
    --features "$feature"

  BASE_WASM="$CRATE_DIR/target/wasm32-unknown-unknown/release/butterfly_bot_wasm_tool.wasm"
  cp "$BASE_WASM" "$OUT_DIR/${tool}_tool.wasm"
  echo "built $OUT_DIR/${tool}_tool.wasm"
done
