# P4 — NPCs & AI removal manifest

Repo: `/Users/terminator2/Coding/citysim` (Bevy 0.18.1 + lightyear 0.26)
Recovery: `git show citysim-final:<path>` — the full FPS is preserved at tag `citysim-final`.
Scope: server AI domain, NPC replication surface, client NPC visuals/animation/ragdoll, NPC dialogue.

Every path and symbol below was verified to exist by reading the file at the cited line.

---

## 0. READ THIS FIRST — DANGER

### D1. `SpawnMarkerKind::NpcGroup` is PERSISTED in the shipped map. Do NOT delete the variant.
`shared/src/map/editor_schema.rs:246-250`
```rust
pub enum SpawnMarkerKind { Player, NpcGroup, Poi }
```
`client/assets/maps/city_alpha/edits.ron:7031880` contains a live marker:
```
/*[4]*/ ( id: 5, kind: NpcGroup, position: (-845.1979, 17.379326, -615.8031), rotation_degrees: 0.0, radius: 22.0 )
```
`shared/src/map/save.rs:23-38` (`load_map_edits_optional`) does `ron::from_str::<MapEditsDefinition>()` and returns a hard `Err` on parse failure; `shared/src/map/loader.rs:59` propagates it out of `load_map()`. Removing the `NpcGroup` variant makes **serde fail with "unknown variant `NpcGroup`"**, which breaks map loading for **server, client AND the editor** (all three call `load_map` / `load_default_map`). That is three KEEP-list crates dead on boot.

**Decision required (pick one, do not skip):**
- (a) **KEEP `SpawnMarkerKind::NpcGroup` as-is** in P4 — it is a 1-line editor annotation and costs nothing. Rename to something RTS-ish later *with a data migration*. ← recommended.
- (b) Rewrite `client/assets/maps/city_alpha/edits.ron` first (drop marker id 5 / rewrite `kind`), also `client/assets/maps/city_alpha_backup_2026-07-13/edits.ron` if you care about it, THEN drop the variant. `edits.ron` is **122 MB** — script the edit, do not open it in an editor.
- (c) Add `#[serde(alias = ...)]` / a catch-all variant. More code than (a).

### D2. `MapDefinition.npc_groups` removal is load-safe but write-lossy.
`shared/src/map/schema.rs:19-20` is `#[serde(default)] pub npc_groups: Vec<MapNpcGroup>`. Both shipped maps have `npc_groups: []` (`client/assets/maps/city_alpha/map.ron:65027`, `.../city_alpha_backup_2026-07-13/map.ron:135833`), and there is **no `#[serde(deny_unknown_fields)]` anywhere in the repo**, so serde silently ignores the leftover key on load. Safe to delete the field. Note the editor's `save_map_definition_atomic` will then rewrite `map.ron` without it — irreversible for anyone with authored groups (nobody currently has any).

### D3. `server/src/ai/spawn.rs` is NOT purely NPC — it also owns the debug physics boxes.
`handle_spawn_physics_box_debug` (line 375) and `sync_debug_physics_boxes` (line 482) live in this file and drive `DebugPhysicsBox` / `SpawnPhysicsBoxDebug`, which are **not** part of the P4 kill list and are still wired at `server/src/app/schedule.rs:118` and consumed by `client/src/render/systems/debug_physics.rs` + `client/src/ui/debug_time_menu/`. **Move those two functions out (e.g. to `server/src/physics/debug_boxes.rs`) before deleting the file**, or you take out the debug-box feature by accident. (If you *want* the debug boxes gone too, that is a separate deliberate decision — say so, and also strip `DebugPhysicsBox*` from `shared/src/components/actors.rs:102-115`, `shared/src/protocol/plugin.rs:49-53`, and `client/src/render/systems/debug_physics.rs`.)

### D4. `shared/src/spatial.rs` (KEEP list) is generic — but it is currently ONLY fed by AI code.
`SpatialObstacleGrid` / `ObstacleEntry` / `ObstacleAABB` are a plain rotated-AABB spatial hash with **zero NPC types in their API** (`shared/src/spatial.rs:1-277`). Verdict: **genuinely generic, keep it.** But the only writer is `server/src/ai/obstacles.rs::sync_obstacle_grid` and the only readers are `server/src/ai/pathfinding.rs` + `server/src/ai/tick/state_steps.rs`. If you delete all of `server/src/ai/`, the grid becomes an empty resource nobody fills. **Preserve `sync_obstacle_grid` (47 lines) somewhere outside `ai/`** — it is exactly the "static building footprints → navigation blockers" step an RTS pathfinder needs. See §6.

### D5. Ragdoll is load-bearing for three collision resolvers, one of which survives P4.
`crate::ai::ragdoll::{CorpseBodyPoint, CorpseCollisionIndex}` is imported by:
- `server/src/collision/geometry.rs:7` (KEEP-ish, generic geometry helpers)
- `server/src/collision/resolve_player.rs:12` (P5, survives P4)
- `server/src/collision/resolve_vehicle.rs:9` (P3, may already be gone)
- `server/src/collision/resolve_npc.rs:11` (deleted here)

P4 must strip corpse collision from `geometry.rs` and `resolve_player.rs` in the same commit, or the server crate does not compile. See §3.3.

### D6. Player profiles are untouched.
Verified: `rg -i npc shared/src/player_profile.rs server/src/persistence/*.rs` → **no hits**. No persisted-profile migration is needed for P4. (Inventory fields are P2's problem.)

---

## 1. Delete wholesale

| Path | Lines | Why |
|---|---|---|
| `server/src/ai/death_cleanup.rs` | 58 | `DeadNpcDespawnTimer`, `ensure_dead_npc_despawn_timers`, `update_dead_npc_despawn_timers` |
| `server/src/ai/identity.rs` | 56 | `npc_identity_for_group/_for_archetype/_for_debug`, occupation tables |
| `server/src/ai/mod.rs` | 21 | module root |
| `server/src/ai/obstacles.rs` | 47 | ⚠ **salvage first** — see D4 / §6.1 |
| `server/src/ai/pathfinding.rs` | 254 | ⚠ **salvage first** — see §6.2 |
| `server/src/ai/ragdoll.rs` | 1017 | whole authoritative ragdoll runtime |
| `server/src/ai/relevance.rs` | 137 | ⚠ **salvage pattern** — see §6.3 |
| `server/src/ai/spawn.rs` | 501 | ⚠ **D3: extract debug-box systems first** |
| `server/src/ai/state.rs` | 103 | ⚠ **salvage `XorShift64`** — see §6.4 |
| `server/src/ai/tick/mod.rs` | 300 | `handle_npc_damage_events`, `update_npc_ai` (LOD cadence loop) |
| `server/src/ai/tick/state_steps.rs` | 349 | `tick_idle_state`, `tick_walking_state`, `tick_fleeing_state` |
| `server/src/collision/resolve_npc.rs` | 94 | `handle_npc_static_collisions` — **already dead code**, not registered in any schedule (verified: only self-reference in `rg resolve_npc`) |
| `shared/src/npc.rs` | 334 | NPC geometry/health/naming consts + `HUMANOID_RAGDOLL_BODIES` table |
| `client/src/dialogue.rs` | 380 | whole `DialoguePlugin` |
| `client/src/render/systems/npc/animation.rs` | 306 | |
| `client/src/render/systems/npc/assets.rs` | 43 | loads `characters/custom/oilman_animated.glb` |
| `client/src/render/systems/npc/debug.rs` | 221 | hitbox + ragdoll gizmos |
| `client/src/render/systems/npc/mod.rs` | 41 | |
| `client/src/render/systems/npc/ragdoll.rs` | 450 | pose receive/interp/apply |
| `client/src/render/systems/npc/spawn.rs` | 439 | rig setup, bone mapping, dummy build |
| `client/src/render/systems/npc/state.rs` | 236 | `NpcAssets`, `NpcRagdollRig`, `NpcNetSmoothing`, … (verified: **zero** references outside `client/src/render/systems/npc/`) |
| `client/src/render/systems/npc/sync.rs` | 228 | |
| `client/src/render/systems/npc/visibility.rs` | 213 | |

Directory-level: `rm -r server/src/ai client/src/render/systems/npc`.

**Total deleted-file lines: 5,828.**

---

## 2. Shared crate edits

### 2.1 `shared/src/lib.rs`
- [ ] Remove line 8: `pub mod npc;`

### 2.2 `shared/src/prelude.rs`
- [ ] Line 6-9: drop `Npc,` and `NpcPosition,` from the `crate::components::{…}` re-export. (Nothing in the workspace actually imports `shared::prelude` — verified — but it must still compile.)

### 2.3 `shared/src/components/actors.rs`
Delete these definitions (all `Component`s registered for replication):
- [ ] `NpcArchetype` — lines 26-39 (enum: `Oilman`, `DesertOutpost`, `Dummy`, `CombatDummy`)
- [ ] `Npc` — lines 49-54
- [ ] `NpcActivityKind` — lines 56-72
- [ ] `NpcActivity` — lines 74-76
- [ ] `NpcIdentity` — lines 78-84
- [ ] `NpcPosition` — lines 86-88
- [ ] `NpcRotation` — lines 90-92
- [ ] `NpcVelocity` — lines 94-96
- [ ] `NpcFleeing` — lines 98-100

**KEEP** in the same file: `Player`, `PlayerProgression`, `PlayerCharacter` (line 42; note the *player* `Oilman` variant is unrelated to `NpcArchetype::Oilman`), `DebugPhysicsBox*` (102-115, see D3), all `Player*` state, `Ground`, `LocalPlayer`.

### 2.4 `shared/src/components/combat.rs`
- [ ] Delete `NpcDamageEvent` — lines 180-189. (P1 also removes this file's combat types; if P1 already ran, verify.)

### 2.5 `shared/src/map/schema.rs`
- [ ] Line 5: drop `use crate::components::NpcArchetype;`
- [ ] Lines 19-20: drop the `npc_groups` field from `MapDefinition` (see D2)
- [ ] Lines 58-82: drop the whole `for (index, npc_group) in self.npc_groups…` validation block from `MapDefinition::validate`
- [ ] Lines 230-267: delete `struct MapNpcGroup` + its `impl` (`zone_center_vec2`, `zone_half_extents_vec2`, `authored_occupation`, `authored_faction`)
- [ ] Lines 269-274: delete `enum MapBehaviorPreset` (`IdleWanderZone`, `PatrolRoute`, `StandAndFaceFlow`) — verified used only by `MapNpcGroup`, `server/src/ai/spawn.rs`, `server/src/ai/identity.rs`
- [ ] Lines 501-516: delete test `npc_group_trims_authored_metadata`
- [ ] **KEEP** `MapBounds`, `MapTerrain`, `MapObjectSpawn`, `MapBlocker`, `HeightmapData`, and the other four tests.

### 2.6 `shared/src/map/editor_schema.rs`
- [ ] **DO NOT TOUCH `SpawnMarkerKind`** unless you executed D1(b) first.

### 2.7 `shared/src/protocol/messages.rs`
- [ ] Line 4: drop `NpcArchetype` from `use crate::components::{NpcArchetype, PlayerCharacter};`
- [ ] Lines 303-309: delete `struct SpawnOilmanDebug`
- [ ] Line 325: remove the `Npc` variant from `enum BulletImpactSurface` (P1 deletes the whole enum — if P1 already ran this is moot)
- [ ] Lines 477-497: delete `enum RagdollBodyId` (16 variants)
- [ ] Lines 499-540: delete `PackedQuatI16`, `quantize_quat_component`, `dequantize_quat_component`, `pack_quat_i16`, `unpack_quat_i16` — verified: their **only** consumers are `shared/src/protocol/messages.rs` itself, `server/src/ai/ragdoll.rs`, and `client/src/render/systems/npc/ragdoll.rs`. All three go away.
- [ ] Lines 541-547: delete `struct RagdollBodyPose`
- [ ] Lines 549-556: delete `struct NpcRagdollStarted`
- [ ] Lines 560-568: delete `struct NpcRagdollPoseSample`
- [ ] Lines 570-575: delete `struct NpcRagdollPoseBatch`
- [ ] Lines 583-584: delete `pub struct RagdollPoseChannel;`
- [ ] Lines 669-712: delete test `npc_ragdoll_messages_roundtrip`
- [ ] **KEEP** `ReliableChannel` (line 578-ish) and `InputChannel` — used everywhere.

### 2.8 `shared/src/protocol/plugin.rs` — the replication registry
- [ ] Lines 4-10: drop `Npc, NpcActivity, NpcFleeing, NpcIdentity, NpcPosition, NpcRotation, NpcVelocity` from the `components::{…}` import
- [ ] Lines 42-48: delete the `// === NPC COMPONENTS ===` block: `register_component::<Npc>`, `<NpcPosition>`, `<NpcRotation>`, `<NpcVelocity>`, `<NpcActivity>`, `<NpcFleeing>` (**keep** the three `DebugPhysicsBox*` registrations at 49-53 unless D3 says otherwise)
- [ ] Lines 79-80: delete `// === NPC IDENTITY ===` + `register_component::<NpcIdentity>()`
- [ ] Lines 123-124: delete `register_message::<SpawnOilmanDebug>()`
- [ ] Lines 177-180: delete `register_message::<NpcRagdollStarted>()` and `register_message::<NpcRagdollPoseBatch>()`
- [ ] Lines 197-201: delete the `add_channel::<RagdollPoseChannel>` block

> **Protocol ordering caution:** lightyear derives component/message net-ids from registration order. Client and server both build `ProtocolPlugin`, so as long as both binaries ship from the same commit this is fine. A stale client will silently mis-decode. Not a persisted-data issue.

---

## 3. Server crate edits

### 3.1 `server/src/main.rs`
- [ ] Line 2: remove `mod ai;`

### 3.2 `server/src/app/resources.rs`
- [ ] Line 9: remove `use crate::ai;`
- [ ] Line 32: remove `app.init_resource::<ai::obstacles::ObstacleGridState>();` (or re-point at the salvaged module, §6.1)
- [ ] Line 33: remove `app.init_resource::<ai::ragdoll::CorpseBudget>();`
- [ ] Line 34: remove `app.init_resource::<ai::ragdoll::RagdollPoseStream>();`
- [ ] Line 35: remove `app.init_resource::<ai::ragdoll::CorpseCollisionIndex>();`
- [ ] Line 36: remove `app.init_resource::<ai::ragdoll::RagdollTelemetry>();`
- [ ] Line 37: remove `app.init_resource::<ai::relevance::NpcRelevanceSettings>();`
- [ ] Line 38: remove `app.init_resource::<ai::pathfinding::PathfindingBudgetSettings>();`
- [ ] Line 44: remove `app.init_resource::<physics::dynamic_actors::NpcPhysicsLodSettings>();`
- [ ] **KEEP** line 31 `app.init_resource::<SpatialObstacleGrid>();` and line 6 `use shared::spatial::SpatialObstacleGrid;` if you salvage §6.1; drop both if you don't.

### 3.3 `server/src/app/schedule.rs` — this file is where P4 actually bites
- [ ] Line 13: remove `use crate::ai;`
- [ ] Lines 100-104 (`FpsServerSet::PhysicsWorld` block): remove
  `physics::dynamic_actors::ensure_npc_physics_bodies`,
  `physics::dynamic_actors::cleanup_npc_physics_when_ragdoll_activates`,
  `physics::dynamic_actors::sync_npcs_from_physics_before_ai`
- [ ] Line 117: remove `ai::spawn::handle_spawn_oilman_debug` (keep line 118 `handle_spawn_physics_box_debug`, re-pathed per D3)
- [ ] Lines 140-155: **delete the entire `FpsServerSet::AISim` block** (`ai::obstacles::sync_obstacle_grid`, `ai::tick::handle_npc_damage_events`, `ai::tick::update_npc_ai`, `ai::ragdoll::debug_auto_kill_npcs`, `ai::ragdoll::activate_npc_ragdolls`, `ai::ragdoll::evict_excess_corpses`, `ai::death_cleanup::ensure_dead_npc_despawn_timers`, `ai::death_cleanup::update_dead_npc_despawn_timers`) and the `FpsServerSet::AISim` variant at line 50 + its entry in the `configure_sets` chain at line 68.
- [ ] Line 161: remove `physics::dynamic_actors::apply_npc_controls_from_ai`
- [ ] Lines 175, 177-180 (`PhysicsPost`): remove
  `physics::dynamic_actors::sync_npcs_from_physics_after_writeback`,
  `ai::ragdoll::stabilize_soft_ragdoll_bodies`,
  `ai::ragdoll::sync_npc_roots_from_ragdolls`,
  `ai::ragdoll::sync_corpse_collision_index`,
  `ai::ragdoll::send_ragdoll_pose_snapshots`
- [ ] Line 205: remove `ai::relevance::update_npc_network_visibility`
- [ ] Lines 271-274: the telemetry phase anchors reference deleted systems —
  `handle_perf_npc_inventory_build_phase_begin.before(ai::obstacles::sync_obstacle_grid)` must be re-anchored (or the whole `NpcInventoryBuild` phase dropped, §3.7).
- [ ] Line 99 comment / doc header line 3-4 mention AI — cosmetic.
- [ ] Rail schedule (`configure_rail_fixed_schedule`, lines 305-382) has **no** AI references — untouched.

### 3.4 `server/src/app/bootstrap.rs`
- [ ] Line 99: remove `app.add_systems(Update, ai::spawn::spawn_npcs_once.run_if(server_is_started));`. Note the surrounding `if rail_mode_enabled() { … } else { … }` — after removal the `else` arm keeps only `spawn_world_vehicles` (P3) and `spawn_world_chests` (P2); if those are already gone, collapse the branch.
- [ ] Remove the now-unused `use crate::ai;` if present.

### 3.5 `server/src/physics/dynamic_actors.rs` (survives P4, gutted in P5)
- [ ] Line 10-11: drop `Npc, NpcActivity, NpcActivityKind, NpcPosition, NpcRotation, NpcVelocity` from the components import
- [ ] Line 14: drop `use shared::npc::{NPC_MOVE_SPEED, NPC_RADIUS};`
- [ ] Line 29: drop `use crate::ai::ragdoll::NpcRagdoll;`
- [ ] Line 35: delete `const NPC_MASS_KG`
- [ ] Lines 42-43: delete `DEFAULT_NPC_PHYSICS_ACTIVATE_RADIUS`, `DEFAULT_NPC_PHYSICS_DEACTIVATE_RADIUS`
- [ ] Line 49: delete `pub struct NpcPhysicsBody;`
- [ ] Lines 52-78: delete `NpcPhysicsLodSettings` + its `Default` impl (reads `CITYSIM_NPC_PHYSICS_RADIUS`, `CITYSIM_NPC_PHYSICS_EXIT_RADIUS`)
- [ ] Lines 224-304: delete `ensure_npc_physics_bodies`
- [ ] Lines 306-315: delete `cleanup_npc_physics_when_ragdoll_activates`
- [ ] Lines 688-699: delete `sync_npcs_from_physics_before_ai`
- [ ] Lines 701-782: delete `apply_npc_controls_from_ai`
- [ ] Lines 784-806: delete `sync_npcs_from_physics_after_writeback`
- [ ] Line ~905: delete test `npc_physics_radius_checks_all_players`
- [ ] **KEEP** `ensure_player_physics_bodies`, `apply_player_controls`, `sync_players_from_physics`, `clamp_players_to_map_bounds`, `tick_player_jump_timers`, `sync_debug_boxes_from_physics`.

### 3.6 `server/src/physics/terrain_colliders.rs` (KEEP — terrain physics survives)
- [ ] Line 7: drop `Npc, NpcPosition` from `use shared::components::{…}`
- [ ] Line 125: remove the `npcs: &Query<&NpcPosition, With<Npc>>` parameter of `gather_centers`
- [ ] Line 140: remove the `for pos in npcs.iter().take(8)` loop that seeds collider streaming centers from NPCs
- [ ] Line 161: remove the `npcs: Query<&NpcPosition, With<Npc>>` system param
- [ ] Line 165: fix the `gather_centers(&players, &vehicles, &npcs)` call site
> Behavioural note: terrain colliders will now stream only around players (+ vehicles until P3). That is the intent, but if the RTS camera is a disembodied commander (P5), terrain-collider streaming will need a new anchor entirely.

### 3.7 `server/src/physics/layers.rs` (KEEP — rapier group bitmasks)
- [ ] Line 8: `GROUP_NPC: Group = Group::GROUP_4` — delete or reserve for RTS units
- [ ] Line 10: `GROUP_RAGDOLL: Group = Group::GROUP_6` — delete
- [ ] Line 64-77: delete `pub fn npc_groups()`
- [ ] Lines 92-105: delete `pub fn ragdoll_groups()`
- [ ] Lines 107-120: delete `pub fn ragdoll_no_self_groups()`
- [ ] Scrub `GROUP_NPC` / `GROUP_RAGDOLL` from the surviving membership masks at lines 51, 53, 70, 72, 84, 86, 98, 100, 113, 127, 129, 139.
> Consider *renaming* `GROUP_NPC` → `GROUP_UNIT` instead of deleting; RTS units will want exactly this bit.

### 3.8 `server/src/collision/` — corpse-collision de-linking (see D5)
- [ ] `server/src/collision/mod.rs:16` — remove `pub mod resolve_npc;`; fix the doc comment at line 6
- [ ] `server/src/collision/geometry.rs:7` — remove `use crate::ai::ragdoll::{CorpseBodyPoint, CorpseCollisionIndex};`
- [ ] `server/src/collision/geometry.rs:271-330` — delete `pub fn handle_capsule_vs_corpse_spheres`
- [ ] `server/src/collision/geometry.rs:332-350` — delete `pub fn handle_vehicle_proxy_vs_corpse_spheres`
- [ ] `server/src/collision/resolve_player.rs:12` — remove the `ai::ragdoll` import; line 15 — drop `handle_capsule_vs_corpse_spheres` from the `geometry::{…}` import; line 26 — remove the `corpse_index: Res<CorpseCollisionIndex>` param; line 42 — remove `mut corpse_candidates: Local<Vec<CorpseBodyPoint>>`; lines 87-94 — delete the `handle_capsule_vs_corpse_spheres(...)` call
- [ ] `server/src/collision/resolve_vehicle.rs:9,13,24,31,77-84` — same treatment (skip if P3 already deleted the file)

### 3.9 `server/src/net/connection.rs`
- [ ] Line 20: drop `NpcRagdollPoseBatch, NpcRagdollStarted` from the `shared::protocol::{…}` import
- [ ] Lines 121-122: delete `MessageSender::<NpcRagdollStarted>::default(),` and `MessageSender::<NpcRagdollPoseBatch>::default(),`
- [ ] Also drop the `MessageReceiver::<SpawnOilmanDebug>` registration if present in the receiver block (grep `SpawnOilmanDebug` in this file after P4 edits).

### 3.10 `server/src/player/spatial.rs` (KEEP — reusable nearest-entity index)
- [ ] Line 113: remove the `Without<shared::components::Npc>` query filter on `sync_player_spatial_index`.
> `PlayerSpatialIndex` / `nearest_alive_distance_sq` is a clean ring-search spatial hash — **worth keeping** for RTS unit queries even though its only current caller (`update_npc_ai`) dies. If nothing else calls it after P4, either keep it warm or delete `sync_player_spatial_index` from `schedule.rs:204` too.

### 3.11 `server/src/telemetry/network.rs`
- [ ] Line 9: drop `Npc, NpcPosition, NpcRotation` from the components import
- [ ] Line 12: drop `use crate::ai::ragdoll::RagdollTelemetry;`
- [ ] Lines 44-45, 48-49: delete fields `changed_npc_pos_sum`, `changed_npc_rot_sum`, `last_ragdoll_pose_msgs_total`, `last_ragdoll_pose_bytes_total`
- [ ] Lines 81-82: delete their resets
- [ ] Lines 131-132: delete the `changed_npc_pos` / `changed_npc_rot` `Query` params of `sample_replication_change_pressure`; lines 142-147 delete the accumulation
- [ ] Line 159: delete `ragdoll_telemetry: Res<RagdollTelemetry>` param of the log system; lines 202-229 delete the derived stats; lines 272-292 rewrite the `info!` format string and args (remove `npc_pos=`, `npc_rot=`, the whole `| ragdoll …` segment, and the trailing `last_ragdoll_*` writebacks)

### 3.12 `server/src/telemetry/perf.rs`
- [ ] Line 4: drop `Npc` from `use shared::components::{Bullet, Npc, Player};`
- [ ] Line 10: drop `use crate::ai::ragdoll::RagdollTelemetry;`
- [ ] Lines 19, 34: `Phase::NpcInventoryBuild` — rename to `InventoryBuild` or delete; lines 219-228 (`handle_perf_npc_inventory_build_phase_begin` / `_end`) follow
- [ ] Lines 22-23, 37-38, 141-156, 278-282: `Phase::AiCadence` and `Phase::Pathfinding` + `record_ai_cadence_ms` / `record_pathfinding_ms` become unreachable (only caller was `server/src/ai/tick/mod.rs:271-273`) — delete
- [ ] Line 246: delete the `npcs: Query<(), With<Npc>>` param; line 249 delete `ragdoll_telemetry: Res<RagdollTelemetry>`
- [ ] Lines 271-273: delete `npc_avg_ms` / `npc_max_ms`
- [ ] Lines 331-360: rewrite the `ServerPerf` `info!` format string + args (drop `npc=`, `ai_cadence=`, `pathfinding=`, `npcs=`, `corpses=`, `evicted_corpses=`)

### 3.13 `server/src/combat/` (P1 territory — verify after P1)
If P1 has already deleted `server/src/combat/`, skip. Otherwise these files will not compile after P4:
- `server/src/combat/hit_characters.rs` — imports `Npc, NpcArchetype, NpcDamageEvent, NpcPosition, NpcRotation` (line 10), the whole `shared::npc::{…}` hitbox API (lines 13-16), `crate::ai::ragdoll::{CorpseCollisionIndex, NpcDeathImpact}` (line 27); contains `AnatomicalNpcHit` (38), `hit_anatomical_npc` (46), `Victim::Npc` (167), the corpse-hit path (565), and NPC damage application (703-745).
- `server/src/combat/melee.rs` — line 17 components import, line 20 `shared::npc::{npc_capsule_endpoints, NPC_RADIUS}`, line 29 `crate::ai::ragdoll::NpcDeathImpact`, `VictimKind::Npc` (107), NPC arc test (303-330), NPC damage (446-492).
- `server/src/combat/target_index.rs` — `HittableSpatialIndex.npc_cells` (16, 25, 113, 123-128) and `collect_npc_candidates_segment` (84-100).

### 3.14 `server/src/world/bootstrap.rs`
- [ ] Line 15: comment `// - NPC spawning` — cosmetic.

---

## 4. Client crate edits

### 4.1 `client/src/main.rs`
- [ ] Line 9: remove `mod dialogue;`

### 4.2 `client/src/app_wiring/mod.rs`
- [ ] Line 34: drop `dialogue,` from the `use crate::{…}` list

### 4.3 `client/src/app_wiring/plugins.rs`
- [ ] Line 132: remove `app.add_plugins(dialogue::DialoguePlugin);`
- [ ] Line 125: fix the comment that lists "dialogue" as a shooter-only feature

### 4.4 `client/src/app_wiring/systems.rs` (all inside `wire_fps_systems`)
- [ ] Line 162: remove `game_systems::setup_npc_assets` from the `Startup` tuple
- [ ] Line 206: remove `game_systems::handle_npc_spawned` from the replication-driven `Update` chain
- [ ] Line 229: remove `game_systems::sync_npc_transforms` from the chained transform-sync tuple
- [ ] Lines 258-277: **delete the whole "NPC visuals/animation + debug hitboxes" `add_systems` block** — `setup_npc_rig`, `receive_ragdoll_started`, `receive_ragdoll_pose_batch`, `apply_ragdoll_pose`, `update_npc_visibility`, `apply_npc_no_frustum_culling_to_new_meshes`, `apply_npc_shadow_state_to_new_meshes`, `apply_double_sided_npc_materials`, `update_npc_animation`, `update_npc_hitbox_debug_gizmos`, `update_npc_ragdoll_debug_gizmos`

### 4.5 `client/src/render/systems/mod.rs`
- [ ] Line 7: remove `mod npc;`
- [ ] Line 17: remove `pub use npc::*;`

### 4.6 `client/src/render/systems/connection.rs`
- [ ] Line 21: remove `use shared::components::Npc;`
- [ ] Line 104: remove `MessageSender::<shared::protocol::SpawnOilmanDebug>::default(),`
- [ ] Lines 138-139: remove `MessageReceiver::<shared::protocol::NpcRagdollStarted>` and `…::NpcRagdollPoseBatch`
- [ ] Line 226: remove the `npcs: Query<Entity, With<Npc>>` param of `cleanup_enter_main_menu`; lines 247-249 remove the despawn loop

### 4.7 `client/src/audio/` (KEEP — audio framework survives)
- [ ] `client/src/audio/mod.rs:30` — drop `Npc` from `use shared::components::{LocalPlayer, Npc, Player, PlayerPosition};` (verified: `Npc` appears **only** on that import line in this file — it is already an unused import).
- [ ] `client/src/audio/state.rs:4` — drop `use shared::components::NpcArchetype;`
- [ ] `client/src/audio/state.rs:56-59` — remove the `Dialogue = 2` variant from `AudioPriority` (renumber carefully; it is an explicit-discriminant enum)
- [ ] `client/src/audio/state.rs:69-75` — delete `pub struct DialogueRequest { npc_entity, distance_sq, archetype }`
- [ ] `client/src/audio/state.rs:83, 89, 96, 99` — delete `AudioManager::max_dialogue` and `AudioManager::dialogue_queue` + their `Default` initializers
- [ ] `client/src/audio/remote_players.rs:137, 153-154, 199-207` — remove the `npcs: Query<(Entity, &Transform), With<Npc>>` param of `ensure_remote_footstep_emitters`, its candidate loop, and fix the doc comment. **Keep** the remote-player footstep path.
- [ ] `client/src/audio/limits.rs:10` — untouched (`ManagedAudioTag` survives).

### 4.8 `client/src/ui/debug_time_menu/` (KEEP — debug menu survives)
- [ ] `mod.rs:23` — drop `SpawnOilmanDebug` from the `shared::protocol::{…}` import (keep `SpawnPhysicsBoxDebug`)
- [ ] `mod.rs:169` — delete `struct SpawnOilmanNpcButton;`
- [ ] `mod.rs:171-173` — delete the doc comment + `struct SpawnDummyNpcButton;`
- [ ] `layout.rs:134,141` — remove the `"OILMAN"` / `"NPC debug"` section labels
- [ ] `layout.rs:153-154` — remove `spawn_oilman_npc_button(panel);` and `spawn_dummy_npc_button(panel);`
- [ ] `layout.rs:298-316` — delete `pub(super) fn spawn_oilman_npc_button`
- [ ] `layout.rs:318-336` — delete `pub(super) fn spawn_dummy_npc_button`
- [ ] `actions.rs:66-67` — delete the `npc_spawn_sender: Query<&mut MessageSender<SpawnOilmanDebug>>` param
- [ ] `actions.rs:85-86, 102-103` — delete the `Option<&SpawnOilmanNpcButton>` / `Option<&SpawnDummyNpcButton>` query members and destructuring
- [ ] `actions.rs:162-180` — delete both `if …_spawn_button.is_some()` blocks
- [ ] **KEEP** lines 142-143 (`PlayerCharacter::Oilman` toggle) — that is the *player* model, not an NPC.

### 4.9 `client/src/weapons/projectiles.rs` (P1 kill list)
- [ ] Lines 310-311: `BulletImpactSurface::Npc` match arm — dies with P1.

### 4.10 `client/src/camera.rs:291`
- [ ] Comment mentions "point-blank NPCs/props" — cosmetic only.

---

## 5. Editor crate (KEEP — must still compile and run)

Only two of the four editor hits are real code changes; the rest depend on D1.

- [ ] `editor/src/tools.rs:564` — `session.map_definition.npc_groups.clear();` inside the "clear map" action. **Must be removed** once `MapDefinition.npc_groups` is gone (§2.5), or the editor will not compile.
- [ ] `editor/src/ui.rs:350` — the confirm-dialog text "…markers, player spawn, NPC groups, and blockers…". Cosmetic, update the wording.
- [ ] `editor/src/ui.rs:926-927`, `editor/src/tools.rs:1895/1923`, `editor/src/session.rs:185`, `editor/src/worldgen.rs:1361`, `editor/src/ui.rs:1186` — all reference `SpawnMarkerKind::NpcGroup`. **Leave them alone under D1(a).** Under D1(b) each must be updated:
  - `session.rs:185` default `selected_spawn_kind` must pick a surviving variant (`Poi`)
  - `worldgen.rs:1361` `Landmark { name: "Outpost", kind: SpawnMarkerKind::NpcGroup }` must change kind
  - `tools.rs:1922-1924` and `ui.rs:924-936` are exhaustive `match` / selectable lists — the compiler will find them.

---

## 6. Genuinely reusable for RTS unit movement — salvage BEFORE deleting

Honest assessment; named functions only.

### 6.1 `server/src/ai/obstacles.rs::sync_obstacle_grid` (lines 16-47) — **HIGH value, take it verbatim**
Zero NPC types in the body. It walks `BuildingSpatialIndex` (a KEEP-list resource), expands each building's `footprint` by its `flatten_radius`, and inserts rotated AABBs into `shared::spatial::SpatialObstacleGrid`, rebuilding only when `building_index.version` changes. That is exactly "buildings become navigation blockers" for an RTS. Move it (plus `ObstacleGridState`, lines 9-12) to e.g. `server/src/world/navgrid.rs` and re-register in `resources.rs`/`schedule.rs`.

### 6.2 `server/src/ai/pathfinding.rs` — **MEDIUM value, take two functions, drop the rest**
- `find_path_a_star_with_scratch` (line 136) + `PathfindingScratch` (129) + `GridPos`/`world_to_grid`/`grid_to_world`/`heuristic`/`OpenNode` (74-126): a 2 m grid A* with a `GRID_MAX_STEP = 1.2` slope gate, 8-neighbour expansion, `GRID_MAX_NODES = 4000` safety cap, and reusable scratch buffers (heap + two HashMaps + a terrain-height cache). Signature is `(&WorldTerrain, &SpatialObstacleGrid, start: Vec3, goal: Vec3, &mut scratch) -> Vec<Vec3>` — **no NPC type anywhere**. Directly reusable; for RTS you'd want flow-fields or HPA* eventually, but this is a correct, budgeted starting point.
- `PathfindingBudgetSettings` (17-33, env `CITYSIM_PATHFINDING_REQUESTS_PER_TICK`, default 6, clamped 1..128) — the per-tick request-budget pattern is worth keeping; the default of 6 is absurdly low for hundreds of units.
- `pick_random_target` (line 35) — wander-specific, **discard**.

### 6.3 `server/src/ai/tick/mod.rs::update_npc_ai` cadence machinery — **MEDIUM value as a pattern, not as code**
The body is wander/flee-specific, but the LOD scheduler is the reusable idea: distance bands (`NPC_AI_NEAR_RADIUS 220 / MID 520 / FAR 900`, cadences `1/2/4/10`, `server/src/ai/state.rs:8-15`), a `cadence_cache: Local<HashMap<Entity,(u64,u64)>>` reclassified only every 10 ticks, per-entity phase offset (`npc.id % cadence`) to spread work, and a crowd cadence derived from `CITYSIM_NPC_MAX_UPDATES_PER_TICK` (default 90) so total updates/tick stay bounded. **Copy the algorithm into the RTS unit tick; do not keep the file.**
`server/src/ai/relevance.rs::update_npc_network_visibility` is the matching replication-relevance pattern: hysteresis enter/exit radii (600/680) + a fixed 10 Hz re-evaluation, driving lightyear's `ReplicationState::gain_visibility` / `lose_visibility` per client. **The hysteresis + per-client `NetworkVisibility` wiring is directly reusable for RTS fog/relevance.** (Its ragdoll `build_started_message` catch-up branch, lines 103-109, is dead weight.)

### 6.4 `server/src/ai/state.rs::XorShift64` (lines 77-103) — **take it**
6-line deterministic `xorshift64*` with `next_u64` / `next_f32`, no deps. Used all over spawn/pathfinding for reproducible placement. Move to `shared/` (it is currently `pub(crate)` in the server). Deterministic RNG matters more, not less, if the unit sim later goes lockstep.

### 6.5 `server/src/player/spatial.rs::PlayerSpatialIndex` — **keep in place**
`nearest_alive_distance_sq` (line 48) is a correct expanding-ring cell search with an early-out on the minimum-possible-ring distance. Generic; only the `Player` component types are baked in. Ideal basis for "nearest enemy unit" queries.

### 6.6 Explicitly NOT reusable — delete without regret
`server/src/ai/ragdoll.rs` (all 1017 lines: rapier joint construction, corpse budget/eviction, 30 Hz i16-quantized pose streaming), `server/src/ai/tick/state_steps.rs` (idle/walk/flee FSM steps), `server/src/ai/identity.rs` (NPC name/occupation flavour), `server/src/ai/death_cleanup.rs`, the whole `client/src/render/systems/npc/` ragdoll receive/rebind stack, `client/src/dialogue.rs`. None of it maps onto a top-down unit-tactics game.

---

## 7. Env flags / smoke-test path retired by P4

| Flag | Defined at | Notes |
|---|---|---|
| `CITYSIM_DUMMY_NPCS` | `server/src/ai/spawn.rs:82` (default **1**) | Spawns anatomical `NpcArchetype::CombatDummy` targets near the player spawn. Referenced in `~/.claude/.../memory/citysim-conventions-and-gotchas.md` as the ragdoll ground-truth harness. Retire the memory note with the code. |
| `CITYSIM_MAX_NPCS` | `server/src/ai/spawn.rs:30` (default **0**) | Documented in `README.md:126-127` and `:134`, **and in `.claude/skills/verify/SKILL.md:17-18`** (`CITYSIM_MAX_NPCS=8 ./target/debug/server`). **Update the verify skill** or the standard launch recipe stops making sense. |
| `CITYSIM_NPC_MAX_UPDATES_PER_TICK` | `server/src/ai/tick/mod.rs:31` (default 90) | see §6.3 |
| `CITYSIM_PATHFINDING_REQUESTS_PER_TICK` | `server/src/ai/pathfinding.rs:24` (default 6) | see §6.2 |
| `CITYSIM_NPC_RELEVANCE_RADIUS` / `_EXIT_RADIUS` / `CITYSIM_NPC_RELEVANCE_HZ` | `server/src/ai/relevance.rs:32-46` | |
| `CITYSIM_NPC_PHYSICS_RADIUS` / `CITYSIM_NPC_PHYSICS_EXIT_RADIUS` | `server/src/physics/dynamic_actors.rs:66-74` | |
| `CITYSIM_RAGDOLL_TEST_SECS` | `server/src/ai/ragdoll.rs:388-400` (`debug_auto_kill_npcs`) | Unattended ragdoll smoke test; paired with `FISTFORCE_AUTOCONNECT`. Documented in the user's memory file. |

Docs to update: `README.md` lines 9, 101, 104, 109, 126-127, 134, 310; `.claude/skills/verify/SKILL.md` line 17-18; `server/src/app/schedule.rs` module doc (lines 3-6).

---

## 8. Assets that become orphaned

| Asset | Size | Status |
|---|---|---|
| `client/assets/audio/dialogue/peasant/{peasant1,peasant2,peasant3}.ogg` | 92 KB | Only consumer is `client/src/dialogue.rs:92-94`. Safe to delete. |
| `client/assets/audio/dialogue/king/{king1,king2,king3}.ogg` | 92 KB | **Already unreferenced** in the current tree (no `rg` hit). Delete with the rest of `client/assets/audio/dialogue/`. |
| `client/assets/characters/custom/oilman_animated.glb` | 4.3 MB | **DO NOT DELETE.** `client/src/render/systems/npc/assets.rs:10` goes away, but `client/src/render/systems/player/assets.rs:57-79` still loads the same file (10 animation clips) and the KEEP list explicitly protects `client/assets/characters/`. |
| `client/assets/characters/custom/basemodel.glb`, `sarah_animated.glb` | 0.9 / 7.2 MB | Untouched by P4. |

---

## 9. Build-order checklist

1. Salvage §6.1 / §6.2 / §6.4 into their new homes **first** (separate commit, still compiling).
2. Extract `handle_spawn_physics_box_debug` + `sync_debug_physics_boxes` out of `server/src/ai/spawn.rs` (D3).
3. Decide D1 (recommend: keep `SpawnMarkerKind::NpcGroup`).
4. Delete the files in §1.
5. Apply §2 (shared) → `cargo check -p shared`.
6. Apply §3 (server) → `cargo check -p server`. Expect the compiler to also point at `server/src/combat/*` if P1 has not run yet.
7. Apply §4 (client) → `cargo check -p client`.
8. Apply §5 (editor) → `cargo check -p editor` **and actually launch it** (`./run.sh editor`) — the editor is the only thing that exercises `edits.ron` round-tripping, and a serde regression there is silent at compile time.
9. `cargo test --workspace` — deleted tests: `shared/src/map/schema.rs::npc_group_trims_authored_metadata`, `shared/src/protocol/messages.rs::npc_ragdoll_messages_roundtrip`, `server/src/ai/relevance.rs::relevance_distance_ignores_height_and_honors_boundary`, `server/src/physics/dynamic_actors.rs::npc_physics_radius_checks_all_players`. `shared/src/spatial.rs`'s three tests survive.
10. Boot server + client per `.claude/skills/verify/SKILL.md` (drop the `CITYSIM_MAX_NPCS=8`) and confirm `Name accepted!` + `Spawned client world visuals`.
