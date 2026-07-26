# Strip Plan — FPS ➜ Top-Down Tactics

Tracking doc for converting this repo from **FistForce** (multiplayer FPS sandbox) into a
**top-down multiplayer unit-tactics game** (many units, formations, huge maps).

> **Status:** Phase 0 in progress.

**Detailed analysis lives in [`docs/strip/`](docs/strip/):**
[MASTER-STRIP-PLAN.md](docs/strip/MASTER-STRIP-PLAN.md) (the authoritative execution plan — ordering
rationale, 12 resolved conflicts, per-phase file lists, danger analysis) plus seven per-slice manifests.
Produced by an 8-agent mapping pass over the whole workspace. **Read the master plan before executing
any phase** — this file is only the checklist.

---

## Safety net

The complete FPS game is preserved at git tag **`citysim-final`** (commit `281aa7c`).

```bash
git show citysim-final:client/src/weapons/mod.rs     # read a deleted file
git checkout citysim-final -- client/src/weapons/    # restore a whole subsystem
```

The `origin` remote was **deliberately removed**. Re-add it only to push the final FPS state, then
disconnect again.

⚠️ `281aa7c` currently exists **only on local disk** — it was created after the last push.
⚠️ `server_data/` is gitignored and **not** in the tag — it is not recoverable by any means.

---

## Ground rules

1. **The compiler is the safety net** — but only for references. The items in *Dangers* below are
   invisible to it. Delete first, then fix every error it reports.
2. **One phase = one commit**, ending green. Any phase can be reverted independently.
3. **`cargo check -p editor` is a hard gate after every phase.** The editor is the only KEEP-list
   crate nothing else depends on, so nothing else will catch its breakage.
4. **No shims** (per `CONTRIBUTING.md`): no compatibility aliases, no legacy bridges. Anything
   deliberately temporary (the P5 flycam) is named as such and tracked here.
5. **Bump `PROFILE_VERSION` in every phase that changes profile layout** (P1→2, P2→3, P3→4, P5→5).
   Four cheap bumps beat one clever migration.
6. **Never renumber `PackedPlayerInput` bits mid-strip** — leave gaps, one clean rewrite in P6.
7. **Assets are deleted last** (P6), only after code deletion proves nothing loads them.

---

## ⚠️ Dangers — the things the compiler will NOT catch

| # | Risk | Phase | Mitigation |
|---|------|-------|------------|
| 1 | **`SpawnMarkerKind::NpcGroup` bricks all 3 binaries at boot.** `city_alpha/edits.ron` contains a live one; RON hard-fails unknown enum variants → `panic!` in `WorldTerrain` init | P4 | **Keep the variant.** Relabel its UI string only |
| 2 | **48 player profiles silently decode to garbage.** bincode is positional; `version` is field #0 so the guard can't catch a layout shift | P1/P2/P3/P5 | `mv server_data/players` aside (P0-8) + `PROFILE_VERSION` bump per phase |
| 3 | **Streaming fails OPEN.** `streaming_anchor()` returns `Option`; all 10 callers `else { return }` → empty world, zero log output | P3/P5 | P0-5 re-anchor + `warn_once!` on the `None` path |
| 4 | **Server terrain colliders collapse to chunk (0,0).** `gather_centers` walks players→vehicles→NPCs→origin | P3→P5 | Keep `PlayerPosition` as commander anchor; check `CITYSIM_TERRAIN_COLLIDER_*` logs |
| 5 | **All client audio goes permanently silent.** `ensure_audio_assets_loaded` gates readiness on a 5-tuple including 4 gunshot `.ogg`s | P1 | Rewrite the readiness match to require ambient only |
| 6 | **A KEEP plugin deleted by accident.** `WorldMapPlugin` + `GameAudioPlugin` are registered inside the `if !rail_mode` branch | P3 | Hoist both out before deleting the branch |
| 7 | **Wire-format skew.** `PackedPlayerInput` has hand-written `Serialize`/`Deserialize` impls | P1/P3/P5/P6 | Edit both in lockstep; keep the roundtrip test green; bump `PROTOCOL_ID` in P6 |
| 8 | **Protocol net-id skew.** Registration *order* determines lightyear net-ids | P6 | Non-rolling restart of both binaries on every protocol edit |

---

## Keep list — why we strip in-place instead of starting fresh

| Area | What survives |
|------|---------------|
| **Editor** | The entire `editor/` crate — sculpt, paint, scatter brush, worldgen, roads/plots, resize |
| **Terrain** | Heightfield, chunking, streaming, generator, splat painting, KTX pipeline |
| **Map data** | `shared/src/map` schema + persistence, `edits.ron` weightmaps/deltas |
| **World content** | `shared/src/city` (roads/plots), `shared/src/building`, `shared/src/props` |
| **Rendering** | Water, wind/foliage shaders, sky + day/night, props/LOD (`render/lod.rs` is generic — keep) |
| **UI** | World map, main menu, pause menu, name entry, modal framework |
| **Net** | lightyear connection/channels/protocol-plugin structure |
| **Spatial** | `shared/src/spatial.rs` grid index |
| **Audio** | The framework — minus weapon/engine sounds |
| **Assets** | `client/assets/characters/` — soldier models & animations become the units |
| **Rescued** | `Health`, `PlayerPosition`/`PlayerRotation`, segment raycasts, the RTS camera, perf overlay |

---

## Phases

Legend: ⬜ not started · 🟨 in progress · ✅ done

### 🟨 P0 — Pre-flight refactor (NO deletions)
Pure moves/renames. Tree compiles and game runs identically after each. **Highest-leverage phase.**

- [x] **P0-1** Perf overlay out of `client/src/weapons/` → `client/src/perf_overlay/`
      (F3 overlay, `FISTFORCE_CLIENT_PERF`, and the verify skill's success grep depend on it).
      Dropped the bullet/tracer/muzzle counters from the overlay while moving, so P1 need not touch it.
- [x] **P0-2** `WeaponDebugMode` → `shared/src/debug.rs` as `DebugGizmoMode` (used by prop collider gizmos)
- [x] **P0-3** ~~`Pickable` → `client/src/ui/mod.rs`~~ — **plan was wrong; deleted instead.**
      `Pickable` is exported by `bevy::prelude` (bevy_picking 0.18). The `crosshair` copy was a redundant
      *inert* unit-struct shadowing Bevy's real component — its `IGNORE` did nothing. Deleting it means
      every call site (incl. `water/overlay.rs`) now resolves to Bevy's, which is correct **and** functional.
- [x] **P0-4** Harvest RTS camera from `client/src/rail/` → `client/src/camera_rts.rs` as `CommanderCamera`
      (+ `LocalPeerId`, `CursorTerrainHit`, `intersect_terrain`). Resource init moved to `app_wiring`
      so it survives the P3 rail deletion.
- [x] **P0-5** Re-anchor `client/src/streaming.rs` onto it + `warn_once!` on `None` (Danger 3)
- [x] **P0-6** Re-anchor 7 telemetry ordering constraints onto `FpsServerSet` instead of dying systems.
      Note: `handle_perf_core_phase_end` now anchors on the `PhysicsPost` *set*, which widens that
      bracket slightly to include ragdoll sync (those systems die in P4 anyway).
- [x] **P0-7** Delete `shared/src/prelude.rs` (zero consumers, re-exported 17 kill-side symbols)
- [x] **P0-8** `mv server_data/players server_data/players.pre-strip` — 48 profiles (Danger 2)
- [x] Compile checkpoint: `cargo check --workspace --all-targets` green · `cargo check -p editor` green ·
      61 shared tests pass · 4 editor tests pass
- [x] Smoke test **passed on both streaming-anchor paths**:
      - FPS mode — no panics, no anchor warning, `ClientPerf frame_ms_p50=16.67` emitted from
        `client::perf_overlay` (overlay move verified end-to-end), terrain/props/player/ambient audio alive.
      - Rail mode — no panics, no anchor warning; `Rail command rejected: Track length…` proves the
        harvested `update_cursor_terrain_hit` + `intersect_terrain` still resolve real world positions.
      - Editor — boots on `city_alpha`, terrain arrays load, no panics.
      - *(Pre-existing, not a regression: rail mode emits no `ClientPerf` because the perf systems are
        wired only in `wire_fps_systems`.)*

### ⬜ P1 — Weapons & combat (~9,100 lines)
- [ ] Rescue first: split `Health` → `shared/src/components/health.rs`; move segment raycasts →
      `server/src/collision/raycast.rs`; `PROFILE_VERSION` 1→2
- [ ] `client/src/{weapons,weapon_view,crosshair}/`, `render/sniper_fisheye.*`
- [ ] `server/src/combat/`, `server/src/inventory/death_drop.rs`
- [ ] `shared/src/weapons/`, `shared/src/components/combat.rs`
- [ ] Fix audio readiness match (**Danger 5**) — verify audio is NOT silent
- [ ] `cargo check` green · `cargo test -p shared` · commit

### ⬜ P2 — Items & inventory (~3,700 lines)
- [ ] `PROFILE_VERSION` 2→3
- [ ] `shared/src/items/`, `server/src/inventory/`, `client/src/ui/inventory/`,
      `client/src/pickup/`, `client/src/chest.rs`
- [ ] Fix `shared/src/building/defs.rs`
- [ ] `cargo build --workspace` (catches `tools/collider_baker`) · verify profile round-trip · commit

### ⬜ P3 — Vehicles & rail (~6,000 lines)
- [ ] `PROFILE_VERSION` 3→4; hoist `WorldMapPlugin` + `GameAudioPlugin` out of the `rail_mode` branch (**Danger 6**)
- [ ] `shared/src/{vehicle,rail.rs,economy.rs}`, `server/src/{vehicle,rail}`, `client/src/rail`,
      `client/src/render/systems/vehicle`, `client/src/audio/vehicles.rs`, `FISTFORCE_RAIL`
- [ ] `cargo check -p editor` · **verify terrain + prop streaming** (Danger 3) · commit

### ⬜ P4 — NPCs & AI (~6,450 lines)
- [ ] Salvage `sync_obstacle_grid`, A\* core, `XorShift64` before deleting
- [ ] **KEEP `SpawnMarkerKind::NpcGroup`** (Danger 1)
- [ ] `server/src/ai/`, `shared/src/npc.rs`, `client/src/dialogue.rs`, `client/src/render/systems/npc/`
- [ ] Fix `editor/src/tools.rs:564`
- [ ] `cargo check -p editor` **and** `./run.sh editor` against `city_alpha` · commit

### ⬜ P5 — FPS embodiment ➜ commander (~5,300 lines)
- [ ] `PROFILE_VERSION` 4→5; copy the `.glb` animation-index table out first
- [ ] **KEEP `PlayerPosition`/`PlayerRotation`** — they *become* the commander view (no `CommanderView` type)
- [ ] `client/src/render/systems/player/`, `server/src/player/{movement,lifecycle,spatial}.rs`,
      `server/src/physics/{dynamic_actors,contacts}.rs`,
      `server/src/collision/{resolve_player,geometry}.rs` + `building_geometry/`,
      `shared/src/physics/character.rs`
- [ ] Trim `InputState` — do not delete it
- [ ] `cargo clippy -D warnings` · **verify server terrain colliders** (Danger 4) · commit

### ⬜ P6 — Protocol, wiring, assets, naming (~1,550 lines)
- [ ] Bump `PROTOCOL_ID`; slim `plugin.rs` (9 components, 6 messages, 2 channels)
- [ ] One clean `PackedPlayerInput` rewrite (all bit gaps closed at once)
- [ ] Cargo trim; asset deletion (~16.6 MB slice-attributable + ~55 MB pre-existing orphans, separate commit)
- [ ] Rename FistForce/citysim identifiers (⚠️ asset paths, save dirs, env vars)
- [ ] Rewrite `README.md`; drop the dead `RAGDOLL_HANDOFF.md` link
- [ ] lightyear required-component experiment as its **own isolated commit**
- [ ] Full verify-skill run · commit

**Net removal: ~29,000 lines** (~32,100 raw, minus ~3,100 double-claimed).

---

## Verification protocol

Run after **every** phase — a green compile is necessary, not sufficient:

```bash
cargo check --workspace --all-targets     # must be green before committing
cargo check -p editor                     # hard gate — nothing else catches editor breakage
cargo test -p shared                      # 61 tests

# Smoke test (no `timeout` on this Mac — background + sleep + kill)
./target/debug/server &
BEVY_ASSET_ROOT=$PWD/client FISTFORCE_AUTOCONNECT=Test$(date +%H%M%S) ./target/debug/client &
sleep 25; pkill -f "citysim/target"
```

Grep logs for panics / shader errors / asset-not-found. Then check by **behaviour**, not logs:
terrain + props stream while moving · audio audible · F3 overlay renders · editor opens `city_alpha`.

---

## Deferred decision — netcode model

**Not decided, and does not block the strip.** Once the unit sim starts:

- **Deterministic lockstep** — exchange only commands; scales to thousands of units; requires a fully
  deterministic sim (fixed-point or disciplined f32, seeded RNG, no map-iteration-order leaks).
- **Server-authoritative + interest management** — reuses far more of the existing lightyear
  replication; caps practical unit count much lower.

Either way the connection/channel plumbing survives, which is why it is on the keep list.
