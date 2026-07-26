# P1 — Weapons & Combat removal manifest

Repo: `/Users/terminator2/Coding/citysim` (Bevy 0.18.1 + lightyear 0.26).
Recovery: everything is preserved at git tag `citysim-final` (`git show citysim-final:<path>`).

Scope of this manifest: guns, bullets/ballistics, melee (sword/shield/block), damage/hit-zones,
weapon view models, ammo HUD, crosshair/hit markers/death screen, blood/muzzle/impact FX,
sniper ADS + fisheye post-process, server combat simulation, and all combat protocol traffic.

**This manifest deliberately does NOT delete four things that live inside the combat tree but
are not combat.** They are listed under "RESCUE BEFORE DELETING" and must be moved first, or
the KEEP list (props/foliage debug, FPS/perf overlay, server line-of-sight raycasts) breaks.

---

## 0. RESCUE BEFORE DELETING (do this first — these are on the KEEP path)

### R1. FPS / perf overlay lives inside `client/src/weapons/`
`client/src/weapons/debug.rs` + `client/src/weapons/state.rs` contain the entire client
performance overlay, which has nothing to do with weapons.

Move these to a new module (suggest `client/src/debug_overlay/mod.rs`, or fold into the
existing `client/src/profiling.rs`):

| Symbol | Current location |
| --- | --- |
| `DebugOverlay`, `FpsText`, `PerfStatsText` (components) | `client/src/weapons/state.rs:9,13,17` |
| `ClientPerfConfig` (+ `Default` impl reading `FISTFORCE_CLIENT_PERF`, `FISTFORCE_CLIENT_PERF_INTERVAL_SECS`, `FISTFORCE_HITCH_THRESHOLD_MS`) | `client/src/weapons/state.rs:249-277` |
| `ClientPerfSnapshot` | `client/src/weapons/state.rs:281` |
| `PerfOverlayEnabled` (F3 toggle) | `client/src/weapons/state.rs:312` |
| `PerfDropMonitor` | `client/src/weapons/state.rs:316` |
| `handle_toggle_perf_overlay` (F3) | `client/src/weapons/debug.rs:76` |
| `update_client_perf_snapshot` | `client/src/weapons/debug.rs:87` |
| `emit_client_perf_summary` | `client/src/weapons/debug.rs:134` |
| `percentile_sorted` (private helper) | `client/src/weapons/debug.rs:166` |
| `spawn_debug_overlay` / `update_debug_overlay` / `despawn_debug_overlay` | `client/src/weapons/debug.rs:177,231,455` |
| `update_perf_drop_monitor` | `client/src/weapons/debug.rs:393` |

While moving `update_debug_overlay`, delete these ParamSet members (combat-only counters):
`Query<(), With<Bullet>>` (`debug.rs:250`), `Query<(), With<LocalTracer>>` (`debug.rs:251`),
`Query<(), With<MuzzleSmoke>>` (`debug.rs:254`), `Query<(), With<MuzzleFlash>>` (`debug.rs:257`),
and drop `Bullets: {} (local tracers: {})` / `Muzzle smoke: {} | Flash: {}` from the format
string at `debug.rs:345` (plus the corresponding `counts_a.p3()/p4()/p7()` and `counts_b.p0()`
args). Everything else it reads (`LoadedChunks`, `EnvironmentProp`, `ClientDerivedColliderLibrary`,
`SandParticle`, `CloudLayer`, `CloudCard`, `Vehicle`, asset counts) is KEEP-list or P3.

### R2. `WeaponDebugMode` is the global F4 gizmo toggle, used by props (KEEP list)
`shared/src/weapons/debug.rs:5` defines `pub struct WeaponDebugMode(pub bool)`.
Consumers OUTSIDE combat:
- `client/src/props/debug.rs:5,147` — `debug_draw_prop_colliders` (props are KEEP)
- `client/src/render/systems/npc/debug.rs:29,119` and `npc/mod.rs:39` (P4, but still must compile until P4)
- `client/src/app_wiring/resources.rs:7` (`init_resource`), `client/src/app_wiring/mod.rs:31` (import)

Action: move it to e.g. `shared/src/debug.rs` as `DebugGizmoMode`, keep the `F4` toggle system
(`handle_toggle_debug_mode`, `client/src/weapons/debug.rs:66`) alongside the perf overlay in R1.

### R3. Server raycast/segment helpers used for line-of-sight live in `server/src/combat/geometry.rs`
The P5 plan explicitly keeps rapier raycast queries for line-of-sight. These three functions in
`server/src/combat/geometry.rs` are generic world-segment queries, not bullet-specific:
- `segment_terrain_intersection` (line 306) — also has a test at line 563
- `segment_props_intersection` (line 362)
- `segment_buildings_intersection` (line 431)
- private helper `ray_triangle_intersection` (line 487) which they depend on

They are currently `pub(super)`, i.e. only visible inside `combat::`. Move them to a surviving
module (suggest `server/src/collision/raycast.rs` next to the existing
`server/src/collision/geometry.rs`) and re-export, then delete the rest of the file.

The rest of `geometry.rs` IS combat-only and can go: `ray_sphere_intersection` (12),
`ray_sphere_intersection_exact` (28), `ray_oriented_capsule_intersection` (56),
`ray_capsule_intersection` (124), `ray_obb_intersection` (161), plus the `#[cfg(test)]` modules
at lines 270 and 531 (keep only `segment_terrain_intersection_returns_world_distance`, line 563).

### R4. `Pickable` UI marker lives in `client/src/crosshair/mod.rs`
`client/src/crosshair/mod.rs:61` defines `pub struct Pickable` with `Pickable::IGNORE`.
It is used by `client/src/water/overlay.rs:32` (water underwater overlay — KEEP list), reached
via a glob import. Move `Pickable` to `client/src/ui/mod.rs` (or `client/src/ui/styles.rs`)
before deleting `crosshair/`.

---

## 1. DELETE WHOLESALE (verified to exist)

### Client
- [ ] `client/src/weapons/` — entire dir (after R1/R2 rescue)
  - `mod.rs`, `assets.rs`, `debug.rs`, `input.rs`, `paths.rs`, `projectiles.rs`, `state.rs`, `warmup.rs`
  - `effects/mod.rs`, `effects/blood.rs`, `effects/impacts.rs`, `effects/muzzle.rs`
- [ ] `client/src/weapon_view/` — entire dir
  - `mod.rs`, `assets.rs`, `hotbar_input.rs`, `melee_models.rs`, `offhand.rs`, `slash_trail.rs`,
    `third_person.rs`, `view_model.rs`, `weapon_hud.rs`
- [ ] `client/src/crosshair/` — entire dir (after R4 rescue)
  - `mod.rs`, `death_screen.rs`, `hit_markers.rs`, `hud.rs`
- [ ] `client/src/render/sniper_fisheye.rs` (277 lines)
- [ ] `client/src/render/sniper_fisheye.wgsl` (28 lines)

### Server
- [ ] `server/src/combat/` — entire dir (after R3 rescue of geometry.rs segment fns)
  - `mod.rs`, `bullet_sim.rs`, `cleanup.rs`, `fire.rs`, `geometry.rs`, `hit_characters.rs`,
    `hit_world.rs`, `melee.rs`, `reload.rs`, `target_index.rs`

### Shared
- [ ] `shared/src/weapons/` — entire dir (after R2 rescue of `WeaponDebugMode`)
  - `mod.rs`, `ballistics.rs`, `constants.rs`, `damage.rs`, `debug.rs`, `melee.rs`, `offsets.rs`, `types.rs`
- [ ] `shared/src/components/combat.rs` — **conditional**, see §4 (Health decision)

### NOT deleted (explicitly checked, do not touch)
- `client/src/render/lod.rs` — generic `VisibilityRangeBuilder` / `LodPolicy` / `LodLevel` /
  `apply_lod_visibility` / `build_lod_visibility_range` / `ShadowCullPolicy`. Consumed by
  `client/src/props/lod/mod.rs:19` (KEEP list). Nothing in it is combat-related despite being
  named in the task brief.
- `client/src/render/systems/particles.rs` — **NOT combat.** It is `SandParticle` /
  `ParticleAssets` / `setup_particle_assets` / `spawn_sand_particles` / `update_sand_particles`,
  driven purely by `shared::vehicle::{Vehicle, VehicleState}`. This belongs to **P3 (vehicles)**,
  not P1. Blood/muzzle/impact particles live in `client/src/weapons/effects/` instead.
- `client/src/render/shadow_cull.rs`, `client/src/render/hierarchy_fix.rs` — unrelated.

---

## 2. EDIT: files that survive but must change

### 2a. Client module wiring
| File | Change |
| --- | --- |
| `client/src/main.rs:8,21,22` | remove `mod crosshair;`, `mod weapon_view;`, `mod weapons;` |
| `client/src/render/mod.rs:4` | remove `pub mod sniper_fisheye;` |
| `client/src/app_wiring/mod.rs:31` | remove `use shared::weapons::WeaponDebugMode;` (or repoint to rescued module) |
| `client/src/app_wiring/mod.rs:34-35` | drop `crosshair`, `weapon_view`, `weapons` from the `use crate::{...}` list |

### 2b. `client/src/app_wiring/resources.rs`
Remove `init_resource` calls (lines 7-23), keeping only rescued ones:
- delete: `weapons::ShootingState` (10), `weapons::MeleeSwingState` (11),
  `weapon_view::offhand::OffhandShieldIndex` (12), `weapons::ShootInputSuppress` (13),
  `weapons::ReloadState` (14), `weapons::DebugBulletTrails` (15),
  `weapons::PlayerOwnerIndex` (18), `weapons::RemoteMuzzleIndex` (19),
  `weapons::WeaponWarmupQueue` (20), `weapon_view::CurrentWeaponView` (21),
  `weapon_view::CurrentThirdPersonWeapon` (22), `weapon_view::RemoteWeaponIndex` (23)
- repoint (R1/R2): `WeaponDebugMode` (7), `weapons::PerfOverlayEnabled` (8),
  `weapons::PerfDropMonitor` (9), `weapons::ClientPerfConfig` (16), `weapons::ClientPerfSnapshot` (17)

### 2c. `client/src/app_wiring/plugins.rs`
- line 113: remove `app.add_plugins(render::sniper_fisheye::SniperFisheyePlugin);`

### 2d. `client/src/app_wiring/systems.rs` (the big one — `wire_fps_systems`)
Remove from `Startup` (lines 152-164): `weapons::setup_weapon_visual_assets`,
`weapons::setup_weapon_audio_assets`, `weapon_view::setup_weapon_model_assets`.

Remove from `OnEnter(GameState::Playing)` (167-177): `crosshair::spawn_crosshair`,
`crosshair::spawn_death_screen`, `weapon_view::spawn_weapon_hud`,
`weapons::cleanup_shoot_input_suppress`. (`weapons::spawn_debug_overlay` → rescued name.)

Remove from `OnExit(GameState::Playing)` (180-191): `crosshair::despawn_crosshair`,
`crosshair::despawn_death_screen`, `weapon_view::despawn_weapon_hud`,
`weapon_view::despawn_third_person_weapon`, `weapon_view::despawn_remote_third_person_weapons`,
`weapons::reset_projectile_indices`. (`weapons::despawn_debug_overlay` → rescued name.)

Remove `camera::update_sniper_fisheye` (line 238).

Delete these whole `add_systems` blocks:
- 280-289 crosshair block (`update_crosshair_visibility`, `update_crosshair_ads`,
  `update_hit_markers`, `update_death_screen`)
- 291-308 weapons block (`sync_player_owner_index`, `sync_remote_muzzle_index`,
  `update_weapon_warmup_queue`, `spawn_weapon_warmups`, `cleanup_weapon_warmups`,
  `handle_shoot_input`, `handle_reload_input`, `handle_weapon_sounds`, `handle_bullet_spawned`,
  `update_recoil_recovery`) — **but keep `weapons::update_client_perf_snapshot` (297)** under
  its rescued path
- 310-318 bullet visuals block (`update_bullet_visuals`, `update_local_tracers`,
  `handle_bullet_impacts`)
- 320-332 FX block (`update_impact_markers`, `update_blood_bursts`, `update_blood_droplets`,
  `update_blood_ground_splats`, `update_muzzle_smoke`, `update_muzzle_flash`, `handle_hit_confirms`)
- 344-347 `weapons::update_trajectory_debug_gizmos`
- 372-397 the entire `weapon_view` block (`handle_weapon_switch`, `update_weapon_hud`,
  `update_first_person_weapon`, `update_third_person_weapon`, `update_remote_third_person_weapons`,
  `offhand::*` ×4, `animate_third_person_melee`, `update_weapon_animation`,
  `slash_trail::spawn_slash_trails`, `slash_trail::animate_slash_trails`)

Keep (repoint to rescued module): `weapons::handle_toggle_perf_overlay` (336),
`weapons::handle_toggle_debug_mode` (341), `weapons::update_debug_overlay` (351),
`weapons::update_perf_drop_monitor` (360), `weapons::emit_client_perf_summary` (369).

### 2e. `client/src/camera.rs`
- delete `update_sniper_fisheye` (lines 321-345) entirely
- `update_camera_fov` (line 274): delete param `local_player: Query<&EquippedWeapon, With<LocalPlayer>>`
  (line 279) and the `WeaponType::Sniper` branch (lines 294-303); collapse to
  `FOV_SPRINT` / `FOV_DEFAULT` only. `FOV_ADS` / `FOV_SNIPER_ADS` / `SNIPER_FISHEYE_*` consts
  become dead.
- `is_sprinting_on_foot` (line 187) reads `input_state.aiming` at line 192 — see 2f.

### 2f. `client/src/input.rs`
`InputState` fields to remove: `blocking_held` (line 44), `aiming` (59), `is_dead` (62)
(+ their `Default` init at 90, 96, 97). Consequences:
- `handle_mouse_input` (167): delete the `local_weapon: Query<&EquippedWeapon, ...>` param (169)
  and the whole can_block/ADS block (180-197); keep the raw mouse-look delta accumulation, and
  drop the `if input_state.aiming { 0.5x sensitivity }` branch (206).
- `update_death_state` (268) — **delete the whole system** (it is the only consumer of `Health`
  in the client input path) and its registration at `app_wiring/systems.rs:221`.
- `handle_send_input_to_server`: line 347 `block: input_state.blocking_held` → remove field
  (see §3, PlayerInput), line 351 `|| input_state.is_dead` → remove.
- Every other `is_dead` reader must be updated: `client/src/chest.rs:108`,
  `client/src/pickup/prompts.rs:16,123`, `client/src/crosshair/death_screen.rs:60` (deleted).
  `chest.rs` and `pickup/` are P2 — if P1 lands first, just drop the `|| input_state.is_dead`
  clause there.
- `client/src/camera.rs:192`, `client/src/weapon_view/view_model.rs:232,305`,
  `client/src/render/systems/player/animation.rs` all read `aiming`; the weapon_view ones die
  with the module.

### 2g. `client/src/render/systems/connection.rs`
- lines 99-102: remove `MessageSender::<ShootRequest>`, `<SwitchWeapon>`, `<ReloadRequest>`,
  `<MeleeAttackRequest>`
- lines 134-137: remove `MessageReceiver::<HitConfirm>`, `<BulletImpact>`, `<DamageReceived>`,
  `<PlayerKilled>`
- (`NpcRagdollStarted`/`NpcRagdollPoseBatch` stay until P4)

### 2h. `client/src/render/systems/rendering/setup.rs`
- line 122: remove `camera.insert(crate::render::sniper_fisheye::SniperFisheye::default());`

### 2i. `client/src/audio/` — gunshot/melee spatial audio
- `client/src/audio/mod.rs:31` — remove `use shared::protocol::{AudioEvent, AudioEventKind};`
- `client/src/audio/mod.rs:18` + `:66-69` — remove `handle_remote_audio_events` export and its
  `add_systems` registration; line 104 `.after(handle_remote_audio_events)` on
  `apply_audio_limits` must be dropped too
- `client/src/audio/remote_players.rs:11-135` — delete `handle_remote_audio_events` (the whole
  fn). The rest of the file (`ensure_remote_footstep_emitters`, `update_remote_footstep_emitters`)
  is KEEP.
- `client/src/audio/state.rs` — `GameAudio` fields `assault_shot`, `revolver_shot`,
  `shotgun_shot`, `sniper_shot` (lines 10-13) become unused; `RemoteSpatialSound` (49) becomes
  unused; `AudioPriority::CombatRemote` (57) and `AudioManager::max_remote_combat` (85, 96)
  become unused.
- `client/src/audio/assets.rs:9-12,28-36,56-93` — drop the four gunshot handles from
  `setup_audio` and from the `ensure_audio_assets_loaded` readiness match (which currently
  requires all four gunshot .oggs to load before ANY audio is considered ready — see DANGER D4).
- `client/src/audio/paths.rs:4-7` — delete `SFX_ASSAULT_SHOT`, `SFX_REVOLVER_SHOT`,
  `SFX_SHOTGUN_SHOT`, `SFX_SNIPER_SHOT`.

### 2j. `client/src/pickup/`
- `client/src/pickup/mod.rs:36` — `use crate::weapon_view::WeaponModelAssets;` breaks.
- `client/src/pickup/visuals.rs:8,22-31` — the `ItemType::Weapon(weapon_type)` branch that looks
  up `WeaponModelAssets::scenes` breaks.
  P2 deletes `pickup/` entirely; if P1 lands first, stub the weapon branch to `return`.

### 2k. `client/src/ui/inventory/`
- `client/src/ui/inventory/mod.rs:30` — `use shared::weapons::WeaponType;`
- `client/src/ui/inventory/layout.rs:7,22,26,30,34` — `ItemType::Weapon(WeaponType::*)` icon rows
  P2 deletes this; if P1 lands first, remove the weapon icon entries.

### 2l. `client/src/props/debug.rs`
- lines 5, 147 — repoint `WeaponDebugMode` to the rescued `DebugGizmoMode` (R2). **This is on
  the KEEP list; it must keep working.**

### 2m. `client/src/render/systems/npc/` (P4 territory, must still compile after P1)
- `npc/mod.rs:34-35,39` — `humanoid_body_part`, `humanoid_body_shape`, `HUMANOID_RAGDOLL_BODIES`
  (fine, `shared::npc`) and `WeaponDebugMode` (repoint per R2)
- `npc/debug.rs:50-54` — the `HitZone::{Head,Chest,Stomach,Arms,Legs}` → colour match dies with
  `shared::weapons::damage`. Replace with a `RagdollBodyId`-based palette or drop the colouring.
- `npc/spawn.rs:136,192` — `use shared::weapons::damage::HitZone;` and
  `humanoid_body_part(def.id).hit_zone()` in the CombatDummy visual builder. Same fix.
- `client/src/render/systems/player/animation.rs:187,210-229` — reads `Health::is_dead()` for the
  death animation. Survives only if Health survives (§4).

### 2n. Server module wiring
| File | Change |
| --- | --- |
| `server/src/main.rs:9-10` | remove `#[path = "combat/mod.rs"] mod combat;` |
| `server/src/app/resources.rs:11` | remove `use crate::combat;` |
| `server/src/app/resources.rs:45-46` | remove `init_resource::<combat::target_index::HittableSpatialIndex>()` and `::<combat::hit_world::BulletWorldHitCache>()` |
| `server/src/app/schedule.rs:15` | remove `use crate::combat;` |
| `server/src/app/schedule.rs:57,75` | remove `FpsServerSet::Combat` variant and its entry in the `configure_sets` chain |
| `server/src/app/schedule.rs:241-261` | delete the whole Combat `add_systems` block (12 systems). `inventory::death_drop::handle_inventory_drop_on_death` (256) is P2 — re-home it into `FpsServerSet::Inventory` or delete with P2. |
| `server/src/app/schedule.rs:275-278` | remove `telemetry::perf::handle_perf_weapons_phase_begin` (which is `.before(combat::reload::update_reload_timers)`) and `..._end` |
| `server/src/app/schedule.rs:279-282` | `update_server_perf_log` and `sample_replication_change_pressure` are `.after(inventory::death_drop::handle_inventory_drop_on_death)` — repoint to a surviving anchor |

### 2o. `server/src/telemetry/perf.rs`
- line 4 — `use shared::components::{Bullet, Npc, Player};` → drop `Bullet`
- line 17-43 — remove `Phase::Weapons`, `Phase::BulletHits`, `Phase::WorldHits` from the enum,
  `Phase::COUNT` (29) `8` → `5`, and the `idx()` arms
- lines 161-179 — delete `record_bullet_hits_ms`, `record_world_hits_ms`
- lines 231-241 — delete `handle_perf_weapons_phase_begin`, `handle_perf_weapons_phase_end`
- line 247 — delete `bullets: Query<(), With<Bullet>>` param
- lines 276-287 — delete `weapons_avg_ms/max`, `bullet_hits_avg_ms/max`, `world_hits_avg_ms/max`
- lines 330-361 — trim the `ServerPerf ...` format string + args (`weapons=`, `bullet_hits=`,
  `world_hits=`, `bullets={}`)
- Callers of the removed recorders: `server/src/combat/hit_characters.rs` and `hit_world.rs` (both deleted).
  `record_collision_ms`, `record_ai_cadence_ms`, `record_pathfinding_ms` stay.

### 2p. `server/src/player/spawn.rs`
- line 11 — drop `EquippedWeapon` from the `shared::components` import
- line 24 — drop `use shared::weapons::WeaponType;`
- lines 101-217 — the giant spawn tuple carries `EquippedWeapon` + `weapon_ammo: u32`; remove
  both from the tuple type (115-116), all 4 construction arms (132-133, 153-157, 185-189,
  212-216), and the `equipped_weapon_component` block (~line 228)
- `Health` handling here depends on §4

### 2q. `server/src/player/lifecycle.rs`
Pure death/respawn. `handle_player_deaths` (32), `update_respawn_timers` (57),
`is_player_alive` (89), `RespawnTimer` (15) all key off `Health::is_dead()`. Nothing here fires
without combat damage. **Decide with §4:** either keep it inert (players never die) or delete it
in P5 with the player-character controller. Callers of `is_player_alive`/`RespawnTimer` that
survive P1: `server/src/net/connection.rs:31,150`, `server/src/persistence/autosave.rs:14,37`,
`server/src/physics/dynamic_actors.rs:32,227,395`, `server/src/player/movement.rs:15,33`,
`server/src/player/spatial.rs:111`.

### 2r. `server/src/inventory/hotbar.rs` (P2 file, breaks at P1)
- line 6 `use shared::components::{EquippedWeapon, Player};`, line 11 `use shared::weapons::WeaponType;`
- `sync_equipped_weapon_from_hotbar` (71-152) is entirely weapon logic (off-hand shield,
  ammo save/restore, `WeaponType::Unarmed`) — delete the function, its registration at
  `server/src/app/schedule.rs:230`, and `PreviousHotbarSlot` (17) if nothing else uses it.
- `server/src/inventory/chest.rs:32` uses `WeaponType` in a test.

### 2s. `server/src/net/connection.rs`
- lines 18-23 — drop `BulletImpact`, `DamageReceived`, `HitConfirm`, `PlayerKilled`,
  `ReloadRequest`, `ShootRequest`, `SwitchWeapon` from the protocol import
- lines 85-87 — remove `MessageReceiver::<ShootRequest>`, `<SwitchWeapon>`, `<ReloadRequest>`
- lines 117-120 — remove `MessageSender::<HitConfirm>`, `<DamageReceived>`, `<PlayerKilled>`, `<BulletImpact>`
- line 9 — drop `EquippedWeapon` (and `Health`, per §4) from the `shared::components` import
- lines 144-145 — remove `&Health`, `&EquippedWeapon` from the disconnect-save query and the
  destructuring at ~184-193; drop `equipped_weapon` / `weapon_ammo_in_mag` from the profile write

### 2t. `server/src/persistence/autosave.rs`
- line 5 — drop `EquippedWeapon` (and `Health`) from the import
- lines 31-32 — remove `&Health`, `&EquippedWeapon` from the query and destructuring
- profile write (~103-108, 116) — remove `health_current`, `health_max`, `equipped_weapon`,
  `weapon_ammo_in_mag`; `is_dead` / `death_timestamp` currently derive from
  `respawn_timer.is_some() || health.is_dead()`

### 2u. `server/src/ai/` (P4 territory, must still compile after P1)
- `server/src/ai/tick/mod.rs:7,52-80` — `handle_npc_damage_events` consumes `NpcDamageEvent`.
  Only producers are `server/src/combat/hit_characters.rs:728` and `melee.rs:468` (both deleted),
  so the system becomes dead but still compiles **only if `NpcDamageEvent` survives**. It lives
  in `shared/src/components/combat.rs:183` and its fields are `HitZone`/`HitBodyPart` typed →
  it must be deleted with the weapons module, so `handle_npc_damage_events` must be deleted too
  (and its registration at `server/src/app/schedule.rs:144`).
- `server/src/ai/ragdoll.rs:60,388-413,438,470,657` — `NpcDeathImpact` is produced by
  `combat/hit_characters.rs:736` and `combat/melee.rs:482` (deleted) and by
  `debug_auto_kill_npcs` (`ragdoll.rs:388`, which survives). Ragdolls keep working via the debug
  auto-kill path; no code change needed beyond letting the combat producers go.
- `server/src/ai/death_cleanup.rs:4,20`, `server/src/ai/spawn.rs:10,112,188,225,263,356`,
  `server/src/ai/ragdoll.rs:8,391,433`, `server/src/ai/tick/mod.rs:52,103`,
  `server/src/collision/resolve_npc.rs:6,29` all read `Health` → §4.

### 2v. `client/src/dialogue.rs` (P4)
- line 11 imports `Health`; lines 124, 148, 323, 330 gate dialogue on `!health.is_dead()` → §4.

---

## 3. PROTOCOL: registrations to remove

All in `shared/src/protocol/`.

### `shared/src/protocol/plugin.rs`
Replicated components:
- line 61 `app.register_component::<Health>()` — **conditional, see §4**
- line 62 `app.register_component::<EquippedWeapon>()`
- line 65 `app.register_component::<Bullet>()`
- line 66 `app.register_component::<BulletVelocity>()`
- line 35 `app.register_component::<PlayerMeleeState>()`
- imports at lines 5-9: drop `Bullet`, `BulletVelocity`, `EquippedWeapon`, `PlayerMeleeState`
  (and `Health` per §4)

Client → Server messages:
- line 111 `ShootRequest`
- line 113 `SwitchWeapon`
- line 115 `ReloadRequest`
- line 117 `MeleeAttackRequest`

Server → Client messages:
- line 165 `HitConfirm`
- line 167 `BulletImpact`
- line 169 `DamageReceived`
- line 171 `PlayerKilled`
- line 173 `AudioEvent`

Channels: `ReliableChannel` (183) and `InputChannel` (190) are shared infrastructure — **KEEP**.
`RagdollPoseChannel` (197) is P4.

### `shared/src/protocol/messages.rs`
Delete these type definitions:
- `ShootRequest` (206), `HitConfirm` (217), `DamageReceived` (234), `PlayerKilled` (245),
  `SwitchWeapon` (256), `ReloadRequest` (263), `MeleeAttackRequest` (267),
  `BulletImpactSurface` (321), `BulletImpact` (330), `AudioEventKind` (342), `AudioEvent` (356)
- line 8 `use crate::weapons::damage::{HitBodyPart, HitZone};`
- test `hit_confirmation_preserves_precise_body_part` (line 716)

`PlayerInput` packed-flag surgery (this is also called out for P6, do it here since `block` is
combat-only):
- struct field `block: bool` (line 32) and its `Default` (line 103)
- `FLAG_BLOCK: u16 = 1 << 11` (line 55)
- `Serialize` impl lines 141-143
- `Deserialize` impl line 195
- tests `player_input_roundtrip_on_foot_preserves_flags_and_has_small_yaw_error` (591) and
  `player_input_roundtrip_vehicle_preserves_air_control_and_quantized_controls` (627) both set
  `block: true` — update them.
Producer: `client/src/input.rs:347`. Consumer: `server/src/combat/melee.rs::stamp_block_state`
(deleted).

**Note:** `AudioEvent` is registered and constructed on the server
(`combat/fire.rs:140`, `combat/hit_characters.rs:879`, `combat/melee.rs:248,385,453`) but
`MessageSender::<AudioEvent>` is **never inserted** on `ClientOf` entities in
`server/src/net/connection.rs` — the `audio_senders` queries at `fire.rs:27`,
`hit_characters.rs:161`, `melee.rs:163` match nothing today. Remote gunshot audio is already
dead code. Removing it is a no-op behaviourally.

---

## 4. THE HEALTH DECISION (do not decide by accident)

`Health` is defined in `shared/src/components/combat.rs:8-43`. It is a **clean, standalone
struct**: `{ current: f32, max: f32 }` with `new/take_damage/heal/is_dead/percentage`.
It has **zero dependency on any weapon type** — the file's `use crate::weapons::WeaponType;`
(line 4) is consumed only by `EquippedWeapon` and `Bullet`, never by `Health`.

Verdict: **Health is cleanly separable and should be kept for the tactics units.**

Recommended action:
1. Create `shared/src/components/health.rs` containing ONLY `Health` (43 lines, verbatim).
2. Update `shared/src/components/mod.rs:4,8` (`mod combat;` / `pub use combat::*;`) →
   `mod health;` / `pub use health::*;`.
3. Delete `shared/src/components/combat.rs` (the remaining `EquippedWeapon` (47),
   `Bullet` (152), `BulletVelocity` (167), `BulletPrevPosition` (171), `LocalTracer` (175),
   `NpcDamageEvent` (183) are all weapon-coupled and go).
4. `shared/src/prelude.rs:7` — drop `EquippedWeapon`, keep `Health`.
5. `shared/src/protocol/plugin.rs:61` — **keep** `register_component::<Health>()` if you want it
   replicated for units; it costs one component in the protocol and preserves the client's
   ability to show unit HP later.

Consumers of `Health` that survive P1 and want it (all verified):
`server/src/ai/{spawn.rs, tick/mod.rs, ragdoll.rs, death_cleanup.rs}`,
`server/src/collision/resolve_npc.rs:29`, `server/src/player/{lifecycle.rs, movement.rs, spatial.rs}`,
`server/src/physics/dynamic_actors.rs:227,388,705`, `server/src/net/connection.rs:144`,
`server/src/persistence/autosave.rs:31`, `client/src/dialogue.rs`,
`client/src/render/systems/{npc/animation.rs:105, npc/debug.rs:30, player/animation.rs:187}`.

**Consumers that must be deleted regardless** (P1): `client/src/input.rs:269`
(`update_death_state`), `client/src/crosshair/death_screen.rs:56`,
`server/src/combat/*`, `server/src/inventory/death_drop.rs:12` (P2).

If you instead delete `Health` in P1, every path above breaks at once and P4/P5 get much harder.
Don't.

Related: `shared/src/npc.rs:74 npc_max_health(archetype)` is Health-adjacent but weapon-free —
keep. `shared/src/npc.rs:148` imports `HitBodyPart` for `humanoid_body_part()` (line 221) — that
function IS weapon-coupled and is used by `server/src/combat/hit_characters.rs:102` (deleted),
`client/src/render/systems/npc/{debug.rs:48, spawn.rs:192}` (P4, must be fixed — see 2m).
`humanoid_body_shape` / `HUMANOID_RAGDOLL_BODIES` / `humanoid_body_bounding_radius` are
weapon-free and used by `server/src/ai/ragdoll.rs` — keep those.

---

## 5. PERSISTED DATA (DANGER)

### D1. `shared/src/player_profile.rs` — bincode, on-disk, ~48 live profiles
`/Users/terminator2/Coding/citysim/server_data/players/*.bin` (49 files as of writing).
Fields to remove:
- `equipped_weapon: WeaponType` (line 39)
- `weapon_ammo_in_mag: u32` (line 41)
- (Health: `health_current` / `health_max`, lines 35/37 — keep per §4)
- `is_dead` (65) / `death_timestamp` (67) — keep or drop with §4
- `use crate::weapons::WeaponType;` (line 10)
- `PlayerProfile::new_player` (98-195): slots 0/1/2 (`ItemType::Weapon(AssaultRifle/Sword/Shield)`,
  lines 104-121), slots 3-7 ammo (124-150), `equipped_weapon` / `weapon_ammo_in_mag` (164-165)

**Migration risk:** `server/src/persistence/profiles.rs:48` calls `bincode::deserialize` BEFORE
the `profile.version != PROFILE_VERSION` check at line 51. bincode has no field names — removing
a field silently shifts every subsequent field. In practice deserialize will error out and
`server/src/player/spawn.rs:95-98` catches it and creates a fresh profile, so **existing saves
are silently reset, not corrupted** — but names, levels, prestige, bank_gold and positions are
all lost. Mitigations, pick one:
- Accept the reset and bump `PROFILE_VERSION` (`shared/src/player_profile.rs:14`) to 2 anyway
  so the intent is recorded.
- Or write a one-shot migration tool that reads v1 with the old struct and writes v2 without the
  weapon fields (recoverable via `git show citysim-final:shared/src/player_profile.rs`).
- Or (cheapest) `rm -rf server_data/players/` deliberately, since these are dev-test profiles
  with names like `dfds.bin`, `asdasd.bin`, `test164406.bin`.

### D2. `shared/src/items/types.rs` — `ItemType::Weapon(WeaponType)`
`ItemType` (line 8) has a `Weapon(WeaponType)` variant (line 19), serialized into
`ItemStack` → `PlayerProfile::inventory_slots` and `ChestStorage`. Deleting `WeaponType`
forces deleting that variant, which changes the `ItemType` bincode discriminant layout →
same reset consequence as D1. Also `ItemType::{RifleAmmo, ShotgunShells, SniperRounds}`
(lines 10-13) are combat ammo with no non-combat use.
`ItemType` is P2's problem; P1 only needs the `Weapon(_)` variant gone, and P2 kills the rest.

### D3. Map `.ron` schema — **clean**
`rg 'WeaponType|ItemType|Weapon\(' shared/src/map/ editor/src/` returns nothing. The editor
crate has **zero** references to `weapons`, `Health`, `EquippedWeapon`, `combat`, or `Bullet`.
Map persistence is unaffected by P1.

### D4. Audio readiness gate
`client/src/audio/assets.rs:62-97` gates `AudioState::assets_ready` on a 5-tuple that includes
all four gunshot .oggs. If you remove the handles but not the match, `assets_ready` never flips
and **all** ambient/footstep/vehicle audio goes silent. Change the match to only require
`AMBIENT_WALKING_DESERT`.

---

## 6. ASSETS (P1-only)

Safe to delete (verified: no remaining `.rs` or `.ron` reference after P1):
- `client/assets/game_assets/weapons/automatic_rifle.glb` (474 KB)
- `client/assets/game_assets/weapons/revolver.glb` (452 KB)
- `client/assets/game_assets/weapons/shotgun.glb` (696 KB)
- `client/assets/game_assets/weapons/sniper.glb` (723 KB)
  (referenced only by `client/src/weapon_view/assets.rs:23,27,31,35`)
- `client/assets/audio/sfx/assault_shot.ogg` (25 KB), `revolver_shot.ogg` (21 KB),
  `shutgun_shot.ogg` (17 KB), `sniper_shot.ogg` (38 KB)
- `client/assets/audio/sfx/assualt_rifle_reload.ogg` (36 KB), `gun_reload.ogg` (15 KB),
  `revolver_reload.ogg` (33 KB), `shotgun_reload.ogg` (10 KB), `sniper_reload.ogg` (58 KB),
  `out_of_ammo.ogg` (11 KB)
  (referenced only by `client/src/weapons/paths.rs` and `client/src/audio/paths.rs:4-7`)
- `client/assets/VFX/kenney_smoke-particles/` (whole dir) — referenced ONLY by
  `client/src/weapons/assets.rs:67,128,152` (blood mist, muzzle smoke, muzzle flash). Nothing
  else in the repo loads it. **Consider keeping it**: a top-down tactics game will want smoke
  puffs for explosions/dust, and re-adding a Kenney pack is annoying.

Already dead (unreferenced anywhere, safe to drop opportunistically):
- `client/assets/game_assets/weapons/Wep_Axe_01.glb` (249 KB)
- `client/assets/game_assets/weapons/Wep_Pickaxe_01.glb` (248 KB)
- `client/assets/game_assets/weapons/Wep_Spade_01.glb` (258 KB)

Do NOT touch `client/assets/characters/` (KEEP list — these become the tactics units).

---

## 7. CARGO DEPENDENCIES

No workspace or per-crate dependency becomes unused from P1 alone. Everything in
`client/Cargo.toml`, `server/Cargo.toml`, `shared/Cargo.toml` is used by KEEP-list code:
- `noise` — used by `client/src/weapons/assets.rs:6` (blood splatter FBM) but ALSO by terrain
  generation (`shared/src/terrain`), so it stays.
- `rand` — `shared/src/weapons/ballistics.rs:90` (spread) but also NPC/props/terrain.
- `bincode` — profiles, colliders, terrain.
- `bevy_rapier3d` — server physics, still needed for terrain/LOS raycasts.
- `image` — blood splat image generation uses `bevy::image`, not the `image` crate; `image` is
  used by the terrain KTX path.

---

## 8. TESTS

Deleted with their files:
- `server/src/combat/geometry.rs:270-303` (`oriented_capsule_hits_horizontal_limb_at_nearest_surface`,
  `oriented_capsule_misses_beyond_radius`) — delete
- `server/src/combat/geometry.rs:531-575` — `ray_obb_intersection_hits_axis_aligned_box`,
  `ray_obb_intersection_misses_when_parallel_outside_slab` delete;
  `segment_terrain_intersection_returns_world_distance` (562) **moves with R3**
- `server/src/combat/hit_characters.rs:904-940` (2 tests) — delete
- `shared/src/weapons/ballistics.rs:109-136` (`test_bullet_drop`, `test_step_physics`) — delete
- `shared/src/weapons/damage.rs:193-231` (4 tests) — delete
- `shared/src/weapons/melee.rs:126-187` (3 tests) — delete

Must be edited (files survive):
- `shared/src/protocol/messages.rs:715-729` `hit_confirmation_preserves_precise_body_part` — delete
- `shared/src/protocol/messages.rs:590-624` and `:626-657` — remove `block: true` from the
  `PlayerInput` literals; the `assert!(bytes.len() <= 8)` at line 623 still holds
- `server/src/inventory/chest.rs:32` — `use shared::weapons::WeaponType;` in a test module (P2)
- `shared/src/items/inventory.rs` — has a `#[cfg(test)]` module; check for `WeaponType` fixtures

There are no `tests/` integration-test directories in the workspace and no combat references
under `tools/`.

---

## 9. SUGGESTED EXECUTION ORDER

1. R1–R4 rescues (move perf overlay, `WeaponDebugMode`, segment raycasts, `Pickable`). Compile.
2. §4 Health split (`components/health.rs`). Compile.
3. Protocol trim (§3) + `PlayerInput.block` removal.
4. Delete `server/src/combat/` + server wiring edits (2n–2u) + telemetry (2o).
5. Delete client `weapons/`, `weapon_view/`, `crosshair/`, `sniper_fisheye` + client wiring
   (2a–2i, 2m).
6. `shared/src/weapons/` deletion + `player_profile` / `items` fallout (2p, D1, D2).
7. Assets (§6), tests (§8).
8. Verify with the `verify` skill (client+server launch, `FISTFORCE_AUTOCONNECT`).
