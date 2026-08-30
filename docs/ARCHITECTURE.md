# Game architecture

Decisions first recorded 2026-07-27 and reconciled with the live simulation on
2026-08-15. Retrofitting these boundaries is expensive, so read this before extending
simulation code.

The companion document [WORLD-DESIGN.md](WORLD-DESIGN.md) describes what runs ON this
architecture: settlements, goods, caravans, clans, and the player's climb from one guy
to a realm. [CIVIC-ECONOMY.md](CIVIC-ECONOMY.md) is the executable reference for market
ownership, municipal finance and settlement policy. The build order lives in
[ROADMAP.md](ROADMAP.md). Client presentation and interaction rules live in
[UI-ARCHITECTURE.md](UI-ARCHITECTURE.md). The real-renderer verification contract and
capture artifact architecture live in [VISUAL-CAPTURE.md](VISUAL-CAPTURE.md).

> **Status, audited 2026-08-15.** This remains a design record, with implementation state
> marked **[not built]**, **[partial]** or **[done]**. The living-village foundation now
> has stable world identities, one authoritative simulation clock, shared live/lab
> scheduling, region-scoped settlement detail, a global settlement directory and an
> aggregate off-screen economy. Stable companies now add 1,000-share cap tables, one
> treasury, site cost centres and settlement-local physical branches. Politics, armies,
> combat and world-state persistence are
> still unbuilt. Do not read an unmarked future rule as working code.

## The game

Real-time strategy in a persistent multiplayer world. You start controlling a single
character and expand — retinue, then holdings, then trade routes, then territory — until
clans contest whole realms. Mount & Blade / The Guild in ambition, RTS in perspective,
with seamless zoom from one soldier to the entire map.

Two choices define everything else:

- **Seamless continuous zoom.** One world, one camera, no campaign/battle mode switch.
- **Persistent always-on world.** The server simulates continuously; your trade routes
  earn and rivals expand while you are logged off.

---

## 1. Netcode: server-authoritative with interest management

**Deterministic lockstep is ruled out.** It was a live option while this was a
match-based RTS; the persistent world kills it. Lockstep requires every client to
simulate the entire world identically from turn zero, which cannot survive players
joining mid-game, logging off while their economy keeps running, or playing in regions
nobody else is watching. It also cannot have a server own persistence.

So: the server is authoritative, clients render and send intent, and each client receives
only what is relevant to it (see §3). This is what the surviving lightyear plumbing
already does, so no rewrite is needed — but it means **the server is the bottleneck to
design around**, not client frame time.

### Never dirty a replicated component you did not change

Replication sends on Bevy's **change flag**, not on a value difference: lightyear (via
bevy_replicon) re-serialises any component whose change tick moved, even when the new value is
byte-identical. On the 1,000-villager stress world this was the entire bandwidth problem — the
server pushed 417 KiB/s to one client while only ~100 villagers were actually walking. Three
ways a no-op write sneaks in, all measured and all fixed:

1. **Unconditional assignment.** `*activity = Idle` in a state machine that re-asserts its state
   every tick. Use `activity.set_if_neq(Idle)`; for a plain `&mut T` helper argument, guard with
   `if *t != v { *t = v }`.
2. **`Option<Mut<T>>::as_deref_mut()`.** `Mut::deref_mut` calls `set_changed()` *before* your
   guard decides not to write, so probing an optional component dirties it every tick. Use
   `as_mut()`, read through `Deref` (`**m`), and deref-mut only on the write path.
3. **Tuple inserts.** `insert((objective, navigation))` behind an OR-ed guard re-sends whichever
   member did not change. Insert each component under its own comparison.

Together these took the same world from 417 KiB/s to 233 KiB/s (-44%) with no simulation or
presentation change. Verify with `CITYSIM_NET_DEBUG=1`, which logs `Server net debug: … KiB/s …
villager changed/tick pos= rot= motion= …` every 2 s: **changed/tick for a component should track
the number of entities for which it genuinely changed** — if `motion` exceeds `pos`, or climbs
while the walker count is flat, something is dirtying it for standing entities. Because the lab
world grows as it runs, compare two builds at matched `pos=` bands rather than by wall clock.

A change-flag fix compiles identically whether or not it works, so pin each one with a test that
counts `Changed<T>` inside an `App` and confirm it fails on the unfixed code (see
`a_villager_waiting_for_a_route_never_dirties_its_replicated_motion` in `server/src/player/hero.rs`).

## 2. Two-tier simulation — the load-bearing decision

You cannot simulate ten thousand individual soldiers across a realm, and nothing at this
scale tries. Run two simulations and move entities between them.

### Strategic layer
Regions, settlements, clans, trade routes, and armies-as-single-parties. Ticks slowly
(≈1 Hz or slower). Runs **everywhere, always, for the whole world** — including regions no
player has ever visited.

Hard constraint: this must stay cheap enough to run forever for the entire map. That
bounds its design — **no pathfinding, no physics, no per-soldier anything**. A caravan is
one entity with cargo, a route, and an ETA. A garrison is a number. Movement is
interpolation along a graph edge, not navigation.

### Tactical layer
Individual units with world positions, pathfinding, collision and combat. Ticks at the
fixed 60 Hz rate. Instantiated **only** where a player is looking, or where something
contested is happening.

### Promotion / demotion
An army crossing the map is one strategic entity. When a player zooms in on it, or it
meets a hostile force, it **promotes** into N tactical units. When attention leaves and
the situation resolves, it **demotes** back to a strength number.

> **[partially built]** — ordinary off-screen villagers now carry `StrategicPerson` and
> shed movement targets, door/shopping state and trade-specific tactical progress.
> A resident already travelling retains one cheap `StrategicTravel` route cursor and
> advances it at the strategic cadence; promotion catches up the fractional interval and
> restores the remaining tactical waypoints instead of teleporting or restarting.
> Settlement production, workplace storage, porter sales and household purchasing advance
> in aggregate on the strategic step. Returning to a tactical region removes the marker
> and the normal assignment systems rebuild embodied routines from durable `PersonId`,
> `EmployedAt`, `LivesAt` and settlement relationships. Migration, construction, roads and
> market deliveries are transition-critical: an actor already doing one may finish before
> demotion. Armies and lossless battle promotion remain unbuilt.

Rules that keep this sane:

- Promotion must be **deterministic from strategic state** — the same party always
  produces the same roster, so a player zooming in does not reroll the world.
- Demotion must be **lossless in aggregate** — casualties, morale and cargo survive the
  round trip, or players will exploit zoom to dodge outcomes.
- Battles nobody observes are **resolved by formula**, not simulated. If a player is
  watching, simulate; if not, compute a result. These two paths must agree statistically
  or players will learn to look away at the right moment.
- Transitions need **hysteresis** — promote and demote at different thresholds, otherwise
  an entity at the boundary thrashes every frame.

## 3. Regions are one primitive, not four

The world is divided into regions. That single division serves all of:

| Role | Meaning | State |
|------|---------|-------|
| **Interest management** | What the server replicates to a given client | **[done]** |
| **Simulation LOD** | Whether this region is tactical or strategic right now | **[partial]** — ordinary villagers promote/demote between tactical routines, per-person strategic travel and aggregate settlement production/economy; construction, military travel parties and combat still need dedicated strategic forms. |
| **Political** | Who owns this land | **[not built]** — `RegionState` has no owner. See the correction below. |
| **Persistence** | The unit that gets saved and loaded | **[not built]** — `RegionState` does not even derive `Serialize`. See the correction below. |

Keeping these aligned is deliberate. When they diverge you end up maintaining several
spatial systems that disagree with each other, and every feature has to reconcile them.

**Correction — the political and persistence roles move off regions.** This section and
WORLD-DESIGN §5/§7 flatly contradicted each other: this doc said persistence is per region
and regions are the unit of conquest, while WORLD-DESIGN says region control is *derived,
never saved* and persistence is one world-state file. Resolved in WORLD-DESIGN's favour,
because it is the one that matches the gameplay verb — you take a *town*, not a grid
square:

- **Settlements** are the political and persistence unit. Ownership lives on the
  settlement; territory is computed from it.
- **Regions** are the interest / sim-LOD / travel-graph unit.

A useful consequence: regions do not need to become ECS entities. They stay a resource-held
map, and the per-region thing the strategic tick actually wants is an *index of the
settlements inside it* — a field, not an architecture.

**Known divergence — there are three spatial partitions today, not one.** The claim in this
section's title is already false in practice, and the note below about the 8m grid
understates it. Three live grids run and do not know about each other:

| Grid | Size | Used for | Radius policy |
|---|---|---|---|
| `RegionCoord` | 512m | replication interest | scales with camera zoom |
| `ChunkCoord` | 64m | terrain + static collider streaming | fixed +/-192m |
| `SPATIAL_CELL_SIZE` | 8m | close-range obstacle queries | n/a |

At max zoom the server replicates entities from a radius of kilometres while holding
colliders for 192m — precisely the divergence this section says regions exist to prevent.
This is *accepted* for now, not solved: collider streaming is an FPS-era structure that the
tactical-unit work is expected to replace outright. Recorded here so the next reader does
not assume it was reconciled.

## 4. What seamless zoom demands

This is the most technically demanding choice on the board. It requires:

- **Every entity has a representation at every zoom band.** A soldier is a model up
  close, part of an instanced blob at medium range, and a contribution to an army icon
  far out. Nothing may simply vanish.
- **Rendering LOD and simulation LOD are separate systems.** You can render a region
  you are not tactically simulating (as icons/abstract), and you must simulate a region
  no one is rendering (strategic tick). Do not couple them.
- **Transitions must not pop.** Cross-fade or match silhouettes across LOD bands.
  **[partial]** — the terrain seam is genuinely invisible; water hard-pops, non-tree props
  snap out at the wrong distance, and entities have no LOD to cross-fade at all.
- **Near and far water are one visual contract.** Detailed ocean and sloping rivers use the
  terrain-crossing water mesh. The far mesh supplies an opaque ocean underlay and a widened
  cartographic river only outside the streamed detail hole. Its river marker must be removed
  across the complete interpolated triangle fan inside that hole, or coarse vertices appear as
  X-shaped water streaks over close rivers. Generated inland banks derive sand/damp-earth paint
  from the same chunk-indexed river segments and local water surface; absolute sea height cannot
  identify a riverbank.
- ~~**Camera range grows enormously** — roughly 5 m to 20 km, versus today's 55–900 m.~~
  **[done, and the figures were stale].** The commander camera is already 12m–12,000m, and
  the shipped map is 8192m across, so 12km already frames the entire world. There is
  nothing left to gain from more range.
- ~~Expect depth-buffer precision problems; plan for a logarithmic depth buffer or per-band
  camera settings.~~ **[void — do not plan for this].** Bevy 0.19 builds
  `Mat4::perspective_infinite_reverse_rh`, so `far` never enters the depth matrix. With
  reverse-Z, 5cm of separation at 12km is still tens of ULP. No logarithmic depth buffer
  and no per-band camera settings are needed. This is an entire risk the roadmap can skip.

**The real gap is entity representation, not camera range.** Above ~1.5km nothing changes
for the remaining 8x of zoom: the world is one static coloured mesh with nothing on it.
Terrain is the ONLY thing with a far-zoom representation — heroes, buildings, and every
future settlement, caravan and army have exactly one representation (a full 3D mesh) and
either render as sub-pixel geometry or vanish. Read step 5 of the build order as *entity
representation across bands* (not started), not as camera work (done).

Rough bands to design against:

```
   5m   individual character, animation, gear
 100m   squads and formations, town streets
   1km  armies as instanced blobs, settlement models
  10km  region borders, clan colours, trade-route lines
```

## 5. What an always-on persistent world demands

- **The strategic tick runs for the entire world, forever.** Budget it as the primary
  server cost. If it is not cheap, nothing else matters.
- **Offline progress must be designed, not emergent.** Players will be away for days;
  decide explicitly what accrues, what decays, and what is protected.
- **Absent players need grief protection**, or the game punishes having a job.
- ~~**Persistence is per region**, and must handle a region being loaded/unloaded while
  neighbours stay live.~~ **[superseded — see the correction in §3].** Persistence is one
  world-state document keyed by stable settlement ids. Per-region sharding is a scale answer
  to a problem an 8km world with ~30 settlements does not have, and stable ids make it a
  pure write-side change if it is ever needed.
- **It needs a hosted server.** This is an operational commitment, not just code. The
  current server deliberately uses process-lifetime worlds and accounts: reconnect is
  supported, restart is a clean reset. Before durable seasons or worlds ship, hosting needs
  a volume and world/account saves need one shared versioned lifetime; persisting only an
  account into a reset society would preserve the wrong half of ownership.

On budgeting: the strategic tick now has real settlement production and commerce work.
`cargo village-scale-lab` is the regression gate: its 2026-08-05 reference fixture held
5,000 NPCs in 30 settlements and measured the complete steady bundle at 0.881 ms, the
daily economy, civic-finance and history burst at 1.446 ms, the full stable-identity/reconciliation
pass at 0.048 ms and aggregate strategic villages at 0.151 ms on the development machine,
with no entity or route-queue growth. These are reference numbers, not a platform
guarantee; retain the fixture and compare deltas whenever a world-wide rule is added.

## 6. What already exists and fits

- `shared/src/city` — **[partial, and read the caveat]**. The *geometry* layer (road
  strips, plot rects, frontage and facing math) is excellent, tested, genre-neutral and the
  single most reusable asset in the repo. The *taxonomy* is not: `RoadClass` is
  Alley/Local/Collector/Arterial with lane counts and parking, `PlotZone` is modern land-use
  zoning, and `CityBuildingKind` is nine modern apartment blocks. The medieval GLBs exist on
  disk and in `BuildingType` but no plot can reference them. The shipped map also has
  `roads: []` and `plots: []`, so none of this pipeline has ever run on the current world.
- `server/src/world/navgrid.rs` and village routing — **[live, bounded local use]**.
  The navgrid keeps the shared building obstacle index current; village travel, trades,
  ambient movement, construction and hero steps query it. `village_roads` owns the
  obstacle surveys, locally invalidated route cache, resumable long-route searches and
  road graph. Committed journeys use a priority lane ahead of ambient wandering; mass
  migration admission is paced in real time, nearby migrants can join a certified cohort
  approach, and Moot queue-rank changes use short local steps instead of global A*. Building
  and streamed-prop changes invalidate only intersecting route-cache entries. Repeated
  blocked-goal warnings are spatially and temporally coalesced, while the final live
  collision proof remains authoritative. Permitted worksite footprints block road surveys
  before their shells exist. The old generic `find_path` routine in
  `pathfinding.rs` remains unused, although its wall-clock budget settings are shared by
  the live queue. Future commanded groups still require regional flow fields rather than
  multiplying these local searches.
- Villager inspection intent — **[done]**. Server-only routines are folded at the end of
  the shared village activity schedule into compact, replicated objective and navigation
  enums. The client can distinguish a queue, work trip, delivery, home trip, route search
  or blocked route without receiving routine internals, destinations or formatted debug
  strings for every tactical NPC. Component writes are change-gated.
- Chunked terrain streaming and huge maps. **[done]**
- lightyear replication + session identity. **[done]** — heroes, cargo, wallets and
  account-keyed commanded units survive disconnect/reconnect to the same process. A server
  restart intentionally starts clean until world and account durability can ship together.
- Replicated character velocity and bounded client extrapolation. **[done]** — the client
  advances an actor for at most 80 ms beyond its latest authoritative snapshot, then uses
  normal correction smoothing. Turns and stops replicate immediately enough to prevent
  runaway prediction without requiring 60 Hz position snapshots.
- Dense-crowd character rendering. **[done for the current village target]** — the nearest
  160 visible people use full rigs; farther visible people retain individual moving roots
  and selection state while sharing proxy mesh/material assets. The proxy is rendered
  directly on each replicated root, avoiding a second entity and hierarchy transform per
  distant person. This rendering LOD is independent of server simulation LOD.
- Stable `PersonId`, `SettlementId` and `BuildingId` relationships, global settlement
  summaries plus region-scoped physical/economic detail, and aggregate off-screen village
  production. **[done for the current village simulation]**
- The live server and Village Lab share one ordered village registration. Its
  core is explicitly partitioned into identity/population, civic,
  economy/planning, construction, activity and directory sets, with nested
  economy sets for markets, households, settlement accounts and permits. These
  are scheduling/telemetry boundaries, not duplicated implementations.
- ~~The commander camera, which needs its zoom range extended by ~20×.~~ **[done]** —
  12m–12,000m, which covers the whole map.

**What does NOT exist, despite being easy to assume from the rest of this document:**
combat (stripped wholesale — ~9,600 lines — and never replaced), strategic armies,
caravans, clans, political ownership, world-state persistence, and a production path for
founding or commanding a body outside the current dev/gameplay tools.

## 7. Build order

**Moved to [ROADMAP.md](ROADMAP.md).** This section and WORLD-DESIGN §8 used to carry two
different orderings, and they disagreed about when the promotion seam and flow fields land.
One list now covers both, with per-phase checklists.

The engine steps this section listed map onto it as follows, with their real state:

| Old step | Reality | Lands in |
|---|---|---|
| 1. Region layer | Interest management done; ownership and persistence never started, and both move off regions entirely (§3) | Phase 1 |
| 2. Strategic tick | **Partial.** Villager production, workplace stock, porter commerce and household purchasing run in aggregate. The first tactical company-owned caravan route now performs buyer-funded Stone contracts between two observed settlements; off-screen caravan aggregation, armies and strategic construction remain future work. | Phase 3 |
| 3. Tactical units + flow fields | Not started. Deliberately moved LATE: it is the biggest block of work and carries the least architectural uncertainty. | Phase 6 |
| 4. Promotion/demotion | **Partial for ordinary villagers.** Tactical routine state is shed/rebuilt across `SimLevel`; army and travelling-party aggregate contracts remain. | Phase 2 |
| 5. Zoom bands + render LOD | Camera range and dense-villager full-rig/proxy split done; buildings, armies and effects still need representation across bands. | as needed |
| 6. Art pass | ongoing | — |

**The parting advice of this section still stands, and is why the order changed.** "Step 4
is where the design actually gets tested, so do not leave it until last" was being violated
by default: the old world-design order stacked four phases of economy on top of a seam it
never validated. The seam is now Phase 2, tested on the cheapest entity that has one — a
single traveller, measured by *arrival time*, which needs no combat code at all.

One amendment: the advice was also impossible to act on as written, because the seam has
neither endpoint — there is no strategic entity to promote and no tactical unit type to
promote into. So the actionable form is: **write the promotion contract as tests the first
time any strategic entity exists**, before the machinery it constrains.

## 8. Living-world implementation rules

The current village simulation uses these rules as hard boundaries:

- **Identity is data, names are labels.** `PersonId`, `SettlementId`, `BuildingId` and `CompanyId` are
  authoritative across regions, payroll, ownership, employment, housing, UI commands and
  serialized relationships. Legacy name rosters remain for readable panels and old-state
  migration only; the versioned world-state file itself is still a roadmap item.
- **There is one simulation clock.** `SimulationDelta` captures real seconds, world
  seconds and warp once at the start of the shared tick; `SimulationTime` is the read-only
  system parameter used by gameplay. No gameplay system multiplies `Time` by `TimeWarp`
  independently. Strategic work integrates the full elapsed interval, including exact
  shift overlap at high warp.
- **The live game and Village Lab share one ordered schedule.** Add village behaviour to
  `server/src/world/village/schedule.rs`; do not maintain a second hand-copied lab list.
- **Summary and detail are different entities.** `SettlementSummary` is tiny and global.
  Halls, buildings, worksites, roads, fields, piers, markets and inventories carry
  `RegionCoord` and replicate only through interest management. They join by stable id.
- **Off-screen work is aggregate; travel is a cheap cursor.** A strategic person must not
  own tactical pathfinding, a movement target, door timer, seat, shopping trip or resource
  animation. An already planned journey may retain immutable waypoints plus one progress
  index and advance at the bounded strategic cadence. Add world-wide work rules to the
  strategic settlement pass and cover tactical/strategic agreement with tests.
- **Village domains have explicit owners.** `village.rs` is the public facade and shared
  state model; migration and resident counts live in `village/population.rs`, demand and
  geography-aware permits in `village/planning.rs`, material supply and building work in
  `village/construction.rs`, vacancy matching in `village/employment.rs`, physical Moot
  Steward collection in `village/commerce.rs`, municipal budgets/payroll and the enacted
  relief/reserve/staffing/subsidy policy model in
  `village/civic.rs`, business transaction settlement and
  owner strategy/solvency in `village/businesses/`, bounded firm/market/settlement
  archives in `village/history.rs`, legal firms, pooled treasuries, shares and consolidated
  ledgers in `village/companies.rs`, aggregate food/prosperity in
  `village/settlement_economy.rs`, household provisioning in `village/households.rs`,
  physical trades in `village/trades.rs`, production rates in `village/production.rs`,
  strategic LOD in `village/strategic.rs`, and shared ordering in `village/schedule.rs`.
  `village_roads.rs` owns the local survey primitives and public road state; connector
  construction lives in `village_roads/construction.rs`, graph routing and caches in
  `village_roads/routing.rs`, the Moot Steward's civic repair duty in
  `village_roads/steward.rs`, and width/dryness
  geometry in `village_roads/geometry.rs`. Keep extending those seams instead of growing
  either facade into a monolith.
- **UI chrome is a shared foundation, not screen-local behavior.** `ui/styles.rs` owns the
  palette, `ui/foundation.rs` owns semantic layers, type scale, buttons, disabled/focus state
  and live-panel refresh safety, `ui/modal.rs` owns the one-scrim modal structure, and
  `ui/scroll.rs` owns nested wheel bubbling. Economy screens preserve the entity under the
  pointer and bound structural refresh at high simulation speed. See
  [UI-ARCHITECTURE.md](UI-ARCHITECTURE.md).
