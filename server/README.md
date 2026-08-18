# Server module map

`server` is the authoritative simulation. `server/src/main.rs` only declares the
top-level modules and calls `app::run()`; runtime rules belong to their domain.

## Top-level domains

| Module | Owns |
|---|---|
| `app` | Bootstrap, resources and ordered fixed-update wiring |
| `net` | Connections, peer identity and client-message ingress |
| `player` | Commander views, hero lifecycle, rosters, movement orders, nearby Hall trading, permit/company funding and placement, physical hero construction, Company Master policy commands, share trading, authoritative caravan timetable orders and player indexes |
| `collision` | Baked/derived building colliders, spatial indexes, raycasts and streamed static collision |
| `world` | Time, identity, regions, settlements, village simulation, roads, development and lab fixtures |
| `persistence` | Session profile snapshots plus legacy profile migration/IO tooling; the live server deliberately starts fresh |
| `telemetry` | Tick/phase timing, replication pressure and opt-in diagnostics |
| `city` | Synchronisation for authored plot buildings; autonomous settlements live under `world` |

The removed combat, vehicle, inventory and legacy NPC-AI domains are not runtime
dependencies. Do not recreate generic versions of them when a living-world domain owns the
rule more precisely.

## World and village ownership

`world/mod.rs` is an orchestration surface. Its principal modules are:

- `identity.rs`: allocates and indexes durable `PersonId`, `SettlementId`,
  `BuildingId` and `CompanyId` values and migrates remaining readable legacy relationships.
- `simulation_time.rs` and `time.rs`: the one real/world/warp clock and world-day state.
- `regions.rs`: interest management, region visibility and tactical/strategic level.
- `settlement_directory.rs`: tiny globally replicated settlement summaries.
- `settlement_development.rs`: Hamlet → Village → Town → City gates, civic projects and
  main-road upgrades.
- `village.rs`: public village state/facade. Implementation is split by domain under
  `world/village/`:
  - `population`: settlement choice, migration, visible Moot registration and resident reconciliation
  - `planning`: demand, permits and geography/layout-aware siting
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
  - `households`: homes, pantry funding, shopping, meals and daily schedules
  - `tavern`: compact per-person day plans, private meal pricing, embodied Tavern visits,
    Innkeeper shifts and equivalent cheap off-screen service
  - `trades` / `production`: physical and aggregate farming, fishing, livestock, lumber and Stone work,
    plus the shared Wheat → Flour → Bread recipes and paired Meat/Wool output
  - `quarry`: embodied outdoor Stone extraction and livestock tending, bounded personal loads and workplace deposit
  - `trade_routes`: buyer-funded civic import contracts, company-owned route assets,
    staffed Storage Hall dispatch, ordered merchant stops, physical inter-settlement cargo and freight/sale accounting
  - `processing`: embodied Windmill and Bakery shifts using bounded private inventories
  - `property_market`: compact Hall-published takeover listings for completed firms and worksites
  - `strategic`: off-screen person compression and aggregate settlement work
  - `history`: bounded person/settlement records and request handlers
  - `mortality`: sparse time-warp-safe hunger ceilings, gradual fed recovery, critical starvation damage, death records,
    household estates, civic/job cleanup, orphaned construction and business succession
  - `ambient`: continuously budgeted, neighbourhood-local observed-region idle life
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
Their cached output budget is shared by tactical and strategic production. Solvent unwanted
sites mothball and reopen before the permit planner considers duplicate capacity. All
expansion/dividend reserves must use enabled positions, not architectural maximums.
NPC Storage Halls normally require an established branch with two other local sites. A real
export contract lowers that threshold to one productive site so a quarry concern can vertically
integrate into transport; player permits stay available independently of the autopilot rule.

## Scheduling rules

The server runs authoritative work in `FixedUpdate`. `app/schedule.rs` establishes the
high-level order, while `world/village/schedule.rs` registers the village sets used by both
the live server and lab.

The important dependencies are:

1. Capture `SimulationDelta`, apply world/admin time and update authoritative world time.
2. Reconcile identity, collision and region state.
3. Run village core decisions, households, economy, construction and physical work.
4. Run bounded road/route planning and actor movement.
5. Apply interest visibility and update simulation LOD.
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
  zero wage from the still-open day's ledger.
- `SettlementSummary` is global; physical/economic detail carries `RegionCoord` and is
  replicated only through interest management.
- A `StrategicPerson` retains durable social/economic state but owns no tactical path,
  door timer, seat or animation progress. World-wide work belongs in aggregate passes.
- Embodied routes must remain bounded, cached and shared where possible. Future army/group
  command requires flow fields rather than multiplying local A*.
- Watercraft use the separate `player::boat` stack. `Vessel` is the generic navigation
  opt-in; road/character routes must never move a vessel. Direct water lines are the fast
  path, obstructed searches are water-certified A*, and `VesselNavigationQueue` admits at
  most four searches per fixed tick so a fleet order cannot monopolize network ingress.
  Server movement rechecks water and derives speed from the shared deterministic wind.
- Avoid per-tick full-population scans, string joins and allocations. Reconcile on changed
  state or slow world boundaries.
- The live server does not load player or world state after restart. Legacy player-profile
  tooling uses positional bincode and requires `PROFILE_VERSION` changes; any future durable
  settlement/world save must be versioned and self-describing instead.

## Verification

From the workspace root:

```bash
cargo check --workspace --all-targets
cargo test --workspace
cargo village-lab
cargo village-scale-lab
```

Use `./run.sh testworld` for a rendered deterministic village,
`./run.sh stressworld` for three rendered 200-person villages at 10x, and
`./run.sh denseworld` for one rendered 1,000-person village at 10x. Use
`./run.sh realworld` for the logged generated-world stress fixture. Full usage and diagnostics are in
[`docs/VILLAGE-LAB.md`](../docs/VILLAGE-LAB.md).
The stress launcher writes complete server/client logs quietly by default; set
`FISTWORLD_STREAM_LOGS=1` only when terminal mirroring is desired.
