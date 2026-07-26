# Top-Down Tactics (working title)

A multiplayer top-down unit-tactics game built with **Rust** and **Bevy 0.18** — many units,
formations, huge maps.

> **This repo was a first-person shooter (FistForce) until July 2026.** It is mid-conversion:
> the FPS layers have been stripped out and the world/terrain/editor foundation kept. The unit
> simulation is not written yet.
>
> - The complete FPS game is preserved at git tag **`citysim-final`**.
> - Conversion status and remaining work: [`STRIP_PLAN.md`](STRIP_PLAN.md).
> - Full analysis behind it: [`docs/strip/`](docs/strip/).

---

## What works today

- 🗺️ **Huge authored maps** — chunk-streamed terrain, resizable up to ~2.8 km²
- 🎨 **Full map editor** — sculpt, texture paint, scatter brush, roads/plots, undo, live minimap
- 🌍 **Procedural world generation** — Island / Mainland / "Great Open World" (A\*-pathed roads,
  mountains, rivers, beaches, archipelagos, a village)
- 🌊 **Water** — shore foam, caustics, wet sand, depth shading, wind-driven waves
- 🌲 **Props & foliage** — wind sway, LOD, baked collider library
- 🌅 **Day/night cycle** with atmospheric scattering and clouds
- 🎥 **Top-down commander camera** — WASD pan, RMB orbit, wheel zoom, cursor→terrain picking
- 🔌 **Multiplayer plumbing** — lightyear connection, replication, profile persistence

## Not built yet

Units, selection, orders, formations, combat, and the AI/economy that make it a game.
See the netcode decision still open at the bottom of [`STRIP_PLAN.md`](STRIP_PLAN.md).

---

## Workspace Layout

| Crate | Description |
|-------|-------------|
| `client/` | Bevy app: rendering, camera, UI, terrain/prop streaming |
| `server/` | Headless authoritative server: world tick, persistence, collider streaming, navigation |
| `shared/` | Deterministic terrain/props, protocol, components, map schema |
| `editor/` | Map editor (terrain sculpt, painting, prop placement, worldgen) |
| `tools/collider_baker/` | Offline tool to bake convex-hull colliders from GLTF meshes |
| `tools/terrain_ktx_builder/` | Offline terrain texture-array packer |

Assets live in `client/assets/` (models, audio, `colliders.bin`).

Contributor workflow and architecture guardrails: [`CONTRIBUTING.md`](CONTRIBUTING.md).
Character `.glb` animation indices: [`docs/character-animations.md`](docs/character-animations.md).

---

## Quick Start

```bash
./run.sh              # server in background, then client
./run.sh editor       # map editor
```

Or manually:

```bash
cargo run -p server --release     # terminal 1
cargo run -p client --release     # terminal 2
cargo run -p editor -- --map city_alpha
```

Useful env flags:

| Flag | Effect |
|------|--------|
| `CITYSIM_MAP_ID=<id>` | Select authored map at startup (client/server/editor) |
| `CITYSIM_TERRAIN_COLLIDER_RADIUS_CHUNKS=<n>` | Radius (in chunks) of streamed terrain colliders |
| `CITYSIM_TERRAIN_COLLIDER_MAX_LOAD_PER_TICK=<n>` | Max collider chunks spawned per fixed tick |
| `CITYSIM_TERRAIN_COLLIDER_RESOLUTION=<n>` | Per-chunk heightfield resolution for colliders |
| `FISTFORCE_AUTOCONNECT=<name>` | Skip main menu + name entry (dev) |
| `FISTFORCE_CLIENT_PERF=1` | Emit rolling `ClientPerf` frame-time lines |
| `FISTFORCE_SERVER_PERF=1` | Emit `ServerPerf` tick/phase lines |

---

## Architecture notes

### World

- **Authored map data**: client, server and editor load the same map definition
  (`shared/src/map/`) and sample the same heightmap.
- **Chunk streaming**: terrain meshes and props load/unload around the commander camera focus.
  This anchor is load-bearing — see the Danger notes in `STRIP_PLAN.md`.
- **Runtime bounds**: active map bounds are authoritative and replicated server → client.

### Server

- **Single physics authority**: one Rapier world for terrain and static world colliders,
  streamed around the commander view. Raycast helpers for line-of-sight live in
  `server/src/collision/raycast.rs` and `server/src/physics/queries.rs`.
- **Navigation groundwork**: `server/src/world/navgrid.rs` keeps a spatial obstacle grid fed
  from authored buildings; `server/src/world/pathfinding.rs` is grid A\* over terrain +
  obstacles. Both are agent-agnostic and awaiting the unit sim. Note that hundreds of units
  moving to a shared goal want a flow field, not per-unit A\*.
- **Persistence**: player profiles are bincode; `PROFILE_VERSION` must be bumped on any
  layout change (bincode is positional and fails silently otherwise).
