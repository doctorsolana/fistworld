# Fistworld

A persistent-world multiplayer RTS built with **Rust** and **Bevy 0.19** — one seed-generated
round world of continents and biomes, seamless zoom from a single soldier to the whole map,
villages growing into cities, clans contesting the realm. Design: [`docs/WORLD-DESIGN.md`](docs/WORLD-DESIGN.md),
engine architecture: [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md),
build order: [`docs/ROADMAP.md`](docs/ROADMAP.md).

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
cargo run -p editor -- --map big_world
```

### Village simulation lab

Full usage and failure-diagnosis notes: [`docs/VILLAGE-LAB.md`](docs/VILLAGE-LAB.md).

Run the deterministic dual-climate village world at 100x without rendering:

```bash
cargo village-lab
```

It spawns the same fixed-seed, eight-person meadow settlement on every run and
runs 160 simulated minutes through the real collider, navigation, construction,
road, household, food, prosperity and work systems. Set
`FISTWORLD_LAB_SCENARIO=dual` when the second frozen inland control is useful.
It prints five-minute inventory/progress reports and fails with the exact NPC
state if active embodied work makes no progress for ten simulated minutes. Useful
overrides are `FISTWORLD_LAB_MINUTES`, `FISTWORLD_LAB_WARP`, and
`FISTWORLD_LAB_VERBOSE=1`.

To watch the simpler one-village fixture in the real client, already staged on
the same compact seed-3 map:

```bash
./run.sh testworld
```

The client opens over the eight-person Lab Meadow at 1x and connects as
`LabObserver`. Use WASD to pan, the mouse wheel to zoom, right-drag to orbit,
and the HUD buttons to pause or switch between 1x, 10x and 100x. Server and
client evidence is retained under the timestamped `logs/testworld-*` directory
printed by the launcher. The comparative fixture remains available with
`FISTWORLD_LAB_SCENARIO=dual ./run.sh testworld`.

To reproduce a crowded god-mode founding on the ordinary generated world,
with the real renderer and durable diagnostics, run:

```bash
./run.sh realworld
```

This stages 32 villagers at the reported Oakfell test location with an empty
hall store and normal settlement policy. It starts at 1x; use the HUD speed
controls when ready. Every run creates timestamped
`logs/realworld-*/server.log` and `client.log` files. `VillageTrace` summarizes
population, construction, roads, movement, work, inventories and failed routes
every three real seconds, while `ServerPerf` separates navigation time from the
core simulation. `VillageRoutePerf` reports route-cache hits, queue pressure,
survey expansion and per-stage planner cost every ten seconds. Override the fixture with
`FISTWORLD_REALWORLD_VILLAGERS=64`, `FISTWORLD_REALWORLD_AT=x,z`, or
`FISTWORLD_RUN_LOG_DIR=/absolute/path`. Use
`FISTWORLD_REALWORLD_RUST_LOG=...` only when you intentionally want a narrower
log filter.

Press F3 near a settlement for its population and live planning-distance
readout. Press F4 to draw the same planning rings in the world: green housing,
amber workplaces and blue coastal reach. These are preferred search bands,
not a political border.

To inspect the compact map empty and found settlements manually with god mode:

```bash
CITYSIM_MAP_ID=village_lab ./run.sh
```

### Visual capture

Screenshot the real renderer with no server, for verifying how things actually look:

```bash
cargo run -p client --bin capture -- --at -226,-163 --preset survey
cargo run -p client --bin capture -- --at 0,0 --preset daycycle --out /tmp/shots
cargo run -p client --bin capture -- --help
```

Presets: `survey` (near/mid/far/horizon), `orbit` (4 yaws — catches one-sided geometry),
`daycycle` (dawn/noon/dusk), `water` (low angle over water). Compiling proves nothing about
winding order, shaders, foliage orientation or lighting; this does.

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
