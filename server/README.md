# Server module map

`server` is the authoritative simulation. `server/src/main.rs` only declares the
top-level modules and calls `app::run()`; runtime rules belong to their domain.

## Top-level domains

| Module | Owns |
|---|---|
| `app` | Bootstrap, resources and ordered fixed-update wiring |
| `net` | Connections, peer identity, client-message ingress and personal-possession recipient filters |
| `player` | Commander views, hero/boat lifecycle, rosters, battalions and formation orders, melee, nearby Hall trading, permit/company funding and placement, physical hero construction, Company Master policies, shares and caravan timetable orders |
| `collision` | Baked/derived building colliders, spatial indexes, raycasts and streamed static collision |
| `world` | Time, identity, regions, settlements, village simulation, roads, development and lab fixtures |
| `persistence` | In-memory session account/commander snapshots; the live server deliberately starts fresh |
| `telemetry` | Tick/phase timing, replication pressure and opt-in diagnostics |
| `city` | Synchronisation for authored plot buildings; autonomous settlements live under `world` |

The removed first-person combat, vehicles and NPC AI are not runtime dependencies.
The current RTS owns melee in `player/combat.rs`, boats in `player/boat.rs`, armies in
`player/army.rs` and `player/army/`, tactical intent and shared route fields in
`player/orders.rs` and `player/orders/`, and physical inventories in `shared::economy`. Extend those live domains.
See [the game code map](../docs/GAME-CODE-MAP.md) for the cross-crate ownership map.

`net/possessions.rs` filters `Wallet` and `GoodsInventory` at the replication
backend, allowing only the authenticated hero owner or current `CommandedBy`
account to receive personal values. These economic components require a denied
filter on initial spawn; changed ownership/account state settles in `PostUpdate`
before replication sends. Identified buildings, markets and construction storage
retain their public region-scoped contract. Unclassified storage remains private.
`CarriedLoad` exposes only the visible good/appearance; exact quantities stay in
`GoodsInventory`. Packet tests cover first spawn, later joins, authority loss and
public storage. Changes to company/civic account privacy are a separate policy.

`net/chat.rs` sends ephemeral server-wide plaintext messages only between accepted,
currently connected accounts. Sender names come from the session registry. Each
connection may send a burst of three messages, then one every two real seconds;
simulation pause and speed changes do not refill this allowance. Requests are
limited to 280 Unicode characters and 1,024 UTF-8 bytes, with controls rejected.
Only four requests per connection are inspected per update; excess messages are
discarded. A separate ordered reliable channel carries messages and private
rejection feedback. Disconnect/account replacement clears chat state; the server
keeps no history and never logs message bodies.

## World and village ownership

`world/mod.rs` is an orchestration surface. Its principal modules are:

- `bootstrap.rs` selects the normal server's world seed before terrain-dependent resources
  initialize. `new_world/` surveys sites and land regions, validates complete layouts and
  initializes named society once before the socket opens. It has no recurring growth or
  economy overrides; see [NEW-WORLD.md](../docs/NEW-WORLD.md).
  `new_world/layout/economy.rs` rates initial food chains and workforce from approved plots.
  `new_world/trade_access.rs` owns the server-only founding land-group gate consumed by
  civic and player trade routes; it does not bypass ordinary caravan navigation.
- `identity.rs`: allocates and indexes durable `PersonId`, `SettlementId`,
  `HouseholdId`, `BuildingId` and `CompanyId` values and migrates remaining readable legacy relationships.
- `simulation_time.rs` and `time.rs`: the one real/world/warp clock and world-day state.
- `regions.rs`: interest management, region visibility and observer counts; no simulation-level selection.
- `settlement_directory.rs`: tiny globally replicated settlement summaries.
- `household_yards.rs`: bounded household land fitting, local refitting after roads or
  neighbouring buildings change, and shared yard obstacles. See
  [HOUSEHOLD-YARDS.md](../docs/HOUSEHOLD-YARDS.md).
- `farm_boundaries.rs`: publishes accepted crop fences only when their physical
  segments are clear of actor bodies, and refreshes the local navigation geometry.
  `village/field_parcels.rs` owns deterministic crop surveys; `farm_productivity.rs`
  applies accepted field area to the same worker output rules everywhere. See
  [FARM-FIELDS.md](../docs/FARM-FIELDS.md).
- `settlement_development.rs`: Hamlet → Village → Town → City gates, civic projects and
  main-road upgrades.
- `village.rs`: public village state/facade. Implementation is split by domain under
  `world/village/`:
  - `population`: settlement choice, migration, visible Moot registration and resident reconciliation
  - `planning`: demand, permits and geography/layout-aware siting; `neighborhood`
    owns bounded frontage infill and soft compatible-neighbor preferences
  - `construction`: physical material supply and building work
  - `employment`: private vacancy matching
  - `commerce`: physical Moot Steward collection work and owner leisure
  - `businesses`: sale settlement, site accounts, pricing/strategy, protected company distributions,
    insolvency, physical stock liquidation and property takeover
  - `companies`: existing-site migration, 1,000-share cap tables, appointed Company Masters,
    one authoritative company treasury, current/completed-day site cost-centre consolidation,
    dividends, executive review and company-permit fee recovery
  - `civic`: municipal hiring budgets, unified payroll/arrears, profit levies, staffing posture,
    growth subsidies and bounded policy review
  - `settlement_economy`: Moot transactions, food security, prosperity and the daily
    hunger/homelessness/unpaid-work unrest reading
  - `households`: home schedules; `households/membership` owns stable domestic groups,
    `provisioning` owns scheduled food/fuel purchasing and shared funding, `needs`
    owns hearth use and exact contribution math, `shopping` owns physical cargo
  - `tavern`: compact per-person day plans, private meal pricing, embodied Tavern visits,
    Innkeeper shifts and the same queued service throughout the world
  - `trades` / `production`: physical farming, fishing, livestock, lumber and Stone work,
    plus the shared Wheat → Flour → Bread recipes and paired Meat/Wool output
  - `quarry`: embodied outdoor Stone extraction and livestock tending, bounded personal loads and workplace deposit
  - `trade_routes`: buyer-funded civic import contracts, company-owned route assets,
    staffed Storage Hall dispatch, ordered merchant stops, physical inter-settlement cargo and freight/sale accounting
  - `processing`: embodied Windmill and Bakery shifts using bounded private inventories
  - `property_market`: compact Hall-published takeover listings for completed firms and worksites
  - `history`: bounded person/settlement records and request handlers
  - `mortality`: sparse time-warp-safe hunger ceilings, gradual fed recovery, critical starvation damage, death records,
    household estates, civic/job cleanup, orphaned construction and business succession
  - `ambient`: continuously budgeted, neighbourhood-local idle life in every town
  - `schedule`: the shared ordered production/Lab schedule, with explicit
    identity, civic, economy, construction, activity and directory timing sets
- `village_roads.rs`: local-road public state and survey facade. Implementation under
  `world/village_roads/` owns connector construction, cached routing, geometry and the Moot
  Steward's road-repair duty.
- `village_lab.rs` and `village_lab_scenario.rs`: deterministic integration harness and
  rendered fixture setup. They must use the shared village schedule, never a copied list.

Extend those ownership seams instead of moving implementation back into `village.rs` or
`village_roads.rs`.

The exact civic money flows, policy ranges, staffing targets and weekly Reeve decision
order are documented in [`docs/CIVIC-ECONOMY.md`](../docs/CIVIC-ECONOMY.md). Update that
guide whenever a civic revenue source, expense, liability or policy effect changes.
Company/share authority, retained-cash expansion and vertical-integration rules are in
[`docs/COMPANY-ECONOMY-IMPLEMENTATION.md`](../docs/COMPANY-ECONOMY-IMPLEMENTATION.md).
Processor input management exposes 0–7 days of physical coverage. Public output policy is
instead an absolute per-good retain amount on the company's settlement-local branch, followed
by one `Sell excess`/`Hold all` choice. Sites expose a separate bounded enabled-position target.
Storage Halls and Company Porters extend a local branch. Cross-settlement movement is legal
only through an explicit `CompanyTradeRoute`. Cash-backed civic Stone tenders bind an exact
seller and use locked pickup/delivery stops; the carrier is paid only at the destination, with a
small minimum call-out fee for otherwise uneconomic partial loads. Player merchant routes use
two to eight ordered `Buy`/`Load`/`Sell`/`Unload` stops. Buy spends company cash, Sell creates an
ordinary seller-owned market consignment, and private Load/Unload is legal only at that company's
Storage Hall. Both modes require an employed Company Porter and use the bounded
coarse-middle/fine-endpoint overland planner. Do not add implicit shared stock between branches.
Autonomous firms begin with one enabled position, then change by one position per day toward
the marginally profitable roster supported by recent sales, unmet demand and existing stock.
The cached forecast informs staffing and investment; it does not ration physical output.
Employed workers everywhere continue through their shift while resources, inputs
and storage permit. Solvent unwanted sites mothball and reopen before the permit planner
considers duplicate capacity. All
expansion/dividend reserves must use enabled positions, not architectural maximums.
NPC Storage Halls normally require an established branch with two other local sites. Funded
civic export or merchant opportunity signals can also justify a standalone logistics firm;
player permits stay available independently of the autopilot rule. Bounded autonomous
merchant trials use delayed reports and the same physical routes as player timetables.

## Scheduling rules

The server runs authoritative work in `FixedUpdate`. `app/schedule.rs` establishes the
high-level order, while `world/village/schedule.rs` registers the village sets used by both
the live server and lab.

The important dependencies are:

1. Capture `SimulationDelta`, apply world/admin time and update authoritative world time.
2. Reconcile identity, collision and region state.
3. Run village core decisions, households, economy, construction and physical work.
4. Run bounded road/route planning and actor movement.
5. Apply network interest visibility and update observer counts; do not change simulation.
6. Refresh reconnectable session state and close telemetry brackets.

Add a village rule to its existing shared set. Do not multiply Bevy `Time` by `TimeWarp`
inside the new system, and do not add lab-only ordering to make a test pass.

## Data and performance rules

- Names are display strings. Ownership, employment, housing, civic rosters and adjunct
  entities join through durable IDs.
- Productive sites join their legal/economic firm through `CompanyId`. Sites have no cash
  allocation: `BusinessAccount` is a cost-centre ledger and `CompanyAccount` is the sole
  spendable treasury. Internal supply is
  eliminated in consolidation and company/person transfers use explicit capital, wage,
  dividend or share-sale paths.
- Private payroll closes the completed shift at dawn. It must debit the company treasury,
  credit the worker, and attribute the expense to the completed site ledger exactly once.
  Company consolidation must publish both the open day and completed day; never infer a
  zero wage from the still-open day's ledger. Consolidate into a value before updating
  `CompanyAccount`; unchanged books must not advance its replication change tick.
- `SettlementSummary` is global; physical/economic detail carries `RegionCoord` and is
  replicated only through interest management. Directory building counts are cached until
  building or settlement-assignment components change, including removals. Update summary and
  position independently so a statistics change does not re-send an unchanged location.
- Static-prop collider coverage is anchored by authoritative actor/building chunks, excluding
  camera-only `Player` entities. Wild horses use a smaller local footprint.
  Multiple actors in one chunk share streaming work; building-zone revisions still refresh
  loaded chunks even when all desired chunks are already present.
- Every person follows the same authoritative movement, work, cargo, queue and needs
  routines. Camera coverage must not change job admission or substitute aggregate work.
  The former strategic/physical fork is retired; see [SIMULATION-PARITY.md](../docs/SIMULATION-PARITY.md).
  Fresh whole-world performance and matched-observation acceptance remain unverified.
- Tactical prop surveys use live static colliders for loaded chunks. Replaying the
  immutable prop recipe there would resurrect trees already cleared by roads, fields
  or building plots. Unloaded ground retains conservative generated blockers and known
  axe-work clearance; surviving permanent colliders remain solid.
- A Tavern's land reservation includes its walkable courtyard. Its own road connector
  certifies a straight apron between the actual tables and starts public-road turns
  outside the patio; neighbouring plots still respect the whole reserved courtyard.
  Cached actor routes include the same shared table obstacles as movement collision;
  only the inn shell participates in doorway recovery.
- `village_roads/start_recovery` repairs an embodied route origin caught inside newly
  available live prop collision. It admits at most a 4 m correction within the route
  budget, with a dry connector that monotonically exits every initial overlap and
  crosses no other prop or building. The destination then uses ordinary route
  certification; no collision bypass persists on the actor. Opt-in lab diagnostics
  record the initial solid and subsequent actual movement.
- Embodied routes must remain bounded, cached and shared where possible. Army commands
  already use bounded shared local fields; future regional routing must extend that
  boundary rather than multiplying per-soldier A*.
- Live land permits enter `planning::plots::find_permit_site`, which always budgets
  one outward band plus its open-land fallback. All ordinary kinds share the budget;
  dense service/civic sites must not bypass it and synchronously scan a whole town.
  A failed band resumes at the next review, and the fixed Marketplace square remains
  its authoritative anchor. Startup layout surveys retain their separate full search.
- `planning/search_access.rs` compares built-road geometry and local terrain chunk
  revisions only at an admitted permit review. Meaningful access changes rewind that
  settlement's ordinary land searches; metadata, unfinished suffixes and distant
  terrain edits preserve progress. Road completion must wake an exhausted search even
  when no new road entity was added.
- `commerce/collection_failures.rs` keeps eight recent failed pickup sites per porter
  in fixed server-only storage. Alternate sellers remain eligible; failed entrances
  retry after 20–25 unwarped seconds or immediately after that entrance moves. This
  prevents alternating destinations from bypassing navigation's single-goal backoff.
- Watercraft use the separate `player::boat` stack. `Vessel` is the generic navigation
  opt-in; road/character routes must never move a vessel. Direct water lines are the fast
  path; `player/boat/navigation.rs` retains direct-line sampling, water A*, route
  reconstruction and shortcut validation between bounded slices. `VesselNavigationQueue`
  advances at most four slices per fixed tick in rotation, with at most four retained
  fleet searches; superseding an order or pausing
  an account releases its obsolete frontier. A shared 64-entry terrain-versioned LRU reuses
  positive and negative water results for player boats and natural arrivals. The exact
  start-to-grid connector is retained and every shortcut remains water-certified.
  Server movement rechecks water and derives speed from the shared deterministic wind.
- `world::immigration` owns natural arrivals. It scores settlements from public opportunity
  plus bounded personal/geographic bias, uses the generic vessel navigator for an ephemeral
  map-edge Dinghy, and only removes the boat after it reaches its certified mooring. The
  passenger then resumes the existing land-route and Moot-queue flow; this is not a second
  admission implementation. `immigration::admission` creates the real passenger and
  full-hull-safe dinghy first, with no target. `immigration::director` then scores current
  opportunities from the hull's actual position and retains one landfall/water proof.
  `ImmigrantArrival` records entry and committed-choice facts for observation. A lost route
  retries through the same planner with capped real-time backoff; a missing/ruined town or
  flooded landing returns that same body and hull to bounded destination selection.
  No-town arrivals wait in real boats under the eight-voyage cap; no abstract backlog grows.
  The Hall departure occupancy list is built only when a served embodied immigrant actually
  needs a physical exit reservation. Coast-to-Hall viability uses a retained search budget;
  reachable/unreachable results are cached and terrain/Hall-entrance changes invalidate them.
  Ordinary worlds enable natural immigration by default, while labs disable it unless
  `FISTWORLD_NATURAL_IMMIGRATION=1` is explicitly supplied. Defaults are a base three world
  arrivals per day with seasonal variation and a 5,000-villager world ceiling. Explicit
  `immigrants_per_day` configuration or `FISTWORLD_IMMIGRANTS_PER_DAY` uses steady global
  spacing (zero disables recurring arrivals); `FISTWORLD_WORLD_NPC_CAP` overrides the cap.
  No town receives a quota or guaranteed share of newcomers.
- Avoid per-tick full-population scans, string joins and allocations. Reconcile on changed
  state or slow world boundaries.
- The live server does not load player or world state after restart. Profiles are in-memory
  session snapshots. Any future durable account/settlement/world save needs an explicit
  version envelope and migration policy before payload decoding.

## Verification

From the workspace root:

```bash
cargo check --workspace --all-targets
cargo test --workspace
cargo village-lab
cargo village-scale-lab
```

Use `./run.sh testworld` for a rendered deterministic village,
`./run.sh regionalworld` for four gently growing settlements under contrasting
generated resource conditions,
`./run.sh stressworld` for three rendered 200-person villages at 10x, and
`./run.sh denseworld` for one rendered 1,000-person village at 10x. Use
`./run.sh realworld` for the logged generated-world stress fixture. Full usage and diagnostics are in
[`docs/VILLAGE-LAB.md`](../docs/VILLAGE-LAB.md).
The stress launcher writes complete server/client logs quietly by default; set
`FISTWORLD_STREAM_LOGS=1` only when terminal mirroring is desired.

### Large-town development

`world/village/planning/districts.rs` owns append-only residential wards and bounded
frontage infill; `reservations.rs` protects nearby accepted defense corridors.
`world/fortifications/` owns dry-land circuit surveys, material ownership, civic
construction and opt-in passage diagnostics. Shared section geometry feeds both
`world/navgrid.rs` and `village_roads/routing.rs`; hero movement and arrow collision
also enforce completed defenses. See [FORTIFICATIONS](../docs/FORTIFICATIONS.md)
and [TOWN-GROWTH-LAB](../docs/TOWN-GROWTH-LAB.md) for scope and validation.

### Wildlife

`world/wildlife/` owns natural horse placement, habitat validation and observation
budgets. Only active or ridden horses participate in collider streaming.
`player/riding/` owns mounted-pair commands, cavalry equipment and lifecycle;
ambient behavior lives in wildlife. Issued mounts share session IDs but do not
consume the ambient population budget. See [the wildlife contract](../docs/WILDLIFE.md)
and [cavalry lab](../docs/CAVALRY.md).
