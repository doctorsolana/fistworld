---
name: verify
description: Build and drive the citysim client+server to verify changes at runtime (launch recipe, env flags, log locations).
---

# Verifying citysim changes at runtime

Build (dev profile is fine and much faster than release):

```bash
cargo build -p server -p client
```

Launch headless-driveable (no clicking needed):

```bash
# Server (FPS shooter sim). CITYSIM_MAX_NPCS=0 by default; set >0 to exercise NPCs.
CITYSIM_MAX_NPCS=8 ./target/debug/server > /tmp/server.log 2>&1 &

# Client. BEVY_ASSET_ROOT is REQUIRED when running the binary directly
# (otherwise Bevy resolves assets against target/debug/ and every asset 404s).
# FISTFORCE_AUTOCONNECT skips the main menu AND auto-submits the player name.
# ALWAYS use a FRESH per-run profile name: profiles persist in
# server_data/players/ (ammo, position, vehicle state), so reusing a name
# hands the user a depleted/ammo-less loadout when they take over the window.
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
- `FISTFORCE_RAIL=1` (set on BOTH client and server) boots the rail-tycoon prototype instead of the shooter; default is the shooter.
- Kill with `pkill -f target/debug/client; pkill -f target/debug/server`.
- No mouse/keyboard automation is available on this Mac (no cliclick/pyobjc); anything beyond boot-to-ingame needs the user to playtest.
