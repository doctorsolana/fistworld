#!/bin/bash
# Run script for Fistworld
# Usage: ./run.sh [server|client|both|multi|editor] [--release|--dev]
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

cleanup_all() {
    if [[ "$CLEANED_UP" -eq 1 ]]; then
        return
    fi
    CLEANED_UP=1

    if [[ -n "$CLIENT1_PID" ]]; then
        kill "$CLIENT1_PID" 2>/dev/null || true
        wait "$CLIENT1_PID" 2>/dev/null || true
    fi
    if [[ -n "$CLIENT2_PID" ]]; then
        kill "$CLIENT2_PID" 2>/dev/null || true
        wait "$CLIENT2_PID" 2>/dev/null || true
    fi
    if [[ -n "$SERVER_PID" ]]; then
        kill "$SERVER_PID" 2>/dev/null || true
        wait "$SERVER_PID" 2>/dev/null || true
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
    editor)
        echo -e "${BLUE}Starting map editor...${NC}"
        cargo run "${CARGO_PROFILE[@]+"${CARGO_PROFILE[@]}"}" -p editor -- "${@:2}"
        ;;
    both)
        cleanup_server
        echo -e "${GREEN}Starting server in background...${NC}"
        cargo run "${CARGO_PROFILE[@]+"${CARGO_PROFILE[@]}"}" -p server &
        SERVER_PID=$!
        STARTED_SERVER=1
        
        # Wait for server to start
        sleep 2
        
        echo -e "${BLUE}Starting client...${NC}"
        cargo run "${CARGO_PROFILE[@]+"${CARGO_PROFILE[@]}"}" -p client
        
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
        sleep 2
        
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

        sleep 2

        echo -e "${BLUE}Launching Windows client (GPU accelerated)...${NC}"
        WIN_EXE=$(get_windows_path "$WIN_TARGET/client.exe")
        powershell.exe -Command "Start-Process -FilePath '$WIN_EXE' -WorkingDirectory '$(get_windows_path "$WIN_TARGET")' -Wait"

        echo -e "${GREEN}Client closed. Stopping server...${NC}"
        ;;
    *)
        echo "Usage: ./run.sh [server|client|both|multi|editor|windows] [--release|--dev]"
        echo "  server  - Start only the server"
        echo "  client  - Start only the client"
        echo "  both    - Start server then client (default)"
        echo "  multi   - Start server + 2 clients for multiplayer testing"
        echo "  editor  - Start map editor (pass map via --map <id>)"
        echo "  windows - Build & run Windows client with GPU (for WSL2)"
        echo
        echo "Profiles: default=playtest (fast rebuilds, release-grade speed)"
        echo "          --dev     fastest rebuilds, slightly slower runtime"
        echo "          --release true shipping build; slow to rebuild"
        exit 1
        ;;
esac
