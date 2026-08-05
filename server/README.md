# Server module map

`server` is the authoritative simulation. `server/src/main.rs` only declares the
top-level modules and calls `app::run()`; runtime rules belong to their domain.

## Top-level domains

| Module | Owns |
|---|---|
| `app` | Bootstrap, resources and ordered fixed-update wiring |
| `net` | Connections, peer identity and client-message ingress |
| `player` | Commander views, hero lifecycle, rosters, movement orders and player indexes |
| `collision` | Baked/derived building colliders, spatial indexes, raycasts and streamed static collision |
| `world` | Time, identity, regions, settlements, village simulation, roads, development and lab fixtures |
| `persistence` | Versioned player profiles, autosave and background IO; full world-state persistence is not built |
| `telemetry` | Tick/phase timing, replication pressure and opt-in diagnostics |
| `city` | Synchronisation for authored plot buildings; autonomous settlements live under `world` |

The removed combat, vehicle, inventory and legacy NPC-AI domains are not runtime
dependencies. Do not recreate generic versions of them when a living-world domain owns the
rule more precisely.

## World and village ownership

`world/mod.rs` is an orchestration surface. Its principal modules are:

- `identity.rs`: allocates and indexes durable `PersonId`, `SettlementId` and
  `BuildingId` values and migrates remaining readable legacy relationships.
- `simulation_time.rs` and `time.rs`: the one real/world/warp clock and world-day state.
- `regions.rs`: interest management, region visibility and tactical/strategic level.
- `settlement_directory.rs`: tiny globally replicated settlement summaries.
- `settlement_development.rs`: Hamlet → Village → Town → City gates, civic projects and
  main-road upgrades.
- `village.rs`: public village state/facade. Implementation is split by domain under
  `world/village/`:
  - `population`: migration and resident reconciliation
  - `planning`: demand, permits and geography/layout-aware siting
  - `construction`: physical material supply and building work
  - `employment`: private vacancy matching
  - `commerce`: porter work, business accounts, payroll and owner leisure
  - `settlement_economy`: Moot transactions, food security and prosperity
  - `households`: homes, pantry funding, shopping, meals and daily schedules
  - `trades` / `production`: physical and aggregate farming, fishing and lumber work
  - `strategic`: off-screen person compression and aggregate settlement work
  - `history`: bounded person/settlement records and request handlers
  - `ambient`: cheap observed-region idle life
  - `schedule`: the shared ordered schedule used by production and Village Lab
- `village_roads.rs`: local-road public state and survey facade. Implementation under
  `world/village_roads/` owns connector construction, cached routing, geometry and Road
  Steward repair.
- `village_lab.rs` and `village_lab_scenario.rs`: deterministic integration harness and
  rendered fixture setup. They must use the shared village schedule, never a copied list.

Extend those ownership seams instead of moving implementation back into `village.rs` or
`village_roads.rs`.

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
6. Persist player state and close telemetry brackets.

Add a village rule to its existing shared set. Do not multiply Bevy `Time` by `TimeWarp`
inside the new system, and do not add lab-only ordering to make a test pass.

## Data and performance rules

- Names are display strings. Ownership, employment, housing, civic rosters and adjunct
  entities join through durable IDs.
- `SettlementSummary` is global; physical/economic detail carries `RegionCoord` and is
  replicated only through interest management.
- A `StrategicPerson` retains durable social/economic state but owns no tactical path,
  door timer, seat or animation progress. World-wide work belongs in aggregate passes.
- Embodied routes must remain bounded, cached and shared where possible. Future army/group
  command requires flow fields rather than multiplying local A*.
- Avoid per-tick full-population scans, string joins and allocations. Reconcile on changed
  state or slow world boundaries.
- Player profiles use positional bincode and require `PROFILE_VERSION` changes. The planned
  settlement/world save must be versioned and self-describing instead.

## Verification

From the workspace root:

```bash
cargo check --workspace --all-targets
cargo test --workspace
cargo village-lab
cargo village-scale-lab
```

Use `./run.sh testworld` for a rendered deterministic village and `./run.sh realworld` for
the logged generated-world stress fixture. Full usage and diagnostics are in
[`docs/VILLAGE-LAB.md`](../docs/VILLAGE-LAB.md).
