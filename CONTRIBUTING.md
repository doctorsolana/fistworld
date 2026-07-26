# Contributing Guide

This project is designed to be expanded without another rewrite. Use this guide for all
feature work across `client`, `server`, `shared`, and `editor`.

> **Context:** this repo was a first-person shooter until July 2026 and is now the base for a
> top-down multiplayer unit-tactics game. The FPS layers were removed in a seven-phase strip
> (see [`STRIP_PLAN.md`](STRIP_PLAN.md)); the world, terrain, editor and netcode foundation was
> kept. If you find a reference to weapons, vehicles, NPCs or ragdolls, it is a leftover —
> recover the original from the `citysim-final` tag rather than reviving it in place.

## Core Rules

- Keep behavior ownership explicit:
  - `shared` = contracts, deterministic math/sampling, protocol, shared data models.
  - `server` = authoritative simulation and persistence.
  - `client` = rendering, camera, input, UI, FX, local presentation.
  - `editor` = offline authoring. It depends on `shared` only; nothing depends on it.
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

Current domains:

- `client`: `terrain`, `props`, `water`, `render`, `ui`, `audio`, `camera_rts`, `perf_overlay`, `city`
- `server`: `world` (tick, map state, navgrid, pathfinding), `physics`, `collision`, `net`, `player`,
  `persistence`, `telemetry`, `city`
- `shared`: `terrain`, `map`, `city`, `building`, `props`, `protocol`, `components`, `spatial`, `physics`, `rng`
- `editor`: `tools`, `ui`, `worldgen`, `city`, `camera`, `session`

## Cross-Crate Feature Workflow

1. Define the feature boundary: which crate owns source of truth, which data crosses the network.
2. Implement contracts first in `shared` (components/messages/types); bump `PROTOCOL_ID` if the
   wire format changes.
3. Implement authoritative server logic.
4. Implement client presentation.
5. Add targeted regression tests for changed behavior.
6. Run the validation commands below before committing.

## Performance Guardrails

- Hot paths must avoid per-frame/per-tick allocations and full scans.
- Cache expensive derived values and lookup indices at load/setup time.
- Avoid string-based lookups in inner loops.
- Avoid O(n²) dedupe patterns in runtime loops.
- Keep high-frequency network messages compact.
- Keep verbose logs behind env flags; the default runtime should be quiet in hot loops.

**Scale target.** This game is meant to run hundreds-to-thousands of units. Anything per-unit
per-frame is a design decision, not a detail:

- Per-agent A\* does not scale to a shared destination — use a flow field (one sweep from the goal,
  every unit reads a direction). `server/src/world/pathfinding.rs` is for single-agent queries.
- One `AnimationPlayer` per unit will not survive. Plan on instancing or vertex-animation textures.
- One replicated entity per unit will not survive naive replication either — see the netcode
  decision below.

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
  client, server *and* editor. Boot the editor against a real map before committing schema changes.

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
cargo check -p editor                   # nothing else depends on it, so nothing else catches it
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

## Definition of Done

- Feature is in the correct crate boundaries.
- No compatibility shim/placeholder code introduced.
- Hot path impact reviewed against the scale target.
- Tests and checks pass; the tree has no new warnings.
- Docs updated when architecture, protocol, or workflow changes.

## Open Architecture Decision

The netcode model for the unit simulation is **not decided**, and it shapes almost everything:

- **Deterministic lockstep** — clients exchange only commands and simulate identically. Scales to
  thousands of units on tiny bandwidth, but requires a fully deterministic sim: fixed-point or very
  disciplined f32, seeded RNG (`shared/src/rng.rs`), and no `HashMap` iteration-order leaks.
- **Server-authoritative + interest management** — reuses the existing lightyear replication and the
  commander view as the interest anchor, but caps practical unit counts much lower.

Do not add unit-simulation code that silently assumes one of these before it is chosen.
