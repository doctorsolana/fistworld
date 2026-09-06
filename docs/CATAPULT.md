# Catapult

A playable siege unit, available through **God mode → Spawn Catapult**. This is the
current acquisition path; workshop production, paid crews, ammunition resupply and
persistent siege inventories are future work.

Placement returns to normal selection after one click. Select the carriage and press
**C** to enter combat mode. Right-click clear ground to
move; right-click an enemy to keep bombarding that target. **F**, or the Fire at ground
button, arms a ground shot: right-click the displayed blast circle. The minimum and
maximum range rings and dotted arc explain the shot before committing. **H** / Hold
fire cancels the order. An already launched stone still lands. Escape cancels aiming.
Remembered control groups include individually selected catapults. Several selected
catapults receive spaced destinations; a mixed marching selection puts them behind the
soldier formation. Placement prevents overlapping carriages and caps each account at eight.

The carriage moves at 1.15 m/s and turns at 0.65 rad/s. It must stop and turn toward
its target before a 1.6-second wind-up. Reload takes 8.5 seconds, followed by the next
wind-up; there are 20 stones and 350 health. Effective range is 16–125 m. It does not
chase an out-of-range target automatically: reposition the siege unit. The HUD reports
movement, blockage, targeting, reload and ammunition.

Stones follow a high, non-homing arc. Each shot locks its aim during wind-up, so
running soldiers can evade it. Terrain can intercept the flight before the aim point.
A six-metre blast deals up to 115 damage, falling off quadratically to zero at the
edge. Allies, enemy soldiers and other catapults can be hurt. People use the existing
combat reaction and mortality pipeline. A destroyed launcher cannot erase an airborne
stone. Building destruction, wall interception and tree breakage are not implemented.

## Code ownership

- `shared/src/components/siege.rs`: replicated status/events, tuning, ballistic
  sampling, arm motion and the launch socket shared by renderer and simulation.
- `server/src/player/siege.rs`: ownership, placement, orders and footprint clearance.
  `siege/fire.rs` owns firing, terrain interception, impact damage and wreck retirement.
- `server/src/player/orders/navigation.rs` and `hero::step_units`: the existing bounded
  shared route planner and mover now accept the carriage's conservative 3 m clearance.
  Soldier movement retains its original speed and navigation behavior.
- `client/src/siege/`: articulated presentation, pooled stone/dust assets, aim preview
  and retained controls. The client never decides hits or edits health.
- `asset_creation/siege/build_catapult.py`: deterministic Blender authoring source for
  `client/assets/game_assets/vehicles/Catapult.glb`. The named arm, four wheels and winch
  are animated without per-frame mesh rebuilding.
- `capture/scenarios/catapult.ron` and `client/src/capture/siege.rs`: connected rehearsal
  using ordinary F/RMB/H input, production server damage and real Bevy screenshots.

Firing/status changes replicate sparsely. Stones transmit an analytic timeline rather
than a position every tick; only interest-region changes update during flight. Damage
uses one victim pass on impact ticks, with a reused event buffer. Impact entities retire
after 2.4 world seconds. Meshes/materials are shared, and panel text refresh is bounded.

## Verification

Run `cargo test -p server player::siege` for ownership, rejected orders, wind-up
cancellation, reload protection, friendly splash, one-shot damage resolution, projectile
survival, wide routing, movement speed, terrain interception and mixed-selection checks.
Shared tests cover trajectory sampling, splash falloff and replicated timeline round trips.

The connected rehearsal moves six metres, fires at ground, observes splash damage, holds
fire through a full reload, and then attacks a living enemy through the ordinary picker.
It records PNG + `.capture.json` renderer metadata + `.siege.json` state for every frame,
and writes `summary.json` with movement, ammunition, impacts and damaged people.

### Verified on 2026-09-06

`cargo check --workspace --all-targets`, `cargo test --workspace`, and
`cargo build --workspace --profile playtest` passed. The full suite has 862 passing
tests and 11 intentionally ignored tests, including eight server siege regressions.
`cargo fmt --all -- --check` and `git diff --check` also passed.

The connected rehearsal verified 6.0 m of travel at a maximum reported authoritative
speed of 1.1499 m/s, visible wind-up, two impacts, damage to 31 people, 18 remaining
stones, and 10.5 seconds of held fire. All 75 final PNG/metadata/state triples completed.
Real scene/window captures were inspected for carriage movement, arm release, aiming
controls, elevated flight, damage reactions and impact debris.
Local evidence lives in `logs/captures/catapult/`, with PNGs, matching renderer/state
metadata, a machine-readable summary and `catapult-demo.mp4`. The edited demo combines
the preceding rehearsal's denser arm-release excerpt with the final rehearsal's flight
camera and impact; `video-manifest.json` identifies every original frame and timestamp.
These are visual and functional checks, not a performance benchmark.
