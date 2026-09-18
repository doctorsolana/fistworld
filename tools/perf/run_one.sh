#!/bin/zsh
# Usage: run_one.sh <label> <scenario> [extra env assignments ...]
#   scenario: dense-stress | secure | fs0-<anchor> ...
# All extra args are passed through `env` to the client.
# Server env per label is derived from scenario arg:
#   FISTWORLD_LAB_SCENARIO=<scenario>
# The client gets FISTWORLD_AUTOSPEED_AFTER=120,1 (the documented replacement
# for the removed server-side FISTWORLD_AUTOSPEED_AFTER).
set -u
ROOT=/Users/terminator2/Coding/fistworld-perf-audit
cd "$ROOT" || exit 1
OUT="$ROOT/logs/perf-audit-2026-09-17"
mkdir -p "$OUT"

label="$1"; shift
scenario="$1"; shift
focus="${FOCUS_OVERRIDE:-112,-158}"

COLD_FILE="$OUT/cold_ms.txt"
if [[ ! -f "$COLD_FILE" ]]; then
  python3 -c "import time;t=time.time();sum(i*i for i in range(3_000_000));print(round((time.time()-t)*1000))" > "$COLD_FILE"
fi
cold=$(cat "$COLD_FILE")

probe() {
  python3 -c "import time;t=time.time();sum(i*i for i in range(3_000_000));print(round((time.time()-t)*1000))"
}
speed_limit() {
  pmset -g therm 2>/dev/null | awk -F'[ =]+' '/CPU_Speed_Limit/{print $NF}' | head -1
}

# --- thermal policing: never start warm -------------------------------------
while true; do
  p=$(probe)
  lim=$(speed_limit)
  lim=${lim:-100}
  echo "[$(date +%H:%M:%S)] probe=${p}ms cold=${cold}ms speed_limit=${lim}"
  if (( p <= cold * 125 / 100 )) && (( lim >= 100 )); then
    break
  fi
  echo "warm (probe or speed limit); cooling 180s"
  sleep 180
done

# --- wait for other heavy work to stop --------------------------------------
while pgrep -f 'cargo (build|run|check|test)|rustc' > /dev/null; do
  echo "[$(date +%H:%M:%S)] another cargo/rustc is running; waiting 20s"
  sleep 20
done

# --- kill stale game processes, only ones whose cwd is THIS worktree --------
kill_mine() {
  local exe="$1"
  for pid in $(pgrep -f "target/playtest/${exe}" 2>/dev/null); do
    local cwd
    cwd=$(lsof -a -p "$pid" -d cwd -Fn 2>/dev/null | tail -1)
    if [[ "$cwd" == *fistworld-perf-audit* ]]; then
      echo "killing stale ${exe} pid=$pid ($cwd)"
      kill -9 "$pid" 2>/dev/null
    fi
  done
}
kill_mine client
kill_mine server
until ! lsof -nP -iUDP 2>/dev/null | grep -q ':5000'; do sleep 1; done

srvlog="$OUT/${label}.server.log"
clilog="$OUT/${label}.client.log"
profile="audit$(date +%H%M%S)$((RANDOM % 1000))"

CITYSIM_MAP_ID=village_lab \
FISTWORLD_VILLAGE_LAB_RUNTIME=1 \
FISTWORLD_LAB_SCENARIO="$scenario" \
FISTWORLD_LAB_WARP=10 \
FISTWORLD_DEV=1 \
  ./target/playtest/server > "$srvlog" 2>&1 &
srvpid=$!
echo "[$(date +%H:%M:%S)] server pid=$srvpid scenario=$scenario label=$label"

# wait for UDP bind
for i in {1..90}; do
  lsof -nP -iUDP 2>/dev/null | grep -q ':5000' && break
  sleep 1
done
if ! lsof -nP -iUDP 2>/dev/null | grep -q ':5000'; then
  echo "server never bound; tail:"; tail -20 "$srvlog"; kill -9 $srvpid 2>/dev/null; exit 1
fi
sleep 6

probe_before=$p
pmset_before=$(speed_limit)

loadlog="$OUT/${label}.load.txt"
( while true; do
    echo "$(date +%H:%M:%S) load=$(uptime | sed 's/.*load averages*: //') top=$(ps -Ao pcpu,comm -r | sed -n '2p')"
    sleep 20
  done ) > "$loadlog" 2>&1 &
loadpid=$!

env BEVY_ASSET_ROOT="$ROOT/client/assets" \
  FISTFORCE_CLIENT_PERF=1 \
  FISTFORCE_CLIENT_PERF_INTERVAL_SECS=10 \
  FISTFORCE_AUTOCONNECT="$profile" \
  FISTFORCE_START_FOCUS="$focus" \
  FISTFORCE_CAMERA_LOCK=1 \
  FISTFORCE_FRAME_CAP=60 \
  FISTFORCE_EXIT_AFTER_SECS=240 \
  FISTFORCE_RENDER_DIAG=1 \
  FISTFORCE_LOG_DIAGNOSTICS=1 \
  FISTWORLD_AUTOSPEED_AFTER=120,1 \
  FISTFORCE_AUTOTIME_PRESET=midday \
  FISTWORLD_AUTOCREATE_VOYAGE=1 \
  FISTWORLD_UX_TOWN=1 \
  "$@" \
  ./target/playtest/client > "$clilog" 2>&1 &
clipid=$!
echo "[$(date +%H:%M:%S)] client pid=$clipid profile=$profile"

# wait for clean exit (max 320s)
for i in {1..320}; do
  kill -0 $clipid 2>/dev/null || break
  sleep 1
done
if kill -0 $clipid 2>/dev/null; then
  echo "client did not exit; killing"
  kill -9 $clipid 2>/dev/null
fi
wait $clipid 2>/dev/null
kill -9 $loadpid 2>/dev/null

kill -9 $srvpid 2>/dev/null
until ! lsof -nP -iUDP 2>/dev/null | grep -q ':5000'; do sleep 1; done

probe_after=$(probe)
pmset_after=$(speed_limit)

{
  echo "label=$label"
  echo "scenario=$scenario"
  echo "focus=$focus"
  echo "profile=$profile"
  echo "probe_cold=$cold"
  echo "probe_before=$probe_before"
  echo "probe_after=$probe_after"
  echo "speed_limit_before=$pmset_before"
  echo "speed_limit_after=$pmset_after"
  echo "date=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "git=$(git rev-parse --short HEAD)"
  echo "extra_env=$*"
} > "$OUT/${label}.meta.txt"

python3 "$OUT/parse.py" "$clilog" > "$OUT/${label}.summary.txt" 2>&1
echo "=== $label ==="
cat "$OUT/${label}.summary.txt"
