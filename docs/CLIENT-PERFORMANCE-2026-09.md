# Client maintenance and Bevy 0.19 review — 2026-09-05

Scope: small client-side waste and streaming fixes. The user confirmed that
Blender renders for another project were sharing this Mac and requested a focus
on low-hanging fruit. FPS comparisons were stopped. This pass does not establish
an FPS gain or certify a minimum Mac specification.

## Changes retained

- **Finish terrain geometry on the existing workers.** Shoreline subdivision,
  attribute construction and tangent fallback previously happened on the main
  thread after the asynchronous base-height calculation. Workers now return a
  complete Bevy `Mesh`; the main thread registers it with `Assets<Mesh>`.
  Geometry, normals, paint, resolution and rendering settings are unchanged.
- **Budget the actual finalization.** The 8 ms normal / 30 ms bootstrap guard
  previously bounded only polling completed tasks. Asset creation happened in
  an unbudgeted second loop. Each ready result is now finalized before checking
  the deadline again. Unpolled results stay queued. Existing chunk replacements
  remain an atomic old/new swap, and count limits still bound deferred work.
  This is a soft CPU budget: one indivisible chunk can overrun it, and later
  render-world uploads are outside this system's timer.
- **Select the next prop/grass chunk directly.** These systems only process one
  chunk, but allocated lists and sorted them every frame. They now select the
  minimum existing priority without those lists/sorts. Priority, density,
  streaming radius and visibility rules are preserved.
- **Release diagnostic strings.** The opt-in changed-mesh census leaked every
  component-name string using `String::leak()`. It now owns and drops them.
- **Keep unsupported Metal counters disabled.** Bevy 0.19's render diagnostics
  provide CPU timings on Metal; its GPU recorder supports Vulkan/DX12. Turning
  diagnostics on no longer unnecessarily enables Metal timestamp/pipeline
  counters that this recorder cannot use.

## Bevy 0.19 details that affect future work

Reviewed the resolved 0.19.0 registry sources, including `bevy_asset::Assets`,
`bevy_render::PipelineCache`, `bevy_winit::WinitSettings`, render diagnostics,
and `bevy_pbr::ExtendedMaterial`, alongside the official
[Bevy 0.19 release notes](https://bevy.org/news/bevy-0-19/).

- Grass already uses GPU instance buffers and uploads them only on revision
  changes. Adding instancing from scratch is not the missing optimization.
- `Assets::get_mut` returns an `AssetMut` guard. An immutable read through that
  guard does not itself dirty the asset; mutable dereferencing does. Some older
  comments in the repo describe the pre-0.19 behavior.
- Pipeline compilation is synchronous on macOS in the pinned Bevy source.
  First-use material/vertex variants can therefore stall. Future warmup work
  should check pipeline readiness and exercise real variants.
- The new partial-bindless Metal path is not automatically enabled for the
  game's custom extensions. `ExtendedMaterial` requires both the base and
  extension to opt in. Terrain and wind-foliage extensions currently do not.
  Converting their bindings is a separate shader change requiring profiling
  and visual verification, not part of this maintenance pass.

## Repeatable measurements for a quiet machine

The new `capture --benchmark` mode and `capture/performance_flights.py` are
documented in [VISUAL-CAPTURE.md](VISUAL-CAPTURE.md#measuring-frame-pacing).
They retain fixed simulation inputs but measure wall-clock frame intervals,
exclude warmup and screenshot readbacks, disable both frame caps and Winit's
unfocused event-loop throttle, and use an owned offscreen presentation target
instead of a hidden macOS swapchain. The report includes actual scene extent,
resolved graphics settings, per-frame CPU counters and tail-latency summaries.

Early exploratory runs were unsuitable for FPS conclusions: they inherited
background event-loop throttling and/or hidden-window presentation waits, and
Blender was concurrently rendering. Do not use those numbers as a baseline.
The planned paired release comparison was not run after the user's clarification.

## Verification

- `cargo check --workspace --all-targets`: passed, including the final source.
- `cargo test --workspace`: **822 passed**, 11 explicitly ignored, no failures.
  Added coverage includes the complete worker-built shoreline mesh layout and
  wall-clock percentile reporting without hiding long frames.
- `cargo build --workspace --profile playtest`: passed.
- `cargo build --release -p client --bin capture`: passed.
- Formatting and diff whitespace checks passed; no new compiler warnings.
- The three `forest-floor.ron` shots passed their existing image comparison
  thresholds. Mean image error was below 0.0018, with no baseline updates.
- Two real continuous 1,200-frame flights produced 13 PNG/JSON probes each:
  forest zoom 52–2400 and sandy river-mouth traversal. Inspected representative
  PNGs at close, wide, map, outbound and return positions, and every sidecar.
  No terrain holes appeared in the inspected probes. Loaded terrain stayed at
  286–289 chunks in the zoom sweep and 289–306 at the coast. The final entity
  counts returned to their starting values (3,742 forest; 2,028 coast).

Local evidence is under `/tmp/fistworld-client-performance-20260905/`, especially
`candidate-forest-visual/`, `candidate-forest-flight/`, `candidate-coast-flight/`,
`tests.log`, and `final-check.log`. These are rendering/streaming checks without
networking or NPC simulation; no connected gameplay behavior changed.
