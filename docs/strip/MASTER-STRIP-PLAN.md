# MASTER STRIP PLAN — citysim FPS → top-down multiplayer unit-tactics game

Repo: `/Users/terminator2/Coding/citysim` @ `main` (`0862414`), Bevy 0.18.1 + lightyear 0.26.4.
Workspace members: `shared/ client/ server/ editor/ tools/collider_baker tools/terrain_ktx_builder`.
**Recovery for everything deleted:** `git show citysim-final:<path>`.
**NOT recoverable:** `server_data/` (gitignored, absent from the tag).

Synthesised from seven mapping manifests (`manifest-p1-combat.md` … `manifest-p6-protocol-assets.md`,
`manifest-keep-boundary.md`). Where a slice manifest and the KEEP-side boundary audit disagreed, the
boundary audit won unless direct inspection of the tree said otherwise — every such override is recorded
in §2 with the evidence.

**Rule for the whole strip:** `cargo check --workspace --all-targets` must be green at the end of every
numbered step below, and `cargo check -p editor` is a hard gate after **every** phase. The editor is the
only KEEP-list crate that nothing else depends on, so nothing else will catch its breakage.

---

## 1. ORDERING — recommended execution order and why

```
P0  Pre-flight refactor (NEW — no deletions)   ← the whole plan hinges on this
P1  Weapons & combat
P2  Items & inventory
P3  Vehicles & rail
P4  NPCs & AI
P5  FPS embodiment → commander
P6  Protocol slim, wiring, assets, naming
```

The stated P1→P6 order is **correct and should be kept**, with one structural change and three
qualifications.

### 1.1 Why a P0 is mandatory

Five KEEP-list subsystems have their load-bearing types physically located inside kill-list modules.
Deleting in the stated order without a P0 means you discover each of these *mid-deletion*, with the tree
already broken, and end up refactoring a KEEP subsystem while its neighbours are half-deleted.

| Type / symbol | Lives in (kill list) | Needed by (KEEP list) | Breaks at |
|---|---|---|---|
| `RtsRailCamera` (focus field) | `client/src/rail/mod.rs:70` | `client/src/streaming.rs:13,17` → **all** terrain + prop + LOD streaming (10 call sites) | P3 |
| perf overlay (`ClientPerfConfig`, `PerfOverlayEnabled`, `spawn/update/despawn_debug_overlay`, `emit_client_perf_summary`, …) | `client/src/weapons/{state,debug}.rs` | F3 FPS overlay, `FISTFORCE_CLIENT_PERF`, and the `verify` skill's success grep `ClientPerf frame_ms_p50=` | P1 |
| `WeaponDebugMode` (bare `Resource(pub bool)`) | `shared/src/weapons/debug.rs:5` | `client/src/props/debug.rs:5,147` `debug_draw_prop_colliders` | P1 |
| `Pickable` / `Pickable::IGNORE` | `client/src/crosshair/mod.rs:61` | `client/src/water/overlay.rs:32` (reached via glob import — invisible to a naive grep) | P1 |
| `peer_id_to_u64` | `client/src/camera.rs:36` **and** `client/src/render/systems/player/ids.rs:6` | `client/src/audio/mod.rs:37` | P5 |

Plus two non-type P0 items that remove churn from four later phases (§3, P0-6 and P0-7).

### 1.2 Qualifications on the stated order

1. **P3 must run before P5** — `server/src/collision/resolve_vehicle.rs` imports
   `collision::building_geometry::handle_capsule_vs_buildings` and `collision::geometry::*`, both of
   which P5 deletes. Same for P4's `resolve_npc.rs`. Running P5 early leaves the tree unbuildable across
   several commits.
2. **P4 must run before P5** — `server/src/physics/dynamic_actors.rs:29` imports
   `crate::ai::ragdoll::NpcRagdoll`, and `server/src/collision/resolve_player.rs:12` imports
   `crate::ai::ragdoll::{CorpseBodyPoint, CorpseCollisionIndex}`. P5 deletes those files, but they must
   still *compile* between P4 and P5.
3. **P6 must run last** — `shared/src/protocol/messages.rs:4-8` imports from five different dying slices
   (`HitZone`/`HitBodyPart` P1, `VehicleInput`+`CargoKind`+rail ids P3, `NpcArchetype` P4,
   `PlayerCharacter` P5). Slimming the protocol before those land produces an unresolvable import
   cascade. **Exception:** three P6 items are hoisted into P0 (`prelude.rs` deletion, `streaming.rs`
   re-anchor, telemetry re-anchor) — see §3.

### 1.3 Rejected alternative orders

* **P2 before P1** — rejected. `shared/src/weapons/types.rs:177-195` (`ammo_type`, `as_item_type`),
  `shared/src/components/combat.rs:118-137` (`reload_from_inventory`), `server/src/combat/reload.rs`,
  `client/src/weapon_view/{hotbar_input,weapon_hud}.rs` and `client/src/weapons/input.rs:311` all reach
  *into* items. Running P2 first forces you to surgically edit six files that P1 deletes wholesale a day
  later. P1-first means those files simply vanish.
* **P5 early (to unblock the tactics camera)** — rejected. P5 is the most entangled phase and depends on
  P3+P4 (§1.2). The tactics camera you actually want arrives in **P0** via the `RtsRailCamera` harvest,
  so there is no reason to pull P5 forward.
* **A single mega-commit** — rejected. `server_data` corruption (§6) is silent; per-phase
  `PROFILE_VERSION` bumps are the only detection mechanism.

---

## 2. RESOLVED CONFLICTS AND OVERRIDES

Twelve genuine disagreements between manifests. Each resolution below is binding for the rest of this
document.

### C1. `PlayerPosition` / `PlayerRotation` — **KEEP, do not introduce `CommanderView`**

* P5 manifest: delete `PlayerPosition`, add a new replicated `CommanderView(Vec3)`.
* P6 manifest + boundary audit: keep `PlayerPosition`, it *becomes* the commander view position.

**Ruling: P6/boundary wins. Keep `PlayerPosition` and `PlayerRotation`; do NOT add `CommanderView`.**
Verified in the tree: `shared/src/components/actors.rs:119` is `pub struct PlayerPosition(pub Vec3);` and
`:123` is `pub struct PlayerRotation(pub f32);` — bare newtypes with zero embodiment semantics.
Reusing them instead of renaming avoids editing **all** of: `client/src/streaming.rs` (`AnchorPlayer`),
`client/src/ui/world_map/markers.rs`, `server/src/physics/terrain_colliders.rs`,
`server/src/collision/streaming.rs`, `server/src/persistence/autosave.rs`,
`server/src/net/connection.rs` profile writer, and the two protocol registrations. Estimated saving:
~150 lines of churn and six KEEP-file rewrites, for zero functional difference. The commander writes
`PlayerPosition` = camera focus, `PlayerRotation` = camera yaw.

### C2. `Health` — **KEEP, split into `shared/src/components/health.rs`**

P1 argued for it in detail; P6 left it open; boundary audit said "decide now".
**Ruling: keep.** `shared/src/components/combat.rs:8-43` defines `Health { current, max }` with
`new/take_damage/heal/is_dead/percentage` and **zero** weapon coupling (the file's
`use crate::weapons::WeaponType` at line 4 serves only `EquippedWeapon` and `Bullet`). 15+ surviving call
sites want it, and tactics units will want it. Move it verbatim to
`shared/src/components/health.rs`, repoint `shared/src/components/mod.rs:4,8`, keep
`register_component::<Health>()` at `shared/src/protocol/plugin.rs:61`, then delete `combat.rs`.

### C3. `AudioEvent` — **DELETE in P1** (boundary audit overridden, with evidence)

* Boundary audit §7.7: "must survive P1, STUB the variant set".
* P1 + P6: delete it; every `AudioEventKind` variant is combat.

**Ruling: delete.** Verified directly: `rg -n AudioEvent server/src/net/connection.rs` returns **nothing**
— the server never inserts `MessageSender::<AudioEvent>` on `ClientOf` entities, so the `audio_senders`
queries at `server/src/combat/{fire.rs:27, hit_characters.rs:161, melee.rs:163}` match nothing and remote
gunshot/melee audio is **already dead at runtime**. Preserving a message with no producer, no consumer
that survives, and an all-combat variant set is pure liability. Re-adding a `UnitAudioEvent` later is
~20 lines. *Do not mistake the post-P1 silence for a regression you introduced.*

### C4. `client/src/render/lod.rs` — **DO NOT DELETE** (task brief overridden)

The brief lists it under P1. P1's own manifest and the boundary audit both flag this as wrong: the file
is 100% generic LOD (`VisibilityRangeBuilder`, `LodPolicy`, `LodLevel`, `apply_lod_visibility`,
`build_lod_visibility_range`, `ShadowCullPolicy`) and is consumed by `client/src/props/lod/mod.rs:19`,
which is explicitly KEEP. **Keep it.**

### C5. `client/src/render/systems/particles.rs` — **P3 owns the spawner, P6 owns the file**

* P1: "not combat, belongs to P3".  * P3: delete `spawn_sand_particles` only, keep setup/update.
* P6: keep sand particles.

**Ruling:** P3 deletes `spawn_sand_particles` (75-163) + `use shared::vehicle::*` (7) +
`MAX_SAND_PARTICLES` (72), leaving `ParticleAssets` / `setup_particle_assets` / `update_sand_particles` /
`SandParticle` compiling but with no producer. P6 then deletes the whole file **or** keeps
`ParticleAssets` under `#[allow(dead_code)]` as the future unit-dust API. Do not delete the file in P3 —
`client/src/render/systems/connection.rs:16,228` and `client/src/weapons/debug.rs:253,403` still query
`SandParticle` at that point.

### C6. `inventory::death_drop::handle_inventory_drop_on_death` — **P1 deletes it**

P1 said "re-home into `FpsServerSet::Inventory` or delete with P2"; P2 said "delete with the Inventory
block". It is registered inside P1's `Combat` block (`server/src/app/schedule.rs:256`) **and** is the
`.after()` anchor for three telemetry systems (`:278, :280, :282`).
**Ruling: P1 deletes the system and the file** (`server/src/inventory/death_drop.rs`). Its trigger is
`Changed<Health>` + `is_dead`, which can never fire once nothing applies damage. Deleting it in P1 is
correct, not merely convenient — and P0-7 (§3) has already re-anchored the telemetry by then.

### C7. `DebugPhysicsBox` feature — **DELETE ENTIRELY in P4** (decision, not a discovery)

* P4: extract `handle_spawn_physics_box_debug` + `sync_debug_physics_boxes` out of `server/src/ai/spawn.rs`
  before deleting it (they are not on the P4 kill list).
* P5/P6: the feature dies at P5 anyway (it is a rapier **dynamic-body** demo; P5 removes dynamic actors).

**Ruling: delete the whole feature in P4** rather than extracting it and deleting it one phase later.
Scope of the deletion: `SpawnPhysicsBoxDebug` message + `DebugPhysicsBox`/`DebugPhysicsBoxPosition`/
`DebugPhysicsBoxRotation` components (`shared/src/components/actors.rs:102-115`,
`shared/src/protocol/plugin.rs:49-53,125-126`), `client/src/render/systems/debug_physics.rs`, the
debug-menu button, `server/src/ai/spawn.rs:375,482`, and
`physics::dynamic_actors::sync_debug_boxes_from_physics`.
**If you disagree and want the boxes**, extract the two fns to `server/src/physics/debug_boxes.rs` in
P4 — but understand they stop working in P5 when dynamic bodies go.

### C8. `server/src/combat/geometry.rs` segment raycasts — **rescue in P1**

Only P1 flagged these. `segment_terrain_intersection` (:306), `segment_props_intersection` (:362),
`segment_buildings_intersection` (:431) + private `ray_triangle_intersection` (:487). They are
`pub(super)`, so **the compiler will not warn you** when they are deleted.
**Ruling: move to `server/src/collision/raycast.rs` in P1**, carrying the test
`segment_terrain_intersection_returns_world_distance` (:562), and mark `#[allow(dead_code)]`.
Rationale beyond "P5 wants LOS": these query `WorldTerrain` / prop / building data **directly**, so they
work at any distance — unlike `server/src/physics/queries.rs::cast_world_impact`, which only sees terrain
colliders that have actually been *streamed* around the anchor. For a tactics game with a camera that
can pan far from any unit, the direct-query version is the more useful of the two. Keep both.

### C9. `SpawnMarkerKind::NpcGroup` — **DO NOT REMOVE THE VARIANT** (unanimous, restated because it bricks three crates)

P4, P6 and the boundary audit all agree. `client/assets/maps/city_alpha/edits.ron` (122 MB) contains a
live `kind: NpcGroup` marker. RON fails hard on unknown *enum variants* (unlike unknown struct fields).
Chain: `shared/src/map/save.rs:23-38` → `Err` → `shared/src/map/loader.rs:59` → `Err` →
`shared/src/terrain/generator/map_access.rs:21-23` **`panic!`**, which fires in `WorldTerrain` init in
**client, server AND editor**. Keep the variant; relabel its UI string to "Deploy Zone" if you like.
(`MapDefinition::npc_groups` — a *field* — is safe to delete; both shipped maps have `npc_groups: []` and
there is no `deny_unknown_fields` anywhere in the repo.)

### C10. `server_data/players/*.bin` — **move aside at P0, bump `PROFILE_VERSION` every schema commit**

P1 suggested "accept the reset"; P2 proved empirically that 48/48 profiles break and that an
*empty-inventory* profile decodes into **silent garbage that passes the version guard**; P3/P5/P6 all
demand a bump. Verified: 48 `.bin` files exist, `PROFILE_VERSION = 1` at
`shared/src/player_profile.rs:14`, and `server/src/persistence/profiles.rs:51` checks the version
**after** `bincode::deserialize` at `:48`.
**Ruling: do both.** `mv server_data/players server_data/players.pre-strip` at P0, **and** bump
`PROFILE_VERSION` in each phase that changes the layout: `1→2` (P1), `2→3` (P2), `3→4` (P3), `4→5` (P5).
Four cheap bumps beat one clever migration.

### C11. Redundant `MessageSender`/`MessageReceiver` inserts — **do NOT bulk-delete during the strip**

P6 claims (citing `lightyear_messages-0.26.4/src/server.rs:129-144` and `client.rs:11-26`) that
`add_direction()` already registers these as required components, so the ~85 lines of manual inserts in
`server/src/net/connection.rs:84-126` and `client/src/render/systems/connection.rs:97-144` can be deleted
wholesale.
**Ruling: prune entry-by-entry per phase** (compile-checked, zero risk). Then, as a **separate isolated
commit at the end of P6**, try the wholesale deletion and validate with the `verify` skill. The failure
mode of being wrong is "the server silently ignores client input", which looks like a gameplay bug, not a
build error — do not entangle it with 30k lines of deletions.

### C12. `PackedPlayerInput` bit renumbering — **leave gaps until P6**

P1 removes `FLAG_BLOCK` (bit 11), P3 removes bits 8/9/10, P5 removes bits 0-7. P3's manifest offers
"renumber `FLAG_BLOCK` from bit 11 → bit 9". **Do not renumber mid-strip.** Each phase deletes its field
and its flag constant and leaves the bit position vacant; the manual `Serialize` (`messages.rs:108`) and
`Deserialize` (`:170`) impls are edited in lockstep each time. P6 does one clean rewrite of the whole
packed struct. Two renumbering events = two chances to skew the wire format silently.

### Non-conflicts worth restating

* `client/src/pickup/` straddles P2 (items) and P3 (vehicle mount prompt) — **P2 deletes the directory**;
  P3's vehicle-prompt entries become no-ops if P2 ran first, which it does.
* `server/src/collision/resolve_npc.rs` appears in both P4 and P5 delete lists — **P4 owns it** (it is
  already unwired dead code; verified nothing registers it).
* `server/src/player/spatial.rs` — **P4 edits** (`Without<Npc>` filter at `:113`), **P5 deletes**.
* `client/src/audio/ambient.rs` + `walking_desert.ogg` — **P5 owns** (footstep ambience keyed off
  `InputState.forward/…` + `PlayerPosition`); P6's delete list is the follow-through.
* `shared/src/player.rs` **survives** (slimmed to `MOUSE_SENSITIVITY` + `SPAWN_POSITION`). The boundary
  audit's "move `SPAWN_POSITION` out" is unnecessary — the file itself is not deleted.

---

## 3. PHASE 0 — PRE-FLIGHT REFACTOR (no deletions, ~1 day)

Every item is a pure move/rename/re-anchor. The tree compiles and the game runs identically after each.
**This is the highest-leverage phase in the plan.**

### P0-1. Move the client perf overlay out of `client/src/weapons/`

Create `client/src/perf_overlay/mod.rs` (+ `mod perf_overlay;` in `client/src/main.rs`).

Move from `client/src/weapons/state.rs`:
`DebugOverlay` (:9), `FpsText` (:13), `PerfStatsText` (:17), `ClientPerfConfig` (:249-277, incl. the
`FISTFORCE_CLIENT_PERF` / `FISTFORCE_CLIENT_PERF_INTERVAL_SECS` / `FISTFORCE_HITCH_THRESHOLD_MS` reads at
:260-267), `ClientPerfSnapshot` (:281), `PerfOverlayEnabled` (:312), `PerfDropMonitor` (:316).

Move from `client/src/weapons/debug.rs`:
`handle_toggle_perf_overlay` (:76), `update_client_perf_snapshot` (:87), `emit_client_perf_summary`
(:134), `percentile_sorted` (:166), `spawn_debug_overlay` (:177), `update_debug_overlay` (:231),
`update_perf_drop_monitor` (:393), `despawn_debug_overlay` (:455), and `handle_toggle_debug_mode` (:66).
**Leave behind** `update_trajectory_debug_gizmos` (:7-64) — bullet-only, dies with P1.

While moving `update_debug_overlay`, delete these ParamSet members and their format-string slots
(`debug.rs:345`): `Query<(), With<Bullet>>` (:250), `With<LocalTracer>` (:251), `With<MuzzleSmoke>` (:254),
`With<MuzzleFlash>` (:257). Keep the `Vehicle` (:252) and `SandParticle` counters for now — they die in P3.

Repoint: `client/src/app_wiring/resources.rs:8,9,16,17`; `client/src/app_wiring/systems.rs:174,189,297,336,341,351,360,369`.

> **Do not skip.** `.claude/skills/verify/SKILL.md:36` greps `/tmp/client.log` for
> `ClientPerf frame_ms_p50=` as its success signal, and commit `5750fb6` was explicitly about bringing
> the FPS counter back.

### P0-2. Move `WeaponDebugMode` → `shared/src/debug.rs` as `DebugGizmoMode`

From `shared/src/weapons/debug.rs:5` (a one-line `#[derive(Resource)] pub struct WeaponDebugMode(pub bool)`).
Add `pub mod debug;` to `shared/src/lib.rs`. Repoint: `client/src/props/debug.rs:5,147` (**KEEP list**),
`client/src/app_wiring/mod.rs:31`, `client/src/app_wiring/resources.rs:7`,
`client/src/render/systems/npc/{mod.rs:39, debug.rs:29,119}` (P4, must compile until then).

### P0-3. Move `Pickable` → `client/src/ui/mod.rs`

From `client/src/crosshair/mod.rs:61-65` (`Pickable` + `Pickable::IGNORE`). Its KEEP-list consumer is
`client/src/water/overlay.rs:32`, which reaches it via `use super::*` — **grep will not show you this
edge**. Verify with `cargo check -p client` after the move.

### P0-4. Harvest the RTS camera out of `client/src/rail/` → `client/src/camera_rts.rs`

Move verbatim from `client/src/rail/mod.rs`:
`RtsRailCamera` struct (:70-80) + `impl Default` (:82-96) → rename `CommanderCamera`;
`ensure_rts_camera_controller` (:187-200); `update_rts_camera` (:202-277, WASD pan / RMB yaw / wheel zoom
/ map-bounds clamp); `release_cursor_for_rts` (:279-291); `update_cursor_terrain_hit` (:293-329,
viewport-scaled cursor ray — it already accounts for the offscreen scaled render target);
`apply_rts_transform` (:682-696); `intersect_terrain` (:698-737, march + binary-search vs heightfield).

Also move `RailLocalPeerId` (`client/src/rail/mod.rs:21`, a plain `Resource(pub u64)`) → `LocalPeerId` in
`client/src/render/systems/connection.rs` or a new `client/src/net_ids.rs`; repoint `connection.rs:63`.

**This is your tactics camera.** `editor/src/camera.rs` (KEEP) has a richer dual-mode version
(`EditorCameraMode::{Rts, Free}`, `EditorCameraController`, `update_editor_camera`) worth cribbing from.
Leave the rail module otherwise intact — P3 deletes the remainder.

### P0-5. Re-anchor `client/src/streaming.rs` on a keep-side focus component

Current file (verified verbatim):
```rust
use shared::components::{LocalPlayer, PlayerPosition};
use crate::rail::RtsRailCamera;                                          // :13
pub type AnchorPlayer<'w,'s> = Query<'w,'s, &'static PlayerPosition, With<LocalPlayer>>;   // :15
pub type AnchorCamera<'w,'s> =
    Query<'w,'s, (&'static GlobalTransform, Option<&'static RtsRailCamera>), With<Camera3d>>; // :16-17
pub fn streaming_anchor(player: &AnchorPlayer, camera: &AnchorCamera) -> Option<Vec3> { … }   // :19-27
```
Change **only** line 13/17: `Option<&'static RtsRailCamera>` → `Option<&'static crate::camera_rts::CommanderCamera>`
(or introduce `#[derive(Component)] pub struct WorldViewFocus { pub focus: Vec3 }` in `streaming.rs`
itself and have the camera insert it — marginally cleaner, one extra component).

**Keep `AnchorPlayer` and the type-alias names.** `PlayerPosition` survives per C1, so the 10 downstream
call sites need **zero** changes:
`client/src/terrain/streaming/{mod.rs:26, spawn.rs:8,9,23,101,102,108,158,159,168, regenerate.rs:65,83, far_terrain.rs:81,82,91}`,
`client/src/props/{spawn.rs:9,120,141,340,341,348, lod/mod.rs:21, lod/visibility.rs:110,111,127}`.

**Also add a `warn_once!` when `streaming_anchor` returns `None`.** This API **fails open**: every caller
does `let Some(anchor) = … else { return; }`, so a broken anchor produces an empty world with zero log
output. That silent mode is DANGER-3 in §6.

### P0-6. Re-anchor the nine telemetry ordering constraints onto SystemSets

`server/src/app/schedule.rs:263-293` (verified verbatim) orders nine telemetry systems against
*individual systems* that die across P1/P2/P4/P5:

| System | Current anchor | Dies in |
|---|---|---|
| `handle_perf_core_phase_end` | `.after(physics::dynamic_actors::sync_debug_boxes_from_physics)` | P4/P5 |
| `handle_perf_npc_inventory_build_phase_begin` | `.before(ai::obstacles::sync_obstacle_grid)` | P4 |
| `handle_perf_npc_inventory_build_phase_end` | `.after(inventory::chest::update_distant_chest_auto_close)` | P2 |
| `handle_perf_weapons_phase_begin` | `.before(combat::reload::update_reload_timers)` | P1 |
| `handle_perf_weapons_phase_end` | `.after(inventory::death_drop::handle_inventory_drop_on_death)` | P1 |
| `update_server_perf_log` | same | P1 |
| `sample_replication_change_pressure` | same | P1 |

Rewrite all of them to `.after(FpsServerSet::X)` / `.before(FpsServerSet::Y)` **now**, while every named
system still exists. This deletes an "re-anchor the telemetry" step from four separate phases and removes
the risk of a careless re-anchor silently skewing the `ServerPerf` phase brackets.
(`handle_perf_tick_begin` / `handle_perf_core_phase_begin` `.before(world::time::handle_set_time_of_day)`
and the `PostUpdate` `sample_link_flow_post_send` anchors survive untouched.)

### P0-7. Delete `shared/src/prelude.rs`

**Verified: zero consumers.** `rg -n "prelude"` across the workspace returns only `bevy::prelude` and
`lightyear::prelude`. The file re-exports 17 kill-side symbols (`EquippedWeapon`, `Npc`, `NpcPosition`,
`ChestStorage`, `HotbarSelection`, `Inventory`, `ItemStack`, `ItemType`, `can_interact_with_vehicle`,
`vehicle_def`, `InVehicle`, `Vehicle`, `VehicleDriver`, `VehicleState`, `VehicleType`, `WeaponStats`,
`WeaponType`) and will hard-break the build in P1/P2/P3 for no benefit.
Delete the file and `pub mod prelude;` (`shared/src/lib.rs:12`).

### P0-8. Move `server_data/players` aside

```
mv server_data/players server_data/players.pre-strip
```
48 bincode profiles, all dev junk (`asd`, `qweq`, `123123123`, `devclient`). Gitignored → not in
`citysim-final` → **not recoverable by any other means**. Do this before the first post-P1 server boot.

### P0 compile checkpoint
```
cargo check --workspace --all-targets
cargo check -p editor
cargo test -p shared
```
### P0 smoke test
Launch server + client per `.claude/skills/verify/SKILL.md`. Confirm:
`Name accepted!` · `Spawned client world visuals` · `ClientPerf frame_ms_p50=` in `/tmp/client.log` ·
F3 overlay renders · F4 prop-collider gizmos still toggle · **terrain chunks and props still stream while
walking** (the `streaming.rs` change is the one thing here that can silently do nothing).
`./run.sh editor` — open `city_alpha`, confirm the map loads and spawn markers render.

---

## 4. PHASE-BY-PHASE EXECUTION

Each phase: **(a) rescue/prep → (b) delete → (c) edit → (d) compile checkpoint → (e) smoke test.**

---

### PHASE 1 — Weapons & combat  (~9,100 lines)

#### 1a. Prep (P0 already did the overlay / `WeaponDebugMode` / `Pickable` rescues)

1. **Split `Health` out** (C2): create `shared/src/components/health.rs` with `Health` verbatim from
   `shared/src/components/combat.rs:8-43`; change `shared/src/components/mod.rs:4,8` from
   `mod combat; pub use combat::*;` to `mod health; pub use health::*;`. Compile.
2. **Rescue the segment raycasts** (C8): move `segment_terrain_intersection` (:306),
   `segment_props_intersection` (:362), `segment_buildings_intersection` (:431),
   `ray_triangle_intersection` (:487) and the test at `:562` from `server/src/combat/geometry.rs` to a new
   `server/src/collision/raycast.rs`; add `pub mod raycast;` to `server/src/collision/mod.rs`; mark
   `#[allow(dead_code)]`. Compile.
3. Bump `PROFILE_VERSION` `1 → 2` (`shared/src/player_profile.rs:14`).

#### 1b. Delete

```
client/src/weapons/                       (mod, assets, debug, input, paths, projectiles, state, warmup,
                                           effects/{mod,blood,impacts,muzzle})
client/src/weapon_view/                   (mod, assets, hotbar_input, melee_models, offhand, slash_trail,
                                           third_person, view_model, weapon_hud)
client/src/crosshair/                     (mod, death_screen, hit_markers, hud)
client/src/render/sniper_fisheye.rs
client/src/render/sniper_fisheye.wgsl
server/src/combat/                        (mod, bullet_sim, cleanup, fire, geometry, hit_characters,
                                           hit_world, melee, reload, target_index)
server/src/inventory/death_drop.rs        (C6)
shared/src/weapons/                       (mod, ballistics, constants, damage, debug, melee, offsets, types)
shared/src/components/combat.rs           (after the Health split)
```

#### 1c. Edit — exact symbols

**shared/**
* `shared/src/lib.rs:21` — remove `pub mod weapons;`.
* `shared/src/protocol/plugin.rs` — remove `register_component::<EquippedWeapon>` (:62), `<Bullet>` (:65),
  `<BulletVelocity>` (:66), `<PlayerMeleeState>` (:35); **keep `<Health>` (:61)**; remove
  `register_message::<{ShootRequest(:111), SwitchWeapon(:113), ReloadRequest(:115), MeleeAttackRequest(:117),
  HitConfirm(:165), BulletImpact(:167), DamageReceived(:169), PlayerKilled(:171), AudioEvent(:173)}>`;
  trim imports (:5-9). **Keep `ReliableChannel` (:183) and `InputChannel` (:190).**
* `shared/src/protocol/messages.rs` — delete `ShootRequest`(206), `HitConfirm`(217), `DamageReceived`(234),
  `PlayerKilled`(245), `SwitchWeapon`(256), `ReloadRequest`(263), `MeleeAttackRequest`(267),
  `BulletImpactSurface`(321), `BulletImpact`(330), `AudioEventKind`(342), `AudioEvent`(356); delete
  `use crate::weapons::damage::{HitBodyPart, HitZone};`(8); `PlayerInput`: remove `block`(32),
  its `Default`(103), `FLAG_BLOCK`(55) — **leave bit 11 vacant, do not renumber (C12)** — and the
  `Serialize`(141-143)/`Deserialize`(195) lines; delete test
  `hit_confirmation_preserves_precise_body_part`(716); strip `block: true` from tests at 591 and 627.
* `shared/src/player_profile.rs` — drop `use crate::weapons::WeaponType;`(10), fields
  `equipped_weapon`(39) + `weapon_ammo_in_mag`(41); rewrite `new_player`(98-195) removing the weapon
  slots 0-2 (104-121) and ammo slots 3-7 (124-150) and defaults (164-165). **Keep** `health_current`/
  `health_max`/`is_dead`/`death_timestamp` for now (C2) — P5 decides their fate.
* `shared/src/items/types.rs:4,19,33,46-55` — drop `use crate::weapons::WeaponType;`, the
  `ItemType::Weapon(WeaponType)` variant and its arms in `max_stack_size` / `display_name`.
* `shared/src/items/inventory.rs:4,28-46` — `with_starting_items` builds AssaultRifle/Sword/Shield.
* `shared/src/items/world.rs:4,35` — `GroundItem::new_weapon`.
* `shared/src/npc.rs:148,221` — delete `use crate::weapons::damage::HitBodyPart;` and
  `pub fn humanoid_body_part`. **KEEP** `humanoid_body_shape`(168), `humanoid_body_bounding_radius`,
  `HUMANOID_RAGDOLL_BODIES`(239), `npc_max_health`(74) — load-bearing for `server/src/ai/ragdoll.rs`.

**server/**
* `server/src/main.rs:9-10` — remove `#[path="combat/mod.rs"] mod combat;`.
* `server/src/app/resources.rs:11,45,46` — drop `use crate::combat;` and the two `init_resource`
  (`HittableSpatialIndex`, `BulletWorldHitCache`).
* `server/src/app/schedule.rs:15,57,75,241-261` — drop `use crate::combat;`, the `FpsServerSet::Combat`
  variant + its `.chain()` entry, and the whole Combat `add_systems` block (13 systems). Also
  `:230` remove `inventory::hotbar::sync_equipped_weapon_from_hotbar`. *Telemetry anchors already fixed
  in P0-6.*
* `server/src/telemetry/perf.rs` — drop `Bullet` from the import (:4); remove `Phase::{Weapons, BulletHits,
  WorldHits}` (:17-43) and set `Phase::COUNT` 8→5; delete `record_bullet_hits_ms`(161),
  `record_world_hits_ms`(171), `handle_perf_weapons_phase_begin`(231)/`_end`(237); drop
  `bullets: Query<(), With<Bullet>>`(247), the three stat computations (276-287) and their format args
  (330-361).
* `server/src/player/spawn.rs` — drop `EquippedWeapon`(11), `use shared::weapons::WeaponType;`(24);
  remove `EquippedWeapon` + `weapon_ammo` from the 9-tuple type (101-119) and all four construction arms
  (132-133, 153-157, 185-189, 212-216) plus the `equipped_weapon_component` block (~228).
* `server/src/net/connection.rs` — imports (18-23) drop `BulletImpact, DamageReceived, HitConfirm,
  PlayerKilled, ReloadRequest, ShootRequest, SwitchWeapon`; `:9` drop `EquippedWeapon`;
  `:85-87` remove the three `MessageReceiver`; `:117-120` remove the four `MessageSender`;
  `:144-145` remove `&EquippedWeapon` from the disconnect query + destructuring; drop
  `equipped_weapon`/`weapon_ammo_in_mag` from the profile write. **Keep `&Health`.**
* `server/src/persistence/autosave.rs:5,31-32,~103-108` — same shape; drop `EquippedWeapon`, keep `Health`.
* `server/src/inventory/hotbar.rs:6,11,71-152` — delete `sync_equipped_weapon_from_hotbar` entirely and
  `PreviousHotbarSlot`(17) if unused.
* `server/src/inventory/chest.rs:32` — `use shared::weapons::WeaponType;` inside `#[cfg(test)]`.
* `server/src/ai/tick/mod.rs:7,46-81` — delete `handle_npc_damage_events` (its type `NpcDamageEvent` is
  `HitZone`/`HitBodyPart`-typed and cannot outlive `shared/weapons`) and its registration at
  `schedule.rs:144`. **Consequence: NPCs lose flee-on-damage now, in P1, not P4.**
* `server/src/player/lifecycle.rs` — leave wired but inert (nothing writes damage into `Health`). P5
  deletes it. Note `is_player_alive` now always returns `true`.

**client/**
* `client/src/main.rs:8,21,22` — remove `mod crosshair; mod weapon_view; mod weapons;`.
* `client/src/render/mod.rs:4` — remove `pub mod sniper_fisheye;` (**keep `pub mod lod;`**, C4).
* `client/src/app_wiring/mod.rs:31,34-35` — repoint `WeaponDebugMode`→`DebugGizmoMode`; drop `crosshair,
  weapon_view, weapons`.
* `client/src/app_wiring/resources.rs` — delete `init_resource` at :10-15, :18-23 (12 resources).
  The four perf ones (:8,:9,:16,:17) were repointed in P0-1.
* `client/src/app_wiring/plugins.rs:113` — remove `SniperFisheyePlugin`.
* `client/src/app_wiring/systems.rs` — Startup 158-160; OnEnter 171-175; OnExit 183-189; `:221`
  `update_death_state`; `:238` `update_sniper_fisheye`; delete blocks 280-289 (crosshair), 291-308
  (weapons), 310-318 (bullet visuals), 320-332 (FX + `handle_hit_confirms`), 344-347
  (`update_trajectory_debug_gizmos`), 372-397 (weapon_view). The rescued perf systems at
  174/189/297/336/341/351/360/369 already point at `perf_overlay::`.
* `client/src/camera.rs` — delete `update_sniper_fisheye`(321-345); in `update_camera_fov`(274) delete
  the `EquippedWeapon` query param(279) and the `WeaponType::Sniper` branch(294-303); drop the
  `input_state.aiming` read at :192. `FOV_ADS`/`FOV_SNIPER_ADS`/`SNIPER_FISHEYE_*` become dead.
* `client/src/input.rs` — remove `InputState` fields `blocking_held`(44), `aiming`(59), `is_dead`(62) +
  defaults (90,96,97); in `handle_mouse_input`(167) delete the `EquippedWeapon` query(169), the
  can_block/ADS block(180-197), the aiming-sensitivity branch(206); **delete `update_death_state`
  (266-284) whole**; in `handle_send_input_to_server` drop `block:`(347) and `|| input_state.is_dead`(351).
  Cascade: drop the `|| input_state.is_dead` clause at `client/src/chest.rs:108` and
  `client/src/pickup/prompts.rs:16,123` (both P2 files, must compile now).
* `client/src/render/systems/connection.rs:99-102,134-137` — remove the four weapon `MessageSender` and
  four combat `MessageReceiver`.
* `client/src/render/systems/rendering/setup.rs:122` — remove the `SniperFisheye::default()` insert
  (**KEEP-list camera bootstrap**).
* `client/src/audio/mod.rs:18,31,66-69,104` — drop the `AudioEvent`/`AudioEventKind` import,
  `handle_remote_audio_events` from `pub use` + its `add_systems`, and the
  `.after(handle_remote_audio_events)` ordering on `apply_audio_limits` (**this last one is a compile
  error if you forget it**).
* `client/src/audio/remote_players.rs:11-135` — delete `handle_remote_audio_events`. Keep
  `ensure_remote_footstep_emitters`(143) + `update_remote_footstep_emitters`(247).
* `client/src/audio/state.rs:10-13,49,57,85,96` — drop the four gunshot `GameAudio` fields,
  `RemoteSpatialSound`, `AudioPriority::CombatRemote`, `AudioManager::max_remote_combat`.
* `client/src/audio/paths.rs:4-7` — delete the four gunshot consts.
* **`client/src/audio/assets.rs` — DANGER-5.** `setup_audio` drops the four gunshot loads (9-12) and
  `GameAudio` fields (29-32), **and `ensure_audio_assets_loaded` (43-98) must be rewritten** — it
  currently gates `AudioState::assets_ready` on a 5-tuple that includes all four gunshot `.ogg`s. Reduce
  it to the ambient handle only, or **all** client audio (ambient, footsteps, vehicles — all KEEP)
  goes permanently silent with **no compile error**.
* `client/src/props/debug.rs:5,147` — already repointed in P0-2; verify.
* `client/src/render/systems/npc/debug.rs:29,48-54,119` — the `HitZone::{Head,Chest,Stomach,Arms,Legs}`
  colour match dies. Replace with a `RagdollBodyId` palette or drop the colouring.
* `client/src/render/systems/npc/spawn.rs:136,192` — `use shared::weapons::damage::HitZone;` and
  `humanoid_body_part(def.id).hit_zone()` in the CombatDummy builder. Same fix.
* `client/src/pickup/{mod.rs:36, visuals.rs:8,22-31}` — `WeaponModelAssets` for ground weapon items;
  stub the `ItemType::Weapon` branch to early-return (P2 deletes the module).
* `client/src/ui/inventory/{mod.rs:30, layout.rs:7,22,26,30,34}` — remove the weapon icon rows.

#### 1d. Compile checkpoint
```
cargo check -p shared && cargo check -p server && cargo check -p client
cargo check -p editor          # must be a no-op; editor has ZERO weapon references
cargo check --workspace --all-targets
cargo test -p shared           # PlayerInput roundtrip tests must still pass
```

#### 1e. Smoke test
Fresh `server_data/`. Server + client with `FISTFORCE_AUTOCONNECT`. Confirm: `Name accepted!` ·
`Spawned client world visuals` · `ClientPerf frame_ms_p50=` · **audio is NOT silent** (walk → footsteps;
this is the D5 regression check) · F3 overlay · F4 prop gizmos · no `ERROR bevy_asset`.

#### 1f. Assets (separate commit)
`client/assets/game_assets/weapons/` (7 `.glb`, 3.0 MB — 3 already orphaned);
`client/assets/audio/sfx/{assault_shot, revolver_shot, shutgun_shot, sniper_shot, out_of_ammo,
gun_reload, assualt_rifle_reload, revolver_reload, shotgun_reload, sniper_reload}.ogg` (~230 KB).
**Consider KEEPING `client/assets/VFX/kenney_smoke-particles/`** (5.9 MB) — a tactics game wants smoke
and dust puffs, and re-sourcing a Kenney pack is annoying. It has no code reference after P1 either way.

---

### PHASE 2 — Items & inventory  (~3,700 lines)

#### 2a. Prep
Bump `PROFILE_VERSION` `2 → 3`. Confirm `server_data/players` is still moved aside.

#### 2b. Delete
```
shared/src/items/            (mod, constants, types, inventory, world, messages)     705
server/src/inventory/        (mod, chest, ground_items, hotbar)                      ~670 (death_drop went in P1)
client/src/ui/inventory/     (mod, chest, drag_drop, layout, slots)                 1185
client/src/pickup/           (mod, assets, items, prompts, visuals)                  547
client/src/chest.rs                                                                  315
client/assets/game_assets/items/          (5 .glb, 424 KB)
client/assets/ui/item_preview/            (9 .png, ~128 KB — dir becomes empty)
```

#### 2c. Edit
* **`shared/src/building/defs.rs` — the KEEP-list breakage.** Drop `use crate::items::ItemType;`(4),
  the field `pub cost: &'static [(ItemType, u32)]`(224) + its doc(223), and the `cost:` initialiser from
  all six `BuildingDef` literals (118, 128, 138, 148, 158, 209). **Verified in the tree: `rg '\.cost\b'`
  returns zero readers repo-wide** — it is dead data. This edge also transitively breaks
  `tools/collider_baker`, which is a workspace member.
* `shared/src/lib.rs:6` — remove `pub mod items;`.
* `shared/src/player_profile.rs` — drop `use crate::items::{…}`(7), fields `inventory_slots`(45) +
  `hotbar_selection`(47), the starting-inventory block in `new_player`(98-151), and the two struct-literal
  entries (168-169). Optionally drop the orphan `bank_gold`(87) in the same commit.
* `shared/src/protocol/plugin.rs` — delete the `use crate::items::{…}` block (11-14);
  `register_component::<Inventory>`(74), `<GroundItem>`(75), `<GroundItemPosition>`(76),
  `<HotbarSelection>`(83), `<ChestStorage>`(86), `<ChestPosition>`(87); the 7
  `register_message` blocks (127-140).
* `server/src/main.rs:12` — `mod inventory;`.
* `server/src/app/resources.rs:12,30` — `use crate::inventory;` + `OpenChests`.
* `server/src/app/schedule.rs:16,56,74,222-239` — `use crate::inventory;`, the `FpsServerSet::Inventory`
  variant + chain entry, and the 8-system Inventory block.
* `server/src/app/bootstrap.rs:104` — remove `crate::inventory::chest::spawn_world_chests,` and
  **de-tuple** the remaining `spawn_world_vehicles` (a 1-element `add_systems` tuple will not type-check).
* `server/src/net/connection.rs:12-15,107-113,146-147,193-194,208-209,225-226,286-287` — the items import
  block, the 7-`MessageReceiver` insert statement, `&Inventory`/`&HotbarSelection` in the disconnect
  query + the three destructurings, and the two profile-literal fields.
* `server/src/persistence/autosave.rs:8,33-34,58-59,110-111` — same shape.
* `server/src/player/spawn.rs:14,26,99-119,135,142-146,174-178,201-205,246-250` — imports, the 9-tuple
  type, the three identical `Inventory::new()` restore loops, and the three bundle components.
* `server/src/telemetry/perf.rs:5,248,331,359` — `GroundItem` import, the `ground_items` query param,
  and the `ground_items={}` format slot + arg (**must go together or the format arity breaks**).
* `client/src/main.rs:6,11` — `mod chest; mod pickup;`.
* `client/src/app_wiring/mod.rs:34` — drop `chest, pickup`.
* `client/src/app_wiring/plugins.rs:124-130` — remove `ui::InventoryPlugin`, `pickup::PickupPlugin`,
  `chest::ChestPlugin`.
* `client/src/ui/mod.rs:4,13` — `pub mod inventory;` + the `InventoryPlugin` re-export.
* `client/src/render/systems/connection.rs:106-109,116-120` — the 4 item `MessageSender` and the whole
  second chest-sender `insert(…)` statement.
* `client/src/input.rs` — **KEEP `InputState::inventory_open`** for now (D5 in the P2 manifest): its only
  writers live in deleted files, and three KEEP-list UI files read it
  (`ui/world_map/layout.rs:15`, `ui/pause_menu/actions.rs:461`, `ui/debug_time_menu/actions.rs:22`) plus
  `ui_blocking()` at `:110`. Sweep it in P6.

#### 2d. Compile checkpoint
```
cargo check -p shared
cargo check -p server && cargo check -p client
cargo check -p editor                  # VERIFIED CLEAN: zero item references in editor/src
cargo build --workspace                # ← catches tools/collider_baker via shared::building
cargo test -p shared
```

#### 2e. Smoke test
Server + client, fresh profile: name entry → spawn → roster shows the name → disconnect → reconnect and
confirm the profile round-trips (level/prestige preserved). World map opens. Pause menu opens.

---

### PHASE 3 — Vehicles & rail  (~6,000 lines)

#### 3a. Prep
Bump `PROFILE_VERSION` `3 → 4`. **Confirm P0-4 (RTS camera harvest) and P0-5 (streaming re-anchor) are
already merged** — `client/src/rail/mod.rs` is deleted in this phase and `client/src/streaming.rs` is on
the KEEP list.

**DANGER — two `SubmitPlayerName` handlers exist.** `server/src/rail/mod.rs:158
handle_company_name_submission` and `server/src/player/spawn.rs:~55 handle_player_name_submission` both
consume `MessageReceiver<SubmitPlayerName>` and both reply `NameSubmissionResult`; they are wired mutually
exclusively via `FISTFORCE_RAIL`. **Delete only the rail one.** Deleting both locks every client out at
the name-entry screen — and the rail one looks "newer" because rail was the most recent work.

#### 3b. Delete
```
shared/src/vehicle/       (mod, components, tuning, physics/{mod,bike,car,car_v2,common,interaction}) 1518
shared/src/rail.rs                                                                                    207
shared/src/economy.rs     (CargoKind/IndustryKind/EconomyInventory — rail-only, verified)              125
server/src/vehicle/       (mod, bootstrap, interaction, simulation)                                    253
server/src/rail/          (mod.rs)                                                                     851
server/src/collision/resolve_vehicle.rs                                                                 92
client/src/rail/          (mod.rs — ONLY after P0-4)                                                   737
client/src/render/systems/vehicle/  (mod, angles, assets, hover, spawn, steam_car, sync, visibility)   899
client/src/audio/vehicles.rs                                                                           364
```
Assets: `client/assets/game_assets/vehicles/` (4.3 MB), `trains/train.glb` (1.4 MB),
`buildings/train/train_station_lvl_1.glb` (1.0 MB, dir becomes empty),
`audio/sfx/{hover_idle_loop, hover_idle_loop_old, bike_cruise_loop}.ogg` (~106 KB).

#### 3c. Edit — key symbols
* `shared/src/lib.rs:5,15,19` — `pub mod economy; pub mod rail; pub mod vehicle;`.
* `shared/src/protocol/plugin.rs` — imports 16-19 + 21; delete `register_component` for `Vehicle`(56),
  `VehicleState`(57), `VehicleDriver`(58) and the nine rail components (94-103); delete the seven rail
  `register_message` (145-157) + `RailCommandRejected`(163). **No channel is rail/vehicle-specific.**
* `shared/src/protocol/messages.rs` — imports 5,6,7; `PlayerInput.vehicle_input`(28);
  `PackedPlayerInput.{throttle_q,brake_q,steer_q}`(36-42); `FLAG_HAS_VEHICLE_INPUT`(53) +
  `FLAG_VEHICLE_AIR_CONTROL`(54) — **leave bits 8/9/10 vacant (C12)**; the four now-dead quantizers
  `quantize_unit_u8`/`dequantize_unit_u8`/`quantize_signed_i8`/`dequantize_signed_i8`(58-75);
  `Default`(101); `Serialize`(145-157); `Deserialize`(176,188-193); the 8 rail message structs
  (421-475); tests at 602/619 and the whole `player_input_roundtrip_vehicle_…` test (626-657).
* `shared/src/player_profile.rs:10,49-60,168-173` — drop `VehicleType` and the six vehicle fields.
* `shared/src/items/constants.rs:14` — `VEHICLE_INTERACTION_RANGE` (already gone if P2 ran; it did).
* `server/src/main.rs:22,26`; `server/src/app/resources.rs:17,26`; `server/src/app/bootstrap.rs:18,93-108`
  (collapse the `if rail_mode_enabled()` branch entirely).
* `server/src/app/schedule.rs` — `use crate::{rail, vehicle}` (:21,:23); **delete `rail_mode_enabled()`
  (28-33)** and the `FISTFORCE_RAIL` branch in `configure_fixed_schedule`(35-42);
  `FpsServerSet::VehicleSim` variant(50) + chain(68) + block(127-138); **`enum RailServerSet` +
  `configure_rail_fixed_schedule` in full (296-382)**.
* `server/src/net/connection.rs:17-24,25,96-104,125,149,152,243-274,288-293,334-338` — rail message
  imports, the vehicle import, the 7-receiver rail tuple, the `RailCommandRejected` sender, the
  `Option<&InVehicle>` + `vehicles` query, the `vehicle_data` block, 5 profile fields, the driver-clearing
  loop.
* `server/src/net/input.rs:15,79-80` — delete `ClientInputs.latest_by_driver_id` (its only reader was
  `server/src/vehicle/simulation.rs:32`) plus both removers at `connection.rs:239,345`.
* `server/src/persistence/autosave.rs:10,36,39,61,69-98,112-117`.
* `server/src/player/spawn.rs:23,33,110,119,136,156,161-191,190,260-291` — **KEEP
  `handle_player_name_submission` and its `NameSubmissionResult` sends (:77,:83,:298)**.
* `server/src/player/{lifecycle.rs:9,51, movement.rs:12,32,36,51,70-73,94}`,
  `server/src/collision/resolve_player.rs:9,35,51,52` (all unwired or P5-bound, but must compile).
* `server/src/physics/dynamic_actors.rs:26,338,342,351,354-357,396,421,437`.
* **`server/src/physics/terrain_colliders.rs:9,124,134-137,160,165`** — KEEP-list file. Remove the
  vehicle fallback from `gather_centers`. Anchor chain becomes players → NPCs.
  **Flag for P5: after P4 removes NPCs, players are the only anchor left.**
* `server/src/collision/geometry.rs:28-115,332-362` — delete `handle_vehicle_vs_static` and
  `handle_vehicle_proxy_vs_corpse_spheres`. **KEEP `handle_capsule_vs_static`(116) and
  `handle_capsule_vs_corpse_spheres`(271)** — used by `resolve_player`/`resolve_npc`.
* `server/src/collision/mod.rs:18` — `pub mod resolve_vehicle;`.
* `client/src/main.rs:14`; `client/src/app_wiring/{mod.rs:34, dev.rs:5-9 (delete `rail_mode_enabled`),
  plugins.rs:7,15-19,124-133, resources.rs:35, systems.rs:2-6,10-17,100-147 (delete `wire_rail_systems`),
  157,207,219,224,226,227,234,235,236,239}`.
  **When unwrapping the `if !rail_mode { … }` block at `plugins.rs:124-133`, hoist
  `ui::WorldMapPlugin` and `audio::GameAudioPlugin` out — both are KEEP and are currently registered
  ONLY inside that branch.** Deleting the branch wholesale silently removes the world map and all audio,
  with no compile error.
* `client/src/streaming.rs` — already re-anchored in P0-5; just confirm no `crate::rail` reference remains.
* `client/src/render/systems/{mod.rs:11,21, connection.rs:63,91,122-130,143}`.
* `client/src/render/systems/particles.rs:7,72,75-163` — delete `spawn_sand_particles` only (C5).
* `client/src/input.rs:11,52-56,93-95,184,190,194-196,212-221,240-264,345,361-383` — `InputState`
  vehicle fields + `update_vehicle_state`.
* `client/src/camera.rs:6,11,16-18,50-51,54,75-123,126-145,181,197-214,238-245` — **keep
  `peer_id_to_u64`**.
* `client/src/render/systems/player/{mod.rs:36,40, sync.rs:66-70,79,81-82,105-127,132-152,
  animation.rs:83-85,192,198-203,293-301}` + drop the now-unproduced `MovementAnim::Driving` variant.
* `client/src/audio/{mod.rs:9,21-24,33,53,81-98,105, state.rs:16-17,24-38,122-133,140-143,
  assets.rs:15-17,24-25,34-35,39, paths.rs:9-10, ambient.rs:51,75,82-83,96-99,
  remote_players.rs:155-156,176-178}`.
* `README.md:11,43,101,104,289`; `.claude/skills/verify/SKILL.md:24,42` (the `FISTFORCE_RAIL=1` note —
  **the flag ceases to exist in this phase**).

#### 3d. Compile checkpoint
```
cargo check --workspace --all-targets
cargo check -p editor        # VERIFIED CLEAN: zero rail/vehicle code; only 2 stale comments in worldgen.rs
cargo test -p shared         # PlayerInput roundtrip: the vehicle test is deleted, the on-foot one must pass
```

#### 3e. Smoke test
**The critical one for this phase: terrain and prop streaming.** Boot client, walk/pan around, confirm
chunks load and props appear (the `streaming.rs` anchor is the P0-5 change under real load). Also:
world map opens (it was inside the deleted `if !rail_mode` block), audio still plays, name entry works
(the two-handler trap), server terrain colliders still stream (`CITYSIM_TERRAIN_COLLIDER_*` logs).

---

### PHASE 4 — NPCs & AI  (~6,450 lines)

#### 4a. Prep — salvage before deleting
1. **`sync_obstacle_grid`** (`server/src/ai/obstacles.rs:16-47`) + `ObstacleGridState`(9-12) → move to
   `server/src/world/navgrid.rs`. Zero NPC types in the body: it walks `BuildingSpatialIndex` (KEEP),
   expands each building footprint by `flatten_radius`, and fills `shared::spatial::SpatialObstacleGrid`
   (KEEP list). Without it, `SpatialObstacleGrid` becomes a permanently-empty resource and
   "buildings block navigation" is silently lost.
2. **A\* pathfinding** (`server/src/ai/pathfinding.rs`): keep `find_path_a_star_with_scratch`(136),
   `PathfindingScratch`(129), `GridPos`/`world_to_grid`/`grid_to_world`/`heuristic`/`OpenNode`(74-126),
   `PathfindingBudgetSettings`(17-33). Signature is
   `(&WorldTerrain, &SpatialObstacleGrid, Vec3, Vec3, &mut scratch) -> Vec<Vec3>` — **no NPC type
   anywhere**. Discard `pick_random_target`(35).
3. **`XorShift64`** (`server/src/ai/state.rs:77-103`) → move to `shared/`. 6 lines, deterministic,
   matters more if the unit sim later goes lockstep.
4. **Copy out** (as comments/docs, not code): the AI LOD cadence bands from `ai/tick/mod.rs` +
   `ai/state.rs:8-15`, and the replication-relevance hysteresis from `ai/relevance.rs`.
5. **Decision C7 applies here** — the DebugPhysicsBox feature is deleted, not extracted.

#### 4b. Delete
```
server/src/ai/                 (death_cleanup, identity, mod, obstacles, pathfinding, ragdoll,
                                relevance, spawn, state, tick/{mod,state_steps})              ~2,844
server/src/collision/resolve_npc.rs   (already dead code — nothing registers it)                  94
shared/src/npc.rs                                                                                334
client/src/dialogue.rs                                                                           380
client/src/render/systems/npc/ (animation, assets, debug, mod, ragdoll, spawn, state, sync,
                                visibility)                                                    2,177
client/src/render/systems/debug_physics.rs   (C7)
client/assets/audio/dialogue/  (184 KB — peasant/ + the already-orphaned king/)
```

#### 4c. Edit
* `shared/src/lib.rs:8` — `pub mod npc;`.
* `shared/src/components/actors.rs` — delete `NpcArchetype`(26-39), `Npc`(49-54),
  `NpcActivityKind`(56-72), `NpcActivity`(74-76), `NpcIdentity`(78-84), `NpcPosition`(86-88),
  `NpcRotation`(90-92), `NpcVelocity`(94-96), `NpcFleeing`(98-100), and — per C7 —
  `DebugPhysicsBox`/`DebugPhysicsBoxPosition`/`DebugPhysicsBoxRotation`(102-115).
  **KEEP** `Player`, `PlayerProgression`, `PlayerCharacter` (note its unrelated `Oilman` variant),
  all `Player*` state, `Ground`, `LocalPlayer`.
* **`shared/src/map/schema.rs`** (KEEP module) — `use crate::components::NpcArchetype;`(5);
  `MapDefinition.npc_groups`(19-20); the validation loop(58-82); `struct MapNpcGroup` + impl(230-267);
  `enum MapBehaviorPreset`(269-274); test `npc_group_trims_authored_metadata`(501-516).
* **`shared/src/map/editor_schema.rs:248` — DO NOT TOUCH `SpawnMarkerKind::NpcGroup` (C9).**
* `shared/src/protocol/messages.rs` — `NpcArchetype` from the import(4); `SpawnOilmanDebug`(303-309);
  `SpawnPhysicsBoxDebug`(314, per C7); `RagdollBodyId`(477-497); `PackedQuatI16` +
  `quantize_quat_component`/`dequantize_quat_component`/`pack_quat_i16`/`unpack_quat_i16`(499-540);
  `RagdollBodyPose`(541-547); `NpcRagdollStarted`(549-556); `NpcRagdollPoseSample`(560-568);
  `NpcRagdollPoseBatch`(570-575); `RagdollPoseChannel`(583-584); tests
  `npc_ragdoll_messages_roundtrip`(669-712) and `packed_quat_roundtrip…`(659).
* `shared/src/protocol/plugin.rs` — the NPC component block(42-48), the three `DebugPhysicsBox*`
  registrations(49-53, per C7), `NpcIdentity`(79-80), `register_message::<SpawnOilmanDebug>`(123-124)
  and `<SpawnPhysicsBoxDebug>`(125-126), `<NpcRagdollStarted>`/`<NpcRagdollPoseBatch>`(177-180),
  `add_channel::<RagdollPoseChannel>`(197-201).
* `server/src/main.rs:2`; `server/src/app/resources.rs:9,32-38,44`;
  `server/src/app/bootstrap.rs:99` (+ the now-unused `use crate::ai;`).
* `server/src/app/schedule.rs` — `use crate::ai;`(13); PhysicsWorld 100-104 (3 dynamic_actors NPC
  systems); NetIngress 117 (`handle_spawn_oilman_debug`) **and** 118 (`handle_spawn_physics_box_debug`,
  per C7); the whole `AISim` block(140-155) + variant(50) + chain entry(68); PhysicsControl 161
  (`apply_npc_controls_from_ai`); PhysicsPost 175,177-180; Indices 205
  (`update_npc_network_visibility`). *Telemetry anchors were already set-based from P0-6.*
* `server/src/physics/dynamic_actors.rs:10-11,14,29,35,42-43,49,52-78,224-304,306-315,688-699,701-782,
  784-806,~905` — all NPC physics + `NpcPhysicsLodSettings` + the `sync_debug_boxes_from_physics` writeback
  (C7). **KEEP** `ensure_player_physics_bodies`, `apply_player_controls`, `sync_players_from_physics`,
  `clamp_players_to_map_bounds`, `tick_player_jump_timers` (all die in P5).
* **`server/src/physics/terrain_colliders.rs:7,125,140,161,165`** (KEEP) — drop the NPC streaming-center
  loop. **After this the only anchor is `PlayerPosition` — P5 must not break it (DANGER-4).**
* `server/src/physics/layers.rs:8,10,51,53,64-77,70,72,84,86,92-105,98,100,107-120,113,127,129,139` —
  `GROUP_NPC` (consider renaming to `GROUP_UNIT` rather than deleting), `GROUP_RAGDOLL`, `npc_groups()`,
  `ragdoll_groups()`, `ragdoll_no_self_groups()`, and their appearances in surviving masks.
* **`server/src/collision/geometry.rs:7,271-330,332-350`** (KEEP geometry library) — drop the
  `crate::ai::ragdoll` import and both corpse fns. **KEEP `handle_capsule_vs_static`.**
* `server/src/collision/resolve_player.rs:12,15,26,42,87-94` — strip the corpse branch (P5 deletes the
  file, but it must compile now).
* `server/src/collision/mod.rs:16` + the doc at :6.
* `server/src/net/connection.rs:20,121-122` + the `SpawnOilmanDebug`/`SpawnPhysicsBoxDebug` receivers.
* `server/src/player/spatial.rs:113` — drop the `Without<shared::components::Npc>` filter.
* `server/src/telemetry/network.rs:9,12,44-45,48-49,81-82,131-132,142-147,159,202-229,272-292`.
* `server/src/telemetry/perf.rs:4,10,19,22-23,34,37-38,141-156,219-228,246,249,271-273,278-282,331-360` —
  drop `Phase::{NpcInventoryBuild, AiCadence, Pathfinding}`, `record_ai_cadence_ms`,
  `record_pathfinding_ms`, `RagdollTelemetry`, the `npcs` query, and rewrite the `ServerPerf` format.
* `client/src/main.rs:9`; `client/src/app_wiring/{mod.rs:34, plugins.rs:125,132,
  systems.rs:162,206,229,258-277}`.
* `client/src/render/systems/mod.rs:7,17` (+ the `debug_physics` mod/re-export per C7).
* `client/src/render/systems/connection.rs:21,104,138-139,226,247-249`.
* `client/src/audio/mod.rs:30` (`Npc` — verified already an unused import);
  `client/src/audio/state.rs:4,56-59,69-75,83,89,96,99` (**`AudioPriority` has explicit discriminants —
  removing `Dialogue = 2` changes the numeric ordering used by `client/src/audio/limits.rs`; renumber
  deliberately**); `client/src/audio/remote_players.rs:137,154,199-207`.
* `client/src/ui/debug_time_menu/{mod.rs:23,169,171-173, layout.rs:134,141,153-154,298-336,
  actions.rs:66-67,85-86,102-103,162-180}` + the physics-box button (C7). **KEEP `actions.rs:142-143`
  (`PlayerCharacter::Oilman`) — that is the player model, not an NPC.**
* **`editor/src/tools.rs:564`** — `session.map_definition.npc_groups.clear();` — **required** or the
  KEEP-list editor crate will not compile.
* `editor/src/ui.rs:350` — confirm-dialog wording mentions "NPC groups" (cosmetic).
* **Leave alone** (C9): `editor/src/{session.rs:185, ui.rs:926-927,1186, tools.rs:1895,1923,
  worldgen.rs:1361}` — all reference the surviving `SpawnMarkerKind::NpcGroup` variant.
* `README.md:9,101,104,109,126-127,134,310`; `.claude/skills/verify/SKILL.md:17-18`
  (`CITYSIM_MAX_NPCS=8` is in the standard launch recipe and stops existing in this phase).

#### 4d. Compile checkpoint
```
cargo check --workspace --all-targets
cargo check -p editor          # ← the tools.rs:564 edit is a hard requirement
cargo test --workspace
```

#### 4e. Smoke test
**Run the editor for real: `./run.sh editor`, open `city_alpha`, place a spawn marker, save, reload.**
The editor is the only thing that round-trips `edits.ron`, and a serde regression there is invisible at
compile time (C9). Then server + client: name entry → spawn → terrain/props stream → world map → audio.

---

### PHASE 5 — FPS embodiment → commander  (~5,300 lines)

#### 5a. Prep
Bump `PROFILE_VERSION` `4 → 5`. **Confirm P3 and P4 are merged** (§1.2). Copy the animation-graph index
table out of `client/src/render/systems/player/assets.rs:14-110` into a comment or the future unit module
before deleting it — the `.glb`s are KEEP but **only that file knows which `#AnimationN` is which clip**
(Oilman: 0 tpose, 1 idle, 2 jog-fwd, 3 jog-back, 4 strafe-R, 5 strafe-L, 6 run, 7 jump, 8 driving,
9 look-behind-run; basemodel: 0 run, 1 walk, 2 fall).

**Per C1 there is no `CommanderView` to add.** `PlayerPosition` becomes the commander's view focus and
`PlayerRotation` its yaw; the commander camera writes them, the server replicates them, and every KEEP
consumer (streaming anchors, world-map marker, profile writers) keeps working unchanged.

#### 5b. Delete
```
client/src/render/systems/player/  (mod, animation, assets, spawn, sync, visibility, ids)   1,391
client/src/audio/ambient.rs        (footstep ambience keyed off InputState + PlayerPosition)
server/src/player/movement.rs                                                                 161
server/src/player/lifecycle.rs                                                                 91
server/src/player/spatial.rs                                                                  130
server/src/physics/dynamic_actors.rs                                                          ~700 (post-P4)
server/src/physics/contacts.rs                                                                 59
server/src/collision/resolve_player.rs                                                        116
server/src/collision/geometry.rs                                                              533
server/src/collision/building_geometry/  (mod, shapes)                                        543
shared/src/physics/character.rs                                                               190
client/assets/audio/ambient/walking_desert.ogg  (48 KB)
```
> `server/src/collision/raycast.rs` (rescued in P1, C8) is **NOT** part of `geometry.rs` — verify it
> survives.

#### 5c. Edit
* `client/src/camera.rs` — 347 → ~110. **Must survive:** `CAMERA_NEAR_CLIP`(15, read by
  `render/systems/rendering/setup.rs:99`) and `peer_id_to_u64`(36-43, read by `audio/mod.rs:37`).
  **Port the more complete `peer_id_to_u64` from `render/systems/player/ids.rs:6`** — verified in the
  tree: it handles `PeerId::Entity` and `PeerId::Raw` (hashed), whereas the `camera.rs` copy maps both to
  `0`, a live hash-collision bug. Delete everything else (`update_camera`, `first_person_target`,
  `third_person_target`, `orbit_position`, `look_at_level`, `update_camera_fov`, the FOV/THIRD_PERSON
  consts). Attach the `CommanderCamera` from P0-4 at
  `client/src/render/systems/rendering/setup.rs:55` (the sole `Camera3d`).
* `client/src/input.rs` — 416 → ~120. **Do NOT delete `InputState`** — it is the UI modal mutex.
  Keep `inventory_open` (or drop it here and fix the three readers), `pause_menu_open`, `map_open`,
  `debug_menu_open`, and `ui_blocking()`(109-111). Delete `CameraMode`(21-26),
  `handle_keyboard_input`(115-161), `handle_mouse_input`(164-228), the local `peer_id_to_u64`(231-238),
  `handle_send_input_to_server`(287-416), `INPUT_HEARTBEAT_SECS`/`INPUT_CHANGE_BURST_TICKS`(17-18).
  Keep `crate::render::systems::InputSettings` (`rendering/settings.rs:183-195`) — the pause menu exposes
  `mouse_sensitivity` and the new camera should honour it.
* `client/src/render/systems/mod.rs:9,19` — `mod player;` + `pub use player::*;`.
* `client/src/app_wiring/systems.rs:161,194-198,204-208,218-221,228,231,237-238,246-256` +
  `resources.rs:34` (`game_systems::LastCameraMode` — the type lives in the deleted dir).
* `client/src/render/systems/rendering/setup.rs:122` — already removed in P1; confirm.
* `client/src/render/systems/connection.rs:192-213` — replace `apply_cursor_grab` with the harvested
  `release_cursor_for_rts`; an RTS wants a free cursor. **Keep `cleanup_enter_main_menu`** incl. the
  `Query<Entity, With<Player>>` despawn.
* `client/src/water/overlay.rs:47` (KEEP) — `Query<&PlayerWaterState, With<LocalPlayer>>` → drive from
  the existing camera query at `:48` vs `terrain.water_level()`.
* `client/src/water/material.rs:189-228` + `water/mod.rs:30,60` — delete `emit_water_ripples` and its
  registration (no waders exist). `update_water_cull_mode` already uses `Camera3d` and is fine.
* `client/src/ui/world_map/{markers.rs:5-48, mod.rs:26}` (KEEP) — `update_player_marker` keeps working
  **unchanged** under C1 (`PlayerPosition` + `PlayerRotation` survive). Verify the arrow points sensibly;
  the UI convention is documented at `markers.rs:27-40` (`angle = -yaw`).
* `client/src/ui/debug_time_menu/{state_sync.rs:22,32-46, actions.rs:74,118,183-186}` — the character
  selector and fly-toggle buttons.
* `shared/src/components/actors.rs` — delete `PlayerVelocity`(126-127), `PlayerGrounded`(131-139 + impl
  170-183), `PlayerWaterState`(142-148), `PlayerJumpState`(151-155), `PlayerMeleeState`(158-164),
  `FlyMode`(167-168). **KEEP `Player`, `PlayerPosition`, `PlayerRotation`, `PlayerProgression`,
  `LocalPlayer`, `Ground`** (C1).
* `shared/src/physics/mod.rs:3,6` + delete `character.rs`; `shared/src/physics/constants.rs` — keep
  `GRAVITY`(4, used by `server/src/app/mod.rs:14,24` for the Rapier world), delete the rest incl.
  `ground_clearance_center()`(47-50) — its P4 callers are gone by now.
* `shared/src/player.rs` — keep `MOUSE_SENSITIVITY`(22) and `SPAWN_POSITION`(25, referenced by
  `player_profile.rs:8,157`); delete `PLAYER_SPEED`, `PLAYER_SPRINT_MULT`, `PLAYER_HEIGHT`,
  `PLAYER_RADIUS`, `JUMP_ANIM_MIN_SECS`, `STEP_UP_HEIGHT`, `PLAYER_MAX_HEALTH`, `RESPAWN_TIME`.
* `shared/src/protocol/plugin.rs` — delete `register_component` for `PlayerVelocity`(33),
  `PlayerJumpState`(34), `PlayerWaterState`(36-37); `register_message::<SetPlayerCharacter>`(121-122)
  and `<SpawnPlayer>`(107-108, **verified dead: no sender, no receiver anywhere**).
  **Keep `Player`(30), `PlayerPosition`(31), `PlayerRotation`(32), `PlayerProgression`(38-39).**
* `shared/src/player_profile.rs` — strip `velocity`(31), `health_current`(35)/`health_max`(37),
  `is_dead`(65)/`death_timestamp`(67). **Keep `position`** (the commander's saved view focus) and
  `rotation`(29, the saved yaw). Keep name/level/prestige/reputation/stamina/intelligence/bank_gold/
  last_login/total_playtime_secs.
* `server/src/player/mod.rs:13,14,17`; `server/src/player/index.rs` — **no change** (`PlayerEntityIndex`
  is exactly the commander lookup); `roster.rs`/`roster_cache.rs` — **no change**.
* `server/src/player/spawn.rs` — 337 → ~130. Keep `handle_player_name_submission`(49-99, 293-299) and
  `resolve_map_spawn_position`(35-45, minus the `+ ground_clearance_center()` term). Delete the spawn
  bundle's movement components(240-247) and `handle_set_player_character`(305-337).
  **Keep verbatim:** `ReplicationGroup::new_from_entity().set_priority(PLAYER_REPLICATION_PRIORITY)`,
  `Replicate::new(...)`, `ControlledBy { owner, lifetime }` (251-256).
* **`server/src/physics/terrain_colliders.rs:122-165`** — `gather_centers` now walks
  `players → ChunkCoord(0,0)` only. **Verify at runtime (DANGER-4).**
* **`server/src/collision/streaming.rs:66,86-92,143-148`** — `Query<&PlayerPosition>` survives under C1;
  no change needed. Its test at 244-286 stays green.
* `server/src/physics/mod.rs:3,4`; `server/src/physics/layers.rs` — 141 → ~50, keep `GROUP_TERRAIN`,
  `GROUP_STATIC_WORLD`, `GROUP_BULLET_QUERY`→`GROUP_LOS_QUERY`, `terrain_groups()`,
  `static_world_groups()`, `bullet_world_query_groups()`→`los_query_groups()`; rewrite
  `world_query_mask()`(31, it composes the deleted `dynamic_actor_mask()`).
* **`server/src/physics/queries.rs`** — KEEP as the LOS API. All its callers died in P1, so add
  `#[allow(dead_code)]` **or** immediately land `pub fn has_line_of_sight(ctx, from, to) -> bool`.
  A `-D warnings` CI gate will fail otherwise.
* `server/src/collision/library.rs` — keep `StaticColliders`/`BakedColliderLibrary`/
  `StaticColliderInstance`/`setup_baked_colliders`. `DerivedColliderLibrary`(17-19),
  `DerivedBuildingColliderLibrary`(23-25), `HullFace`(29-33), `DerivedHull`(36-41),
  `DerivedCollider`(44-49), `derive_collider`(163-191), `build_hull_from_points`(134-161),
  `triangulate_convex_hull`(194-309) become orphaned (~200 lines) yet `setup_baked_colliders`(82-132)
  still builds them at every startup — **delete them** (preferred) or `#[allow(dead_code)]`.
* `server/src/collision/mod.rs:12,14,17` — `building_geometry`, `geometry`, `resolve_player`.
  **Keep `building_index`(13), `library`(15), `streaming`(19), `raycast`(new).**
* `server/src/app/schedule.rs` — PhysicsWorld 100-105; the whole `PhysicsControl` block(157-167) + set
  (50,70); `PhysicsPost`(169-186) becomes empty → drop the set(51,71); `PlayerSim`(188-198) → drop the
  set(52,72); `Indices` 204 (`sync_player_spatial_index`); keep 203 (`sync_player_entity_index`).
* `server/src/app/resources.rs:29,44`.
* `server/src/net/connection.rs:132-346` + `server/src/persistence/autosave.rs:22-155` — the two profile
  writers. Query becomes `(&Player, &PlayerPosition, &PlayerRotation, &PlayerProgression)`.
  **Fix the pre-existing bug while you are here:** `connection.rs:278` saves `player_name: name_lower`
  (lowercased) while `spawn.rs:97` creates profiles with the original case — display names get
  lowercased after the first disconnect.
* `shared/src/protocol/messages.rs` — `PlayerInput` loses `forward/backward/left/right/jump/fly_mode/
  fly_down/fly_fast/interact` and the matching flags (bits 0-8). **Decision:** reduce it to
  `{ yaw: f32, focus: Vec3 }` so the server can keep streaming colliders around a panning commander
  (recommended), rather than deleting it end-to-end. `server/src/net/input.rs` `ClientInputs` +
  `handle_client_input_messages` survive in that shape.
* **`shared/src/map/schema.rs:16,37-39` — DO NOT REMOVE `player_spawn: Option<[f32;3]>`.** The editor
  writes it (`worldgen.rs:833,835`, `tools.rs:471,562,801,943`) and 124 MB of saved maps carry it.
  Repurpose it as the commander's initial camera focus.

#### 5d. Compile checkpoint
```
cargo check --workspace --all-targets
cargo clippy --workspace -- -D warnings     # ← catches physics/queries.rs + library.rs orphans
cargo check -p editor
cargo test --workspace
```

#### 5e. Smoke test — the heaviest of the strip
1. Server boots; **terrain colliders stream around the commander camera** (grep the
   `CITYSIM_TERRAIN_COLLIDER_*` logs; DANGER-4 is silent otherwise).
2. Client: name entry → connect → **terrain chunks stream under a panning camera**, props + LOD stream,
   water renders, sky/day-night runs, world map opens with the camera marker, pause menu opens,
   debug time menu opens, F3 perf overlay reports frames.
3. Disconnect → reconnect: profile round-trips (name case correct, level/prestige preserved).
4. `./run.sh editor` — map loads and saves.

---

### PHASE 6 — Protocol slim, wiring, assets, naming  (~1,550 lines)

Everything here is "delete the registration whose type already died". P0 already took the three items
that had to happen early (`prelude.rs`, `streaming.rs`, telemetry anchors).

#### 6a. Protocol
1. **Bump `PROTOCOL_ID`** (`shared/src/protocol/config.rs:5`, currently `0x1234567890ABCDF4`) so a stale
   client fails the netcode handshake instead of connecting and desyncing on unknown net-ids.
2. `shared/src/protocol/plugin.rs` — 203 → ~60 lines. Surviving components (8 of 40):
   `Player`, `PlayerPosition`, `PlayerRotation`, `PlayerProgression`, `WorldTime`, `CloudSeed`,
   `ActiveMapState`, `TerrainDeltaChunk` (+ `Health`, per C2 → 9). Surviving messages (6 of 32):
   `PlayerInput`, `SetTimeOfDay`, `SubmitPlayerName`, `RequestPlayerRoster`, `NameSubmissionResult`,
   `PlayerRoster`. Surviving channels: `ReliableChannel`, `InputChannel`.
   Collapse the import block (4-21).
3. `shared/src/protocol/messages.rs` — 730 → ~200. One clean rewrite of `PlayerInput` +
   `PackedPlayerInput` + both manual serde impls (C12); delete the four dead quantizers if the final
   shape does not need them; rewrite the surviving roundtrip test.
4. `shared/src/lib.rs` — final sweep of `pub mod` lines.

#### 6b. Wiring
* `server/src/app/schedule.rs` — 382 → ~90. `FpsServerSet` shrinks 12 → 5 variants
  (`WorldTick`, `PhysicsWorld`, `NetIngress`, `Indices`, `Persistence`); rename it off "Fps"
  (e.g. `SimSet`).
* `server/src/app/{bootstrap,resources}.rs` final sweep; `server/src/main.rs` module decls.
* `client/src/main.rs`, `client/src/app_wiring/{mod,plugins,resources,systems,dev}.rs` final sweep.
  **Keep `dev.rs` autoconnect verbatim**, including the exact log string
  `"FISTFORCE_AUTOCONNECT: skipping main menu"` at `:42` — the `verify` skill greps for it.
* `client/src/audio/` — final shape is `state.rs` (`AudioManager`) + `limits.rs` (`apply_audio_limits`) +
  a gutted `assets.rs`. `paths.rs`, `remote_players.rs`, `vehicles.rs`, `ambient.rs` are gone.
* `client/src/render/systems/particles.rs` — resolve C5 (delete, or keep `ParticleAssets` with
  `#[allow(dead_code)]`).
* `client/src/input.rs` — remove `inventory_open` and fix its three KEEP-list readers.
* **Separate, isolated commit:** try the wholesale `MessageSender`/`MessageReceiver` block deletion
  (C11) in `server/src/net/connection.rs:84-126` and
  `client/src/render/systems/connection.rs:97-144`, then run the full smoke test. Revert if input stops
  flowing.

#### 6c. Cargo
* `client/Cargo.toml` — remove `image` (**verified already unused**: `rg '(^|[^:.\w])image::' client/src`
  returns nothing; every hit is `bevy::image::…`). Keep `noise` (`rendering/clouds.rs:513`), `rand`,
  `ron`, `arboard`.
* `shared/Cargo.toml` — remove `rand` (its only use was `shared/src/weapons/ballistics.rs:90-91`).
  **Keep** `bincode` (`colliders.rs:29` + profiles), `image` (`map/loader.rs:2`), `ron`, `noise`.
* Root `Cargo.toml` — drop the bevy `jpeg` feature (zero `.jpg`/`.jpeg` under `client/assets`).
  Keep `ktx2`. **Decide on `vorbis`** — after the strip `client/assets/audio/` is empty; keeping it costs
  compile time but keeps the audio framework immediately usable.
* **Do NOT remove `bevy_rapier3d`** — still used by `server/src/physics/{terrain_colliders,
  static_world_colliders,queries,layers}.rs`, `server/src/app/mod.rs:43`, and `tools/collider_baker`.
* `editor/Cargo.toml`, `tools/*` — no changes.

#### 6d. Assets
Slice-attributable deletions already happened per phase (~16.6 MB total).
**Separate, independently revertable commit** for the pre-existing orphans (~55 MB), because several may
be referenced from inside `.gltf` `uri` fields: `game_assets/props/{camp,walls,containers,treasure,
furniture,misc}/`, `game_assets/buildings/desert/`, `game_assets/textures/forest/` (37 MB — ⚠ verify;
`game_assets/environment/trees/*.gltf` use relative `uri`s), `game_assets/environment/{trees_lowpoly,
clouds}/`, `sky_10_2k/sky_10_cubemap_2k/`, `maps/city_alpha_backup_2026-07-13/` + the `*.bak` files.
**Never touch:** `client/assets/characters/` (incl. the unreferenced `sarah_animated.glb`),
`maps/city_alpha/`, `textures/`, `shaders/`, `colliders.bin` + `colliders_manifest.ron`
(loaded by `server/src/collision/library.rs:83` **and copied into the Docker image**), `servers.ron`.

#### 6e. Naming
Safe in one commit: window title (`plugins.rs:18`), doc comments, `README.md`,
`client/Cargo.toml [package.metadata.bundle]` (`"3DGame"` / `"com.terninator.3dgame"`),
`ui/fistforce.png` + `main_menu/layout.rs:38`.
**Rename only with the matching skill/doc edit in the same commit:** `FISTFORCE_AUTOCONNECT`,
`FISTFORCE_CLIENT_PERF[_INTERVAL_SECS]` (`.claude/skills/verify/SKILL.md:22,27-28,33`).
**Cross-crate contracts — all sites in one commit or not at all:** `FISTFORCE_ASSET_PATH`
(`editor/src/app.rs:28` sets; `app.rs:126`, `editor/src/ui.rs:1191`, `shared/src/map/loader.rs:261` read),
`CITYSIM_MAP_ID` (3 sites).
**Never rename:** `FLY_APP_NAME` (Fly.io platform var), `client/assets/maps/city_alpha/` (124 MB dir +
`DEFAULT_MAP_ID`). **Defer:** `fly.toml:1 app = 'fistforce'` (renaming a Fly app means re-creating it
with new IPs and editing `.github/workflows/fly-deploy.yml`).

#### 6f. Compile checkpoint + smoke test
```
cargo check --workspace --all-targets
cargo clippy --workspace -- -D warnings
cargo test --workspace
cargo run -p editor
```
Full `verify`-skill run: `Name accepted!` · `Spawned client world visuals` · `ClientPerf frame_ms_p50=` ·
no `ERROR bevy_asset` · terrain/props stream · world map + pause menu + debug menu open · profile
round-trips.

---

## 5. WHERE THE WORKSPACE WOULD FAIL TO COMPILE MID-PHASE

Every entry below is a KEEP-list (or must-still-compile) file that breaks the moment its phase's
deletions land. The "shim" column is what keeps the tree green.

| # | Phase | Broken file:line | Cause | Shim / fix |
|---|---|---|---|---|
| B1 | P1 | `client/src/props/debug.rs:5,147` | `shared::weapons::WeaponDebugMode` | **P0-2** moved it to `shared/src/debug.rs::DebugGizmoMode` |
| B2 | P1 | `client/src/water/overlay.rs:32` | `Pickable::IGNORE` from `crosshair/mod.rs:61` via glob import | **P0-3** moved `Pickable` to `client/src/ui/mod.rs` |
| B3 | P1 | `client/src/render/systems/rendering/setup.rs:122` | `SniperFisheye::default()` insert in the KEEP camera bootstrap | delete the one line |
| B4 | P1 | `client/src/audio/mod.rs:104` | `.after(handle_remote_audio_events)` ordering on `apply_audio_limits` | drop the constraint with the system |
| B5 | P1 | `client/src/render/systems/npc/{debug.rs:50-54, spawn.rs:136,192}` | `shared::weapons::damage::HitZone` for hitbox colours | recolour off `RagdollBodyId` or drop the colouring (P4 deletes these files) |
| B6 | P1 | `client/src/{chest.rs:108, pickup/prompts.rs:16,123}` | `InputState::is_dead` removed | drop the `\|\| input_state.is_dead` clause |
| B7 | P1 | `server/src/ai/tick/mod.rs:7,46-81` | `NpcDamageEvent` is `HitZone`-typed | delete `handle_npc_damage_events` + `schedule.rs:144` |
| B8 | P1 | `shared/src/npc.rs:148,221` | `HitBodyPart` in `humanoid_body_part` | delete that fn only; keep the rest of the file |
| B9 | P2 | **`shared/src/building/defs.rs:4,118-158,224`** | `ItemType` in `BuildingDef::cost` | delete the field + 6 initialisers (verified zero readers). **Also unbreaks `tools/collider_baker`** |
| B10 | P2 | `server/src/app/bootstrap.rs:104` | 2-element `add_systems` tuple loses one element | de-tuple the survivor |
| B11 | P2 | `server/src/telemetry/perf.rs:331,359` | `ground_items={}` format arity | remove the slot and the arg together |
| B12 | P3 | **`client/src/streaming.rs:13,17`** | `crate::rail::RtsRailCamera` → 10 KEEP call sites | **P0-4 + P0-5** |
| B13 | P3 | `client/src/render/systems/connection.rs:63` | `crate::rail::RailLocalPeerId` | **P0-4** moved it to `LocalPeerId` |
| B14 | P3 | `client/src/{camera.rs:6,51,239, render/systems/player/{mod.rs:40, sync.rs:70,123-126}}` | `VehicleHoverBob` leaks via `pub use vehicle::*` in `render/systems/mod.rs:21` — **grepping `shared::vehicle` misses it** | grep the type names, not the module path |
| B15 | P3 | `client/src/app_wiring/plugins.rs:124-133` | `ui::WorldMapPlugin` + `audio::GameAudioPlugin` are registered **only** inside `if !rail_mode` | hoist both out **before** deleting the branch (silent loss, not a compile error) |
| B16 | P3 | `server/src/physics/terrain_colliders.rs:124,160,165` | `gather_centers` signature | drop the vehicle param + call site |
| B17 | P4 | **`editor/src/tools.rs:564`** | `session.map_definition.npc_groups.clear()` | delete the line — **the only editor compile break in the whole strip** |
| B18 | P4 | `server/src/collision/{geometry.rs:7,271-350, resolve_player.rs:12,15,26,42,87-94}` | `ai::ragdoll::{CorpseBodyPoint, CorpseCollisionIndex}` | delete both corpse fns + the resolve_player corpse branch |
| B19 | P4 | `server/src/player/spatial.rs:113` | `Without<shared::components::Npc>` | drop the filter |
| B20 | P4 | `client/src/audio/limits.rs` (behaviour, not compile) | `AudioPriority` explicit discriminants shift when `Dialogue = 2` is removed | renumber deliberately |
| B21 | P5 | `client/src/audio/mod.rs:37` | `crate::camera::peer_id_to_u64` | keep the helper in the rewritten `camera.rs`; port the correct `ids.rs` version |
| B22 | P5 | `client/src/render/systems/rendering/setup.rs:99` | `crate::camera::CAMERA_NEAR_CLIP` | preserve the const |
| B23 | P5 | `client/src/water/{overlay.rs:47, material.rs:192, mod.rs:30,60}` | `PlayerWaterState` / `PlayerPosition` in ripple + overlay systems | drive the overlay from the camera; delete `emit_water_ripples` + its registration |
| B24 | P5 | `client/src/app_wiring/resources.rs:34` | `game_systems::LastCameraMode` lives in the deleted `render/systems/player/mod.rs:175` | delete the `init_resource` line |
| B25 | P5 | `client/src/ui/*` (6 modules) | `InputState` as the UI modal mutex | **trim `InputState`, never delete it** |
| B26 | P5 | `server/src/physics/queries.rs` + `server/src/collision/library.rs` Derived* | orphaned after P1/P5 → `-D warnings` CI fails | land an LOS wrapper / delete the Derived half, or `#[allow(dead_code)]` |
| B27 | P5 | `server/src/app/schedule.rs:269-270` | telemetry `.after(sync_debug_boxes_from_physics)` | **P0-6** made all nine anchors set-based |

---

## 6. DANGER — the non-compiler-checked items

Ranked by (probability × cost). **These are the ones that will actually hurt.**

### DANGER 1 — `SpawnMarkerKind::NpcGroup` bricks client + server + editor at boot (P4)
`client/assets/maps/city_alpha/edits.ron` (122 MB) contains one live `kind: NpcGroup` marker.
RON fails hard on unknown enum variants. `shared/src/map/save.rs:23-38` → `Err` →
`shared/src/map/loader.rs:59` → `Err` → **`shared/src/terrain/generator/map_access.rs:21-23 panic!`**,
inside `WorldTerrain` init, which all three binaries perform.
**Mitigation: keep the variant (C9).** Compile-clean, boot-dead — the single highest-risk item.

### DANGER 2 — `PlayerProfile` bincode layout silently destroys all 48 saves (P1/P2/P3/P5)
bincode is positional and not self-describing; `#[serde(default)]` on
`shared/src/player_profile.rs:71-87` does **nothing**. Worse, `version: u32` is field **#0**, so it still
decodes as `1` after a layout shift — the guard at `server/src/persistence/profiles.rs:51` (which runs
*after* `bincode::deserialize` at `:48`) **cannot catch it**. The P2 agent verified empirically with a
hand-written layout parser: 48/48 profiles fail after the P2 field removal, and an *empty-inventory*
profile decodes "successfully" into garbage (epoch `last_login`, zeroed `bank_gold`/`intelligence`) with
25 bytes silently trailing (`bincode-1.3.3/src/lib.rs:183` uses `.allow_trailing_bytes()`).
Failure path: `server/src/player/spawn.rs:90-98` swallows any `Err` into "Creating new profile", then the
next autosave overwrites the file. `server/src/player/roster_cache.rs:38-43` silently `continue`s, so the
roster empties with no log.
**Mitigation: P0-8 (`mv server_data/players server_data/players.pre-strip`) + a `PROFILE_VERSION` bump in
every schema-changing phase (C10).**

### DANGER 3 — client streaming fails OPEN, not closed (P3/P5)
`streaming_anchor()` returns `Option<Vec3>`; every one of its 10 KEEP-list callers does
`let Some(anchor) = … else { return; }`. A broken anchor produces an **empty world with zero log output**
— not a crash, not a warning.
**Mitigation: P0-5, plus a `warn_once!` on the `None` path.** Verify by walking/panning, not by reading
logs.

### DANGER 4 — server terrain colliders silently collapse to chunk (0,0) (P3→P4→P5)
`server/src/physics/terrain_colliders.rs:122-152 gather_centers` walks
players → vehicles → NPCs → `ChunkCoord::new(0,0)`. P3 removes vehicles, P4 removes NPCs, and if P5's
anchor is fumbled, players go too. Symptom: LOS raycasts pass straight through hills more than ~6 chunks
from the origin. No crash, no log.
**Mitigation: C1 keeps `PlayerPosition` alive as the commander anchor**; verify the
`CITYSIM_TERRAIN_COLLIDER_*` logs after P5.

### DANGER 5 — all client audio goes permanently silent (P1)
`client/src/audio/assets.rs:43-98 ensure_audio_assets_loaded` gates `AudioState::assets_ready` on a
5-tuple that includes **all four gunshot `.ogg`s**. Removing the handles without rewriting the readiness
match leaves `assets_ready == false` forever — ambient, footsteps and vehicle audio (all KEEP) go
silent with **no compile error**.
**Mitigation: rewrite the match to require only the ambient handle.** Smoke-test by walking.

### DANGER 6 — a KEEP plugin deleted by accident (P3)
`client/src/app_wiring/plugins.rs:126-133` registers `ui::WorldMapPlugin` and `audio::GameAudioPlugin`
**only** inside the `if !rail_mode { … }` branch, alongside four kill-side plugins. Deleting the branch
wholesale removes the world map and the entire audio system with no compile error.
**Mitigation: hoist both out first (B15).**

### DANGER 7 — wire-format skew from `PackedPlayerInput` (P1/P3/P5/P6)
`shared/src/protocol/messages.rs` has **hand-written** `Serialize`(:108) and `Deserialize`(:170) impls.
Editing one and not the other, or renumbering flag bits mid-strip, silently corrupts every input packet —
a stale binary reads yaw and movement from the wrong bits and produces phantom input.
**Mitigation: C12 (leave bit gaps until P6) + keep the roundtrip test green + bump `PROTOCOL_ID` in P6.**

### DANGER 8 — protocol net-id skew between client and server
`shared/src/protocol/plugin.rs` registration **order** determines lightyear's component/message net-ids.
Client and server share `ProtocolPlugin`, so a partial edit is impossible — but a stale *running* binary
against a rebuilt one mis-routes messages with no error.
**Mitigation: treat every protocol edit as a non-rolling restart of both binaries; bump `PROTOCOL_ID` at
P6 so the handshake refuses stale clients loudly.**

### Lesser dangers (know about them, don't lose sleep)
* `MapDefinition::npc_groups` removal is load-safe (no `deny_unknown_fields`, both maps have `[]`) but
  the editor will rewrite `map.ron` without the key — one-way for anyone holding an authored map. **Boot
  the editor against `city_alpha` before committing P4.**
* `shared/src/map/schema.rs:16 player_spawn` **must stay** — the editor writes it and 124 MB of maps
  carry it (P5).
* `AudioPriority` explicit discriminants (`client/src/audio/state.rs:56-59`) change numeric ordering when
  `Dialogue = 2` is removed (P4).
* `client/assets/colliders.bin` is loaded by `server/src/collision/library.rs:83` (with a hard `panic!`)
  and copied into the Docker image — **never sweep it up as a "client asset"**.
* `SwitchWeapon` and `MeleeAttackRequest` are already half-wired today (`SwitchWeapon` has a receiver but
  no handler; `MeleeAttackRequest` has a handler but the receiver is never inserted). **Melee probably
  does not work at all right now** — do not attribute post-strip behaviour changes there to yourself.

---

## 7. LINE BUDGET

| Phase | Deleted files | Edits | Manifest estimate |
|---|---:|---:|---:|
| P0 pre-flight | 28 (`prelude.rs`) | ~600 moved (not deleted) | ~0 net |
| P1 weapons & combat | ~7,900 | ~1,200 | **9,100** |
| P2 items & inventory | ~3,480 | ~220 | **3,700** |
| P3 vehicles & rail | ~5,046 | ~950 | **6,000** |
| P4 NPCs & AI | ~5,828 | ~620 | **6,450** |
| P5 embodiment → commander | ~3,900 | ~1,400 | **5,300** |
| P6 protocol/wiring/assets | ~800 | ~750 | **1,550** |
| **Raw sum** | | | **~32,100** |
| **De-duplicated net** | | | **~29,000** |

The ~3,100-line gap is double-claimed work: `client/src/pickup/` (P2 + P3), `server/src/combat/melee.rs`
+ `client/src/weapons/*` (P1 + P3 edits), `server/src/collision/resolve_npc.rs` (P4 + P5),
`shared/src/protocol/*` (every phase + P6), and `client/src/audio/*` (P1 + P3 + P4 + P5 + P6).
Asset deletion: **~16.6 MB** slice-attributable, **~55 MB** pre-existing orphans (separate commit).

---

## 8. ONE-PAGE EXECUTION CARD

```
P0  perf-overlay move · WeaponDebugMode move · Pickable move · RTS-camera harvest ·
    streaming re-anchor · telemetry set-anchors · delete prelude.rs · mv server_data/players
    -> check --workspace; check -p editor; boot both; VERIFY STREAMING + AUDIO + F3

P1  Health split · segment-raycast rescue · PROFILE_VERSION 1->2 ·
    rm client/src/{weapons,weapon_view,crosshair} render/sniper_fisheye.* ·
    rm server/src/combat server/src/inventory/death_drop.rs · rm shared/src/weapons components/combat.rs
    -> check; test -p shared; VERIFY AUDIO IS NOT SILENT (DANGER 5)

P2  PROFILE_VERSION 2->3 · rm shared/src/items server/src/inventory client/src/ui/inventory
    client/src/pickup client/src/chest.rs · fix shared/src/building/defs.rs (B9)
    -> build --workspace (catches tools/collider_baker); VERIFY PROFILE ROUND-TRIP

P3  PROFILE_VERSION 3->4 · hoist WorldMapPlugin+GameAudioPlugin (B15) ·
    rm shared/src/{vehicle,rail.rs,economy.rs} server/src/{vehicle,rail} client/src/rail
    client/src/render/systems/vehicle client/src/audio/vehicles.rs · kill FISTFORCE_RAIL
    -> check -p editor; VERIFY TERRAIN + PROP STREAMING (DANGER 3)

P4  salvage sync_obstacle_grid + A* + XorShift64 · KEEP SpawnMarkerKind::NpcGroup (DANGER 1) ·
    rm server/src/ai shared/src/npc.rs client/src/dialogue.rs client/src/render/systems/npc ·
    fix editor/src/tools.rs:564 (B17)
    -> check -p editor AND ./run.sh editor against city_alpha

P5  PROFILE_VERSION 4->5 · copy the .glb animation-index table out · KEEP PlayerPosition (C1) ·
    rm client/src/render/systems/player server/src/player/{movement,lifecycle,spatial}.rs
    server/src/physics/{dynamic_actors,contacts}.rs server/src/collision/{resolve_player,geometry}.rs
    + building_geometry/ shared/src/physics/character.rs · trim InputState, do not delete it
    -> clippy -D warnings; VERIFY SERVER TERRAIN COLLIDERS (DANGER 4)

P6  bump PROTOCOL_ID · slim plugin.rs (9 components, 6 messages, 2 channels) · one clean
    PackedPlayerInput rewrite · Cargo (drop client image, shared rand, bevy jpeg) · assets · naming ·
    the lightyear required-component experiment as its OWN commit (C11)
    -> full verify-skill run
```
