# P3 — Vehicles & Rail removal manifest

Repo: `/Users/terminator2/Coding/citysim` @ `main` (0862414). Recovery: `git show citysim-final:<path>`.
Every path/symbol below was verified to exist by reading the file.

Scope: `shared/src/vehicle/`, `shared/src/rail.rs`, `shared/src/economy.rs`, `server/src/vehicle/`,
`server/src/rail/`, `server/src/collision/resolve_vehicle.rs`, `client/src/rail/`,
`client/src/render/systems/vehicle/`, `client/src/audio/vehicles.rs`, the `InVehicle` gate everywhere,
the vehicle+rail protocol surface, vehicle fields on the persisted `PlayerProfile`, and the
`FISTFORCE_RAIL` dual-mode wiring.

---

## 0. READ THIS FIRST — the four landmines

### DANGER 1 — the ONLY top-down camera in the repo lives in `client/src/rail/mod.rs`
`RtsRailCamera` (client/src/rail/mod.rs:70-96) + `apply_rts_transform` (:682) + `update_rts_camera` (:202)
+ `ensure_rts_camera_controller` (:187) + `intersect_terrain` (:698) + `update_cursor_terrain_hit` (:293)
are the repo's **only** RTS/top-down camera, terrain-raycast-under-cursor, and cursor-release code.
The pivot target is a top-down tactics game. **Do not `rm -rf client/src/rail/` and move on** — lift
these six items into a new `client/src/camera_rts.rs` (or into the surviving `client/src/camera.rs`)
FIRST, then delete the rest of the module.

Worse: `RtsRailCamera` is load-bearing for the KEEP list. `client/src/streaming.rs:13,17` types the
whole streaming-anchor system on it:
```rust
pub type AnchorCamera<'w, 's> =
    Query<'w, 's, (&'static GlobalTransform, Option<&'static RtsRailCamera>), With<Camera3d>>;
```
and `streaming_anchor()` returns `controller.focus` when present. `AnchorCamera`/`streaming_anchor`
are consumed by **11 call sites** across terrain streaming and prop LOD — all KEEP list:
`client/src/terrain/streaming/spawn.rs:8,101,158`, `client/src/terrain/streaming/far_terrain.rs:81`,
`client/src/terrain/streaming/regenerate.rs:65`, `client/src/terrain/streaming/mod.rs:26`,
`client/src/props/spawn.rs:120,340`, `client/src/props/lod/visibility.rs:110`, `client/src/props/lod/mod.rs:21`.
Deleting `crate::rail` without relocating `RtsRailCamera` breaks terrain streaming, far terrain,
terrain regeneration, prop spawning and prop LOD in one shot.

### DANGER 2 — `SubmitPlayerName` has TWO server handlers; the rail one owns company creation
`server/src/rail/mod.rs:158 handle_company_name_submission` and
`server/src/player/spawn.rs:~55 handle_player_name_submission` both consume
`MessageReceiver<SubmitPlayerName>` and both reply `NameSubmissionResult`. They are wired mutually
exclusively via `FISTFORCE_RAIL` (see `server/src/app/schedule.rs:6` comment, :115 vs :333).
Name entry + `NameSubmissionResult` are KEEP (name-entry UI). **Keep `player::spawn::handle_player_name_submission`;
delete only the rail one.** Do not accidentally delete both, or nobody can ever join.

### DANGER 3 — persisted `PlayerProfile` bincode layout (server_data/players/*.bin)
`shared/src/player_profile.rs:49-60` carries six vehicle fields inside a **bincode** struct with
`PROFILE_VERSION = 1` (:14). bincode is positional and these fields have **no `#[serde(default)]`**.
There are **48 live `.bin` profiles** in `server_data/players/`. Removing the fields silently changes
the byte layout; `profiles.rs:48 bincode::deserialize` will either error out or (worse) mis-decode the
tail of the struct (`is_dead`, `death_timestamp`, `level`, `prestige`, `bank_gold`, `last_login`,
`total_playtime_secs`) into garbage — the version check at `profiles.rs:51` runs **after** deserialize,
so it cannot save you.
**Required:** bump `PROFILE_VERSION` to `2` in the same commit that removes the fields. That makes
`load_profile` fail cleanly and back the file up to `<name>.v1.backup` (profiles.rs:52-62) instead of
corrupting. Optionally just wipe `server_data/players/` — the names are all test junk
(`asdasd.bin`, `qweq.bin`, `twerk.bin`, …).

### DANGER 4 — map schema is CLEAN, do not touch it
Verified: `shared/src/map/{schema.rs,editor_schema.rs,loader.rs,save.rs}` contain **zero** occurrences
of vehicle/rail/train/station. `SpawnMarkerKind` (editor_schema.rs:246) is `Player | NpcGroup | Poi`.
`grep -i` over `client/assets/maps/city_alpha/{map.ron,edits.ron}` returns **0 hits** for
vehicle|rail|train|station. The editor crate has **no** rail or vehicle tools — only two stale comments
in `editor/src/worldgen.rs:486` and `:1401` ("so vehicles can drive it") about road flattening.
**No map migration is needed. Do not edit `shared/src/map/` or the `.ron` files for P3.**

---

## 1. Delete wholesale (verified to exist)

| Path | Lines | Why |
|---|---|---|
| `shared/src/vehicle/` (whole dir: `mod.rs`, `components.rs`, `tuning.rs`, `physics/{mod,bike,car,car_v2,common,interaction}.rs`) | 1518 | Vehicle components + all 3 physics models + tuning |
| `shared/src/rail.rs` | 207 | `Company`, `RailTrackSegment`, `RailStation`, `Train`, `TrainState`, `TrainRoute`, `Industry`, `Town`, bezier helpers |
| `shared/src/economy.rs` | 125 | `CargoKind`, `IndustryKind`, `EconomyInventory`, cost constants — used **only** by rail |
| `server/src/vehicle/` (whole dir: `mod.rs`, `bootstrap.rs`, `interaction.rs`, `simulation.rs`) | 253 | World vehicle spawns, enter/exit, per-tick sim |
| `server/src/rail/mod.rs` (delete dir `server/src/rail/`) | 851 | Whole rail tycoon server |
| `server/src/collision/resolve_vehicle.rs` | 92 | `handle_vehicle_static_collisions` |
| `client/src/rail/mod.rs` (delete dir `client/src/rail/`) | 737 | **AFTER extracting `RtsRailCamera` — see DANGER 1** |
| `client/src/render/systems/vehicle/` (whole dir: `mod.rs`, `angles.rs`, `assets.rs`, `hover.rs`, `spawn.rs`, `steam_car.rs`, `sync.rs`, `visibility.rs`) | 899 | Vehicle visuals, steam-car rig, smoothing, shadow culling |
| `client/src/audio/vehicles.rs` | 364 | Engine idle/cruise loops, local + remote spatial emitters |

**Subtotal deleted outright: 5046 lines.**

Assets (see §6 for sizes) — safe to delete, no other referent:
- `client/assets/game_assets/vehicles/` (hoverbike.glb, Veh_Steam_Car_01.glb, Car.glb)
- `client/assets/game_assets/trains/train.glb`
- `client/assets/game_assets/buildings/train/train_station_lvl_1.glb`
- `client/assets/audio/sfx/hover_idle_loop.ogg`, `hover_idle_loop_old.ogg`, `bike_cruise_loop.ogg`

---

## 2. Module declarations to remove

- [ ] `shared/src/lib.rs:15` — `pub mod rail;`
- [ ] `shared/src/lib.rs:19` — `pub mod vehicle;`
- [ ] `shared/src/lib.rs:5` — `pub mod economy;`
- [ ] `server/src/main.rs:22` — `mod rail;`
- [ ] `server/src/main.rs:26` — `mod vehicle;`
- [ ] `client/src/main.rs:14` — `mod rail;` (after camera extraction)
- [ ] `client/src/render/systems/mod.rs:11` — `mod vehicle;` and :21 `pub use vehicle::*;`
- [ ] `client/src/audio/mod.rs:9` — `pub mod vehicles;` and :22-24 the `pub use vehicles::{...}` block
- [ ] `server/src/collision/mod.rs:18` — `pub mod resolve_vehicle;` (and fix the :6 doc comment "Player/NPC/vehicle")

---

## 3. Protocol surface (`shared/src/protocol/`)

### 3a. `shared/src/protocol/plugin.rs`
Remove imports :16-19 (`crate::rail::{...}`) and :21 (`crate::vehicle::{...}`), then delete these
registrations:

Replicated components (11):
- [ ] :56 `Vehicle`
- [ ] :57 `VehicleState`
- [ ] :58 `VehicleDriver`
- [ ] :94 `Company`
- [ ] :95 `CompanyLedger`
- [ ] :96 `RailTrackSegment`
- [ ] :98 `RailStation`
- [ ] :99 `Train`
- [ ] :100 `TrainState`
- [ ] :101 `TrainRoute`
- [ ] :102 `Industry`
- [ ] :103 `Town`
(delete the whole `// === VEHICLE COMPONENTS ===` block :55-58 and `// === RAIL TYCOON COMPONENTS ===` block :93-103)

Client→Server messages (7):
- [ ] :145 `CreateCompanyRequest`
- [ ] :147 `BuildTrackRequest`
- [ ] :149 `BuildStationRequest`
- [ ] :151 `BuyTrainRequest`
- [ ] :153 `AssignRouteRequest`
- [ ] :155 `SetTrainCargoPolicyRequest`
- [ ] :157 `DemolishRailRequest`

Server→Client messages (1):
- [ ] :163 `RailCommandRejected`

Channels: **none are rail/vehicle-specific.** `ReliableChannel`, `InputChannel`,
`RagdollPoseChannel` all survive P3 (`RagdollPoseChannel` dies in P4). Do not remove channels here.

### 3b. `shared/src/protocol/messages.rs`
- [ ] :5 remove `use crate::economy::CargoKind;`
- [ ] :6 remove `use crate::rail::{RouteStop, StationId, TrackSegmentId, TrainId};`
- [ ] :7 remove `use crate::vehicle::VehicleInput;`
- [ ] :28 remove field `pub vehicle_input: Option<VehicleInput>` from `PlayerInput`
- [ ] :36-42 shrink `PackedPlayerInput` — drop `throttle_q: u8`, `brake_q: u8`, `steer_q: i8`
- [ ] :53-54 drop `FLAG_HAS_VEHICLE_INPUT` (bit 9) and `FLAG_VEHICLE_AIR_CONTROL` (bit 10).
      **Either renumber `FLAG_BLOCK` from bit 11 → bit 9, or leave the gap.** Renumbering is a wire
      break; harmless because client+server ship together, but note it in the commit.
- [ ] :58-75 `quantize_unit_u8` / `dequantize_unit_u8` / `quantize_signed_i8` / `dequantize_signed_i8`
      become dead once vehicle input is gone — delete all four (nothing else calls them; `quantize_yaw_u16`
      at :78 and the quat quantizers at :508 stay).
- [ ] :101 remove `vehicle_input: None` from `Default for PlayerInput`
- [ ] :145-157 remove the `let (throttle_q, brake_q, steer_q) = ...` block in `Serialize`
- [ ] :176, :188-193 remove `has_vehicle_input` and the `vehicle_input:` field in `Deserialize`
- [ ] :421-475 delete message structs `CreateCompanyRequest`, `BuildTrackRequest`,
      `BuildStationRequest`, `BuyTrainRequest`, `AssignRouteRequest`, `SetTrainCargoPolicyRequest`,
      `DemolishRailRequest`, `RailCommandRejected`
- [ ] tests: :602 `vehicle_input: None` in `player_input_roundtrip...`; :619 `assert!(decoded.vehicle_input.is_none())`;
      :626-657 delete the whole `player_input_roundtrip_vehicle_preserves_air_control_and_quantized_controls` test.
      The `assert!(bytes.len() <= 8)` at :623 will now over-provision — tighten it or leave it.

### 3c. `shared/src/prelude.rs`
- [ ] :16-19 delete the whole `pub use crate::vehicle::{can_interact_with_vehicle, vehicle_def, InVehicle, Vehicle, VehicleDriver, VehicleState, VehicleType};`
      Note: **`shared::prelude` has zero consumers** in the workspace (verified) — it is dead but still compiles, so it must still be edited.

---

## 4. Server edits

### `server/src/main.rs`
- [ ] :22 `mod rail;`, :26 `mod vehicle;`

### `server/src/app/resources.rs`
- [ ] :17 `use crate::rail;`
- [ ] :26 `app.init_resource::<rail::RailServerState>();`

### `server/src/app/bootstrap.rs`
- [ ] :18 `use super::schedule::rail_mode_enabled;`
- [ ] :93-108 collapse the `if rail_mode_enabled() { rail::setup_initial_industries } else { ai::spawn::spawn_npcs_once; (vehicle::bootstrap::spawn_world_vehicles, inventory::chest::spawn_world_chests) }`
      into just the else-branch minus `spawn_world_vehicles`. (`spawn_npcs_once` dies in P4, `spawn_world_chests` in P2.)

### `server/src/app/schedule.rs`
- [ ] :4-6 doc comment mentioning `FISTFORCE_RAIL` / mutually-exclusive schedules
- [ ] :21 `use crate::rail;`, :23 `use crate::vehicle;`
- [ ] :28-33 delete `rail_mode_enabled()`
- [ ] :35-42 `configure_fixed_schedule` collapses to always calling `configure_fps_fixed_schedule`
- [ ] :50 remove `FpsServerSet::VehicleSim` from the enum, :68 from the `.chain()` tuple
- [ ] :127-138 delete the entire `FpsServerSet::VehicleSim` `add_systems` block
      (`vehicle::interaction::handle_vehicle_interaction_requests`, `vehicle::simulation::ensure_car_suspension_state`,
      `vehicle::simulation::update_vehicles`, `collision::resolve_vehicle::handle_vehicle_static_collisions`)
- [ ] :296-382 delete `enum RailServerSet` and `fn configure_rail_fixed_schedule` in full

### `server/src/net/connection.rs`
- [ ] :17-24 import list: drop `AssignRouteRequest`, `BuildStationRequest`, `BuildTrackRequest`,
      `BuyTrainRequest`, `CreateCompanyRequest`, `DemolishRailRequest`, `RailCommandRejected`,
      `SetTrainCargoPolicyRequest`
- [ ] :25 drop `use shared::vehicle::{InVehicle, Vehicle, VehicleDriver, VehicleState};`
- [ ] :96-104 delete the whole second `insert((MessageReceiver::<CreateCompanyRequest>… DemolishRailRequest…))` tuple
- [ ] :125 remove `MessageSender::<RailCommandRejected>::default(),`
- [ ] :149 remove `Option<&InVehicle>` from the `players` query in `handle_disconnections`
- [ ] :152 remove `mut vehicles: Query<(&mut VehicleDriver, &VehicleState, &Vehicle)>`
- [ ] :243-274 delete the `let (vehicle_data, in_veh) = …` block
- [ ] :288-293 drop the 5 vehicle fields from the `PlayerProfile` literal
- [ ] :334-338 delete the `for (mut driver, _, _) in vehicles.iter_mut()` driver-clearing loop
- [ ] :239 / :345 `inputs.latest_by_driver_id.remove(...)` — see `net/input.rs` below

### `server/src/net/input.rs`
- [ ] :15 `pub latest_by_driver_id: HashMap<u64, PlayerInput>` on `ClientInputs` — its **only** reader is
      `server/src/vehicle/simulation.rs:32`. Once vehicles are gone it is write-only. Delete the field,
      the writer at :79-80, and both removers (`connection.rs:239`, `connection.rs:345`).

### `server/src/persistence/autosave.rs`
- [ ] :10 `use shared::vehicle::{InVehicle, Vehicle, VehicleState};`
- [ ] :36 `Option<&InVehicle>` in the players query; :39 `vehicles: Query<(&VehicleState, &Vehicle)>`
- [ ] :61 `in_vehicle,` binding in the destructure
- [ ] :69-98 the `let (vehicle_data, in_veh) = …` block
- [ ] :112-117 the 6 vehicle fields in the `PlayerProfile` literal

### `server/src/player/spawn.rs`
- [ ] :23 `use shared::vehicle::{InVehicle, Vehicle, VehicleDriver, VehicleState, VehicleType};`
- [ ] :33 `const VEHICLE_REPLICATION_PRIORITY: f32 = 5.0;`
- [ ] :110 `vehicle_spawn,` in the big destructured tuple; :119 the
      `Option<(VehicleType, [f32;3], [f32;3], [f32;3], [f32;3])>` type slot; and the `None` /
      `Some((veh_type, …))` values in every branch (:136, :156, :190)
- [ ] :161-191 delete the whole `else if profile.in_vehicle { … }` restore branch
- [ ] :260-291 delete the `if let Some((veh_type, …)) = vehicle_spawn { commands.spawn((Vehicle…, VehicleState…, VehicleDriver…)); … insert(InVehicle …) }` block
- [ ] **KEEP** `handle_player_name_submission` (:~55) and its `NameSubmissionResult` sends (:77, :83, :298)

### `server/src/player/lifecycle.rs`
- [ ] :9 `use shared::vehicle::{InVehicle, VehicleDriver};`
- [ ] :~40-51 in `handle_player_deaths`: the driver-clearing loop and `commands.entity(entity).remove::<InVehicle>();` (:51)

### `server/src/player/movement.rs` (currently **unwired dead code** — no schedule references `update_players`)
- [ ] :12 `use shared::vehicle::{InVehicle, VehicleState};`
- [ ] :32 `Option<&InVehicle>`, :36 `vehicles: Query<&VehicleState>`, :51 `in_vehicle` binding
- [ ] :70-73 the "ride the vehicle" position override
- [ ] :94 `input.fly_mode && in_vehicle.is_none()` → `input.fly_mode`

### `server/src/physics/dynamic_actors.rs`
- [ ] :26 `use shared::vehicle::{InVehicle, VehicleState};`
- [ ] :338 `Option<&InVehicle>` + :342 `vehicles: Query<&VehicleState>` + :351 binding + :354-357 the
      "snap player body to vehicle" block in `sync_player_bodies_from_authoritative_state`
- [ ] :396 `Option<&InVehicle>` + :421 binding + :437 `if in_vehicle.is_some() { … }` early-out in `apply_player_controls`

### `server/src/physics/terrain_colliders.rs`  ← **KEEP-list adjacent, be careful**
- [ ] :9 `use shared::vehicle::{Vehicle, VehicleState};`
- [ ] :124 `vehicles: &Query<&VehicleState, With<Vehicle>>` param of `gather_centers`
- [ ] :134-137 the vehicle fallback inside `gather_centers`
- [ ] :160 `vehicles: Query<&VehicleState, With<Vehicle>>` param of `sync_terrain_colliders`; :165 call site
Terrain collider streaming is KEEP. Vehicles are only the **second** fallback anchor
(players → vehicles → NPCs). After P3 the chain is players → NPCs; after P5 the commander/camera
must become the anchor or the server streams no terrain colliders at all. **Flag for P5.**

### `server/src/collision/resolve_player.rs` (currently unwired dead code)
- [ ] :9 `use shared::vehicle::InVehicle;`, :35 `Option<&InVehicle>`, :51 binding,
      :52 `if in_vehicle.is_some() || fly_mode.is_some() { continue }` → `if fly_mode.is_some()`

### `server/src/collision/geometry.rs`
- [ ] :28-115 delete `handle_vehicle_vs_static` (~88 lines; only caller is `resolve_vehicle.rs:54`)
- [ ] :332-362 delete `handle_vehicle_proxy_vs_corpse_spheres` (~31 lines; only caller is `resolve_vehicle.rs:77`)
      Both depend on `CorpseCollisionIndex` from `ai::ragdoll` (P4) — remove them in P3 anyway.
- [ ] **KEEP** `handle_capsule_vs_static` (:116) and `handle_capsule_vs_corpse_spheres` (:271).

### `server/src/combat/melee.rs` (whole file dies in P1 — if P1 already ran, skip)
- [ ] :130 `Option<&shared::vehicle::InVehicle>` in the attackers ParamSet
- [ ] :215 binding, :220 `|| in_vehicle.is_some()`

---

## 5. Client edits

### `client/src/main.rs`
- [ ] :14 `mod rail;`

### `client/src/app_wiring/mod.rs`
- [ ] :34 remove `rail` from the big `use crate::{…}` import list

### `client/src/app_wiring/dev.rs`
- [ ] :5-9 delete `rail_mode_enabled()` (and its doc comment)

### `client/src/app_wiring/plugins.rs`
- [ ] :7 `let rail_mode = super::dev::rail_mode_enabled();`
- [ ] :15-19 the window-title ternary → plain `"FistForce".to_string()` (or the new project name, P6)
- [ ] :124-133 delete the `if !rail_mode { … }` wrapper, keeping the plugin adds inside it
      (`ui::InventoryPlugin`, `ui::WorldMapPlugin`, `pickup::PickupPlugin`, `chest::ChestPlugin`,
      `audio::GameAudioPlugin`, `dialogue::DialoguePlugin` — those die in P2/P4, not P3)

### `client/src/app_wiring/resources.rs`
- [ ] :35 `rail::setup_rail_resources(app);`

### `client/src/app_wiring/systems.rs`
- [ ] :2-6 doc comment about the two mutually exclusive modes
- [ ] :10-17 `setup_systems` collapses to `wire_common_systems(app); wire_fps_systems(app);`
- [ ] :100-147 delete `fn wire_rail_systems` in full (18 `rail::*` system references)
- [ ] :157 `game_systems::setup_vehicle_visual_assets`
- [ ] :207 `game_systems::handle_vehicle_spawned`
- [ ] :219 `input::update_vehicle_state`
- [ ] :224 `game_systems::setup_steam_car_visual_rigs`
- [ ] :226 `game_systems::sync_vehicle_transforms`
- [ ] :227 `game_systems::update_steam_car_visuals`
- [ ] :234 `game_systems::update_vehicle_hover`
- [ ] :235 `game_systems::update_vehicle_shadow_culling`
- [ ] :236 `game_systems::apply_vehicle_shadow_state_to_new_meshes`
- [ ] :239 `game_systems::spawn_sand_particles`
  Note: :104 (`rail::setup_rail_assets, game_systems::setup_particle_assets`) disappears with `wire_rail_systems`;
  `setup_particle_assets` is still wired at :156 for the FPS path — keep that one.

### `client/src/render/systems/connection.rs`
- [ ] :16 `use super::particles::SandParticle;` and :228 `particles: Query<Entity, With<SandParticle>>`
      — **only if** you also drop `SandParticle` (see particles.rs below). `SandParticle` also has
      readers in `client/src/weapons/debug.rs:253,403` (P1 file).
- [ ] :63 `commands.insert_resource(crate::rail::RailLocalPeerId(client_id));`
- [ ] :91 comment mentioning `Vehicle`
- [ ] :122-130 delete the whole `insert((MessageSender::<CreateCompanyRequest> … DemolishRailRequest))` tuple
- [ ] :143 `MessageReceiver::<shared::protocol::RailCommandRejected>::default(),`

### `client/src/streaming.rs` ← **KEEP list, see DANGER 1**
- [ ] :13 `use crate::rail::RtsRailCamera;` → point at the relocated type
- [ ] :17 `AnchorCamera` type alias
- [ ] :19-27 `streaming_anchor` — decide the post-pivot anchor. Simplest safe move: relocate
      `RtsRailCamera` to a neutral module and change nothing else. If you instead delete the type,
      `streaming_anchor` must fall back to `camera_hit.map(|(transform, _)| transform.translation())`
      **and you must verify chunk streaming still loads**, because a top-down camera hovering 280 m up
      anchors streaming at the camera position, not the ground focus point — that shifts every loaded
      chunk set (`client/src/rail/mod.rs:693` puts the camera at `focus + rotation * (0,0,zoom)`).

### `client/src/input.rs`
- [ ] :11 `use shared::vehicle::{VehicleDriver, VehicleInput};`
- [ ] :45 doc comment about air tricks
- [ ] :52-56 `InputState` fields `in_vehicle`, `vehicle_look_yaw`, `vehicle_look_pitch`; :93-95 their defaults
- [ ] :184 `can_block && !input_state.in_vehicle` → `can_block`
- [ ] :190 `&& !input_state.in_vehicle`
- [ ] :194-196 "Disable ADS when entering vehicle" block
- [ ] :212-221 the `if input_state.in_vehicle { … } else { … }` mouse-look split → keep only the else branch
- [ ] :240-264 delete `pub fn update_vehicle_state` in full
- [ ] :345 `vehicle_input: None` in the `PlayerInput` literal
- [ ] :361 `input.vehicle_input = None;` and :362-383 the whole `else if input_state.in_vehicle { … }` branch
  `in_vehicle` is read by 8 other files (see §7) — remove those first or in the same commit.

### `client/src/camera.rs`
- [ ] :6 `use crate::render::systems::VehicleHoverBob;`
- [ ] :11 `use shared::vehicle::{Vehicle, VehicleDriver};`
- [ ] :16-18 `VEHICLE_FP_SEAT_HEIGHT`, `HOVERBIKE_FP_SEAT_HEIGHT`, `VEHICLE_FP_SEAT_FORWARD`
- [ ] :50-51 `vehicles_query` + `hover_bobs` system params; :54 `Without<Vehicle>` filter
- [ ] :75-123 the `vehicle_pose` / `vehicle_bob` / `in_vehicle` snap path in `update_camera`
- [ ] :126-145 `vehicle_pose`/`vehicle_bob` params + seat branch in the first-person helper
- [ ] :181 `!input_state.in_vehicle`
- [ ] :197-214 `vehicle_pose` param + vehicle-orbit branch in `third_person_target`
- [ ] :238-245 delete `fn vehicle_bob_offset`
  `peer_id_to_u64` lives here (`client/src/camera.rs`) and is used by `client/src/input.rs` and
  `client/src/audio/*` — **keep it**.

### `client/src/render/systems/particles.rs`
- [ ] :7 `use shared::vehicle::{vehicle_def, Vehicle, VehicleState};`
- [ ] :72 `MAX_SAND_PARTICLES`
- [ ] :75-163 delete `pub fn spawn_sand_particles` in full (its only trigger is a moving `Vehicle`)
- [ ] **KEEP** `setup_particle_assets` (:34) and `update_sand_particles` (:170) — `ParticleAssets` is
      also wired for the rail path today and `SandParticle` is queried by `connection.rs:228` and
      `weapons/debug.rs:253,403`. With no spawner, `SandParticle` becomes vestigial; the clean move is
      to delete the whole file in P6 once `weapons/debug.rs` is gone.

### `client/src/render/systems/player/mod.rs`
- [ ] :36 `use shared::vehicle::{Vehicle, VehicleDriver, VehicleType};`
- [ ] :40 `use crate::render::systems::VehicleHoverBob;`

### `client/src/render/systems/player/sync.rs`
- [ ] :66-70 `vehicles` query + `hover_bobs` query params; :79 `Without<Vehicle>` filter
- [ ] :81-82 `driver_to_vehicle` / `vehicle_bobs` `Local<HashMap<…>>` params
- [ ] :105-127 the driver→vehicle map build and bob accumulation
- [ ] :132-152 the "attach player visual to vehicle seat" branch incl. the `VehicleType` seat table
      (:141-145: Motorbike-hover 0.90/0.45, Motorbike 0.65/0.20, Car|CarV2 0.92/0.08)

### `client/src/render/systems/player/animation.rs`
- [ ] :83-85 `if input.in_vehicle { return MovementAnim::Driving; }`
- [ ] :192 `vehicles: Query<&VehicleDriver, With<Vehicle>>` param
- [ ] :198-203 `active_drivers` HashSet build
- [ ] :293-301 `is_driving_remote` / `is_driving` and every downstream use
- [ ] the `MovementAnim::Driving` variant itself (and its animation clip lookup) once nothing produces it

### `client/src/pickup/` (whole module dies in P2 — do the vehicle half here anyway)
- [ ] `mod.rs:5` doc line; :14-17 import of `detect_nearby_vehicles`, `show_vehicle_prompt`
- [ ] `mod.rs:29` `VEHICLE_INTERACTION_RANGE` import; :32 `use shared::vehicle::{…}`
- [ ] `mod.rs:45` `app.init_resource::<NearbyVehicle>()`; :68-74 the vehicle-prompt `add_systems` block
- [ ] `mod.rs:100-110` `NearbyVehicle` resource + `VehiclePrompt` marker
- [ ] `prompts.rs:5-62` delete `detect_nearby_vehicles`; :64-110 delete `show_vehicle_prompt`
- [ ] `prompts.rs:122-126` `input_state.in_vehicle ||` guard in `detect_nearby_items`
- [ ] `prompts.rs:245,250-252` `vehicle_prompts` cleanup in `cleanup_pickup_ui`
- [ ] `shared/src/items/constants.rs:14` `pub const VEHICLE_INTERACTION_RANGE: f32 = 3.0;`

### `client/src/audio/`
- [ ] `mod.rs:9` `pub mod vehicles;`
- [ ] `mod.rs:21-24` the `pub use vehicles::{cleanup_vehicle_sounds, ensure_remote_vehicle_audio_emitters, update_remote_vehicle_audio_emitters, update_vehicle_audio, update_vehicle_audio_state};`
- [ ] `mod.rs:33` `use shared::vehicle::{Vehicle, VehicleDriver, VehicleState};`
- [ ] `mod.rs:53` `cleanup_vehicle_sounds` in the `OnExit(Playing)` tuple
- [ ] `mod.rs:81-98` the four `add_systems` for `ensure_remote_vehicle_audio_emitters`,
      `update_remote_vehicle_audio_emitters`, `update_vehicle_audio_state`, `update_vehicle_audio`
- [ ] `mod.rs:105` the `.after(ensure_remote_vehicle_audio_emitters)` ordering constraint on `apply_audio_limits`
- [ ] `state.rs:16-17` `GameAudio.hover_idle`, `GameAudio.bike_cruise`
- [ ] `state.rs:24-38` `VehicleIdleSound`, `VehicleCruiseSound`, `VehicleAudioState`
- [ ] `state.rs:122-133` `RemoteVehicleIdleSound`, `RemoteVehicleCruiseSound`,
      `REMOTE_VEHICLE_MAX_SPAWN_DISTANCE`, `REMOTE_VEHICLE_DESPAWN_DISTANCE`
- [ ] `state.rs:140-143` the four `vehicle_*` maps on `RemoteAudioEmitterIndex`
- [ ] `assets.rs:15-17,24-25,34-35` load + log + struct init of `hover_idle` / `bike_cruise`; :39 `init_resource::<VehicleAudioState>()`
- [ ] `paths.rs:9-10` `SFX_HOVER_IDLE_LOOP`, `SFX_BIKE_CRUISE_LOOP`
- [ ] `ambient.rs:51` `&& !input_state.in_vehicle`; :75 doc comment; :82-83 `remote_idle`/`remote_cruise`
      queries in `cleanup_remote_loop_sounds`; :96-99 the four `emitter_index.vehicle_*.clear()` calls
- [ ] `remote_players.rs:155-156` `vehicles: Query<&VehicleDriver, With<Vehicle>>`; :176-178 the
      "skip footsteps for drivers" loop

### `client/src/chest.rs` (dies in P2)
- [ ] :107-108 `if input_state.in_vehicle || input_state.is_dead`

### `client/src/weapons/` (dies in P1)
- [ ] `mod.rs:56` `use shared::vehicle::Vehicle;`
- [ ] `input.rs:98-100` and :324-326 `in_vehicle` early-outs
- [ ] `debug.rs:252` `Query<(), With<Vehicle>>` in `counts_a`, :345 the `Vehicles: {}` format slot,
      :402 `counts_b: Query<(), With<Vehicle>>`, :439 the `vehicles={}` format slot

### `client/src/terrain/streaming/spawn.rs`
- [ ] :5-6 doc comment "local player, or the RTS camera focus in the rail build"

---

## 6. Assets used only by this slice

| Path | Size |
|---|---|
| `client/assets/game_assets/vehicles/hoverbike.glb` | 1.6 MB |
| `client/assets/game_assets/vehicles/Veh_Steam_Car_01.glb` | 2.7 MB |
| `client/assets/game_assets/vehicles/Car.glb` | 53 KB — **already unreferenced** (dead before P3) |
| `client/assets/game_assets/trains/train.glb` | 1.4 MB |
| `client/assets/game_assets/buildings/train/train_station_lvl_1.glb` | 1.0 MB |
| `client/assets/audio/sfx/hover_idle_loop.ogg` | 30 KB |
| `client/assets/audio/sfx/hover_idle_loop_old.ogg` | 30 KB — already unreferenced |
| `client/assets/audio/sfx/bike_cruise_loop.ogg` | 46 KB |

~6.8 MB total. `client/assets/colliders_manifest.ron` contains **no** vehicle/train entries (verified) —
nothing to re-bake.

---

## 7. Every `InVehicle` / `in_vehicle` gate (exhaustive)

Server, component `shared::vehicle::InVehicle`:
1. `server/src/vehicle/interaction.rs:15,17,30,36` — add/remove on enter/exit (file deleted)
2. `server/src/player/lifecycle.rs:51` — stripped on death
3. `server/src/player/spawn.rs:288` — re-inserted on profile restore
4. `server/src/persistence/autosave.rs:36,61,69` — drives what gets saved
5. `server/src/net/connection.rs:149,243` — drives what gets saved on disconnect
6. `server/src/physics/dynamic_actors.rs:338/354` (body snapped to vehicle) and `:396/437` (controls suppressed)
7. `server/src/player/movement.rs:32,70,94` (unwired dead code)
8. `server/src/collision/resolve_player.rs:35,52` — skips static collision while riding (unwired dead code)
9. `server/src/combat/melee.rs:130,215,220` — cannot melee while riding

Client, `InputState.in_vehicle` (a **local mirror**, set by `input::update_vehicle_state` from
replicated `VehicleDriver`, not from `InVehicle` — the component is never replicated to the client):
1. `client/src/input.rs:184,190,195,212,258,263,362` — block/ADS/mouse-look/movement-suppression
2. `client/src/camera.rs:113,181` — snap vs. lerp, first vs. third person
3. `client/src/pickup/prompts.rs:16,123` — suppress item + vehicle prompts
4. `client/src/chest.rs:108` — suppress chest prompt
5. `client/src/weapons/input.rs:99,325` — no shooting / no reloading while driving
6. `client/src/render/systems/player/animation.rs:83,298` — `MovementAnim::Driving`
7. `client/src/audio/ambient.rs:51` — suppress footstep ambience
8. `client/src/audio/vehicles.rs:257,310` — engine loop lifecycle

`InVehicle` is **NOT** in `shared/src/protocol/plugin.rs` — it is server-only state, so removing it has
no wire consequence beyond `PlayerProfile.in_vehicle`.

---

## 8. Suggested execution order (each step compiles)

1. Extract `RtsRailCamera` + `apply_rts_transform` + `update_rts_camera` + `ensure_rts_camera_controller`
   + `release_cursor_for_rts` + `update_cursor_terrain_hit` + `intersect_terrain` out of
   `client/src/rail/mod.rs` into `client/src/camera_rts.rs`; repoint `client/src/streaming.rs:13`.
   Build. **Verify terrain chunks still stream.**
2. Bump `PROFILE_VERSION` to 2 in `shared/src/player_profile.rs:14` and delete the 6 vehicle fields;
   fix `autosave.rs`, `net/connection.rs`, `player/spawn.rs`. Wipe or let it back up `server_data/players/`.
3. Un-wire: `server/src/app/schedule.rs`, `server/src/app/bootstrap.rs`, `server/src/app/resources.rs`,
   `client/src/app_wiring/{plugins,resources,systems,dev}.rs`. Delete the `FISTFORCE_RAIL` flag entirely.
4. Delete the rail slice: `shared/src/rail.rs`, `shared/src/economy.rs`, `server/src/rail/`,
   the rest of `client/src/rail/`, plus the 8 rail messages + 9 rail components in
   `shared/src/protocol/{plugin,messages}.rs` and their `MessageSender`/`MessageReceiver` registrations
   in `server/src/net/connection.rs` and `client/src/render/systems/connection.rs`.
5. Delete the vehicle slice: `shared/src/vehicle/`, `server/src/vehicle/`,
   `server/src/collision/resolve_vehicle.rs`, the two vehicle fns in `server/src/collision/geometry.rs`,
   `client/src/render/systems/vehicle/`, `client/src/audio/vehicles.rs`.
6. Strip the `InVehicle` / `in_vehicle` gates (§7) and the `vehicle_input` half of `PlayerInput`.
7. `cargo build --workspace` (**including `-p editor`**, which must be untouched), `cargo test -p shared`.
8. Delete assets (§6). Update `README.md:11` ("Driveable vehicles (motorbike)"), `:43`, `:101`, `:104`,
   `:289` ("E | Enter/exit vehicle"), and `.claude/skills/verify/SKILL.md:24` and `:42`
   (the `FISTFORCE_RAIL=1` note).

---

## 9. Line budget

| Bucket | Lines |
|---|---|
| Deleted files | 5046 |
| Edits (protocol, wiring, `InVehicle` gates, profile, audio, camera, input, particles, collision geometry) | ~950 |
| **Total Rust removed** | **~6000** |
