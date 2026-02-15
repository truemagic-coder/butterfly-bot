#!/bin/sh
set -eu

APP_ROOT="$SNAP_USER_COMMON/butterfly-bot"
WASM_BUNDLE="$SNAP/usr/share/butterfly-bot/wasm"

mkdir -p "$APP_ROOT" "$APP_ROOT/data" "$APP_ROOT/.cache" "$APP_ROOT/.config"

if [ -d "$WASM_BUNDLE" ]; then
    if [ -e "$APP_ROOT/wasm" ] && [ ! -L "$APP_ROOT/wasm" ]; then
        rm -rf "$APP_ROOT/wasm"
    fi
    if [ ! -L "$APP_ROOT/wasm" ]; then
        ln -s "$WASM_BUNDLE" "$APP_ROOT/wasm"
    fi
fi

cd "$APP_ROOT"

export BUTTERFLY_BOT_DB="${BUTTERFLY_BOT_DB:-$APP_ROOT/data/butterfly-bot.db}"
export BUTTERFLY_BOT_DISABLE_KEYRING="${BUTTERFLY_BOT_DISABLE_KEYRING:-1}"
export XDG_CACHE_HOME="${XDG_CACHE_HOME:-$APP_ROOT/.cache}"
export XDG_CONFIG_HOME="${XDG_CONFIG_HOME:-$APP_ROOT/.config}"
