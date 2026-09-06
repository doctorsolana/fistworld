# Flexible melee verification — 2026-09-06

This pass adds persistent formation files with local reactions, distinct contact
faces for several attackers, independent partial selections, casualty replacement,
attack-move resumption, and authored guard/strike/recoil/fall presentation.
See [COMBAT-DESIGN.md](COMBAT-DESIGN.md) for ownership and limits, and
[VISUAL-CAPTURE.md](VISUAL-CAPTURE.md#connected-clash-scenarios) for reproduction.

## Code verification

- `cargo check --workspace --all-targets`: passed, no warnings.
- `cargo test --workspace`: 852 passed, 0 failed, 11 ignored.
- `cargo build --workspace --profile playtest`: passed, both real binaries rebuilt.
- `cargo fmt --all` and `git diff --check`: clean.
- Python authoring scripts parse; combat clips and sidearm were generated with
  headless Blender and the exported assets loaded in the connected client.

New regressions cover screening the second rank, stable file replacement after a
casualty, three distinct approaches regardless of command order, independent
attacker contact while the main front stays engaged, attack-move pause/resume,
distant independent attack goals, a shared route around a wall, body occlusion,
combat component serialization, and once-only estate settlement followed by corpse
expiration. Existing command, cooldown, death, membership and movement tests pass.

The final Rust checks are recorded in `/tmp/fistworld-clash-check-verified2.log`,
`/tmp/fistworld-clash-tests-verified2.log` and
`/tmp/fistworld-clash-build-verified2.log` on the development machine.

## Connected captures

These are real authoritative server and Bevy client runs on `battle_lab`, 1x world
time, 1600×900 scene output. Each sampled frame has PNG, `.capture.json` and
`.battle.json`; assertions require a confirmed Attack command, observed contacts,
casualties and completed writes. Casualties below include both sides. They are
observations, not balance targets or deterministic benchmark baselines.

| Scenario | Result | Attacking groups reaching contact | Peak contacts | Casualties | Captures |
|---|---|---|---|---|---|
| 50 vs 50, close view | PASS | 1/1 | 19 | 76 over 35 s after contact | 129 |
| 100 vs 50 | PASS | 2/2 | 22 | 87 over 45 s | 166 |
| 150 vs 50 | PASS | 3/3 | 28 | 84 over 45 s | 162 |
| 50 vs 50, then one independent attacker and a pair | PASS | 1/1 + all 3 independents | 26 | 80 over 35 s | 124 |

Artifacts on the development machine:

- `/tmp/fistworld-battle-1v1-final`: inspected `0020` and `0060` at zoom 32,
  including hit/strike timestamps and four fatal reactions in the latter.
- `/tmp/fistworld-battle-2v1-pass1`: inspected `0060`, showing the front and flank.
- `/tmp/fistworld-battle-3v1-pass4`: inspected `0020` and `0080`, showing separated
  approaches and a later irregular contact edge. `replay.mp4` encodes the captures.
- `/tmp/fistworld-battle-skirmish-pass1`: inspected `0030`, `0045` and `0060` with
  metadata. All three independent people have recorded swing deadlines; this is
  stronger evidence than a sent attack packet. `replay.mp4` encodes the captures
  using their recorded world-time spacing; a decoded frame was also inspected.

The 250-person army regression passed both its forward deployment and 90-degree
redeployment. All 250 arrived and aligned; maximum reported slot errors were
0.250 m and 0.199 m, within its unchanged 0.3 m bound. Inspected the composed
`02-selected-ui.png` and both arrival metadata files in
`/tmp/fistworld-army-250-combat-regression`.

The final close-view run uses the final rebuilt Rust sources. The multi-battalion,
mixed and army runs preceded the final corpse navigation cleanup, membership-pause
cleanup and carrying-animation override fix; their relevant behaviour also passes
the final regression suite. Captures record the preceding HEAD because this was a
dirty worktree during iteration. The commit containing this report identifies the
finished implementation; use the checked-in scenarios to rerun it.

## What changed after looking

The initial harness supplied only a ground cursor, so its supposed attack click
was actually Move. Those earlier outputs were rejected as attack evidence. The
harness now provides the matching picking ray and requires accepted-attack feedback.

The first confirmed three-on-one approach allowed an outer battalion to reserve
the centre's nearest face. Prioritising the closest formation before reserving
faces removed the unnecessary trip around the rear. Reorientation now assigns
ranks from physical positions once; deaths retain file queues. The final views
show distinct blocks, local side fighting and replacement after losses, without
all supporting ranks independently chasing one clicked soldier.

In the mixed sequence, rear defenders turn and step locally while the front keeps
fighting. The independent attackers find exposed opponents, and ultimately lose;
a handful of people does not force the entire defending battalion to turn around.

## Limits

This is a cohesive but locally reactive melee baseline, not a finished tactical AI.
Contact faces are geometric reservations; there is no morale, fatigue, formation
pressure, autonomous retreat/rally, adaptive column transition or diplomacy yet.
The animation set is a shared short-sidearm style, not paired duels, weapon classes
or a physical blade simulation. Supporting ranks intentionally remain relatively
ordered. Morale/fatigue and giving ground are the next useful step for longer fights.

The connected scenarios cover open terrain. A wall approach has a headless test
through the production mover and shared field; complicated forest/building battles
still need dedicated connected scenarios. These screenshot runs and builds ran
with other work on the machine and are not performance measurements. No FPS gain
or low-end Mac performance claim is made. Protocol CDF7 requires coordinated client
and server restart.
