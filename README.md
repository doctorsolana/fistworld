# FistForce

A multiplayer 3D sandbox shooter built with **Rust** and **Bevy 0.18**.

## Features

- 🗺️ Fixed authored city map (`city_alpha`) loaded from `map.ron` + heightmap
- 🎯 Server-authoritative shooting with realistic bullet ballistics (drop, travel time)
- 🤖 NPCs with pathfinding AI and hit detection (headshots, body zones)
- 🌅 Dynamic day/night cycle with atmospheric scattering
- 🚗 Driveable vehicles (motorbike)
- 🎮 Client-side prediction with server reconciliation
- 🌲 Baked convex-hull colliders for environment props (trees, rocks)

---

## Workspace Layout

| Crate | Description |
|-------|-------------|
| `client/` | Bevy app with rendering, input, UI, terrain/prop streaming |
| `server/` | Headless authoritative server (physics, AI, hit detection) |
| `shared/` | Deterministic terrain/props, protocol, components, ballistics |
| `tools/collider_baker/` | Offline tool to bake convex-hull colliders from GLTF meshes |

Assets live in `client/assets/` (models, audio, `colliders.bin`).

Contributor workflow and architecture guardrails:
- See `CONTRIBUTING.md`.

## Module Rules

- `mod.rs` files are orchestration surfaces only:
  - declare `mod ...`
  - `pub use ...` re-exports
  - plugin/build wiring
  - tiny guards/constants needed for module wiring
- Large implementation bodies belong in focused submodules.
- Domain ownership is explicit:
  - `client`: feature domains (`weapons`, `audio`, `ui`, `terrain`, `systems`, etc.)
  - `server`: runtime domains (`net`, `player`, `vehicle`, `inventory`, `world`, `combat`, `collision`, `ai`, `persistence`, `telemetry`)
  - `shared`: protocol/data/physics domains with namespaced modules as the default import style
- Compatibility aliases are intentionally avoided; call canonical names directly.

### Post-Rewrite Status

- Shared flat compatibility bridge has been removed.
- Namespaced shared imports are standard across runtime and tools.
- Lean `mod.rs` policy is enforced for high-impact domains and remains the default for new modules.

### Post-Rewrite Performance Baseline (Validated February 14, 2026)

This repository includes a full pass of behavior-preserving performance work across `client`, `server`, and `shared`.

- Client:
  - Added frame-time instrumentation and periodic perf logs (`FISTFORCE_CLIENT_PERF`, `FISTFORCE_CLIENT_PERF_INTERVAL_SECS`).
  - Reduced projectile and remote-weapon hot-path overhead with cached indices and incremental reconciliation.
  - Batched remote audio event handling and moved emitter admission to cached-membership + squared-distance checks.
  - Added send-on-change `PlayerInput` transport with burst resend and heartbeat to reduce idle traffic while keeping controls responsive on unreliable channels.
- Server:
  - Added broadphase indices for bullet/entity checks and reduced message fanout overhead with batched dispatch paths.
  - Moved profile persistence off fixed tick via async save queue; added roster cache for lower IO pressure.
  - Switched building spatial index rebuild gating to ECS change/removal signals (no no-op full scans each tick).
  - Incrementalized collider streaming with cached build-zone chunk lookup and loaded-chunk refresh when building zones change.
  - Improved AI/pathfinding cadence and scratch reuse in hot loops.
- Shared:
  - Optimized terrain mesh generation by caching stencil samples and deriving biome/material data from cached authored values.
  - Optimized `sample_height` via cached sampling scalars in `HeightmapData`.
  - Added map object pre-resolution and per-chunk indexing at map load; prop rotations are precomputed quaternions.
  - Added shared build-zone precompute + chunk indexing helpers used by both client prop filtering and server collider filtering.
  - Packed `PlayerInput` wire format (bitfield + quantized controls/yaw) and protocol update.
  - Removed dead reserve-ammo compatibility path (`EquippedWeapon`) in favor of inventory-driven reload flow only.

Current baseline checks:
- `cargo check --workspace`
- `cargo test -p shared`
- Targeted server collision/index tests for streaming + building index invalidation

---

## Architecture Overview

### Networking (Lightyear)

- **Server-authoritative**: The server owns all gameplay state (positions, health, bullets).
- **Client-side prediction**: The client predicts local player movement; server corrects if needed.
- **Replication**: Components marked with `Replicate` are automatically synced to clients.
- **Messages**: `PlayerInput`, `ShootRequest`, `HitConfirm`, `BulletImpact`, etc.

### Terrain & Props

- **Authored map data**: Client and server load the same map definition (`shared/src/map/`) and sample the same heightmap.
- **Chunk streaming**: Client loads/unloads terrain meshes and authored props based on player/camera position.
- **Runtime bounds**: Active map bounds are authoritative and replicated from server to clients.

### Collisions

- **Ground**: Heightfield lookup via `WorldTerrain::get_height(x, z)` — automatically includes terrain modifications (building flattening).
- **Static props**: Baked convex-hull colliders loaded at server startup; spatial-hash streaming around players.
- **Entities**: Capsule (player/NPC) and OBB (vehicle) vs convex-hull resolution.

### Weapons & Combat

- **Ballistics**: Bullets are physical projectiles with velocity, gravity, drag.
- **Hit detection**: Server raycasts against NPC/player hitboxes (head, chest, limbs).
- **Recoil**: Accumulative recoil for rapid fire; reduced when ADS.

---

## Quick Start

```bash
# Build and run (starts server in background, then client)
./run.sh
```

Or manually:

```bash
# Terminal 1 — server
cargo run -p server --release
# Optional: reduce NPCs for faster local testing
CITYSIM_MAX_NPCS=20 cargo run -p server --release

# Terminal 2 — client
cargo run -p client --release
```

Useful env flags:
- `CITYSIM_MAX_NPCS=<n>`: cap total spawned NPCs (`0` = none).
- `FISTFORCE_SERVER_PERF=0`: disable server phase timing logs.
- `CITYSIM_SERVER_HOTLOG=1`: enable extra hot-loop debug logs on server.
- `FISTFORCE_CLIENT_PERF=1`: enable client frame/perf rolling logs.
- `FISTFORCE_CLIENT_PERF_INTERVAL_SECS=<n>`: client perf log cadence in seconds.
- `FISTFORCE_HIERARCHY_AUDIT=1`: log which scene nodes are triggering parent hierarchy warnings.

---

## Build for macOS (MacBook)

### Release build (fastest)

```bash
cargo build -p client --release
```

### Universal `.app` bundle (Intel + Apple Silicon)

This produces a zip you can copy to another Mac and run by double-clicking:

```bash
# Build both architectures
cargo build -p client --release --target aarch64-apple-darwin
cargo build -p client --release --target x86_64-apple-darwin

# Create a universal .app + zip (outputs dist/client-macos-universal.zip)
rm -rf dist && mkdir -p dist/client.app/Contents/MacOS dist/client.app/Contents/Resources
lipo -create -output dist/client.app/Contents/MacOS/client \
  target/aarch64-apple-darwin/release/client \
  target/x86_64-apple-darwin/release/client
cp -R client/assets dist/client.app/Contents/MacOS/assets

cat > dist/client.app/Contents/Info.plist <<'PLIST'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>3DGame</string>
  <key>CFBundleDisplayName</key><string>3DGame</string>
  <key>CFBundleIdentifier</key><string>com.terninator.3dgame</string>
  <key>CFBundleVersion</key><string>1.0.0</string>
  <key>CFBundleShortVersionString</key><string>1.0.0</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleExecutable</key><string>client</string>
  <key>LSMinimumSystemVersion</key><string>11.0</string>
  <key>NSHighResolutionCapable</key><true/>
</dict>
</plist>
PLIST

chmod +x dist/client.app/Contents/MacOS/client
(cd dist && ditto -c -k --sequesterRsrc --keepParent client.app client-macos-universal.zip)
```

If macOS blocks the app on the other machine: right-click `client.app` → **Open** → **Open** (one-time).

## Collider Baker

Environment props (trees, rocks) use **pre-baked convex-hull colliders** so the server doesn't need to load meshes at runtime.

### When to run

- After adding new collidable props to `client/assets/`
- After editing `client/assets/colliders_manifest.ron`
- After changing a prop's mesh file

### How to run

```bash
cargo run -p collider_baker --release
```

This reads `colliders_manifest.ron`, loads each GLTF, computes a convex hull (with optional trunk-slice for trees), and writes `colliders.bin`.

### Collider Baker v2 (convex decomposition)

Use this when you need better collisions for **concave buildings** (balconies/overhangs).

```bash
cargo run -p collider_baker --bin collider_baker_v2 --release
```

Notes:
- v2 only applies to manifest entries with `mode: ConvexDecomposition`.
- v1 (`collider_baker`) still works and ignores that mode (falls back to convex hulls).
- Accuracy/perf knobs live in `tools/collider_baker/src/bin/collider_baker_v2.rs`.
  v2 auto-adjusts quality based on triangle count; tweak the thresholds there if needed:
  - `resolution`, `concavity`, `max_convex_hulls`, `plane_downsampling`, `convex_hull_downsampling`

### Manifest format (`colliders_manifest.ron`)

```ron
(
    entries: [
        ( kind: "KayKitTree1A", gltf_path: "Assetsfromassetpack/gltf/tree_1a.gltf", mode: ConvexHull, vertex_filter: LowerYPercent(0.35) ),
        ( kind: "KayKitRock1A", gltf_path: "Assetsfromassetpack/gltf/rock_1a.gltf", mode: ConvexHull, vertex_filter: All ),
        ( kind: "building_church", gltf_path: "game_assets/buildings/village/Church.glb#Scene0", mode: ConvexDecomposition, vertex_filter: All ),
        // ...
    ]
)
```

- `vertex_filter: LowerYPercent(0.35)` → only use the bottom 35% of vertices (avoids giant canopy colliders on trees).
- `vertex_filter: All` → use all vertices (rocks, buildings).

---

## Controls

| Key | Action |
|-----|--------|
| WASD | Move |
| Space | Jump |
| Mouse | Look |
| LMB | Shoot |
| RMB | Toggle ADS (Aim Down Sights) |
| R | Reload |
| E | Enter/exit vehicle |
| 1-4 | Switch weapon |
| F3 | Toggle debug overlay |
| Esc | Release cursor / Pause menu |

---

## Debug Overlay (F3)

When enabled, the overlay shows:

- **FPS** (color-coded: green ≥55, yellow ≥30, red <30)
- **Entity count**
- **Loaded terrain chunks**
- **Props** (total and collidable)
- **Collider chunks** (server streaming radius)
- **Render CPU times** (top 5 passes)

Gizmos are drawn for:

- Bullet trajectories (green lines)
- NPC hitboxes (body capsule + head sphere)
- Collidable prop colliders (cyan cylinders)

---

## Tech Stack

| Crate | Purpose |
|-------|---------|
| [Bevy 0.18](https://bevyengine.org/) | Game engine (ECS, rendering, audio) |
| [Lightyear 0.26](https://github.com/cBournhonesque/lightyear) | Networking (replication, prediction) |
| [bevy_rapier3d](https://github.com/dimforge/bevy_rapier) | Convex-hull computation (bake tool only) |
| [noise](https://docs.rs/noise) | Procedural noise utilities (used in visual effects) |
| [ron](https://docs.rs/ron) | Manifest file format |
| [bincode](https://docs.rs/bincode) | Baked collider serialization |

---

## License

Assets from [KayKit](https://kaylousberg.itch.io/) and [Stylized Nature MegaKit](https://quaternius.com/) — see their respective license files in `client/assets/`.
