# Game architecture

Decisions made 2026-07-27, before the simulation exists, because retrofitting any of them
is expensive. If you are about to write simulation code, read this first.

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

| Role | Meaning |
|------|---------|
| **Political** | Who owns this land; the unit of conquest |
| **Interest management** | What the server replicates to a given client |
| **Simulation LOD** | Whether this region is tactical or strategic right now |
| **Persistence** | The unit that gets saved and loaded |

Keeping these aligned is deliberate. When they diverge you end up maintaining several
spatial systems that disagree with each other, and every feature has to reconcile them.

Note the existing `SPATIAL_CELL_SIZE = 8.0` grid is an FPS-era structure for close-range
queries. Regions are a **coarse layer above it**, not a replacement.

## 4. What seamless zoom demands

This is the most technically demanding choice on the board. It requires:

- **Every entity has a representation at every zoom band.** A soldier is a model up
  close, part of an instanced blob at medium range, and a contribution to an army icon
  far out. Nothing may simply vanish.
- **Rendering LOD and simulation LOD are separate systems.** You can render a region
  you are not tactically simulating (as icons/abstract), and you must simulate a region
  no one is rendering (strategic tick). Do not couple them.
- **Transitions must not pop.** Cross-fade or match silhouettes across LOD bands.
- **Camera range grows enormously** — roughly 5 m to 20 km, versus today's 55–900 m.
  Expect depth-buffer precision problems; plan for a logarithmic depth buffer or
  per-band camera settings.

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
- **Persistence is per region**, and must handle a region being loaded/unloaded while
  neighbours stay live.
- **It needs a hosted server.** This is an operational commitment, not just code.

## 6. What already exists and fits

- `shared/src/city` already models `MapRoad`, `MapPlot`, `PlotZone`, `PlotArchetype` —
  most of a settlement's data model, authorable in the editor today.
- `server/src/world/navgrid.rs` + `pathfinding.rs` — tactical-layer movement.
  **Note:** per-agent A\* does not scale to many units heading to one place; the tactical
  layer wants flow fields.
- Chunked terrain streaming, huge maps, and the map editor.
- lightyear replication + profile persistence.
- The commander camera, which needs its zoom range extended by ~20×.

## 7. Build order

1. **Region layer** — define regions, ownership, and make them the interest/persistence
   unit. Everything else hangs off this.
2. **Strategic tick** — settlements produce, caravans move along routes, clans hold
   territory. No rendering beyond map symbols. Prove it is cheap at full world scale.
3. **Tactical units, grey-boxed** — selection, move orders, formations, flow-field
   pathfinding, with primitive shapes. No art dependency.
4. **Promotion/demotion** — the seam between the two layers, plus unobserved-battle
   resolution.
5. **Zoom bands + rendering LOD** — make the seam invisible.
6. **Art pass** — import the low-poly library once the game is fun.

Steps 1–3 are independent enough to be worked in parallel; step 4 is where the design
actually gets tested, so do not leave it until last.
