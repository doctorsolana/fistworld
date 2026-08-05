# Fistworld

A persistent-world multiplayer RTS built with **Rust** and **Bevy 0.19**. One
seed-generated world contains autonomous settlements whose named residents migrate, form
households, take jobs, produce and trade physical goods, build roads and grow their town.
The long-term game expands that foundation into player businesses, caravans, clans,
territory and war.

The old first-person game is preserved at git tag `citysim-final`; the live workspace is
the RTS/living-world codebase. Start with [WORLD-DESIGN.md](docs/WORLD-DESIGN.md) for the
game, [ARCHITECTURE.md](docs/ARCHITECTURE.md) for its technical boundaries, and
[ROADMAP.md](docs/ROADMAP.md) for implemented and future work.

## Current playable foundation

- A chunk-streamed generated world with biomes, rivers, coastlines, water, foliage,
  atmospheric day/night lighting and a seamless commander camera.
- Server-authoritative multiplayer, region interest management, player profiles and
  stable `PersonId`, `SettlementId` and `BuildingId` relationships.
- God-mode settlement founding and villager spawning. Unaffiliated people choose a
  settlement, migrate to its hall and become residents.
- Autonomous housing, employment, permits and geography-aware construction. Seeded
  planning grammars create organic lanes, radial commons, grids, avenues or clustered
  neighbourhoods without moving completed buildings.
- Physical Wood supply and construction; Farmsteads with two wheat fields, Fisherman's
  Huts with piers, Lumberjack Huts with reachable-tree checks, and repeated housing/food
  construction driven by measured shortages.
- Continuous visible work loops for farming, fishing and chopping, bounded personal and
  building inventories, one early Market Porter, adaptive wages, business accounts,
  household food purchases, Poor Relief and stock-sensitive Moot prices.
- Door traversal, designated homes, night routines, occupied-window lighting, ambient
  walking/sitting, character attributes and bounded life histories.
- Builder-made obstacle-safe roads, a shared local travel graph and a paid Road Steward
  who audits and repairs disconnected buildings.
- Inspectable residents, households, buildings, worksites, markets, settlement progress,
  inventories and prosperity in the selection UI and encyclopedia.
- One authoritative simulation clock, the same ordered village schedule in the live game
  and lab, aggregate off-screen village production, and summary/detail replication.

The live tier ladder is **Hamlet → Village → Town → City**. Its current executable
population gates are 4, 12 and 24 residents, combined with sustained food, prosperity,
trade and civic-building requirements. Those values are prototype balance, not final
design.

## Major work still ahead

- Versioned world-state persistence, migrations, backups and hosted durable storage.
- Births, aging, deaths, decline, persistent tree depletion/regrowth and processed foods.
- Player trading, business ownership, carts, caravans and inter-settlement markets.
- Physical palisades, stone walls, gates, guards and patrols.
- Strategic travelling parties and armies with lossless tactical promotion/demotion.
- Retinues, formations, flow fields, combat, clans, political ownership and realm war.

The checked state and dependencies for each item live in [ROADMAP.md](docs/ROADMAP.md).

## Workspace

| Path | Responsibility |
|---|---|
| `client/` | Rendering, commander camera, selection, UI, settlement presentation and streaming |
| `server/` | Authoritative world, people, settlements, economy, roads, navigation, networking and persistence |
| `shared/` | Replicated contracts, stable identities, economy models, terrain/world generation and asset registries |
| `tools/collider_baker/` | Offline convex-hull collider baker for GLTF assets |
| `tools/terrain_ktx_builder/` | Offline terrain texture-array packer |
| `asset_creation/` | Canonical Blender/export/validation pipelines and art contracts |

Runtime assets live in `client/assets/`. The server's domain map is in
[server/README.md](server/README.md), contributor guardrails in
[CONTRIBUTING.md](CONTRIBUTING.md), the current character contract in
[character-animations.md](docs/character-animations.md), and asset naming in
[ASSET_NAMING.md](asset_creation/ASSET_NAMING.md).

## Run the game

```bash
./run.sh                 # server in the background, then the client
./run.sh --dev           # faster compile, slower runtime
./run.sh --release       # shipping/performance measurement profile
```

The default `playtest` profile keeps release-grade optimisation without thin LTO and with
incremental compilation. Use it for normal play and iteration.

### Deterministic Village Lab

The headless integration lab runs the real village systems against a fixed seed:

```bash
cargo village-lab
```

The default `secure` scenario runs 190 simulated minutes at 100x with eight founders and
eight day-2 arrivals. It checks migration, construction supply, work, inventories,
households, food, commerce, roads, doors, tier progress, wealth histories and stalls. The
optional two-climate comparison adds the food-poor frozen control:

```bash
FISTWORLD_LAB_SCENARIO=dual cargo village-lab
```

To watch the same one-village fixture through the real server, network and renderer:

```bash
./run.sh testworld
```

It starts at 1x. Use the HUD to pause or switch between 1x, 10x, 25x and 100x. Each run
prints a timestamped `logs/testworld-*` directory containing its server and client logs.
See [VILLAGE-LAB.md](docs/VILLAGE-LAB.md) for scenarios, overrides, expected evidence and
failure diagnosis.

### Generated-world stress fixture

```bash
./run.sh realworld
```

This stages 32 villagers at Oakfell on the ordinary generated world with normal policy and
an empty founding store. It starts at 1x and records `VillageTrace`, `ServerPerf` and route
diagnostics under `logs/realworld-*`. Useful overrides include
`FISTWORLD_REALWORLD_VILLAGERS=64`, `FISTWORLD_REALWORLD_AT=x,z` and
`FISTWORLD_RUN_LOG_DIR=/absolute/path`.

Press F3 near a settlement for population and planning-distance diagnostics. Press F4 to
draw its housing, workplace and coastal planning bands. These are preferred search areas,
not political borders.

To open the compact map without a staged settlement and found one manually:

```bash
CITYSIM_MAP_ID=village_lab ./run.sh
```

### Visual capture

```bash
cargo run -p client --bin capture -- --at -226,-163 --preset survey
cargo run -p client --bin capture -- --at 0,0 --preset daycycle --out /tmp/shots
cargo run -p client --bin capture -- --help
```

Presets include `survey`, `orbit`, `daycycle` and `water`. Rendering is required to catch
mesh winding, shader, foliage, lighting and anchor problems that compilation cannot.

## Verify changes

```bash
cargo check --workspace --all-targets
cargo test --workspace
cargo village-lab
cargo village-scale-lab       # release-only 5,000-person / 30-settlement benchmark
```

The full lab is intentionally ignored by ordinary `cargo test` runs.

## Important runtime flags

| Flag | Effect |
|---|---|
| `CITYSIM_MAP_ID=<id>` | Select the map for client and server |
| `CITYSIM_TERRAIN_COLLIDER_RADIUS_CHUNKS=<n>` | Streamed terrain-collider radius |
| `CITYSIM_TERRAIN_COLLIDER_MAX_LOAD_PER_TICK=<n>` | Collider chunks spawned per fixed tick |
| `CITYSIM_TERRAIN_COLLIDER_RESOLUTION=<n>` | Heightfield resolution per collider chunk |
| `FISTFORCE_AUTOCONNECT=<name>` | Skip local login/name entry |
| `FISTFORCE_CLIENT_PERF=1` | Emit rolling client frame-time diagnostics |
| `FISTFORCE_SERVER_PERF=1` | Emit server tick and phase diagnostics |

## Architecture in one minute

- The server is authoritative; clients send intent and render replicated truth.
- `SimulationDelta` captures real time, world time and global warp once per tick. Gameplay
  systems consume that clock instead of applying speed independently.
- Ordinary observed villagers use physical routes and animations. Unobserved ordinary
  residents retain durable identity/economic state and contribute through aggregate
  settlement production rather than pathfinding.
- `SettlementSummary` is globally visible; halls, buildings, markets, roads, fields and
  piers are region-scoped detail joined by stable IDs.
- Tactical village travel uses bounded obstacle surveys plus a cached shared road graph.
  Future large commanded groups still require regional flow fields.
- Player profiles currently persist; full settlement/world persistence does not. Do not
  mistake a long-running local simulation for a durable saved world yet.
