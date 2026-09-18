#!/bin/zsh
# Usage: run_server.sh <label> <world.ron> <warp 1|25> <seconds> [extra env assignments ...]
# Headless server load run. No client, no display: the only gate is that no
# other heavy work is running.
set -u
ROOT=/Users/terminator2/Coding/fistworld-perf-audit
OUT="$ROOT/logs/perf-server"
cd "$ROOT" || exit 1
label="$1"; world="$2"; warp="$3"; secs="$4"; shift 4

while pgrep -f 'cargo (build|test)|rustc' >/dev/null; do echo "[$(date +%H:%M:%S)] waiting for a build to finish"; sleep 20; done
# Only kill servers belonging to THIS worktree.
for pid in $(pgrep -f 'target/playtest/server' 2>/dev/null); do
  cwd=$(lsof -a -p "$pid" -d cwd -Fn 2>/dev/null | tail -1)
  [[ "$cwd" == *fistworld-perf-audit* ]] && kill -9 "$pid" 2>/dev/null
done

probe() { python3 -c "import time;t=time.time();sum(i*i for i in range(3_000_000));print(round((time.time()-t)*1000))"; }
port=$(( 5100 + RANDOM % 300 ))
tracedir="$OUT/trace-$label"; rm -rf "$tracedir"; mkdir -p "$tracedir"
log="$OUT/${label}.log"
probe_before=$(probe)

env FISTWORLD_WORLD_CONFIG="$ROOT/$world" \
    FISTWORLD_WORLD_SEED="${SEED:-91}" \
    FISTWORLD_SMALL_WORLD_TRACE_DIR="$tracedir" \
    FISTWORLD_SMALL_WORLD_TRACE_WARP="$warp" \
    FISTWORLD_SMALL_WORLD_TRACE_SAMPLE_SECONDS=10 \
    FISTWORLD_SERVER_PORT="$port" \
    "$@" \
    "${BINARY:-./target/playtest/server}" > "$log" 2>&1 &
pid=$!
echo "[$(date +%H:%M:%S)] $label pid=$pid port=$port world=$world warp=$warp for ${secs}s"
for i in $(seq 1 $secs); do kill -0 $pid 2>/dev/null || { echo "server exited early at ${i}s"; break; }; sleep 1; done
kill -9 $pid 2>/dev/null; wait $pid 2>/dev/null
probe_after=$(probe)

{
  echo "label=$label"; echo "world=$world"; echo "warp=$warp"; echo "seconds=$secs"
  echo "seed=${SEED:-91}"; echo "extra_env=$*"
  echo "probe_before=$probe_before"; echo "probe_after=$probe_after"
  echo "date=$(date -u +%Y-%m-%dT%H:%M:%SZ)"; echo "git=$(git rev-parse --short HEAD)"
} > "$OUT/${label}.meta.txt"
python3 "$OUT/parse_server.py" "$log" > "$OUT/${label}.summary.txt" 2>&1
echo "=== $label ==="; cat "$OUT/${label}.summary.txt"
