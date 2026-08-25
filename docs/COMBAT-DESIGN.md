# Combat design — battles on the living map

Audited against the working tree 2026-08-24. This is the plan for Phase 7's
battle system: Rome Total War / Bannerlord in *feel* — massed melee, squads,
formations, morale, individual soldiers who differ — but **never an instanced
battle arena**. A battle is an event at a real place on the one persistent
map: the terrain is the terrain, the buildings are the buildings, and the
people who die are real residents whose estates settle through the same
mortality pipeline as everyone else's. That last part is not a burden, it is
the design: wars here have economic consequences automatically, because a
dead soldier's wallet folds into his household, his business lists for sale,
and his company shares transfer — all code that already runs today.

## 1. Principles

1. **One world.** No battle scenes, no loading, no separate simulation rules.
   A siege is literally an army standing at a real settlement.
2. **Same discipline as everything else.** Server-authoritative in the shared
   60 Hz village schedule; all combat timers in *world seconds* through
   `SimulationTime` (a 100x tick delivers ~1.67 world-seconds and can contain
   several swings — every cadence loop must consume its whole budget, the
   same rule as `step_units`' multi-waypoint loop); replication follows the
   change-flag rules in ARCHITECTURE.md — per-swing state never crosses the
   wire.
3. **Individuals compose upward.** Resolution happens per soldier (their
   physique sets their damage and toughness, their own Health takes the
   hits); command and cohesion happen per squad; strategy per army. The RTW
   feel comes from the squad layer, the Bannerlord feel from the individual
   layer underneath it.
4. **Two tiers, like the villagers.** Observed battles are embodied. Distant
   battles will eventually resolve abstractly on the strategic layer and
   reconstruct when someone looks — the same promotion/demotion philosophy
   the population already uses. (Late phase; embodied comes first.)

## 2. What already exists (verified)

| Substrate | State | Combat use |
| --- | --- | --- |
| `Health` + `take_damage() -> died` | replicated, live | the one damage sink; write only on hit |
| `process_character_deaths` | live | estate/business/shares/ledger settlement + despawn, keyed on `Changed<Health>` — combat deaths ride it unchanged |
| `CharacterAttributes { physique, .. }` 0–100, seeded 8–20, `train_physique` | replicated, live | per-soldier damage/toughness variance; veterancy = training on kills |
| `MoveTarget` + `step_units` | proven at 1,000+ NPCs, warp-correct, collision-checked | approach, chase, rout movement |
| `TacticalCrowdGrid` (2.5 m cells, 3×3 scan) | live, idle early-out | melee target acquisition in weapon reach — widen its populate predicate to "movers OR combatants" |
| `CommandedBy` + `UnitMoveOrder` (256 cap) + drag-box | live | the player command surface; squads-as-entities bypass the cap later |
| `CivicRole::Guard` | job only, no behavior | the first defenders — guards get combat AI before anyone |
| Lab harness (`LabScenario`, run.sh modes, VillageTrace/StuckWatch) | live | the battle test world plugs in as one more scenario |
| Old melee math (`git show 041deaa^:shared/src/weapons/melee.rs`) | deleted, recoverable | pure, unit-tested arc-sweep + range + shield-block math — salvage as code |
| Old melee pipeline (`git show 041deaa^:server/src/combat/melee.rs`) | deleted | salvage as *design*: validate → broadphase → arc → nearest-first → direct Health mutation; cooldown kept outside the weapon component |
| LOS raycasts (`git show 9752cc6^:server/src/collision/raycast.rs`) | deleted twice | recover only when sieges need line-of-sight; verify against current collision modules |

Not salvaged: anatomical hitboxes, hit zones, ballistics, ragdoll impulses —
FPS fidelity, wrong cost model at two thousand units.

## 3. Component model

**Shared / replicated (low churn only):**

- `DeathCause::Combat` — append-only serde variant.
- `CharacterActivity::Fighting` — the animation channel; one send per
  engage/disengage, never per swing.
- Later: `Health` on building entities (wire-compatible today), a replicated
  squad identity for UI.

**Server-only (never replicated):**

- `WarParty { banner: String }` — explicit opt-in hostility. Two entities are
  hostile iff both carry `WarParty` with different banners. Nothing is
  hostile by accident; villages do not spontaneously go to war because their
  names differ. The real war-declaration model (who MAY raise a party
  against whom) is its own later phase.
- `MeleeSkirmisher { weapon: MeleeWeapon }` — combat capability. Weapon stats
  (`damage, reach, arc, cooldown, max_targets`) resurrected from the old
  `MeleeStats` shape.
- `MeleeCooldown { ready_at: f64 }` — absolute world-clock deadline
  (`ConstructionSupplyCooldown` pattern), consumed in a loop so high warp
  lands multiple swings per tick.
- `CombatTarget(Entity)` — current engagement.
- `Morale { value, .. }` — phase 4 accumulator; only its state *transitions*
  (fighting → fleeing → rallied) ever surface to clients.
- `PendingDeathCause(DeathCause)` — stamped by the killing blow, read by
  `process_character_deaths` instead of the current infer-from-hunger logic.

**Damage formula (v1):** `weapon.damage * (0.7 + physique / 100 * 0.6)`,
so a physique-8 recruit swings ~0.75x and a physique-100 veteran ~1.3x; the
seeded 8–20 spread gives every levy its own feel without any new data.
Toughness later multiplies `Health::max` the same way.

## 4. The engagement loop (v1 systems, in the Navigation set after `step_units`)

1. **acquire** — every `MeleeSkirmisher` with a `WarParty` scans the crowd
   grid (3×3 of 2.5 m cells) for the nearest hostile. In acquisition range
   (~6 m): set `CombatTarget`, chase via `MoveTarget` if beyond reach; in
   reach: `CharacterActivity::Fighting` (set_if_neq).
2. **swing** — for engaged pairs in reach: loop the world-seconds cooldown
   budget; each swing runs the salvaged arc test at chest height and applies
   `take_damage` directly. A miss writes nothing. A kill stamps
   `PendingDeathCause(Combat)` and trains the killer's physique by 1.
3. **mortality** — unchanged. Estates settle, the ledger records the death
   with the true cause, the entity despawns. (A `Corpse` marker that defers
   despawn for battlefield presentation is a later, purely visual phase.)
4. **disengage** — target dead or gone: clear and re-acquire; no hostiles in
   range: stand down to Idle. Guards and off-duty behavior resume on their
   own because combatants are ordinary characters.

Costs at scale: acquisition is O(N × 9 cells); swings happen only along the
contact front (reach is 2 m — rear ranks physically cannot fight, which is
exactly the RTW line-grind look); movement is already paid for. Replication
per second is bounded by actual hits landed, not by army size.

## 5. The battle lab

`LabScenario::BattleLab` (alias `battle`) on `village_lab`, plus a
`./run.sh battleworld` mode. Knobs:

- `FISTWORLD_LAB_BATTLE=NvM` — e.g. `1v1`, `2v1`, `5v5`, `100v100`,
  `1000v1000`. Spawns two `WarParty` groups ("Red"/"Blue") of seeded
  skirmishers facing each other on open ground near the hall.
- `FISTWORLD_LAB_BATTLE_WEAPON=fists|club|spear|sword` (later).
- `FISTWORLD_BATTLE_TRACE=1` — per-2s per-banner line: alive, engaged,
  fleeing, mean health, kills; the VillageTrace idiom.

Headless pins (the shared schedule runs identically in tests):

- `a_1v1_duel_ends_with_exactly_one_survivor_and_a_combat_death_record`
- `two_on_one_wins_faster_than_one_on_one` (statistical, seeded)
- `a_duel_at_100x_warp_lands_the_same_world_seconds_of_damage_as_1x`
- `a_villager_without_a_war_party_is_never_targeted`

## 6. Build order

| Milestone | Delivers | Proof |
| --- | --- | --- |
| **M1 First blood** | WarParty/MeleeSkirmisher/CombatTarget/cooldown, acquire+swing systems, `DeathCause::Combat` carrier, crowd-grid predicate widened, `Fighting` activity, battle lab + run.sh mode | headless pins + watch a 5v5 in battleworld |
| **M2 Soldiers** | weapon kinds, physique scaling + toughness, client Fighting animation (reuse an existing swing clip as placeholder), guards defend: hostiles near their settlement turn `CivicRole::Guard` holders into skirmishers | 10 raiders vs a guarded village |
| **M3 Squads** | squad entities (members, facing, line/column formation slots computed server-side, hold/advance/charge stance), squad orders from the retinue/selection UI, one order moves a whole squad past the 256 cap | 100v100 with two lines that hold shape |
| | *Partially landed early (2026-08-24): `Battalion` entity + `MemberOfBattalion` durable-id tag (both replicated; no Entity in components — repo rule), `ArmyOrder` (Muster/Assign/Dismiss/Disband) + `FormationMoveOrder` messages, server rank-and-file slot assignment (ranks of 8, strongest front, no path-braiding within a rank) in `server/src/player/army.rs`, Army encyclopedia tab (muster from selection, enlist/dismiss, select/locate/disband), right-click routes an all-one-battalion selection through the formation order. Still missing from M3: facing/stance orders, the past-256-cap squad order, hold/advance/charge.* | |
| **M4 Morale** | server-only morale: drains on nearby friendly deaths / local outnumbering / health, restored by cohesion and a leader's charm; rout (flee away from enemy centroid) and rally; only state flips replicate | routs visibly cascade from a flank |
| **M5 Scale** | 1000v1000 in battleworld at warp under ServerPerf + BattleTrace budgets; batch Health sends if measurement demands | tick ≤ 16.7 ms through full contact |
| **M6 War & sieges** | who-may-fight-whom (war declarations, raiding parties), building `Health` + destruction pass (obstacle-grid removal, `navigation_geometry_version` bump on disappear — only the appear direction exists today, economy settlement modeled on mortality.rs), walls/gates, off-screen strategic resolution | besiege Brackwater |

Each milestone is independently playable in the battle lab and lands with its
regression pins before the next begins.

## 7. Known traps (from the audits — do not relearn these)

- A combat system that writes `CharacterMotion`/`CharacterActivity` or any
  replicated component every tick recreates the 417 KiB/s bandwidth bug.
  `set_if_neq`, `as_mut()` never `as_deref_mut()`, no tuple inserts.
- Warp: any "attack if cooldown ready" check that fires once per tick
  silently under-damages at high warp. Loop the budget.
- `StrategicPerson` entities are invisible to `step_units` and the crowd
  grid — battle participants must be pinned tactical.
- The client `formation_targets` ring is arrival spread only; real formations
  are a *server* concept or authority and intent desync.
- `LabScenario::from_environment` panics on unknown values — new scenarios
  must update its error string; `stage_rendered_lab_once` refuses wrong
  `CITYSIM_MAP_ID`.
- `ensure_character_vitals` auto-gives Health to every `CharacterKind`
  entity — soldiers get vitals for free, but anything that should NOT settle
  an estate on death must not be a character.
