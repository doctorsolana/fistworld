# Contributing Guide

This project is designed to be expanded without another rewrite. Use this guide for all
feature work across `client`, `server`, and `shared`.

> **Context:** the old first-person game is preserved at tag `citysim-final`. The live
> repository is a top-down persistent living-world RTS with an implemented autonomous
> settlement/economy slice. Do not revive removed combat, vehicle or legacy NPC systems as
> shortcuts; new features must fit the current server-authoritative architecture.

## Core Rules

- Keep behavior ownership explicit:
  - `shared` = contracts, deterministic math/sampling, protocol, shared data models.
  - `server` = authoritative simulation and persistence.
  - `client` = rendering, camera, input, UI, FX, local presentation.
- If a rule affects multiplayer correctness, it belongs in `shared` and/or `server`, not only in `client`.
- Do not add compatibility aliases or temporary legacy bridges.
- Keep `mod.rs` files as orchestration surfaces only.
- Avoid placeholder logic in production paths. If something is deliberately a stub, say so in a
  comment naming what replaces it.

## Feature Placement

- Put wire formats, replicated components, map schema, and shared deterministic utilities in `shared`.
- Put authoritative decisions and anti-cheat boundaries in `server`.
- Put visual-only behavior, camera, HUD, and cosmetic systems in `client`.
- Prefer extending existing domain modules over adding generic buckets like `util`, `misc`, or `helpers`.

Current high-level domains:

- `client`: `terrain`, `props`, `water`, `render`, `selection`, `settlement`, `hero`,
  `ui`, `audio`, `camera_rts`, `perf_overlay`, `city`
- `server`: `world` (time, identity, regions, settlement development, villages,
  roads and labs), `collision`, `net`, `player`, `persistence`, `telemetry`, `city`
- `shared`: `terrain`, `map`, `city`, `building`, `props`, `protocol`, `components`,
  `economy`, `character`, `region`, `spatial`, `physics`, `rng`, `worldgen`

Within `server::world`, follow the explicit ownership map in
[`server/README.md`](server/README.md). In particular, keep `village.rs` and
`village_roads.rs` as facades and extend their focused submodules.

**Generated worlds are recipes, not data (the Valheim model).** A generated map stores
its style, seed, half extent and noise scale in `map.ron` — a few values — and every
binary rebuilds the identical terrain grid from them at load time via `shared::worldgen`
(`GeneratedWorld::build_grid`). Surface paint is computed procedurally from the same formula
(`TerrainGenerator::get_surface_weights`); `edits.ron` holds only sparse hand edits, which
still layer on top (deltas over the generated base, baked weightmaps over the procedural
paint). Never bake generated terrain into `edits.ron` — that was 644MB for an 8km map and
scales quadratically. Rules for code in `shared::worldgen`: all randomness derives from the
world seed via `splitmix64`; generation stages read the immutable raw-height field, so order
must not become hidden serialized state. Roads are settlement runtime infrastructure, not
part of the terrain-generation recipe.

## Cross-Crate Feature Workflow

1. Define the feature boundary: which crate owns source of truth, which data crosses the network.
2. Implement contracts first in `shared` (components/messages/types); bump `PROTOCOL_ID` if the
   wire format changes.
3. Implement authoritative server logic.
4. Implement client presentation.
5. Add targeted regression tests for changed behavior.
6. Run the validation commands below before committing.

### Documentation ownership

Treat documentation as part of an economy/gameplay change, not as a later cleanup:

- update `docs/COMPANY-ECONOMY-IMPLEMENTATION.md` when company authority, shares,
  site accounting, staffing, storage or private supply changes;
- update `docs/CIVIC-ECONOMY.md` when Hall markets, permits, taxes, relief, public payroll
  or civic policy changes;
- update `docs/WORLD-DESIGN.md` and the root `README.md` when the player-visible living-world
  loop changes;
- update `docs/ROADMAP.md` whenever an implemented/future boundary moves;
- update `docs/VILLAGE-LAB.md` when a scenario, diagnostic field or interpretation changes;
- update `server/README.md` or `docs/ARCHITECTURE.md` when code ownership, scheduling,
  persistence or scaling boundaries change.

Do not describe a planned state as live. Use explicit future wording for incomplete work,
and preserve dates on benchmark/reference results unless that exact measurement was rerun.

## Performance Guardrails

- Hot paths must avoid per-frame/per-tick allocations and full scans.
- Cache expensive derived values and lookup indices at load/setup time.
- Avoid string-based lookups in inner loops.
- Avoid O(n²) dedupe patterns in runtime loops.
- Keep high-frequency network messages compact.
- Keep verbose logs behind env flags; the default runtime should be quiet in hot loops.

**Scale target.** This game is meant to retain thousands of people while embodying only the
observed subset. Anything per-person per-frame is a design decision, not a detail:

- Ordinary off-screen people must carry durable identity/economic state without routes,
  physics, door choreography, seats or animations. Extend aggregate strategic passes for
  world-wide rules.
- Local village routes use bounded surveys, an obstacle-versioned cache and the shared road
  graph. Per-agent A\* does not scale to a commanded group or shared destination; future
  formations require a flow field.
- One `AnimationPlayer` per unit will not survive. Plan on instancing or vertex-animation textures.
- Replicate global settlement summaries separately from region-scoped physical and economic
  detail. Do not make new settlement detail globally visible.

## Rendering and Visibility Guardrails

- Be explicit about visibility ownership: distance/LOD systems, frustum culling for animated
  hierarchies, camera near plane.
- Any system mutating `Visibility` or `VisibilityRange` must be scoped to intended entity types only.
- `VisibilityRange` does **not** propagate into GLB scene children; toggle the root `Visibility`.

## Protocol and Data Rules

- Wire changes require a coordinated client/server rollout **and** a restart of both binaries.
  Registration *order* in `shared/src/protocol/plugin.rs` determines lightyear net-ids, so a stale
  binary mis-routes messages rather than failing loudly. Bump `PROTOCOL_ID` so the handshake rejects it.
- **Never insert a `MessageSender`/`MessageReceiver` for a message that is not registered in the
  protocol plugin** — lightyear panics inside `MessagePlugin::clear` with an unhelpful
  `Option::unwrap()` on `None`.
- Add roundtrip tests for network types and edge values. `PlayerInput` used to hand-write
  `Serialize`/`Deserialize` around a bit-packed struct; that class of silent skew is why it is now a
  plain derive. Don't reintroduce hand-packing without a roundtrip test.
- Keep map-load preprocessing deterministic and reusable by both client and server.
- **Player profiles are bincode, which is positional and not self-describing.** Any field
  add/remove/reorder in `PlayerProfile` requires bumping `PROFILE_VERSION`. The version guard runs
  *after* deserialize and cannot catch a layout shift — old files decode into silent garbage.
- **Map `.ron` files fail hard on unknown enum variants** (unknown struct *fields* are fine).
  Removing a `SpawnMarkerKind` variant that exists in a saved map panics `WorldTerrain` init in
  both client and server. Preserve serialized variants or migrate authored maps first.

## World and Streaming Rules

- `PlayerPosition` (the commander's camera focus) is the **only** anchor for server terrain-collider
  streaming and client terrain/prop streaming. If it stops being written, the world silently empties
  and colliders collapse to chunk (0,0) — no crash, no log. `streaming_anchor()` warns once on the
  `None` path; keep that warning.
- Runtime collision must come from the server Rapier world; do not reintroduce terrain-proxy clamps
  or custom pushout loops in gameplay systems.
- Mutating an `Image` asset creates a new GPU texture, but material bind groups only rebuild on
  **material** asset events — `materials.get_mut(&handle)` after `images.get_mut` or the change is
  invisible until restart.

## Testing and Verification

Minimum before committing gameplay or shared changes:

```bash
cargo check --workspace --all-targets   # NOT `check -p <crate>` — see below
cargo test --workspace
```

`cargo check -p <crate>` is not sufficient: `--all-targets` and a full `cargo build --workspace`
catch breakage in test targets and in `tools/collider_baker`, which depends on `shared`.

For runtime verification (launch recipe, env flags, log signals, and the process-kill trap that
will otherwise cost you an hour) see `.claude/skills/verify/SKILL.md`.

Add regression tests when fixing:

- Protocol bugs.
- Terrain/sampling/mesh bugs.
- Spatial indexing/collision bugs.
- Visibility/culling bugs.
- Migration, household, economy, construction, route and time-warp bugs.

## Definition of Done

- Feature is in the correct crate boundaries.
- No compatibility shim/placeholder code introduced.
- Hot path impact reviewed against the scale target.
- Tests and checks pass; the tree has no new warnings.
- Docs updated when architecture, protocol, or workflow changes.

## Architecture

The netcode model is **decided**: server-authoritative with interest management, not
deterministic lockstep. The simulation is **two-tier**: a cheap always-on strategic layer
and a 60 Hz tactical layer, with durable entities promoted/demoted between them. Regions
currently own interest management and simulation LOD; settlements, not grid squares, are
the future political and persistence unit.

Read [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) before writing simulation code. In
particular: the strategic tick runs for the entire world forever, so it must contain no
pathfinding, no physics and no per-person embodied routine. Cheap durable person records are
intentional; tactical bodies are conditional on observation.
