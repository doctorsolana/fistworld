# Combat on the living map

Implementation status: 2026-09-06. Combat is server-authoritative and shares the
village simulation's fixed schedule, world clock, collision and mortality pipeline.
Battles happen on the existing map. There is no separate battle scene.

## Current controls

- **C** changes how the client interprets clicks. It is not a server ceasefire.
- Left-click any battalion member to select the whole battalion. Box selection
  expands every touched battalion too. **Shift** adds to a box selection or toggles
  a clicked battalion. Remove a member in Army management before controlling them
  individually; Alt-click does not bypass membership.
- In combat mode, right-click an enemy to engage its nearby line or empty ground
  to move. Multiple selected battalions distribute across nearby enemy battalions.
  **Ctrl/Cmd + right-click** focuses the selected battalions on the clicked target.
  **Right-drag** lays out a frontage: drag left to right when facing the enemy.
  The gold preview shows footprints, slots, facing and file/rank counts.
  Reverse the drag to reverse facing. **[ / ]** narrows/widens selected battalions
  in place by two files; **comma / period** turns each in place by 15 degrees. **Alt + right-drag** orbits the camera when units are selected.
- **X**, then a destination: attack-move. Engage nearby enemies, then resume the
  original march. **R**, then a destination: retreat without acquiring enemies
  during movement. Ordinary movement also obeys the destination over acquisition.
- **H** stops the current order and holds the formation: exposed soldiers may step
  locally to meet a threat. Arrival changes a marching unit to idle guard. The
  persistent **Hold line** stance disables automatic movement. Escape clears an
  armed command mode.
- **Ctrl/Cmd + 0–9** saves a control group. The digit recalls it; Shift adds it.
  Battalions retain their identity as membership changes; a group saved from any
  member remembers the battalion. Unassigned troops retain individual identities.
- Battalion cards show selected battalions. Shift-click adds or removes a
  card's battalion. The Army encyclopedia handles muster, assignment and disbanding.

## Army management and standing stances

The Army page has a battalion sidebar and two separate troop lists. Choose a battalion,
then add/remove a single troop, check rows for bulk changes, or fill its free slots from
unassigned reserves. **Other battalions** permits direct transfers without removing a
soldier first. Capacity is 64; unavailable embarked troops are visible but disabled.
Removing troops or disbanding a battalion keeps those people in the player's army.
**New battalion** creates an empty group and selects it; it never takes an unrelated
battlefield selection implicitly. Disband uses an inline confirmation.

Standing policy is independent of an active tactical objective:

- **Defensive** (default): after a nearby catapult impact, idle troops move to new
  ground. Members reposition together; individual unassigned troops can react too.
  They retain the new position instead of returning to the same bombardment point.
  A detached troop recently transferred on the roster reacts locally; distant members
  are not pulled across the map.
- **Hold line**: no autonomous translation, local melee steps, casualty gap filling
  or crowd pushes. Soldiers still turn and strike enemies already within reach.
- Direct move, retreat, attack-move and attack objectives take priority over both
  policies. One active member objective prevents a bombardment response from splitting
  its battalion. Changing policy cancels an automatic escape, not a direct objective.
- H stops an objective; it does not change the persistent policy. An idle Defensive
  battalion may therefore still reposition after an impact. Choose Hold line to forbid it.

`army/response.rs` consumes each resolved impact once, with a six-second response
cooldown. It checks the blast plus a four-metre warning margin, tries up to eight
fourteen-metre offsets against terrain and static obstacles, and submits a normal
formation move through the existing bounded route planner and mover. It does not
predict airborne stones or override a blocked player route. If no escape is navigable,
troops remain where they are. This is repositioning, not a morale/routing simulation.

`BattalionStance` is replicated on each battalion and inherited by its members.
Transfers apply the destination's policy; released troops revert to the Defensive
default. The Army page's model, layout, bindings and actions have separate modules;
health, stance and checkbox updates preserve the controls under the pointer.

## Contracts and ownership

| Concern | Source of truth |
|---|---|
| Tactical wire intent | `shared/src/protocol/unit_orders.rs` |
| Formation geometry and preview | `shared/src/formation.rs` |
| Ownership, validation, interruption and command ordering | `server/src/player/orders.rs` |
| Shared formation route fields | `server/src/player/orders/{navigation,flow}.rs` |
| Battalion identity/lifecycle and sequential membership | `server/src/player/army.rs`, `army/membership.rs` |
| Contact/cooldown/damage | `server/src/player/combat.rs` |
| Cohesion, local approaches, file replacement and body index | `server/src/player/combat/fronts/` |
| Independent unassigned troop approaches | `server/src/player/combat/skirmish.rs` |
| Clock-sampled combat clips and weapon presentation | `client/src/hero/{combat_animation,attachments}.rs` |
| Local acquisition and body separation | `server/src/player/combat/{targeting,separation}.rs` |
| Derived client roster | `client/src/army_roster.rs` |
| Gestures, control groups and formation preview | `client/src/selection/` |

`UnitOrder` is one message type over the ordered reliable channel. Move, attack,
retreat, attack-move and hold therefore apply in receive order. Its selection
contains durable `BattalionId`s and mapped entities for unassigned individuals.
The server also expands a member sent individually, then deduplicates and checks ownership, health and
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
A frontage drag changes the number of files, up to the surviving roster size
(maximum 64), with small continuous spacing adjustments between 1.15 and 1.65 m.
Fifty people can form 25 files by two ranks. The preferred `BattalionFormation`
lives on the battalion entity, so an ordinary move preserves its file count and
spacing. Five default 50-person battalions occupy an 83 m frontage.

Blocks preserve their current lateral order. Rank and lateral assignment use the
last ordered `FormationSeat` offsets, falling back to physical positions for new
members. Stable person IDs resolve ties across server/client entity mapping.
This avoids both strength-based reshuffles and assignment changes caused by
walking replication delay. Short last ranks occupy explicit file indices, so
quiet regrouping uses the same columns as deployment. Seats are cleared on
membership changes and updated only at accepted movement boundaries.
Arrival waits for the mover's completed endpoint before setting the final facing.
Idle formed troops restore their assigned slots after friendly body separation;
Hold line still prevents this movement. This avoids permanent spacing errors
when a late arrival nudges a soldier whose march already completed.

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

## Individual movement with battalion cohesion

Battalion membership controls selection and orders, independently of the movement
model. Persistent files retain the preferred quiet deployment shape. They do not
reserve an enemy face, forbid a rear-rank soldier from fighting, or require the
battalion to line up before contact. The former rectangular flank allocation and
three-file bend systems have been removed.

An explicit attack advances the preferred files toward the enemy's occupied
outline. Within local combat range, soldiers consider up to eight nearby opponents
and five approach angles; crowded contact points cost more. Exposed soldiers,
soldiers within 2.5 metres of a contact position, and reserves with a clear approach
may move independently. Screened reserves follow the actual soldier ahead, with
normal rank spacing, until an opening becomes available. This prevents deep ranks
from scattering just to overtake their own files. Existing approaches receive a
preference and valid fights retain their opponent. Once a soldier takes a local
approach, that freedom lasts until combat settles or a new order replaces it;
a briefly obstructed approach must not send them back toward their old file.
A reachable opponent takes priority over returning to a rank slot.
Contact-point choices persist for 0.55–0.75 seconds, staggered across soldiers;
dead or out-of-range opponents invalidate the choice immediately. Subsequent
choices penalize large changes of approach. Reserves ignore corrections smaller
than 0.55 metres by the person ahead, avoiding repeated tiny reversals.

`fronts/steering.rs` certifies short steps against nearby bodies and the existing
terrain/static-collision boundary. Nine directions at two lookahead lengths let
soldiers sidestep blockers without introducing per-person A*. Direction memory
holds a chosen passing side for 1.5 seconds. When no local step is clear, a soldier
waits; they do not repeatedly push toward an occupied slot. An established fight
has priority in body separation, so an arriving soldier yields to its participants.
While following a file, steering prefers shorter forward steps to large detours
and ignores its own formation in the lookahead crowd penalty. Swept body collision
still applies to every candidate. All positional integration remains in the
existing mover.

Idle Defensive formations defend their occupied footprint plus a six-metre margin;
rear ranks may cross their own formation to fill an opening. Their local opponent
search extends to twelve metres so a deep rear rank can see the active front.
Hold line permits turning and striking but no automatic translation. A local rear
attack does not rotate the whole defending battalion. After two quiet seconds,
formations regroup around the ground occupied by their survivors, with no return
to a distant pre-battle anchor. Attack-move retains its shared march while contact
interrupts it and resumes when local enemies are gone. Direct move/retreat clears
the combat movement state and restores explicit deployment control.

Unassigned people, including explicitly removed members, retain individual orders
and exposed-opponent approach choices through `combat/skirmish.rs`. Distant or
obstructed approaches use the bounded tactical route planner; formed troops share
one reverse field. Automatic bombardment responses retain their internal local
boundary so a distant recruit is not pulled across the map by an impact.

This is local crowd movement and target choice, not strategic flanking AI. There
may be more supporting soldiers than reachable fighting positions. An army does
not need every soldier swinging simultaneously to make progress.

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

The refreshed character asset includes forward and backward fatal falls, selected
by stable `PersonId` parity. Both last one second and hold their final pose until
the 1.4-second server removal deadline. The client pauses clock-sampled clips while
seeking so Bevy cannot wrap a finished death back to standing. The connected
`battle-animation.ron` scenario verifies these clips with ordinary damage and
mortality; the asset handover records the authored timing and equipment contracts.

Do not dirty replicated motion, rotation, activity or health on no-op writes.
Acquisition and separation reuse scratch buffers. The client builds one roster on
membership/identity/vital changes; walking does not rebuild the battalion bar and
encyclopedia's membership aggregates. Do not infer a measured FPS gain from these
algorithmic improvements.

## Verification and remaining work

The connected scenario `capture/scenarios/army-fluid.ron` widens three 50-person
battalions to two ranks, follows with an ordinary move that must preserve the
shape, then rotates/redeploys them. `battle-3v3.ron` starts selection from one
member per friendly battalion before issuing a normal army attack.

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

Additional weapon classes, armour, morale/routing/rallying, guards, military wages, diplomacy,
fortification sieges, adaptive passage reservations and lossless
strategic army promotion/demotion remain future milestones. They must retain these
authority, identity, clock and bounded-work contracts.

### Playable catapult — 2026-09-06

The first ranged siege weapon is implemented through God placement, ordinary unit
selection and the ordered command stream. F/RMB ground bombardment and RMB enemy attacks
use server-owned, non-homing stone trajectories with local splash and friendly fire.
H stops future shots. See [CATAPULT.md](CATAPULT.md) for tuning and remaining scope.

### Archers — 2026-09-07

Army management can equip archers with finite quivers. V and the Army page toggle
fire policy. Ranged attacks settle at effective range, use authored draw/release
timing and authoritative non-homing arrows, and switch to existing sword melee
when crowded. See [ARCHERY.md](ARCHERY.md) for controls, collision, budgets and limits.
