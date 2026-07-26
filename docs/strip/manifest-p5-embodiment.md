# P5 — FPS embodiment → disembodied commander

**Repo:** `/Users/terminator2/Coding/citysim` · **Recovery:** `git show citysim-final:<path>`
**Scope:** turn the walking, colliding, animated FPS player into a *commander* = connection + name +
persistence hook + a view position. Client gets a flycam/RTS camera placeholder. Server keeps Rapier
for terrain/static colliders + raycasts (LOS), drops the whole character controller.

This is the most entangled phase: `Player`/`PlayerPosition`/`LocalPlayer`/`InputState` are the de-facto
"where is the world centred" signal for **terrain streaming, prop LOD, water, audio, world map, and
server collider streaming** — all of which are on the KEEP list. Do the **anchor rework (§0) first**;
every deletion after it is then compiler-checked.

---

## 0. DO THIS FIRST — the two load-bearing anchors

### 0.1 Client streaming anchor (`client/src/streaming.rs`, 28 lines)

```rust
pub type AnchorPlayer<'w,'s> = Query<'w,'s, &'static PlayerPosition, With<LocalPlayer>>;   // line 15
pub type AnchorCamera<'w,'s> = Query<'w,'s, (&'static GlobalTransform, Option<&'static RtsRailCamera>), With<Camera3d>>; // line 16-17
pub fn streaming_anchor(player: &AnchorPlayer, camera: &AnchorCamera) -> Option<Vec3>       // line 19
```

Consumers (**all KEEP**):

| File | Lines |
|---|---|
| `client/src/terrain/streaming/mod.rs` | 26 (import) |
| `client/src/terrain/streaming/spawn.rs` | 8-9, 23, 101-102, 108, 158-159, 168 |
| `client/src/terrain/streaming/regenerate.rs` | 65, 83 |
| `client/src/terrain/streaming/far_terrain.rs` | 81-82, 91 |
| `client/src/props/spawn.rs` | 9, 120, 141, 340-341, 348 |
| `client/src/props/lod/mod.rs` | 21 |
| `client/src/props/lod/visibility.rs` | 110-111, 127 |

**Action:** rewrite `streaming.rs` so the anchor comes from the (single) `Camera3d` `GlobalTransform` /
new `CommanderCamera.focus`, and **delete `AnchorPlayer` entirely**, collapsing the two-query signature
to one. Keep the type aliases' *names* so the 7 consumers only need their param lists trimmed.

Notes that make this safe:
* There is exactly **one `Camera3d`** in the client (`client/src/render/systems/rendering/setup.rs:55`);
  the second camera created by `scaled_target.rs:70` is a `Camera2d` present-camera. So
  `Query<&GlobalTransform, With<Camera3d>>` is unambiguous.
* `streaming.rs:13` imports `crate::rail::RtsRailCamera` — **rail dies in P3**. Whoever runs P3 or P5
  first must not leave this dangling. See §0.3.

### 0.2 Server collider-streaming anchor

Both server streaming systems centre chunk loading on `PlayerPosition`:

* `server/src/physics/terrain_colliders.rs:122-152` `gather_centers(players, vehicles, npcs)` and
  `:154-165` `sync_terrain_colliders(... players: Query<&PlayerPosition, With<Player>> ...)`
  — falls back to vehicles, then NPCs, then `ChunkCoord::new(0,0)`. After P3/P4/P5 **all three
  sources are gone** and every client silently gets a single chunk of terrain collider at the origin.
* `server/src/collision/streaming.rs:62-103` `update_static_collider_streaming(... players: Query<&PlayerPosition> ...)`
  — same failure mode for prop colliders.

**Action:** introduce the commander view component (§1.1) and point both systems at it:
`Query<&CommanderView, With<Player>>`. Keep the vehicle/NPC fallbacks deleted, keep the
`ChunkCoord::new(0,0)` last-resort.

### 0.3 Harvest the RTS camera from `client/src/rail/` BEFORE P3 deletes it

`client/src/rail/mod.rs` already contains a complete, working RTS camera — this is the flycam
placeholder, already written:

| Symbol | Line |
|---|---|
| `pub struct RtsRailCamera { yaw, focus, pan_speed, zoom, zoom_min, zoom_max, zoom_speed, tilt, look_sensitivity }` | 70-80 |
| `impl Default for RtsRailCamera` | 82-96 |
| `pub fn ensure_rts_camera_controller` | 187-200 |
| `pub fn update_rts_camera` (WASD pan, RMB yaw, wheel zoom, map-bounds clamp) | 202-277 |
| `pub fn release_cursor_for_rts` | 279-291 |
| `pub fn update_cursor_terrain_hit` | 293-… |
| `fn apply_rts_transform` | 682-695 |
| `fn intersect_terrain(ray, terrain)` (raymarch + binary refine vs heightfield) | 698-… |

`editor/src/camera.rs` (**KEEP**) has the richer dual-mode version worth copying instead:
`EditorCameraMode::{Rts, Free}` (line 10-14), `EditorCameraController` (16-31),
`spawn_editor_camera` (53), `update_editor_camera` (67), `apply_rts_transform` (via line 56).

**Action:** move `RtsRailCamera` (rename `CommanderCamera`), `apply_rts_transform`, `update_rts_camera`,
`ensure_rts_camera_controller`, `release_cursor_for_rts`, `intersect_terrain` into the new
`client/src/camera.rs`. Do this **before** P3 deletes `client/src/rail/`, or you will be restoring it
from the tag.

---

## 1. Server — the commander entity

### 1.1 What survives

The minimum commander entity spawned in `server/src/player/spawn.rs`:

```
Player { client_id: PeerId }          // shared/src/components/actors.rs:6-9   KEEP (replicated)
PlayerProgression { .. }              // actors.rs:12-24                       KEEP (persisted)
CommanderView(Vec3)                   // NEW — replaces PlayerPosition as the streaming/relevance anchor
ReplicationGroup / Replicate / ControlledBy                                   KEEP verbatim
```

`PlayerCharacter` (actors.rs:42-47) — keep only if the commander still picks a portrait/faction;
otherwise drop with the `SetPlayerCharacter` message (§4).

### 1.2 DELETE — `server/src/player/`

| Path | Lines | Why |
|---|---|---|
| `server/src/player/movement.rs` | 161 | `update_players` — the whole on-foot sim: `step_character`, `FlyMode`, `PlayerJumpState`, `PlayerWaterState`, `InVehicle` seat-follow |
| `server/src/player/lifecycle.rs` | 91 | `RespawnTimer`, `handle_player_deaths`, `update_respawn_timers`, `is_player_alive`, `resolve_map_spawn_position` — commanders don't die |
| `server/src/player/spatial.rs` | 130 | `PlayerSpatialIndex`, `sync_player_spatial_index`. Only consumer is `server/src/ai/relevance.rs` (P4). If P4 lands first this is already dead. **If you want NPC/unit network relevance later, port the cell-hash to key off `CommanderView` instead of deleting.** |

### 1.3 KEEP + EDIT — `server/src/player/`

* `server/src/player/mod.rs` (18) — drop `pub mod lifecycle; pub mod movement; pub mod spatial;` (lines 13, 14, 17).
* `server/src/player/index.rs` (35) — **keep as-is**. `PlayerEntityIndex` (peer→entity) is exactly the
  commander lookup you need. Zero changes.
* `server/src/player/roster.rs` (33) + `roster_cache.rs` (101) — **keep as-is**. Name/level/prestige roster,
  no positional coupling.
* `server/src/player/spawn.rs` (337) — **major rewrite, keep the file**.
  * Keep: `handle_player_name_submission` skeleton (validate → load/create profile → insert peer↔name
    maps → `roster_cache.upsert_profile` → `NameSubmissionResult::Accepted`), lines 49-99 and 293-299.
  * Keep: `resolve_map_spawn_position` (35-45) but return the *view focus*, not a capsule centre —
    drop the `+ ground_clearance_center()` term.
  * Delete: the 120-line spawn-state tuple (101-222) covering inventory/weapon/vehicle restore;
    the `PLAYER_REPLICATION_PRIORITY` bundle's movement components (240-247: `PlayerPosition`,
    `PlayerRotation`, `PlayerVelocity`, `PlayerGrounded`, `EquippedWeapon`, `Inventory`,
    `HotbarSelection`, `PreviousHotbarSlot`); the whole vehicle respawn block (260-291).
  * Delete: `handle_set_player_character` (305-337) **unless** commander avatars survive.
  * Delete imports: `shared::items::{HotbarSelection, Inventory}` (14), `shared::physics::ground_clearance_center` (15),
    `shared::vehicle::*` (23), `shared::weapons::WeaponType` (24), `crate::inventory::hotbar::PreviousHotbarSlot` (26).

### 1.4 DELETE — `server/src/physics/`

| Path | Lines | Why |
|---|---|---|
| `server/src/physics/dynamic_actors.rs` | 916 | Entirely character controller. `PlayerPhysicsBody`, `NpcPhysicsBody`, `NpcPhysicsLodSettings`, `PlayerJumpControllerState`, `ensure_player_physics_bodies`, `ensure_npc_physics_bodies`, `cleanup_npc_physics_when_ragdoll_activates`, `sync_player_bodies_from_authoritative_state`, `apply_player_controls` (378-626 — swim/jump/fly/air-control), `tick_player_jump_timers`, `clamp_players_to_map_bounds`, `sync_players_from_physics`, `sync_npcs_from_physics_before_ai`, `apply_npc_controls_from_ai`, `sync_npcs_from_physics_after_writeback`, `sync_debug_boxes_from_physics` |
| `server/src/physics/contacts.rs` | 59 | `update_player_grounding_from_queries` — downward capsule ray for coyote-time grounding |

### 1.5 KEEP — `server/src/physics/` (this is the LOS/terrain core)

* `server/src/physics/terrain_colliders.rs` (265) — **KEEP**, re-anchor per §0.2. Heightfield chunk
  colliders (`TerrainColliderChunk`, `TerrainColliderSettings`, `TerrainColliderRegistry`,
  `sync_terrain_colliders`) are the raycast surface for line-of-sight. Env knobs
  `CITYSIM_TERRAIN_COLLIDER_{RADIUS_CHUNKS,MAX_LOAD_PER_TICK,RESOLUTION}` survive.
* `server/src/physics/static_world_colliders.rs` (296) — **KEEP unchanged**. `StaticPropCollider`,
  `StaticBuildingCollider`, `sync_static_prop_colliders`, `sync_static_building_colliders`. Depends on
  `BuildingSpatialIndex` + `BakedColliderLibrary` + `StaticColliders`, all of which survive. Its two
  `#[cfg(test)]` tests (210-295) stay green.
* `server/src/physics/queries.rs` (24) — **KEEP. This is the LOS API.** `cast_world_impact(ctx, origin,
  dir, max_dist)` filtered by `layers::bullet_world_query_groups()`. Its *only* current callers are
  P1 combat (`combat/fire.rs:18,90`, `combat/hit_world.rs:15,70`, `combat/melee.rs:33,358`), so after
  P1 it is dead code → **`cargo build` will warn/`-D warnings` CI will fail**. Either add
  `#[allow(dead_code)]` or immediately land a `pub fn has_line_of_sight(ctx, from, to) -> bool` wrapper
  and rename the group helper (`bullet_world_query_groups` → `los_query_groups`).
* `server/src/physics/layers.rs` (141) — **KEEP, slim**. Survivors: `GROUP_TERRAIN` (5),
  `GROUP_STATIC_WORLD` (6), `GROUP_BULLET_QUERY`→rename `GROUP_LOS_QUERY` (12),
  `terrain_groups()` (36), `static_world_groups()` (41), `bullet_world_query_groups()` (136).
  Delete: `GROUP_PLAYER`/`GROUP_NPC`/`GROUP_VEHICLE`/`GROUP_RAGDOLL`/`GROUP_DEBUG_BOX` (7-11) and
  `world_solid_mask`, `dynamic_actor_mask`, `player_groups`, `player_seated_groups`, `npc_groups`,
  `vehicle_groups`, `ragdoll_groups`, `ragdoll_no_self_groups`, `debug_box_groups` (15-133).
  `world_query_mask()` (31) must be rewritten (it composes `dynamic_actor_mask()`).
* `server/src/physics/mod.rs` (8) — drop `pub mod contacts;` (3) and `pub mod dynamic_actors;` (4).

`bevy_rapier3d` stays in `server/Cargo.toml` and `RapierPhysicsPlugin` stays in
`server/src/app/mod.rs:43`. `configure_rapier_gravity` (18-25) can go — nothing dynamic remains — but
it is harmless and cheap to keep.

### 1.6 `server/src/collision/` — split KEEP vs DELETE

**DELETE (the software capsule-vs-hull resolver, only ever used by the three character resolvers):**

| Path | Lines | Sole consumers |
|---|---|---|
| `server/src/collision/resolve_player.rs` | 116 | schedule (removed) |
| `server/src/collision/resolve_npc.rs` | 94 | schedule (removed); nominally P4 but it dies with the same helpers |
| `server/src/collision/geometry.rs` | 533 | `handle_capsule_vs_static` (116) + `handle_capsule_vs_corpse_spheres` (271) used only by `resolve_player.rs:62,87`, `resolve_npc.rs:48,71`, `resolve_vehicle.rs:12` (P3); `sphere_vs_compound_hulls`/`SupportContact` used only by `building_geometry/mod.rs:12` |
| `server/src/collision/building_geometry/` (`mod.rs` 312, `shapes.rs` 231) | 543 | `handle_capsule_vs_buildings` used only by `resolve_player.rs:73`, `resolve_npc.rs:59`, `resolve_vehicle.rs:65` |

> **Verified (question c):** the editor does **not** touch any of this. `editor/Cargo.toml` depends only
> on `bevy, shared, serde, ron, bevy_egui, noise` — no `bevy_rapier3d`, no `server`. The only editor hit
> for "building_geometry" is the unrelated method `EditorPlotState::apply_selected_building_geometry`
> (`editor/src/city/state.rs:79,96`, `editor/src/city/ui.rs:123`). Terrain does not use it either.

**KEEP:**

* `server/src/collision/library.rs` (309) — `BakedColliderLibrary`, `DerivedColliderLibrary`,
  `DerivedBuildingColliderLibrary`, `HullFace`, `DerivedHull`, `DerivedCollider`,
  `StaticColliderInstance`, `StaticColliders`, `setup_baked_colliders` (82, wired at
  `server/src/app/bootstrap.rs:75`, loads `client/assets/colliders.bin`).
  ⚠️ After P5 the *Derived\** halves have no consumer (only `resolve_*` and `combat/geometry.rs`
  read them). `StaticColliders` + `BakedColliderLibrary` are still required by
  `physics/static_world_colliders.rs:12` and `collision/streaming.rs:11`. Either delete
  `DerivedColliderLibrary`/`DerivedBuildingColliderLibrary`/`DerivedCollider`/`DerivedHull`/`HullFace`/
  `derive_collider`/`build_hull_from_points`/`triangulate_convex_hull` (~200 of the 309 lines) or
  `#[allow(dead_code)]` them pending a future unit-vs-world resolver.
* `server/src/collision/building_index.rs` (163) — `BuildingSpatialIndex`, `IndexedBuilding`,
  `sync_building_spatial_index`. Needed by `physics/static_world_colliders.rs:11`,
  `collision/streaming.rs:10`, `ai/obstacles.rs:6` (P4). Test at 104-162 stays green.
* `server/src/collision/streaming.rs` (287) — **KEEP**, re-anchor per §0.2. Test at 244-286 unaffected.
* `server/src/collision/mod.rs` (19) — drop lines 14 (`geometry`), 16 (`resolve_npc`),
  17 (`resolve_player`), 12 (`building_geometry`), and 18 (`resolve_vehicle`, P3).

### 1.7 `server/src/app/schedule.rs` (382) — remove from `configure_fps_fixed_schedule`

* `FpsServerSet::PhysicsWorld` block (94-109): delete lines 100-105 (all five `dynamic_actors::*`),
  keep 97-99 (`sync_terrain_colliders`, `sync_static_prop_colliders`, `sync_static_building_colliders`).
* `FpsServerSet::PhysicsControl` (157-167): delete the whole `add_systems` call — both members are
  `dynamic_actors::apply_*`. Remove `FpsServerSet::PhysicsControl` from the enum (50) and the `.chain()` tuple (70).
* `FpsServerSet::PhysicsPost` (169-186): delete 172-176 (`clamp_players_to_map_bounds`,
  `sync_players_from_physics`, `contacts::update_player_grounding_from_queries`, NPC + debug-box writeback).
  Lines 177-180 are ragdoll (P4). The set becomes empty → drop it (51, 71).
* `FpsServerSet::PlayerSim` (188-198): all three members die
  (`tick_player_jump_timers`, `handle_player_deaths`, `update_respawn_timers`) → drop the set (52, 72).
* `FpsServerSet::Indices` (200-210): delete 204 (`player::spatial::sync_player_spatial_index`);
  keep 203 (`player::index::sync_player_entity_index`). 205 is P4.
* `WorldTick` (80-92): keep all four survivors (86 `sync_building_spatial_index`,
  87 `update_static_collider_streaming`).
* Telemetry ordering constraints reference deleted systems — fix
  `handle_perf_core_phase_end.after(physics::dynamic_actors::sync_debug_boxes_from_physics)` (269-270).
* ⚠️ `configure_rail_fixed_schedule` (305-382) and `rail_mode_enabled()` (29-33) are P3's problem, but
  `configure_fixed_schedule` (35-42) branches on it — coordinate so this file compiles.

### 1.8 `server/src/app/resources.rs` (58) — remove inits

* line 29 `player::spatial::PlayerSpatialIndex`
* line 44 `physics::dynamic_actors::NpcPhysicsLodSettings`
* keep 28 (`PlayerEntityIndex`), 39-43 (building index, collider streaming state, terrain collider
  settings/registry, static world collider registry), 50 (`PlayerRosterCache`), 47/53 (`PlayerProfiles`,
  `ProfileIoQueue`).

### 1.9 Persistence — the delicate part

`server/src/net/connection.rs:132-346` `handle_disconnections` and
`server/src/persistence/autosave.rs:22-155` `update_periodic_player_save` both build a full
`PlayerProfile` from `(&Player, &PlayerPosition, &PlayerRotation, &PlayerVelocity, &Health,
&EquippedWeapon, &Inventory, &HotbarSelection, &PlayerProgression, Option<&InVehicle>,
Option<&RespawnTimer>)`.

**Edits (both files, identical shape):**
* Query becomes `(&Player, &CommanderView, &PlayerProgression)`.
* `position:` ← `view.0`; `rotation: 0.0`; `velocity: [0.0;3]` (or delete the fields, §1.10).
* Delete `health_*`, `equipped_weapon`, `weapon_ammo_in_mag`, `inventory_slots`, `hotbar_selection`,
  `in_vehicle`, `vehicle_*`, `is_dead`, `death_timestamp` usages.
* `connection.rs`: drop imports at 8-11 (`EquippedWeapon, Health, PlayerPosition, PlayerRotation,
  PlayerVelocity`), 12-15 (items), 25 (vehicle), 31 (`RespawnTimer`); drop the vehicle-driver clear
  loops at 243-274 and 334-338.
* `autosave.rs`: drop imports at 4-10 and the `vehicles` query (39).
* `connection.rs:57-127` `handle_connections` — remove the `MessageReceiver`/`MessageSender`
  registrations for messages killed in P1/P2/P3 (84-114, 116-126). Keep `SubmitPlayerName`,
  `RequestPlayerRoster`, `NameSubmissionResult`, `PlayerRoster`, `ReplicationSender`, and `PlayerInput`
  only if the trimmed input message survives (§4).

### 1.10 `shared/src/player_profile.rs` (196) — **PERSISTED FORMAT, READ §DANGER**

`PROFILE_VERSION = 1` (line 14). Fields to strip: `rotation` (29), `velocity` (31),
`health_current`/`health_max` (35/37), `equipped_weapon` (39), `weapon_ammo_in_mag` (41),
`inventory_slots` (45), `hotbar_selection` (47), the six `in_vehicle`/`vehicle_*` fields (51-61),
`is_dead`/`death_timestamp` (65/67). Keep `version`, `player_name`, `position` (the commander's saved
view focus), `level`, `prestige`, `reputation`, `stamina`, `intelligence`, `bank_gold`, `last_login`,
`total_playtime_secs`. `PlayerProfile::new_player` (98-195) loses its whole starting-inventory body.

---

## 2. Client — camera, input, character rendering

### 2.1 DELETE — `client/src/render/systems/player/` (1391 lines, whole directory)

| File | Lines | Contents |
|---|---|---|
| `mod.rs` | 179 | `CharacterAssets`, `PlayerCharacterAssets`, `PlayerModelRoot`, `PlayerShadowState`, `NeedsPlayerRigSetup`, `PlayerAnimationRoot`, `PlayerRigOwner`, `PlayerRigCharacter`, `MovementAnim`, `PlayerAnimState`, `LocalPlayerModel`, `LastCameraMode` |
| `animation.rs` | 475 | `setup_player_rig`, `update_player_animation`, `determine_local_target_anim`, `determine_remote_target_anim`, `movement_anim_to_node` |
| `assets.rs` | 115 | `setup_player_character_assets` — loads `characters/custom/basemodel.glb` + `oilman_animated.glb` animation graphs |
| `spawn.rs` | 258 | `ensure_local_player_tag`, `handle_player_spawned`, `sync_player_character_models`, `spawn_player_model`, `despawn_with_children` |
| `sync.rs` | 232 | `sync_player_transforms` + `LocalPlayerNetDebugWindow` (`CITYSIM_NET_DEBUG`, `CITYSIM_NET_DEBUG_INTERVAL_SECS`) |
| `visibility.rs` | 112 | `update_local_player_visibility` (FP/TP model hide), `update_player_shadow_culling`, `apply_player_shadow_state_to_new_meshes` |
| `ids.rs` | 20 | `peer_id_to_u64` (duplicate of `client/src/camera.rs:36`) |

**Verified:** nothing outside this directory imports its symbols except
`client/src/app_wiring/resources.rs:34` (`LastCameraMode`) and `client/src/app_wiring/systems.rs`
(via the `pub use player::*` glob in `client/src/render/systems/mod.rs:19`). `weapon_view` attaches
weapons by transform offset, not by rig bone, so it has **no** compile dependency on this module.

Then edit `client/src/render/systems/mod.rs`: delete `mod player;` (9) and `pub use player::*;` (19).

⚠️ **Salvage first:** `animation.rs` is the only place that knows the animation-graph node ordering for
`basemodel.glb`/`oilman_animated.glb`, and `assets.rs:14-110` documents which `#AnimationN` index is
which clip (Oilman: 0 tpose, 1 idle, 2 jog fwd, 3 jog back, 4 strafe R, 5 strafe L, 6 run, 7 jump,
8 driving, 9 look-behind-run). Those `.glb`s are **KEEP** (they become the tactics units). Copy the
index table into the new unit-rendering module or a comment before deleting.

### 2.2 REWRITE — `client/src/camera.rs` (347 → ~110)

Everything in this file is FPS/vehicle-specific. Delete:
`CAMERA_HEIGHT_OFFSET` (14), `VEHICLE_FP_SEAT_HEIGHT`/`HOVERBIKE_FP_SEAT_HEIGHT`/`VEHICLE_FP_SEAT_FORWARD`
(16-18), `THIRD_PERSON_*` (21-23), `FOV_*` (26-29), `SNIPER_FISHEYE_*` (31-33),
`update_camera` (46-122), `first_person_target` (124-177), `is_moving_on_foot` (180),
`is_sprinting_on_foot` (187), `third_person_target` (195-236), `vehicle_bob_offset` (238-247),
`orbit_position` (250-265), `look_at_level` (268-273), `update_camera_fov` (276-319),
`update_sniper_fisheye` (322-347).

**Must survive in this file (external consumers on the KEEP list):**
* `pub(crate) const CAMERA_NEAR_CLIP: f32` (line 15) — read by
  `client/src/render/systems/rendering/setup.rs:99`.
* `pub(crate) fn peer_id_to_u64(peer_id: PeerId) -> u64` (36-43) — read by
  `client/src/audio/mod.rs:37`, `client/src/audio/vehicles.rs:318` (P3),
  `client/src/weapons/mod.rs:73` (P1). Audio is KEEP → **keep this helper** (or move it to a
  `client/src/net_ids.rs`). Note the `render/systems/player/ids.rs:6` copy handles `PeerId::Entity`
  and `PeerId::Raw` which the camera.rs copy does not — port the more complete version.

Then paste in the harvested `CommanderCamera` + `update_commander_camera` + `apply_rts_transform` from
§0.3. Also drop the `SniperFisheye` insert at `client/src/render/systems/rendering/setup.rs:122` and
the `SniperFisheyePlugin` at `client/src/app_wiring/plugins.rs:113` (P1 owns the renderer itself).

### 2.3 REWRITE — `client/src/input.rs` (416 → ~120)

`InputState` (30-76) is consumed by 33 files; most die in P1/P2/P3 but **these KEEP files read it**:

| File:line | Field |
|---|---|
| `client/src/render/systems/connection.rs:196,199` | `ui_blocking()` (cursor grab) |
| `client/src/ui/modal.rs:109,120` | `ui_blocking()` |
| `client/src/ui/pause_menu/actions.rs:15,98,108,138,455,461,465` | `pause_menu_open`, `inventory_open`, `map_open`, `debug_menu_open` |
| `client/src/ui/world_map/layout.rs:9,15,16,17,48,55` | `inventory_open`, `pause_menu_open`, `debug_menu_open`, `map_open` |
| `client/src/ui/inventory/layout.rs:72,78,79,80,87,99` | (P2) |
| `client/src/ui/debug_time_menu/{actions.rs:17,22,53,118,220,223, layout.rs:362,371, state_sync.rs:7,11}` | `debug_menu_open`, `fly_mode`, others |
| `client/src/audio/ambient.rs:41,50,51` | `forward/backward/left/right`, `in_vehicle` |
| `client/src/audio/vehicles.rs:246,257,300,310` | `in_vehicle` (P3) |

**Minimum surviving `InputState`:** `inventory_open` (P2 may remove), `pause_menu_open`, `map_open`,
`debug_menu_open`, `ui_blocking()`. Everything else (`forward/backward/left/right/jump/yaw/pitch/
interact/interact_just_pressed/blocking_held/shift/camera_mode/in_vehicle/vehicle_look_*/aiming/
is_dead/fly_mode/fly_down`) goes.

Delete: `CameraMode` enum (21-26) — also referenced by `client/src/crosshair/{mod.rs,hud.rs}` (P1),
`client/src/weapon_view/{offhand,third_person,view_model}.rs` (P1), `client/src/weapons/{input,mod}.rs` (P1);
`handle_keyboard_input` (115-161), `handle_mouse_input` (164-228), `peer_id_to_u64` (231-238),
`update_vehicle_state` (242-264), `update_death_state` (267-284), `handle_send_input_to_server` (287-416),
`INPUT_HEARTBEAT_SECS`/`INPUT_CHANGE_BURST_TICKS` (17-18).

Keep `crate::render::systems::InputSettings` (`client/src/render/systems/rendering/settings.rs:183-195`,
`mouse_sensitivity`) — the pause menu exposes it and the new camera should honour it.

**`client/src/audio/ambient.rs:38-60` `update_desert_walking_ambient` is a KEEP-list system that will
not compile**: it queries `Query<&PlayerPosition, With<LocalPlayer>>` (43) and reads
`input_state.{forward,backward,left,right,in_vehicle}` (50-51). Rework it to key off camera-focus
biome + camera pan velocity, or gate it out.

### 2.4 `client/src/app_wiring/` edits

* `systems.rs:150-397` `wire_fps_systems` — delete lines 161 (`setup_player_character_assets`),
  204-208 (`handle_player_spawned`, `sync_player_character_models`, `ensure_local_player_tag`),
  218-221 (`handle_keyboard_input`, `update_vehicle_state`, `handle_mouse_input`, `update_death_state`),
  228 (`sync_player_transforms`), 231 (`camera::update_camera`), 237-238
  (`update_camera_fov`, `update_sniper_fisheye`), 246-256 (the whole player-visuals block), and the
  `FixedUpdate` input sender at 194-198. Repoint 394-395 `.after(...)` constraints.
* `resources.rs:33` `input::InputState` — keep (trimmed struct). `resources.rs:34`
  `game_systems::LastCameraMode` — **delete** (type is gone).
* `plugins.rs:113` `render::sniper_fisheye::SniperFisheyePlugin` — delete.
* `mod.rs:33-36` — the `use crate::{...}` list drops `camera`? No: `camera` stays (new module).

### 2.5 Client KEEP-list systems that reference player components (must be rewired)

| File:line | Symbol | Fix |
|---|---|---|
| `client/src/streaming.rs:11,15` | `LocalPlayer`, `PlayerPosition` | §0.1 |
| `client/src/audio/mod.rs:30` | `LocalPlayer, Npc, Player, PlayerPosition` imports | trim |
| `client/src/audio/ambient.rs:41,43,50,51` | `InputState` + `PlayerPosition` | §2.3 |
| `client/src/audio/remote_players.rs:152` | `Query<(Entity,&Player,&Transform), Without<LocalPlayer>>` | becomes the unit-audio emitter; keep the `Camera3d` listener queries at 17, 251, 261 |
| `client/src/water/overlay.rs:47` | `Query<&PlayerWaterState, With<LocalPlayer>>` | commanders never submerge → drive `update_underwater_overlay` from `camera.translation().y <= water_level` (the camera query at 48 is already there) |
| `client/src/water/material.rs:192` | `emit_water_ripples(waders: Query<(Entity,&PlayerPosition,&PlayerWaterState)>)` | no waders → delete the system (registered at `water/mod.rs:60`) or repoint at units in a later phase |
| `client/src/water/mod.rs:30` | import | trim |
| `client/src/ui/world_map/markers.rs:8` | `Query<(&PlayerPosition,&PlayerRotation), With<LocalPlayer>>` | **world map is KEEP** — draw the camera focus arrow from `CommanderCamera.focus` + `.yaw` (note the yaw convention comment at 27-40: UI angle = `-yaw`) |
| `client/src/ui/world_map/mod.rs:26` | import | trim |
| `client/src/ui/debug_time_menu/state_sync.rs:22,40-41` | `Query<&PlayerCharacter, With<LocalPlayer>>` | delete `sync_debug_character_selection` + `update_character_button_label` if `PlayerCharacter` goes |
| `client/src/ui/debug_time_menu/actions.rs:74,183-186` | `local_player_transforms` for `SpawnPhysicsBoxDebug.anchor_position` | use camera focus, or delete with the debug-box message |
| `client/src/ui/debug_time_menu/actions.rs:118` | `input_state.fly_mode` toggle button | remove the button |
| `client/src/render/systems/rendering/setup.rs:99` | `crate::camera::CAMERA_NEAR_CLIP` | keep the const (§2.2) |
| `client/src/render/systems/rendering/setup.rs:122` | `SniperFisheye::default()` | delete |
| `client/src/render/systems/connection.rs:192-213` | `apply_cursor_grab` uses `InputState::ui_blocking` | RTS wants a *free* cursor — replace with the harvested `release_cursor_for_rts` |
| `client/src/render/systems/connection.rs:225,243-245` | despawns `Query<Entity, With<Player>>` on menu exit | keep, still correct for commanders |
| `client/src/render/systems/npc/{mod.rs:30-31, visibility.rs:42}` | `LocalPlayer`+`PlayerPosition` | P4 territory; if P4 lands after P5 it breaks here |

---

## 3. Shared

### 3.1 `shared/src/components/actors.rs` (191)

Delete: `PlayerPosition` (118-119), `PlayerRotation` (122-123), `PlayerVelocity` (126-127),
`PlayerGrounded` (131-139 + `impl` 170-183), `PlayerWaterState` (142-148), `PlayerJumpState` (151-155),
`PlayerMeleeState` (158-164, P1), `FlyMode` (167-168).
Keep: `Player` (6-9), `PlayerProgression` (12-24), `LocalPlayer` (186-187, still the "this commander is
me" tag), `Ground` (190-191). `PlayerCharacter` (42-47) — see §1.1.
Add `CommanderView(pub Vec3)`.
`Npc*`/`DebugPhysicsBox*` in this file are P4's.

### 3.2 `shared/src/physics/` — delete `character.rs`, keep constants

* `shared/src/physics/character.rs` (190) — **DELETE**. `step_character` has exactly one caller,
  `server/src/player/movement.rs:8,98,111`.
* `shared/src/physics/mod.rs` (8) — drop `mod character;` (3) and `pub use character::step_character;` (6).
* `shared/src/physics/constants.rs` (50) — keep **only** `GRAVITY` (4), used by
  `server/src/app/mod.rs:14,24` for the Rapier world; and `WALKABLE_THRESHOLD` (44) **only if** you keep
  it for LOS slope checks (its consumers `collision/geometry.rs:5`, `building_geometry/mod.rs:8`,
  `physics/contacts.rs:7` all die). Delete `MOVE_ACCEL`, `MOVE_BRAKE`, `GROUND_SNAP_DISTANCE`, all
  `WATER_*`, `JUMP_VELOCITY`, `FLY_SPEED`, `FLY_FAST_MULT`, `ground_clearance_center()`.
  ⚠️ `ground_clearance_center()` still has callers in `server/src/ai/{tick/mod.rs:14, pathfinding.rs:4,
  spawn.rs:15}` (P4) — sequence accordingly.
* `shared/src/prelude.rs:11` re-exports `ground_clearance_center, step_character`; line 7-9 re-exports
  `PlayerPosition, PlayerRotation, PlayerVelocity, LocalPlayer` — update both.

### 3.3 `shared/src/player.rs` (31)

Delete `PLAYER_SPEED` (4), `PLAYER_SPRINT_MULT` (7), `PLAYER_HEIGHT` (10), `PLAYER_RADIUS` (13),
`JUMP_ANIM_MIN_SECS` (16), `STEP_UP_HEIGHT` (19), `PLAYER_MAX_HEALTH` (28), `RESPAWN_TIME` (31).
Keep `MOUSE_SENSITIVITY` (22, camera look) and `SPAWN_POSITION` (25, commander default focus — also
referenced by `shared/src/player_profile.rs:8,157`).
⚠️ `PLAYER_HEIGHT`/`PLAYER_RADIUS`/`STEP_UP_HEIGHT` still have P3/P4 callers
(`server/src/ai/*`, `server/src/combat/*`, `shared/src/npc.rs`) — sequence accordingly.

---

## 4. Protocol / replication (question: what registration sites change)

`shared/src/protocol/plugin.rs` `ProtocolPlugin::build`:

* **Delete component registrations:** `PlayerPosition` (31), `PlayerRotation` (32), `PlayerVelocity` (33),
  `PlayerJumpState` (34), `PlayerMeleeState` (35), `PlayerWaterState` (36-37).
* **Keep:** `Player` (30), `PlayerProgression` (38-39), `PlayerCharacter` (40, conditional).
* **Add:** `app.register_component::<CommanderView>().add_prediction();`
* **Message churn owned by P5:** `SpawnPlayer` (107-108) and `SetPlayerCharacter` (121-122) if
  `PlayerCharacter` goes. `PlayerInput` (109-110) survives but is gutted (§4.1).
  `MeleeAttackRequest`/`ShootRequest`/`SwitchWeapon`/`ReloadRequest` are P1's;
  `SpawnPhysicsBoxDebug`/`SpawnOilmanDebug` are P4's.
* **Channels:** `ReliableChannel` (183-188) and `InputChannel` (190-195) survive (KEEP list says the
  channel/plugin structure stays). `RagdollPoseChannel` (197-201) is P4's.
* Update the imports at lines 4-10 accordingly.

### 4.1 `shared/src/protocol/messages.rs` — `PlayerInput`

`pub struct PlayerInput` (line 12-33) + `PackedPlayerInput { flags: u16, yaw_q: u16, throttle_q, brake_q,
steer_q }` (35-42) + its `Serialize`/`Deserialize` impls. Delete `forward/backward/left/right/jump/
fly_mode/fly_down/fly_fast/vehicle_input/interact/block`. What remains of a *commander* input is a
camera focus + selection/order stream, so P6's "slim the PlayerInput packed flags" work effectively
starts here. Interim: either delete `PlayerInput` end-to-end (and the client sender
`client/src/input.rs:388,402,411` + `server/src/net/input.rs`), or reduce it to `{ focus: Vec3 }` so the
server can keep streaming colliders around a moving commander without a full order protocol.

`server/src/net/input.rs` (92) — `ClientInputs { latest, latest_by_driver_id }` (12-16) and
`handle_client_input_messages` (52-91). `latest_by_driver_id` exists only for vehicles (P3) — drop it.
`ClientInputIngressStats` (19-49) survives.

---

## 5. Assets

* **KEEP** `client/assets/characters/custom/basemodel.glb` (884 KB) and `oilman_animated.glb` (4.3 MB) —
  explicitly on the KEEP list, they become the tactics units.
* **`client/assets/characters/custom/sarah_animated.glb` (7.2 MB) is referenced by ZERO source files**
  (verified: `rg -in sarah` across `client/src server/src shared/src editor/src` → no hits).
  Free deletion, though it belongs to P6 cleanup.
* No other asset is P5-exclusive. `client/assets/colliders.bin` (160 KB) stays — required by
  `setup_baked_colliders` for the static prop/building colliders that survive.

---

## 6. Cargo

Nothing becomes unused **by P5 alone**. `bevy_rapier3d` (workspace + `server/Cargo.toml:9`) is
explicitly retained for terrain heightfields + LOS raycasts. If a later decision drops LOS raycasts,
`bevy_rapier3d` falls out of `server/Cargo.toml` and `Cargo.toml:[workspace.dependencies]` and
`server/src/app/mod.rs:9-10,43` loses `RapierPhysicsPlugin`/`RapierConfiguration`. The editor already
does not depend on rapier.

---

## DANGER

1. **`server_data/players/*.bin` will be silently discarded.** 49 profiles exist today. `PlayerProfile`
   is serialized with **bincode**, which is *not* self-describing — `#[serde(default)]` on
   `player_profile.rs:71-87` does **nothing** for bincode. Removing *any* field changes the byte layout,
   so `bincode::deserialize` on an old file either errors or silently mis-parses. The error path is
   `profiles.rs:47-49 → Err(...)` → `spawn.rs:95-99` "Creating new profile" → **level/prestige/bank_gold
   lost**. And `PlayerRosterCache::from_storage_dir` (`roster_cache.rs:25-50`) silently `continue`s on
   deserialize failure, so the roster empties too.
   → **Bump `PROFILE_VERSION` to 2** so `load_profile` takes the explicit version branch
   (`profiles.rs:51-64`) and writes a `.v1.backup` copy, *or* write a one-shot migration that reads v1
   and emits v2 with `{name, position, level, prestige, reputation, stamina, intelligence, bank_gold,
   last_login, total_playtime_secs}` preserved.
2. **Pre-existing bug you will inherit:** `server/src/net/connection.rs:278` saves
   `player_name: name_lower` (lowercased) while `spawn.rs:97` creates profiles with the original case.
   If you touch the profile writer, fix this rather than propagate it.
3. **Client terrain/prop streaming stops** the moment `LocalPlayer + PlayerPosition` disappears if §0.1
   is not done first: `streaming_anchor` falls through to `camera.iter().next()` **only** because of the
   rail branch; with rail deleted and no player, `streaming_anchor` still returns the camera translation
   (line 27) — but the *rail* import at line 13 makes the file fail to compile as soon as P3 lands. Order:
   §0.3 harvest → §0.1 rewrite → then any deletion.
4. **Server terrain colliders collapse to chunk (0,0)** if §0.2 is skipped. `gather_centers`
   (`terrain_colliders.rs:122-152`) walks players → vehicles → NPCs → `ChunkCoord::new(0,0)`. All three
   real sources die across P3/P4/P5. Symptom: LOS raycasts pass through hills more than ~6 chunks from
   origin; no crash, no log.
5. **`physics/queries.rs` becomes dead code after P1** and will trip `#[deny(warnings)]` /
   `cargo clippy -- -D warnings` in `.github/`. Wire an LOS helper or `#[allow(dead_code)]` it in the
   same commit.
6. **`DerivedColliderLibrary` / `DerivedBuildingColliderLibrary` become orphaned resources** once
   `collision/geometry.rs` + `building_geometry/` go — `setup_baked_colliders`
   (`library.rs:82-132`) still builds and inserts them (the ~200-line `triangulate_convex_hull` runs at
   startup). Delete them or accept the startup cost.
7. **`shared/src/map/schema.rs:16` `player_spawn: Option<[f32;3]>` MUST stay.** The editor writes it
   (`editor/src/worldgen.rs:833,835`, `editor/src/tools.rs:471,562,801,943`) and validates it
   (`schema.rs:37-39`). Removing it breaks every saved `.ron` map under `client/assets/maps` (124 MB)
   and the editor's spawn-marker tool. Repurpose it as the commander's initial camera focus.
8. **Phase-ordering coupling.** P5 deletes `collision/geometry.rs` + `building_geometry/`, which P3's
   `collision/resolve_vehicle.rs` and P4's `collision/resolve_npc.rs` both import
   (`resolve_vehicle.rs:10,12`, `resolve_npc.rs:12,14`). Likewise P4's `ai/ragdoll::CorpseCollisionIndex`
   is imported by `resolve_player.rs:12`. **Run P3 and P4 before P5, or expect a broken tree between
   commits.** Similarly `physics/dynamic_actors.rs` imports `crate::ai::ragdoll::NpcRagdoll` (line 29).
9. **`InputState` is a shared UI mutex**, not just gameplay input. Six KEEP-list UI modules write
   `pause_menu_open`/`map_open`/`inventory_open`/`debug_menu_open` and read `ui_blocking()`. Do **not**
   delete the resource — trim it.
10. **`crate::camera::peer_id_to_u64` has a KEEP-list consumer** (`client/src/audio/mod.rs:37`).
    `client/src/render/systems/player/ids.rs:6` has the *more correct* implementation (handles
    `PeerId::Entity` and `PeerId::Raw`); port that one before deleting the directory.
11. **Two `peer_id_to_u64` copies on the server too** — `server/src/net/peer.rs` (keep) vs the client
    ones. Don't accidentally delete the server one; it is used by `connection.rs:239,335`.

---

## Execution checklist

- [ ] Harvest `RtsRailCamera` + `apply_rts_transform` + `update_rts_camera` + `release_cursor_for_rts` + `intersect_terrain` out of `client/src/rail/mod.rs` into `client/src/camera.rs` (§0.3)
- [ ] Add `CommanderView(Vec3)` to `shared/src/components/actors.rs`; register in `shared/src/protocol/plugin.rs`
- [ ] Rewrite `client/src/streaming.rs` to a camera-only anchor; trim the 7 consumers' param lists (§0.1)
- [ ] Re-anchor `server/src/physics/terrain_colliders.rs:122-165` and `server/src/collision/streaming.rs:62-103` to `CommanderView` (§0.2)
- [ ] Rewrite `client/src/camera.rs` keeping `CAMERA_NEAR_CLIP` + `peer_id_to_u64` (§2.2)
- [ ] Trim `client/src/input.rs` to the UI-modal flags only (§2.3)
- [ ] Delete `client/src/render/systems/player/`; edit `render/systems/mod.rs:9,19` (§2.1)
- [ ] Edit `client/src/app_wiring/{systems.rs,resources.rs,plugins.rs}` (§2.4)
- [ ] Fix KEEP-list client consumers: audio ambient, water overlay/ripples, world-map marker, debug-time menu (§2.5)
- [ ] Delete `server/src/player/{movement,lifecycle,spatial}.rs`; edit `player/mod.rs` (§1.2/1.3)
- [ ] Rewrite `server/src/player/spawn.rs` to the minimal commander bundle (§1.3)
- [ ] Delete `server/src/physics/{dynamic_actors,contacts}.rs`; edit `physics/mod.rs`; slim `physics/layers.rs`; keep `queries.rs` as the LOS API (§1.4/1.5)
- [ ] Delete `server/src/collision/{resolve_player,resolve_npc,geometry}.rs` + `building_geometry/`; edit `collision/mod.rs` (§1.6)
- [ ] Edit `server/src/app/{schedule.rs,resources.rs}` (§1.7/1.8)
- [ ] Rewrite the two profile writers (`net/connection.rs`, `persistence/autosave.rs`) (§1.9)
- [ ] **Bump `PROFILE_VERSION` to 2** and strip `PlayerProfile` (§1.10, DANGER 1)
- [ ] Delete `shared/src/physics/character.rs`; slim `constants.rs` and `shared/src/player.rs`; fix `shared/src/prelude.rs` (§3.2/3.3)
- [ ] Slim `shared/src/components/actors.rs` and `shared/src/protocol/{plugin.rs,messages.rs}` (§3.1/§4)
- [ ] `cargo check -p editor` — must be untouched and green
- [ ] Boot server + client, verify: terrain streams under a panning camera, props/LOD stream, water renders, world map opens, pause menu opens, name entry → profile save/load round-trips
