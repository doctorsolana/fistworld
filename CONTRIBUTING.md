# Contributing Guide

This project is designed to be expanded without another rewrite. Use this guide for all
feature work across `client`, `server`, and `shared`.

> **Context:** the old first-person game is preserved at tag `citysim-final`. The live
> repository is a top-down persistent living-world RTS with an implemented autonomous
> settlement/economy slice, embodied heroes, boats, battalions and player-ordered melee.
> Do not revive the removed first-person systems as shortcuts; new features must fit the
> current server-authoritative architecture. See [the game code map](docs/GAME-CODE-MAP.md)
> for current module ownership.

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
- update `docs/VISUAL-CAPTURE.md` when capture scenarios, readiness, assertions, artifact
  metadata, recording or baseline behavior changes;
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

**Scale target.** The whole living world uses one authoritative simulation, including
people and animals no player has ever seen. Camera coverage controls replication and
presentation, never movement, production, service, construction or combat rules. Thousands
of active people remain a performance target, not a certified capacity:

- Preserve the same durable worker, route, cargo, progress and service ownership everywhere.
  Do not introduce observer-gated jobs, an alternate aggregate economy or straight-ETA travel.
  Optimise the shared path with spatial indexes, retained bounded searches, cached derived
  state and explicit review cadences; prove that the optimisation preserves outcomes.
- Local village routes use bounded surveys, an obstacle-versioned cache and the shared road
  graph. Per-agent A\* does not scale to a commanded group or shared destination;
  commanded formations already share bounded local fields; regional travel should extend
  that ownership rather than multiplying per-soldier searches.
- Keep full animation rigs within the existing close-character budget and reuse distant
  proxies. Instanced animation or vertex-animation textures are future options if measurements
  justify them, not a prerequisite for replacing the current bounded rig/proxy implementation.
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
- **Network serialization is positional.** Preserve serialized field/variant order when moving
  shared types between files. A source-only module split does not require a protocol bump;
  changing their wire layout or registration does. `PlayerProfile` is an in-memory session
  snapshot, not a disk format; the live world and account registry start fresh on restart.
  Future durable saves need an explicit version envelope checked before decoding the payload.
- **Map `.ron` files fail hard on unknown enum variants** (unknown struct *fields* are fine).
  Removing a `SpawnMarkerKind` variant that exists in a saved map panics `WorldTerrain` init in
  both client and server. Preserve serialized variants or migrate authored maps first.

## World and Streaming Rules

- Server terrain/prop colliders follow authoritative body and building `PlayerPosition`
  neighborhoods, including wildlife, and explicitly exclude commander camera entities.
  Movement must wait for its swept collider footprint to be ready; network observation
  cannot authorize or remove physical collision. Client terrain/prop streaming follows
  the commander's camera focus separately. Keep the client `streaming_anchor()` warning
  when that presentation anchor is missing.
- Runtime collision is server authoritative. Reuse the current terrain/prop query,
  navigation obstacle grid and exact footprint tests; do not create a competing
  client collision model or ad-hoc pushout loops. The current server uses Parry
  terrain queries and spatial obstacle indexes, not a Rapier simulation world.
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

For visual work, compilation is only the first gate. Follow
[`docs/VISUAL-CAPTURE.md`](docs/VISUAL-CAPTURE.md), run the real Bevy renderer through a
checked-in scenario where practical, and inspect its PNG **and** `.capture.json`. Use a
continuous scenario for streaming or camera-motion defects. The repository-level
[`AGENTS.md`](AGENTS.md) makes this contract discoverable to coding agents before they begin.

For runtime verification (launch recipe, env flags, log signals, and the process-kill trap that
will otherwise cost you an hour) see `.claude/skills/verify/SKILL.md`.

Add regression tests when fixing:

- Protocol bugs.
- Terrain/sampling/mesh bugs.
- Spatial indexing/collision bugs.
- Visibility/culling bugs.
- Migration, household, economy, construction, route and time-warp bugs.

Tests should protect current behavior, ownership and invariants, rather than
repeat a constant or freeze an incidental implementation detail. A fixture must
contain the data it claims to validate: assert nonempty authored objects before
checking their identifiers, and exercise the real selector when testing separate
budgets. Keep absolute wall-clock thresholds in explicitly run performance labs;
ordinary tests should prefer bounded work and deterministic outcomes. Do not
remove useful tests merely because their names or creation dates are old.

State each fixture's scope. A recipe-rate test does not prove navigation, a
serialization roundtrip does not prove compatibility with an older binary, and
an economy accounting pass does not prove that every staffed workplace produces
or every resident can afford food. Use connected sessions for physical work and
inspect per-worker and per-business evidence in longer economy runs.

### Targeted test audit — 2026-09-15

This audit sampled current client/shared contracts and worker/navigation fixtures
for stale assumptions, vacuous assertions and misleading performance claims. It
was not a line-by-line review of every workspace test. Useful inventory, protocol,
LOD, input-focus, ownership and physical-handoff regressions were retained.

- Removed two cases: a client test that only asserted `100 / 4 == 25`, and a
  historical world-recipe field test whose stated dependency no maintained map used.
  Current recipe validation, compact serialization and wire roundtrips remain.
- [Authored-map coverage](shared/src/map/schema.rs) now checks three populated
  maintained maps, requires nonempty props, and verifies resolution and idempotent
  normalization. The previous generated-map fixture had an empty object list.
- [Horse selection](client/src/animals/rendering.rs) now runs the real ECS selector
  with mixed wild/mounted populations and checks separate budgets, scene eviction,
  proxy fallback and zoom-out cleanup. Resident scene roots isolate selection;
  the test does not load animation rigs or measure GPU performance.
- [Ocean geometry](client/src/water/edge.rs) now checks finite vertices, valid
  indices, upward winding, coverage and resource ceilings instead of one exact
  vertex count. It permits cheaper tessellation and makes no FPS claim.
- The [tavern fixture](server/src/world/village/tavern/outdoor_tests.rs) supplies
  arrival at the authored exterior target and lets the real door system clear
  occupancy; it no longer removes those markers itself. This tests handoff,
  not the intervening navigation. The [120-person migration test](server/src/world/village/tests/immigration_tests.rs)
  retains deterministic route-admission assertions but reports elapsed time
  diagnostically; hardware thresholds belong in explicit optimized scale probes.

The subsequent workspace run passed **1,802 tests**: 540 client, 957 server,
304 shared and one collider-baker test; **29 were ignored**, not passed. These
results establish the exercised contracts, not an exhaustive test audit, visual
acceptance or proof that the overall economy is balanced. Connected worker
captures and longer per-business economy runs remain separate evidence.

## Definition of Done

- Feature is in the correct crate boundaries.
- No compatibility shim/placeholder code introduced.
- Hot path impact reviewed against the scale target.
- Tests and checks pass; the tree has no new warnings.
- Renderer/UI work has an inspected capture artifact, not only a successful build.
- Docs updated when architecture, protocol, or workflow changes.

## Architecture

The netcode model is **decided**: server-authoritative with interest management, not
deterministic lockstep. There is **one canonical world simulation**. Regions count observers
and own network interest; they do not select a simulation level or promote/demote people.
Settlements, not grid squares, are the future political and persistence unit.

Read [`docs/ARCHITECTURE.md`](docs/ARCHITECTURE.md) before writing simulation code and
[`docs/SIMULATION-PARITY.md`](docs/SIMULATION-PARITY.md) before changing execution budgets.
A headless server must keep the same authoritative workflows running without any client.
Bound navigation and decision work, retain progress between slices, and measure complete
world cost. Old aggregate-simulation benchmarks do not certify this implementation.
