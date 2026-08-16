# Fistworld

A persistent-world multiplayer RTS built with **Rust** and **Bevy 0.19**. One
seed-generated world contains autonomous settlements whose named residents migrate, form
households, take jobs, produce and trade physical goods, build roads and grow their town.
The long-term game expands that foundation into player businesses, caravans, clans,
territory and war.

The old first-person game is preserved at git tag `citysim-final`; the live workspace is
the RTS/living-world codebase. Start with [WORLD-DESIGN.md](docs/WORLD-DESIGN.md) for the
game, [CIVIC-ECONOMY.md](docs/CIVIC-ECONOMY.md) for the executable market and policy
rules, [COMPANY-ECONOMY-IMPLEMENTATION.md](docs/COMPANY-ECONOMY-IMPLEMENTATION.md)
for shares, pooled finance and vertical integration,
[ARCHITECTURE.md](docs/ARCHITECTURE.md) for technical boundaries, and
[ROADMAP.md](docs/ROADMAP.md) for implemented and future work.

## Current playable foundation

- A chunk-streamed generated world with biomes, rivers, coastlines, water, foliage,
  atmospheric day/night lighting and a seamless commander camera.
- Server-authoritative multiplayer, region interest management, session accounts and
  stable `PersonId`, `SettlementId`, `BuildingId`, `CompanyId`, `TradeContractId` and
  `TradeRouteId` relationships. A disconnected player
  can rejoin the same running server and re-adopt their live hero, cargo, coin and retinue;
  restarting the server intentionally begins a fresh world.
- God-mode settlement founding and villager spawning. Unaffiliated people choose a
  settlement, migrate to its hall and become residents.
- Autonomous housing, employment, permits and geography-aware construction. Seeded
  planning grammars create organic lanes, radial commons, grids, avenues or clustered
  neighbourhoods without moving completed buildings.
- Physical Wood supply and construction; Farmsteads with two wheat fields, Fisherman's
  Huts with piers, Lumberjack Huts with reachable-tree checks, authored Windmills and
  Bakeries, and repeated housing/food construction driven by measured shortages.
- Continuous visible work loops for farming, fishing, chopping, milling and baking;
  non-edible Wheat becomes household Flour, while Bakeries convert two Flour to four
  ready-to-eat Bread. Bounded personal and
  building inventories, up to two combined early Moot Stewards, seller-owned Moot consignments,
  adaptive wages, real business profit/loss, protected working capital, NPC owner strategies,
  input procurement, stock liquidation/property auctions, household food purchases, Poor
  Relief and durable business lifecycle states.
- Stable companies above productive sites: every firm has exactly 1,000 ordinary
  shares, a separately appointed Company Master, one authoritative treasury, consolidated liabilities,
  tax and pro-rata dividends. A sole proprietor is normally owner, Master and worker of
  one site; successful firms can retain profit, fund another permit and grow into a
  multi-site company without moving money through the founder's wallet.
  Buildings retain site P&L and policy ledgers but never receive separate cash allocations.
  A hero explicitly founds and capitalises a company at a Hall before it can buy a
  business permit. When the hero masters several firms, the Hall always shows and transmits
  the selected `ACTING AS` company; personal and company money are never silently mixed.
  Daily payroll is charged to the completed shift at dawn, paid from the company treasury
  and attributed to the worker's site; unpaid amounts remain explicit arrears.
- Same-company Farmstead → Windmill → Bakery chains give active company input needs first
  claim on owned Wheat or Flour, then release only genuine surplus. Players manage this with
  readable days-of-stock coverage, `Company first`/`Best value`/`Company only` sourcing and
  `Sell surplus`/`Hold all`, while the server derives bounded unit targets from real staffing
  and recipes. Tactical stewards carry the real
  goods door to door; strategic simulation performs the same bounded transaction. Site
  ledgers retain attributed internal flow while company P&L eliminates both memorandum
  sides. The encyclopedia's Companies tab exposes a global firm directory, each Hero's
  multi-company share portfolio, cap tables, public offers, linked sites, capital assets,
  decisions, settlement-local branches and company-owned inter-settlement route history.
- Buyer-funded inter-settlement Stone delivery is live. A qualified Town Works project
  posts a cash-backed public tender even when no quarry has opened yet. Stone-rich settlements
  can respond to that real demand; once enough Stone is listed the tender binds the exact seller. The source town then
  needs a completed Storage Hall and one employed Company Porter; that ordinary company
  owns the reusable route, physically collects the exact seller's cargo, carries it between
  settlements, and earns freight only after delivery. A small minimum call-out keeps a valid
  partial load above the carrier's fixed wage cost. There is no hardcoded trade-company
  category, and active contracts/routes are visible in Hall and company UI.
- Company Masters can also author merchant caravan timetables from the Companies encyclopedia.
  A route is a building-like operating asset based at a staffed Storage Hall, with two to eight
  ordered town stops. Each stop buys from the public market, loads owned warehouse stock, sells
  by real consignment or unloads into an owned destination warehouse. The panel exposes a
  left-to-right stop lane, cart target, buy ceiling, sale floor, one-circuit/repeating service,
  current caravaner and bounded trip economics. Merchant firms risk their own cash; posting cargo
  at a destination records asking value only and pays nothing until a real buyer purchases it.
- Door traversal, designated homes, night routines, occupied-window lighting, ambient
  walking/sitting, character attributes and bounded life histories.
- Shared 100-point Health for heroes and villagers, staged hunger ceilings, gradual fed recovery, safe offline-Hero dormancy, bounded
  death history, household estate settlement, vacant-job recovery and inherited-business
  takeover listings.
- Builder-made obstacle-safe roads, a shared local travel graph and the paid Moot Steward,
  who combines consignment collection with audits and repairs of disconnected buildings.
- Auditable civic finance: payroll reserves and durable arrears for every public role,
  market fees, positive-profit levies, paid public procurement, Surplus-Only Poor Relief,
  food/payroll targets, staffing posture, permit subsidies and weekly Reeve policy review.
- Inspectable residents, households, buildings, worksites, markets, settlement progress,
  inventories and prosperity in the selection UI and encyclopedia. Every Hall also has a
  scrollable Permits & Property board with live permit prices/demand and inherited or insolvent
  business listings, alongside pull-based settlement, civic, market, individual-business
  and consolidated company history ledgers.
- Left click inspects any visible person, building or worksite; left-drag selects the local
  hero and future player-commanded units inside the marquee. Picking follows current rendered
  character positions, respects exact rotated building footprints and remains aligned when the
  3D render scale is below the window resolution.
- Player heroes have bounded physical inventories and wallets. Walking within 12 metres of
  a Hall and pressing `E` opens its exchange: buying transfers real listed stock, while
  posting an offer deposits cargo into a person-owned consignment and pays nothing until a
  real buyer clears it. A newly created hero begins with 20 coin; reconnecting to that same
  live hero preserves the existing wallet instead of granting the endowment again.
- The Hall's property ledger is actionable for an embodied hero. It returns an exact
  company-specific business-permit quote, charges only its refundable permit fee, and opens a
  world placement ghost. Road frontage snaps magnetically while free placement remains
  available; footprints, fields, water, charter bounds and access are server-authoritative.
  Farmstead and Lumberjack Hut placement exposes live farmland/timber quality, and unused
  permits remain in a bounded tray where they can be resumed or surrendered for a fee refund.
  Recommended processor capital is advisory and remains ordinary spendable company cash.
- One authoritative simulation clock, the same ordered village schedule in the live game
  and lab, aggregate off-screen village production, and summary/detail replication.

The live tier ladder is **Hamlet → Village → Town → City**. Its current executable
population gates are 12, 30 and a provisional 75 residents, combined with sustained food,
prosperity, trade and civic-building requirements. Promotion is now physical: a qualified
Hamlet must purchase and stage 12 Wood for its Village Hall, while a qualified Village must
purchase and stage 8 Stone for its Town Hall, importing it by a physical company route when
the local market cannot supply it; a named civic worker then walks to the Hall and
constructs it. Those values remain prototype balance, not final design.

## Major work still ahead

- Versioned world-state persistence, migrations, backups and hosted durable storage.
- Births, aging, non-starvation mortality, decline and persistent tree depletion/regrowth.
- NPC merchant speculation, wagon art, multi-wagon route scaling and general regional price
  discovery remain ahead. Buyer-funded Stone contracts and player-authored physical multi-town
  merchant routes are live. Players can now
  commission and physically build an ordinary business, then manage strategy, wages, prices,
  collection, input procurement, company shares, private supply and profit retention through
  the same policies as NPC owners. Company-funded purchases of existing listed firms and
  durable restart persistence remain future work.
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
[ASSET_NAMING.md](asset_creation/ASSET_NAMING.md). UI structure, theming, modal and
live-panel rules are in [UI-ARCHITECTURE.md](docs/UI-ARCHITECTURE.md).

## Run the game

```bash
./run.sh                 # server in the background, then the client
./run.sh --dev           # faster compile, slower runtime
./run.sh --release       # shipping/performance measurement profile
```

The default `playtest` profile keeps release-grade optimisation without thin LTO and with
incremental compilation. Use it for normal play and iteration.

### Display settings

Open **Pause → Graphics** to choose a conventional display mode and output resolution:

- **Windowed** uses the selected physical client-area resolution.
- **Borderless** uses the desktop's current/native resolution, as required by the windowing
  system. Choose **Fullscreen** when you want a lower true output resolution.
- **Fullscreen** uses an exact video mode reported by the active monitor. When several modes
  share a resolution, the client selects the highest refresh rate and then bit depth.

Mode and resolution changes apply immediately and show a 15-second **Keep / Revert** prompt.
They are not saved until confirmed, and automatically return to the last working setting if
the countdown expires. **3D Render Scale** is independent: it lowers only the world render
target while keeping the window and UI sharp.

The same panel also exposes **3D Grass Renderer**. **Chunked** is the production default: it
preserves the same deterministic 1x grass placement, authored blades, climate colour, wind,
terrain height, and road/building exclusions, but sends compact GPU instance buffers in coarse
camera-cullable sectors. **Legacy** remains available as a compatibility/debug fallback.
Switches apply live and the unused renderer is torn down rather than left doubled in memory.

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

The Stone comparison puts a fertile Meadow beside a genuinely Stone-rich control. It checks
the autonomous Quarry permit, embodied mining and the paid Hall-upgrade material pipeline:

```bash
FISTWORLD_LAB_SCENARIO=stone-comparison FISTWORLD_LAB_WARP=10 cargo village-lab
./run.sh stoneworld
```

The focused rendered trade fixture starts two established Village controls with 12 founders each,
then grows them smoothly to 35 residents at 10x. Whichever settlement first qualifies for Town
Works becomes the real Stone buyer; the other economy can respond with a quarry, warehouse,
porter and company route. The lab follows the resulting market direction instead of forcing a
named settlement to win the development race. The chosen Meadow and Stonefield controls are on a
proved walkable overland corridor, so this fixture tests economics and embodied transport rather
than a future bridge or shipping mechanic:

```bash
./run.sh tradeworld
```

For the repeatable heavy fixture, run three settlements with 200 residents
each at 10x:

```bash
FISTWORLD_LAB_SCENARIO=triple-stress FISTWORLD_LAB_WARP=10 FISTWORLD_LAB_MINUTES=180 cargo village-lab
```

For the one-town density target, run 1,000 residents through the same real 10x
simulation:

```bash
FISTWORLD_LAB_SCENARIO=dense-stress FISTWORLD_LAB_WARP=10 FISTWORLD_LAB_MINUTES=180 cargo village-lab
```

To watch the same one-village fixture through the real server, network and renderer:

```bash
./run.sh testworld
```

It starts at 1x. Use the HUD to pause or switch between 1x, 10x, 25x and 100x. Each run
prints a timestamped `logs/testworld-*` directory containing its server and client logs.
See [VILLAGE-LAB.md](docs/VILLAGE-LAB.md) for scenarios, overrides, expected evidence and
failure diagnosis.

Use `./run.sh stressworld` to watch the three 200-person settlements together.
It starts at 10x with a wide camera, enables server/client performance telemetry,
and records both processes under `logs/stressworld-*`. Logs stay out of the
terminal by default so terminal I/O does not distort the stress result; use
`FISTWORLD_STREAM_LOGS=1 ./run.sh stressworld` when live log mirroring is useful.

Use `./run.sh denseworld` for one 1,000-person Meadow town at 10x. All people
remain separate replicated actors; neighbourhood views give full character rigs
to at most the closest 160, while map-scale people use continuously moving
proxy meshes directly on their selectable actor roots, so the client does not
instantiate tens of thousands of skeleton or hierarchy entities at once.

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

For a deterministic grass comparison that cannot accidentally frame open ocean:

```bash
CITYSIM_MAP_ID=village_lab FISTFORCE_GRASS_RENDERER=legacy \
  cargo run -p client --bin capture -- --at 112,-158 --name grass --zoom 90 --tilt 0.55
CITYSIM_MAP_ID=village_lab FISTFORCE_GRASS_RENDERER=chunked \
  cargo run -p client --bin capture -- --at 112,-158 --name grass --zoom 90 --tilt 0.55
```

To stress the renderer without changing normal-world density, add
`FISTFORCE_GRASS_STRESS_DENSITY=4` (or up to `32`) to either command. This is a
capture/profiling override only; ordinary gameplay and saved graphics settings remain at 1x.

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
| `FISTFORCE_GRASS_RENDERER=legacy\|chunked` | Override the saved 3D grass renderer for captures and profiling |
| `FISTFORCE_GRASS_STRESS_DENSITY=<1..32>` | Capture/profiling-only grass density multiplier; normal gameplay remains 1x |
| `FISTFORCE_DISPLAY_MODE=windowed\|borderless\|fullscreen` | Override the saved display mode for this run |
| `FISTFORCE_RESOLUTION=<width>x<height>` | Override Windowed/Fullscreen output resolution |
| `FISTFORCE_RENDER_SCALE=<0.5..1.0>` | Override the independent 3D render-target scale |

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
- Player, hero and commanded-NPC state currently persists only for the lifetime of the
  running server process. Restarting starts clean; durable settlement/world saves remain a
  later, explicitly versioned feature.
