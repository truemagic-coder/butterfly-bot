#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

MODE="${1:-smaps}"
TS="$(date -u +%Y%m%dT%H%M%SZ)"
OUT_DIR="$ROOT_DIR/artifacts/memory-profiles/$TS"
mkdir -p "$OUT_DIR"

UI_BIN="$ROOT_DIR/target/profiling/butterfly-bot"
DAEMON_BIN="$ROOT_DIR/target/profiling/butterfly-botd"
DAEMON_URL="http://127.0.0.1:7878"
SLICE_UNIT="butterfly-bot.slice"
USE_SYSTEMD="${BUTTERFLY_PROFILE_USE_SYSTEMD:-1}"

export NO_COLOR=1
export CLICOLOR=0
export RUST_LOG_STYLE=never
export BUTTERFLY_UI_MANAGE_DAEMON=0
export BUTTERFLY_UI_DAEMON_AUTOSTART=0
export BUTTERFLY_UI_AUTOBOOT=0
export BUTTERFLY_UI_WEBKIT_LOW_MEM="${BUTTERFLY_UI_WEBKIT_LOW_MEM:-1}"

if [[ "$BUTTERFLY_UI_WEBKIT_LOW_MEM" == "1" ]]; then
  export WEBKIT_DISABLE_COMPOSITING_MODE=1
  export WEBKIT_DISABLE_DMABUF_RENDERER=1
  export GSK_RENDERER=cairo
fi

cleanup() {
  if [[ -n "${DAEMON_UNIT:-}" ]] && command -v systemctl >/dev/null 2>&1; then
    systemctl --user stop "${DAEMON_UNIT}.service" >/dev/null 2>&1 || true
  fi
  if [[ -n "${UI_UNIT:-}" ]] && command -v systemctl >/dev/null 2>&1; then
    systemctl --user stop "${UI_UNIT}.service" >/dev/null 2>&1 || true
  fi
  if [[ -n "${UI_PID:-}" ]] && kill -0 "$UI_PID" >/dev/null 2>&1; then
    kill "$UI_PID" >/dev/null 2>&1 || true
  fi
  if [[ -n "${SAMPLER_PID:-}" ]] && kill -0 "$SAMPLER_PID" >/dev/null 2>&1; then
    kill "$SAMPLER_PID" >/dev/null 2>&1 || true
  fi
  if [[ -n "${DAEMON_PID:-}" ]] && kill -0 "$DAEMON_PID" >/dev/null 2>&1; then
    kill "$DAEMON_PID" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT INT TERM

echo "[mem-profile] building profiling binaries"
cargo build --profile profiling --bin butterfly-bot --bin butterfly-botd

echo "[mem-profile] starting daemon"
DAEMON_UNIT="butterfly-bot-profile-daemon-$TS"
if [[ "$USE_SYSTEMD" == "1" ]] && command -v systemd-run >/dev/null 2>&1; then
  systemd-run --user \
    --unit="$DAEMON_UNIT" \
    --slice="$SLICE_UNIT" \
    --same-dir \
    --property=OOMScoreAdjust=1000 \
    --property=ManagedOOMPreference=omit \
    "$DAEMON_BIN" --host 127.0.0.1 --port 7878 >"$OUT_DIR/daemon.log" 2>&1 &
  DAEMON_RUNNER_PID=$!

  for _ in $(seq 1 50); do
    DAEMON_PID="$(systemctl --user show "$DAEMON_UNIT.service" -p MainPID --value 2>/dev/null || echo 0)"
    if [[ "$DAEMON_PID" =~ ^[0-9]+$ ]] && [[ "$DAEMON_PID" -gt 1 ]]; then
      break
    fi
    sleep 0.2
  done
  if [[ -z "${DAEMON_PID:-}" || "$DAEMON_PID" -le 1 ]]; then
    echo "[mem-profile] daemon failed to start; see $OUT_DIR/daemon.log" >&2
    exit 1
  fi
else
  "$DAEMON_BIN" --host 127.0.0.1 --port 7878 >"$OUT_DIR/daemon.log" 2>&1 &
  DAEMON_PID=$!
  sleep 1
  if ! kill -0 "$DAEMON_PID" >/dev/null 2>&1; then
    echo "[mem-profile] daemon failed to start; see $OUT_DIR/daemon.log" >&2
    exit 1
  fi
fi

echo "[mem-profile] mode=$MODE out=$OUT_DIR"

case "$MODE" in
  heaptrack)
    if ! command -v heaptrack >/dev/null 2>&1; then
      echo "heaptrack not found" >&2
      exit 1
    fi
    heaptrack --output "$OUT_DIR/heaptrack.ui.gz" \
      "$UI_BIN" --daemon "$DAEMON_URL" >"$OUT_DIR/ui.log" 2>&1 || true
    ;;

  massif)
    if ! command -v valgrind >/dev/null 2>&1; then
      echo "valgrind not found" >&2
      exit 1
    fi
    valgrind --tool=massif \
      --massif-out-file="$OUT_DIR/massif.ui.out" \
      "$UI_BIN" --daemon "$DAEMON_URL" >"$OUT_DIR/ui.log" 2>&1 || true
    if command -v ms_print >/dev/null 2>&1; then
      ms_print "$OUT_DIR/massif.ui.out" > "$OUT_DIR/massif.ui.txt" || true
    fi
    ;;

  smaps)
    UI_UNIT="butterfly-bot-profile-ui-$TS"
    if [[ "$USE_SYSTEMD" == "1" ]] && command -v systemd-run >/dev/null 2>&1; then
      systemd-run --user \
        --unit="$UI_UNIT" \
        --collect \
        --wait \
        --pipe \
        --service-type=exec \
        --slice="$SLICE_UNIT" \
        --same-dir \
        --setenv=BUTTERFLY_UI_MANAGE_DAEMON=0 \
        --setenv=BUTTERFLY_UI_DAEMON_AUTOSTART=0 \
        --setenv=BUTTERFLY_UI_AUTOBOOT=0 \
        --setenv=BUTTERFLY_UI_WEBKIT_LOW_MEM=$BUTTERFLY_UI_WEBKIT_LOW_MEM \
        --setenv=WEBKIT_DISABLE_COMPOSITING_MODE=${WEBKIT_DISABLE_COMPOSITING_MODE:-} \
        --setenv=WEBKIT_DISABLE_DMABUF_RENDERER=${WEBKIT_DISABLE_DMABUF_RENDERER:-} \
        --setenv=GSK_RENDERER=${GSK_RENDERER:-} \
        --property=OOMScoreAdjust=100 \
        --property=ManagedOOMPreference=omit \
        "$UI_BIN" --daemon "$DAEMON_URL" >"$OUT_DIR/ui.log" 2>&1 &
      UI_RUNNER_PID=$!

      for _ in $(seq 1 50); do
        UI_PID="$(systemctl --user show "$UI_UNIT.service" -p MainPID --value 2>/dev/null || echo 0)"
        if [[ "$UI_PID" =~ ^[0-9]+$ ]] && [[ "$UI_PID" -gt 1 ]]; then
          break
        fi
        sleep 0.2
      done
      if [[ -z "${UI_PID:-}" || "$UI_PID" -le 1 ]]; then
        echo "[mem-profile] failed to resolve UI MainPID for $UI_UNIT" >&2
        exit 1
      fi
    else
      "$UI_BIN" --daemon "$DAEMON_URL" >"$OUT_DIR/ui.log" 2>&1 &
      UI_PID=$!
      UI_RUNNER_PID=$UI_PID
    fi

    CGROUP_PATH="$(awk -F: '/^[0-9]+::/{print $3; exit}' "/proc/$UI_PID/cgroup" 2>/dev/null || true)"
    if [[ -z "$CGROUP_PATH" || "$CGROUP_PATH" == "/" ]]; then
      CGROUP_DIR="/sys/fs/cgroup"
    else
      CGROUP_DIR="/sys/fs/cgroup$CGROUP_PATH"
    fi

    {
      echo "ui_pid=$UI_PID"
      echo "cgroup_path=${CGROUP_PATH:-/}"
      echo "cgroup_dir=$CGROUP_DIR"
    } > "$OUT_DIR/cgroup-meta.txt"

    if [[ "$USE_SYSTEMD" != "1" ]] && [[ "${CGROUP_PATH:-}" == *"/app-"*".scope"* ]]; then
      echo "[mem-profile] warning: UI is running in shared app scope: ${CGROUP_PATH}" >&2
      echo "[mem-profile] warning: this can hide attribution and cause external kills unrelated to butterfly-bot" >&2
      {
        echo "warning=shared_app_scope"
        echo "warning_detail=non-systemd run is inside ${CGROUP_PATH}"
      } >> "$OUT_DIR/cgroup-meta.txt"
    fi

    (
      while kill -0 "$UI_PID" >/dev/null 2>&1; do
        sample_ts="$(date -u +%Y%m%dT%H%M%SZ)"
        if [[ -r "/proc/$UI_PID/smaps_rollup" ]]; then
          cat "/proc/$UI_PID/smaps_rollup" > "$OUT_DIR/smaps-rollup-$sample_ts.txt" || true
        fi
        if [[ -r "/proc/$UI_PID/status" ]]; then
          grep -E 'VmRSS|VmHWM|VmSize|Threads' "/proc/$UI_PID/status" > "$OUT_DIR/status-$sample_ts.txt" || true
        fi
        if [[ -d "$CGROUP_DIR" ]]; then
          {
            echo "timestamp=$sample_ts"
            for f in memory.current memory.peak memory.max memory.high memory.events memory.pressure memory.stat; do
              if [[ -r "$CGROUP_DIR/$f" ]]; then
                echo "[$f]"
                cat "$CGROUP_DIR/$f"
              fi
            done
            if [[ -r "/proc/pressure/memory" ]]; then
              echo "[/proc/pressure/memory]"
              cat "/proc/pressure/memory"
            fi
          } > "$OUT_DIR/cgroup-$sample_ts.txt" || true

          if [[ -r "$CGROUP_DIR/cgroup.procs" ]]; then
            cp "$CGROUP_DIR/cgroup.procs" "$OUT_DIR/cgroup-procs-$sample_ts.txt" || true
            {
              echo "timestamp=$sample_ts"
              while read -r pid; do
                [[ -z "$pid" ]] && continue
                [[ ! -r "/proc/$pid/status" ]] && continue
                awk -v pid="$pid" '
                  /^Name:/ {name=$2}
                  /^VmRSS:/ {vmrss=$2}
                  /^RssAnon:/ {rss_anon=$2}
                  /^RssFile:/ {rss_file=$2}
                  /^RssShmem:/ {rss_shmem=$2}
                  /^VmSwap:/ {vmswap=$2}
                  /^Threads:/ {threads=$2}
                  END {
                    if (name == "") name = "unknown"
                    if (vmrss == "") vmrss = 0
                    if (rss_anon == "") rss_anon = 0
                    if (rss_file == "") rss_file = 0
                    if (rss_shmem == "") rss_shmem = 0
                    if (vmswap == "") vmswap = 0
                    if (threads == "") threads = 0
                    printf "pid=%s name=%s vmrss_kb=%s rss_anon_kb=%s rss_file_kb=%s rss_shmem_kb=%s vmswap_kb=%s threads=%s\n", pid, name, vmrss, rss_anon, rss_file, rss_shmem, vmswap, threads
                  }
                ' "/proc/$pid/status"

                comm="$(tr -d '\n' < "/proc/$pid/comm" 2>/dev/null || echo "unknown")"
                safe_comm="$(echo "$comm" | tr -c '[:alnum:]_.-' '_')"
                if [[ -r "/proc/$pid/smaps_rollup" ]]; then
                  smaps_out="$OUT_DIR/cgroup-smaps-rollup-$sample_ts-$pid-$safe_comm.txt"
                  if ! cat "/proc/$pid/smaps_rollup" > "$smaps_out" 2>/dev/null; then
                    rm -f "$smaps_out"
                    echo "pid=$pid name=$comm smaps_rollup=permission_denied" >> "$OUT_DIR/cgroup-smaps-denied-$sample_ts.txt"
                  fi
                fi
              done < "$CGROUP_DIR/cgroup.procs"
            } > "$OUT_DIR/cgroup-proc-status-$sample_ts.txt" || true

            while read -r pid; do
              [[ -z "$pid" ]] && continue
              if [[ -r "/proc/$pid/status" ]]; then
                rss_kb="$(awk '/VmRSS:/{print $2; exit}' "/proc/$pid/status" 2>/dev/null || echo 0)"
                comm="$(tr -d '\n' < "/proc/$pid/comm" 2>/dev/null || echo "unknown")"
                printf "%10s %8s %s\n" "${rss_kb:-0}" "$pid" "$comm"
              fi
            done < "$CGROUP_DIR/cgroup.procs" | sort -nr | head -n 30 > "$OUT_DIR/cgroup-top-rss-$sample_ts.txt" || true
          fi
        fi
        sleep 2
      done
    ) &
    SAMPLER_PID=$!

    wait "$UI_RUNNER_PID" || true

    if [[ "$USE_SYSTEMD" == "1" ]] && command -v systemctl >/dev/null 2>&1; then
      systemctl --user show "$UI_UNIT.service" \
        -p Id \
        -p Names \
        -p MainPID \
        -p ExecMainCode \
        -p ExecMainStatus \
        -p Result \
        -p SubState \
        -p ActiveState \
        -p ControlGroup \
        -p OOMPolicy \
        -p OOMScoreAdjust \
        -p MemoryCurrent \
        -p MemoryPeak \
        -p CPUUsageNSec \
        -p RuntimeMaxUSec \
        > "$OUT_DIR/ui-unit-final.txt" 2>&1 || true
    fi

    if [[ "$USE_SYSTEMD" == "1" ]] && command -v journalctl >/dev/null 2>&1; then
      journalctl --user -u "$UI_UNIT.service" --no-pager -n 200 \
        > "$OUT_DIR/ui-unit-journal.txt" 2>&1 || true
    fi
    ;;

  *)
    echo "unknown mode: $MODE (use: smaps|heaptrack|massif)" >&2
    exit 1
    ;;
esac

echo "[mem-profile] done. artifacts: $OUT_DIR"