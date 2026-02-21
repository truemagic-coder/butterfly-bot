#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

if ! command -v systemd-run >/dev/null 2>&1; then
  echo "systemd-run is required for scoped launch" >&2
  exec target/release/butterfly-bot
fi

DAEMON_UNIT="butterfly-bot-daemon"
UI_UNIT="butterfly-bot-ui"
SLICE_UNIT="butterfly-bot.slice"

if command -v systemctl >/dev/null 2>&1; then
  systemctl --user stop "${DAEMON_UNIT}.service" >/dev/null 2>&1 || true
fi

systemd-run --user \
  --unit="$DAEMON_UNIT" \
  --slice="$SLICE_UNIT" \
  --same-dir \
  --property=OOMScoreAdjust=1000 \
  --property=ManagedOOMPreference=omit \
  "$ROOT_DIR/target/release/butterfly-botd" \
  --host 127.0.0.1 \
  --port 7878

SYSTEMD_ARGS=(
  --user
  --unit="$UI_UNIT"
  --collect
  --wait
  --pipe
  --service-type=exec
  --slice="$SLICE_UNIT"
  --same-dir
  --property=OOMScoreAdjust=0
  --property=MemoryHigh=infinity
  --property=MemoryMax=infinity
  --property=ManagedOOMPreference=omit
)

for var_name in DISPLAY WAYLAND_DISPLAY XDG_RUNTIME_DIR DBUS_SESSION_BUS_ADDRESS XAUTHORITY RUST_LOG; do
  if [[ -n "${!var_name:-}" ]]; then
    SYSTEMD_ARGS+=("--setenv=${var_name}=${!var_name}")
  fi
done

SYSTEMD_ARGS+=("--setenv=BUTTERFLY_UI_AUTOBOOT=0")
SYSTEMD_ARGS+=("--setenv=BUTTERFLY_UI_DAEMON_AUTOSTART=0")
SYSTEMD_ARGS+=("--setenv=BUTTERFLY_UI_MANAGE_DAEMON=0")

WEBKIT_LOW_MEM="${BUTTERFLY_UI_WEBKIT_LOW_MEM:-1}"
SYSTEMD_ARGS+=("--setenv=BUTTERFLY_UI_WEBKIT_LOW_MEM=${WEBKIT_LOW_MEM}")
if [[ "$WEBKIT_LOW_MEM" == "1" ]]; then
  SYSTEMD_ARGS+=("--setenv=WEBKIT_DISABLE_COMPOSITING_MODE=1")
  SYSTEMD_ARGS+=("--setenv=WEBKIT_DISABLE_DMABUF_RENDERER=1")
  SYSTEMD_ARGS+=("--setenv=GSK_RENDERER=cairo")
fi

stop_units() {
  if command -v systemctl >/dev/null 2>&1; then
    systemctl --user stop "${UI_UNIT}.service" >/dev/null 2>&1 || true
    systemctl --user stop "${DAEMON_UNIT}.service" >/dev/null 2>&1 || true
  fi
}

on_interrupt() {
  echo "Interrupted; stopping ${UI_UNIT}.service and ${DAEMON_UNIT}.service" >&2
  stop_units
  exit 130
}

trap on_interrupt INT TERM

systemd-run "${SYSTEMD_ARGS[@]}" "$ROOT_DIR/target/release/butterfly-bot"
rc=$?

if command -v systemctl >/dev/null 2>&1; then
  systemctl --user show "${UI_UNIT}.service" \
    -p Result -p ExecMainCode -p ExecMainStatus -p OOMPolicy -p MemoryPeak || true
fi

if command -v journalctl >/dev/null 2>&1; then
  journalctl --user -u "${UI_UNIT}.service" -n 20 --no-pager || true
fi

stop_units
exit $rc
