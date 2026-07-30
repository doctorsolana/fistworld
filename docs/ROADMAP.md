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
| 1 | The world remembers | L | not started |
| 2 | The seam | L | not started |
| 3 | They eat | M | not started |
| 4 | Prices and the hand cart | M | not started |
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

- [ ] **Stable identity.** `SettlementId`, and an allocator. Not `Entity` (unstable across
      restarts) and not `PeerId` (random per session). Every later cross-reference —
      `Settlement.owner`, caravan origin, business slot — joins on this.
- [ ] **World-state file, versioned and self-describing.** NOT bincode: it is positional,
      which is exactly why `PROFILE_VERSION` is at 6 and the profile loader is forced to
      reject-and-backup. A wipe is an inconvenience for an outfit and fatal for months of
      settlement state. RON is already a workspace dependency.
- [ ] **Write the v1 to v2 migration before there is anything to lose.** A migration path
      that is never exercised is the one that fails when it matters.
- [ ] Route world saves through the existing background IO worker
      (`server/src/persistence/io_queue.rs`), not the main thread
- [ ] Backup rotation and load-newest-valid-on-corrupt for the world file
- [ ] **The replication split**, decided rather than assumed: lightyear 0.28 visibility is
      per-ENTITY, not per-component, so "summaries global, detail on interest" needs either
      two entities per settlement or a global directory message plus an interest-managed
      detail entity. Decide now; it shapes every later entity class.
- [ ] Radius aggregator over `BiomeField::resources` — does not exist in any form, and
      `resources()` has never had a production caller, so validate it discriminates before
      building site scoring on it
- [ ] Deterministic settlement site selection from the seed
- [ ] Settlement names
- [ ] Map markers (there is no marker layer today; the map's only marker is bound to a
      component nothing inserts)
- [ ] Screen-space picking so a settlement can be clicked
- [ ] Inspect panel (copy the encyclopedia's master/detail layout)

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

The strategic tick finally gets a body. Note it currently runs an empty loop — the "prove
it is cheap at world scale" claim has so far been made by timing nothing.

- [ ] Population as a float, growing logistically toward a food-supported cap
- [ ] Food production from `population x resource_profile`
- [ ] Consumption
- [ ] Prosperity scalar
- [ ] Tier ladder both directions with hysteresis, down to Ruins
- [ ] Stagger economy work per settlement (30-60s) rather than sweeping every region
- [ ] Measure the tick at full world scale and write the real numbers into ARCHITECTURE
- [ ] **Decide the warp policy.** At 100x the tick hands the economy a 100-simulated-second
      integration step, so nonlinear growth drifts from a 1x world. Sub-step, or cap warp.
- [ ] **Decide who may warp.** `TimeWarp` is a single replicated global set by a dev
      command: on a shared persistent world, one player warping is a world-altering action.

**Ordering note:** population is defined HERE and consumed by Phase 4, not the reverse.

**Exit:** an evening at warp visibly changes the political map of who is thriving.

---

## Phase 4 — Prices and the hand cart

**Playable:** buy grain cheap in a meadows village, cart it to the highland quarry town,
sell it dear. The M&B opening hour.

- [ ] Four goods and coin
- [ ] Stocks per settlement
- [ ] Local prices from local stocks (no global market)
- [ ] Carry capacity — no inventory concept exists in any crate today
- [ ] Buy/sell UI — needs a quantity stepper and a coin readout, neither of which exists
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
- [ ] Strategic movement along the region graph
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

- [ ] Warbands and garrisons
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
