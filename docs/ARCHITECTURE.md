# Game architecture

Decisions first recorded 2026-07-27, with implementation status reviewed on
2026-09-16. Historical performance measurements retain their original dates and do
not certify the revised world-wide execution model.
Read these boundaries before extending simulation code.

Ordinary world bootstrap lives in `shared::map::session` (validated terrain recipe),
`server::world::bootstrap` (seed selection), `world::start_config` (validated starting
configuration) and `world::new_world` (site survey, complete
layout validation and finite society initialization before the socket opens). Founding
scores world-wide coverage and tags Halls with certified local land networks; trade review
uses these server-only tags to reject cross-group commitments without per-tick path searches.
Actual workplace quality and recipe throughput bound initial food capacity. Later growth
remains owned by the ordinary village systems. Reliable name acceptance carries the map
recipe/hash; `client::ui::name_entry::network` prepares it asynchronously, invalidates old
rendering caches and enters Playing only after validation. Replicated earthworks remain
separate from the immutable recipe. See [NEW-WORLD.md](NEW-WORLD.md).

The optional Frontier opening reuses certified sites and access, but commits only Halls
and unhoused founders on surveyed clear ground. Its exact settlement count, finite starting
stock and population are validated before publishing. The Mature profile preserves the
existing complete-town layout. Both use the same generated terrain recipe and ordinary
simulation after initialization; neither supplies ongoing production or construction.

Client startup connection ownership lives in `render::systems::connection`: DNS runs
on the I/O pool, connection attempts are bounded by a deadline, and cancellation or
disconnect returns to the launcher with retained feedback. `ui::name_entry` separates
editable, submitted and preparing phases, locking the submitted account identity and
dropping the pending map-install path when leaving the screen. `ui::startup` owns only
shared presentation; it does not invent world progress or bypass server validation.

Regional journeys and paid connections are described in [REGIONAL-TRAVEL.md](REGIONAL-TRAVEL.md).
`player/boat/navigation.rs` owns retained water planning: each geometric advance has
a 500 μs deadline and a 32,768-work-unit ceiling. Long routes try a 24 m grid before
falling back to 6 m; both allow at most 60,000 unique expansions and enforce the same
hull clearance. Retained searches and sailing certificates track their terrain
footprint. A touched search may retain its frontier as a proposal, but must freshly
certify the entire resulting route against current terrain and bridge/pier geometry
before use. Full map replacement discards the search. The 64-entry route cache may
propose a successful same-goal/hull suffix for a different start or revision; the
new connector and complete suffix require the same fresh proof. Stale failures
cannot veto newly opened water. Immigration stages a real boat first, then pins one
valid destination through landfall and water proof without holding up other due
world entries. Changing town scores do not restart that in-progress choice.
`world::regional_roads`
collects completed cargo evidence, incrementally certifies short road sections and reserves
real treasury wages/materials before assigning an embodied worker. Shared `RoadBridge`
geometry grants crossing only when complete; rendering cannot authorize water passage.
The active project pass addresses at most two workers/sections by entity and retains their
plans without per-frame corridor cloning. These are implemented work bounds, not measured
whole-world speed claims.

The companion document [WORLD-DESIGN.md](WORLD-DESIGN.md) describes what runs ON this
architecture: settlements, goods, caravans, clans, and the player's climb from one guy
to a realm. [CIVIC-ECONOMY.md](CIVIC-ECONOMY.md) is the executable reference for market
ownership, municipal finance and settlement policy. The build order lives in
[ROADMAP.md](ROADMAP.md). Client presentation and interaction rules live in
[UI-ARCHITECTURE.md](UI-ARCHITECTURE.md). The real-renderer verification contract and
capture artifact architecture live in [VISUAL-CAPTURE.md](VISUAL-CAPTURE.md).
[GAME-CODE-MAP.md](GAME-CODE-MAP.md) maps common changes to their current source owners.

Music is local presentation under `client::audio::music`, with one non-spatial
voice independent of scenery. The opening boat requests its cue there; the
controller waits for actual sink completion before background music, pauses/resumes
the retained background on mute, and clears playback on exit from Playing.
`audio::catalog` retains seven approved effects. `audio::sfx` admits at most four
UI voices from coalesced semantic requests; unavailable or stale clicks are dropped.
`ui::sound` adapts accepted UI input and book navigation without owning game actions.
`audio::carts` admits at most four observed moving-cart loops, with a ground-focused
listener, zoom/distance fades and cleanup when idle, unobserved, muted or disconnected.
`audio::perspective` defines reusable world-sound distance/zoom/tone profiles.
`audio::filtered` prepares bounded shared mono PCM asynchronously and supplies
live per-voice low-pass filtering through native Bevy `Decodable`. Its decoder
owns looping (`PlaybackSettings::ONCE`); the producer owns voice lifetime. The
cache holds at most eight 2 MiB clips, while carts still admit at most four voices.
The obsolete remote-player walking loop has been removed. No effect networking is
needed for this slice. `audio::settings` persists independent music/effects switches
and Master/Music/Effects levels, with immediate mixing and debounced drag saves.
The shared capture settings-isolation flag disables both file reads and writes.
The [audio workshop](../asset_creation/audio/README.md) owns original sources,
compression and provenance. Generation has no runtime or server dependency.
The [sound-effects design](AUDIO-DESIGN.md) distinguishes this implemented first pack
from future footsteps, combat and ambience adapters; asset authors follow its SFX pipeline.

Accepted crop parcels and household yards are authoritative land geometry, not
independent client decorations. The shared shape contracts supply fitting,
containment, access and fence segments; the server owns publication, ownership,
navigation and collision. The client batches ground-following crops, fences and
domestic details from those contracts with bounded rebuilds and distance detail.
Geometry revisions invalidate nearby terrain/vegetation caches; quality-only
production changes do not rebuild the land. See [FARM-FIELDS.md](FARM-FIELDS.md),
[HOUSEHOLD-YARDS.md](HOUSEHOLD-YARDS.md) and [TOWN-DRESSING.md](TOWN-DRESSING.md).

A terrain edit keeps its old chunk resident while the replacement mesh builds;
`LoadedChunks.rebuilding` tracks replacement work separately from streaming residency.
Scenery consumers must not interpret a dirty chunk as an unload. Building and civic
claims compare effective XZ geometry, so unchanged replication and foundation-height
writes do not clear vegetation. Changed plots remove only covered roots; released
plots use budgeted refills that preserve surviving root identities. Accepted farm
records can change inferred plot clearances even without a building-component write.
Committed earthworks re-ground local surviving props and refresh local grass batches.
Building mesh LOD selection runs before Bevy's material specialization change detector,
so retained draw bins see a mesh swap in the same frame.
Building and civic-hall upgrades retain the current scene and its door/light wiring
until the replacement asset and its dependencies are loaded; a pending strong handle
keeps that request alive. Initial appearances retain ordinary streaming behavior.

Road presentation paints only the replicated built prefix. Full planned-route tangents
give its Hermite curves stable construction progress and exact surveyed door/junction
anchors. Smooth bounded bends and endpoint wear use spare width inside the accepted
right-of-way; raster coverage clips to each original segment's surveyed capsule.
Simulation paths and land permissions remain the shared surveyed polyline.
Accepted household garden paths join this same terrain compositor through a
separate owner-indexed paint source, clipped to the parcel and its reserved street
approach. They share dirt coverage and the existing upload budget; stone streets
and civic paving retain priority. They do not create additional road simulation
entities or path meshes.
Geometry changes update an index of dense ribbon segments by 64 m terrain chunk;
a repaint reads only that chunk's segments and unions dirt/paving coverage
independently of entity order.
Loaded weightmap handle changes trigger repaint without rebuilding the road index.
Road-bearing maps use inclusive endpoint samples with an explicit shader UV mapping;
removing the last surface restores the exact original cell-centred base bytes and
sampling mode. This guarantees matching road coverage at chunk boundaries, not
identical historic authored terrain pixels on either side of every boundary.

> **Status, reconciled 2026-09-09.** This remains a design record, with implementation state
> marked **[not built]**, **[partial]** or **[done]**. The living-village foundation now
> has stable world identities, one authoritative simulation clock, shared live/lab
> scheduling, region-scoped settlement detail and a global settlement directory.
> The former aggregate offscreen economy is retired by the 2026-09-16 contract in §2. Stable companies now add 1,000-share cap tables, one
> treasury, site cost centres and settlement-local physical branches. Player heroes, boats,
> tactical battalions, formation orders, melee, archers, lab cavalry and catapults are live.
> Strategic armies, political control and world-state persistence remain future work.
> Do not read an unmarked future rule as working code.

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
already does. Budget authoritative simulation and replication separately from client
frame time: lower-end hardware still needs bounded animation, streaming and GPU work.
Neither a server tick measurement nor a draw-call count proves the other budget is healthy.

Ephemeral [server chat](CHAT.md) uses a separate ordered reliable channel. The
server supplies the accepted account identity and applies bounded real-time rate
limits; it broadcasts across regions without querying simulated people. The
client owns only editing, a 100-message local history and retained HUD rendering.
Chat does not create replicated world components or persistent history.

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

## 2. One canonical simulation for the whole world

Every living person and animal uses the same authoritative routines whether observed,
unobserved or running on a server with zero clients. Camera position must not choose an
alternate economy, movement timer, combat resolver or wildlife activation rule. The former
strategic/physical fork is retired: `StrategicPerson`, `StrategicTravel`,
`PendingStrategicDemotion` and region `SimLevel` are not runtime contracts.

The same worker retains the same accepted job, incomplete batch, cargo title, route,
queue/door position, needs and earned payments. A meal still needs collection, a builder
still needs materials and physical arrival, and a carrier still owns an actual load.
Observation cannot erase a transaction or replace a journey with a straight-distance ETA.
The server's ordinary fixed schedule advances this world continuously; clients receive
only the relevant details and render them with independent visual budgets.

This decision removes inconsistent rules; it does **not** establish world-scale capacity.
Use bounded retained route planning, shared road graphs, collision indexes, change-driven
caches and ordinary review intervals to control cost without changing outcomes. No unbounded
A* per actor per tick, whole-world rescans for a local decision or observer-triggered restart
is acceptable. Existing performance figures from the retired aggregate layer are historical.
Fresh headless liveness, matched observation schedules and isolated load measurements remain
required. [SIMULATION-PARITY.md](SIMULATION-PARITY.md) records the audit and evidence limits.

## 3. Regions are spatial and network indexes

`RegionCoord` is the 512 m interest partition. `RegionRegistry` tracks observer counts
for replication; `update_region_observers` changes coverage, not actor eligibility.
An unobserved region remains alive. There is no region simulation level or secondary
strategic clock.

- **Settlements** are the political and persistence unit. Ownership lives on the
  settlement; territory is computed from it. Full world-state persistence is still planned.
- **Regions** index network interest and spatial summaries. They need not become ECS entities.
- **64 m terrain chunks** stream terrain and static geometry; finer spatial hashes support
  collision, melee and ranged broad phases. These indexes need not share a cell size.

Static-prop collider streaming takes the union of neighborhoods around distinct actor and
building `PlayerPosition` chunks, rather than requiring a camera. Long-route surveys also
resolve relevant procedural props outside currently streamed chunks. Actor support, collider
warm-up and queue fairness must be tested without observers; bounded chunk processing alone
does not prove equal first-frame collision outcomes or adequate throughput.

## 4. What seamless zoom demands

This is the most technically demanding choice on the board. It requires:

- **Every entity has a representation at every zoom band.** A soldier is a model up
  close, part of an instanced blob at medium range, and a contribution to an army icon
  far out. Nothing may simply vanish.
- **Rendering LOD changes presentation only.** Models, proxies, icons and network interest
  may vary with the camera. The same world simulation continues when nobody renders it.
- **Transitions must not pop.** Cross-fade or match silhouettes across LOD bands.
  **[partial]** — terrain/water streaming, prop LOD and building mesh LOD have dedicated implementations;
  characters already have a full-rig/proxy split. Smoothness across moving zoom bands
  still needs renderer evidence; this document does not establish a current popping defect.
- **Authored building LOD** lives in `client/src/render/building_lod/`: asset catalog,
  scene binding, and screen-size selection have separate owners. All 19 village variants
  retain the source scene and switch between full and reduced meshes at 120 projected
  pixels, hiding below 4 pixels, with 12% hysteresis. Selection runs at 10 Hz after
  transform propagation and before Bevy propagates visibility and recalculates mesh
  bounds. Doors, materials, window glow, windmill mechanisms,
  collision and NPC anchors keep their existing ownership. Source replacements and hot
  reload rebind the primitive entities. See [asset build/verification](../asset_creation/BUILDING_LODS.md).
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

**The remaining gap is strategic representation.** Camera range, a world-map Hero
marker/camera footprint and the dense-character rig/proxy split already exist. Readable
settlement, army and caravan symbols, trade lines and political overlays remain open.
These should use appropriate summaries rather than keeping every distant unit fully
replicated or rendered just to draw a map icon. Their authoritative simulation continues.

Rough bands to design against:

```
   5m   individual character, animation, gear
 100m   squads and formations, town streets
   1km  armies as instanced blobs, settlement models
  10km  region borders, clan colours, trade-route lines
```

## 5. What an always-on persistent world demands

- **The same world simulation runs everywhere, continuously.** Budget navigation,
  movement, collisions, work, needs and decisions together; measure their complete cost.
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

Performance remains **uncertified for the canonical world simulation**. The 2026-08-05
5,000-person/30-settlement reference used aggregate offscreen work (0.881 ms steady,
1.446 ms daily burst on that development machine). It cannot be extrapolated to full
physical routes, queues, needs and production everywhere. Retain dated results as historical
context, then measure fresh no-client, observed and mixed-interest workloads with actual
routes and growing towns. Record clock delivery and backlog growth, not only average tick
cost. Never obtain a passing benchmark by restoring a cheaper set of offscreen game rules.

## 6. What already exists and fits

- `shared/src/city` — **[partial, and read the caveat]**. The *geometry* layer (road
  strips, plot rects, frontage and facing math) is excellent, tested, genre-neutral and the
  single most reusable asset in the repo. The *taxonomy* is not: `RoadClass` is
  Alley/Local/Collector/Arterial with lane counts and parking, `PlotZone` is modern land-use
  zoning, and `CityBuildingKind` is nine modern apartment blocks. The medieval GLBs exist on
  disk and in `BuildingType` but no plot can reference them. The shipped map also has
  `roads: []` and `plots: []`, so none of this pipeline has ever run on the current world.
- `server/src/world/navgrid.rs` and village routing — **[live, bounded local and regional travel]**.
  The navgrid keeps the shared building obstacle index current; village travel, trades,
  ambient movement, construction and hero steps query it. `village_roads` owns the
  obstacle surveys, locally invalidated route cache, resumable long-route searches and
  road graph. Committed journeys use a priority lane ahead of ambient wandering; mass
  migration admission is paced in real time, nearby migrants can join a certified cohort
  approach, and Moot queue-rank changes use short local steps instead of global A*. Building
  and streamed-prop changes invalidate only intersecting route-cache entries. Repeated
  blocked-goal warnings are spatially and temporally coalesced, while the final live
  collision proof remains authoritative. Route certification checks props through the
  destination instead of exempting its final two metres. Completed ports contribute the
  same authored office, cargo, post and rail solids to the retained planner cache and
  live movement index, preserving their open walking lane. Permitted worksite
  footprints block road surveys before their shells exist. `pathfinding.rs` contains the wall-clock budget settings
  used by the live queue; the retired generic `find_path` implementation is gone.
  Large commanded groups still require regional flow fields rather than
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
  distant person. This rendering LOD does not change server execution.
- Stable `PersonId`, `SettlementId` and `BuildingId` relationships, global settlement
  summaries plus region-scoped physical/economic detail. **[implemented]**
  Offscreen people use the same production, needs and transaction routines.
- The live server and Village Lab share one ordered village registration. Its
  core is explicitly partitioned into identity/population, civic,
  economy/planning, construction, activity and directory sets, with nested
  economy sets for markets, households, settlement accounts and permits. These
  are scheduling/telemetry boundaries, not duplicated implementations.
- ~~The commander camera, which needs its zoom range extended by ~20×.~~ **[done]** —
  12m–12,000m, which covers the whole map.

**Implementation limits:** the new RTS melee and tactical battalion systems live in
`server/src/player/combat.rs`, `army.rs` and `orders.rs`; they do not restore the removed first-person
combat. Regional trade routes and embodied caravan work are implemented, but strategic
military campaigns, clans, political territory and world-state persistence remain future
work. Heroes and their possessions survive disconnects within a running server session;
restarting the server creates a fresh world.

### Tactical command and formation boundary (2026-09-06)

One ordered `UnitOrder` stream owns move/attack/hold intent. Membership edits commit
sequentially before the next edit is validated. Durable battalion IDs compact complete
selections; any named member also expands to the owned battalion on the server.
Unassigned individual entities are mapped only in messages. Shared pure geometry drives
both preview and authority, with `PersonId` tie-breaks. `EngagedWith(PersonId)` carries
confirmed targeting to client presentation.

`player/orders/navigation.rs` owns bounded shared formation fields and certified routes;
`hero::step_units` remains the only marching position integrator. Civilian road planners
do not own commanded formations. The client derives its army roster only when membership,
identity, formation preferences or vitals change.
An individual, unmounted hero's plain move uses `NavigationRoutePending` and the existing
incremental road planner instead of a formation field. Long personal trips receive its
bounded regional survey window and reuse road connectivity; a battlefield grid that grows
coarser with journey length cannot reliably represent village exits and river detours.
Regional trips over 512m use a wider 768m detour margin and a 12m middle lattice, with
fine endpoint cells and the existing 24,000-node ceiling. Coarse cells share one aligned
grid; every candidate edge and the completed route still receive collision checks.
Cold deterministic prop chunks are prepared across planner ticks before building the
route's collision snapshot. Visible-path simplification probes progressively farther
points and refines failed spans, avoiding quadratic resampling of long clear stretches.
Swimming, mounted units, battalions and explicit formation/attack-move orders retain their
own movement contracts. Changing an order clears the prior route and pending state before
the new intent commits.
`BattalionFormation` retains preferred files/spacing; sparse `FormationSeat` offsets
keep rank assignment consistent despite client/server walking delay. The drag preview
caches layouts and retains its footprint/count UI while the pointer is stationary.

`player/orders/attack.rs` assigns selected battalions to the nearby enemy line at
the command boundary; a focus modifier keeps the clicked objective.
`player/combat/fronts` retains battalion intent and quiet deployment files;
`fronts/steering.rs` chooses individual combat approaches with bounded local
avoidance. Distant approaches preserve files; screened reserves follow the person
ahead until nearby contact or a clear approach releases them. This replaces rectangular enemy-face
reservations and section bends. Supporting soldiers wait or sidestep instead of
being forced into exact combat slots. Existing fights have separation priority.
Contact-point decisions are staggered and briefly retained, while death invalidates
them immediately; local motion does not rescore every possible approach each tick.
`combat/skirmish` owns unassigned individuals’ approaches. Both use the shared
local body index and authoritative mover. Survivors regroup after local combat
has remained quiet; direct movement immediately replaces combat intent.
Server-clock attack/reaction components drive client-authored clips, and mortality
settles immediately while a marked fatal body remains briefly for its fall.
See [COMBAT-DESIGN.md](COMBAT-DESIGN.md) for budgets and limits.

## 7. Build order

**Moved to [ROADMAP.md](ROADMAP.md).** This section and WORLD-DESIGN §8 used to carry two
different orderings, and they disagreed about when the promotion seam and flow fields land.
One list now covers both, with per-phase checklists.

The old strategic-tick and promotion/demotion milestones are superseded by the canonical
simulation decision in §2. Bounded local/regional navigation, shared formation fields,
physical production, trade, civic construction and network interest are implemented.
Whole-world correctness and load capacity still need the evidence in
[SIMULATION-PARITY.md](SIMULATION-PARITY.md); a previous aggregate benchmark or finite
transaction test does not complete that work. Rendering LOD and art improvements remain
independent of this simulation contract.

## First construction and work presentation

Personal inventories use 24 bulk (six Wood); porter inventories use 144 bulk
(36 Wood). Stock, purchase reservations and money remain authoritative. Capacity
is a bulk limit, not a guarantee of that many units of every good.

Construction self-supply searches a bounded nearby tree pool from the carrier's
current position. It excludes known cleared trees, failed approaches and trees
claimed by other construction workers. Route failures retain bounded backoff;
nearest-tree preference must not create an unbounded per-tick path search.
Builders and woodcutters reach a collision-safe trunk stand within 0.35 m before
chopping. Road crews have a separate eight-second clearance task; clearing an
obstruction does not also manufacture saleable timber.
The road routine starts known tree clearance before processing a failed road
waypoint, including failures published in the same tick as the previous section.
`village::workplace_access` releases waiting employees to ambient life and resumes
ordinary assignment when their connector opens. That temporary wait must not
consume their entire workday or reopen a genuinely completed shift.
Freight service uses a completed Market entrance or the actual Hall entrance;
a reserved Market plot is never a pickup counter.

An assigned hero participates in construction objective/navigation replication.
When any tactical character loses its movement target, its motion is settled;
otherwise stale hero velocity can keep a work animation looking like walking.
Stationary chopping/building/mining/farming/fishing takes precedence over a held
cargo pose. The bundle is stowed visually while its authoritative inventory remains.

## 8. Living-world implementation rules

The current village simulation uses these rules as hard boundaries:

- **Identity is data, names are labels.** `PersonId`, `HouseholdId`, `SettlementId`, `BuildingId` and `CompanyId` are
  authoritative across regions, payroll, ownership, employment, housing, UI commands and
  serialized relationships. Legacy name rosters remain for readable panels and old-state
  migration only; the versioned world-state file itself is still a roadmap item.
- **There is one simulation clock.** `SimulationDelta` captures real seconds, world
  seconds and warp once at the start of the shared tick; `SimulationTime` is the read-only
  system parameter used by gameplay. No gameplay system multiplies `Time` by `TimeWarp`
  independently. Shared work schedules account for shift overlap at high warp; productive
  and travel state must not bank blocked time or manufacture catch-up output. At 1x the display clock is linear: one real second is one
  world minute and a complete day is 24 real minutes. Sunrise is 05:00 and sunset is
  23:00; only the sun's below-horizon arc accelerates through the six-hour night. NPC
  schedules use explicit clock hours (ordinary work is 06:00-18:00), never a fraction of
  the lighting arc. The authoritative Bevy fixed schedule and network protocol both use
  the shared 60 Hz constant. `ServerPerf` reports `clock_delivery`; sustained values below
  100% mean the fixed schedule is dropping elapsed time under overload rather than a game
  mechanic intentionally slowing the clock.
- **The live game and Village Lab share one ordered schedule.** Add village behaviour to
  `server/src/world/village/schedule.rs`; do not maintain a second hand-copied lab list.
- **Summary and detail are different entities.** `SettlementSummary` is tiny and global.
  Halls, buildings, worksites, roads, fields, piers, markets and inventories carry
  `RegionCoord` and replicate only through interest management. They join by stable id.
- **Observation changes no game rules.** Retain the same worker routines, actual movement
  targets, certified routes, cargo, door timers, service queues and work progress everywhere.
  New project admission uses the shared activity boundary and respects existing journeys.
  No camera-gated dispatcher, aggregate substitute or straight-ETA position assignment.
- **Village domains have explicit owners.** `village.rs` is the public facade and shared
  state model; migration and resident counts live in `village/population.rs`, demand and
  geography-aware permits behind `village/planning.rs` and its focused `planning/` modules,
  material supply and building work in
  `village/construction.rs`, vacancy matching in `village/employment.rs`, physical Moot
  Steward collection in `village/commerce.rs`, municipal budgets/payroll and the enacted
  relief/reserve/staffing/subsidy policy model in
  `village/civic.rs`, business transaction settlement and
  owner strategy/solvency in `village/businesses/`, bounded firm/market/settlement
  archives in `village/history.rs`, legal firms, pooled treasuries, shares and consolidated
  ledgers in `village/companies.rs`, aggregate food/prosperity in
  `village/settlement_economy.rs`, household membership in `village/households/membership.rs`,
  procurement and scheduled household budgets in `households/provisioning.rs`,
  hearth consumption and contribution math in `households/needs.rs`, physical
  household cargo in `households/shopping.rs` and home schedules in `households.rs`,
  physical trades in `village/trades.rs`, production rates in `village/production.rs`,
  shared activity ownership in `village/worker_activity.rs`, and ordering in `village/schedule.rs`.
  `village_roads.rs` owns the local survey primitives and public road state; connector
  construction lives in `village_roads/construction.rs`, graph routing and caches in
  `village_roads/routing.rs`, the Moot Steward's civic repair duty in
  `village_roads/steward.rs`, and width/dryness
  geometry in `village_roads/geometry.rs`. Keep extending those seams instead of growing
  either facade into a monolith. Persistent residential wards and reserved public
  ground live behind `planning/districts.rs` and `planning/reservations.rs`.
- **Defenses share one geometric contract.** `shared/components/fortifications.rs`
  defines immutable accepted circuits and region-scoped physical sections.
  `world/fortifications/` owns surveying, paid hauling/construction and diagnostic
  passage tracing. Completed walls feed both the authoritative obstacle grid and
  the village route cache. Hero movement and combat separation enforce these
  walls even where ordinary building collision has exemptions. Archery maintains
  a changed-section spatial cache for wall bodies and overhead gate beams.
  `client/settlement/fortifications/` renders one closed mesh per section and
  bounds mesh construction to eight sections per frame. Reservations alone do
  not block walking. Gates currently stay open; closing and destruction need
  their own authoritative state. See [FORTIFICATIONS.md](FORTIFICATIONS.md).
- **UI chrome is a shared foundation, not screen-local behavior.** `ui/styles.rs` owns the
  palette, `ui/foundation.rs` owns semantic layers, type scale, buttons, disabled/focus state
  and live-panel refresh safety, `ui/modal.rs` owns the one-scrim modal structure, and
  `ui/scroll.rs` owns nested wheel bubbling. Economy screens preserve the entity under the
  pointer and bound structural refresh at high simulation speed. See
  [UI-ARCHITECTURE.md](UI-ARCHITECTURE.md).

### Siege units

Catapults use `player::siege` for authoritative orders, firing and splash damage and
`client::siege` for presentation. Their footprint uses the existing formation route
budget and mover. Replicated stones contain absolute launch/impact timelines; damage
resolves once on the server. See [CATAPULT.md](CATAPULT.md) for controls, ownership,
scaling and the current developer-placement boundary.

### Standing army policies

`army/response.rs` owns standing battalion policies and event-driven bombardment
responses. Policy replicates separately from a transient move/attack objective;
membership edits propagate the destination policy. Responses reuse `orders` and its
bounded formation routes. The retained Army page under `ui/encyclopedia/army/` separates
pure roster/action models, layout, binding and input. See COMBAT-DESIGN.md for priority.

### Archer combat

`player/archery` owns equipment validation, weapon transitions, staggered ranged
targeting, baked convex collision queries and swept arrow impacts. Its systems run
in the shared Navigation chain: weapon selection before formation steering, firing
and impacts after movement and melee. Shared launch/shot timelines drive cached
client bow graphs and arrow scenes. The coarse ranged body grid supplements the
existing melee grid. See [ARCHERY.md](ARCHERY.md) for contracts and limits.

### Wildlife

`world::wildlife` owns seed-ordered meadow-herd placement and authoritative
wandering everywhere, including on a server with no clients. Wildlife bodies
anchor a bounded collider neighborhood; movement checks swept-footprint readiness
before advancing. Observation only affects client presentation:
`client::animals` owns the shared horse graph and a 32-rig budget.
Breeding, migration and disk durability are future contracts. See
[WILDLIFE.md](WILDLIFE.md) for limits and capture recipes.

### Cavalry

`player::riding` pairs cavalry soldiers with provisioned horse equipment. Cavalry
orders use the ordinary authoritative movement/combat pipeline with role-aware
shared formation dimensions and body clearance. Mounted-pair reconciliation reuses
scratch indices. Issued horses share wildlife IDs but have separate population and
rig budgets. `client::hero::mounted` owns socket attachment and masked riding/melee
animation; the person root remains at ground level for selection. See
[CAVALRY.md](CAVALRY.md) for the lab launch and current gameplay limits.

### Stable household accounts

A household is a separate regional entity with `HouseholdId`, `HouseholdMembers`
and its one `HouseholdEconomy` purse. Person and dwelling links reference that ID;
the house's `Household` roster is a derived occupancy view. Physical inventories
and partial hearth energy stay at the building. Membership reconciliation is
change-driven, and procurement has a once-per-world-minute entry gate with
staggered per-household deadlines. Active shopping caches the account and home
entities while validating stable ownership; it performs no global account lookup
per shopper/frame. See [HOUSEHOLD-ECONOMY.md](HOUSEHOLD-ECONOMY.md).

Owner-funded extensions in `world/house_upgrades/` retain this home and account
identity. A separate replicated worksite describes material delivery and paid
work, while private project escrow and transit goods remain authoritative server
state and participate in economic audits. Completion changes `HouseAppearance`
in place, raising physical capacity from four to eight and triggering the normal
membership and yard reconciliation. Worker recruitment is globally bounded to
one roster search per fixed tick; daily investment candidates are staggered.
See [HOUSE-UPGRADES.md](HOUSE-UPGRADES.md).

Ordinary self-supplied construction joins a plot's reserved access near the worksite
instead of always taking freshly cut timber past the Hall. The approach uses at most
four corridor-join probes and the existing budgeted navigation queue. If an uncleared
trunk blocks the reserved delivery apron, at most 41 local point probes select a dry,
collision-clear stand within the same reserved frontage and the existing 2.5 m work
reach. The person must physically finish the approach before transferring materials;
the final reached stand also anchors subsequent building work. A reserved corridor is
rechecked against live obstructions before reuse. If every local stand is blocked,
the load stays on its carrier and access is reconsidered after 60 world seconds;
the reservation neither clears trees nor grants passage through them. A blocked
loaded delivery releases its freight-counter ticket without discarding its Wood.
Private delivery state remembers the entry actually reached so empty return trips
reuse the same corridor prefix. Purchased
upgrade materials still require a real pickup from Hall stock.

### Settlement development

`village/development_evidence.rs` aggregates living residents, valid occupied homes,
dated private business activity and completed Market access once per world day.
`settlement_development/progression.rs` consumes those compact summaries, qualifies
two of the last three completed dates and retains the funded Hall construction
pipeline. A missing observation never inherits the present state. Population and
occupied housing establish Village eligibility; Town adds operating commerce.
Wellbeing remains separately visible in the economy summary. No per-person timers
or new pathfinding are added. See [SETTLEMENT-DEVELOPMENT.md](SETTLEMENT-DEVELOPMENT.md).

### Bounded labour and market decisions (2026-09-15)

See [WORKER-ACTIVITIES.md](WORKER-ACTIVITIES.md) for the shared activity-admission, production-lifecycle and work-hour contracts, their trade-specific boundaries and required validation.

`village/economy.rs` reviews private wage offers daily after payroll, reserving all
company sites before allocating spare cash in stable building order. Reserves include
the larger of each site's enabled positions and current roster, input needs and debts.
`village/civic_labor.rs` owns hourly public/private vacancy observations and safe
public job changes. `village/employment.rs` retries private employee choices hourly
after cargo or other committed work clears, with one completed review per person/day.
Both use indexed offers; movement remains owned by the existing navigation systems.
`shared/economy/demand.rs` stores at most twelve bid bands per good/day. Overflow
uses canonical power-of-two buckets rounded down, independent of insertion order;
claims withdraw through their original bid and expire with their ledger epoch.
Focused `development_market/investment.rs`, `mortality/takeovers.rs` and
`trade_routes/{merchant_economics,civic_review}.rs` own investment, resale and trade
review math, using market/company snapshots and bounded quote candidates.

`village/commerce/payroll_claims.rs` retains server-only named private creditors,
while `BusinessAccount.wage_arrears` remains the replicated aggregate liability.
Job changes preserve the earned claim, and liquidation/takeover transfer that same
ledger. `mortality/payroll.rs` indexes claims only when deaths occur, paying available
cash into the worker's estate and recording any unpaid remainder as a default.
These are living-world records; durable disk saves still need a versioned format.
The replicated demand layout changes the protocol and requires matching rebuilt
client/server binaries and a coordinated restart.
