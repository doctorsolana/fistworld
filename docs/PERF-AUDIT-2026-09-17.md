# Overnight client performance audit — 2026-09-17

Branch `perf-audit-2026-09-17`, worktree `/Users/terminator2/Coding/fistworld-perf-audit`.
Hardware: MacBook Pro `Mac17,3`, Apple M5 (10 cores), macOS 26.5.2. Window is borderless
native 2940x1846; the 3D scene renders into a 1764x1108 offscreen target at the
fresh-install default `render_scale = 0.60`; software frame cap 60.

Scene under test: server `CITYSIM_MAP_ID=village_lab FISTWORLD_VILLAGE_LAB_RUNTIME=1
FISTWORLD_LAB_SCENARIO=dense-stress FISTWORLD_LAB_WARP=10 FISTWORLD_DEV=1`; one settlement
(`Lab Meadow`, 24 blocks of `Humanoid.glb` villagers, 1000 staged), client camera locked at
the town focus `112,-158`, default zoom 280 m, 10x sim for the first 120 s then 1x.
The last 80–90 s of each 240 s run (all at 1x) is the measurement window. Every run used
`FISTFORCE_FRAME_CAP=60`, `FISTFORCE_SHADOWS` etc. as listed, `FISTFORCE_CAMERA_LOCK=1`,
`FISTFORCE_AUTOTIME_PRESET=midday`, and the client-side gameplay-entry hooks below.

## 0. Run validity gate (this wasted the first baseline)

`FISTFORCE_AUTOCONNECT` skips the menu and submits the player name, but it does **not**
start the hero flow. The first baseline measured the **hero-creator overlay** (the
"BEGIN JOURNEY" screen), not gameplay. All reported runs add:

- `FISTWORLD_AUTOCREATE_VOYAGE=1` — sends the normal `CreateHero` request
  (`client/src/ui/name_entry/network.rs:181`).
- `FISTWORLD_UX_TOWN=1` — client-only; cancels the opening boat cinematic
  (`client/src/ui/name_entry/network.rs:175-201`). It is not set for the server, which
  still stages the dense-stress lab.

A run counts only if the log has `FISTWORLD_AUTOCREATE_VOYAGE: sent normal CreateHero
request` and no `creator-preview` mesh churn. `FISTWORLD_AUTOSPEED_AFTER` was also moved
from the task's server line to the client: the server-side variable no longer exists; the
replacement is the client `hero::control::auto_set_time_warp_after`
(`client/src/hero/control.rs:386`). `FISTFORCE_GRASS_STRESS_DENSITY` is clamped to a
minimum of 1.0 (`client/src/props/ground_cover_chunked.rs:60-66`), so it cannot be used as
a grass kill switch; grass is covered by `FISTFORCE_PROPS=0`.

## 1. The answer

**In the audit town the frame costs ~32 ms (about 31 fps). The single biggest concentrated
cost is the replicated crowd: 1,001 villagers, each a full wardrobe rig with 21 animated
bones and 23 mesh parts. Removing the villagers (a small-town server scenario) drops the
frame to ~20.7 ms; pausing only their animation drops it to ~28.2 ms. Everything GPU-side —
shadows, TAA, the shadow filter, clouds, cloud-shadow material churn, half resolution —
measured at or below noise, because the frame is CPU-bound and the render thread spends
most of its time waiting.**

Mechanism, in one line: at RTS zoom every villager is on screen, so the existing
"can't be seen, don't animate" gate never fires; all 1,001 rigs animate, and Bevy's CPU
animation evaluation plus per-part skinned-mesh batching/visibility dominate the frame.

**Top three changes, by measured saving:**

| # | change | expected saving | evidence |
|---|---|---:|---|
| 1 | **Animation LOD for villagers**: pause rigs outside a distance/nearest-N budget (the pause/weight-0 path already exists in `drive_hero_locomotion`), or update each rig every 2nd–3rd frame | **≈ 4 ms** (scales with crowd size; removes the single largest CPU span, 54.6 ms CPU/frame) | A/B run with all rigs forced into the paused path: 32.1 → 28.2 ms |
| 2 | **Stop paying for 23 mesh parts per villager**: merge the worn outfit into one skinned mesh per outfit (asset build), and/or despawn the hidden wardrobe children after dressing | **≈ 4–8 ms** | crowd total ~11.5 ms minus animation ~3.9 ms; trace shows ~30 ms CPU/frame of mesh extract/batch/prepare for ~5.7 k visible parts |
| 3 | **Prop/grass tuning** (render distance, LOD, density) | **≈ 1.5 ms** | back-to-back pair 32.34 → 30.87 ms |

The remaining ~19 ms floor is terrain + UI/present at native resolution + engine overhead;
the trace shows no single span there above ~7 ms (see §3), so it is not one fix.

## 2. Ablation table (sorted by measured saving)

All runs 240 s, dense-stress unless noted, camera locked at `112,-158`. "p50 min" is the
minimum of the eight 10-s steady-state p50 samples, which is the most drift-resistant
number; "Δ" is against the nearest-in-time baseline. `probe` is the 3 M-iteration Python
thermal probe (cold baseline 82 ms) before/after the run.

| run | env change | p50 mean | p50 min | Δ min (vs) | probe | note |
|---|---|---:|---:|---:|---|---|
| no-villagers | server scenario `secure` (17 rigs) | 20.72 | 20.59 | **−11.5** (b2 32.06) | 94/124 | smaller town too |
| empty-forest | focus `1200,-1200` | 20.86 | 20.51 | **−11.6** (b2) | 100/80 | no town near camera |
| anim-off | temporary patch: pause every rig | 28.46 | 28.21 | **−3.9** (b2) | 83/79 | same compiler; patch reverted |
| props-off2 | `FISTFORCE_PROPS=0` | 31.12 | 30.87 | **−1.5** (b3 32.34) | 83/81 | back-to-back pair |
| props-off | `FISTFORCE_PROPS=0` | 33.03 | 32.66 | −4.5 (b1 37.15)* | 91/111 | *cross-compiler |
| baseline2 | — (clean) | 32.24 | 32.06 | — | 80/78 | rustc 1.98 |
| baseline3 | — (clean) | 32.70 | 32.34 | — | 86/84 | rustc 1.98 |
| baseline | — (first) | 38.08 | 37.15 | — | 80/93 | rustc 1.97, see caveat |
| atmosphere-off | `FISTFORCE_ATMOSPHERE=0` | 40.17 | 36.68 | +4.6 (b2) | 84/99 | warm run |
| shadows-off | `FISTFORCE_SHADOWS=0` | 39.89 | 37.52 | +0.4 (b1) | 81/75 | no effect |
| shadow-filter-hw | `FISTFORCE_SHADOW_FILTER=hw` (also removes TAA) | 42.17 | 39.04 | +1.9 (b1) | 80/82 | no effect |
| cloud-shadow-freeze | `FISTFORCE_CLOUD_SHADOW_FREEZE=1` | 43.70 | 41.59 | +4.4 (b1) | 83/99 | warm-ish, no effect |
| half-res | `FISTFORCE_RENDER_SCALE=0.5` | 43.57 | 41.57 | +4.4 (b1) | 82/210 | throttled; no effect |
| clouds-off | `FISTFORCE_CLOUDS=0` | 50.90 | 45.11 | +8.0 (b1) | 89/157 | throttled; no effect |

The positive deltas are not costs: `probe_after` 210/157 ms on half-res/clouds-off shows the
machine was hot; several others are within the ±1.5 ms run-to-run spread measured between
identical baselines. The direction is unambiguous, though: **turning GPU work off never
helped.** The first baseline (rustc 1.97, 37–38 ms) is 5 ms slower than every later baseline
(rustc 1.98, 32.1–32.3 ms); a rustup update by another process forced a toolchain change and
a rebuild mid-audit, so cross-compiler rows are only used directionally.

## 3. Chrome trace (150 s, 8.1 GB, tail 30 s at 1x, 26.5 fps)

Built `CARGO_TARGET_DIR=target-trace cargo build --profile playtest -p client --features
bevy/trace,bevy/trace_chrome` (`trace_chrome` alone only adds the layer; `bevy/trace` adds
the spans). Aggregated with the streaming parser `logs/perf-audit-2026-09-17/trace_tail2.py`.

**Main thread (wall):** whole `update()` 37.8 ms/frame; `PostUpdate` **25.8 ms** (25.5 ms of
it self time — waiting for the task pool plus non-instrumented main-thread systems);
`Update` 5.1; `ExtractSchedule` 3.0; `PreUpdate` 2.1.

**Render thread:** the `Render` schedule spans 30.6 ms/frame, but actual render-graph work
(`render_system`) is **7.1 ms**, `camera_driver` 4.1, `submit_pending_command_buffers` 2.95.
The remaining ~22 ms is idle/wait inside the schedule: the GPU has headroom and the render
thread waits on the main thread. `main_opaque_pass_3d` 0.42 ms, `early_prepass` 0.39,
`bloom` 0.49, `main_transparent_pass_3d` 0.02 — all noise.

**Top CPU spans across task-pool threads (self ms/frame):**

| span | ms/frame |
|---|---:|
| `par_for_each` inside `bevy_animation::animate_targets` (bone evaluation) | **54.6** |
| `write_indirect_parameters_buffers` | 7.1 |
| `write_batched_instance_buffers<Mesh>` | 6.7 |
| `prepared_mesh_producer` | 6.2 |
| `collect_meshes_for_gpu_building` | 3.1 |
| visibility `par_for_each`s (3 variants) | ~4.5 |
| `extract_skins` | 2.5 |
| `write_binned_instance_buffers` (2 variants) | 2.8 |
| `update_skinned_mesh_bounds` | 1.9 |
| `prepare_skins` | 1.7 |
| `propagate_parent_transforms` | 1.4 |
| `mark_dirty_trees` | 1.0 |
| `check_visibility_cpu_culling` | 1.0 |

`animate_targets`' carrying-thread critical-path chunk measured 14.9 ms; the total 54.6 ms of
CPU is spread over ~6 tasks, which is why the wall-clock A/B saving (3.9 ms) is smaller than
the CPU number. Client game systems were all individually below 0.5 ms/frame
(`drive_hero_locomotion`, the UI sync systems, replication receive at 0.07 ms) — the cost is
inside Bevy's per-rig animation and per-part render preparation, not in game logic.

**Rig geometry (`client/assets/characters/Humanoid.glb`):** 45 nodes, 1 skin with **21
joints**, 23 primitives (body + 22 wardrobe items across 4 slots), 34 clips / 2,142 channels.
1,001 replicated rigs → ~21 k animation targets, 23,857 `Mesh3d` entities, ~5.7 k parts with
`InheritedVisibility` (i.e. ~5.7 k skinned draws at this camera). Every rig instantiates the
whole wardrobe; only the worn items are shown. `ClientPerfChangedMeshes`/`ClientPerfMeshes`
report 23,857 parts and ~7,999 mesh transforms changed per frame (villager motion).

## 4. Confirmed / Plausible / Rejected

### Confirmed

- **The frame is CPU-bound; the render thread is mostly idle.** Trace: main thread
  `update()` 37.8 ms vs render `render_system` 7.1 ms with ~22 ms self time waiting.
  Corroborated by every GPU ablation measuring ≥0.
- **Bevy CPU animation evaluation is the largest single span.** `animate_targets` →
  `par_for_each` 54.6 ms CPU/frame for 1,001 rigs × 21 bones
  (`client/src/hero/animation.rs:467` gates rigs; Bevy's `bevy_animation` evaluates them).
  Forcing all rigs into the existing paused path measured **−3.9 ms** wall
  (`FISTFORCE_ANIM_OFF=1` on a temporary patch that was reverted with
  `git checkout -- client/src/hero/animation.rs` before this report was written).
- **The crowd as a whole costs ~11.5 ms.** small-town scenario (`secure`, 17 rigs) 20.6 ms
  vs dense-stress 32.1 ms.
- **Per-part skinned render preparation is the crowd's second cost.** Trace: 5.7 k visible
  parts drive ~30 ms CPU/frame across `write_indirect_parameters_buffers`,
  `write_batched_instance_buffers`, `prepared_mesh_producer`, `collect_meshes_for_gpu_building`,
  `extract_skins`, `prepare_skins` and skinned-AABB updates. 23 mesh entities exist per rig;
  22 of them are wardrobe variants. `client/src/hero/appearance.rs:53-82` spawns the whole
  scene, `:132-199` hides the unworn items but leaves the entities in the hierarchy.
- **Props/trees/grass ≈ 1.5 ms** in the back-to-back pair (`FISTFORCE_PROPS=0` also disables
  ground cover: `client/src/props/ground_cover_chunked.rs:597`, `props/spawn.rs:339`).

### Plausible (mechanism clear, not directly ablated)

- **The ~19 ms floor is terrain + native-resolution UI/present + engine overhead.** It
  survives with no town and few rigs, no trace span in it exceeds ~7 ms, and it is not
  fill-rate (half-res no-op). The UI layout system is not instrumented in the trace, so its
  share is unmeasured; `ui_scale` is 1.84 at 2940x1846 and the present pass is full native
  resolution (`render/systems/rendering/scaled_target.rs:39-46`).
- **Hidden wardrobe parts still cost transform/visibility/AABB propagation.** 22 of 23 parts
  per rig are hidden but remain in the hierarchy; visibility `par_for_each`s + transform
  propagation + skinned-bounds updates total ~9 ms CPU/frame across threads, much of it on
  these entities. No direct ablation was possible without an asset/runtime change.
- **The exact CPU/GPU split of the crowd's rendering cost is unresolved.** `empty-forest`
  (same replicated 1,001 rigs, camera 1.7 km away) also lands at 20.5 ms, yet its census
  still reports `animating = 999`, `any_part_seen = 999`, `in_frustum_margin = 0` — the rigs
  are outside every camera frustum but still count as visible/animated. If Bevy extracts
  them anyway (skinned parts without valid AABBs count as visible), then empty-forest already
  pays the crowd's CPU extraction cost, and the 7.7 ms gap to anim-off is GPU vertex/skinning
  work that half-resolution would not remove. Either way the fix is the same (fewer/merged
  parts), but do not assume the whole 7.7 ms is CPU batching.
- **The town's static visuals share part of the crowd delta**: the no-villagers run also has
  a smaller town. The first-pass props pair bounds the town/prop share at ~1.5 ms.

### Rejected (checked, not a problem — do not re-audit)

- **Directional shadow cascades / shadow casters**: `FISTFORCE_SHADOWS=0` (turns the sun's
  `shadow_maps_enabled` off, `settings.rs:734-741`) and `FISTFORCE_SHADOW_FILTER=hw` (which
  also strips TAA) both measured within noise. Cascade config at zoom 280 is 2 cascades of
  2048² over 700 m (`settings.rs:200-243`, `sync_shadow_cascades_to_zoom` `:807-840`).
- **TAA / temporal shadow filter**: removed by the `hw` run; no change.
- **Fill rate / render scale**: `FISTFORCE_RENDER_SCALE=0.5` removes 4× the 3D pixels and
  changed nothing (the scene target goes 1764x1108 → 1470x923). Not fill-bound.
- **Clouds and cloud-shadow material churn**: `FISTFORCE_CLOUDS=0` and
  `FISTFORCE_CLOUD_SHADOW_FREEZE=1` (the diagnostic that stops the 48-lane-per-frame
  `materials.get_mut` sweep, `cloud_shadows.rs:40-50,275-287`) both measured ≥0.
- **Atmosphere/sky**: `FISTFORCE_ATMOSPHERE=0` (despawns the `Atmosphere` entity,
  `atmosphere.rs:176-217`) measured within noise.
- **Replication**: `bevy_replicon::client::receive_replication` 0.07 ms/frame,
  lightyear transport buffer 0.01 — not a cost at this scale.
- **Terrain streaming in steady state**: 289 chunks loaded, spawn/unload 0 after startup;
  `ClientPerfTerrain` steady.
- **The opt-in diagnostic itself**: `log_changed_mesh_archetypes` costs 0.17 ms/frame; the
  census is fine to leave on for perf runs.

## 5. Leaks (Phase 3)

**Static soak** (8 min, camera locked, dense-stress; `logs/perf-audit-2026-09-17/soak-static.*`):
RSS sampled every 10 s: 878 → 902 MB, oscillating around ~893 MB with no monotonic climb
(final 894 MB, probe after 72 ms). Census counters: rigs stable at 1,001 (999 animating);
terrain chunks stable 289; `delta_chunks` 0→7, `max_version` 0→10 (the village keeps doing
earthworks/upgrades at 1x); meshes 494→560, std_materials 117→158, images flat at 341. The
slow mesh/material climb is consistent with continuing construction and villager dressing,
not a leak (RSS flat, no unbounded counter). Owning systems: settlement construction
visuals (`client/src/settlement/construction.rs`, `buildings.rs`) and hero dressing
(`client/src/hero/appearance.rs`).

**Streaming churn** (`capture/scenarios/meadow-trees-flight.ron`, out-and-back over the
meadow, same start/end camera): entity count 3,586 → 3,211, chunks constant 289, props
910 → 640, trees 418 → 193, grass batches 84 → 84. Counts oscillate with the path and end
below the start — no ratchet. Chunk unload drops the mesh, material, weightmap image and
despawns the entity (`client/src/terrain/streaming/spawn.rs:93-101`); grass state prunes
chunks and render entities (`props/ground_cover_chunked.rs:530,650`); cloud-shadow
`applied` map prunes at 2× live (`cloud_shadows.rs:288-295`). No leak found.

## Appendix A — top 15 spans per thread (trace tail, ms/frame)

Main thread (tid 0). Note: `PostUpdate`'s 25.5 ms self time is the main thread waiting for
work executed on other threads; the systems themselves appear in §3's cross-thread table.

| span | incl | self |
|---|---:|---:|
| `update:` | 37.77 | 0.01 |
| `main app:` | 34.50 | 0.00 |
| `schedule: name=Main` | 34.49 | 0.04 |
| `schedule: name=PostUpdate` | 25.83 | 25.46 |
| `schedule: name=Update` | 5.06 | 4.15 |
| `sub app: name=RenderExtractApp` | 3.27 | 0.25 |
| `schedule: name=ExtractSchedule` | 3.01 | 2.98 |
| `schedule: name=PreUpdate` | 2.06 | 1.88 |
| `multithreaded executor:` | 0.92 | 0.84 |
| `schedule: name=RunFixedMainLoop` | 0.56 | 0.04 |
| `system: bevy_time::fixed::run_fixed_main_schedule` | 0.50 | 0.01 |
| `schedule: name=FixedMain` | 0.49 | 0.04 |
| `schedule: name=StateTransition` | 0.37 | 0.30 |
| `schedule: name=First` | 0.24 | 0.24 |
| `schedule: name=Last` | 0.20 | 0.18 |

Render thread (tid 4). The 22.4 ms `Render` self time is idle/wait; the real render graph is
7.1 ms.

| span | incl | self |
|---|---:|---:|
| `sub app: name=RenderApp` | 30.74 | 0.01 |
| `schedule: name=RenderRecovery` | 30.73 | 0.06 |
| `system: bevy_render::run_render_schedule` | 30.66 | 0.01 |
| `schedule: name=Render` | 30.64 | 22.40 |
| `system: bevy_render::renderer::render_system` | 7.13 | 0.01 |
| `main_render_schedule:` | 7.13 | 0.04 |
| `schedule: name=RenderGraph` | 7.08 | 0.05 |
| `system: bevy_core_pipeline::schedule::camera_driver` | 4.07 | 0.03 |
| `camera_schedule: camera="Camera 0 (350v0)"` | 3.75 | 0.01 |
| `schedule: name=Core3d` | 3.74 | 1.09 |
| `system: bevy_core_pipeline::schedule::submit_pending_command_buffers` | 2.95 | 0.01 |
| `queue_submit: count=19` | 2.94 | 2.94 |
| `system: bevy_pbr::render::skin::extract_skins` | 0.54 | 0.54 |
| `RenderContextState::apply: bloom` | 0.49 | 0.49 |
| `RenderContextState::apply: main_opaque_pass_3d` | 0.42 | 0.42 |

## 6. Every command (reproduction)

```sh
# build
cargo build --profile playtest -p server -p client

# thermal probe before EVERY run (cold baseline recorded once: 82 ms)
python3 -c "import time;t=time.time();sum(i*i for i in range(3_000_000));print(round((time.time()-t)*1000))"
pmset -g therm        # CPU_Speed_Limit; blank on this OS build, treated as 100

# server (per run; killed after)
CITYSIM_MAP_ID=village_lab FISTWORLD_VILLAGE_LAB_RUNTIME=1 FISTWORLD_LAB_SCENARIO=dense-stress \
FISTWORLD_LAB_WARP=10 FISTWORLD_DEV=1 ./target/playtest/server > run.server.log 2>&1 &

# client baseline (append per-run env changes for the ablations)
env BEVY_ASSET_ROOT=$PWD/client/assets \
  FISTFORCE_CLIENT_PERF=1 FISTFORCE_CLIENT_PERF_INTERVAL_SECS=10 \
  FISTFORCE_AUTOCONNECT=auditNNN FISTFORCE_START_FOCUS=112,-158 FISTFORCE_CAMERA_LOCK=1 \
  FISTFORCE_FRAME_CAP=60 FISTFORCE_EXIT_AFTER_SECS=240 \
  FISTFORCE_RENDER_DIAG=1 FISTFORCE_LOG_DIAGNOSTICS=1 \
  FISTWORLD_AUTOSPEED_AFTER=120,1 FISTFORCE_AUTOTIME_PRESET=midday \
  FISTWORLD_AUTOCREATE_VOYAGE=1 FISTWORLD_UX_TOWN=1 \
  ./target/playtest/client > run.client.log 2>&1

# trace build (separate target dir) and run
CARGO_TARGET_DIR=target-trace cargo build --profile playtest -p client \
  --features bevy/trace,bevy/trace_chrome
TRACE_CHROME=$PWD/logs/perf-audit-2026-09-17/trace.json FISTFORCE_EXIT_AFTER_SECS=150 \
  FISTWORLD_AUTOSPEED_AFTER=60,1 <same env> ./target-trace/playtest/client

# trace tail aggregation (streaming, never loads the 8.1 GB file)
python3 logs/perf-audit-2026-09-17/trace_tail2.py logs/perf-audit-2026-09-17/trace.json 30

# leak: static soak (8 min, RSS every 10 s) and streaming flight
./logs/perf-audit-2026-09-17/soak_static.sh
BEVY_ASSET_ROOT=$PWD/client/assets FISTFORCE_CLIENT_PERF=1 \
  ./target/playtest/capture --scenario capture/scenarios/meadow-trees-flight.ron
```

Harness scripts (in ignored `logs/perf-audit-2026-09-17/`): `run_one.sh` (thermal gate,
server+client orchestration, load sampling, parsing), `ladder.sh`/`ladder2.sh`/`ladder3.sh`,
`parse.py`/`collect.py`/`trace_tail2.py`, `soak_static.sh`, `pair.sh`.

Environment caveats: another agent shares this Mac (checkout
`/Users/terminator2/Coding/fistworld`); a rustup update corrupted the toolchain mid-audit and
forced `target-trace` to be recreated (left as `target-trace-stale-v1/-v2`, nothing
deleted); probe_after values above 125% of cold invalidate a run's absolute level but not
within-run comparisons. The temporary `FISTFORCE_ANIM_OFF` instrumentation in
`client/src/hero/animation.rs` was reverted (`git checkout -- client/src/hero/animation.rs`)
and the client rebuilt from the clean tree; no game-code change is part of this deliverable.

---

# Verification pass — 2026-09-17, same day (Claude, in this worktree)

The DeepSeek report above was checked against the code, its raw logs, and new
measurements. Everything below was run through the same `run_one.sh` harness.

## What holds

- Rig geometry (23 parts, 21 joints, 34 clips, whole wardrobe spawned and hidden),
  cascade config (2 x 2048), the grass-density clamp, the props flag gating grass,
  the crowd ablation (no-villagers 20.7 vs 32.1 ms), the anim-off delta (-3.9 ms),
  shadows/TAA measuring zero on cool runs, and "no leak" (RSS 890-930 MB over 8 min)
  all reproduce from the code and logs.
- The `animate_targets` and render-preparation span totals reproduce from the trace
  thread tables.

## What was wrong

- **"The render thread is mostly idle waiting on the main thread"** is not what the
  trace shows. The `Render` schedule's 22 ms of self time on the render thread is the
  render thread waiting for its *own* systems executing on the task pool:
  `write_indirect_parameters_buffers` (7.0 ms), `write_batched_instance_buffers`
  (6.4), `prepared_mesh_producer` (5.7), `collect_meshes_for_gpu_building` (2.8),
  `extract_skins`/`prepare_skins` (3.6) - about 27 ms of CPU per frame that scales with
  the number of extracted mesh instances. Both threads are busy for ~30 ms; the frame
  is the slower of the two. That is why pausing animation alone bought only 4 ms.
  Consequence: **fix 2 (fewer mesh entities per villager) is the bigger lever, not
  fix 1.** Fix 1 alone cannot get the town below ~28 ms.
- The half-resolution and clouds-off rows were throttled (probe 210/157 vs 82 cold)
  and prove nothing either way; the summary presented them as "no effect".
- The trace (09:00-09:02 local) was captured while the display was off (see below),
  so its absolute frame time carries the same caveat as any display-off run.

## The unresolved item, resolved: why off-screen villagers were "seen"

DeepSeek's empty-forest run (actually open sea, focus `1200,-1200`, 1.5 km from
town) still reported 999 rigs animating and 999 with a part ViewVisible while none
were in the camera frustum. A per-view diagnostic (`FISTFORCE_RIG_VIS_DIAG=1`) showed
two independent causes, each sufficient on its own:

1. **The 2D present camera** (`scaled_target.rs`) sits at the world origin with the
   default orthographic box (viewport-sized, +-1000 m deep). Bevy ORs
   `ViewVisibility` across every camera, so every 3D entity within ~1.4 km of the
   origin was marked visible every frame. Diagnostic: that camera listed 5,290
   `Mesh3d` entities as visible while the 3D camera listed 18.
2. **Sun shadow cascades**: `check_dir_light_mesh_visibility` tests casters with
   `intersect_near = false`, so anything up-sun of the camera counts at any distance.
   Diagnostic: 10,520 cascade entities for the same 18-mesh view; the rig's parts
   were in the cascades.

Both fixed (commit `8ba3ceb1`): `RenderLayers::none()` on the present camera, and
light-only visibility keeps a rig animating only within 250 m of the camera. All 31
`hero::` tests pass. Kill switch for A/B: `FISTFORCE_VIS_FIX_OFF=1`.

Measured, open-sea scene, display on, same binary:

| run | animating | culled | p50 |
|---|---:|---:|---:|
| shadows off only (2D camera still marking) | 999 | 0 | 17.42 |
| shadows off + 2D camera fixed | 0 | 999 | 16.74 |
| 17-rig scenario (floor) | 16 | 0 | 16.67 |
| full fix, shadows on (`ocean-fixed`; census valid, p50 tainted by display-off) | 0 | 985 | 16.83* |

In the lab the off-screen crowd is worth ~0.7-2.5 ms because the sea/sky/UI floor
dominates. The structural point is that every villager inside replication range was
animated and render-prepared regardless of view; with several villages in the 512 m
interest radius that cost scales with population, and the fix makes the existing
cull work.

## Measurement hazard found today: display off / locked screen

Runs whose window ended while the display was off or the Mac was locked measured
**19-22 ms in town instead of 32-33 ms** (`town-fixed`, `town-ab-off2`), with the same
binary and settings, and `town-ab-on1` died with "No windows are open" when the
window was closed. `pmset -g log | grep 'Display is turned'` gives the timeline;
`screencapture -x` during a run shows a black frame or the lock screen. Any
frame-time comparison must be made with the display on, unlocked, and the game
window frontmost. **The town A/B of the fix is therefore still open**: no valid
"fix on" town run exists yet. Expected effect in town is ~0 (the crowd is on screen).
To finish it with the user present:

```sh
logs/perf-audit-2026-09-17/verify_ab.sh   # off/on/off/on, same binary, ~25 min
```

## Every run today (times UTC; `git` is HEAD at run time, uncommitted fix runs show debd3197)

| time | run | scenario | focus | env | p50 mean | p50 min | animating | culled | probe b/a | valid | git |
|---|---|---|---|---|---:|---:|---:|---:|---|---|---|

---

# Follow-up 2026-09-17 afternoon: animation LOD + wardrobe stash (branch `perf-crowd-lod`)

Same harness, dense-stress town, camera locked at `112,-158`, display on and unlocked
(verified via `pmset -g log`), one binary with env kill switches:

| run | env | mesh entities | rigs skipped/frame | p50 | probe after |
|---|---|---:|---:|---:|---|
| lod-base | `FISTFORCE_ANIM_LOD_OFF=1 FISTFORCE_WARDROBE_STASH_OFF=1` | 23,820 | 0 | **33.09** | 84 |
| lod-only | `FISTFORCE_WARDROBE_STASH_OFF=1` | 23,834 | 747 | **30.03** | 94 |
| stash-only | `FISTFORCE_ANIM_LOD_OFF=1` | 5,857 | 0 | **31.58** | 116 (warm) |
| lod-both | (none) | 5,849 | 750 | **28.20** | 83 |

- Animation update-rate LOD (tiers 40/100/200 m -> 20/12/10 Hz): -3.1 ms.
- Despawning unworn wardrobe primitives (stash + rebuild on re-dress): -1.5 ms.
- Together: -4.9 ms (33.1 -> 28.2 ms, ~15%). Effects are additive.
- Tuning knobs: `ANIM_LOD_TIERS` / `ANIM_LOD_FAR_INTERVAL` in `hero/animation.rs`.
  Visual check of the 10 Hz far tier at RTS zoom is still to be done by eye.
- Next lever: one skinned mesh per villager, see `CROWD-MESH-MERGE-PLAN.md`.

## Follow-up: one-mesh-per-villager (Phase A) — implemented, measured, defaulted OFF

Works (1,001 merged rigs, mesh entities 5.8k -> 2.8k, pixel-identical capture, tests
pass) but measures 0-1.5 ms in back-to-back pairs, within noise. Left behind
`FISTFORCE_MERGE_OUTFITS=1`. Details and the corrected reasoning in
`CROWD-MESH-MERGE-PLAN.md`.
