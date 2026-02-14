# Contributing Guide

This project is designed to be expanded without another rewrite. Use this guide for all feature work across `client`, `server`, and `shared`.

## Core Rules

- Keep behavior ownership explicit:
  - `shared` = contracts, deterministic math/sampling, protocol, shared data models.
  - `server` = authoritative gameplay simulation and persistence.
  - `client` = rendering, input, UI, FX, local prediction/presentation.
- If a rule affects multiplayer correctness, it belongs in `shared` and/or `server`, not only in `client`.
- Do not add compatibility aliases or temporary legacy bridges.
- Keep `mod.rs` files as orchestration surfaces only.
- Avoid placeholder logic in production paths.

## Feature Placement

- Put wire formats, replicated components, map schema, and shared deterministic utilities in `shared`.
- Put authoritative decisions and anti-cheat boundaries in `server`.
- Put visual-only behavior, camera, HUD, and cosmetic systems in `client`.
- Prefer extending existing domain modules over adding generic buckets like `util`, `misc`, or `helpers`.

## Cross-Repo Feature Workflow

1. Define feature boundary:
   - Which crate owns source of truth.
   - Which data crosses network.
2. Implement contracts first in `shared`:
   - Components/messages/types.
   - Protocol ID bump if wire format changes.
3. Implement authoritative server logic.
4. Implement client presentation/prediction.
5. Add targeted regression tests for changed behavior.
6. Run validation commands before commit.

## Performance Guardrails

- Hot paths must avoid per-frame/per-tick allocations and full scans.
- Cache expensive derived values and lookup indices at load/setup time.
- Avoid string-based lookups in inner loops.
- Avoid O(n²) dedupe patterns in runtime loops.
- Keep high-frequency network messages compact and quantized when appropriate.
- Keep verbose logs behind env flags; default runtime should be quiet in hot loops.

## Rendering and Visibility Guardrails

- Be explicit about visibility ownership:
  - Distance/LOD visibility systems.
  - Frustum culling behavior for animated hierarchies.
  - Camera near plane for point-blank interactions.
- Any system mutating `Visibility` or `VisibilityRange` must be scoped to intended entity types only.
- New close-range gameplay entities should be tested in extreme camera proximity.

## Protocol and Data Rules

- Wire changes require coordinated client/server rollout.
- Keep gameplay-facing API ergonomic even if transport is packed/quantized.
- Add roundtrip tests for packed network types and edge values.
- Keep map-load preprocessing deterministic and reusable by both client and server.

## Testing and Verification

Minimum before merging gameplay or shared changes:

- `cargo check --workspace`
- `cargo test -p shared`
- Relevant targeted crate tests for touched systems.

Add regression tests when fixing:

- Protocol bugs.
- Terrain/sampling/mesh bugs.
- Spatial indexing/collision bugs.
- Visibility/culling bugs.

## Definition of Done

- Feature is in the correct crate boundaries.
- No compatibility shim/placeholder code introduced.
- Hot path impact reviewed.
- Tests and checks pass.
- Docs updated when architecture, protocol, or workflow changes.
