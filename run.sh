#!/bin/bash
# Run script for Fistworld
# Usage: ./run.sh [server|client|both|testworld|uxworld|uxstressworld|regionalworld|stoneworld|tradeworld|merchantworld|economyworld|stressworld|denseworld|battleworld|battle5v5|archerworld|realworld|multi] [--release|--dev]
#
# BUILD PROFILE. This used to build --release every time, which meant a ten
# minute wait for a one line change: release turns on thin LTO, which re-links
# the whole program on every edit, and cargo disables incremental compilation for
# release profiles entirely. Measured on this repo, same one line change:
#
#     --release    9m 59s
#     --dev            6s
#
# So the default is now `playtest`: opt-level 3 everywhere like release, but
# without LTO and with incremental on. Near-release runtime speed, rebuilds in
# seconds.
#
#   (default)   playtest  -- play the game
#   --dev       dev       -- fastest rebuilds, workspace code at opt-level 1
#   --release   release   -- true shipping build; use when MEASURING performance
#
# IF THE LINK FAILS with "Undefined symbols for architecture arm64" naming a
# mangled generic (typically something like ...WindExtension...), the code is
# fine and a stale playtest artifact is not. `playtest` runs incremental with
# codegen-units=256 and no LTO, which occasionally leaves an rlib referencing a
# monomorphisation that no longer exists. Note `cargo test` will still pass,
# because it uses the dev profile.
#
#     cargo clean -p client --profile playtest
#
# That is ~30s and fixes it. Clearing target/playtest/incremental alone does
# NOT -- the stale reference lives in the rlib, not the incremental cache.

set -euo pipefail

PROFILE="playtest"
ARGS=()
for arg in "$@"; do
    case "$arg" in
        --release) PROFILE="release" ;;
        --dev)     PROFILE="dev" ;;
        *)         ARGS+=("$arg") ;;
    esac
done
set -- "${ARGS[@]+"${ARGS[@]}"}"

MODE=${1:-both}

# Rendered village fixtures retain both process logs so a visual observation can
# be matched to authoritative server state after the window closes.
CAPTURE_VILLAGE_LOGS=0
VILLAGE_LOG_DIR=""
STREAM_VILLAGE_LOGS="${FISTWORLD_STREAM_LOGS:-1}"

# The rendered Village Lab is explicit rather than tied to the map id. This
# preserves `CITYSIM_MAP_ID=village_lab ./run.sh` as an empty god-mode sandbox.
if [[ "$MODE" == "testworld" || "$MODE" == "testlab" || "$MODE" == "uxworld" || "$MODE" == "uxstressworld" || "$MODE" == "regionalworld" || "$MODE" == "stoneworld" || "$MODE" == "tradeworld" || "$MODE" == "merchantworld" || "$MODE" == "economyworld" || "$MODE" == "stressworld" || "$MODE" == "denseworld" || "$MODE" == "battleworld" ]]; then
    if [[ "$MODE" == "uxworld" || "$MODE" == "uxstressworld" ]]; then
        # A mature City fixture for exercising player-facing management UX
        # without waiting through days of organic development. Its residents,
        # buildings, inventories and companies are ordinary runtime entities;
        # only their prepared opening state is synthetic.
        export CITYSIM_MAP_ID="village_lab"
        export FISTWORLD_UX_TOWN="${FISTWORLD_UX_TOWN:-1}"
        export FISTWORLD_LAB_SCENARIO="${FISTWORLD_LAB_SCENARIO:-secure}"
        if [[ "$MODE" == "uxstressworld" ]]; then
            # Let the prepared economy establish its first routines, then add
            # one real 500-person immigration shock on scenario day 2. The
            # causal watchdog distinguishes legitimate queues/work from actors
            # whose target, route or freight ownership has genuinely stalled.
            export FISTWORLD_UX_STRESS="${FISTWORLD_UX_STRESS:-1}"
            export FISTWORLD_STUCK_WATCH="${FISTWORLD_STUCK_WATCH:-1}"
            export FISTWORLD_LAB_DAY_TWO_ARRIVALS="${FISTWORLD_LAB_DAY_TWO_ARRIVALS:-500}"
            export FISTWORLD_LAB_ARRIVAL_DAY="${FISTWORLD_LAB_ARRIVAL_DAY:-2}"
            export FISTWORLD_LAB_ARRIVAL_OFFSET="${FISTWORLD_LAB_ARRIVAL_OFFSET:-0,-90}"
            export FISTWORLD_LAB_WARP="${FISTWORLD_LAB_WARP:-10}"
            export FISTFORCE_START_ZOOM="${FISTFORCE_START_ZOOM:-520}"
        else
            export FISTWORLD_LAB_DAY_TWO_ARRIVALS="${FISTWORLD_LAB_DAY_TWO_ARRIVALS:-0}"
            export FISTWORLD_LAB_WARP="${FISTWORLD_LAB_WARP:-1}"
            export FISTFORCE_START_ZOOM="${FISTFORCE_START_ZOOM:-430}"
        fi
        export FISTFORCE_START_FOCUS="${FISTFORCE_START_FOCUS:--108,220}"
        export FISTFORCE_SERVER_PERF="${FISTFORCE_SERVER_PERF:-1}"
        export FISTFORCE_CLIENT_PERF="${FISTFORCE_CLIENT_PERF:-1}"
        # Retain the full trace on disk without making hundreds of prepared
        # actors compete with the renderer for terminal output at startup.
        STREAM_VILLAGE_LOGS="${FISTWORLD_STREAM_LOGS:-0}"
    elif [[ "$MODE" == "regionalworld" ]]; then
        export CITYSIM_MAP_ID="regional_lab"
    elif [[ "$MODE" == "battleworld" ]]; then
        # A quarter-size open field of its own; no village belongs on it.
        export CITYSIM_MAP_ID="battle_lab"
    else
        export CITYSIM_MAP_ID="village_lab"
    fi
    export FISTWORLD_VILLAGE_LAB_RUNTIME="${FISTWORLD_VILLAGE_LAB_RUNTIME:-1}"
    if [[ "$MODE" == "regionalworld" ]]; then
        # Four ordinary autonomous settlements on the larger seed-37 map:
        # fertile coast, frozen poor soil, forest edge and Stone country.
        # Identical gentle arrivals expose how geography changes investment,
        # labour, prices and household outcomes without a crowd shock.
        export FISTWORLD_LAB_SCENARIO="${FISTWORLD_LAB_SCENARIO:-regional-economy}"
        export FISTWORLD_LAB_WARP="${FISTWORLD_LAB_WARP:-10}"
        export FISTFORCE_START_FOCUS="${FISTFORCE_START_FOCUS:-0,0}"
        export FISTFORCE_START_ZOOM="${FISTFORCE_START_ZOOM:-1250}"
        export FISTFORCE_SERVER_PERF="${FISTFORCE_SERVER_PERF:-1}"
        export FISTFORCE_CLIENT_PERF="${FISTFORCE_CLIENT_PERF:-1}"
    elif [[ "$MODE" == "battleworld" ]]; then
        # A battlefield, not a village: the skirmish scenario stages NO
        # settlement - just idle villagers on open ground waiting to be
        # conscripted. No economy, no migration, nothing moves until you act.
        # The hero auto-spawns standing at the camera focus (no name modal,
        # no dinghy voyage, no cinematic). God mode is on: SPAWN VILLAGER
        # (G panel) drops more bodies, the encyclopedia's retinue button
        # conscripts them, the ARMY tab musters battalions, C arms combat.
        export FISTWORLD_LAB_SCENARIO="${FISTWORLD_LAB_SCENARIO:-skirmish}"
        export FISTWORLD_LAB_WARP="${FISTWORLD_LAB_WARP:-1}"
        export FISTWORLD_LAB_FOUNDERS="${FISTWORLD_LAB_FOUNDERS:-8}"
        export FISTWORLD_DEV=1
        export FISTWORLD_AUTOSPAWN_HERO="${FISTWORLD_AUTOSPAWN_HERO:-1}"
        export FISTFORCE_START_FOCUS="${FISTFORCE_START_FOCUS:--20,-40}"
        export FISTFORCE_START_ZOOM="${FISTFORCE_START_ZOOM:-110}"
    elif [[ "$MODE" == "economyworld" ]]; then
        export FISTWORLD_LAB_SCENARIO="${FISTWORLD_LAB_SCENARIO:-economy-soak}"
        export FISTWORLD_LAB_WARP="${FISTWORLD_LAB_WARP:-10}"
        export FISTFORCE_START_FOCUS="${FISTFORCE_START_FOCUS:--95,-120}"
        export FISTFORCE_START_ZOOM="${FISTFORCE_START_ZOOM:-720}"
        export FISTFORCE_SERVER_PERF="${FISTFORCE_SERVER_PERF:-1}"
        export FISTFORCE_CLIENT_PERF="${FISTFORCE_CLIENT_PERF:-1}"
    elif [[ "$MODE" == "merchantworld" ]]; then
        # Lab Meadow remains an ordinary growing destination. Its zero-resident
        # sister is a controlled Village market with a bounded Treasury shelf
        # of 192 Bread at 0.10 coin each day. Meadow companies must discover,
        # finance and physically operate the profitable import route with their
        # own Storage Hall and Company Porter.
        export FISTWORLD_LAB_SCENARIO="${FISTWORLD_LAB_SCENARIO:-merchant-beacon}"
        export FISTWORLD_LAB_FOUNDERS="${FISTWORLD_LAB_FOUNDERS:-12}"
        export FISTWORLD_LAB_WARP="${FISTWORLD_LAB_WARP:-10}"
        export FISTFORCE_START_FOCUS="${FISTFORCE_START_FOCUS:--40,52}"
        export FISTFORCE_START_ZOOM="${FISTFORCE_START_ZOOM:-650}"
        export FISTFORCE_SERVER_PERF="${FISTFORCE_SERVER_PERF:-1}"
        export FISTFORCE_CLIENT_PERF="${FISTFORCE_CLIENT_PERF:-1}"
    elif [[ "$MODE" == "tradeworld" ]]; then
        # Two otherwise ordinary autonomous Village controls begin with twelve
        # founders, then each grows smoothly to 35 residents. The inland Meadow
        # and Stonefield share a proved overland corridor, but neither receives
        # free goods, a warehouse, a porter or a route.
        export FISTWORLD_LAB_SCENARIO="${FISTWORLD_LAB_SCENARIO:-trade-comparison}"
        export FISTWORLD_LAB_FOUNDERS="${FISTWORLD_LAB_FOUNDERS:-12}"
        export FISTWORLD_LAB_WARP="${FISTWORLD_LAB_WARP:-10}"
        export FISTFORCE_START_FOCUS="${FISTFORCE_START_FOCUS:--249,161}"
        export FISTFORCE_START_ZOOM="${FISTFORCE_START_ZOOM:-650}"
        export FISTFORCE_SERVER_PERF="${FISTFORCE_SERVER_PERF:-1}"
        export FISTFORCE_CLIENT_PERF="${FISTFORCE_CLIENT_PERF:-1}"
    elif [[ "$MODE" == "stoneworld" ]]; then
        # Two ordinary autonomous settlements: fertile Lab Meadow and a
        # separated Stone-rich control. Both remain in frame so quarry permits,
        # physical extraction and Hall material staging can be watched together.
        export FISTWORLD_LAB_SCENARIO="${FISTWORLD_LAB_SCENARIO:-stone-comparison}"
        export FISTWORLD_LAB_WARP="${FISTWORLD_LAB_WARP:-1}"
        export FISTFORCE_START_FOCUS="${FISTFORCE_START_FOCUS:--139,-28}"
        export FISTFORCE_START_ZOOM="${FISTFORCE_START_ZOOM:-650}"
    elif [[ "$MODE" == "stressworld" || "$MODE" == "denseworld" ]]; then
        export FISTWORLD_LAB_WARP="${FISTWORLD_LAB_WARP:-10}"
        export FISTWORLD_LAB_DAY_TWO_ARRIVALS="${FISTWORLD_LAB_DAY_TWO_ARRIVALS:-0}"
        if [[ "$MODE" == "denseworld" ]]; then
            # One 1,000-person settlement: all bodies remain replicated and
            # visible. Neighbourhood views use at most 160 full rigs; the wide
            # opening view uses the continuously moving crowd representation.
            export FISTWORLD_LAB_SCENARIO="${FISTWORLD_LAB_SCENARIO:-dense-stress}"
            export FISTFORCE_START_FOCUS="${FISTFORCE_START_FOCUS:-112,-158}"
            export FISTFORCE_START_ZOOM="${FISTFORCE_START_ZOOM:-520}"
        else
            # Three settlements and 600 founders on the compact map. The wide
            # opening camera keeps all three regions tactical.
            export FISTWORLD_LAB_SCENARIO="${FISTWORLD_LAB_SCENARIO:-triple-stress}"
            export FISTFORCE_START_FOCUS="${FISTFORCE_START_FOCUS:--95,-120}"
            export FISTFORCE_START_ZOOM="${FISTFORCE_START_ZOOM:-720}"
        fi
        export FISTFORCE_SERVER_PERF="${FISTFORCE_SERVER_PERF:-1}"
        export FISTFORCE_CLIENT_PERF="${FISTFORCE_CLIENT_PERF:-1}"
        # Six hundred actor setup/queue INFO lines can make terminal rendering
        # the bottleneck being measured. Preserve every byte in the two log
        # files, but keep the stress terminal quiet unless explicitly asked.
        STREAM_VILLAGE_LOGS="${FISTWORLD_STREAM_LOGS:-0}"
    else
        # The default growing town has room inland. The original coastal
        # regression fixture remains available with FISTWORLD_LAB_SCENARIO=secure.
        export FISTWORLD_LAB_SCENARIO="${FISTWORLD_LAB_SCENARIO:-town-growth}"
        export FISTWORLD_LAB_WARP="${FISTWORLD_LAB_WARP:-1}"
        if [[ "$FISTWORLD_LAB_SCENARIO" == "town-growth" ]]; then
            export FISTWORLD_TOWN_SEED="${FISTWORLD_TOWN_SEED:-23}"
            export FISTWORLD_TOWN_PROFILE="${FISTWORLD_TOWN_PROFILE:-steady}"
            # Matches the validated TOWN_GROWTH_ANCHOR in village_lab_scenario/town_growth.rs.
            export FISTFORCE_START_FOCUS="${FISTFORCE_START_FOCUS:--100,120}"
        else
            export FISTFORCE_START_FOCUS="${FISTFORCE_START_FOCUS:-112,-158}"
        fi
        export FISTFORCE_START_ZOOM="${FISTFORCE_START_ZOOM:-190}"
    fi
    export FISTWORLD_VILLAGE_TRACE="${FISTWORLD_VILLAGE_TRACE:-1}"
    export RUST_LOG="${FISTWORLD_TESTWORLD_RUST_LOG:-info}"
    export FISTFORCE_AUTOCONNECT="${FISTFORCE_AUTOCONNECT:-LabObserver}"
    CAPTURE_VILLAGE_LOGS=1
    if [[ "$MODE" == "regionalworld" || "$MODE" == "economyworld" || "$MODE" == "stressworld" || "$MODE" == "denseworld" ]]; then
        VILLAGE_LOG_DIR="${FISTWORLD_RUN_LOG_DIR:-$(pwd)/logs/${MODE}-$(date +%Y%m%d-%H%M%S)}"
    else
        VILLAGE_LOG_DIR="${FISTWORLD_RUN_LOG_DIR:-$(pwd)/logs/${MODE}-$(date +%Y%m%d-%H%M%S)}"
    fi
    mkdir -p "$VILLAGE_LOG_DIR"
fi

# A dense, uncurated reproduction on the ordinary generated world. Unlike the
# compact Village Lab this mirrors a god-mode founding: empty store, normal
# policy, and villagers who must gather their own first timber. Both processes
# are tee'd so a visual playtest always leaves evidence behind.
if [[ "$MODE" == "realworld" || "$MODE" == "reallab" ]]; then
    export CITYSIM_MAP_ID="big_world"
    export FISTWORLD_REALWORLD_LAB_RUNTIME="${FISTWORLD_REALWORLD_LAB_RUNTIME:-1}"
    export FISTWORLD_REALWORLD_VILLAGERS="${FISTWORLD_REALWORLD_VILLAGERS:-32}"
    export FISTWORLD_REALWORLD_AT="${FISTWORLD_REALWORLD_AT:--346,306}"
    export FISTWORLD_LAB_WARP="${FISTWORLD_LAB_WARP:-1}"
    export FISTWORLD_VILLAGE_TRACE="${FISTWORLD_VILLAGE_TRACE:-1}"
    export FISTFORCE_SERVER_PERF="${FISTFORCE_SERVER_PERF:-1}"
    export FISTFORCE_CLIENT_PERF="${FISTFORCE_CLIENT_PERF:-1}"
    export FISTFORCE_START_FOCUS="${FISTFORCE_START_FOCUS:--346,306}"
    export FISTFORCE_START_ZOOM="${FISTFORCE_START_ZOOM:-240}"
    export FISTFORCE_AUTOCONNECT="${FISTFORCE_AUTOCONNECT:-RealworldObserver}"
    # Do not inherit a global warn-only RUST_LOG: the whole point of this mode
    # is to leave a complete village record. Use the dedicated override when a
    # narrower capture is intentional.
    # INFO already records permits, deliveries, construction, work, roads,
    # diagnostics and perf. Per-door DEBUG transitions become their own source
    # of terminal/tee lag at 100x, so opt into them with the dedicated override.
    export RUST_LOG="${FISTWORLD_REALWORLD_RUST_LOG:-info}"
    CAPTURE_VILLAGE_LOGS=1
    VILLAGE_LOG_DIR="${FISTWORLD_RUN_LOG_DIR:-$(pwd)/logs/realworld-$(date +%Y%m%d-%H%M%S)}"
    mkdir -p "$VILLAGE_LOG_DIR"
fi

# `dev` is the one profile cargo names with a flag rather than a value.
if [[ "$PROFILE" == "dev" ]]; then
    CARGO_PROFILE=()
    TARGET_DIR="debug"
else
    CARGO_PROFILE=(--profile "$PROFILE")
    TARGET_DIR="$PROFILE"
fi

# Which map the server and client load; big_world is the generated round world.
export CITYSIM_MAP_ID="${CITYSIM_MAP_ID:-big_world}"

# Local servers run with god commands enabled; production config (fly.toml) never
# sets this, and an explicitly exported value wins.
export FISTWORLD_DEV="${FISTWORLD_DEV:-1}"

SERVER_PID=""
CLIENT1_PID=""
CLIENT2_PID=""
STARTED_SERVER=0
CLEANED_UP=0

# Get Windows path for WSL
get_windows_path() {
    wslpath -w "$1"
}

# Colors for output
GREEN='\033[0;32m'
BLUE='\033[0;34m'
YELLOW='\033[0;33m'
NC='\033[0m' # No Color

# Kill any existing server processes to avoid "Address already in use"
cleanup_server() {
    # Any profile's server: the profile can change between runs, so matching one
    # target directory would leave a stale server holding the port.
    pkill -f "target/[a-z-]*/server" 2>/dev/null || true
    pkill -f "cargo run .* -p server" 2>/dev/null || true
    sleep 0.5
}

# Bevy/Lightyear can take longer than an ordinary shell process to leave its
# network loop after SIGTERM. Never let closing a rendered lab strand this
# launcher in `wait` forever: allow a short graceful window, then reap only the
# exact child PID that this script started.
stop_spawned_process() {
    local pid="$1"
    if [[ -z "$pid" ]] || ! kill -0 "$pid" 2>/dev/null; then
        return
    fi
    kill "$pid" 2>/dev/null || true
    local attempt
    for attempt in {1..20}; do
        if ! kill -0 "$pid" 2>/dev/null; then
            wait "$pid" 2>/dev/null || true
            return
        fi
        sleep 0.1
    done
    kill -KILL "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
}

# Do not open a client against a server that is still compiling, generating its
# map, or has already crashed. A fixed sleep made startup timing-dependent and
# turned a server panic into ten seconds of blue water followed by the join
# screen. The server always binds the shared protocol's UDP port 5000 once its
# Bevy startup schedule has initialized successfully.
wait_for_local_server() {
    local attempt
    local max_attempts=1800 # 15 minutes at 0.5s; enough for a clean release build.
    echo -e "${YELLOW}Waiting for server readiness on UDP 5000...${NC}"
    for ((attempt = 1; attempt <= max_attempts; attempt++)); do
        if ! kill -0 "$SERVER_PID" 2>/dev/null; then
            wait "$SERVER_PID" 2>/dev/null || true
            echo -e "${YELLOW}Server exited before opening UDP 5000; client was not started.${NC}" >&2
            return 1
        fi

        if command -v lsof >/dev/null 2>&1; then
            if lsof -nP -iUDP:5000 -t 2>/dev/null | grep -q .; then
                echo -e "${GREEN}Server is ready.${NC}"
                return 0
            fi
        elif command -v ss >/dev/null 2>&1; then
            if ss -H -uln 2>/dev/null | grep -Eq '(^|[[:space:]])[^[:space:]]*:5000([[:space:]]|$)'; then
                echo -e "${GREEN}Server is ready.${NC}"
                return 0
            fi
        else
            # Very small environments may provide neither socket inspector.
            # Still guard against an immediate crash, then preserve the old
            # behaviour with a clearly bounded fallback.
            if ((attempt >= 4)); then
                echo -e "${YELLOW}No lsof/ss available; continuing after server survived two seconds.${NC}"
                return 0
            fi
        fi
        sleep 0.5
    done

    echo -e "${YELLOW}Server did not become ready on UDP 5000; client was not started.${NC}" >&2
    return 1
}

cleanup_all() {
    if [[ "$CLEANED_UP" -eq 1 ]]; then
        return
    fi
    CLEANED_UP=1

    if [[ -n "$CLIENT1_PID" ]]; then
        stop_spawned_process "$CLIENT1_PID"
    fi
    if [[ -n "$CLIENT2_PID" ]]; then
        stop_spawned_process "$CLIENT2_PID"
    fi
    if [[ -n "$SERVER_PID" ]]; then
        stop_spawned_process "$SERVER_PID"
    fi

    if [[ "$STARTED_SERVER" -eq 1 ]]; then
        cleanup_server
    fi
}

on_interrupt() {
    echo -e "\n${YELLOW}Interrupted. Stopping spawned processes...${NC}"
    exit 130
}

trap cleanup_all EXIT
trap on_interrupt INT TERM

echo -e "${YELLOW}Build profile: ${PROFILE}${NC}"

case $MODE in
    server)
        cleanup_server
        echo -e "${GREEN}Starting server...${NC}"
        STARTED_SERVER=1
        cargo run "${CARGO_PROFILE[@]+"${CARGO_PROFILE[@]}"}" -p server
        ;;
    client)
        echo -e "${BLUE}Starting client...${NC}"
        cargo run "${CARGO_PROFILE[@]+"${CARGO_PROFILE[@]}"}" -p client
        ;;
    battle5v5|archerworld)
        cd "$(dirname "${BASH_SOURCE[0]}")"
        # This fixture needs a fresh server. Do not terminate an unrelated
        # running game or connect its client to the wrong world.
        if { command -v lsof >/dev/null 2>&1 && lsof -nP -iUDP:5000 -t 2>/dev/null | grep -q .; } ||
           { command -v ss >/dev/null 2>&1 && ss -H -uln 2>/dev/null | grep -Eq '(^|[[:space:]])[^[:space:]]*:5000([[:space:]]|$)'; }; then
            echo "UDP 5000 is already in use. Stop the current game server, then rerun ./run.sh $MODE." >&2
            exit 1
        fi

        export CITYSIM_MAP_ID=battle_lab FISTWORLD_DEV=1
        export FISTWORLD_VILLAGE_LAB_RUNTIME=1 FISTWORLD_LAB_SCENARIO=skirmish
        export FISTWORLD_LAB_WARP="${FISTWORLD_LAB_WARP:-1}"
        export FISTFORCE_AUTOCONNECT=battlelab FISTWORLD_AUTOSPAWN_HERO=1
        export FISTWORLD_AUTOSPAWN_AT=-32,-72 FISTFORCE_COMBAT_MODE=1
        export FISTFORCE_START_FOCUS="${FISTFORCE_START_FOCUS:--20,-36}"
        export FISTFORCE_START_ZOOM="${FISTFORCE_START_ZOOM:-125}"
        export FISTFORCE_START_YAW="${FISTFORCE_START_YAW:-0}"
        export BEVY_ASSET_ROOT="$PWD/client/assets"
        export RUST_LOG="${RUST_LOG:-info}"
        BATTLE_LOG_DIR="${FISTWORLD_RUN_LOG_DIR:-$PWD/logs/$MODE-$(date +%Y%m%d-%H%M%S)}"
        mkdir -p "$BATTLE_LOG_DIR"
        BATTLE_SCENARIO="battle-5v5"
        if [[ "$MODE" == "archerworld" ]]; then
            BATTLE_SCENARIO="battle-mixed-archers"
            echo -e "${YELLOW}Two infantry battalions and one archer battalion versus three enemy battalions.${NC}"
            echo -e "${YELLOW}Right-click an enemy to attack; Army management controls equipment and firing policy.${NC}"
        else
            echo -e "${YELLOW}5 vs 5 battalions: 250 soldiers per side, manual control.${NC}"
        fi
        echo -e "${YELLOW}Logs: $BATTLE_LOG_DIR${NC}"

        # Build the pair before starting either process. Launch the binaries
        # directly so the existing EXIT/interrupt trap owns their exact PIDs.
        cargo build "${CARGO_PROFILE[@]+"${CARGO_PROFILE[@]}"}" -p server -p client
        env FISTWORLD_ARMY_SCENARIO="$PWD/capture/scenarios/$BATTLE_SCENARIO.ron" \
            "./target/$TARGET_DIR/server" >"$BATTLE_LOG_DIR/server.log" 2>&1 &
        SERVER_PID=$!
        # STARTED_SERVER stays zero: cleanup_all reaps these exact children,
        # without using the older modes' broad cleanup_server fallback.
        if ! wait_for_local_server; then
            tail -n 30 "$BATTLE_LOG_DIR/server.log" >&2
            exit 1
        fi
        echo -e "${BLUE}Opening battle. Drag-select your troops and right-click an enemy to attack.${NC}"
        # Only the server receives the fixture. Giving it to the client would
        # activate the automated attack/capture/exit driver and take control.
        env -u FISTWORLD_ARMY_SCENARIO "./target/$TARGET_DIR/client" \
            >"$BATTLE_LOG_DIR/client.log" 2>&1 &
        CLIENT1_PID=$!
        if wait "$CLIENT1_PID"; then
            CLIENT1_PID=""
        else
            client_status=$?
            CLIENT1_PID=""
            tail -n 30 "$BATTLE_LOG_DIR/client.log" >&2
            exit "$client_status"
        fi
        echo -e "${GREEN}Client closed. Stopping battle server...${NC}"
        ;;
    both|testworld|testlab|uxworld|uxstressworld|regionalworld|stoneworld|tradeworld|merchantworld|economyworld|stressworld|denseworld|battleworld|realworld|reallab)
        cleanup_server
        if [[ "$MODE" == "testworld" || "$MODE" == "testlab" || "$MODE" == "uxworld" || "$MODE" == "uxstressworld" || "$MODE" == "regionalworld" || "$MODE" == "stoneworld" || "$MODE" == "tradeworld" || "$MODE" == "merchantworld" || "$MODE" == "economyworld" || "$MODE" == "stressworld" || "$MODE" == "denseworld" || "$MODE" == "battleworld" ]]; then
            echo -e "${YELLOW}Village Lab: ${FISTWORLD_LAB_SCENARIO}, map ${CITYSIM_MAP_ID}, starting at ${FISTWORLD_LAB_WARP}x (HUD: pause / 1x / 10x / 25x / 100x)${NC}"
            echo -e "${YELLOW}Logs: ${VILLAGE_LOG_DIR}${NC}"
        fi
        if [[ "$MODE" == "realworld" || "$MODE" == "reallab" ]]; then
            echo -e "${YELLOW}Realworld Village Lab: ${FISTWORLD_REALWORLD_VILLAGERS} villagers at ${FISTWORLD_REALWORLD_AT}, starting at ${FISTWORLD_LAB_WARP}x${NC}"
            echo -e "${YELLOW}Logs: ${VILLAGE_LOG_DIR}${NC}"
        fi
        echo -e "${GREEN}Starting server in background...${NC}"
        if [[ "$CAPTURE_VILLAGE_LOGS" -eq 1 ]]; then
            if [[ "$STREAM_VILLAGE_LOGS" -eq 1 ]]; then
                cargo run "${CARGO_PROFILE[@]+"${CARGO_PROFILE[@]}"}" -p server 2>&1 | tee "$VILLAGE_LOG_DIR/server.log" &
            else
                cargo run "${CARGO_PROFILE[@]+"${CARGO_PROFILE[@]}"}" -p server >"$VILLAGE_LOG_DIR/server.log" 2>&1 &
            fi
        else
            cargo run "${CARGO_PROFILE[@]+"${CARGO_PROFILE[@]}"}" -p server &
        fi
        SERVER_PID=$!
        STARTED_SERVER=1
        
        wait_for_local_server
        
        echo -e "${BLUE}Starting client...${NC}"
        if [[ "$CAPTURE_VILLAGE_LOGS" -eq 1 ]]; then
            if [[ "$STREAM_VILLAGE_LOGS" -eq 1 ]]; then
                cargo run "${CARGO_PROFILE[@]+"${CARGO_PROFILE[@]}"}" -p client 2>&1 | tee "$VILLAGE_LOG_DIR/client.log"
            else
                cargo run "${CARGO_PROFILE[@]+"${CARGO_PROFILE[@]}"}" -p client >"$VILLAGE_LOG_DIR/client.log" 2>&1
            fi
        else
            cargo run "${CARGO_PROFILE[@]+"${CARGO_PROFILE[@]}"}" -p client
        fi
        
        # When client exits, kill the server
        echo -e "${GREEN}Client closed. Stopping server...${NC}"
        ;;
    multi)
        cleanup_server
        echo -e "${GREEN}Starting server in background...${NC}"
        cargo run "${CARGO_PROFILE[@]+"${CARGO_PROFILE[@]}"}" -p server &
        SERVER_PID=$!
        STARTED_SERVER=1
        
        # Wait for server to start
        wait_for_local_server
        
        echo -e "${BLUE}Starting client 1...${NC}"
        cargo run "${CARGO_PROFILE[@]+"${CARGO_PROFILE[@]}"}" -p client &
        CLIENT1_PID=$!
        
        sleep 1
        
        echo -e "${YELLOW}Starting client 2...${NC}"
        cargo run "${CARGO_PROFILE[@]+"${CARGO_PROFILE[@]}"}" -p client &
        CLIENT2_PID=$!
        
        echo -e "${GREEN}Server and 2 clients running. Press Enter to stop all...${NC}"
        read
        
        echo -e "${GREEN}Stopping all processes...${NC}"
        ;;
    windows|win)
        cleanup_server
        echo -e "${GREEN}Building Windows client...${NC}"
        cargo build "${CARGO_PROFILE[@]+"${CARGO_PROFILE[@]}"}" -p client --target x86_64-pc-windows-gnu

        # Copy assets next to the exe so Windows can find them
        WIN_TARGET="target/x86_64-pc-windows-gnu/$TARGET_DIR"
        echo -e "${GREEN}Syncing assets...${NC}"
        rsync -a --delete client/assets/ "$WIN_TARGET/assets/"

        echo -e "${GREEN}Starting server in background...${NC}"
        cargo run "${CARGO_PROFILE[@]+"${CARGO_PROFILE[@]}"}" -p server &
        SERVER_PID=$!
        STARTED_SERVER=1

        wait_for_local_server

        echo -e "${BLUE}Launching Windows client (GPU accelerated)...${NC}"
        WIN_EXE=$(get_windows_path "$WIN_TARGET/client.exe")
        powershell.exe -Command "Start-Process -FilePath '$WIN_EXE' -WorkingDirectory '$(get_windows_path "$WIN_TARGET")' -Wait"

        echo -e "${GREEN}Client closed. Stopping server...${NC}"
        ;;
    *)
        echo "Usage: ./run.sh [server|client|both|testworld|uxworld|uxstressworld|regionalworld|stoneworld|tradeworld|merchantworld|economyworld|stressworld|denseworld|battleworld|battle5v5|archerworld|realworld|multi|windows] [--release|--dev]"
        echo "  server  - Start only the server"
        echo "  client  - Start only the client"
        echo "  both    - Start server then client (default)"
        echo "  testworld - Watch one deterministic logged Village Lab settlement (starts at 1x)"
        echo "  uxworld - Open a mature logged 500-resident City for permit/company UX testing (1x)"
        echo "  uxstressworld - Grow that City from 500 toward 1,000 with a day-2 shock and stuck-actor evidence (10x)"
        echo "  regionalworld - Watch four contrasting villages grow on the larger regional lab (10x)"
        echo "  stoneworld - Watch Meadow and Stone-rich settlements develop together (starts at 1x)"
        echo "  tradeworld - Watch two villages grow to 35 and create a physical Stone import route (10x)"
        echo "  merchantworld - Watch NPC firms discover a controlled cheap-Bread trade opportunity (10x)"
        echo "  economyworld - Watch the three-village 50-day economy schedule (starts at 10x)"
        echo "  stressworld - Watch three logged 200-person villages together (starts at 10x)"
        echo "  denseworld - Watch one logged 1,000-person village at 10x"
        echo "  battleworld - Open battlefield sandbox for recruiting troops and making battalions"
        echo "  archerworld - Mixed infantry and archers with an enemy countercharge"
        echo "  battle5v5 - Build and open a manual battle with five 50-person battalions per side"
        echo "  realworld - Watch a logged 32-villager stress village on big_world"
        echo "  multi   - Start server + 2 clients for multiplayer testing"
        echo "  windows - Build & run Windows client with GPU (for WSL2)"
        echo
        echo "Profiles: default=playtest (fast rebuilds, release-grade speed)"
        echo "          --dev     fastest rebuilds, slightly slower runtime"
        echo "          --release true shipping build; slow to rebuild"
        exit 1
        ;;
esac
