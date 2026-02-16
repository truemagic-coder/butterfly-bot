#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT_DIR"

APP_NAME="butterfly-bot"
RUN_AFTER=0
BUILD_MODE="lxd"
SAFE_GL=0

usage() {
  cat <<'EOF'
Usage: ./scripts/test-local-snap.sh [options]

Builds the local snap, installs/updates it from local artifact, and optionally runs it.

Options:
  --run                 Run butterfly-bot after install/refresh
  --safe-gl             Run app with BUTTERFLY_BOT_SAFE_GL=1
  --destructive-mode    Build with snapcraft --destructive-mode (instead of --use-lxd)
  --no-build            Skip snapcraft build and only install/refresh newest local .snap
  -h, --help            Show this help

Environment:
  SNAPCRAFT_ARGS        Extra args appended to snapcraft command

Examples:
  ./scripts/test-local-snap.sh
  ./scripts/test-local-snap.sh --run
  ./scripts/test-local-snap.sh --run --safe-gl
  SNAPCRAFT_ARGS="--verbosity debug" ./scripts/test-local-snap.sh
EOF
}

DO_BUILD=1
while (($# > 0)); do
  case "$1" in
    --run)
      RUN_AFTER=1
      ;;
    --safe-gl)
      SAFE_GL=1
      ;;
    --destructive-mode)
      BUILD_MODE="destructive"
      ;;
    --no-build)
      DO_BUILD=0
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown option: $1" >&2
      usage
      exit 2
      ;;
  esac
  shift
done

if ! command -v snapcraft >/dev/null 2>&1; then
  echo "snapcraft is required but not found in PATH" >&2
  exit 1
fi

if ! command -v snap >/dev/null 2>&1; then
  echo "snap is required but not found in PATH" >&2
  exit 1
fi

if ((DO_BUILD == 1)); then
  if [[ "$BUILD_MODE" == "destructive" ]]; then
    echo "==> Building snap with destructive mode"
    snapcraft pack --destructive-mode ${SNAPCRAFT_ARGS:-}
  else
    echo "==> Building snap with LXD"
    snapcraft pack --use-lxd ${SNAPCRAFT_ARGS:-}
  fi
fi

SNAP_FILE="$(ls -1t ./${APP_NAME}_*.snap 2>/dev/null | head -n1 || true)"
if [[ -z "$SNAP_FILE" ]]; then
  echo "No local snap artifact found matching ${APP_NAME}_*.snap" >&2
  exit 1
fi

echo "==> Using snap artifact: $SNAP_FILE"

INSTALL_HELP="$(snap install --help 2>/dev/null || true)"
HAS_DANGEROUS=0
if grep -q -- '--dangerous' <<<"$INSTALL_HELP"; then
  HAS_DANGEROUS=1
fi

install_from_file() {
  local first_try_output
  local first_try_code

  set +e
  first_try_output="$(sudo snap install "$SNAP_FILE" 2>&1)"
  first_try_code=$?
  set -e

  if ((first_try_code == 0)); then
    echo "$first_try_output"
    return 0
  fi

  if grep -qiE 'dangerous|not signed|no pre-acknowledged signatures|cannot find signatures|signatures with metadata' <<<"$first_try_output"; then
    if ((HAS_DANGEROUS == 1)); then
      echo "==> Retrying install with --dangerous"
      sudo snap install --dangerous "$SNAP_FILE"
      return 0
    else
      echo "==> Retrying install with --devmode (implies unasserted local install)"
      sudo snap install --devmode "$SNAP_FILE"
      return 0
    fi
  fi

  echo "$first_try_output" >&2
  return "$first_try_code"
}

if snap list "$APP_NAME" >/dev/null 2>&1; then
  echo "==> Updating installed snap from local artifact"
  if ! install_from_file; then
    echo "==> Direct update failed, attempting reinstall"
    sudo snap remove "$APP_NAME"
    install_from_file
  fi
else
  echo "==> Installing snap"
  install_from_file
fi

if ((RUN_AFTER == 1)); then
  echo "==> Running app"
  if ((SAFE_GL == 1)); then
    BUTTERFLY_BOT_SAFE_GL=1 snap run "$APP_NAME"
  else
    snap run "$APP_NAME"
  fi
else
  echo "==> Done"
  echo "Run it with: snap run $APP_NAME"
fi
