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
main thread then waits ~10 ms for. The frame is spread across fill-rate and small per-frame
render costs: the largest single knob is the 3D render scale (−3.6 ms at 0.5), and the
largest single feature cost is cloud shading on terrain + sky (−3.4 ms together, of which
~2.7 ms is the terrain/water cloud-shade fragment path). No single low-risk code change
beyond ~1 ms was found; the one that looked like it (the cloud-lane material sweep) was
implemented, measured twice on `secure` and once on `dense-stress`, and had no effect
(§5) — its apparent cost was a mislabeled diagnostic.**

Ranking (paired, uncapped, p50 ms saved; see §2 for both repeats):

| rank | change | Δ p50 | kind |
|---:|---|---:|---|
| 1 | render_scale 0.60 → 0.50 | −3.59 | setting |
| 2 | clouds off (sky layer + terrain cloud shade) | −3.40 | content |
| 3 | cloud shading on terrain/water (freeze diagnostic) | −2.73 | content/fill | <!-- see §2 note; not material writes -->
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

Top three *levers* for this frame, all quality/content tradeoffs measured here:
(1) render scale 0.5 (−3.6 ms); (2) cloud shadows (terrain shading ~2.7 ms + sky layer
~0.7 ms); (3) props/trees/grass (−1.6 ms), water (−1.3 ms), shadow casting (−1.3 ms), fog
(−1.2 ms), TAA (−1.0 ms). The residual render-side CPU (`write_binned_instance_buffers`
3.5 ms, `prepare_windows` 1.6 ms, `queue_submit` 3.3 ms) is Bevy/driver plumbing and the
GPU backpressure point, not client code.

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
- `cloud-shadow-freeze` does **not** measure the material lane writes, despite its name.
  It returns after the first lane write, so only the first sweep's 48 chunk materials carry
  real cloud parameters; the other ~241 chunks keep `stylized_palette()`'s default
  `clouds_b = ZERO` (`shared/src/terrain/material.rs:239-243`) and the terrain fragment
  shader skips its cloud-shade block (`terrain_splat.wgsl:728`,
  `if palette.clouds_b.z > 0.0005`). The −2.73 ms is therefore cloud *shading* on ~83% of
  the terrain, not the cost of writing uniforms. The actual write cost is the quiet-mode
  A/B in §5: **zero**.

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

- **Cloud shading on terrain costs ~2.7 ms, and the sky layer ~0.7 ms more.** Measured by
  the freeze diagnostic (terrain shade off, §2 note and §5) and clouds-off. Each
  `materials.get_mut` on a chunk material does re-prepare five uniform buffers and a bind
  group (`cloud_shadows.rs:31-50`), but the A/B in §5 shows that write path costs ~0 ms;
  the 2.7 ms is the per-fragment cloud field in `terrain_splat.wgsl:728-741`.
- **3D fill rate matters at this resolution: half-res −3.59 ms** (1764x1108 → 1470x923).
  Unlike the dense-town audit, uncapped small-town frames are not purely CPU-bound.
- **TAA alone −0.98 ms**, and the temporal shadow filter adds ~0.5 (shadow-filter-hw −1.47).
- **Shadows (2 cascades, 2048², 700 m span) ~1.3 ms clean**; all 289 terrain chunks cast
  (no `NotShadowCaster` on chunk meshes, `terrain/streaming/spawn.rs:179`).
  `FISTFORCE_RIG_VIS_DIAG=1` on the secure town: the main 3D view lists **330 meshes**; the
  sun's cascades list **1,167 caster entries** per frame (of which 15 are villager parts);
  the fill light casts 0. The 2D present camera lists **0** meshes (the off-screen
  visibility bug from the previous audit is fixed on this branch).
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

## 5. Implemented change and its A/B — a measured negative, reverted

**What was implemented.** The apparent top structural cost was the cloud-shadow terrain
lane sweep, so it was made quiet behind `FISTFORCE_CLOUD_LANE_QUIET=1`: a `LaneGate` widened
every diff gate (anchor 1 s → 4 s, sun projection 0.01 → 0.05, coverage/strength 0.005 →
0.02, speed 0.01 → 0.04) and a new snapshot was published only after the previous sweep had
reached every live chunk. The shaders extrapolate drift from the anchor, so motion stays
continuous with a lagged correction. `cargo test --profile playtest -p client`: 621 passed,
0 failed.

**A/B (uncapped, paired, 240 s runs each):**

| pair | env | base p50 | quiet p50 | Δ50 | base p95 | quiet p95 | Δ95 |
|---|---|---:|---:|---:|---:|---:|---:|
| secure, repeat 1 | `FISTFORCE_CLOUD_LANE_QUIET=1` | 19.41 | 19.95 | **+0.54** | 20.64 | 21.14 | +0.50 |
| secure, repeat 2 | | 20.11 | 20.20 | **+0.09** | 21.52 | 21.41 | −0.11 |
| dense-stress | | 28.40 | 28.40 | **0.00** | 35.52 | 37.58 | +2.06 |

**Pixel diff** (capture scenario `logs/perf-fundamentals/cloud-shadow-diff.ron`, forced
`cloudy`, 1400x900, midday; `quiet-off.png` vs `quiet-on.png`): max **17/255**, mean 0.18,
p99 5, p99.9 11, **0.29 % of pixels changed > 8/255**, overall brightness unchanged. So the
change does alter the image slightly (lagged anchors) while buying **no** frame time.

**Decision: reverted** (commit `07e4f561`). The measurement shows the lane writes cost ~0 ms
at 1x — the freeze diagnostic's −2.73 ms was the terrain cloud-shade fragment path, not the
writes (§2 note). What remains is correctly attributed in §1/§4; no low-risk code change
above ~1 ms was found in this frame, so the actionable levers are the measured quality
settings (render scale, cloud shadows, props, water, shadows, fog, TAA).

## 6. Commands (reproduction)

```sh
# build (pinned toolchain; the machine shares rustup with other agents)
RUSTUP_TOOLCHAIN=stable-aarch64-apple-darwin cargo build --profile playtest -p server -p client

# uncapped small-town run (one condition); harness enforces display/lock/thermal gates,
# writes <label>.meta.txt / .session.txt / .summary.txt into logs/perf-fundamentals/
logs/perf-fundamentals/run_uncapped.sh <label> secure            # baseline
logs/perf-fundamentals/run_uncapped.sh <label> secure FISTFORCE_SHADOWS=0   # example switch

# the whole ladder (13 conditions, each baseline+switch twice, retries invalid sessions)
logs/perf-fundamentals/ladder_uncapped.sh
python3 logs/perf-fundamentals/collect_uncapped.py

# server env used by every run
CITYSIM_MAP_ID=village_lab FISTWORLD_VILLAGE_LAB_RUNTIME=1 FISTWORLD_LAB_SCENARIO=secure \
FISTWORLD_LAB_WARP=10 FISTWORLD_DEV=1 ./target/playtest/server

# chrome trace (separate target dir; both features are required)
CARGO_TARGET_DIR=target-trace cargo build --profile playtest -p client \
  --features bevy/trace,bevy/trace_chrome
logs/perf-fundamentals/trace_run_small.sh          # 150 s, uncapped, TRACE_CHROME=trace.json
python3 logs/perf-audit-2026-09-17/trace_tail2.py logs/perf-fundamentals/trace.json 30

# the quiet-lane A/B and its pixel diff
logs/perf-fundamentals/cloud_lane_pair.sh
BEVY_ASSET_ROOT=$PWD/client/assets ./target/playtest/capture \
  --scenario logs/perf-fundamentals/cloud-shadow-diff.ron
BEVY_ASSET_ROOT=$PWD/client/assets FISTFORCE_CLOUD_LANE_QUIET=1 ./target/playtest/capture \
  --scenario logs/perf-fundamentals/cloud-shadow-diff.ron   # then diff the two PNGs

# tests
cargo test --profile playtest -p client      # 621 passed, 0 failed
```

The generated reports, logs, traces and captures live under ignored
`logs/perf-fundamentals/`; this document and the kill-switch/fix commits are the tracked
deliverable.
