#!/bin/zsh
# Usage: run_uncapped.sh <label> <scenario> [extra client env assignments ...]
# Uncapped (FISTFORCE_FRAME_CAP=0) small-town measurement harness.
# Adapted from logs/perf-audit-2026-09-17/run_one.sh; adds the display/lock/
# frontmost/screenshot validity gate required for uncapped numbers.
set -u
ROOT=/Users/terminator2/Coding/fistworld-perf-audit
cd "$ROOT" || exit 1
OUT="$ROOT/logs/perf-fundamentals"
TRACEIT="$ROOT/logs/perf-audit-2026-09-17"
mkdir -p "$OUT"

label="$1"; shift
scenario="$1"; shift
focus="${FOCUS_OVERRIDE:-112,-158}"

cold=$(cat "$TRACEIT/cold_ms.txt")
probe() {
  python3 -c "import time;t=time.time();sum(i*i for i in range(3_000_000));print(round((time.time()-t)*1000))"
}
display_line() { pmset -g log 2>/dev/null | grep 'Display is turned' | tail -1; }
front_name() {
  local asn; asn=$(lsappinfo front 2>/dev/null)
  [[ -z "$asn" ]] && { echo "<none>"; return; }
  lsappinfo info -only name "$asn" 2>/dev/null | sed -n 's/.*"LSDisplayName"="\([^"]*\)".*/\1/p' | head -1
}

# Seconds since the last keyboard/mouse input (HID idle time).
idle_secs() {
  ioreg -c IOHIDSystem 2>/dev/null | awk '/HIDIdleTime/ {print int($NF/1000000000); exit}'
}
front_bad() {
  local f="$1"
  # macOS reports loginwindow as frontmost both on the lock screen AND when no
  # app window has focus (e.g. right after a fullscreen game window closed).
  # Only treat it as locked when the user has also been idle for 10 minutes.
  if [[ "$f" == "loginwindow" || "$f" == "LoginWindow" || -z "$f" ]]; then
    local idle; idle=$(idle_secs); idle=${idle:-0}
    (( idle > 600 ))
    return
  fi
  [[ "$f" == "UserNotificationCenter" ]]
}

# --- thermal gate ------------------------------------------------------------
while true; do
  p=$(probe)
  echo "[$(date +%H:%M:%S)] probe=${p}ms cold=${cold}ms"
  (( p <= 100 )) && break
  echo "warm; cooling 180s"
  sleep 180
done

# --- quiet gate --------------------------------------------------------------
while pgrep -f 'cargo (build|run|check|test)|rustc' > /dev/null; do
  echo "[$(date +%H:%M:%S)] cargo/rustc running; waiting 20s"
  sleep 20
done

# --- display/lock/front gate -------------------------------------------------
disp="$(display_line)"
lock=$(ioreg -n Root -d1 -a 2>/dev/null | grep -c CGSSessionScreenIsLocked)
front="$(front_name)"
# A stuck notification banner reports as the frontmost application and can sit
# above a fullscreen game, which makes frames artificially cheap. Dismiss it
# once (the daemon relaunches on demand) and re-sample; a persistent banner
# still refuses the run.
if [[ "$front" == "UserNotificationCenter" ]]; then
  killall UserNotificationCenter 2>/dev/null
  sleep 3
  front="$(front_name)"
fi
screenshot="unavailable_tcc"
if screencapture -x "$OUT/${label}.screen.png" 2>/dev/null && [[ -s "$OUT/${label}.screen.png" ]]; then
  screenshot="ok"
fi
echo "[$(date +%H:%M:%S)] display='${disp}' lock_probe=${lock} front='${front}' screenshot=${screenshot}"
if [[ "$disp" != *"turned on"* ]] || front_bad "$front"; then
  echo "SESSION NOT MEASURABLE (display off / lock / front not a real app); refusing to run"
  exit 3
fi

# --- kill stale game processes from THIS worktree only ------------------------
kill_mine() {
  local exe="$1"
  for pid in $(pgrep -f "target/playtest/${exe}" 2>/dev/null); do
    local cwd
    cwd=$(lsof -a -p "$pid" -d cwd -Fn 2>/dev/null | tail -1)
    if [[ "$cwd" == *fistworld-perf-audit* ]]; then
      kill -9 "$pid" 2>/dev/null
    fi
  done
}
kill_mine client
kill_mine server
until ! lsof -nP -iUDP 2>/dev/null | grep -q ':5000'; do sleep 1; done

srvlog="$OUT/${label}.server.log"
clilog="$OUT/${label}.client.log"
profile="fund$(date +%H%M%S)$((RANDOM % 1000))"

CITYSIM_MAP_ID=village_lab \
FISTWORLD_VILLAGE_LAB_RUNTIME=1 \
FISTWORLD_LAB_SCENARIO="$scenario" \
FISTWORLD_LAB_WARP=10 \
FISTWORLD_DEV=1 \
  ./target/playtest/server > "$srvlog" 2>&1 &
srvpid=$!
for i in {1..90}; do
  lsof -nP -iUDP 2>/dev/null | grep -q ':5000' && break
  sleep 1
done
if ! lsof -nP -iUDP 2>/dev/null | grep -q ':5000'; then
  echo "server never bound"; tail -20 "$srvlog"; kill -9 $srvpid 2>/dev/null; exit 1
fi
sleep 6

probe_before=$p

# --- validity sampler while the client runs ----------------------------------
loadlog="$OUT/${label}.session.txt"
( while true; do
    echo "$(date +%H:%M:%S) front='$(front_name)' disp_ok=$([[ "$(display_line)" == *"turned on"* ]] && echo 1 || echo 0) load=$(uptime | sed 's/.*load averages*: //')"
    sleep 20
  done ) > "$loadlog" 2>&1 &
loadpid=$!

env BEVY_ASSET_ROOT="$ROOT/client/assets" \
  FISTFORCE_CLIENT_PERF=1 \
  FISTFORCE_CLIENT_PERF_INTERVAL_SECS=10 \
  FISTFORCE_AUTOCONNECT="$profile" \
  FISTFORCE_START_FOCUS="$focus" \
  FISTFORCE_CAMERA_LOCK=1 \
  FISTFORCE_FRAME_CAP=0 \
  FISTFORCE_EXIT_AFTER_SECS=150 \
  FISTFORCE_RENDER_DIAG=1 \
  FISTFORCE_LOG_DIAGNOSTICS=1 \
  FISTWORLD_AUTOSPEED_AFTER=60,1 \
  FISTFORCE_AUTOTIME_PRESET=midday \
  FISTWORLD_AUTOCREATE_VOYAGE=1 \
  FISTWORLD_UX_TOWN=1 \
  "$@" \
  ./target/playtest/client > "$clilog" 2>&1 &
clipid=$!
echo "[$(date +%H:%M:%S)] client pid=$clipid profile=$profile scenario=$scenario focus=$focus"

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
front_after="$(front_name)"
{
  echo "label=$label"
  echo "scenario=$scenario"
  echo "focus=$focus"
  echo "profile=$profile"
  echo "frame_cap=0"
  echo "probe_cold=$cold"
  echo "probe_before=$probe_before"
  echo "probe_after=$probe_after"
  echo "display_before=${disp}"
  echo "lock_probe_before=${lock}"
  echo "front_before=${front}"
  echo "front_after=${front_after}"
  echo "screenshot=${screenshot}"
  echo "date=$(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "git=$(git rev-parse --short HEAD)"
  echo "extra_env=$*"
} > "$OUT/${label}.meta.txt"

python3 "$TRACEIT/parse.py" "$clilog" > "$OUT/${label}.summary.txt" 2>&1
echo "=== $label ==="
cat "$OUT/${label}.summary.txt"

if grep -q 'VALID_RUN=NO' "$OUT/${label}.summary.txt" \
   || grep -q 'count_panic=[1-9]' "$OUT/${label}.summary.txt"; then
  echo "INVALID RUN: creator overlay or panic"
  exit 4
fi
exit 0
