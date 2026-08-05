# Roadmap

The single build order. [ARCHITECTURE.md](ARCHITECTURE.md) says how the engine carries the
game; [WORLD-DESIGN.md](WORLD-DESIGN.md) says what the world *is*. Both used to carry their
own phase list, and the two disagreed — this file replaces both.

Written 2026-07-30 against a full audit of the code, not against the previous docs. Where a
doc claim and the code disagreed, the code won and the doc was corrected.

**Ordering principle: risk first.** Phases are ordered by which unknown, discovered late,
would invalidate the most already-built work — not by narrative order. The big reordering
against the old lists: the promotion/demotion seam moves from position 4 to position 2, and
flow-field pathfinding moves from position 3 to position 6. Reasons in each phase.

**Every phase ends in something a player can do.** A phase whose only output is
infrastructure is a phase that cannot be tested.

---

## Status at a glance

| Phase | Name | Size | State |
|---|---|---|---|
| 0 | Let me in | M | in progress |
| 1 | The world remembers | L | in progress — stable identity, settlement directory, founding, picking and panels are live; world-state persistence is not |
| 2 | The seam | L | in progress — ordinary villagers now demote to aggregate strategic work; the traveller/army promotion contract is not built |
| 3 | They eat | M | in progress — physical food, daily consumption, prosperity and Hamlet → Village are live; births, decline and later tiers are not |
| 4 | Prices and the hand cart | M | in progress — NPC wallets and local Moot prices are live; player trading and carts are not |
| 5 | Caravans | L | not started |
| 6 | Command | XL | not started |
| 7 | Retinue and businesses | L | not started |
| 8 | Clans and territory | L | not started |
| 9 | War for the realm | XL | not started |

---

## Phase 0 — Let me in

**Playable:** a stranger joins the hosted server with `FISTWORLD_DEV` unset, gets a body,
and walks it around a world that is still there tomorrow.

Today none of that is true. The only path to a body is a god command, and the hosted server
cannot boot — so every "playable" claim in the old build order was really a dev-mode claim
on a local binary.

- [x] Clamp client-supplied `view_radius` (was a one-message remote OOM)
- [x] Fix the Docker build (workspace members `editor` + `tools/terrain_ktx_builder` unstubbed)
- [x] Copy map assets into the image (server panicked at boot without them)
- [x] Decouple the strategic tick rate from time warp (ran 60x/sec at 100x warp)
- [x] Add a release-only 5,000-resident / 30-settlement scale lab and remove
      unchanged household, field and work-routine reconciliation from the hot
      path. The current village bundle is within one 60 Hz tick on the reference
      machine; Phase 2 now also compresses ordinary off-screen villagers, while
      traveller and army promotion remain separate work.
- [x] Fix the interest cache that could never hit (~4k-entry set rebuilt at 60Hz per client)
- [x] Stop replicating the whole world to connected-but-unnamed clients
- [x] Persist heroes across disconnect and server restart
- [ ] **Add a `[mounts]` volume to `fly.toml`** — `server_data/` is on ephemeral storage
      today, so every deploy wipes every profile. Hero persistence is meaningless on the
      hosted server until this lands. Highest-value remaining item in the phase.
- [ ] A non-dev spawn path: move hero creation out of the god panel and out of `DevCommand`
- [ ] Decide and document what a player sees before they have a hero
- [ ] Redeploy and verify a real client can join, get a body, disconnect, and return

**Exit:** a non-dev client on the hosted server spawns a hero, walks it, disconnects,
and finds it again after a redeploy.

---

## Phase 1 — The world remembers

**Playable:** explore an 8km world with ~30 named settlements in it. See them on the map,
walk to one, and inspect it.

Three shapes get decided here while their payload is still trivial, because all three are
join keys or on-disk contracts that are ruinous to change later.

- [x] **Stable identity.** `PersonId`, `SettlementId`, `BuildingId` and an allocator.
      Ownership, employment, housing, civic work, adjunct fields/piers, UI commands and
      the settlement directory join on ids; names remain display/legacy-migration data.
      The allocator observes loaded ids before issuing another, ready for the versioned
      world-state file below.
- [ ] **World-state file, versioned and self-describing.** NOT bincode: it is positional,
      which is exactly why `PROFILE_VERSION` is at 7 and the profile loader has an explicit
      v6 migration plus reject-and-backup for unknown layouts. A wipe is an inconvenience for an outfit and fatal for months of
      settlement state. RON is already a workspace dependency.
- [ ] **Write the v1 to v2 migration before there is anything to lose.** A migration path
      that is never exercised is the one that fails when it matters.
- [ ] Route world saves through the existing background IO worker
      (`server/src/persistence/io_queue.rs`), not the main thread
- [ ] Backup rotation and load-newest-valid-on-corrupt for the world file
- [x] **The replication split:** one globally replicated `SettlementSummary` entity and
      `RegionCoord`-scoped halls, buildings, markets, inventories, worksites, roads, fields
      and piers, joined client-side by `SettlementId`.
- [ ] Radius aggregator over `BiomeField::resources` — does not exist in any form, and
      `resources()` has never had a production caller, so validate it discriminates before
      building site scoring on it
- [x] **`Person` and the settlement roster, before anything writes a population
      float.** WORLD-DESIGN 1a makes population a roster of named people rather
      than a number, and retrofitting that later means tearing out every
      consumer of `population: f32`. It is ~24 bytes a head and the name model
      already exists, so there is no reason to defer it.
- [ ] The three LOCATION STATES from WORLD-DESIGN 1a -- AtPlace, Travelling
      (position derived, not stepped) and Embodied -- decided here even if only
      AtPlace is populated at first. Everyone must always have a knowable
      position; only bodies are conditional on observation.
- [x] **Founding as an act: the moot hall.** `DevCommand::FoundSettlement`
      places a hall and a settlement exists. Spacing, water and naming all
      enforced server-side. No founder is recorded — a world- or god-spawned
      settlement has none, and the hall belongs to the moot.
- [ ] Founders: a hall with an empty roster is a SITE, not a hamlet. Player
      foundings need pioneers brought to them; world-seeded ones start with
      deterministic households.
- [ ] Naming in the UI, for the case where a PLAYER founds: they type it,
      `place_name` only suggests. Needs a text field the UI does not have yet.
      Today founding always sends an empty name and the server generates one,
      which is the correct behaviour for world and god foundings and a gap only
      for player ones.
- [ ] Deterministic settlement site selection from the seed (for the world's
      OWN settlements; player founding does not need it)
- [ ] Map markers (there is no marker layer today; the map's only marker is bound to a
      component nothing inserts)
- [x] Screen-space picking so a settlement can be clicked. Halls opt into
      `Selectable` with a building-sized hit shape; a place is never commandable,
      so selecting one never produces an order.
- [x] Inspect panel. `client/src/ui/settlement_panel.rs` — name, tier, residents
      by name, treasury, what stands (with owners), what is going up, permit
      prices. Contains no controls, because the village decides for itself.
- [x] PLACES tab in the encyclopedia: every known settlement, with bearing and
      distance from the player.

**The autonomous village slice** (WORLD-DESIGN §1b — a whole experiment, run to
answer "can a village run itself?" before any of the economy above exists):

- [x] Villagers start unhoused, unemployed and resident nowhere. God mode spawns
      people; it never places them.
- [x] Uncommitted villagers find the nearest non-Ruins settlement and walk to its
      hall on the real terrain. Arriving makes them residents.
- [x] Resident count RE-DERIVED from the roster every tick, never incremented on
      arrival — a nudged counter drifts, and a population that disagrees with the
      people standing there is the lie the encyclopedia must never tell.
- [x] `Residence` replicated per person, so a panel can name who lives where.
- [x] Concurrent bootstrap permits; needs begin in strict order (food source →
      Lumberjack Hut → enough Houses) counting BUILT and PLANNED alike. The food source
      becomes a Fisherman's Hut where its pier can reach valid open water,
      otherwise a Farmstead. Successive
      decision ticks may reserve different collision-safe plots while earlier
      worksites are still being supplied or built. Housing repeats until every
      resident has a bed; measured food shortage can repeat Farmsteads, and a
      viable shoreline settlement can ultimately support both farm and fish.
- [x] A resident applies and becomes the building's owner, by name — whoever
      holds the fewest already, so each person has a stake rather than one
      villager owning the whole place. No residents, no permits: a foundation
      does not build itself. Needed housing permits are free; every business
      permit debits the applicant's wallet into the settlement treasury, with a
      need discount and progressively higher prices for repeat holdings.
- [x] Deterministic ring siting: 12 bearings, 6m rings, rejecting slope, water
      and overlap. Houses ring close, work buildings far.
- [x] Geometry-led coastal siting: the whole Fisherman's Hut and side route stay
      dry, the authored pier side rotates seaward, and its work end must reach
      broad submerged water. Halls use footprint-wide water checks and may sit
      close enough to shore for a coherent port town.
- [x] Water refused at BOTH founding and siting. A lake bed is the flattest
      ground in reach, so a slope test alone steers a village into the water.
- [x] Permit → bounded Wood worksite → builder time → building, so "under
      construction" is a physical state the panel can honestly show. Farmsteads
      and Fisherman's Huts require 12 Wood; Lumberjack Huts and cabins require 10.
- [x] Physical construction supply. The builder buys Wood from the Moot when
      stock and their wallet allow, but a settlement without affordable stock can bootstrap
      by chopping real trees, carrying bounded loads, and depositing them at the
      site. Raising cannot start until the last required unit arrives; completion
      consumes the committed Wood. Market Wood is a real wallet-to-pool purchase.
- [x] End-to-end test over the real scheduled systems:
      `village::tests::three_villagers_settle_and_build_a_village_unaided`.
- [x] Bounded bulk inventories on villagers, completed buildings and the hall.
      Five physical goods share capacity; coin is a separate fixed-point ledger.
- [x] Occupations and bounded workplace slots. Farmsteads employ Farmers,
      Fisherman's Huts employ Fishers, Lumberjack Huts employ a Woodcutter, and
      Houses employ nobody.
- [x] First observed production loop: hut door → indoors → real tree → chop
      animation → bounded carried load → hut deposit → Moot sale. Realised sale
      proceeds split 80% to worker and 20% to the workplace owner.
- [x] Farmstead → two authored nearby wheat fields → visible field work → bounded
      wheat carry → Farmstead deposit → hall haul under storage pressure.
- [x] Fisherman's Hut → authored paired pier → safe over-water deck traversal →
      placeholder visible work → bounded Food carry → hut deposit → hall haul
      under storage pressure. Hut and pier use authored-anchor lighting after dark.
- [x] Completed buildings and active worksites are selectable. Houses expose
      owner, beds and storage; workplaces expose owner, quality, workers and
      inventory; the hall exposes Moot stock, liquidity and quotes. Worksites show delivered and
      required Wood in the panel and as visible timber bundles in the world. The
      encyclopedia retains the same completed-building snapshot in an expandable
      settlement/building explorer with a dedicated detail sheet per record.
- [x] Capacity-bounded designated households. At sunset villagers interrupt
      work, open their own cabin's authored door, walk through it and sleep;
      sunrise opens the door before they emerge. Shared door demand holds one
      animation open for a group, then closes it once. Non-empty designated
      households fade warm emissive panes and tight window-anchored light pools
      after dark, derived from the cabin's own replicated roster; empty cabins
      and daylight stay dark.
- [x] Cheap ambient life for unemployed or unhoused residents. Only observed
      tactical regions receive deterministic walking/resting orders; villagers
      reuse the bounded road route queue, sit on collision-checked path verges,
      and use the authored seated loop. A shared 4Hz real-time decision pass and
      cached gathering geometry keep cost independent of frame rate and warp;
      unobserved regions receive no ambient movement work. Ordinary residents
      now demote to durable strategic records; traveller and army round trips
      remain Phase 2 work.
- [x] Builder-made local paths. The person who finishes a building surveys from
      its authored door to the closest existing village path (or hall door),
      visibly builds the route in sections, pauses for their household at night,
      and returns to ordinary residency only when it is complete.
- [x] Bounded one-time road survey and cached travel graph. A 1.5m-grid A* avoids
      buildings, deterministic trees/rocks, water and steep steps once per new
      building; ordinary villagers share cached graph routes and get a modest
      speed preference on completed paths. This is local infrastructure, not the
      regional caravan graph in Phase 5 or the group flow fields in Phase 6.
- [x] Obstacle-safe road connectors for ordinary trips. A changed destination
      queues one budgeted local survey, compares its safe direct path with entry
      and exit connectors through the cached road graph, and waits for that answer
      instead of taking a speculative straight step. Movement rechecks the live
      building and baked prop broadphases every 20cm, including at 100x.
- [x] Door-safe obstacle registration. The derived moot hall participates in
      the same building/nav indexes as completed houses and workplaces, and a
      forced short front apron makes every surveyed path enter its destination
      from the authored door rather than finding a side or rear shortcut.
- [x] Progressive path rendering and surgical clearance. Each path is one
      terrain-following crowned ribbon mesh with naturally varied width and worn
      vertex colour. Only the completed prefix clears grass, flowers and shrubs;
      mature trees and rocks remain because the survey routed around them, and
      later permit siting treats completed paths as occupied infrastructure. The
      render ribbon resamples the compact route at 45cm intervals so terrain
      triangles cannot crest through it without increasing network data.
- [x] 100x headless village soak: migration, permits, incremental construction
      supply, construction, staffing, indoor/work/carry states and both production
      loops run through the real systems.
      Every current embodied village timer, movement step and door threshold
      honors the full `TimeWarp`. A focused 100x sunset/sunrise test verifies
      authoritative threshold crossing even when presentation is shorter than
      a network snapshot.
- [x] Dedicated 1km `village_lab` map and `cargo village-lab` regression
      harness. Its default 190-minute scenario runs one fixed-seed meadow village with
      eight founders plus eight day-2 migrants. The opt-in `dual` scenario adds frozen
      inland Coldbarrow. Both use the real collider/navigation/economy stack, report
      expansion and inventories, detect per-villager stalls, and verify
      that every completed building builds its own door connector to the finished
      path network, including very short connectors beside an existing road.
- [x] Village Lab food-secure and food-poor controls. The meadow must build both
      a Farmstead and Fisherman's Hut, feed eight residents, sustain three secure
      days and advance to Village. Low-yield, non-coastal Coldbarrow must record
      hunger, request repeated Farmsteads and remain a Hamlet. The dual contract
      passes at 100x and 500x. Secure/poor aliases isolate either half.
- [x] Focused 100x road acceptance test proves the original building owner keeps
      the task, exposes the build animation, joins the exact two door/network
      anchors and completes. A separate 100x movement test proves ordinary
      travellers consume multiple cached graph points within one server tick.
      Route tests also require villagers to select a safe road around a building
      and prove a single 100x step cannot tunnel through a house or tree collider.
- [ ] Persistent tree nodes, depletion and regrowth. The worker selects a real
      deterministic tree today but does not remove it after harvesting.
- [ ] Founding costs something. Free is fine while only god mode can found and
      wrong the moment ordinary players can.
- [x] Villager wallets and settlement treasury movement. Every new villager has
      10.00 coin; needed housing permits remain free, business permits always
      cost coin, and the panel states both rules and current balances.
- [x] Explicit Poor Relief policy. Disabled settlements let insolvent residents
      go hungry; enabled settlements buy their ration from public treasury coin
      only when recent production covers the population and the purchase leaves
      a three-day emergency reserve.
      while both Moot stock and funds last. The panel and encyclopedia expose it.
- [x] First tier advancement: a Hamlet with at least four residents becomes a
      Village after three consecutive days with three reserve days, recent food
      production covering population, no hunger and prosperity at least 65.
      Later tiers and all regression remain Phase 3 work.

**Deliberately deferred:** a billboard/impostor/symbol renderer. Settlements read as
screen-projected UI labels, using the world-to-panel projection the map already has. That
defers the entity-LOD problem until something actually needs it.

**Exit:** ~30 settlements persist across a server restart with correct ids; the map shows
them; clicking one opens a panel with real data.

---

## Phase 2 — The seam

**Playable:** watch a traveller cross the map as an icon, zoom in and see it become a
person walking real terrain around a bay, zoom out and see it lose nothing.

ARCHITECTURE's own closing line says step 4 "is where the design actually gets tested, so
do not leave it until last" — and then the old build order stacked four phases of economy
on top of a seam it never validated. This phase pulls it forward and tests it on the
cheapest entity that has one.

The insight that makes it cheap: **the first formula-vs-observed test needs no combat.**
Use *arrival time*. A strategic leg is interpolation along a graph edge; a promoted leg
walks real terrain around obstacles. If tactical travel is systematically slower, players
learn to look away at the right moment — that is the look-away exploit, falsifiable with
one entity and zero combat code.

The first half of this seam is now exercised by ordinary villagers: outside tactical
regions they retain durable identity, household, wallet and employment state, shed routes
and animation phases, and contribute through aggregate workplace production and Moot
commerce. Re-observation rebuilds their embodied routines. This proves the scheduling and
state-shedding mechanism, but it does not satisfy the traveller round-trip or arrival-time
contract below.

- [ ] One strategic traveller entity: position, route, ETA
- [ ] Promotion: strategic entity to a real walking body, deterministic from strategic state
- [ ] Demotion: back to numbers, losing nothing
- [ ] Hysteresis on the transition (promote and demote at different thresholds)
- [ ] Round-trip test: promote, demote, promote again — state must be identical
- [ ] **Arrival-time agreement test:** N runs formula-only vs N runs observed; the
      distributions must overlap
- [ ] Traversability gate (water, slope) — movement currently ignores both
- [ ] A scripted second client that can CHOOSE whether to observe, so the look-away exploit
      is testable at all

**Exit:** the promotion contract is enforced by tests, and observing a traveller does not
change when it arrives.

---

## Phase 3 — They eat

**Playable:** watch a meadows village outgrow a moor one; starve a hamlet down to Ruins.

The strategic tick now advances aggregate off-screen workplace production, porter commerce
and household purchasing. Tactical villagers retain the visible per-trip loops; unobserved
ordinary residents shed paths, door choreography and work-animation phases.

- [x] Work slots on built plots, and people filling them
- [x] One quality-scaled observed Wood loop from a filled Lumberjack Hut slot
- [x] One quality-scaled observed Wheat loop from filled Farmstead slots
- [x] One quality-scaled direct Food loop from filled Fisherman's Hut slots
- [x] Wheat is directly edible during the prototype. At each world-day boundary
      every resident tries to buy one portion from the Moot's physical hall stock;
      prepared Food is tried before Wheat. If a wallet cannot pay, the resident
      goes hungry unless Poor Relief spends treasury coin on the same ration.
      Current stock, reserve days, unmet
      portions and three-day production/consumption averages are replicated.
- [x] Shortage-responsive early planner: repeat Houses for missing beds and,
      after a measured day, repeat Farmsteads when reserves or recent production
      are inadequate (currently capped at one Farmstead per four residents).
- [x] Prosperity scalar with a panel breakdown: reserve 40, production 30,
      housing 20, employment 10, and hunger penalty down to -30.
- [x] Hamlet → Village advancement after sustained measurable food security.
- [x] Reconcile observed per-trip production with the distant strategic tick. Both use the
      same quality-scaled rates, worker counts, one-field/two-field Farmstead capacity,
      storage limits, sale policy and market transaction code.
- [ ] Wheat-to-Food processing from FILLED SLOTS. Direct fishing already obeys
      filled slots; an unstaffed farm or mill must likewise produce nothing.
- [ ] Births against a food-supported cap, and deaths, as roster events
- [ ] Food processing beyond direct-edible Wheat, household budgets and
      differentiated consumption
- [ ] Later tier ladder requirements from WORLD-DESIGN 1 (trade and
      administration -> regional pull and amenities), each needing a building,
      a worker and sustained output. Military is deliberately NOT a rung
      requirement -- it gates HOLDING a settlement, not growing one.
- [ ] Hysteresis on every transition
- [ ] The decline ladder: struggling -> abandoned (recoverable) -> Ruins, where
      only the last needs destruction, deliberate razing, or long physical decay.
      Destroying the hall alone must not erase a populated town.
- [ ] Stagger economy work per settlement (30-60s) rather than sweeping every region
- [x] Measure the tick at full world scale and write the real numbers into ARCHITECTURE.
      `cargo village-scale-lab` holds 5,000 NPCs in 30 towns and fails on entity/route growth.
- [x] **Make current strategic production warp-invariant.** `SimulationDelta` captures the
      master speed once, and strategic trades integrate exact elapsed work-shift overlap,
      so a 100x step cannot skip dawn, shift end or a whole short work window.
- [ ] Decide a sub-step policy before adding future nonlinear population or disease models;
      the current linear production, commerce and daily boundaries do not need one.
- [ ] **Decide who may warp.** `TimeWarp` is a single replicated global set by a dev
      command: on a shared persistent world, one player warping is a world-altering action.

**Ordering note:** population is defined HERE and consumed by Phase 4, not the reverse.

**Exit:** an evening at warp visibly changes the political map of who is thriving.

---

## Phase 4 — Prices and the hand cart

**Playable:** buy grain cheap in a meadows village, cart it to the highland quarry town,
sell it dear. The M&B opening hour.

- [x] Five bounded physical goods: Wheat, Food, Wood, Stone and Iron
- [x] NPC coin and workplace ownership ledgers
- [x] Bounded physical stores on villagers, workplaces, houses and halls
- [ ] Persisted settlement stock semantics and ownership
- [x] Local Moot prices from physical stock, target reserves and buying liquidity
- [x] Generic carry capacity and lossless bounded transfers
- [ ] Player buy/sell UI — NPC wallets and live Moot quotes are visible now, but
      player quantity controls and player-owned trading stock do not exist
- [ ] Coin as server-owned player state (`PlayerProgression` is the only per-player numeric
      state today and nothing has ever mutated it — there is no precedent to copy)

**Does NOT need flow fields.** The old build order claimed the cart required them. A hand
cart is one unit following one order, which the hero loop already does end to end.

**Exit:** a player is richer than when they started, purely by trading, and the price they
sold at visibly moved.

---

## Phase 5 — Caravans

**Playable:** highwayman or guard captain. Follow a laden caravan, watch it become real
wagons when you get close.

Cargo rides the seam proven in Phase 2, so this phase adds economics, not architecture.

- [ ] Settlement dispatch toward the best price in range
- [ ] A TRAVERSABLE BASE GRAPH first: rough cross-country routes between
      settlements, so "roads emerge from traffic" is not circular -- traffic
      cannot wear a path along a route it cannot take.
- [ ] Cached routes over it, one per origin/destination pair, shared by everyone
      travelling it. Traffic UPGRADES a route (track -> trail -> road) rather
      than creating connectivity.
- [ ] Positions DERIVED from `(route, departed_at, speed, now)` rather than
      stepped, so an unobserved traveller costs nothing per tick and still has a
      minimap position at all times.
- [ ] Strategic movement along that graph
- [ ] Trade income to origin prosperity
- [ ] Escort and interception interactions
- [ ] Caravan detail on the map (routes read as arteries)

**Exit:** trade routes are visible on the map and worth interfering with.

---

## Phase 6 — Command

**Playable:** select units with a click and a drag box, order a group somewhere, watch them
arrive without shoving each other through walls.

Pushed late deliberately: this is the largest block of work in the roadmap and carries the
LEAST architectural uncertainty. It is a solved genre problem with a known cost model, so
building it early would burn months without falsifying anything.

- [ ] A `Unit` abstraction that is not `Hero`. Note `HeroMoveTargets` is keyed by `PeerId`
      with one target per player — N units per player is not representable without
      replacing it.
- [ ] Selection state and drag-box
- [ ] Group orders and order feedback
- [ ] Per-region traversability cost field
- [ ] Flow-field pathfinding
- [ ] Delete `server/src/world/pathfinding.rs` and `navgrid.rs` — dead salvage from the
      removed NPC AI, zero callers, `#![allow(dead_code)]` to survive compilation

**Exit:** twenty units cross a map together and it looks deliberate.

---

## Phase 7 — Retinue and businesses

**Playable:** hire a squad, escort caravans for real money, own a sawmill that pays while
you are logged off.

- [ ] **Combat, from zero.** Commit `041deaa` stripped ~9,600 lines of weapons and combat;
      `Health` survives registered for replication and attached to nothing. Both docs
      claimed combat existed. It does not. This is the largest hidden cost in the roadmap.
- [ ] Hiring, wages, upkeep
- [ ] Business slots and passive income
- [ ] Death and loss model for hero and retinue
- [ ] Offline income rules

**Exit:** income arrives while logged off, and losing a fight costs something real.

---

## Phase 8 — Clans and territory

**Playable:** factions to befriend or bleed. Strangle a rival's trade route and take their
border without a battle.

- [ ] `ClanId`, clans, relations moved by actions
- [ ] `Settlement.owner`, and territory DERIVED from it (never stored, never saved)
- [ ] Influence propagation over the region graph, recomputed on events under a budget
- [ ] Political tint overlay at map zoom
- [ ] **Emergent roads — resolve the invariant conflict first.** Roads-from-traffic as
      written would write flatten strokes into the map recipe, which the determinism
      boundary forbids. Resolution: roads modify travel cost and surface paint, never
      heights.
- [ ] NPC clan AI on one-step goals per disposition

**Exit:** the map is coloured by who holds what, and trade visibly moves borders.

---

## Phase 9 — War for the realm

**Playable:** take a realm.

- [ ] Warbands and garrisons (note: a garrison is already a Town REQUIREMENT
      from Phase 3, so soldiers exist as people well before this phase)
- [ ] Refugee migration when a settlement is destroyed: form parties holding
      real PersonIds, one route per party, and on arrival they compete for
      vacant homes and jobs. A party groups for ROUTING and map presentation,
      never for identity.
- [ ] Settlement capture
- [ ] Sieges
- [ ] Razing and offline protection, designed TOGETHER (razing is permanent, so the rules
      that allow it and the rules that protect absent players are one decision)
- [ ] Realm victory condition

---

## Cross-cutting, not owned by any phase

Things every phase touches, easy to discover too late:

- [ ] **Client/server version compatibility.** lightyear 0.28 wires bevy_replicon, which
      hashes replication-rule and event registration order and disconnects mismatched
      clients. Every phase registers new components and messages, so every deploy locks out
      every previously distributed client. Needs a version gate and a distribution story.
- [ ] **The editor.** `editor/` is a live workspace member sharing the `shared` crate; it
      breaks on foundational changes and is the only tool that authors maps.
- [ ] **Replication backpressure.** No cap on entities per client, no priority scheme, no
      bandwidth ceiling. Every phase adds entity classes.
- [ ] **The 13MB `map.ron`**, most of it ~74k baked prop spawns that are already derivable
      from the seed recipe. Contradicts the repo's own seed-recipe principle and is copied
      into every container image.
- [ ] **Engine upgrade reserve.** This repo's history shows engine bumps are multi-week
      events. Budget for one.
- [ ] **Multiplayer validation.** Clans, territory and politics are only meaningful with
      concurrent players, and a solo developer cannot discover whether they are fun alone.
