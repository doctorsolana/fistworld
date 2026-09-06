# Visual capture and regression testing

> **Required agent workflow:** repository coding agents discover this contract through
> `AGENTS.md` and `CONTRIBUTING.md`. A visual change is not finished after `cargo check`:
> run the smallest representative scenario, inspect its PNG and `.capture.json`, and report
> what was actually observed. Use continuous capture for motion/streaming bugs and the real
> connected lab for authoritative NPC behavior.

The capture harness boots the real Bevy 0.19 client renderer without networking. It uses the
same terrain streaming, water, props, atmosphere, cameras, assets and retained UI as gameplay.
It is intended to answer questions a compile cannot: whether a shoreline tears while panning,
an animation faces the wrong way, a menu clips, or an LOD transition changes the world.

The implementation separates orchestration from fixture data:

- `client/src/capture.rs` owns application setup, camera movement, readiness and sequencing.
- `client/src/capture/live.rs` owns the connected voyage and Village Lab hooks.
- `client/src/capture/inspection.rs` reads shared world counters and live camera evidence;
  `presentation.rs` owns the offline scene-and-UI render target.
- `world_fixture.rs`, `scene_fixtures.rs`, `ui_fixtures.rs` and `history_fixtures.rs`
  under `client/src/capture/` stage deterministic inputs for the production systems.
- `client/src/capture_artifact.rs` owns scenario RON, readiness, semantic assertions, Bevy
  screenshot observers, PNG/JSON artifacts and baseline comparison.

The screenshot entity is completed by Bevy's `ScreenshotCaptured` observer. The app never polls
the filesystem to guess when rendering is finished and never exits while the last PNG is still
being encoded. The connected Village Lab and voyage hooks use the same observer mailbox.

## Quick use

From the repository root:

```bash
BEVY_ASSET_ROOT="$PWD/client/assets" \
cargo run --profile playtest -p client --bin capture -- \
  --at -226,-163 --preset survey --out /tmp/fistworld-survey

# A checked-in, repeatable contract:
BEVY_ASSET_ROOT="$PWD/client/assets" \
cargo run --profile playtest -p client --bin capture -- \
  --scenario capture/scenarios/world-survey.ron
```

Every shot writes two files:

- `shot.png`: the image a person reviews;
- `shot.capture.json`: map, Git revision, deterministic timestep, camera, dimensions, world
  counters, readiness duration, assertion results and visual-comparison metrics.

On a failed visual comparison it also writes `shot.diff.png`. The process exits non-zero when a
readiness gate times out, a semantic assertion fails, image writing fails, or a baseline exceeds
its configured tolerances.

Run `cargo run -p client --bin capture -- --help` for all one-off flags. Existing presets and
fixture environment variables remain supported.

## Scenario format

Scenarios belong under `capture/scenarios/` and are versioned. `storage-hall.ron` stages the
authored warehouse from three angles; `storage-hall-door.ron` records one continuous open/close
cycle. The latter uses `FISTFORCE_CAPTURE_DOORS=cycle` to alternate the ordinary building-side
door demand every two simulated seconds, exercising the production animation consumer.

Other checked-in references include
`world-survey.ron` for zoom-band scene rendering, `river-banks.ron` for the detailed-river/far-
river handoff and inland ground paint, `forest-floor.ron` for forest accents at morning and noon,
`isolated-fern.ron` for a single shipped fern with all surrounding props suppressed,
`lighting-readability.ron` for people/buildings across three daylight angles, and
`ui-company.ron` / `ui-companies.ron` for real retained-UI fixtures. Character maintenance
checks include `character-work.ron` (one continuous animation sequence),
`character-cargo.ron` (five authored carried bundles) and `porter-cart.ron` (loaded cart).

```ron
(
    version: 1,
    name: "shoreline-regression",
    map: "big_world",
    output_dir: "logs/captures/shoreline-regression",
    resolution: (1600, 900),
    fixed_delta_seconds: 0.016666667,
    target: scene,
    show_window: false,
    warmup_frames: 240,
    readiness: (
        minimum_frames: 60,
        maximum_frames: 1200,
        minimum_loaded_chunks: 64,
        stable_loaded_chunk_frames: 20,
    ),
    environment: {
        "FISTFORCE_CAPTURE_CLOUDS": "clear",
    },
    comparison: Some((
        baseline_dir: "capture/baselines/shoreline-regression",
        update_baselines: false,
        pixel_threshold: 0.04,
        maximum_mean_error: 0.01,
        maximum_changed_fraction: 0.02,
    )),
    shots: [
        (
            name: "shore",
            focus: (-226.0, 0.0, -163.0),
            yaw: -0.45,
            zoom: 105.0,
            time_of_day: 0.5,
            assertions: [
                loaded_chunks_at_least(count: 64),
                entities_at_least(count: 100),
            ],
        ),
    ],
)
```

Unknown fields are rejected. This catches misspelled test settings instead of silently running a
different experiment.

`environment` is applied before Bevy plugins or assets initialize. It is the declarative form of
the existing capture fixtures such as `FISTFORCE_CAPTURE_SETTLEMENT`,
`FISTFORCE_CAPTURE_HERO`, `FISTFORCE_CAPTURE_DINGHY`, `FISTFORCE_CAPTURE_ENCYCLOPEDIA`,
`FISTFORCE_CAPTURE_PAUSE`, and `FISTFORCE_CAPTURE_PERMIT_PLACEMENT`. These are trusted local
developer inputs, not data accepted from a multiplayer server.

## Readiness and assertions

Frame counts alone are not proof that an async streamed world is ready. A shot is taken only when:

1. its minimum frame count has elapsed;
2. at least `minimum_loaded_chunks` detailed terrain chunks exist; and
3. the chunk count has stayed unchanged for `stable_loaded_chunk_frames`.

The run fails after `maximum_frames` rather than hanging forever. A shot may override the scenario
readiness block. Continuous `streaming-flight` scenarios deliberately bypass per-shot readiness:
their purpose is to photograph handoff frames while the camera never pauses.

Supported semantic assertions are:

- `loaded_chunks_at_least(count: N)`
- `entities_at_least(count: N)`
- `villagers_at_least(count: N)`
- `settlements_at_least(count: N)`
- `planning_routes_at_most(count: N)`
- `blocked_routes_at_most(count: N)`

Assertions are evaluated immediately before rendering and are preserved in JSON even on failure,
so a pretty screenshot cannot pass while its intended population or world state is absent.

## Window, offscreen scene and diagnostics

`target: window` captures the production presentation camera: scaled 3D scene plus
native-resolution UI. In the offline capture app this camera renders into an owned image,
and a visible harness window mirrors it. Captures therefore remain usable on a locked
desktop or during swapchain changes. Use it for HUD, encyclopedia, property, company and
interaction fixtures; `show_window: false` keeps the same composed output without a mirror.

The connected Village Lab can select its offscreen 3D image with
`FISTWORLD_LAB_CAPTURE_TARGET=scene`. It waits for stable loaded terrain after the requested
world day and warmup. Live composed-window capture still requires a usable desktop window.

`target: scene` captures the game's `SceneRenderTarget` image directly at 100% render scale. It is
independent of fullscreen mode, monitor scale and swapchain behavior, and excludes UI. Add
`--hidden` for a display-independent offscreen run that does not show the OS window. Bevy still
owns a hidden primary window/event loop; this is intentionally more compatible with the real
client than a separate miniature renderer. A machine still needs a graphics adapter/display
backend supported by wgpu.

Scenario diagnostics can enable:

```ron
diagnostics: (
    performance_overlay: true, // F3 overlay; window target only
    gizmos: true,              // F4 planning/collider visuals
    render_timings: true,      // Bevy render timing logs
),
```

The command-line equivalents are `--perf-overlay`, `--gizmos`, and
`--render-diagnostics`. Prefer a clean color capture plus a separate diagnostic capture rather
than approving a baseline with volatile FPS text.

## Measuring frame pacing

Use a release build for performance decisions. The normal capture clock advances
by a fixed simulation timestep, so its FPS overlay is **not** a wall-clock
benchmark. Screenshot readbacks also disturb timing. `--benchmark` runs a
nonempty continuous scenario with frame limiting, vsync and Winit's background
event-loop throttle disabled, skips PNG readbacks, and writes `performance.json`
with real frame intervals, percentiles,
slow-frame counts and the preceding frame's terrain/prop CPU counters. Warmup
is excluded. Comparison and recording cannot be combined with this mode.
Benchmark presentation uses an owned offscreen image even for `target: scene`,
avoiding hidden macOS swapchain waits. Benchmark mode always hides the window
and omits its presentation mirror.

```bash
python3 capture/performance_flights.py /tmp/fistworld-flights
cargo build --release -p client --bin capture
BEVY_ASSET_ROOT="$PWD/client/assets" target/release/capture \
  --scenario /tmp/fistworld-flights/forest-mid.ron --benchmark --out /tmp/forest-timing
# Run the identical moving path with PNG/JSON probes for visual verification:
BEVY_ASSET_ROOT="$PWD/client/assets" target/release/capture \
  --scenario /tmp/fistworld-flights/forest-mid.ron --out /tmp/forest-visual
```

The generator covers forest close/mid/wide/map views, a zoom sweep, and sandy
river-mouth traversal on Showcase/91. Each 1,200-frame path crosses chunk
boundaries and repeats its circuit. Resolution is 1920×1080 at 100% scene scale;
`--resolution` can override it. The sun advances deterministically too. Keep
the same path, resolution, graphics settings and binary build profile for paired
runs; record hardware, environment overrides and binary revisions separately.
Avoid compilation or other heavy jobs during timing, and repeat each case.

These are offline renderer measurements, not a connected NPC/server workload or
proof of performance on another Mac. Metal does not provide GPU timestamp
results through Bevy 0.19's render diagnostics; frame intervals measure application
throughput and scheduling/render backpressure. A benchmark never substitutes
for personally inspecting the normal run's PNG and `.capture.json`.

## Visual baselines

Create or deliberately replace baselines:

```bash
cargo run --profile playtest -p client --bin capture -- \
  --scenario capture/scenarios/world-survey.ron \
  --update-baselines capture/baselines/world-survey
```

Compare a later build:

```bash
cargo run --profile playtest -p client --bin capture -- \
  --scenario capture/scenarios/world-survey.ron \
  --compare capture/baselines/world-survey
```

Comparison is RGB and resolution-sensitive. A pixel is “changed” when any channel exceeds
`pixel_threshold`; the run then checks both normalized mean absolute error and changed-pixel
fraction. Dimension mismatches always fail. Failed comparisons retain the new PNG, metadata and
a blue-to-red difference image. Never loosen thresholds merely to absorb nondeterministic input:
first pin map, timestep, camera, weather and readiness in the scenario.

## Deterministic recording

Bevy 0.19's recorder is an opt-in native x264 dependency, so ordinary game and screenshot builds
do not pay its compile cost:

```bash
cargo run --profile playtest -p client --features capture-video --bin capture -- \
  --scenario capture/scenarios/world-survey.ron --target scene --record
```

Recording starts after warmup, advances virtual time by an exact frame duration, stops after the
last artifact, and keeps the app alive while the encoder flushes. Raw `.h264` files land under the
scenario output's `video/` directory. They can be remuxed without re-encoding:

```bash
ffmpeg -framerate 30 -i FistForce\ Capture-123456789.h264 -c copy capture.mp4
```

Bevy 0.19 does not support this x264 recorder on Windows. PNG scenarios and comparisons remain
cross-platform. On macOS/Linux the Bevy feature links a system x264 installation; on a Homebrew
Mac install its build prerequisites once with `brew install pkgconf x264`. Recording requires a
`scene` artifact target: Bevy records the presented primary window while the harness captures the
separate scene image, avoiding two same-frame screenshot requests for one target.

## Connected simulation captures

The offline harness is correct for rendering and UI fixtures; it is not a replacement for a real
server when the subject is NPC behavior.

For the rendered Village Lab, set `FISTWORLD_LAB_CAPTURE_DAY`,
`FISTWORLD_LAB_CAPTURE_PATH`, optional `FISTWORLD_LAB_CAPTURE_ZOOM`, and
`FISTWORLD_LAB_CAPTURE_EXIT=1` before `./run.sh testworld`. The authoritative world day gates the
shot and the observer writes matching JSON metadata. Live metadata reads the replicated map,
world day, population, routes and loaded terrain at the request. It also records the commander
controller and actual root camera position/rotation, including cinematic overrides.
`fixed_delta_seconds: 0` identifies a live run rather than a deterministic offline timestep.

When launching the binaries directly, set the same `CITYSIM_MAP_ID` for both client and server;
`run.sh` normally does this for you. Live capture rejects differing map IDs or content hashes
before writing an artifact. `FISTWORLD_LAB_CAPTURE_TARGET=scene` selects the owned 3D target when
the desktop swapchain is unavailable. The requested survey zoom remains fixed throughout warmup
so a late restored commander view cannot replace the framing.

For the real create-Hero/opening-voyage path, set
`FISTWORLD_VOYAGE_CAPTURE_DIR=/absolute/output/directory` and optionally
`FISTWORLD_VOYAGE_CAPTURE_EXIT=1`. It records face hold, camera travel, final RTS framing and an
ordinary right-click sailing result from the live offscreen scene target.

For an unattended voyage check, start a fresh local server and use an unused profile name.
Run these in separate terminals after the playtest build:

```bash
CITYSIM_MAP_ID=big_world target/playtest/server

CITYSIM_MAP_ID=big_world BEVY_ASSET_ROOT="$PWD/client/assets" \
FISTFORCE_NO_SETTINGS_FILE=1 FISTFORCE_AUTOCONNECT=CaptureVoyage \
FISTWORLD_AUTOCREATE_VOYAGE=1 \
FISTWORLD_VOYAGE_CAPTURE_DIR=/tmp/fistworld-voyage \
FISTWORLD_VOYAGE_CAPTURE_EXIT=1 target/playtest/client
```

An unattended NPC observer can use the existing `FISTWORLD_AUTOSPAWN_HERO=1` smoke mode to
suppress the new-player creator. Without server god capability, that flag only skips the creator;
it does not grant a hero. Inspect the village image itself: replicated population counters can
be correct while a modal or creator preview occupies the view. The normal client propagates
Bevy's exit status, so a live capture failure returns a nonzero process status.

## Adding a regression

1. Reproduce it with the smallest existing fixture or scenario.
2. Add a named RON scenario if camera, world state or assertions matter.
3. Use readiness gates that describe the actual dependency; do not add an arbitrary huge sleep.
4. Capture both the clean scene and, when useful, a separate gizmo/performance view.
5. Inspect the PNG and JSON. Only then update a baseline.
6. Run `cargo test -p client capture_artifact` and the scenario itself.

Keep production fixes outside the harness. A capture fixture may stage deterministic data, but it
must exercise the same renderer/UI code as gameplay rather than implementing a second visual path.

## Connected army formation scenario

`capture/scenarios/army-250.ron` uses the shared `ArmyLabScenario` schema rather
than the offline `CaptureScenario` schema. Both real binaries read it through
`FISTWORLD_ARMY_SCENARIO`. It supplies the account, map, five 50-person battalions,
initial layout, two deployment frontages, camera and timeout. This opt-in fixture
bypasses the new-player creator; it does not create local client soldiers or write
client positions. Membership is created through the authoritative army handler.

After `cargo build --workspace --profile playtest`, run these in separate terminals
from the repository root (stop only the processes you start):

```sh
env RUST_LOG=info CITYSIM_MAP_ID=battle_lab FISTWORLD_VILLAGE_LAB_RUNTIME=1 \
  FISTWORLD_LAB_SCENARIO=skirmish FISTWORLD_LAB_WARP=1 \
  FISTWORLD_ARMY_SCENARIO="$PWD/capture/scenarios/army-250.ron" \
  ./target/playtest/server
```

Wait for the server's bound/listening log, then:

```sh
env RUST_LOG=info CITYSIM_MAP_ID=battle_lab \
  FISTWORLD_ARMY_SCENARIO="$PWD/capture/scenarios/army-250.ron" \
  FISTFORCE_AUTOCONNECT=armylab BEVY_ASSET_ROOT="$PWD/client/assets" \
  FISTFORCE_NO_SETTINGS_FILE=1 FISTFORCE_RENDER_SCALE=1 \
  FISTFORCE_START_FOCUS=-15,-25 FISTFORCE_START_ZOOM=125 \
  FISTWORLD_ARMY_CAPTURE_DIR=/tmp/fistworld-army-250 \
  ./target/playtest/client
```

The client waits for all 250 members, five complete rosters, dressed character
models, unblocked UI and stable terrain. It selects the battalions and drives
normal RMB press/drag/release input. Each deployment records preview, movement,
arrival and selected UI; the run remains connected continuously. Arrival requires
all 250 replicated positions within 0.3 m of their own slots and all facings within
0.06 radians. Those are verification bounds, not changed visual comparison tolerances.

Each PNG has the normal `.capture.json` plus `.army.json` with selected/member
counts, movement/arrival/facing counts and per-person measured/expected positions.
The client exits successfully only after every deployment and file write succeeds;
timeout writes `failure.json` and exits with an error. Inspect images and both JSON
files. The army lab uses the existing owned presentation image for `target: window`,
so its composed HUD capture also works when a macOS swapchain returns black.
