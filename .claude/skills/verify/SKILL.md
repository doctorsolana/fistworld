---
name: verify
description: Build and drive the client+server to verify changes at runtime (launch recipe, env flags, log locations).
---

# Verifying changes at runtime

Build (dev profile is fine and much faster than release):

```bash
cargo build -p server -p client
```

Launch headless-driveable (no clicking needed):

```bash
# Server (world + persistence sim). Kill any stale one FIRST and wait for the UDP
# port to free -- see the gotcha below, it costs real time to rediscover.
pkill -9 -f "target/debug/server"; pkill -9 -f "target/debug/client"
until ! lsof -nP -iUDP 2>/dev/null | grep -q ":5000"; do sleep 1; done

./target/debug/server > /tmp/server.log 2>&1 &
sleep 6   # let it bind before connecting

# Client. BEVY_ASSET_ROOT is REQUIRED when running the binary directly
# (otherwise Bevy resolves assets against target/debug/ and every asset 404s).
# FISTFORCE_AUTOCONNECT skips the main menu AND auto-submits the player name.
# Use a FRESH per-run profile name: profiles persist in server_data/players/
# (saved camera focus + progression). Reconnecting under a name that is still
# considered online is rejected with `Name rejected: AlreadyOnline` until the
# server times the old session out.
BEVY_ASSET_ROOT=$PWD/client \
  FISTFORCE_AUTOCONNECT=Test$(date +%H%M%S) \
  FISTFORCE_CLIENT_PERF=1 FISTFORCE_CLIENT_PERF_INTERVAL_SECS=10 \
  ./target/debug/client > /tmp/client.log 2>&1 &
```

Success signals in `/tmp/client.log`:
- `FISTFORCE_AUTOCONNECT: skipping main menu` then `Name accepted!`
- `Spawned client world visuals` (reached Playing)
- `Scene render target resized to WxH (window ... @ scale ...)` (render-scale path alive)
- `ClientPerf frame_ms_p50=...` lines every N seconds (frame stats; vsync caps at ~16.6ms)

Failure signals: `panicked`, `ERROR bevy_asset` (missing/broken asset), no `Name accepted` within ~15s (server not up / connect failure).

Gotchas:
- The client goes borderless-fullscreen on connect — the window takes over the primary display; the user may close it via ESC → EXIT GAME, which logs `Exiting game...` (not a crash).
- **Kill by binary path: `pkill -9 -f "target/debug/server"`.** `pkill -f "citysim/target"` matches
  NOTHING (the processes run as `./target/debug/server`), silently leaves a stale server holding
  UDP :5000, and the next run then fails with `Address already in use` on the server plus
  `the message protocol doesn't match` on the client -- which looks exactly like a protocol
  regression you just introduced. Always wait for the port to free before relaunching.
- Every protocol change needs BOTH binaries rebuilt and restarted; a stale one mis-routes messages.
- No mouse/keyboard automation is available on this Mac (no cliclick/pyobjc); anything beyond boot-to-ingame needs the user to playtest.

## Seeing the game (not just its logs)

Boot logs prove the game *started*, never that it *renders correctly*. For anything visual use
the capture binary — it runs the real renderer without a server and writes PNGs you can read:

```bash
cargo run -p client --bin capture -- --at <x>,<z> --preset survey --out /tmp/shots
```

Then open the PNGs. Notes:
- `--time 0.5` is noon. Getting time-of-day wrong photographs the world at night and looks
  exactly like a broken renderer.
- Props stream far slower than terrain and are position-dependent — check the logged
  `N terrain chunks loaded` line, and aim at somewhere content actually exists
  (`player_spawn` in `map.ron`) rather than the origin.
