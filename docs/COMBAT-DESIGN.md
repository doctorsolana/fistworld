# Combat on the living map

Implementation status: 2026-09-06. Combat is server-authoritative and shares the
village simulation's fixed schedule, world clock, collision and mortality pipeline.
Battles happen on the existing map. There is no separate battle scene.

## Current controls

- **C** changes how the client interprets clicks. It is not a server ceasefire.
- Left-click selects a person; click a standard bearer to select their battalion.
  Drag a box to select owned people. **Shift** adds to a box selection or toggles
  a clicked group. **Alt-click** selects a bearer individually.
- In combat mode, right-click an enemy to attack or empty ground to move.
  **Right-drag** lays out a frontage: drag left to right when facing the enemy.
  The gold preview shows each destination and facing. Reverse the drag to reverse
  facing. **Alt + right-drag** orbits the camera when units are selected.
- **X**, then a destination: attack-move. Engage nearby enemies, then resume the
  original march. **R**, then a destination: retreat without acquiring enemies
  during movement. Ordinary movement also obeys the destination over acquisition.
- **H** holds the formation: exposed soldiers may step locally to meet a threat.
  Arrival changes a marching unit to Hold. Escape clears an armed command mode.
- **Ctrl/Cmd + 0–9** saves a control group. The digit recalls it; Shift adds it.
  Complete battalions retain their identity as membership changes. Partial
  selections retain only the individual people saved.
- Battalion cards show complete/partial selection. Shift-click adds or removes a
  card's battalion. The Army encyclopedia handles muster, assignment and disbanding.

## Contracts and ownership

| Concern | Source of truth |
|---|---|
| Tactical wire intent | `shared/src/protocol/unit_orders.rs` |
| Formation geometry and preview | `shared/src/formation.rs` |
| Ownership, validation, interruption and command ordering | `server/src/player/orders.rs` |
| Shared formation route fields | `server/src/player/orders/{navigation,flow}.rs` |
| Battalion identity/lifecycle and sequential membership | `server/src/player/army.rs`, `army/membership.rs` |
| Contact/cooldown/damage | `server/src/player/combat.rs` |
| Cohesion, contact faces, file replacement and local body index | `server/src/player/combat/fronts/` |
| Independent/partial-selection approaches | `server/src/player/combat/skirmish.rs` |
| Clock-sampled combat clips and weapon presentation | `client/src/hero/{combat_animation,attachments}.rs` |
| Local acquisition and body separation | `server/src/player/combat/{targeting,separation}.rs` |
| Derived client roster | `client/src/army_roster.rs` |
| Gestures, control groups and formation preview | `client/src/selection/` |

`UnitOrder` is one message type over the ordered reliable channel. Move, attack,
retreat, attack-move and hold therefore apply in receive order. Its selection
contains durable `BattalionId`s for complete battalions and mapped entities for
individuals. The server expands, deduplicates and checks ownership, health and
availability, with a 1,024-person command limit. It rejects an excessive command
rather than silently truncating it. `ArmyOrderFeedback` explains accepted/refused
commands. Membership messages commit each edit before validating the next edit.

Each account may maintain 12 battalions of up to 64 people. Disbanding removes
membership and standards while preserving retinue ownership. Empty battalions
remain until explicitly disbanded. Conscription removes village routines and
employment; battalion assignment alone does not create command ownership.

`EngagedWith(PersonId)` is replicated on engagement transitions. Client attack
markers follow this authoritative relation, including automatic acquisition and
stand-down. Sending an attack packet alone does not create a confirmed marker.
Protocol changes require rebuilding and restarting both binaries.

## Formation movement

A multi-battalion order creates a separate block for each battalion. Default blocks
use ten files, 1.4 m file spacing, 1.7 m rank spacing and a 5 m gap between blocks.
A frontage drag changes the number of files, up to twenty. Fifty people normally
form ten files by five ranks; five such battalions occupy an 83 m frontage.

Blocks preserve their current lateral order. Stronger soldiers occupy forward
ranks; people within a rank are assigned by lateral position to reduce crossing.
Stable person IDs resolve ties consistently across server/client entity mapping.
Arrival waits for the mover's completed endpoint before setting the final facing.

Open-ground legs are certified once. An obstructed battalion shares one reverse
Dijkstra field, bounded to roughly 128 cells on each axis, with coarser sampling
for larger areas. Every edge uses existing building/prop collision and terrain
walkability checks. The army shares a 2 ms planning slice, 2,048 expansion cap and
32 route-install cap per tick; those budgets do not multiply by soldier count.
Routes feed the existing mover, which consumes multiple waypoints correctly at
high time warp. Geometry changes invalidate certification; abandoned group fields
are pruned. Invalid arrival slots are rejected before cancelling a valid march.

This is tactical formation routing, not a strategic regional navigation graph.
Very narrow passages can be missed by the bounded sampling, and a disconnected
route may fail. Units follow individual certified paths around obstructions and
reform at their destination; rigid formation wheeling, adaptive column transitions,
charge mechanics and coordinated passage reservations remain future work.

## Flexible battle fronts

A complete battalion selection installs one persistent formation. Its files are
queues: when a soldier falls, the next living person in that file advances, without
sorting all survivors into new ranks. New attack/hold orders preserve physical
rank order; changing approach direction assigns the new ranks from current positions
once, avoiding crossing that a row-major reshuffle caused.

Rank positions are home positions. Exposed front, rear and outside-file soldiers may
step up to 1.1 m from home to meet a nearby opponent. Supporting ranks keep space
behind them. A local rear attack turns the threatened soldiers, not the entire
battalion. Weapon paths cannot pass through a person in front, and movement cannot
push deeper through an occupied body. Existing contacts stay stable until invalid;
new contacts prefer opponents with fewer attackers.

Several attacking battalions reserve distinct front/left/right/rear faces of their
target. Flank attackers approach outside its rectangle and narrow their frontage to
fit the flank. Further formations wait in reserve when all four faces are occupied.
This is a geometric allocation, not a tactical AI choosing an optimal encirclement.
Attack-move retains its original shared march while the front fights, then resumes
when local enemies are gone.

Selecting one person, a few people out of a battalion, or unassigned people gives
independent orders. They detach from rank control without losing battalion membership,
choose reachable exposed opponents, and test several local approach angles. Distant
or obstructed approaches use the existing bounded tactical route planner; full
formations share their existing reverse field. A complete battalion command brings
detached members back under formation control.

## Melee and performance rules

Loose defensive acquisition uses a post-movement grid with 9 m cells. Formed
combat and independent attack manoeuvres share a reusable 3 m body index. Formation
decisions run at 10 Hz and independent steering at roughly 8 Hz in world time; the
ordinary mover and contact checks remain on the fixed tick. Only local neighbouring
cells are inspected for strikes, with 2 m reach. Movement/retreat suppress acquisition. Account-controlled people and explicit `WarParty` banners are combat
participants. Ordinary uncommanded villagers are not automatically targeted.
Different accounts are currently different allegiances; diplomacy is not implemented.

Damage uses `Health::take_damage`, with a 0.8 world-second swing interval and
`14 * (0.7 + physique / 100 * 0.6)` damage. A killing blow immediately prevents
that victim's later swing in the same pass. Leaving reach discards missed contact
opportunities; changing orders preserves the weapon deadline. Sustained-contact
warp catch-up is capped at four swings per tick. Deaths use the existing estate,
company/share and history pipeline immediately; a fatal combat body remains for
1.4 world seconds to finish its fall, with a settlement marker preventing duplicate
estate processing. Dead bodies do not block movement or land further blows.

`CombatReady`, absolute `CombatSwing.impact_at` and `CombatReaction` carry sparse
presentation state. Authored guard, strike, recoil and fall clips sample the same
world clock as damage, including the 0.30 s wind-up. The sword is currently a common
visual sidearm, not an equipment/stat system. Existing rig distance/visibility LOD
continues to apply.

Do not dirty replicated motion, rotation, activity or health on no-op writes.
Acquisition and separation reuse scratch buffers. The client builds one roster on
membership/identity/vital changes; walking does not rebuild the battalion bar and
encyclopedia's membership aggregates. Do not infer a measured FPS gain from these
algorithmic improvements.

## Verification and remaining work

The connected scenario `capture/scenarios/army-250.ron` stages five real server
battalions, selects their 250 replicated members and exercises ordinary formation
drag input for a forward march and a 90-degree redeployment. It checks per-person
arrival/facing, captures start/preview/movement/arrival/UI, and emits measurements
alongside PNG and capture metadata. See [VISUAL-CAPTURE.md](VISUAL-CAPTURE.md).

Regression tests cover ordering, authority, membership batches, pursuit cooldowns,
post-mortem swings, selection semantics, roster invalidation, mapped entity IDs,
formation geometry and shared obstacle routing through the real mover.

Connected `battle-1v1`, `battle-2v1`, `battle-3v1` and `battle-skirmish` scenarios
exercise ordinary enemy-click input, then record continuous PNG, capture metadata
and per-person combat state. The mixed scenario sends one independent attacker,
then two more, into an ongoing clash. See [the verification report](COMBAT-CLASH-VERIFICATION-2026-09.md) for
observations; scenario assertions alone do not establish visual quality.

Weapon classes, armour, morale/routing/rallying, guards, military wages, diplomacy,
sieges, ranged line-of-sight attacks, adaptive passage reservations and lossless
strategic army promotion/demotion remain future milestones. They must retain these
authority, identity, clock and bounded-work contracts.

### Playable catapult — 2026-09-06

The first ranged siege weapon is implemented through God placement, ordinary unit
selection and the ordered command stream. F/RMB ground bombardment and RMB enemy attacks
use server-owned, non-homing stone trajectories with local splash and friendly fire.
H stops future shots. See [CATAPULT.md](CATAPULT.md) for tuning and remaining scope.
