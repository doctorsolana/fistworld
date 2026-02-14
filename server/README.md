# Server Module Map

This crate is organized as domain modules under `server/src`:

- `app`: app bootstrap, schedule sets, and system wiring.
- `net`: connection lifecycle, peer identity, and client input ingress.
- `player`: spawn, roster, movement, death, and respawn lifecycle.
- `vehicle`: driver interaction and vehicle simulation.
- `combat`: fire/reload, bullet simulation, hit resolution, and bullet cleanup.
- `ai`: NPC spawn, obstacle sync, runtime AI tick, and dead NPC cleanup.
- `collision`: static collider libraries, collider streaming, and entity/world collision resolution.
- `inventory`: hotbar, ground items, chest flows, and drop-on-death handling.
- `world`: world bootstrap, world time, and map state replication.
- `persistence`: player profile storage and autosave.
- `telemetry`: fixed-tick performance diagnostics.

`server/src/main.rs` is intentionally thin and only bootstraps module declarations and `app::run()`.

## Post-Rewrite Status

- Server wrapper files were retired; domain modules own runtime logic directly.
- Shared namespaced imports are the default style.
- `mod.rs` files are orchestration surfaces, not implementation dumps.

## Dependency Direction

Preferred dependency direction:

- `app` depends on domain modules.
- Domain modules may depend on `shared`.
- Domain modules should avoid circular dependencies with each other.

## Naming Guide

Use consistent, intent-revealing names:

- System functions use canonical prefixes:
  - `setup_`, `spawn_`, `despawn_`, `update_`, `handle_`, `sync_`, `apply_`, `ensure_`, `cleanup_`.
- Renames should be applied directly at call sites; avoid compatibility aliases.

Prefer domain-qualified nouns for resources/components, and avoid ambiguous top-level module names such as `systems`, `helpers`, `misc`, or `util`.

## Scheduling

Fixed-step systems are grouped by explicit domain sets in `app`:

- `WorldTick`
- `NetIngress`
- `VehicleSim`
- `PlayerSim`
- `Persistence`
- `AISim`
- `Collision`
- `Inventory`
- `Combat`
- `Telemetry`

These sets are chained in execution order to keep behavior stable while making dependencies easier to reason about.
