# P6 — Protocol, wiring & assets: strip manifest

Repo: `/Users/terminator2/Coding/citysim` (Bevy 0.18.1, lightyear 0.26.4, workspace: `shared/ client/ server/ editor/ tools/*`)
Recovery: `git show citysim-final:<path>` (tag `citysim-final`, commit `281aa7c`).

**Scope of this document:** the cross-cutting layer — the lightyear protocol registry, app/system wiring
in both binaries, Cargo dependencies, orphaned assets, and FistForce/citysim naming. It is written to be
executed **last** (after P1–P5), because almost every entry here is "delete the registration whose type
already died".

Everything below was verified by reading the file at the cited path/line. Line numbers are as of the
working tree at the time of writing (`main`, `0862414`).

---

## 0. DANGER — read before touching anything

| # | Danger | Why | Mitigation |
|---|--------|-----|-----------|
| D1 | **`shared/src/prelude.rs` is dead code that will break the build.** | It re-exports `EquippedWeapon, Health, Npc, Inventory, ItemStack, ItemType, ChestStorage, HotbarSelection, Vehicle, VehicleState, VehicleDriver, VehicleType, InVehicle, WeaponStats, WeaponType, can_interact_with_vehicle, vehicle_def`. `rg 'shared::prelude'` across the workspace returns **zero** consumers — it compiles only because it re-exports live types. | Delete `shared/src/prelude.rs` and its `pub mod prelude;` (`shared/src/lib.rs:12`) **in P1**, not P6. It is pure liability. |
| D2 | **`client/src/streaming.rs` is on the KEEP list but imports P3 rail and P5 player types.** | `use crate::rail::RtsRailCamera;` (line 13) and `PlayerPosition`/`LocalPlayer` (line 11). Its `AnchorPlayer`/`AnchorCamera`/`streaming_anchor` are consumed by **terrain streaming** (`client/src/terrain/streaming/{spawn,far_terrain,regenerate}.rs`) and **prop streaming** (`client/src/props/spawn.rs:120`). Killing rail or the player entity without rewriting this file breaks all terrain/prop streaming — the single most load-bearing KEEP subsystem. | Rewrite `streaming_anchor` to anchor on the `Camera3d` `GlobalTransform` only (the flycam), *before* deleting `client/src/rail/`. |
| D3 | **The FPS/perf overlay lives inside the P1 weapons module.** | `PerfOverlayEnabled, PerfDropMonitor, ClientPerfConfig, ClientPerfSnapshot, DebugOverlay, FpsText, PerfStatsText` are defined in `client/src/weapons/state.rs` (lines 8–17, 246–320) and driven by `client/src/weapons/debug.rs` (`spawn_debug_overlay`, `update_debug_overlay`, `update_client_perf_snapshot`, `emit_client_perf_summary`, `update_perf_drop_monitor`, `handle_toggle_perf_overlay`). The `verify` skill greps `/tmp/client.log` for `ClientPerf frame_ms_p50=...` as its success signal. | **Extract** `debug.rs` (minus `update_trajectory_debug_gizmos`) and the perf types from `state.rs` into a new `client/src/perf_overlay/` **before** deleting `client/src/weapons/`. Do not lose `FISTFORCE_CLIENT_PERF*`. |
| D4 | **Player profiles are bincode — not self-describing. Field removal silently wipes saves.** | `shared/src/player_profile.rs` `PlayerProfile` is `bincode::serialize`d to `server_data/players/<name>.bin` (`server/src/persistence/profiles.rs:40,115`). `#[serde(default)]` on `level/prestige/reputation/stamina/intelligence/bank_gold` has **no effect** under bincode. On any decode failure the server does **not** error — `server/src/player/spawn.rs:88-99` falls through to `PlayerProfile::new_player(...)`, silently resetting level/prestige/reputation/bank_gold. `PlayerRosterCache::from_storage_dir` (`server/src/player/roster_cache.rs:25-48`) silently `continue`s past unparseable profiles, so the roster just goes empty. | Removing `equipped_weapon`, `weapon_ammo_in_mag`, `inventory_slots`, `hotbar_selection`, `in_vehicle`, `vehicle_*` **must** be paired with `PROFILE_VERSION: u32 = 2` (`shared/src/player_profile.rs:14`) so the mismatch branch (`profiles.rs:50-63`) backs the old file up to `<name>.v1.backup`. 48 profile files exist locally in `server_data/players/` (gitignored, so no CI impact). |
| D5 | **`SpawnMarkerKind::NpcGroup` is baked into the shipped map data.** | `shared/src/map/editor_schema.rs:246-250` defines `enum SpawnMarkerKind { Player, NpcGroup, Poi }`. `client/assets/maps/city_alpha/edits.ron` (122 MB) contains a `spawn_markers` list with **1** `NpcGroup` entry and `Player` entries. RON enum decoding fails hard on an unknown variant. | **Do not remove the `NpcGroup` variant in P4.** Keep it (or rename with a serde alias) — `shared/src/map` is on the KEEP list and `editor/src/ui.rs:926` still offers it in the marker dropdown. |
| D6 | **`PROTOCOL_ID` should be bumped when the protocol changes.** | `shared/src/protocol/config.rs:5`: `pub const PROTOCOL_ID: u64 = 0x1234567890ABCDF4;`. It gates netcode handshake, so a stale client that survives will otherwise connect and then desync on unknown message net-ids. | Bump it (e.g. `…DF5`) in the same commit that slims the protocol plugin. |
| D7 | **Do not rename `FISTFORCE_AUTOCONNECT`, `FISTFORCE_CLIENT_PERF`, `FISTFORCE_CLIENT_PERF_INTERVAL_SECS`, `CITYSIM_MAX_NPCS` casually.** | They are hard-coded in `.claude/skills/verify/SKILL.md` (lines 18, 27–28, 33) and in `STRIP_PLAN.md:138`, which is the documented smoke-test recipe for every phase. | Either rename env vars **and** the skill in the same commit, or defer renaming to the very end. See §5. |
| D8 | **Do not delete `client/assets/characters/`** — explicit KEEP. That includes `sarah_animated.glb`, which is currently referenced by **no code**; it is a future unit model, not an orphan. |
| D9 | **`client/assets/colliders.bin` is loaded by the server and copied into the Docker image.** | `server/src/collision/library.rs:83` and `Dockerfile` (`COPY client/assets/colliders.bin …`). It is prop collision data (KEEP). | Never sweep it up with "client-only assets". |

---

## 1. `shared/src/protocol/` — complete registry inventory

### 1a. Registered replicated components (`shared/src/protocol/plugin.rs`)

Every entry is `app.register_component::<T>().add_prediction();`.

| Line | Type | Defined in | Slice | Action |
|------|------|-----------|-------|--------|
| 30 | `Player` | `shared/src/components/actors.rs:7` | **KEEP** | keep (commander identity: `client_id: PeerId`) |
| 31 | `PlayerPosition` | actors.rs:119 | **KEEP** | keep — becomes commander view position |
| 32 | `PlayerRotation` | actors.rs:123 | KEEP (marginal) | keep only if the server needs commander facing; else P5 |
| 33 | `PlayerVelocity` | actors.rs:127 | P5 | delete |
| 34 | `PlayerJumpState` | actors.rs:152 | P5 | delete |
| 35 | `PlayerMeleeState` | actors.rs:159 | P1 | delete |
| 36 | `PlayerWaterState` | actors.rs:143 | P5 | delete |
| 38 | `PlayerProgression` | actors.rs:13 | **KEEP** | keep (persisted + roster) |
| 40 | `PlayerCharacter` | actors.rs:43 | P5 | delete |
| 43 | `Npc` | actors.rs:51 | P4 | delete |
| 44 | `NpcPosition` | actors.rs:88 | P4 | delete |
| 45 | `NpcRotation` | actors.rs:92 | P4 | delete |
| 46 | `NpcVelocity` | actors.rs:96 | P4 | delete |
| 47 | `NpcActivity` | actors.rs:76 | P4 | delete |
| 48 | `NpcFleeing` | actors.rs:100 | P4 | delete |
| 49 | `DebugPhysicsBox` | actors.rs:104 | P5 | delete |
| 50 | `DebugPhysicsBoxPosition` | actors.rs:111 | P5 | delete |
| 52 | `DebugPhysicsBoxRotation` | actors.rs:115 | P5 | delete |
| 56 | `Vehicle` | `shared/src/vehicle/components.rs:6` | P3 | delete |
| 57 | `VehicleState` | vehicle/components.rs:21 | P3 | delete |
| 58 | `VehicleDriver` | vehicle/components.rs:61 | P3 | delete |
| 61 | `Health` | `shared/src/components/combat.rs:8` | P1 (see note) | see §1e |
| 62 | `EquippedWeapon` | combat.rs:47 | P1 | delete |
| 65 | `Bullet` | combat.rs:152 | P1 | delete |
| 66 | `BulletVelocity` | combat.rs:167 | P1 | delete |
| 69 | `WorldTime` | `shared/src/components/world.rs:8` | **KEEP** | keep (day/night, ocean clock) |
| 70 | `CloudSeed` | world.rs:21 | **KEEP** | keep |
| 71 | `ActiveMapState` | world.rs:27 | **KEEP** | keep |
| 74 | `Inventory` | `shared/src/items/inventory.rs:10` | P2 | delete |
| 75 | `GroundItem` | `shared/src/items/world.rs:10` | P2 | delete |
| 76 | `GroundItemPosition` | items/world.rs:56 | P2 | delete |
| 80 | `NpcIdentity` | actors.rs:80 | P4 | delete |
| 83 | `HotbarSelection` | `shared/src/items/messages.rs:29` | P2 | delete |
| 86 | `ChestStorage` | items/world.rs:60 | P2 | delete |
| 87 | `ChestPosition` | items/world.rs:130 | P2 | delete |
| 90 | `TerrainDeltaChunk` | `shared/src/terrain/serialization.rs:72` | **KEEP** | keep (client ingests at `client/src/terrain/streaming/ingest.rs:55`). NB: **no server system ever spawns it today** — it is a client-side-only path right now. |
| 94–103 | `Company`, `CompanyLedger`, `RailTrackSegment`, `RailStation`, `Train`, `TrainState`, `TrainRoute`, `Industry`, `Town` (all `shared/src/rail.rs`) | P3 | delete all 9 |

**Result:** 8 of 40 registered components survive (`Player`, `PlayerPosition`, `PlayerRotation`, `PlayerProgression`, `WorldTime`, `CloudSeed`, `ActiveMapState`, `TerrainDeltaChunk`).

### 1b. Registered messages (`shared/src/protocol/plugin.rs:105-180`)

**Client → Server** (`.add_direction(NetworkDirection::ClientToServer)`):

| Line | Message | Sender (client) | Handler (server) | Slice |
|------|---------|-----------------|------------------|-------|
| 107 | `SpawnPlayer` (messages.rs:202) | **none** | **none** | **DEAD ALREADY** — `rg '\bSpawnPlayer\b'` outside `protocol/` returns nothing. Delete now. |
| 109 | `PlayerInput` (messages.rs:12) | `client/src/input.rs:388,402,411` | `server/src/net/input.rs:52` | **KEEP (slimmed)** — see §1d |
| 111 | `ShootRequest` (messages.rs:206) | `client/src/weapons/input.rs:175` | `server/src/combat/fire.rs:24` | P1 |
| 113 | `SwitchWeapon` (messages.rs:256) | never sent (only `MessageSender` registered, `client/src/render/systems/connection.rs:100`) | never received | P1 — **already dead plumbing** |
| 115 | `ReloadRequest` (messages.rs:263) | `client/src/weapons/input.rs:350` | `server/src/combat/reload.rs:70` | P1 |
| 117 | `MeleeAttackRequest` (messages.rs:267) | `client/src/weapons/input.rs:130` | `server/src/combat/melee.rs:119` | P1 |
| 119 | `SetTimeOfDay` (messages.rs:293) | `client/src/ui/debug_time_menu/actions.rs:204` | `server/src/world/time.rs:41` | **KEEP** (debug time menu is KEEP) |
| 121 | `SetPlayerCharacter` (messages.rs:299) | `client/src/ui/debug_time_menu/actions.rs:147` | `server/src/player/spawn.rs:307` | P5 |
| 123 | `SpawnOilmanDebug` (messages.rs:305) | `client/src/ui/debug_time_menu/actions.rs:164,174` | `server/src/ai/spawn.rs:286` | P4 |
| 125 | `SpawnPhysicsBoxDebug` (messages.rs:314) | `client/src/ui/debug_time_menu/actions.rs:188` | `server/src/ai/spawn.rs:380` | P5 |
| 127–140 | `PickupRequest`, `DropRequest`, `SelectHotbarSlot`, `InventoryMoveRequest`, `OpenChestRequest`, `CloseChestRequest`, `ChestTransferRequest` (all `shared/src/items/messages.rs`) | `client/src/pickup/`, `client/src/ui/inventory/`, `client/src/chest.rs` | `server/src/inventory/{ground_items,hotbar,chest}.rs` | P2 (7 messages) |
| 141 | `SubmitPlayerName` (messages.rs:367) | `client/src/ui/name_entry/actions.rs:12,39,59` + `client/src/app_wiring/dev.rs:62` | `server/src/player/spawn.rs:58` (and `server/src/rail/mod.rs:165`) | **KEEP** |
| 143 | `RequestPlayerRoster` (messages.rs:404) | registered only (`connection.rs:112`); UI sender TBD | `server/src/player/roster.rs:19` | **KEEP** |
| 145–158 | `CreateCompanyRequest`, `BuildTrackRequest`, `BuildStationRequest`, `BuyTrainRequest`, `AssignRouteRequest`, `SetTrainCargoPolicyRequest`, `DemolishRailRequest` | `client/src/rail/mod.rs:396-418` | `server/src/rail/mod.rs` | P3 (7 messages) |

**Server → Client** (`.add_direction(NetworkDirection::ServerToClient)`):

| Line | Message | Slice | Notes |
|------|---------|-------|-------|
| 161 | `NameSubmissionResult` (messages.rs:374) | **KEEP** | consumed `client/src/ui/name_entry/mod.rs:19` |
| 163 | `RailCommandRejected` (messages.rs:473) | P3 | `client/src/rail/mod.rs:454` |
| 165 | `HitConfirm` (messages.rs:217) | P1 | carries `HitZone`/`HitBodyPart` from `shared::weapons::damage` |
| 167 | `BulletImpact` (messages.rs:330) | P1 | carries `BulletImpactSurface` + `WeaponType` |
| 169 | `DamageReceived` (messages.rs:234) | P1 | |
| 171 | `PlayerKilled` (messages.rs:245) | P1 | carries `WeaponType` |
| 173 | `AudioEvent` (messages.rs:356) | P1 | **every** `AudioEventKind` variant is combat (`Gunshot{weapon_type}`, `MeleeSwing`, `MeleeImpact{blocked}`) — the whole message dies with P1. Consumed at `client/src/audio/remote_players.rs:22`, produced at `server/src/combat/{fire,melee,hit_characters}.rs` |
| 175 | `PlayerRoster` (messages.rs:408) | **KEEP** | + `PlayerRosterEntry` (messages.rs:414) |
| 177 | `NpcRagdollStarted` (messages.rs:551) | P4 | |
| 179 | `NpcRagdollPoseBatch` (messages.rs:572) | P4 | |

**Result:** 5 of 32 messages survive: `PlayerInput`, `SetTimeOfDay`, `SubmitPlayerName`, `RequestPlayerRoster`, `NameSubmissionResult`, `PlayerRoster` (6 counting `PlayerRoster`).

### 1c. Channels (`shared/src/protocol/plugin.rs:182-201`)

| Line | Channel | Mode | Direction | Slice |
|------|---------|------|-----------|-------|
| 183 | `ReliableChannel` (messages.rs:578) | `OrderedReliable` | Bidirectional | **KEEP** — carries `SubmitPlayerName`/`NameSubmissionResult`/`SetTimeOfDay`/roster |
| 190 | `InputChannel` (messages.rs:581) | `UnorderedUnreliable` | ClientToServer | **KEEP** — `PlayerInput` only (`client/src/input.rs:388`) |
| 197 | `RagdollPoseChannel` (messages.rs:584) | `UnorderedUnreliable` | ServerToClient | **P4 — delete.** Only producer: `server/src/ai/ragdoll.rs:887` |

### 1d. `PlayerInput` packed flag layout (`shared/src/protocol/messages.rs:11-198`)

Wire format is `PackedPlayerInput { flags: u16, yaw_q: u16, throttle_q: u8, brake_q: u8, steer_q: i8 }`
(messages.rs:35-42) — the on-foot roundtrip test asserts `bytes.len() <= 8` (line 623).

| Bit | Const (line) | Field | Consumed by | Slice |
|-----|--------------|-------|-------------|-------|
| 0 | `FLAG_FORWARD` (44) | `forward` | `shared/src/physics/character.rs:99`, `server/src/physics/dynamic_actors.rs` | P5 |
| 1 | `FLAG_BACKWARD` (45) | `backward` | same | P5 |
| 2 | `FLAG_LEFT` (46) | `left` | same | P5 |
| 3 | `FLAG_RIGHT` (47) | `right` | same | P5 |
| 4 | `FLAG_JUMP` (48) | `jump` | `server/src/player/movement.rs:94` | P5 |
| 5 | `FLAG_FLY_MODE` (49) | `fly_mode` | `shared/src/physics/character.rs:27`, `server/src/physics/dynamic_actors.rs:455`, `server/src/collision/resolve_player.rs:52` | P5 |
| 6 | `FLAG_FLY_DOWN` (50) | `fly_down` | `character.rs:59`, `dynamic_actors.rs:489` | P5 |
| 7 | `FLAG_FLY_FAST` (51) | `fly_fast` | `character.rs:49,99`, `dynamic_actors.rs:480,549` | P5 |
| 8 | `FLAG_INTERACT` (52) | `interact` | **only** `server/src/vehicle/interaction.rs:22` | P3 |
| 9 | `FLAG_HAS_VEHICLE_INPUT` (53) | `vehicle_input: Option<VehicleInput>` | `server/src/vehicle/simulation.rs:30-72` | P3 |
| 10 | `FLAG_VEHICLE_AIR_CONTROL` (54) | `vehicle_input.air_control` | same | P3 |
| 11 | `FLAG_BLOCK` (55) | `block` | **only** `server/src/combat/melee.rs:70` | P1 |
| — | — | `yaw: f32` (quantized u16) | `character.rs`, `dynamic_actors.rs` | P5 (but see below) |
| — | — | `throttle_q/brake_q/steer_q` | vehicle sim | P3 |

**Every field dies.** After P1–P5 the only reason to keep a `PlayerInput` message at all is if the
commander camera position must be server-known (line-of-sight / interest management). Two options:

- **(a) Delete `PlayerInput` + `InputChannel` entirely.** Simplest; the flycam is purely client-side.
  Also deletes `shared/src/physics/character.rs::step_character` (its only caller signature is
  `input: &PlayerInput`, `character.rs:17`) and `server/src/net/input.rs` (`ClientInputs`,
  `ClientInputIngressStats`, `handle_client_input_messages`).
- **(b) Replace with a 12-byte `CommanderView { position: Vec3 }`** (or keep `PlayerInput` with only
  `yaw` + a new camera position) if the server needs the view anchor for relevance. **Recommended** —
  P5 explicitly keeps the player entity as "connection + name + persistence + a camera/view position",
  and `PlayerPosition` is still replicated, so something has to write it.

Either way, delete the whole `Serialize`/`Deserialize` impl (lines 108-198), the 12 flag consts
(44-55), `PackedPlayerInput` (35-42), the vehicle quantizers `quantize_unit_u8`/`dequantize_unit_u8`/
`quantize_signed_i8`/`dequantize_signed_i8` (57-75), and the two roundtrip tests (591-657).
Keep `quantize_yaw_u16`/`dequantize_yaw_u16` (77-87) only if yaw survives.

### 1e. Ancillary types in `messages.rs` that go with the messages

| Lines | Type | Slice |
|-------|------|-------|
| 272–289 | `TimeOfDayPreset` + `normalized_time()` | **KEEP** |
| 320–326 | `BulletImpactSurface` | P1 |
| 341–351 | `AudioEventKind` | P1 |
| 374–400 | `NameSubmissionResult`, `NameRejectionReason` | **KEEP** |
| 408–419 | `PlayerRoster`, `PlayerRosterEntry` | **KEEP** |
| 478–496 | `RagdollBodyId` (16 variants) | P4 |
| 499–539 | `PackedQuatI16`, `pack_quat_i16`, `unpack_quat_i16`, `quantize_quat_component`, `dequantize_quat_component` | P4 (only ragdoll uses them) |
| 543–575 | `RagdollBodyPose`, `NpcRagdollPoseSample`, `NpcRagdollPoseBatch`, `NpcRagdollStarted` | P4 |
| 586–730 | `mod tests` — **all 4 tests break**: `player_input_roundtrip_on_foot…` (591), `player_input_roundtrip_vehicle…` (627), `packed_quat_roundtrip…` (659), `npc_ragdoll_messages_roundtrip` (668), `hit_confirmation_preserves_precise_body_part` (715) | delete/rewrite; `cargo test -p shared` is in the verification protocol |

Top-of-file imports that break (`messages.rs:4-8`): `NpcArchetype` & `PlayerCharacter` (P4/P5),
`CargoKind` (P3 `shared/src/economy.rs`), `RouteStop/StationId/TrackSegmentId/TrainId` (P3
`shared/src/rail.rs`), `VehicleInput` (P3), `HitBodyPart/HitZone` (P1 `shared/src/weapons/damage`).

`plugin.rs` import block (lines 4-21) collapses to `use crate::components::{ActiveMapState, CloudSeed,
Player, PlayerPosition, PlayerProgression, PlayerRotation, WorldTime}; use crate::terrain::TerrainDeltaChunk;`.

**Open question carried from `STRIP_PLAN.md`:** `Health` (combat.rs:8-43) has no weapon dependency in
its own definition and is a natural fit for tactics units. If you keep it, move it out of
`shared/src/components/combat.rs` (which also holds `EquippedWeapon`, `Bullet`, `BulletVelocity`,
`BulletPrevPosition`, `LocalTracer`, `NpcDamageEvent` — all P1) into `actors.rs` or a new
`shared/src/components/health.rs`, so P1 can delete `combat.rs` wholesale.

### 1f. `shared/src/protocol/config.rs` — untouched except one bump

All of `SERVER_PORT`, `SERVER_ADDR`, `NETCODE_*`, `PRIVATE_KEY`, `FIXED_TIMESTEP_HZ`, `tick_duration()`,
`get_server_bind_addr()` are **KEEP**. Only `PROTOCOL_ID` (line 5) should be bumped — see D6.

---

## 2. App wiring

### 2a. `server/src/main.rs` (32 lines) — module declarations

Delete `mod ai;` (1-2), `mod combat;` (9-10), `mod inventory;` (11-12), `mod rail;` (21-22),
`mod vehicle;` (25-26). Keep `app, city, collision, net, persistence, physics, player, telemetry, world`.
`collision` is slimmed rather than deleted (P5 keeps terrain/static colliders); `physics` likewise.

### 2b. `server/src/app/mod.rs` (58 lines)

- Line 9-10, 43: `RapierPhysicsPlugin::<NoUserData>::default().in_fixed_schedule()` and
  `configure_rapier_gravity` — **KEEP** (P5 keeps rapier for terrain/static colliders + LOS raycasts).
- Line 35: `StaticTransformOptimizations` init — keep while rapier is present (comment explains why).
- Line 42: `ProtocolPlugin` — KEEP.
- Line 48: `crate::city::log_city_layout_summary` — KEEP.
- No slice-specific plugin registrations here. Only the doc comment at 1 needs a name change.

### 2c. `server/src/app/resources.rs` (58 lines) — `setup_resources`

| Line | Resource | Slice |
|------|----------|-------|
| 23 | `WorldTerrain` | KEEP |
| 24 | `AuthoredCityLayout` | KEEP |
| 25 | `net::input::ClientInputs` | P6 (dies with `PlayerInput` option (a); keep for (b)) |
| 26 | `rail::RailServerState` | P3 |
| 27 | `net::input::ClientInputIngressStats` | P6 (same as 25) |
| 28 | `player::index::PlayerEntityIndex` | KEEP |
| 29 | `player::spatial::PlayerSpatialIndex` | KEEP |
| 30 | `inventory::chest::OpenChests` | P2 |
| 31 | `SpatialObstacleGrid` | KEEP (`shared/src/spatial.rs`) — but check whether anything but AI populates it |
| 32 | `ai::obstacles::ObstacleGridState` | P4 |
| 33–36 | `ai::ragdoll::{CorpseBudget, RagdollPoseStream, CorpseCollisionIndex, RagdollTelemetry}` | P4 |
| 37 | `ai::relevance::NpcRelevanceSettings` | P4 |
| 38 | `ai::pathfinding::PathfindingBudgetSettings` | P4 |
| 39 | `collision::building_index::BuildingSpatialIndex` | KEEP |
| 40 | `collision::streaming::ColliderStreamingState` | KEEP |
| 41–42 | `physics::terrain_colliders::{TerrainColliderSettings, TerrainColliderRegistry}` | KEEP |
| 43 | `physics::static_world_colliders::StaticWorldColliderRegistry` | KEEP |
| 44 | `physics::dynamic_actors::NpcPhysicsLodSettings` | P4/P5 |
| 45 | `combat::target_index::HittableSpatialIndex` | P1 |
| 46 | `combat::hit_world::BulletWorldHitCache` | P1 |
| 47–55 | `PlayerProfiles`, `PlayerRosterCache`, `ProfileIoQueue` (`server_data/players`) | KEEP |
| 56–57 | `telemetry::{ServerPerfMonitor, ServerNetDebugWindow}` | KEEP (see §2f) |

### 2d. `server/src/app/bootstrap.rs` (109 lines)

- `spawn_server` / `start_server` / `server_is_started` / `handle_disconnections` observer (24-81): **KEEP**.
- Line 18, 93-108: the `rail_mode_enabled()` branch — **delete the whole `if/else`**, keeping only
  `world::bootstrap::setup_world`, `collision::library::setup_baked_colliders`, `spawn_server`,
  `world::time::spawn_world_time_once`, `world::map_state::spawn_cloud_seed_once`,
  `world::map_state::spawn_active_map_state_once`.
  Dies with it: `rail::setup_initial_industries` (P3), `ai::spawn::spawn_npcs_once` (P4),
  `vehicle::bootstrap::spawn_world_vehicles` (P3), `inventory::chest::spawn_world_chests` (P2).

### 2e. `server/src/app/schedule.rs` (382 lines) — the biggest wiring edit

`rail_mode_enabled()` (28-33) and `configure_rail_fixed_schedule` (296-382, including
`enum RailServerSet`) are **deleted wholesale (P3)**. `configure_fixed_schedule` (35-42) collapses to a
direct call.

`enum FpsServerSet` (44-58) — 12 variants; survivors are `WorldTick`, `PhysicsWorld`, `NetIngress`,
`Indices`, `Persistence`. Delete `VehicleSim` (P3), `AISim` (P4), `PhysicsControl`/`PhysicsPost`/
`PlayerSim` (P5, partially), `Inventory` (P2), `Combat` (P1). Rename the enum (`FpsServerSet` → e.g. `SimSet`).

| Set (lines) | Systems | Verdict |
|---|---|---|
| `WorldTick` (80-92) | `world::time::handle_set_time_of_day`, `world::time::update_world_time`, `city::buildings::sync_authored_plot_buildings`, `collision::building_index::sync_building_spatial_index`, `collision::streaming::update_static_collider_streaming` | **KEEP all 5** |
| `PhysicsWorld` (94-109) | `sync_terrain_colliders`, `sync_static_prop_colliders`, `sync_static_building_colliders` **KEEP**; `ensure_player_physics_bodies`, `ensure_npc_physics_bodies`, `cleanup_npc_physics_when_ragdoll_activates`, `sync_player_bodies_from_authoritative_state`, `sync_npcs_from_physics_before_ai` **P4/P5 delete** |
| `NetIngress` (111-125) | `net::connection::handle_connections` **KEEP**; `player::spawn::handle_player_name_submission` **KEEP**; `player::roster::handle_player_roster_requests` **KEEP**; `net::input::handle_client_input_messages` **P6/§1d**; `player::spawn::handle_set_player_character` **P5**; `ai::spawn::handle_spawn_oilman_debug` **P4**; `ai::spawn::handle_spawn_physics_box_debug` **P5** |
| `VehicleSim` (127-138) | all 4 → **P3 delete** (`vehicle::interaction::handle_vehicle_interaction_requests`, `vehicle::simulation::ensure_car_suspension_state`, `vehicle::simulation::update_vehicles`, `collision::resolve_vehicle::handle_vehicle_static_collisions`) |
| `AISim` (140-155) | all 8 → **P4 delete** |
| `PhysicsControl` (157-167) | both → **P4/P5 delete**. NB this block carries `.before(bevy_rapier3d::plugin::PhysicsSet::SyncBackend)` (164) |
| `PhysicsPost` (169-186) | all 9 → **P4/P5 delete**. Carries `.after(bevy_rapier3d::plugin::PhysicsSet::Writeback)` (183) |
| `PlayerSim` (188-198) | `tick_player_jump_timers` **P5**; `player::lifecycle::handle_player_deaths`, `update_respawn_timers` **P1/P5** |
| `Indices` (200-210) | `player::index::sync_player_entity_index`, `player::spatial::sync_player_spatial_index` **KEEP**; `ai::relevance::update_npc_network_visibility` **P4** |
| `Persistence` (212-221) | both **KEEP** |
| `Inventory` (223-239) | all 9 → **P2 delete** |
| `Combat` (241-261) | all 13 → **P1/P2 delete** |

**Telemetry ordering anchors (263-293) — these break silently, not loudly.** Nine
`telemetry::perf::*` / `telemetry::network::*` systems are ordered relative to systems that die:

- `handle_perf_tick_begin` / `handle_perf_core_phase_begin` `.before(world::time::handle_set_time_of_day)` — **survives**
- `handle_perf_core_phase_end` `.after(physics::dynamic_actors::sync_debug_boxes_from_physics)` — **P5, breaks**
- `handle_perf_npc_inventory_build_phase_begin` `.before(ai::obstacles::sync_obstacle_grid)` — **P4, breaks**
- `handle_perf_npc_inventory_build_phase_end` `.after(inventory::chest::update_distant_chest_auto_close)` — **P2, breaks**
- `handle_perf_weapons_phase_begin` `.before(combat::reload::update_reload_timers)` — **P1, breaks**
- `handle_perf_weapons_phase_end` / `update_server_perf_log` / `sample_replication_change_pressure`
  `.after(inventory::death_drop::handle_inventory_drop_on_death)` — **P1/P2, breaks**
- `PostUpdate`: `sample_link_flow_post_send.after(ConnectionSystems::Send).before(LinkSystems::Send)` — **KEEP as-is**

Re-anchor these to the surviving set boundaries (`.in_set(SimSet::WorldTick)` etc.) rather than to
individual systems.

### 2f. `server/src/telemetry/` — KEEP module with dying references

- `perf.rs:4-5,10`: `use shared::components::{Bullet, Npc, Player}; use shared::items::GroundItem; use crate::ai::ragdoll::RagdollTelemetry;`
- `perf.rs:16-41`: `enum Phase` — drop `NpcInventoryBuild`(1), `Weapons`(3), `AiCadence`(4),
  `Pathfinding`(5), `BulletHits`(6), `WorldHits`(7); keep `Core`(0), `Collision`(2). `Phase::COUNT`
  (line 27) must be updated and the `phase_sum`/`phase_max` arrays resized.
- `perf.rs:246-249`: `npcs: Query<(), With<Npc>>`, `bullets: Query<(), With<Bullet>>`,
  `ground_items: Query<(), With<GroundItem>>`, `ragdoll_telemetry: Res<RagdollTelemetry>` — all die.
- `perf.rs:272-285`: log-line formatting reading the removed phases.
- `network.rs:9,12,131-132,159`: `Npc/NpcPosition/NpcRotation` change counters and `RagdollTelemetry`.

### 2g. `server/src/net/connection.rs` (347 lines)

- **`handle_connections` (57-128): the three `MessageReceiver::<…>::default()` / `MessageSender::<…>::default()`
  insert blocks (lines 84-126) are REDUNDANT.** Verified in lightyear 0.26.4:
  `lightyear_messages-0.26.4/src/server.rs:129-144` — `add_direction(ClientToServer)` does
  `register_required_components::<ClientOf, MessageReceiver<M>>()` (and `ServerToClient` →
  `MessageSender<M>`). Same on the client: `lightyear_messages-0.26.4/src/client.rs:11-26` registers
  them as required components of `Client`. **Delete lines 84-126 entirely** and keep only
  `ReplicationSender::new(...)` (83). This removes 43 lines and the entire `shared::protocol::{…}`
  import block (17-24). *Verify at runtime with the `verify` skill before committing* — it is a
  behavioural claim, not a compile-checked one.
- **`handle_disconnections` (132-346)**: the profile-save observer. Query tuple (138-152) references
  `Health`, `EquippedWeapon` (P1), `Inventory`, `HotbarSelection` (P2), `InVehicle`/`VehicleDriver`/
  `VehicleState`/`Vehicle` (P3), `RespawnTimer` (P5). The whole vehicle block (243-274, 334-338) and
  the `PlayerProfile { … }` literal (276-321) shrink with §4/D4. Survivors: `Player`, `PlayerPosition`,
  `PlayerRotation`, `PlayerProgression`, `PlayerProfiles`, `PlayerRosterCache`, `ProfileIoQueue`.
  `inputs: ResMut<ClientInputs>` (153, 238-239, 344-345) dies with §1d option (a).
- `configured_replication_send_interval` / `_mode` (34-53) — **KEEP** (`CITYSIM_REPLICATION_SEND_*`).

### 2h. `client/src/main.rs` (63 lines) — module declarations

Delete `mod chest;` (7), `mod crosshair;` (8), `mod dialogue;` (10), `mod pickup;` (11), `mod rail;` (14),
`mod weapon_view;` (21), `mod weapons;` (22). `mod camera;` (6) and `mod input;` (10) are rewritten for
the flycam, not deleted. `get_asset_path()` (32-45) and `GameClient` (29) are **KEEP**.

### 2i. `client/src/app_wiring/mod.rs` (37 lines)

- Line 31: `use shared::weapons::WeaponDebugMode;` — P1.
- Lines 33-36: the big `use crate::{…}` — remove `chest, crosshair, dialogue, pickup, rail, weapon_view, weapons`.
- Everything else (`AssetPlugin`, `AudioPlugin` spatial scale, diagnostics plugins, `WindowMode`,
  `UiScale`, `ClientPlugins`, `ProtocolPlugin`, `GraphicsSettings`, `LAUNCHER_RESOLUTION`) is **KEEP**.

### 2j. `client/src/app_wiring/plugins.rs` (134 lines) — `setup_plugins`

| Line | Registration | Slice |
|------|--------------|-------|
| 7, 15-19 | `rail_mode = super::dev::rail_mode_enabled()` + the `if rail_mode { "Railroad Tycoon Prototype" } else { "FistForce" }` window title | P3 + P6 rename → a single literal |
| 25-28 | `AssetPlugin { file_path }` | KEEP |
| 29-50 | `RenderPlugin` / wgpu features (incl. the macOS `disabled_features` block) | KEEP |
| 53-56 | `AudioPlugin { default_spatial_scale: SpatialScale::new(0.2) }` | KEEP (framework) |
| 60-93 | `FrameTimeDiagnosticsPlugin`, `EntityCountDiagnosticsPlugin`, `RenderDiagnosticsPlugin`, `SystemInformationDiagnosticsPlugin`, `LogDiagnosticsPlugin` behind `FISTFORCE_RENDER_DIAG` / `FISTFORCE_SYSINFO_DIAG` / `FISTFORCE_LOG_DIAGNOSTICS` | KEEP |
| 101 | `app.init_state::<GameState>()` | KEEP |
| 104-107 | `ClientPlugins { tick_duration }`, `ProtocolPlugin` | KEEP |
| 110 | `terrain::TerrainPlugin` | KEEP |
| 111 | `water::WaterPlugin` | KEEP |
| 112 | `city::CityPlugin` | KEEP |
| **113** | **`render::sniper_fisheye::SniperFisheyePlugin`** | **P1 delete** |
| 116 | `props::PropsPlugin` | KEEP |
| 119-122 | `ui::MainMenuPlugin`, `ui::PauseMenuPlugin`, `ui::NameEntryPlugin`, `ui::DebugTimeMenuPlugin` | KEEP |
| 126-133 | the `if !rail_mode { … }` block: `ui::InventoryPlugin` **P2**, `ui::WorldMapPlugin` **KEEP** (move out of the branch), `pickup::PickupPlugin` **P2**, `chest::ChestPlugin` **P2**, `audio::GameAudioPlugin` **KEEP-but-gutted** (§3), `dialogue::DialoguePlugin` **P4** |

### 2k. `client/src/app_wiring/resources.rs` (36 lines) — `setup_resources`

Survivors: `game_systems::GraphicsSettings` (27), `game_systems::InputSettings` (30),
`input::InputState` (33). Everything else dies:

- P1: `WeaponDebugMode`(7), `ShootingState`(10), `MeleeSwingState`(11), `ShootInputSuppress`(13),
  `ReloadState`(14), `DebugBulletTrails`(15), `PlayerOwnerIndex`(18), `RemoteMuzzleIndex`(19),
  `WeaponWarmupQueue`(20), `weapon_view::CurrentWeaponView`(21), `CurrentThirdPersonWeapon`(22),
  `RemoteWeaponIndex`(23), `weapon_view::offhand::OffhandShieldIndex`(12)
- **Rescue (D3):** `weapons::PerfOverlayEnabled`(8), `weapons::PerfDropMonitor`(9),
  `weapons::ClientPerfConfig`(16), `weapons::ClientPerfSnapshot`(17)
- P1: `audio::RemoteAudioEmitterIndex`(24)
- P5: `game_systems::LastCameraMode`(34)
- P3: `rail::setup_rail_resources(app)`(35)

### 2l. `client/src/app_wiring/systems.rs` (398 lines) — `setup_systems`

`wire_rail_systems` (100-147) **deleted wholesale (P3)**; `super::dev::rail_mode_enabled()` branch
(12-16) collapses to a direct `wire_*_systems(app)` call.

`wire_common_systems` (21-98) — **all KEEP**: `apply_connect_window_settings`, `setup_rendering`,
`sync_scene_render_target`, the `FISTFORCE_AUTOCONNECT` pair (35-44), `cleanup_enter_main_menu`,
`handle_start_connection`, `update_connection_status`, the four `render::hierarchy_fix::*`
registrations (64-74), and the sky/day-night/cloud groups (77-97).

`wire_fps_systems` (150-397) — ~247 lines; survivors are few:

| Lines | Group | Verdict |
|---|---|---|
| 152-164 | Startup asset setup: `setup_debug_physics_box_assets`(P5), `setup_particle_assets`(**KEEP** — sand particles), `setup_vehicle_visual_assets`(P3), `weapons::setup_weapon_visual_assets`+`setup_weapon_audio_assets`(P1), `weapon_view::setup_weapon_model_assets`(P1), `setup_player_character_assets`(P5), `setup_npc_assets`(P4) | keep 1 of 8 |
| 167-177 | `OnEnter(Playing)`: `spawn_world` **KEEP**; `crosshair::spawn_crosshair`/`spawn_death_screen`(P1), `weapon_view::spawn_weapon_hud`(P1), `weapons::spawn_debug_overlay`(**rescue, D3**), `weapons::cleanup_shoot_input_suppress`(P1) | keep 1 (+1 rescued) |
| 180-191 | `OnExit(Playing)` cleanup — all P1 except `weapons::despawn_debug_overlay` (**rescue**) |
| 194-198 | `FixedUpdate`: `input::handle_send_input_to_server` | §1d |
| 201-212 | `handle_player_spawned`, `sync_player_character_models`(P5), `handle_npc_spawned`(P4), `handle_vehicle_spawned`(P3), `ensure_local_player_tag`(P5) — this block is the replication→visual bridge; rewrite for units |
| 215-243 | Main gameplay group — `input::handle_keyboard_input` (rewrite for flycam), `update_vehicle_state`(P3), `handle_mouse_input`(rewrite), `update_death_state`(P1), `apply_cursor_grab`(KEEP), `spawn_debug_physics_box_visuals`(P5), `setup_steam_car_visual_rigs`(P3), the chained transform-sync tuple (P3/P4/P5) + `camera::update_camera`(rewrite), vehicle hover/shadow trio(P3), `camera::update_camera_fov`(rewrite), `camera::update_sniper_fisheye`(P1), `spawn_sand_particles`+`update_sand_particles`(**KEEP**) |
| 246-256 | Player rig/animation — all P5 |
| 259-278 | NPC rig/animation/ragdoll/gizmos — all P4 |
| 280-289 | crosshair group — all P1 |
| 291-308 | weapons group — P1 except `weapons::update_client_perf_snapshot` (**rescue**) |
| 310-332 | bullet visuals / blood / muzzle / hit confirms — all P1 |
| 334-347 | `handle_toggle_perf_overlay` (**rescue**), `handle_toggle_debug_mode`(P1), `update_trajectory_debug_gizmos`(P1) |
| 349-370 | `update_debug_overlay` (**rescue**, note the 125 ms `on_timer`), `update_perf_drop_monitor` (**rescue**, 500 ms `on_timer`), `emit_client_perf_summary` (**rescue**) |
| 373-397 | weapon_view group (13 systems) — all P1 |

### 2m. `client/src/app_wiring/dev.rs` (66 lines)

- Lines 5-9: `rail_mode_enabled()` — **P3 delete** (also removes the `FISTFORCE_RAIL` flag).
- Lines 16-66: `autoconnect_name()`, `autoconnect_from_main_menu`, `autoconnect_submit_name` —
  **KEEP** (`FISTFORCE_AUTOCONNECT`; the `verify` skill depends on the exact log strings
  `"FISTFORCE_AUTOCONNECT: skipping main menu"` at line 42 and the `SubmitPlayerName` send at 62).

### 2n. `client/src/states.rs` (14 lines)

`GameState { MainMenu, Connecting, Connected, Playing, Paused }` — **entirely KEEP**, no slice
coupling. Only `Paused` semantics change (no gameplay input to suppress).

### 2o. `client/src/render/systems/connection.rs` (263 lines)

- `handle_start_connection` (30-152): **the four `MessageSender`/`MessageReceiver` insert blocks
  (97-144) are redundant** — same lightyear finding as §2g. Delete them all. Also delete
  `commands.insert_resource(crate::rail::RailLocalPeerId(client_id));` (line 63, **P3**).
- `update_connection_status` (156-185): **KEEP**.
- `apply_cursor_grab` (192-213): KEEP (uses `InputState::ui_blocking`).
- `cleanup_enter_main_menu` (220-262): **external ref** — queries `Query<Entity, With<Npc>>` (226, 247-249)
  and `Query<Entity, With<Vehicle>>` (227, 251-253). Delete those two queries and the imports at
  lines 13 (`shared::vehicle::Vehicle`) and 21 (`shared::components::Npc`). Keep the world-root,
  `Player`, `SandParticle` and `LoadedChunks` cleanup.

### 2p. `client/src/ui/debug_time_menu/` — KEEP menu, three dying buttons

`mod.rs:21-25` imports `PlayerCharacter`(P5), `SetPlayerCharacter`(P5), `SpawnOilmanDebug`(P4),
`SpawnPhysicsBoxDebug`(P5). Remove:

- `actions.rs:62-73` — `char_sender`, `npc_spawn_sender`, `physics_box_sender` system params, and
  the corresponding arms at 147, 164, 174, 188-197.
- `mod.rs:169,173` — `SpawnOilmanNpcButton`, `SpawnDummyNpcButton` markers (+ `SpawnPhysicsBoxButton`).
- `layout.rs:153-154, 298-330` — the three button spawn fns.
- `state_sync.rs` — `sync_debug_character_selection`, `update_character_button_label`, and the
  `DebugCharacterSelection` resource.
- `actions.rs:74` — `local_player_transforms: Query<&Transform, With<LocalPlayer>>` (P5).

Keep: time-of-day buttons, `CloudCoverButton`, `FlyToggleButton` (repoint at the flycam),
`PerfWeightmapToggleButton`, `PerfRenderDiagToggleButton`, `DebugPerfSettings` (`mod.rs:123-126`).

### 2q. Other KEEP files with dying imports (compile-time breakages)

| File:line | Symbol | Slice |
|---|---|---|
| `client/src/streaming.rs:11,13` | `LocalPlayer`, `PlayerPosition`, `crate::rail::RtsRailCamera` | P3/P5 — **see D2** |
| `shared/src/prelude.rs:6-20` | 17 re-exports | see **D1** |
| `client/src/audio/mod.rs:30-37` | `Npc`(P4), `AudioEvent`/`AudioEventKind`(P1), `Vehicle`/`VehicleDriver`/`VehicleState`(P3), `crate::camera::peer_id_to_u64`(P5) | §3 |
| `client/src/input.rs:8,11,231-255` | `Health`(P1), `VehicleDriver`/`VehicleInput`(P3), local `peer_id_to_u64` + `in_vehicle` detection | rewrite |
| `client/src/weapons/mod.rs:74` | `use crate::camera::peer_id_to_u64;` | P1/P5 |
| `server/src/telemetry/{perf,network}.rs` | see §2f | P1/P2/P4 |
| `client/src/render/systems/mod.rs:5-21` | `mod npc; mod player; mod vehicle; mod debug_physics;` + glob re-exports | P3/P4/P5 |

---

## 3. `client/src/audio/` — KEEP framework, gutted content

`GameAudioPlugin` is on the KEEP list, but **every one of its 20 `.ogg` files dies** (see §4).
Module-by-module (`client/src/audio/mod.rs:3-24`):

| Submodule | Verdict |
|---|---|
| `assets.rs` (`setup_audio`, `ensure_audio_assets_loaded`) | keep the loader shell; every handle it loads dies |
| `state.rs` (`AudioManager`) | **KEEP** |
| `limits.rs` (`apply_audio_limits`) | **KEEP** — generic voice cap |
| `ambient.rs` (`ensure_ambient_entity`, `update_desert_walking_ambient`, `cleanup_*`) | `walking_desert.ogg` is a footstep loop → **P5** |
| `remote_players.rs` (`handle_remote_audio_events`, `ensure_remote_footstep_emitters`, `update_remote_footstep_emitters`, `RemoteAudioEmitterIndex`) | **P1** (`AudioEvent` receiver at line 22) + P5 (footsteps) |
| `vehicles.rs` (5 systems) | **P3** |
| `paths.rs` (7 consts) | all 7 die (4× gunshot P1, walking P5, 2× hover/bike P3) |

Net: the audio *plugin* survives as `AudioManager` + `apply_audio_limits` + `setup_audio` with an
empty asset set. Decide whether to keep `bevy` feature `vorbis` (§4c).

---

## 4. Cargo dependencies

### 4a. Workspace root `Cargo.toml`

| Dep | Status after strip |
|---|---|
| `bevy = { features = ["vorbis", "jpeg", "ktx2"] }` | `jpeg` is **already unused** — zero `.jpg`/`.jpeg` files in `client/assets`. `ktx2` **KEEP** (`textures/terrain/optimized_1k/terrain_{albedo,normal}_array.ktx2`). `vorbis` becomes unused once all 20 `.ogg` files go — keep it if the audio framework is meant to stay usable. |
| `lightyear = "0.26"` | **KEEP** |
| `serde` | **KEEP** |
| `bevy_rapier3d = "0.33.0"` | **KEEP** — used by `server/` (terrain/static colliders, `queries.rs` raycasts, `layers.rs`) and by `tools/collider_baker` (prop collider baking, KEEP). Only the *dynamic actor* usage dies. |
| `noise`, `rand`, `ron`, `image` | **KEEP** (see per-crate below) |
| `avian` | **not present anywhere** — no action |

### 4b. `shared/Cargo.toml`

| Dep | Verdict |
|---|---|
| `bevy`, `lightyear`, `serde` | KEEP |
| `noise` | KEEP — `shared/src/terrain/generator/` |
| **`rand`** | **BECOMES UNUSED (P1).** The only usage in the whole crate is `shared/src/weapons/ballistics.rs:90-91` (`rand::random::<f32>()` for shot spread). Remove after P1. |
| `bincode` | KEEP — `shared/src/colliders.rs:29` and `player_profile` round-trips (the `protocol/messages.rs` uses are test-only) |
| `ron` | KEEP — `shared/src/map/{loader,save,editor_schema}.rs` |
| `image` | KEEP — `shared/src/map/loader.rs:2` (`ImageReader` for heightmaps) |

### 4c. `client/Cargo.toml`

| Dep | Verdict |
|---|---|
| `bevy`, `lightyear`, `shared`, `serde` | KEEP |
| `noise` | **KEEP** — still used by `client/src/render/systems/rendering/clouds.rs:513` (`Fbm<Perlin>` cloud texture). Its other user, `client/src/weapons/assets.rs:6,240`, dies with P1. |
| `rand` | KEEP — `main.rs:56` client id, `render/systems/connection.rs:62`, `rendering/mod.rs:45-46` |
| `ron` | KEEP — `client/src/ui/main_menu/network_input.rs` (`assets/servers.ron`) |
| **`image`** | **ALREADY UNUSED.** `rg '(^|[^:.\w])image::' client/src` returns nothing; every hit is `bevy::image::…`. Remove now, independent of any slice. |
| `arboard = "3"` | KEEP — `client/src/ui/main_menu/mod.rs` (paste server address) |
| `[package.metadata.bundle]` name `"3DGame"` / identifier `"com.terninator.3dgame"` | rename candidate (§5) |

### 4d. `server/Cargo.toml`

| Dep | Verdict |
|---|---|
| `bevy`, `lightyear`, `shared` | KEEP |
| `bincode` | KEEP — `persistence/profiles.rs`, `player/roster_cache.rs`; `ai/ragdoll.rs` usage dies with P4 |
| `bevy_rapier3d` | **KEEP** — after P5 it is still needed by `physics/{terrain_colliders,static_world_colliders,queries,layers,contacts}.rs` and `app/mod.rs:43`. Files that lose it: `physics/dynamic_actors.rs` (P5), `combat/{fire,melee,hit_world,hit_characters}.rs` (P1), `ai/{spawn,ragdoll}.rs` (P4), `app/schedule.rs:164,183` (the `PhysicsSet` ordering anchors). |

### 4e. `editor/Cargo.toml` and `tools/*`

No changes. `editor` depends on `bevy, shared, serde, ron, bevy_egui 0.39, noise` — none touch a dying
slice (verified: `rg 'shared::' editor/src` yields only `map`, `city`, `terrain`, `props`, `building`).
`tools/collider_baker` keeps `bevy_rapier3d`; `tools/terrain_ktx_builder` is independent.

---

## 5. `client/assets/` — orphan inventory

Total tree: **~289 MB**, of which `maps/` is 124 MB and `textures/` 54 MB (both KEEP).

### 5a. Deletable, attributable to a dying slice

| Path | Size | Slice | Only referenced by |
|---|---|---|---|
| `client/assets/game_assets/weapons/` | **3.0 MB** | P1 | `client/src/weapon_view/assets.rs:23-35` loads 4 of 7 (`automatic_rifle.glb`, `shotgun.glb`, `sniper.glb`, `revolver.glb`). `Wep_Axe_01.glb`, `Wep_Pickaxe_01.glb`, `Wep_Spade_01.glb` (~0.75 MB) are **already orphaned** — zero references anywhere. |
| `client/assets/VFX/kenney_smoke-particles/` | **5.9 MB** | P1 | `client/src/weapons/assets.rs:67,128,152` (`PNG/White puff/whitePuff{NN}.png` → muzzle smoke + blood mist; `PNG/Flash/flash{NN}.png` → muzzle flash) |
| `client/assets/game_assets/items/` | **424 KB** | P2 | `client/src/pickup/assets.rs:9-29` (`bullet.glb`, `rifle_bullet.glb`, `shotgun_bullet.glb`, `stone.glb`, `wood.glb`) |
| `client/assets/ui/item_preview/` | **128 KB** | P2 | `client/src/ui/inventory/layout.rs:23-61` (9 PNGs) |
| `client/assets/game_assets/vehicles/` | **4.3 MB** | P3 | `client/src/render/systems/vehicle/mod.rs:138-139` (`hoverbike.glb`, `Veh_Steam_Car_01.glb`). `Car.glb` (53 KB) is **already orphaned**. |
| `client/assets/game_assets/trains/train.glb` | **1.3 MB** | P3 | `client/src/rail/mod.rs:130` |
| `client/assets/game_assets/buildings/train/train_station_lvl_1.glb` | **1.0 MB** | P3 | `client/src/rail/mod.rs:132` |
| `client/assets/audio/sfx/*` (gun) | ~230 KB | P1 | `client/src/weapons/paths.rs:4-13` + `client/src/audio/paths.rs:4-7`: `assault_shot`, `revolver_shot`, `shutgun_shot`, `sniper_shot`, `out_of_ammo`, `gun_reload`, `assualt_rifle_reload`, `revolver_reload`, `shotgun_reload`, `sniper_reload` |
| `client/assets/audio/sfx/{hover_idle_loop,bike_cruise_loop,hover_idle_loop_old}.ogg` | ~106 KB | P3 | `client/src/audio/paths.rs:9-10`; `hover_idle_loop_old.ogg` **already orphaned** |
| `client/assets/audio/dialogue/` | **184 KB** | P4 | `client/src/dialogue.rs:92-94` loads only `peasant/peasant1-3.ogg`; the whole `king/` subdir (3 files) is **already orphaned** |
| `client/assets/audio/ambient/walking_desert.ogg` | **48 KB** | P5 | `client/src/audio/paths.rs:8` (footstep loop) |

**Attributable total: ~16.6 MB.** After this, `client/assets/audio/` is **empty** — that is the trigger
to decide on the `vorbis` bevy feature (§4a).

Note: `client/src/render/sniper_fisheye.wgsl` (P1) lives in `client/src/render/`, **not** in
`client/assets/` — it is an `embedded_asset!` (`sniper_fisheye.rs:112`, `load_embedded_asset!` at 159).

### 5b. Explicit KEEP — do not touch

`characters/` (12 MB — D8, incl. the currently-unreferenced `sarah_animated.glb`, 7.1 MB),
`maps/` (124 MB), `textures/` (54 MB), `shaders/` (24 KB — `wind_foliage.wgsl`, `terrain_splat.wgsl`),
`toon_water.wgsl` (16 KB), `sky_10_2k/sky_10_2k.png` (1.2 MB), `colliders.bin` + `colliders_manifest.ron`
(D9), `servers.ron`, `game_assets/environment/` (19 MB, `shared/src/props/kinds.rs:164-213`),
`game_assets/buildings/{village,multistory}/` (`shared/src/building/defs.rs:73-104`,
`shared/src/city/buildings.rs:76-220`).

### 5c. Pre-existing orphans — NOT slice-attributable, verify separately

These have **no code reference** today and are unaffected by the strip. Handle as a separate,
independently revertable commit; several may be referenced from inside `.gltf` `uri` fields:

- `game_assets/props/{camp,walls,containers,treasure,furniture,misc}/` — ~11.8 MB
- `game_assets/buildings/desert/` — 3.4 MB
- `game_assets/textures/forest/` — 37 MB (⚠️ `game_assets/environment/trees/*.gltf` reference textures
  by **relative** `uri` in the same folder, e.g. `"uri":"Bark_NormalTree.png"`, so `textures/forest`
  looks genuinely unused — but confirm before deleting 37 MB)
- `game_assets/environment/{trees_lowpoly,clouds}/`
- `sky_10_2k/sky_10_cubemap_2k/` — 1.2 MB
- `ui/fistforce.png` — 192 KB, referenced at `client/src/ui/main_menu/layout.rs:38`; dies only when
  the game is renamed (§6)
- `maps/city_alpha_backup_2026-07-13/` (3.4 MB) and the three `map.ron.*.bak` / `edits.ron.pre-bake.bak`
  files inside `maps/city_alpha/` (~2.6 MB) — editor backups, not runtime assets

---

## 6. Naming: `FistForce` / `citysim` inventory

55 occurrences across 18 files (excluding `STRIP_PLAN.md`).

### 6a. SAFE to rename (source-internal, compiler-checked)

| Location | Value |
|---|---|
| `client/src/app_wiring/plugins.rs:18` | window title `"FistForce"` (and the `"Railroad Tycoon Prototype"` branch at 16, deleted with P3) |
| `client/src/app_wiring/systems.rs:3,149`, `client/src/app_wiring/dev.rs:6`, `server/src/app/schedule.rs:3` | doc comments |
| `README.md:1,58,141-145` | title + env-var docs |
| `client/Cargo.toml` `[package.metadata.bundle]` | `name = "3DGame"`, `identifier = "com.terninator.3dgame"`, `short_description` |
| `client/src/ui/main_menu/layout.rs:38` + `client/assets/ui/fistforce.png` | logo asset — rename together |

### 6b. RISKY — external contracts, rename only deliberately

| Identifier | Where | Why risky |
|---|---|---|
| **`FISTFORCE_AUTOCONNECT`** | `client/src/app_wiring/dev.rs:20`; log strings at 42, 59 | Hard-coded in `.claude/skills/verify/SKILL.md:22,27,33` and `STRIP_PLAN.md:138`. The skill greps for the literal log line. |
| **`FISTFORCE_CLIENT_PERF`, `FISTFORCE_CLIENT_PERF_INTERVAL_SECS`** | `client/src/weapons/state.rs:260-261` | Same skill, line 28; success signal is `ClientPerf frame_ms_p50=`. Also moving with D3. |
| **`CITYSIM_MAX_NPCS`** | `server/src/ai/spawn.rs:30` | `verify` skill line 18 — but the flag dies with P4 anyway; update the skill in the P4 commit. |
| **`FISTFORCE_ASSET_PATH`** | `editor/src/app.rs:28` (**set**), `editor/src/app.rs:126` + `editor/src/ui.rs:1191` + `shared/src/map/loader.rs:261` (**read**) | Cross-crate contract between the KEEP editor and KEEP map loader. Rename requires all four sites in one commit. |
| **`CITYSIM_MAP_ID`** | `editor/src/app.rs`, `editor/src/tools.rs:76`, `shared/src/terrain/generator/map_access.rs:16` | Selects which `client/assets/maps/<id>/` to load. Same three-site constraint. |
| **`FLY_APP_NAME`** | `shared/src/protocol/config.rs:14`, `server/src/app/bootstrap.rs:35` | **Not ours** — Fly.io platform var. Never rename. |
| `fly.toml:1` `app = 'fistforce'` | deploy | Renaming the Fly app means re-creating it (new IPs) and updating `.github/workflows/fly-deploy.yml`. Low value; defer. |
| `server_data/players/` (`server/src/app/resources.rs:21`) | save dir | Gitignored (`.gitignore:24`), 48 local files. Renaming orphans local saves silently — combine with the `PROFILE_VERSION` bump (D4) if you do it at all. |
| `client/assets/maps/city_alpha/` | map id | `DEFAULT_MAP_ID` in `shared/src/map`; renaming means renaming a 124 MB directory and every reference. **Don't.** |

### 6c. Env vars that simply die with their slice

`FISTFORCE_RAIL` (P3: `client/src/app_wiring/dev.rs:8`, `server/src/app/schedule.rs:30`);
`CITYSIM_MAX_NPCS`, `CITYSIM_DUMMY_NPCS`, `CITYSIM_NPC_MAX_UPDATES_PER_TICK`,
`CITYSIM_NPC_RELEVANCE_{RADIUS,EXIT_RADIUS,HZ}`, `CITYSIM_PATHFINDING_REQUESTS_PER_TICK`,
`CITYSIM_CORPSE_CAP`, `CITYSIM_RAGDOLL_POSE_HZ`, `CITYSIM_RAGDOLL_TEST_SECS` (all P4);
`CITYSIM_NPC_PHYSICS_{RADIUS,EXIT_RADIUS}` (P4/P5).

Survivors to keep working: `CITYSIM_TERRAIN_COLLIDER_{RADIUS_CHUNKS,MAX_LOAD_PER_TICK,RESOLUTION}`,
`CITYSIM_PROP_COLLIDERS_PER_TICK`, `CITYSIM_REPLICATION_SEND_{INTERVAL_MS,MODE}`,
`CITYSIM_NET_DEBUG[_INTERVAL_SECS]`, `CITYSIM_SERVER_HOTLOG`, `CITYSIM_LAYOUT_DEBUG`,
`FISTFORCE_SERVER_PERF`, `FISTFORCE_PROP_CHUNK_RADIUS`, `FISTFORCE_HIERARCHY_{AUDIT,TRACE}`,
`FISTFORCE_HITCH_{DETAIL,THRESHOLD_MS,LOGGING}`, `FISTFORCE_{RENDER_DIAG,SYSINFO_DIAG,LOG_DIAGNOSTICS,
RENDER_DIAG_LOGGING,WEIGHTMAP_STATS,PROFILE_HITCHES}`, `CITYSIM_PROFILE_HITCHES`, `BEVY_ASSET_ROOT`.

### 6d. Docs

`README.md` (full rewrite per STRIP_PLAN P6); `RAGDOLL_HANDOFF.md` is already deleted in the working
tree (` D RAGDOLL_HANDOFF.md`) — remove any remaining link; `CONTRIBUTING.md` — check for FPS-specific
guidance; `run.sh` — mode names are generic (`server|client|both|multi|editor|windows`), no change needed.

---

## 7. Execution checklist (P6, after P1–P5 are green)

1. [ ] Bump `PROTOCOL_ID` — `shared/src/protocol/config.rs:5`. (D6)
2. [ ] `shared/src/protocol/plugin.rs`: delete 32 component registrations, 26 message registrations,
       `RagdollPoseChannel`; collapse the import block (4-21).
3. [ ] `shared/src/protocol/messages.rs`: delete every P1–P5 message struct/enum + the ragdoll
       packing helpers; rewrite/slim `PlayerInput` per §1d; delete or rewrite all 5 tests.
4. [ ] `shared/src/lib.rs`: drop `pub mod` lines for the dead modules; delete `prelude.rs` (D1).
5. [ ] `server/src/main.rs`: drop 5 `mod` declarations.
6. [ ] `server/src/app/schedule.rs`: delete `configure_rail_fixed_schedule` + `RailServerSet`,
       shrink `FpsServerSet` to 5 variants, delete 7 system groups, **re-anchor the 9 telemetry
       ordering constraints (263-293)**.
7. [ ] `server/src/app/bootstrap.rs`: delete the `rail_mode_enabled` branch (93-108).
8. [ ] `server/src/app/resources.rs`: delete 13 `init_resource` lines.
9. [ ] `server/src/net/connection.rs`: delete the redundant sender/receiver insert blocks (84-126)
       and slim `handle_disconnections`' query + `PlayerProfile` literal.
10. [ ] `server/src/telemetry/{perf,network}.rs`: shrink `Phase`, drop the dead entity-count queries.
11. [ ] `client/src/main.rs`: drop 7 `mod` declarations.
12. [ ] `client/src/app_wiring/{mod,plugins,resources,systems,dev}.rs`: per §2i–§2m; **rescue the perf
        overlay first** (D3).
13. [ ] `client/src/render/systems/connection.rs`: delete the redundant message-component blocks
        (97-144), `RailLocalPeerId` (63), the `Npc`/`Vehicle` cleanup queries (226-227, 247-253).
14. [ ] `client/src/streaming.rs`: re-anchor on the camera only (D2) — **do this before P3.**
15. [ ] `client/src/ui/debug_time_menu/`: remove the 3 dead buttons and their senders (§2p).
16. [ ] `client/src/audio/`: delete `remote_players.rs`, `vehicles.rs`, `ambient.rs`, `paths.rs`;
        keep `state.rs` + `limits.rs` + a gutted `assets.rs` (§3).
17. [ ] Cargo: remove `image` from `client/Cargo.toml`; remove `rand` from `shared/Cargo.toml`;
        remove the `jpeg` bevy feature; decide on `vorbis`.
18. [ ] Assets: delete the 11 paths in §5a (~16.6 MB). Separate commit for §5c.
19. [ ] Naming: §6a in one commit; §6b only with the matching skill/doc edits.
20. [ ] Verify: `cargo check --workspace --all-targets` · `cargo test -p shared` · `cargo test -p editor`
        · `cargo run -p editor` · the `verify` skill smoke test (server + `FISTFORCE_AUTOCONNECT`
        client, grep for `Name accepted!`, `Spawned client world visuals`, `ClientPerf frame_ms_p50=`,
        and the absence of `ERROR bevy_asset`).
