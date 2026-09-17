# Small-town frame budget — 2026-09-18

Branch `perf-fundamentals` (from `perf-crowd-lod`), worktree
`/Users/terminator2/Coding/fistworld-perf-audit`. Machine: MacBook Pro `Mac17,3`, Apple M5,
macOS 26.5.2. Window borderless native 2940x1846; scene offscreen target 1764x1108 at the
default `render_scale = 0.60`.

Scene: `secure` lab (17 villagers), camera locked at `112,-158`, default zoom 280 m, 10x sim
for the first 120 s then 1x. Measurement window: the last 80 s of a 240 s run.
**All runs are uncapped (`FISTFORCE_FRAME_CAP=0`).** Baselines run at **p50 ≈ 20.0 ms,
p95 ≈ 21.8 ms** (≈ 50 fps) with `secure`; the dense-stress town baseline is 28.5 ms.

Status: final except where marked. Sections 1-4 are done; §5 (implemented fix) was still
being measured when this file was last written.

## 0. Measurement validity (decisions and limitations)

Every run records a gate in `logs/perf-fundamentals/<label>.meta.txt`:

- Display: `pmset -g log | grep 'Display is turned' | tail -1` must say `turned on`.
- Frontmost app sampled before, during (every 20 s in `<label>.session.txt`) and after;
  `loginwindow` refuses the run; a stuck `UserNotificationCenter` banner is dismissed once
  (`killall UserNotificationCenter`) and re-sampled.
- Thermal probe before/after (cold 82 ms; start only at ≤100 ms); no run within 5 minutes of
  a build; quiet gate on `cargo`/`rustc`.

Two prescribed checks do not work on this OS build:

1. **`screencapture -x` fails** with `could not create image from display` (Screen Recording
   permission is not granted and cannot be granted without the UI). Every meta records
   `screenshot=unavailable_tcc`; the display log and the per-20 s frontmost samples stand in
   for the "black screenshot" check, and the limitation is carried here rather than hidden.
2. **`ioreg -n Root -d1 -a | grep -c CGSSessionScreenIsLocked` always prints 0** because no
   such key exists in the ioreg output on macOS 26.5.2. `loginwindow` in the frontmost sample
   is the lock signal instead.

No run was rejected by the gate during this audit. All session logs show `front='client'`
while the game window was up.

## 1. The answer

**With nothing but a 17-villager town on screen the frame costs ~20 ms (50 fps), and it is
not one thing — it is a ~10.9 ms main-thread game frame plus a ~20 ms render side that the
main thread then waits ~10 ms for. The single largest code-fixable cost is the cloud-shadow
terrain material sweep at −2.7 ms; the largest knob is the 3D render scale (−3.6 ms at 0.5);
no single subsystem explains more than ~18% of the frame.**

Ranking (paired, uncapped, p50 ms saved; see §2 for both repeats):

| rank | change | Δ p50 | kind |
|---:|---|---:|---|
| 1 | render_scale 0.60 → 0.50 | −3.59 | setting |
| 2 | clouds off | −3.40 | content |
| 3 | **cloud-shadow lane sweep frozen** | **−2.73** | **structural, fixed here** |
| 4 | shadows off | −2.66 (−1.25 clean pair) | quality |
| 5 | props/trees/grass off | −1.60 | content |
| 6 | shadow filter (temporal+TA A) → hw | −1.47 | quality |
| 7 | water surface off (corrected) | −1.33 | content |
| 8 | fog off | −1.24 | quality |
| 9 | TAA off | −0.98 | quality |
| 10 | UI hidden | −0.78 | content |
| 11 | bloom off | −0.75 | quality |
| 12 | atmosphere off | −0.63 | quality |

`empty-sea` (same server, camera at `1200,-1200`, no town) is 15.8-16.2 ms — only ~4 ms
cheaper than the whole small town, so the terrain/water/sky/UI floor is ~16 ms and the town's
buildings, props and villagers add only ~4 ms on top.

Top three fixes for this frame: (1) the cloud-shadow lane sweep (implemented, §5); (2) the
residual render-side CPU — `write_binned_instance_buffers` 3.5 ms + `prepare_windows` 1.6 ms
+ `queue_submit` 3.3 ms per frame; (3) shadow casting reach (shadows cost 1.3-2.7 ms for two
2048² cascades that draw every terrain chunk).

## 2. Uncapped ablation ladder (both repeats, paired)

Each row is a back-to-back pair of 240 s runs; `secure` scenario, camera at `112,-158`,
uncapped. Δ is switch minus the baseline run immediately before it. Probe = thermal probe
before/after the run (cold 82 ms).

| condition | env | rep | base p50 | switch p50 | Δ50 | base p95 | switch p95 | Δ95 | probes |
|---|---|---:|---:|---:|---:|---:|---:|---:|---|
| shadows-off | `FISTFORCE_SHADOWS=0` | 1 | 20.07 | 18.82 | −1.25 | 23.98 | 21.15 | −2.83 | 82/83 |
| shadows-off | | 2 | 22.88 | 18.81 | −4.07 | 29.05 | 22.48 | −6.57 | 81/81 |
| shadow-filter-hw | `FISTFORCE_SHADOW_FILTER=hw` | 1 | 20.09 | 18.54 | −1.55 | 21.89 | 20.10 | −1.79 | 108/82 |
| shadow-filter-hw | | 2 | 20.04 | 18.64 | −1.40 | 21.80 | 20.14 | −1.66 | 82/82 |
| taa-off | `FISTFORCE_TAA=0` | 1 | 20.04 | 19.11 | −0.93 | 21.75 | 20.60 | −1.15 | 84/85 |
| taa-off | | 2 | 20.11 | 19.08 | −1.03 | 21.96 | 20.54 | −1.42 | 82/83 |
| atmosphere-off | `FISTFORCE_ATMOSPHERE=0` | 1 | 20.03 | 19.45 | −0.58 | 21.93 | 21.34 | −0.59 | 82/88 |
| atmosphere-off | | 2 | 20.11 | 19.42 | −0.69 | 22.01 | 21.16 | −0.85 | 80/80 |
| clouds-off | `FISTFORCE_CLOUDS=0` | 1 | 19.99 | 16.58 | −3.41 | 21.92 | 21.10 | −0.82 | 81/82 |
| clouds-off | | 2 | 20.02 | 16.62 | −3.40 | 21.77 | 21.20 | −0.57 | 83/82 |
| cloud-shadow-freeze | `FISTFORCE_CLOUD_SHADOW_FREEZE=1` | 1 | 20.05 | 17.35 | −2.70 | 22.02 | 19.49 | −2.53 | 80/82 |
| cloud-shadow-freeze | | 2 | 20.04 | 17.27 | −2.77 | 22.08 | 19.48 | −2.60 | 81/81 |
| bloom-off | `FISTFORCE_BLOOM=0` | 1 | 20.09 | 19.34 | −0.75 | 21.94 | 21.10 | −0.84 | 81/83 |
| bloom-off | | 2 | 20.03 | 19.29 | −0.74 | 21.66 | 20.96 | −0.70 | 82/83 |
| half-res | `FISTFORCE_RENDER_SCALE=0.5` | 1 | 19.90 | 16.33 | −3.57 | 21.77 | 20.24 | −1.53 | 80/81 |
| half-res | | 2 | 19.74 | 16.13 | −3.61 | 21.62 | 20.17 | −1.45 | 85/84 |
| fog-off | `FISTFORCE_FOG=0` | 1 | 19.76 | 19.00 | −0.76 | 21.57 | 22.13 | +0.56 | 80/83 |
| fog-off | | 2 | 20.00 | 18.28 | −1.72 | 21.82 | 21.34 | −0.48 | 81/81 |
| props-off | `FISTFORCE_PROPS=0` | 1 | 20.05 | 18.45 | −1.60 | 21.80 | 20.53 | −1.27 | 79/81 |
| props-off | | 2 | 19.95 | 18.35 | −1.60 | 21.74 | 20.58 | −1.16 | 83/85 |
| water-off | `FISTFORCE_WATER=0` | 1 | 19.43 | 18.23 | −1.20 | 22.86 | 22.44 | −0.42 | 98/86 |
| water-off | | 2 | 19.66 | 18.20 | −1.46 | 23.45 | 23.06 | −0.39 | 88/84 |
| no-ui | `FISTFORCE_UI=0` | 1 | 20.05 | 18.86 | −1.19 | 21.91 | 20.61 | −1.30 | 78/85 |
| no-ui | | 2 | 19.31 | 18.94 | −0.37 | 21.28 | 20.67 | −0.61 | 85/86 |
| empty-sea | focus `1200,-1200` (dense server) | 1 | 28.48 | 15.81 | −12.67 | 35.63 | 22.02 | −13.61 | 84/88 |
| empty-sea | | 2 | 28.51 | 16.21 | −12.30 | 35.75 | 24.76 | −10.99 | 89/96 |

Notes on the two suspect rows:
- `shadows-off` pair 2's baseline (22.88) is 2.8 ms above every other baseline; the switch
  values in both pairs agree (18.81/18.82), so the clean-pair effect is −1.25 ms and the
  mean is inflated by a contaminated baseline.
- `water-off` originally measured **−8.28 ms**, but that arm also closed the far-terrain
  detail hole (the hole only opens when water entries fill; see
  `client/src/terrain/streaming/far_terrain.rs:210`), hiding ~240 detailed terrain chunks.
  The switch was fixed to keep the hole logic identical (`FISTFORCE_WATER=0` no longer
  affects terrain selection) and the corrected measurement is −1.33 ms. The confounded
  results are kept in `logs/perf-fundamentals/confounded-water-v1/`.

## 3. Trace (uncapped secure town, 150 s, 17 GB, tail 30 s)

`target-trace` built with `bevy/trace,bevy/trace_chrome`; parsed with the streaming
`trace_tail2.py`. Tail: 47.6 fps → 21.0 ms/frame.

**Main thread:** `update:` 21.5 ms = `main app` 10.9 ms + `RenderExtractApp` **10.6 ms**
(self time 9.0 ms — waiting on extraction's parallel work / the render-thread handoff).
Inside `main app`: `PostUpdate` 4.5 (self 4.2), `Update` 3.6 (self 2.8), `PreUpdate` 1.5,
`StateTransition` 0.4, `RunFixedMainLoop` 0.4.

**Render thread:** `Render` schedule **20.2 ms/frame**, of which **10.5 ms is self time**
(waiting inside the schedule), `render_system` 8.5, `camera_driver` 4.9
(`Core3d` 4.4), `submit_pending_command_buffers` 3.3, `queue_submit` 3.3 ms per call
(22 command buffers), `prepare_windows` 1.6. The render thread is not idle-waiting on the
main thread: it waits on its own prepare/queue work and on submission.

**Top merged spans (ms/frame, all threads):**

| span | ms/frame |
|---|---:|
| `write_binned_instance_buffers<core_3d>` | 1.83 |
| `write_binned_instance_buffers<prepass>` | 1.70 |
| `prepare_windows` | 1.65 |
| `prepare_preprocess_bind_groups` | 0.82 |
| `prepare_generated_environment_map_bind_groups` | 0.75 |
| `prepare_erased_assets<MeshMaterial3d<..>>` | 0.74 |
| `bloom` | 0.65 |
| animation `par_for_each` (17 rigs) | 0.51 |
| `prepare_material_bind_groups` | 0.45 |
| `mark_dirty_trees` | 0.40 |
| `propagate_parent_transforms` | 0.39 |
| `write_indirect_parameters_buffers` | 0.37 |
| `cluster_on_gpu` | 0.36 |
| `unpack_bins` | 0.34 |

Reading: the small-town frame is not animation (0.5 ms) and not replication (0.04 ms). It is
render-side per-frame CPU (bins/prepare/submit ≈ 8-10 ms), a ~10.9 ms main game frame, and
the wait the main thread pays for the render side. The cloud-lane material sweep does not
appear as a span (it is `materials.get_mut` churn); the freeze switch bounds it at 2.7 ms.

## 4. Confirmed / Plausible / Rejected per subsystem

### Confirmed (paired numbers in §2)

- **Cloud-shadow lane sweep is the top structural cost: −2.73 ms.** Every
  `materials.get_mut` on a chunk's `TerrainSplatMaterial` re-prepares five uniform buffers
  and a bind group (`cloud_shadows.rs:31-50`). The freeze switch isolates it.
- **3D fill rate matters at this resolution: half-res −3.59 ms** (1764x1108 → 1470x923).
  Unlike the dense-town audit, uncapped small-town frames are not purely CPU-bound.
- **TAA alone −0.98 ms**, and the temporal shadow filter adds ~0.5 (shadow-filter-hw −1.47).
- **Shadows (2 cascades, 2048², 700 m span) ~1.3 ms clean**; all 289 terrain chunks cast
  (no `NotShadowCaster` on chunk meshes, `terrain/streaming/spawn.rs:179`).
- **Props/trees/grass −1.60 ms**; grass instances are 81 batches / 12,228 instances at this
  camera, and grass does not cast (`enable_shadows() = false`,
  `props/ground_cover_instancing.rs:75`).
- **Water −1.33 ms** (corrected; ~115 water chunk meshes at this camera).
- **Fog −1.24 ms** (DistanceFog is the only fog path; removing it in `setup.rs` is the off
  state).
- **UI is small: −0.78 ms uncapped** even at native 2940x1846 with the whole UI hidden.
  The `ClientPerfUi` systems themselves are ~0.4 ms/frame total
  (`sync_compact_panel` 0.14, `sync_portraits` 0.13, others ~0.15).
- **Bloom −0.75 ms**, atmosphere −0.63 ms.
- **The empty sea floor is ~16 ms**; a whole small town adds only ~4 ms on top of it.

### Plausible (mechanism clear, not directly ablated)

- **`prepare_windows` 1.65 ms/frame** (`bevy_render::view::window::prepare_windows`): the
  swapchain/window surface path runs every frame. No switch exists; not attacked here.
- **`queue_submit` 3.3 ms/call for 22 command buffers** — plausibly includes driver-side
  waiting on GPU completion; Metal exposes no GPU timings to Bevy, so this is not separable
  without a Metal capture.
- **Far-terrain mesh cost** is not separately ablated (no switch); it is drawn every frame
  (`NotShadowCaster`) plus the ocean skirt, and `empty-sea` shows the combined floor.
- **The extraction/wait 10.6 ms on the main thread** is a wait on render-side work rather
  than extraction work proper; consistent with the render thread's 10.5 ms self time, but
  not proven with a dedicated experiment.

### Rejected (checked, not a problem)

- **Villager animation**: 17 rigs cost 0.51 ms/frame in the trace; the crowd LOD work on
  `perf-crowd-lod` already covers it.
- **Replication**: `receive_replication` 0.04 ms/frame, lightyear receive 0.06 ms.
- **UI layout/rebuild churn**: no system rebuilds every frame (`0r` in every `ClientPerfUi`
  window), and hiding the entire UI saves <1 ms.
- **Atmosphere LUT rebuilds**: the scattering medium rebuilds only on a real blend change,
  throttled to 0.35 s (`atmosphere.rs:294-303`); atmosphere-off is −0.63 ms total.
- **Cloud layer material churn**: the cloud plane and its material are diff-gated
  (`cloud_layer.rs:26-34`); the layer itself is only ~0.7 ms (clouds-off −3.40 vs freeze
  −2.73).
- **Water material writes**: one shared water material, written only on the same gated
  cadence as terrain (`cloud_shadows.rs:277-284`).
- **Per-frame terrain material rewrites at 1x**: `ClientPerfChangedMeshes` shows
  `std_material=0` per frame; the only writer is the cloud-lane sweep above.

## 5. Implemented fix and its A/B

**Chosen: the cloud-shadow terrain lane sweep** (largest code-fixable cost that is not a
quality setting), behind `FISTFORCE_CLOUD_LANE_QUIET=1`:

- `LaneGate` widens the diff gates (anchor 1 s → 4 s, sun projection 0.01 → 0.05, coverage
  0.005 → 0.02, strength 0.005 → 0.02, speed 0.01 → 0.04).
- A new snapshot is published only when the previous sweep has reached every live chunk
  (`sweep_busy`), so a fast trigger stream can no longer re-upload the same chunk materials
  every frame. Lanes are recomputed from live state each frame, so skipped snapshots lose
  nothing.
- The shaders already extrapolate cloud drift and sun sweep from the anchor, so motion stays
  continuous; only the periodic correction lags (bounded by 4 s).

Tests: `cargo test --profile playtest -p client` → **621 passed, 0 failed**.

A/B (uncapped, `secure`, two pairs; dense-stress one pair) and the capture pixel diff are
filled in below when the runs finish.

(Commands and reproduction are in §6; this section is completed last.)
