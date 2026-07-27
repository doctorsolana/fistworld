#!/bin/bash
# Run script for the sandbox game
# Usage: ./run.sh [server|client|both|multi|editor]

set -euo pipefail

MODE=${1:-both}

# Which map the server and client load. Defaults to the generated round world;
# override with CITYSIM_MAP_ID=city_alpha ./run.sh for the legacy hand-authored map.
export CITYSIM_MAP_ID="${CITYSIM_MAP_ID:-big_world}"

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
    pkill -f "target.*/release/server" 2>/dev/null || true
    pkill -f "target/release/server" 2>/dev/null || true
    pkill -f "cargo run -p server --release" 2>/dev/null || true
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

case $MODE in
    server)
        cleanup_server
        echo -e "${GREEN}Starting server...${NC}"
        STARTED_SERVER=1
        cargo run -p server --release
        ;;
    client)
        echo -e "${BLUE}Starting client...${NC}"
        cargo run -p client --release
        ;;
    editor)
        echo -e "${BLUE}Starting map editor...${NC}"
        cargo run -p editor --release -- "${@:2}"
        ;;
    both)
        cleanup_server
        echo -e "${GREEN}Starting server in background...${NC}"
        cargo run -p server --release &
        SERVER_PID=$!
        STARTED_SERVER=1
        
        # Wait for server to start
        sleep 2
        
        echo -e "${BLUE}Starting client...${NC}"
        cargo run -p client --release
        
        # When client exits, kill the server
        echo -e "${GREEN}Client closed. Stopping server...${NC}"
        ;;
    multi)
        cleanup_server
        echo -e "${GREEN}Starting server in background...${NC}"
        cargo run -p server --release &
        SERVER_PID=$!
        STARTED_SERVER=1
        
        # Wait for server to start
        sleep 2
        
        echo -e "${BLUE}Starting client 1...${NC}"
        cargo run -p client --release &
        CLIENT1_PID=$!
        
        sleep 1
        
        echo -e "${YELLOW}Starting client 2...${NC}"
        cargo run -p client --release &
        CLIENT2_PID=$!
        
        echo -e "${GREEN}Server and 2 clients running. Press Enter to stop all...${NC}"
        read
        
        echo -e "${GREEN}Stopping all processes...${NC}"
        ;;
    windows|win)
        cleanup_server
        echo -e "${GREEN}Building Windows client...${NC}"
        cargo build -p client --release --target x86_64-pc-windows-gnu

        # Copy assets next to the exe so Windows can find them
        WIN_TARGET="target/x86_64-pc-windows-gnu/release"
        echo -e "${GREEN}Syncing assets...${NC}"
        rsync -a --delete client/assets/ "$WIN_TARGET/assets/"

        echo -e "${GREEN}Starting server in background...${NC}"
        cargo run -p server --release &
        SERVER_PID=$!
        STARTED_SERVER=1

        sleep 2

        echo -e "${BLUE}Launching Windows client (GPU accelerated)...${NC}"
        WIN_EXE=$(get_windows_path "$WIN_TARGET/client.exe")
        powershell.exe -Command "Start-Process -FilePath '$WIN_EXE' -WorkingDirectory '$(get_windows_path "$WIN_TARGET")' -Wait"

        echo -e "${GREEN}Client closed. Stopping server...${NC}"
        ;;
    *)
        echo "Usage: ./run.sh [server|client|both|multi|editor|windows]"
        echo "  server  - Start only the server"
        echo "  client  - Start only the client"
        echo "  both    - Start server then client (default)"
        echo "  multi   - Start server + 2 clients for multiplayer testing"
        echo "  editor  - Start map editor (pass map via --map <id>)"
        echo "  windows - Build & run Windows client with GPU (for WSL2)"
        exit 1
        ;;
esac
