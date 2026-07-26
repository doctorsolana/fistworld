# KEEP-side boundary audit — citysim FPS → top-down tactics pivot

Inverse analysis: every place on the **KEEP list** that depends on the **KILL list**.
Deletions are compiler-checked; these boundary breakages are the ones that silently
bite (or panic at runtime / corrupt persisted data). Every path + symbol below was
verified against the working tree at commit `0862414`.

Legend for the "Fix" column:
- **DELETE** — remove the call/field outright, nothing keep-side needs it.
- **MOVE** — the type is genuinely keep-side, relocate it out of the kill module.
- **STUB** — keep the shape, replace the body/value with a keep-side default.
- **GENERICIZE** — replace a kill-side concrete type in a signature with a keep-side one.

---

## 0. TL;DR — the five things that will actually hurt

| # | Boundary | Why it hurts |
|---|---|---|
| 1 | `client/src/streaming.rs:13` → `crate::rail::RtsRailCamera` | **All** client terrain + prop streaming funnels through `streaming_anchor()`. Deleting `client/src/rail` (P3) breaks terrain chunk spawn, far terrain, terrain regenerate, prop spawn, and prop LOD in one shot — 10 keep-side systems. |
| 2 | `SpawnMarkerKind::NpcGroup` (`shared/src/map/editor_schema.rs:248`) | The **live authored map** `client/assets/maps/city_alpha/edits.ron` contains `kind: NpcGroup`. Removing the variant makes RON deserialization fail, and `shared/src/terrain/generator/map_access.rs:21` **panics** on map-load failure → client, server AND editor all die on boot. |
| 3 | `shared/src/building/defs.rs:4` → `crate::items::ItemType` | `shared/src/building` is KEEP and is also the only shared dep of `tools/collider_baker`. P2 (items) breaks the collider baker, which is how prop/building colliders get produced. |
| 4 | `shared/src/player_profile.rs` (bincode, `PROFILE_VERSION = 1`) | 48 live profiles in `server_data/players/*.bin`. P1/P2/P3 all rip fields out of `PlayerProfile`. `PROFILE_VERSION` **must** be bumped in the same commit or bincode silently mis-decodes. |
| 5 | `shared/src/protocol/{plugin,messages}.rs` | The protocol *plugin structure* is KEEP but its body is ~70% kill-side registrations. Every phase touches it, and client/server must be edited in lockstep or the netcode component/message IDs desync. |

---

## 1. `shared/` KEEP modules → kill-side deps

The shared crate is remarkably clean. Only **two** genuine keep→kill edges exist in the
domain modules; the rest are in prelude / protocol / persistence (sections 2–4).

### 1.1 `shared/src/building` (KEEP) → `shared/src/items` (P2)

| Site | Kill symbol | Fix |
|---|---|---|
| `shared/src/building/defs.rs:4` | `use crate::items::ItemType;` | **DELETE** the import. |
| `shared/src/building/defs.rs:224` | `pub cost: &'static [(ItemType, u32)]` field on `BuildingDef` | **DELETE** the field. Verified: nothing outside `defs.rs` reads `BuildingDef::cost` (repo-wide grep for `.cost` only hits rail money, A* pathfinding, and comments). |
| `shared/src/building/defs.rs:118,128,138,148,158` | `cost: &[(ItemType::Wood, 20), (ItemType::Stone, 10)]` etc. — 5 literal initializers | **DELETE** the 5 `cost:` lines along with the field. |

Downstream consumers that must keep compiling after this edit (they only touch
`BuildingType` / `ALL_BUILDING_TYPES`, never `cost`):
- `tools/collider_baker/src/main.rs:20`
- `tools/collider_baker/src/bin/collider_baker_v2/pipeline.rs:12`

### 1.2 `shared/src/map` (KEEP) → NPC types (P4)

| Site | Kill symbol | Fix |
|---|---|---|
| `shared/src/map/schema.rs:5` | `use crate::components::NpcArchetype;` | **DELETE** with the struct below. |
| `shared/src/map/schema.rs:232` | `MapNpcGroup { pub archetype: NpcArchetype, ... }` | **DELETE** `MapNpcGroup` + `MapBehaviorPreset` (schema.rs ~ lines 227–290). |
| `shared/src/map/schema.rs` (`MapDefinition`) | `pub npc_groups: Vec<MapNpcGroup>` (field, `#[serde(default)]`) | **DELETE** the field. Safe for on-disk RON: `MapDefinition` has no `#[serde(deny_unknown_fields)]`, so the `npc_groups: []` line present in `client/assets/maps/city_alpha/map.ron` (last line before `blockers`) is silently ignored by serde. |
| `shared/src/map/schema.rs` `MapDefinition::validate()` | the `for (index, npc_group) in self.npc_groups.iter()` block (~lines 66–90) | **DELETE** the whole loop. |
| `shared/src/map/schema.rs:504` (test `npc_group_trims_authored_metadata`) | `NpcArchetype::Oilman`, `MapNpcGroup`, `MapBehaviorPreset::IdleWanderZone` | **DELETE** the test. |

`shared/src/map/editor_schema.rs` is otherwise **clean** — it only pulls from
`crate::city` and `crate::terrain`, both KEEP. **Do NOT touch `SpawnMarkerKind`** — see DANGER §7.1.

### 1.3 Shared modules verified CLEAN (no kill-side imports at all)

`shared/src/terrain/**`, `shared/src/city/**`, `shared/src/props/**`,
`shared/src/spatial.rs`, `shared/src/colliders.rs`, `shared/src/water.rs`,
`shared/src/physics/**`, `shared/src/components/world.rs`,
`shared/src/map/{loader,save,editor_schema}.rs`.

`shared/src/structures.rs` only imports `crate::props::PropKind` (KEEP) and is consumed by
`server/src/collision/building_geometry/{mod,shapes}.rs` (KEEP).

### 1.4 Shared modules that become dead weight

- `shared/src/economy.rs` — used **only** by `shared/src/rail.rs`,
  `shared/src/protocol/messages.rs`, `client/src/rail/mod.rs`, `server/src/rail/mod.rs`,
  `server/src/app/schedule.rs`. It is a P3 casualty; delete it with rail (it is not on the
  KEEP list despite living next to keep modules).

---

## 2. `shared/src/prelude.rs` — a landmine, but currently harmless

`shared/src/prelude.rs` re-exports **12 kill-side symbols**:

```
:10  pub use crate::items::{ChestStorage, HotbarSelection, Inventory, ItemStack, ItemType};
:16  pub use crate::vehicle::{can_interact_with_vehicle, vehicle_def, InVehicle, Vehicle,
                              VehicleDriver, VehicleState, VehicleType};
:20  pub use crate::weapons::{WeaponStats, WeaponType};
:6   pub use crate::components::{EquippedWeapon, ..., Npc, NpcPosition, ...};
```

**Verified: ZERO files in the workspace import `shared::prelude` or `crate::prelude`.**
It is dead code. Fix: **DELETE the `EquippedWeapon`/`Npc*`/items/vehicle/weapons lines**
in whichever phase kills each module (or delete `shared/src/prelude.rs` + its
`pub mod prelude;` in `shared/src/lib.rs:12` outright in P6). It will not compile
otherwise, and it is the easiest thing in the repo to forget.

---

## 3. `shared/src/protocol/**` — KEEP plugin structure, kill-side payload

The KEEP list says "lightyear net plumbing (connection, channels, protocol PLUGIN
structure)". Both files under it are heavily kill-coupled.

### 3.1 `shared/src/protocol/plugin.rs` — registration edits per phase

| Phase | Lines / symbols to remove from `ProtocolPlugin::build` |
|---|---|
| **P1** | imports `Bullet, BulletVelocity, EquippedWeapon, Health` (`:4-10`); `register_component::<Health>`, `::<EquippedWeapon>`, `::<Bullet>`, `::<BulletVelocity>`; messages `ShootRequest, SwitchWeapon, ReloadRequest, MeleeAttackRequest, HitConfirm, BulletImpact, DamageReceived, PlayerKilled`; `PlayerJumpState`/`PlayerMeleeState` registration (P5 or P1 depending on where melee anim state goes). |
| **P2** | `use crate::items::{...}` (`:11-15`); `register_component::<Inventory>`, `::<GroundItem>`, `::<GroundItemPosition>`, `::<HotbarSelection>`, `::<ChestStorage>`, `::<ChestPosition>`; messages `PickupRequest, DropRequest, SelectHotbarSlot, InventoryMoveRequest, OpenChestRequest, CloseChestRequest, ChestTransferRequest`. |
| **P3** | `use crate::vehicle::{Vehicle, VehicleDriver, VehicleState}` (`:21`); `use crate::rail::{Company, CompanyLedger, Industry, RailStation, RailTrackSegment, Town, Train, TrainRoute, TrainState}` (`:16-19`); the whole "VEHICLE COMPONENTS" and "RAIL TYCOON COMPONENTS" blocks; messages `CreateCompanyRequest, BuildTrackRequest, BuildStationRequest, BuyTrainRequest, AssignRouteRequest, SetTrainCargoPolicyRequest, DemolishRailRequest, RailCommandRejected`. |
| **P4** | `Npc, NpcActivity, NpcFleeing, NpcIdentity, NpcPosition, NpcRotation, NpcVelocity` component registrations; messages `SpawnOilmanDebug, NpcRagdollStarted, NpcRagdollPoseBatch`; channel `RagdollPoseChannel` (`add_channel::<RagdollPoseChannel>`). |
| **P5** | `PlayerCharacter`, `PlayerJumpState`, `PlayerMeleeState`, `PlayerWaterState` component registrations; message `SetPlayerCharacter`. Keep `Player`, `PlayerPosition` (commander view position), `PlayerProgression`. |

**MUST SURVIVE** (keep-side registrations): `Player`, `PlayerPosition`, `PlayerRotation`,
`PlayerVelocity`, `PlayerProgression`, `WorldTime`, `CloudSeed`, `ActiveMapState`,
`TerrainDeltaChunk`, `SubmitPlayerName`, `NameSubmissionResult`, `RequestPlayerRoster`,
`PlayerRoster`, `SetTimeOfDay`, `AudioEvent`, `PlayerInput`, `SpawnPlayer`,
channels `ReliableChannel` + `InputChannel`.

### 3.2 `shared/src/protocol/messages.rs`

| Site | Kill symbol | Fix |
|---|---|---|
| `:5` | `use crate::economy::CargoKind;` | DELETE (P3) |
| `:6` | `use crate::rail::{RouteStop, StationId, TrackSegmentId, TrainId};` | DELETE (P3) |
| `:7` | `use crate::vehicle::VehicleInput;` | DELETE (P3) |
| `:8` | `use crate::weapons::damage::{HitBodyPart, HitZone};` | DELETE (P1) |
| `:4` | `use crate::components::{NpcArchetype, PlayerCharacter};` | DELETE `NpcArchetype` (P4), `PlayerCharacter` (P5) |
| `:26` | `PlayerInput::vehicle_input: Option<VehicleInput>` | DELETE field (P3) |
| `:31` | `PlayerInput::block: bool` | DELETE field (P1) |
| `:37-52` | `PackedPlayerInput { throttle_q, brake_q, steer_q }` + `FLAG_HAS_VEHICLE_INPUT (1<<9)`, `FLAG_VEHICLE_AIR_CONTROL (1<<10)`, `FLAG_BLOCK (1<<11)` | **P6 job**: shrink `PackedPlayerInput` to `{ flags: u16, yaw_q: u16 }`. Note the manual `Serialize`/`Deserialize` impls at `:104` and `:161` must be edited together or the wire format silently skews. |
| `:249` | `PlayerKilled::weapon: crate::weapons::WeaponType` | delete message (P1) |
| `:258` | `SwitchWeapon::weapon_type` | delete message (P1) |
| `:332` | `BulletImpact::weapon_type` | delete message (P1) |
| `:345` | `AudioEventKind::Gunshot { weapon_type }` | **STUB**: `AudioEvent` is KEEP (audio framework). Drop the `Gunshot`/`MeleeSwing`/`MeleeImpact` variants and replace with tactics-side kinds, but keep the `AudioEvent` struct + its ServerToClient registration. |
| tests `:600-760` | `player_input_roundtrip_vehicle_*`, `npc_ragdoll_messages_roundtrip`, `hit_confirmation_preserves_precise_body_part` | DELETE with their phases. |

`shared/src/protocol/config.rs` is clean.

---

## 4. `shared/src/player_profile.rs` + `server/src/persistence` — PERSISTED DATA

`PlayerProfile` is a KEEP concept (commander identity + persistence) with a kill-side body.
Serialized with **bincode** (positional, no field names) at `PROFILE_VERSION = 1`.
**48 live profiles exist in `server_data/players/*.bin`.**

| Site | Kill symbol | Phase | Fix |
|---|---|---|---|
| `shared/src/player_profile.rs:7` | `use crate::items::{ItemStack, ItemType, INVENTORY_SLOTS};` | P2 | DELETE |
| `shared/src/player_profile.rs:9` | `use crate::vehicle::VehicleType;` | P3 | DELETE |
| `shared/src/player_profile.rs:10` | `use crate::weapons::WeaponType;` | P1 | DELETE |
| fields `health_current`, `health_max`, `equipped_weapon`, `weapon_ammo_in_mag` | P1 | DELETE (health may return later as a unit stat, not a commander stat) |
| fields `inventory_slots: [Option<ItemStack>; INVENTORY_SLOTS]`, `hotbar_selection` | P2 | DELETE |
| fields `in_vehicle`, `vehicle_type`, `vehicle_position`, `vehicle_rotation`, `vehicle_velocity`, `vehicle_angular_velocity` | P3 | DELETE |
| `PlayerProfile::new_player()` body (~lines 96–190) — the whole starting-loadout construction | P1+P2 | DELETE the inventory/weapon block; keep name/position/progression/metadata defaults |
| `shared/src/player_profile.rs:8` `use crate::player::SPAWN_POSITION;` | P5 | **MOVE** `SPAWN_POSITION` into a keep-side module (it becomes the commander camera home) — `shared/src/player.rs` also holds `PLAYER_HEIGHT/RADIUS/STEP_UP_HEIGHT` which go away with P5. |

Consumers that must be edited in the same commit:
- `server/src/persistence/autosave.rs:5,8,10` — `EquippedWeapon, Health` (P1), `HotbarSelection, Inventory` (P2), `InVehicle, Vehicle, VehicleState` (P3); the query tuple at `:31-39` and the profile construction at `:108-117`.
- `server/src/persistence/profiles.rs:5` `PROFILE_VERSION` version-gate at `:52-63`.
- `server/src/persistence/io_queue.rs:4`
- `server/src/player/roster_cache.rs:4`
- `server/src/player/spawn.rs:17`
- `server/src/net/connection.rs:16`

---

## 5. `client/` KEEP surfaces → kill-side deps

### 5.1 `client/src/streaming.rs` — the #1 breakage (P3 + P5)

```
client/src/streaming.rs:13   use crate::rail::RtsRailCamera;
client/src/streaming.rs:15   pub type AnchorPlayer<'w,'s> = Query<'w,'s, &'static PlayerPosition, With<LocalPlayer>>;
client/src/streaming.rs:16-17 pub type AnchorCamera<'w,'s> =
        Query<'w,'s, (&'static GlobalTransform, Option<&'static RtsRailCamera>), With<Camera3d>>;
client/src/streaming.rs:19   pub fn streaming_anchor(player: &AnchorPlayer, camera: &AnchorCamera) -> Option<Vec3>
```

Every world-streaming system in the client depends on these three items:

- `client/src/terrain/streaming/mod.rs:26`
- `client/src/terrain/streaming/spawn.rs:8,9,23,101,102,108,158,159,168`
- `client/src/terrain/streaming/far_terrain.rs:81,82,91`
- `client/src/terrain/streaming/regenerate.rs:65,83`
- `client/src/props/spawn.rs:9,120,141,340,341,348`
- `client/src/props/lod/mod.rs:21`
- `client/src/props/lod/visibility.rs:110,111,127`

**Fix (do this FIRST, before P3):** **MOVE** the camera-focus concept into `client/src/streaming.rs`
itself (or a new keep-side `client/src/camera/rts.rs`). Define

```rust
#[derive(Component)] pub struct WorldViewFocus { pub focus: Vec3 }
```

and have the RTS/flycam camera insert it. Then `AnchorCamera` queries `Option<&WorldViewFocus>`
instead of `Option<&RtsRailCamera>`. The existing `RtsRailCamera` in
`client/src/rail/mod.rs:70-95` (`yaw/focus/pan_speed/zoom/zoom_min/zoom_max/zoom_speed/tilt/
look_sensitivity`) plus `update_rts_camera` (`:202-278`), `ensure_rts_camera_controller`
(`:187-201`), `apply_rts_transform` (`:684`), `release_cursor_for_rts` (`:279`) and
`update_cursor_terrain_hit` (`:293`) are **exactly the top-down tactics camera you are
building** — salvage them into a keep-side module rather than deleting them with rail.

`AnchorPlayer` (`PlayerPosition` + `LocalPlayer`) survives P5 as the commander position.

### 5.2 `client/src/render/systems/rendering/setup.rs` — P1 breakage

| Site | Kill symbol | Fix |
|---|---|---|
| `client/src/render/systems/rendering/setup.rs:122` | `camera.insert(crate::render::sniper_fisheye::SniperFisheye::default());` | **DELETE** the line. `setup_rendering` is the KEEP camera/render-target bootstrap (Startup system, wired at `client/src/app_wiring/systems.rs:27`). |
| `client/src/app_wiring/plugins.rs:113` | `app.add_plugins(render::sniper_fisheye::SniperFisheyePlugin);` | DELETE |
| `client/src/render/mod.rs:4` | `pub mod sniper_fisheye;` | DELETE |
| `client/src/app_wiring/systems.rs:238` | `camera::update_sniper_fisheye` | DELETE |
| `client/src/camera.rs:322-334` | `pub fn update_sniper_fisheye(...)` querying `SniperFisheye` + `shared::components::EquippedWeapon` + `WeaponType::Sniper` | DELETE the fn |

`client/src/render/{lod,shadow_cull,hierarchy_fix}.rs` and
`client/src/render/systems/rendering/{scaled_target,settings,day_night,clouds,atmosphere}.rs`
are **CLEAN** — no kill-side deps.

### 5.3 `client/src/audio/**` (KEEP framework) → P1 + P3 + P4

| Site | Kill symbol | Phase | Fix |
|---|---|---|---|
| `client/src/audio/mod.rs:30` | `shared::components::Npc` in `use` | P4 | DELETE |
| `client/src/audio/mod.rs:33` | `use shared::vehicle::{Vehicle, VehicleDriver, VehicleState};` | P3 | DELETE |
| `client/src/audio/mod.rs:22-24` | `pub use vehicles::{cleanup_vehicle_sounds, ensure_remote_vehicle_audio_emitters, update_remote_vehicle_audio_emitters, update_vehicle_audio, update_vehicle_audio_state};` and the 6 `add_systems` registrations in `GameAudioPlugin::build` (`:80-102`) + `cleanup_vehicle_sounds` in the `OnExit` tuple (`:52`) | P3 | DELETE |
| `client/src/audio/vehicles.rs` (whole file, 200+ lines) | `Vehicle, VehicleDriver, VehicleState` | P3 | DELETE file + `pub mod vehicles;` at `mod.rs:9` |
| `client/src/audio/state.rs:4` | `use shared::components::NpcArchetype;` | P4 | DELETE |
| `client/src/audio/state.rs` | `VehicleIdleSound`, `VehicleCruiseSound`, `VehicleAudioState`, `RemoteVehicleIdleSound`, `RemoteVehicleCruiseSound` | P3 | DELETE |
| `client/src/audio/state.rs` `GameAudio` | fields `assault_shot, revolver_shot, shotgun_shot, sniper_shot` (P1) and `hover_idle, bike_cruise` (P3) | P1/P3 | DELETE fields; **keep `desert_ambient`** |
| `client/src/audio/assets.rs:9-37` | loads all 7 handles | P1/P3 | trim to `desert_ambient`; `commands.init_resource::<VehicleAudioState>()` at `:39` DELETE |
| `client/src/audio/paths.rs:4-7,9-10` | `SFX_ASSAULT_SHOT`, `SFX_REVOLVER_SHOT`, `SFX_SHOTGUN_SHOT`, `SFX_SNIPER_SHOT`, `SFX_HOVER_IDLE_LOOP`, `SFX_BIKE_CRUISE_LOOP` | P1/P3 | DELETE; keep `AMBIENT_WALKING_DESERT` |
| `client/src/audio/remote_players.rs:41,50-51,107-118` | `shared::weapons::WeaponType` match over 7 variants inside `handle_remote_audio_events` | P1 | DELETE the gunshot branch; keep `handle_remote_audio_events` (it is the generic `AudioEvent` pump) |
| `client/src/audio/remote_players.rs:154-156,176,199` | `Query<..., With<Npc>>` and `Query<&VehicleDriver, With<Vehicle>>` in `ensure_remote_footstep_emitters` | P3/P4 | DELETE the two queries + the driver/npc loops |
| `client/src/audio/ambient.rs:82-83` | `Query<Entity, With<RemoteVehicleIdleSound>>`, `With<RemoteVehicleCruiseSound>` in `cleanup_remote_loop_sounds` | P3 | DELETE the two params |

`client/src/audio/limits.rs` is clean.

### 5.4 `client/src/props` (KEEP) → P1

| Site | Kill symbol | Fix |
|---|---|---|
| `client/src/props/debug.rs:5` | `use shared::weapons::WeaponDebugMode;` | **MOVE**: `WeaponDebugMode` is a bare `#[derive(Resource)] pub struct WeaponDebugMode(pub bool)` at `shared/src/weapons/debug.rs:5` with **no weapon semantics at all**. Rename/move it to a keep-side `client` resource (e.g. `crate::props::debug::WorldDebugMode`) or `shared/src/debug.rs`. |
| `client/src/props/debug.rs:147` | `debug_mode: Res<WeaponDebugMode>` in `debug_draw_prop_colliders` (registered at `client/src/props/plugin.rs:47`) | point at the moved resource |
| `client/src/app_wiring/mod.rs:31` | `use shared::weapons::WeaponDebugMode;` | update import |
| `client/src/app_wiring/resources.rs:7` | `app.init_resource::<WeaponDebugMode>();` | update to moved type |

Other `WeaponDebugMode` users are all kill-side (`client/src/weapons/debug.rs`,
`client/src/weapons/projectiles.rs:250`, `client/src/render/systems/npc/{mod,debug}.rs`).

The rest of `client/src/props/**` (assets, foliage, kinds, lod, simple_mesh, spawn, types,
wind, plugin) is **CLEAN**.

### 5.5 `client/src/render/systems/connection.rs` (KEEP net plumbing)

| Site | Kill symbol | Phase | Fix |
|---|---|---|---|
| `:63` | `commands.insert_resource(crate::rail::RailLocalPeerId(client_id));` | P3 | **MOVE** `RailLocalPeerId` (`client/src/rail/mod.rs:21`, a plain `Resource(pub u64)`) into a keep-side module — the client still needs its own peer id. Rename to `LocalPeerId`. |
| `:13` | `use shared::vehicle::Vehicle;` | P3 | DELETE (+ the `vehicles: Query<Entity, With<Vehicle>>` param and despawn loop in `cleanup_enter_main_menu`) |
| `:21` | `use shared::components::Npc;` | P4 | DELETE (+ the `npcs:` query + loop in `cleanup_enter_main_menu`) |
| `:100-119` | `MessageSender::<shared::items::{PickupRequest, DropRequest, SelectHotbarSlot, InventoryMoveRequest, OpenChestRequest, CloseChestRequest, ChestTransferRequest}>` | P2 | DELETE the 7 senders (and the second `commands.entity(...).insert((...))` chest block entirely) |
| `:95-99` | `MessageSender::<{ShootRequest, SwitchWeapon, ReloadRequest, MeleeAttackRequest}>` | P1 | DELETE |
| `:130-138` | `MessageReceiver::<{HitConfirm, BulletImpact, DamageReceived, PlayerKilled}>` | P1 | DELETE |
| `:134-135` | `MessageReceiver::<{NpcRagdollStarted, NpcRagdollPoseBatch}>` | P4 | DELETE |
| `:121-129` | the 7 rail `MessageSender`s + `MessageReceiver::<RailCommandRejected>` | P3 | DELETE |
| `:16` | `use super::particles::SandParticle;` + the particle despawn loop | P3 | `particles.rs` is vehicle dust — see §5.7 |

**MUST SURVIVE**: `handle_start_connection`, `update_connection_status`, the netcode/UDP
spawn block, `ReplicationReceiver`, `SubmitPlayerName`/`RequestPlayerRoster` senders,
`NameSubmissionResult`/`PlayerRoster` receivers, `apply_cursor_grab`, `cleanup_enter_main_menu`.

### 5.6 `client/src/app_wiring/**` — the wiring hub (touched by EVERY phase)

`client/src/app_wiring/systems.rs` (411 lines) is split into `wire_common_systems()`
(KEEP), `wire_rail_systems()` (P3) and `wire_fps_systems()` (P1–P5). The `FISTFORCE_RAIL`
env flag at `client/src/app_wiring/dev.rs:8` already partitions them.

| File | What to strip |
|---|---|
| `plugins.rs:113` | `render::sniper_fisheye::SniperFisheyePlugin` (P1) |
| `plugins.rs:126-133` | `ui::InventoryPlugin` (P2), `pickup::PickupPlugin` (P2), `chest::ChestPlugin` (P2), `dialogue::DialoguePlugin` (P4). **Ungate `ui::WorldMapPlugin` and `audio::GameAudioPlugin`** — they are KEEP but currently only registered when `!rail_mode`. |
| `plugins.rs:6,10-19` | the whole `rail_mode` branch / window-title switch (P6 naming) |
| `resources.rs:7-27` | 16 `init_resource` calls for `weapons::*` / `weapon_view::*` (P1) |
| `resources.rs:35` | `rail::setup_rail_resources(app);` (P3) |
| `resources.rs:28` | `audio::RemoteAudioEmitterIndex` — **KEEP** |
| `resources.rs:34` | `game_systems::LastCameraMode` — defined at `client/src/render/systems/player/mod.rs:175`, dies with P5. DELETE. |
| `systems.rs:101-149` | `wire_rail_systems` (P3) |
| `systems.rs:151-411` | `wire_fps_systems` — delete piecewise per phase |
| `mod.rs:31` | `use shared::weapons::WeaponDebugMode;` (P1, see §5.4) |
| `mod.rs:33-36` | `use crate::{audio, camera, chest, city, crosshair, dialogue, input, pickup, profiling, props, rail, render, ...}` — trim `chest, crosshair, dialogue, pickup, rail` |
| `client/src/main.rs:5,8,10,13,17,21,22` | `mod chest; mod crosshair; mod dialogue; mod pickup; mod rail; mod weapon_view; mod weapons;` declarations |

`wire_common_systems()` (`systems.rs:22-99`) is **entirely KEEP** — day/night, clouds,
atmosphere, graphics settings, connection flow, hierarchy fixes, autoconnect. Do not touch it.

### 5.7 `client/src/render/systems/particles.rs` — mixed

`SandParticle` / `ParticleAssets` / `setup_particle_assets` / `spawn_sand_particles` /
`update_sand_particles` are **environment dust**, arguably KEEP, but the spawner is driven
by vehicles:
- `:7` `use shared::vehicle::{vehicle_def, Vehicle, VehicleState};`
- `:78` `vehicles: Query<(&Vehicle, &VehicleState, &Transform)>`
- `:91-119` the per-vehicle emission loop

Fix: either delete the file with P3 (it is registered in both `wire_rail_systems` and
`wire_fps_systems` via `game_systems::setup_particle_assets`), or **GENERICIZE**
`spawn_sand_particles` to take a keep-side `MovingGroundActor` marker. Note
`client/src/render/systems/connection.rs:16` imports `SandParticle` for menu cleanup.

### 5.8 `client/src/ui/**`

| Site | Kill symbol | Phase | Fix |
|---|---|---|---|
| `client/src/ui/debug_time_menu/actions.rs:166` | `shared::components::NpcArchetype::Oilman` in a `SpawnOilmanDebug` send | P4 | DELETE the `oilman_spawn_button` branch |
| `client/src/ui/debug_time_menu/actions.rs:176` | `shared::components::NpcArchetype::CombatDummy` | P4 | DELETE the `dummy_spawn_button` branch |
| `client/src/ui/debug_time_menu/mod.rs:21-22` | `PlayerCharacter`, `SetPlayerCharacter`, `SpawnOilmanDebug` | P4/P5 | DELETE with the buttons in `layout.rs` |
| `client/src/ui/mod.rs:4,13` | `pub mod inventory;` / `pub use inventory::InventoryPlugin;` | P2 | DELETE |
| `client/src/ui/inventory/**` (5 files) | whole subtree | P2 | DELETE (kill list) |

`client/src/ui/{world_map,main_menu,pause_menu,name_entry,modal,styles}` are **CLEAN**.
`world_map` only touches `LocalPlayer`, `PlayerPosition`, `PlayerRotation`, `MapBounds`,
`TerrainGenerator` — all survive P5 as commander state.

### 5.9 `client/src/input.rs::InputState` — KEEP-critical, lives in a P5 file

`InputState` is consumed by **KEEP** UI: `ui/modal.rs:6`, `ui/pause_menu/mod.rs:29`,
`ui/world_map/{mod,layout}.rs`, `ui/debug_time_menu/{mod,actions,layout,state_sync}.rs`,
`audio/{mod,ambient}.rs`, and `render/systems/connection.rs::apply_cursor_grab`.
`InputState::ui_blocking()` (`client/src/input.rs:109`) is the gate for cursor grab.

Fix at P5: **keep `InputState` and `ui_blocking()`**; strip only the kill-side fields:
`blocking_held` (P1), `aiming` (P1), `in_vehicle` / `vehicle_look_yaw` / `vehicle_look_pitch`
(P3), `is_dead` (P1/P5), `inventory_open` (P2), `camera_mode: CameraMode` (P5). Keep
`pause_menu_open`, `map_open`, `debug_menu_open` — `ui_blocking()` depends on them.
Also `client/src/input.rs:169` `local_weapon: Query<&shared::components::EquippedWeapon, ...>`
must go at P1.

### 5.10 `client/src/camera.rs`

Mostly P5, but `peer_id_to_u64` (`:35-43`) is used by `client/src/audio/mod.rs:37` (KEEP).
**MOVE** `peer_id_to_u64` to a keep-side helper before deleting the FPS camera.
Kill-side deps in this file: `:9-11` (`LocalPlayer, PlayerWaterState, PLAYER_HEIGHT,
Vehicle, VehicleDriver`), `:5` `crate::input::CameraMode`, `:6`
`crate::render::systems::VehicleHoverBob`, `:279,:325` `EquippedWeapon` queries,
`:298,:333` `WeaponType::Sniper`.

### 5.11 Client dirs verified CLEAN

`client/src/terrain/**` (except the `AnchorPlayer/AnchorCamera` type aliases from §5.1),
`client/src/water/**`, `client/src/city/**`, `client/src/states.rs`,
`client/src/profiling.rs`, `client/src/render/{lod,shadow_cull,hierarchy_fix}.rs`,
`client/src/render/systems/{world,rendering/*}.rs`.

---

## 6. `server/` KEEP-ish surfaces → kill-side deps

Per the brief, server physics is **slimmed** (terrain/static colliders + raycasts), not deleted.

| Site | Kill symbol | Phase | Fix |
|---|---|---|---|
| `server/src/physics/terrain_colliders.rs:7` | `use shared::components::{Npc, NpcPosition, Player, PlayerPosition};` | P4 | drop `Npc, NpcPosition` |
| `server/src/physics/terrain_colliders.rs:9` | `use shared::vehicle::{Vehicle, VehicleState};` | P3 | DELETE |
| `server/src/physics/terrain_colliders.rs:123-125,159-161` | `players`/`vehicles`/`npcs` query params feeding the collider-activation focus set | P3/P4 | **GENERICIZE**: replace the three queries with a single keep-side "physics interest points" iterator (commander positions + later unit positions). This is the fn that decides which terrain collider tiles are live — it must not silently end up with an empty focus set. |
| `server/src/physics/contacts.rs:6,10,21-22` | `PlayerGrounded`, `crate::physics::dynamic_actors::PlayerPhysicsBody` | P5 | dies with `dynamic_actors.rs` |
| `server/src/collision/geometry.rs:7,272,277,333,337` | `use crate::ai::ragdoll::{CorpseBodyPoint, CorpseCollisionIndex};` + 4 fn params | P4 | **DELETE** the corpse params from `resolve_*` helpers. This file is otherwise the KEEP swept-capsule/geometry library. |
| `server/src/collision/streaming.rs:7,66` | `PlayerPosition` query in `ColliderStreamingState` | P5 | survives (commander position) |
| `server/src/telemetry/perf.rs:5` | `use shared::items::GroundItem;` | P2 | DELETE the ground-item counter |
| `server/src/telemetry/perf.rs:10`, `server/src/telemetry/network.rs:12` | `crate::ai::ragdoll::RagdollTelemetry` | P4 | DELETE the telemetry rows |
| `server/src/app/resources.rs:9,11,12,17` | `use crate::{ai, combat, inventory, rail};` + 12 `init_resource` lines (`ai::obstacles::ObstacleGridState`, `ai::ragdoll::{CorpseBudget, RagdollPoseStream, CorpseCollisionIndex, RagdollTelemetry}`, `ai::relevance::NpcRelevanceSettings`, `ai::pathfinding::PathfindingBudgetSettings`, `inventory::chest::OpenChests`, `combat::target_index::HittableSpatialIndex`, `combat::hit_world::BulletWorldHitCache`, `rail::RailServerState`, `physics::dynamic_actors::NpcPhysicsLodSettings`) | P1-P4 | DELETE per phase. **MUST SURVIVE**: `WorldTerrain`, `AuthoredCityLayout`, `SpatialObstacleGrid`, `net::input::{ClientInputs, ClientInputIngressStats}`, `collision::building_index::BuildingSpatialIndex`, `collision::streaming::ColliderStreamingState`, `physics::terrain_colliders::*`, `physics::static_world_colliders::*`, `persistence::*`, `player::{index, spatial, roster_cache}`, `telemetry::*`. |
| `server/src/app/bootstrap.rs:12,15,18,89-107` | `use crate::{ai, rail};` + the `rail_mode_enabled()` branch spawning `ai::spawn::spawn_npcs_once`, `vehicle::bootstrap::spawn_world_vehicles`, `inventory::chest::spawn_world_chests`, `rail::setup_initial_industries` | P2-P4 | DELETE the whole `if rail_mode_enabled() { ... } else { ... }` block. **MUST SURVIVE**: `world::bootstrap::setup_world`, `collision::library::setup_baked_colliders`, `spawn_server`, `start_server`, `net::connection::handle_disconnections`, `world::time::spawn_world_time_once`, `world::map_state::{spawn_cloud_seed_once, spawn_active_map_state_once}`. |
| `server/src/app/schedule.rs:13-24` | `use crate::{ai, combat, inventory, rail, vehicle};` + `FpsServerSet::{VehicleSim, AISim, Inventory, Combat}` + `configure_rail_fixed_schedule` | P1-P4 | DELETE. Keep `WorldTick, PhysicsWorld, NetIngress, PhysicsPost, Indices, Persistence`. |
| `server/src/net/connection.rs:8-25` | `EquippedWeapon, Health` (P1), `shared::items::{7 msg types}` (P2), `shared::vehicle::{InVehicle, Vehicle, VehicleDriver, VehicleState}` (P3), protocol msgs (all phases) | P1-P3 | DELETE per phase. **KEEP** `handle_connections`, `handle_disconnections`, `handle_player_name_submission`, replication-config helpers. |
| `server/src/player/spawn.rs:14,23,24,26` | `shared::items::{HotbarSelection, Inventory}`, `shared::vehicle::{...}`, `shared::weapons::WeaponType`, `crate::inventory::hotbar::PreviousHotbarSlot` | P1-P3 | strip the commander spawn bundle down to `Player + PlayerPosition + PlayerRotation + PlayerProgression + Replicate` |
| `server/src/player/spatial.rs:113` | `Without<shared::components::Npc>` | P4 | DELETE the filter |
| `server/src/player/lifecycle.rs:9`, `server/src/player/movement.rs:12` | `shared::vehicle::{InVehicle, VehicleDriver, VehicleState}` | P3 | DELETE |
| `server/src/main.rs:1-27` | `mod ai; mod combat; mod inventory; mod rail; mod vehicle;` (path-attribute module decls) | P1-P4 | DELETE per phase |

`server/src/world/**`, `server/src/city/**`, `server/src/collision/{library, building_index,
building_geometry/*}`, `server/src/physics/{layers, queries, static_world_colliders}`,
`server/src/net/{input, peer}` are **CLEAN**.

---

## 7. DANGER — things that break the KEEP list or corrupt data

### 7.1 `SpawnMarkerKind::NpcGroup` — DO NOT REMOVE THE VARIANT (P4)

`shared/src/map/editor_schema.rs:243-249`:
```rust
pub enum SpawnMarkerKind { Player, NpcGroup, Poi }
```
`client/assets/maps/city_alpha/edits.ron` (122 MB, the live authored map) contains
**`kind: NpcGroup`** (1 occurrence; also 1 `Player`, 3 `Poi`). Unknown *enum variants* are a
hard deserialization error in serde/RON (unlike unknown *struct fields*, which are ignored).

`shared/src/map/save.rs:31-37` → `load_map_edits_optional` returns `Err` →
`shared/src/map/loader.rs::load_map` returns `Err` →
**`shared/src/terrain/generator/map_access.rs:21-23` calls `panic!("Failed to load authored map ...")`**.

That panic fires in `WorldTerrain` init, which the **client, the server, AND the editor** all do.
Removing this variant bricks all three at boot.

**Safe path:** keep the variant (it becomes "unit deployment zone" for tactics), or
rename it *and* write a one-shot migration over `edits.ron` before removing the old name.

Editor sites that reference `SpawnMarkerKind::NpcGroup` (all must be edited together if renamed):
- `editor/src/session.rs:185` — `selected_spawn_kind: SpawnMarkerKind::NpcGroup` (the **default** UI tool state)
- `editor/src/ui.rs:926` — the "NPC Group" `selectable_value` in the PlaceSpawnMarker tool
- `editor/src/tools.rs:1923` — `SpawnMarkerKind::NpcGroup => npc_material.clone()` in `spawn_spawn_visuals` (exhaustive match, will not compile if the variant is dropped)
- `editor/src/worldgen.rs:1361` — `Landmark { name: "Outpost", kind: SpawnMarkerKind::NpcGroup }` in the procedural world generator

### 7.2 `MapDefinition::npc_groups` removal is SAFE, but verify before shipping

`client/assets/maps/city_alpha/map.ron` ends with `npc_groups: [],`. `MapDefinition` has
**no `#[serde(deny_unknown_fields)]`**, so serde ignores the stale field. Same for the backup
map at `client/assets/maps/city_alpha_backup_2026-07-13/map.ron`. Still: **run the editor once
against `city_alpha` after the P4 edit before committing** — the failure mode is a boot panic,
not a compile error.

### 7.3 `PlayerProfile` bincode corruption (P1/P2/P3)

`server_data/players/` holds **48 `.bin` profiles**. bincode is positional: removing
`health_current`, `equipped_weapon`, `inventory_slots`, the 6 vehicle fields, etc. WITHOUT
bumping `PROFILE_VERSION` (`shared/src/player_profile.rs:14`) means old bytes decode into
wrong fields (or a length-prefix blows up into a multi-GB allocation).

The version gate in `server/src/persistence/profiles.rs:52-63` DOES back up + reject on
mismatch — **but only if you bump `PROFILE_VERSION`**. Bump it to `2` in the same commit as
the first field removal.

### 7.4 Protocol lockstep

`shared/src/protocol/plugin.rs` registration ORDER determines lightyear's component/message
network IDs. Client and server share the same `ProtocolPlugin`, so a partial edit is
impossible — but a stale running server against a new client will mis-route messages with
no error. Restart both after every protocol edit; treat P1–P5 protocol trims as a single
non-rolling deploy.

### 7.5 `client/src/streaming.rs` fails OPEN, not closed

`streaming_anchor()` returns `Option<Vec3>` and every caller does `let Some(anchor) = ... else { return; };`.
If the anchor breaks (P3 removes `RtsRailCamera`, P5 removes `LocalPlayer`), terrain and props
simply **stop streaming with zero log output** — you get an empty world, not a crash. Fix
`streaming.rs` FIRST (§5.1) and add a `warn_once!` when the anchor is `None`.

### 7.6 `WeaponDebugMode` is load-bearing for prop debugging

`shared/src/weapons/debug.rs:5` is a 1-line `Resource(pub bool)` with no weapon semantics, but
it gates `client/src/props/debug.rs:147 debug_draw_prop_colliders` (KEEP). Move it, don't delete it.

### 7.7 `AudioEvent` must survive P1

`AudioEvent` / `AudioEventKind` live in `shared/src/protocol/messages.rs` and are the ONLY
server→client audio channel. `AudioEventKind::Gunshot { weapon_type: WeaponType }` drags
`shared::weapons` into the KEEP audio framework. Replace the variant set; do not delete the
message or `handle_remote_audio_events` (`client/src/audio/remote_players.rs`).

### 7.8 `tools/collider_baker` is a hidden KEEP consumer

It is a workspace member (`Cargo.toml` members list) and will fail `cargo build --workspace`
if `shared::building::BuildingDef` stops compiling. It imports `shared::building`,
`shared::colliders`, `shared::props` — all KEEP — so the ONLY thing that can break it is the
`ItemType` edge in §1.1.

---

## 8. Recommended EXECUTION ORDER (per-phase keep-side blast radius)

The stated order P1→P6 is *not* the safest. The rail/streaming edge (§5.1) is the single
largest keep-side breakage and it sits in P3.

**P0 (new, do first — pure refactor, no deletions):**
1. Move `RtsRailCamera`'s focus concept out of `client/src/rail` into `client/src/streaming.rs`
   (or a new keep-side `client/src/camera/rts.rs`), rewire `AnchorCamera`. Salvage
   `update_rts_camera` / `ensure_rts_camera_controller` / `apply_rts_transform` — they are your
   tactics camera.
2. Move `RailLocalPeerId` → `LocalPeerId` out of `client/src/rail`.
3. Move `WeaponDebugMode` out of `shared/src/weapons`.
4. Move `peer_id_to_u64` out of `client/src/camera.rs`.
5. Move `SPAWN_POSITION` out of `shared/src/player.rs`.
6. Bump `PROFILE_VERSION` → `2`.

After P0, all six phases become mechanical.

| Phase | Keep-side breakages it causes |
|---|---|
| **P1** weapons/combat | `shared/src/prelude.rs:6,20`; `shared/src/components/combat.rs` (whole file, incl. `Health` — decide now whether `Health` survives); `shared/src/protocol/{plugin,messages}.rs`; `shared/src/player_profile.rs` (4 fields) + `PROFILE_VERSION`; `client/src/render/systems/rendering/setup.rs:122` (**KEEP camera bootstrap**); `client/src/app_wiring/{plugins.rs:113, resources.rs:7-27, systems.rs:238}`; `client/src/props/debug.rs:5,147` (**KEEP prop debug**); `client/src/audio/{paths,assets,state,remote_players}.rs` (**KEEP audio**); `client/src/render/systems/connection.rs:95-99,130-133`; `server/src/{net/connection.rs, persistence/autosave.rs, player/spawn.rs, app/{resources,schedule}.rs}`. |
| **P2** items/inventory | `shared/src/building/defs.rs:4,118-158,224` (**KEEP building + breaks `tools/collider_baker`**); `shared/src/prelude.rs:10`; `shared/src/protocol/plugin.rs:11-15`; `shared/src/player_profile.rs` (2 fields); `client/src/ui/mod.rs:4,13`; `client/src/render/systems/connection.rs:100-119`; `server/src/telemetry/perf.rs:5`; `server/src/app/bootstrap.rs:104`. |
| **P3** vehicles/rail | ⚠️ **`client/src/streaming.rs:13,17` → all terrain + prop streaming** (mitigated by P0); `client/src/render/systems/connection.rs:13,63`; `client/src/audio/**` (whole vehicles.rs + 6 plugin registrations); `client/src/render/systems/particles.rs:7,78`; `shared/src/protocol/{plugin.rs:16-21, messages.rs:5-7,26}`; `shared/src/player_profile.rs` (6 fields); `shared/src/economy.rs` (delete); `server/src/physics/terrain_colliders.rs:9,124,160` (**KEEP terrain physics**); `server/src/{player/{lifecycle,movement}.rs, persistence/autosave.rs, collision/resolve_player.rs}`. |
| **P4** NPCs/AI | ⚠️ `shared/src/map/schema.rs:5,232` + `MapDefinition::npc_groups` + validate + tests; **`SpawnMarkerKind::NpcGroup` — DO NOT REMOVE (§7.1)**; `editor/src/{session.rs:185, ui.rs:926, tools.rs:564,1923, worldgen.rs:1361}` (**the editor must keep compiling**); `server/src/collision/geometry.rs:7,272,277,333,337` (**KEEP geometry lib**); `server/src/physics/terrain_colliders.rs:7,125,161`; `server/src/telemetry/{perf.rs:10, network.rs:12}`; `server/src/player/spatial.rs:113`; `client/src/audio/{mod.rs:30, state.rs:4, remote_players.rs:154}`; `client/src/ui/debug_time_menu/actions.rs:166,176`; `shared/src/npc.rs` (delete). |
| **P5** FPS→commander | `client/src/input.rs::InputState` (**KEEP UI gate — strip fields, keep the struct + `ui_blocking()`**); `client/src/camera.rs::peer_id_to_u64` (used by KEEP audio); `client/src/app_wiring/resources.rs:34` (`LastCameraMode`); `client/src/streaming.rs:15` (`AnchorPlayer` — survives); `shared/src/player.rs` constants (`PLAYER_HEIGHT/RADIUS/STEP_UP_HEIGHT` used by kill code only; `SPAWN_POSITION` used by KEEP); `server/src/physics/contacts.rs`; `shared/src/protocol/plugin.rs` (PlayerCharacter/Jump/Melee/Water). |
| **P6** cleanup | `PackedPlayerInput` shrink (`shared/src/protocol/messages.rs:37-52` + the two manual serde impls at `:104`/`:161` — edit together); `shared/src/prelude.rs` delete; `FISTFORCE_*` env-var renames across `client/src/{profiling.rs, app_wiring/dev.rs}` and `server/src/app/schedule.rs:29`; `client/src/app_wiring/plugins.rs:10-19` window title. |

---

## 9. Cargo dependencies that become unused

| Crate | Dep | Evidence |
|---|---|---|
| `shared/Cargo.toml` | `rand` | Only use in the entire shared crate is `shared/src/weapons/ballistics.rs:90,91` (`rand::random::<f32>()`). Unused after P1. |
| `server/Cargo.toml` | `bevy_rapier3d` | **KEEP** — still needed for terrain/static colliders + line-of-sight raycasts (`server/src/physics/{queries,terrain_colliders,static_world_colliders}.rs`). Do NOT remove. |
| `shared/Cargo.toml` | `bincode` | **KEEP** — `shared/src/colliders.rs:29` (baked collider DB) + `player_profile`. |
| `shared/Cargo.toml` | `image` | **KEEP** — `shared/src/map/loader.rs:2` heightmap PNG. |
| `client/Cargo.toml` | `noise`, `image`, `ron`, `arboard` | KEEP (terrain/worldgen, minimap, servers.ron, name-entry paste). |

---

## 10. Assets touched only by the KEEP audio framework's kill-side handles

Deletable once §5.3 lands (small — listed for completeness, not urgency):

- `client/assets/audio/sfx/assault_shot.ogg` (28K), `revolver_shot.ogg` (24K),
  `shutgun_shot.ogg` (20K), `sniper_shot.ogg` (40K) — P1
- `client/assets/audio/sfx/hover_idle_loop.ogg` (32K), `hover_idle_loop_old.ogg` (32K),
  `bike_cruise_loop.ogg` (48K) — P3
- Unreferenced-by-code reload SFX already dead:
  `assualt_rifle_reload.ogg` (36K), `gun_reload.ogg` (16K), `out_of_ammo.ogg` (12K),
  `revolver_reload.ogg` (36K), `shotgun_reload.ogg` (12K), `sniper_reload.ogg` (60K)
- **KEEP**: `client/assets/audio/ambient/walking_desert.ogg` (48K), `client/assets/characters/` (12M),
  `client/assets/maps/` (124M), `client/assets/textures/` (54M), `client/assets/game_assets/` (90M),
  `client/assets/sky_10_2k/` (2.4M), `client/assets/shaders/`, `client/assets/ui/`
- `client/src/render/sniper_fisheye.wgsl` — delete with P1 (embedded asset, not under `assets/`)
