# Fistworld

A persistent-world multiplayer RTS built with **Rust** and **Bevy 0.19**. One
seed-generated world contains autonomous settlements whose named residents migrate, form
households, take jobs, produce and trade physical goods, build roads and grow their town.
Player businesses, physical company caravans and tactical battles already extend that
foundation. The long-term game connects them through paid retinues, durable worlds,
clans, territory and war.

The old first-person game is preserved at git tag `citysim-final`; the live workspace is
the RTS/living-world codebase. Start with [WORLD-DESIGN.md](docs/WORLD-DESIGN.md) for the
game, [CIVIC-ECONOMY.md](docs/CIVIC-ECONOMY.md) for the executable market and policy
rules, [COMPANY-ECONOMY-IMPLEMENTATION.md](docs/COMPANY-ECONOMY-IMPLEMENTATION.md)
for shares, pooled finance and vertical integration,
[ARCHITECTURE.md](docs/ARCHITECTURE.md) for technical boundaries, and
[ROADMAP.md](docs/ROADMAP.md) for implemented and future work. The executable coastal
arrival and reusable ship-navigation contract are documented in
[PLAYER-START-AND-VESSELS.md](docs/PLAYER-START-AND-VESSELS.md).

## Current playable foundation

- A chunk-streamed generated world with biomes, rivers, coastlines, water, foliage,
  atmospheric day/night lighting and a seamless commander camera.
- The ordinary launch creates a random seeded world with roughly ten inhabited settlements.
  Terrain, local resources and certified access govern their sites, sizes and businesses;
  named residents, homes, companies and finite opening stock become ordinary simulation
  state. See [NEW-WORLD.md](docs/NEW-WORLD.md).
- [Wild horse herds](docs/WILDLIFE.md) on meadow grass, with server-owned identity,
  grazing and wandering near observers, and bounded client animation rigs. Stables
  and horse acquisition remain future work.
- Server-authoritative multiplayer, region interest management, session accounts and
  stable `PersonId`, `SettlementId`, `BuildingId`, `CompanyId`, `TradeContractId` and
  `TradeRouteId` relationships. A disconnected player
  can rejoin the same running server and re-adopt their live hero, cargo, coin and retinue;
  restarting the server intentionally begins a fresh world.
- Battalion selection and frontage orders, flexible melee, mounted cavalry in the
  battle lab, archers with finite quivers,
  and animated catapults with authoritative splash damage. Army management handles
  membership, standing stances, equipment and fire policy. Recruitment and catapult
  acquisition still require developer access; paid military supply/upkeep are not built.
- God-mode settlement founding and villager spawning. Unaffiliated people choose a
  settlement, migrate to its hall and become residents.
- Autonomous housing, employment, permits and geography-aware construction. Seeded
  planning grammars create organic lanes, radial commons, grids, avenues or clustered
  neighbourhoods without moving completed buildings.
  Bounded frontage infill encourages small groups of homes, with soft proximity
  preferences for related workplaces. The [town-growth lab](docs/TOWN-GROWTH-LAB.md)
  compares actual development under gentle, steady and burst immigration.
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
- Left click inspects visible people, buildings and worksites; touching an owned battalion
  member selects that battalion. Left-drag selects the local hero and commanded units,
  expanding touched battalions. Picking follows current rendered
  character positions, respects exact rotated building footprints and remains aligned when the
  3D render scale is below the window resolution.
- Player heroes have bounded physical inventories and wallets. Walking within 12 metres of
  a Hall and pressing `E` opens its exchange: buying transfers real listed stock, while
  posting an offer deposits cargo into a person-owned consignment and pays nothing until a
  real buyer clears it. A newly created hero begins with 20 coin; reconnecting to that same
  live hero preserves the existing wallet instead of granting the endowment again.
- A new account now creates its Hero outside God Mode and begins aboard an authored Dinghy
  at a server-chosen map edge. The opening camera moves from the dressed Hero's face into
  the RTS view; the selected boat follows water-only routes, sails faster or slower with
  the shared wind and turns/fills its sail from apparent wind. Right-clicking nearby dry
  shore disembarks and selects the Hero while the one-use starter boat becomes a visible
  wreck. Bounded vessel-route planning is shared infrastructure for later merchant and war
  ships. Returning to the same running server re-adopts the live Hero and skips creation.
- Ordinary worlds also receive natural NPC immigration through physical one-use Dinghies.
  Arrivals choose among towns from food, housing, work, civic conditions, individual taste
  and a bounded distance preference; after a water-certified voyage they disembark on dry
  land, walk with the normal villager pathfinder and join the visible Moot Hall line.
- The Hall's property ledger is actionable for an embodied hero. It returns an exact
  company-specific business-permit quote, charges only its refundable permit fee, and opens a
  world placement ghost. Road frontage snaps magnetically while free placement remains
  available; footprints, fields, water, charter bounds and access are server-authoritative.
  Farmstead and Lumberjack Hut placement exposes live farmland/timber quality, and unused
  permits remain in a bounded tray where they can be resumed or surrendered for a fee refund.
  Recommended processor capital is advisory and remains ordinary spendable company cash.
- One authoritative linear simulation clock (one world minute per real second at 1x),
  with a 05:00 sunrise, 23:00 sunset and independently defined 06:00-18:00 work shift;
  the same ordered village schedule in the live game and lab, aggregate off-screen village
  production, and summary/detail replication.

The live tier ladder is **Moot (Hamlet) → Village → Town**; City is future work.
Its current population gates are 12 and 30 residents, combined with sustained food,
prosperity, trade and civic-building requirements. Promotion is now physical: a qualified
Hamlet must purchase and stage 12 Wood for its Village Hall, while a qualified Village must
purchase and stage 8 Stone for its Town Hall, importing it by a physical company route when
the local market cannot supply it; a named civic worker then walks to the Hall and
constructs it. Those values remain prototype balance, not final design.

## Major work still ahead

- Ordinary player/AI settlement founding, paid introductory work and military recruitment.
  The initial settlement network is generated; later new Halls still require developer access.
- Versioned world-state persistence, migrations, backups and hosted durable storage.
- Births, aging, non-combat/non-starvation mortality, decline and persistent tree depletion/regrowth.
- Strategic caravan travel, larger transport, multi-wagon route scaling, escorts and
  interception remain ahead. Buyer-funded Stone contracts, player-authored physical multi-town
  routes and bounded NPC merchant trials using imperfect price reports are live. Porter
  hand-cart art is already integrated. Players can now
  commission and physically build an ordinary business, then manage strategy, wages, prices,
  collection, input procurement, company shares, private supply and profit retention through
  the same policies as NPC owners. Company-funded purchases of existing listed firms and
  durable restart persistence remain future work.
- Physical palisades, stone walls, gates, guards and patrols.
- Strategic travelling parties and armies with lossless tactical promotion/demotion.
- Military hiring/upkeep, morale, weapon classes, diplomacy, clans and realm war.
  Tactical battalions, formation orders, bounded shared route fields, melee and ballistic archers are live;
  see [combat controls and limits](docs/COMBAT-DESIGN.md).

The checked state and dependencies for each item live in [ROADMAP.md](docs/ROADMAP.md).
The [September plan review](docs/PLAN-REVIEW-2026-09.md) proposes the next playable
milestones and records the documentation corrections behind that recommendation.

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
./run.sh                 # new random inhabited world, then the client
./run.sh --dev           # faster compile, slower runtime
./run.sh --release       # shipping/performance measurement profile
```

The default `playtest` profile keeps release-grade optimisation without thin LTO and with
incremental compilation. Use it for normal play and iteration.

Normal play disables God commands. Each server restart creates a fresh world; the seed
is printed in the terminal and in `logs/game-*/server.log`. Reproduce its initial geography
and settlements with `FISTWORLD_WORLD_SEED=12345 ./run.sh`. Joining another running server
uses that server's recipe automatically. Explicit lab commands keep their fixed scenarios.

### Display settings

Open **Pause → Graphics**. The three display modes are explicit buttons at the top:

- **Windowed** uses the selected physical client-area resolution.
- **Borderless Fullscreen** uses the desktop's current/native resolution. It is the default and
  preserves ordinary macOS Command-Tab, Mission Control and Spaces behavior.
- **Exclusive Fullscreen** changes the monitor to an exact video mode reported by the operating
  system. When several modes share a resolution, the client selects the highest refresh rate and
  then bit depth. On macOS this mode takes control of the display, so normal app/Space switching
  is unavailable until the game leaves exclusive mode.

Mode and resolution changes apply immediately and show a 15-second **Keep / Revert** prompt.
They are not saved until confirmed, and automatically return to the last working setting if
the countdown expires. **Output Resolution** is disabled in Borderless because the desktop
owns its video mode. **3D Resolution** remains adjustable in every mode and shows the actual
scene pixels alongside the percentage, for example `1512 x 982 (50%)` on a 3024 x 1964 display.
It lowers only the world render target while keeping the window and UI sharp. Steps range
from 25% to 100%; macOS defaults to 60%. Settings labels also update after a resize or revert.

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

For broader balancing, `regional-economy` runs four towns with identical gentle
growth under contrasting coast, frozen, forest and Stone conditions on the larger
generated-gradient map. Watch it at the canonical 10x with `./run.sh regionalworld`,
or run its accounting and life-history audit headlessly:

```bash
FISTWORLD_LAB_SCENARIO=regional-economy FISTWORLD_LAB_WARP=25 FISTWORLD_LAB_MINUTES=700 cargo village-lab
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

To watch the inland town-growth fixture through the real server, network and renderer:

```bash
./run.sh testworld
```

It starts at 1x over a broad inland meadow, using charter seed 23 and steady
immigration. Set `FISTWORLD_TOWN_SEED` and `FISTWORLD_TOWN_PROFILE=low|steady|burst`
to compare growth patterns; see [the town-growth lab](docs/TOWN-GROWTH-LAB.md).
Use the HUD to pause or switch between 1x, 10x, 25x and 100x. Each run
prints a timestamped `logs/testworld-*` directory containing its server and client logs.
God mode also exposes **SPAWN IMMIGRANT BOAT**, which preserves the selected speed and follows
one real random-coast arrival through sailing, disembarkation and the walk to the Moot queue.
See [VILLAGE-LAB.md](docs/VILLAGE-LAB.md) for scenarios, overrides, expected evidence and
failure diagnosis.

For player-management UX work, open the prepared mature-city fixture instead:

```bash
./run.sh uxworld
```

It starts at 1x beside a City with 500 admitted residents, 125 completed homes, a paved
market, civic services and a varied private economy. Seventy ordinary NPC companies own its
prepared businesses, including single-site firms and farm–windmill–bakery groups, so permit,
company, ownership, strategy, salary, inventory, market and ledger screens have useful data
immediately. Create the local hero normally; in this fixture only, the hero begins beside the
City Hall. The prepared opening capital and stock make this a deterministic UX/stress fixture,
not an economy-balancing baseline. Logs are written under `logs/uxworld-*`.

To reproduce large-city congestion with ordinary gameplay systems, run:

```bash
./run.sh uxstressworld
```

This starts the same 500-resident City at 10x, lets its jobs and logistics establish, then
spawns 500 ordinary prospective immigrants on day 2. They must queue, immigrate, seek work and
housing, and trigger normal private development. An opt-in causal watchdog records actors who
fail to advance toward an unchanged goal for three world minutes, including their objective,
route state, cargo and active porter routine. The scenario also records navigation priority
peaks, shared-destination route reuse and ambient admission, making it the primary regression
for mass-arrival and stationary-porter bugs. Both process logs are retained under
`logs/uxstressworld-*`. The mode is driven by two opt-in switches that also
work on any other map: `FISTWORLD_UX_STRESS=1` (the stress arrival wave) and
`FISTWORLD_STUCK_WATCH=1` (the low-frequency no-progress watchdog - log
output only, no simulation effect).

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

### Manual battalion battle

```bash
./run.sh battle5v5
```

Builds both binaries and opens five 50-person battalions against five defending
battalions (500 soldiers total), with combat mode and a commander hero ready.
Drag-select your troops and right-click an enemy to attack. The client stays
under manual control. Closing it or pressing Ctrl-C stops the server started by
this command. Logs are retained under `logs/battle5v5-*`.

The usual `--dev` and `--release` profiles work here too. Stop any existing local
server first; this mode reports an occupied port instead of terminating another
session. `./run.sh battleworld` remains the smaller recruitment sandbox.

Use `./run.sh archerworld` for two infantry battalions and one archer battalion
against three enemy battalions (120 soldiers per side). Select the archers and
right-click an enemy to fire; **V** toggles hold fire. Army management has equipment,
fire-policy and quiver controls. See [archery controls and limits](docs/ARCHERY.md).

Use `./run.sh cavalryworld` for two eight-rider cavalry battalions and eight
infantry against 32 enemy infantry. Right-click to move or attack and right-drag
to set formation width/facing. Click either horse or rider to select their
battalion. See [cavalry controls and limits](docs/CAVALRY.md).

Use `./run.sh mixedbattle` for four 32-person battalions per side: your **I** archers,
**II/III** infantry and **IV** cavalry against four enemy infantry battalions
(128 soldiers per side, with 32 horses for your cavalry). Archers begin behind the
infantry and cavalry on the right flank. Right-click an enemy to attack; the enemy
counterattacks ten simulation seconds after your archers first shoot. You retain
manual control throughout. The scenario is `capture/scenarios/battle-mixed-4v4.ron`.

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
cargo run -p client --bin capture -- --scenario capture/scenarios/world-survey.ron
```

Presets include `survey`, `orbit`, `daycycle` and `water`. Rendering is required to catch
mesh winding, shader, foliage, lighting and anchor problems that compilation cannot. Each
scenario shot now emits a PNG plus JSON metadata, waits on explicit streaming readiness, can
assert world state, and can compare against approved pixel baselines. The complete scenario,
offscreen, recording and regression workflow is in
[VISUAL-CAPTURE.md](docs/VISUAL-CAPTURE.md).

For a deterministic grass capture that cannot accidentally frame open ocean:

```bash
CITYSIM_MAP_ID=village_lab \
  cargo run -p client --bin capture -- --at 112,-158 --name grass --zoom 90 --pitch 0.55
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
| `FISTFORCE_CLIENT_PERF=1` | Emit rolling client frame-time diagnostics (`ClientPerf`) plus per-panel UI work counts (`ClientPerfUi`: calls / rebuilds / ms per panel system) |
| `FISTFORCE_OPEN_ENCYCLOPEDIA=people\|places\|retinue\|companies` | Diagnostics: open the encyclopedia on that tab a few seconds into gameplay, for perf runs without input automation |
| `FISTFORCE_SERVER_PERF=1` | Emit server tick and phase diagnostics |
| `FISTFORCE_GRASS_STRESS_DENSITY=<1..32>` | Capture/profiling-only grass density multiplier; normal gameplay remains 1x |
| `FISTFORCE_DISPLAY_MODE=windowed\|borderless\|exclusive` | Override the saved display mode for this run (`fullscreen` remains an alias for exclusive) |
| `FISTFORCE_RESOLUTION=<width>x<height>` | Override Windowed/Exclusive Fullscreen output resolution |
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
  Repeated destinations share bounded reverse shortest-path trees and committed route cohorts;
  optional ambient travel has a per-settlement admission budget, while essential freight,
  work, migration and home trips retain priority and age-based fairness. Embodied crowd
  separation uses a local spatial grid instead of an all-pairs pass. Future large commanded
  groups still require regional flow fields.
- Player, hero and commanded-NPC state currently persists only for the lifetime of the
  running server process. Restarting starts clean; durable settlement/world saves remain a
  later, explicitly versioned feature.
