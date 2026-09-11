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
  These also inspect ordinary seeded worlds: `FISTWORLD_VOYAGE_CAPTURE_LANDING=1`
  adds a fifth shot after an ordinary inland right-click, disembark and walk to an
  inhabited Hall. `FISTWORLD_LAB_CAPTURE_ACTIVITY=settlement` frames the nearest actual
  Hall after the opening cinematic and waits for dressed residents; no lab fixture or
  God access is required.
- `client/src/capture/session.rs` owns opt-in connected player-session inspection and
  input driving. `capture/first_session.py` exercises creation, arrival, trading and
  reconnect with ordinary production input handlers and authoritative replicated state.
- `client/src/capture/inspection.rs` reads shared world counters and live camera evidence;
  `presentation.rs` owns the offline scene-and-UI render target.
- `world_fixture.rs`, `scene_fixtures.rs`, `ui_fixtures.rs` and `history_fixtures.rs`
  under `client/src/capture/` stage deterministic inputs for the production systems.
- `town_fixtures.rs` imports an actual server growth snapshot, including terrain
  earthworks, through the normal settlement/road rendering systems. See
  [TOWN-GROWTH-LAB.md](TOWN-GROWTH-LAB.md) and `capture/town_growth.py`.
- `client/src/capture_artifact.rs` owns scenario RON, readiness, semantic assertions, Bevy
  screenshot observers, PNG/JSON artifacts and baseline comparison.

Capture runs set `FISTFORCE_NO_SETTINGS_FILE=1`: this disables **both reading and
writing** `client_data/settings.ron`. The same session policy applies to connected
test runs that set the flag. Fixture-only changes (hidden scenery, window size and
100% scene scale) must never overwrite the player's preferences.

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

## Ordinary connected first-session regression

Build both binaries together (market messages include a protocol-versioned purchase price
ceiling), then run against a free local UDP port 5000:

```bash
cargo build --workspace --profile playtest
python3 capture/first_session.py run --seed 12345 \
  --out logs/first-session/seed12345 --resolution 1600x1000
python3 capture/first_session.py run --seed 918273 \
  --out logs/first-session/seed918273 --resolution 1280x720 --soak-seconds 1800
```

Each output directory must be fresh. The runner starts and stops only its own processes,
isolates settings, disables developer access, and preserves server/client logs plus a
`report.json`. It does not grant money, manufacture stock, teleport actors or alter prices.
On macOS it holds an AC-power sleep assertion for its own server process; the assertion
ends with the run and does not change saved power preferences. Battery exhaustion or
manual suspension must not be counted as connected play time.
The one-time character creation is clicked through the normal creator; movement uses
right-click orders and market tests press the actual retained BUY/POST buttons. The
optional sustained phase visits ordinary generated towns without disconnecting.
It records authoritative positions in `movement.jsonl`, periodically returns the
camera to the walking hero and captures the streamed view. An accepted order
that makes no progress for 90 seconds fails the run; long journeys receive a
distance-based arrival budget.

`FISTWORLD_SESSION_CAPTURE_DIR` enables the driver only in an explicitly isolated test
session (`FISTFORCE_NO_SETTINGS_FILE=1`). A separate controller can submit numbered JSON
commands using `capture/first_session.py command DIRECTORY '{"action":"key","key":"Home"}'`.
Supported keys are Home, E, N, M and Escape. Other commands click a named visible button,
right-click a terrain point, adjust only the camera view, capture, or quit. `status.json`
publishes hero, market, UI and camera evidence once per second; `reply-N.json` reports
command completion. A disabled-button attempt returns a failure without applying a trade.

Captures use the connected composed presentation image, wait for loaded chunks and
settled camera XZ/zoom, and complete through Bevy's screenshot observer. Inspect **both**
the PNG and `.capture.json`; `.session.json` adds the replicated gameplay state. These are
live-world evidence, not deterministic pixel baselines: the actual market and NPCs continue
to run. All outputs belong under ignored `logs/`, never in the asset tree or Git.

## Scenario format

The meadow tree scenarios use the production foliage shader and actual exported LOD meshes.
`meadow-trees-trio.ron` shows the three new species from two sides; `meadow-trees-lineup.ron`
compares both LODs against an existing oak and pine. Their `FISTFORCE_CAPTURE_TREES` fixture
waits for the meshes and materials before staging specimens. `meadow-trees.ron` checks real
generated meadow placement, morning light and a pine forest. `meadow-trees-flight.ron` moves
continuously out and back through a meadow with changing zoom, recording every 15 frames.
See `asset_creation/MEADOW_TREES.md` for the asset and colour-mask contracts.

Scenarios belong under `capture/scenarios/` and are versioned. `storage-hall.ron` stages the
authored warehouse from three angles; `storage-hall-door.ron` records one continuous open/close
cycle. The latter uses `FISTFORCE_CAPTURE_DOORS=cycle` to alternate the ordinary building-side
door demand every two simulated seconds, exercising the production animation consumer.

The six individual storage hall, lumberjack and house scenarios also include low-angle
`under-eaves-front` and `under-eaves-side` shots. These expose missing roof backing;
`storage-hall.ron` additionally checks cargo and post contact in `loading-bay-contact`.
Both level-2 house scenarios include `porch-joints`, keeping the entrance framing
in view separately from the upper roof. The compact and long level-1 low front
shots already show these joints.
Their free-look eye heights use the generated fixture elevation (8.885553 m), because
the capture camera's `eye` is measured above the local water level, not terrain.

`lumberjack-hut.ron` checks the timber workshop from front/rear, at midnight, and at
gameplay/town zoom. `lumberjack-hut-door.ron` records its ordinary door-demand cycle.

`bakery.ron` checks the bakehouse front/rear, oven, roof and canopy undersides,
night lighting and gameplay scale. The bakery fixture levels its plot with the
normal building footprint/terrain blend contract. `bakery-door.ron` records a
continuous demand-driven door cycle with the counters and entrance visible.
See [the bakery handover](../asset_creation/BAKERY.md) for stock and light contracts.

`tavern.ron` covers the inn, courtyard, dormer, night lighting and roof undersides;
`tavern-door.ron` continuously exercises its door. The opt-in connected eight-guest
review follows authoritative purchases, seating and departure. See
[the tavern handover](../asset_creation/TAVERN_PROCEDURAL_HANDOVER.md) for launch
commands and shared furniture/navigation contracts.

`house-cabin-l1.ron`, `house-cabin-l2.ron`, `house-long-l1.ron` and
`house-long-l2.ron` inspect each occupied home at daylight, midnight, rear and gameplay
zoom. `houses-lineup.ron` compares both families and upgrade levels in one view;
`houses-doors.ron` continuously exercises all four door clips. Their fixture uses
`FISTFORCE_CAPTURE_HOUSES=cabin-l1|cabin-l2|long-l1|long-l2|lineup` and the production
`HouseAppearance`, household lighting and door-demand consumers. Each individual house scenario also
places a dressed character beside the entrance and includes an `entrance-scale`
view at eye height. It has no connected
villager simulation; use the client/server lab for NPC journeys.

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
- `settlement_buildings_at_least(count: N)` (completed building roots, excluding worksites)
- `planning_routes_at_most(count: N)`
- `blocked_routes_at_most(count: N)`

Assertions are evaluated immediately before rendering and are preserved in JSON even on failure,
so a pretty screenshot cannot pass while its intended population or world state is absent.

### Vegetation zoom recovery

`capture/vegetation_zoom.py` generates two continuous close → map → close round trips
on the reported Stoneham terrain (`world` seed `11272639609695457076`) and the maintained
`big_world` forest reference. Each scenario has 661 frames and 23 PNG/JSON probes,
including recovery periods after each return. They use ordinary procedural vegetation;
the offline Stoneham view contains no server-created settlement fixture.

```sh
python3 capture/vegetation_zoom.py logs/captures/vegetation-scenarios
BEVY_ASSET_ROOT="$PWD/client/assets" target/playtest/capture \
  --scenario logs/captures/vegetation-scenarios/stoneham-vegetation-roundtrip.ron
BEVY_ASSET_ROOT="$PWD/client/assets" target/playtest/capture \
  --scenario logs/captures/vegetation-scenarios/forest-vegetation-roundtrip.ron
```

Metadata records `world.prop_roots`, `tree_roots`, `visible_tree_roots` and
`grass_batches`. Visible tree roots have both local and inherited visibility enabled;
this count does not prove that a mesh passed camera frustum/occlusion culling. Grass
batches count instanced render entities, not individual tufts. Inspect the PNGs too.
`prop_roots_at_least/at_most`, `visible_tree_roots_at_least/at_most` and
`grass_batches_at_least/at_most` accept `(count: N)`. The generated scenarios require
vegetation before and after both round trips and no props/batches at full map zoom.
Assertions run on steady probes, after deferred streaming and visibility propagation
have had time to follow the camera; they do not assert on the transition frame.

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

### Fluid formation regression

Use `capture/scenarios/army-fluid.ron` in both binaries with the same army launch
recipe. It widens three 50-person battalions to 25 files/two ranks, then uses an
ordinary ground click (`preserve_shape: true`) before a quarter-turn redeployment.
The client checks every replicated arrival and facing. Drag previews capture the
composed window so the footprint and rank-count label can both be inspected.
`battle-3v3.ron` also verifies selection expansion from one member per battalion
before its normal engage-line attack. These runs are functional/visual checks,
not performance benchmarks.

## Connected clash scenarios

`battle-1v1.ron`, `battle-2v1.ron`, `battle-3v1.ron` and `battle-skirmish.ron`
use the same launch recipe above, replacing the scenario path in both binaries
and the account with `FISTFORCE_AUTOCONNECT=battlelab`. Use a fresh server per run,
`FISTFORCE_RESOLUTION=1600x900`, and a separate output directory. The scenario
owns camera framing. Both sides are ordinary server characters, discharged into
retinues and mustered through the real handlers; defenders receive Hold.

`battle-animation.ron` is the close-up version: eight soldiers per side, a short
approach and twenty seconds of real combat after first contact. Use it when
changing attack, recoil or death clips. Review the PNGs alongside each
`.battle.json` impact/reaction timestamp: an offline fatal pose alone cannot prove
that replication and mortality leave enough time for the animation to finish.

The client waits for the complete replicated population, dressed rigs, owned
roster, stable terrain and unblocked UI. It selects the attacking battalions and
issues an ordinary enemy click. A matching cursor ray is essential: a ground-only
cursor override would silently test Move instead. The server's accepted-attack
feedback is required for success. In the mixed scenario the client subsequently
orders one independent person, then two more, into the ongoing clash.

The connected run continuously captures the owned scene image with PNG and
`.capture.json`. A `.battle.json` beside each frame records positions, health,
battalion/owner, confirmed targets and absolute swing/reaction timestamps. Capture
cadence increases from 2 world seconds during approach to 0.25 seconds after first
contact. `summary.json` requires accepted Attack, contact by every configured
attacking battalion and independent person, casualties and completed captures.
These are functional checks: personally inspect the approach, contact, casualty
replacement and late fight to assess crossing, crowding, poses and stalls.
Screenshot readbacks and compilation disturb timing; these runs are not FPS
benchmarks. Use `army-250.ron` as the separate march/redeployment regression.

For a separate timing run, set `FISTWORLD_BATTLE_METRICS_ONLY=1` on the client,
along with `FISTFORCE_CLIENT_PERF=1`; enable `CITYSIM_NET_DEBUG=1` on the server.
The driver still takes its initial readiness/selection screenshot (two initial
views for cavalry), then records one JSON position sample per simulated second
without recurring GPU readbacks or PNG encoding. The cavalry contact camera
still moves to its close view; use the separate phase statistics below.
Its summary marks `metrics_only`, distinguishes `samples` from `shots`, and keeps
the gameplay assertions. This mode is not a replacement for visual verification.
Exclude startup/pipeline warmup from timing interpretation, retain the render
settings and concurrent machine workload, and use the server's measured phase
costs rather than treating its scheduled 16.7 ms tick interval as CPU time.
`battle-5v5.ron` extends the same check to 500 soldiers.

`summary.json.frame_timing` records raw `Time<Real>` frame intervals for approach,
normal combat and the cavalry close view, plus an overall distribution. It
includes frames while screenshot tickets are pending, but excludes readiness and
the initial screenshots. Each nonempty phase includes sample count, summed wall
seconds, mean FPS, mean/p50/p95/p99/maximum milliseconds and frames over 35 ms.
The ordinary `ClientPerf` snapshot also uses raw real time, so the simulation's
delta clamp cannot hide a long frame. These are complete frame intervals,
including VSync/software pacing, capture overhead and scheduling by the OS;
they do not isolate GPU execution or main-thread CPU work.
`frame_timing.replicated_clock` separately counts rendered frames in which the
replicated absolute world clock stays unchanged while warp is positive, its
longest unchanged wall interval and its largest forward step. This diagnoses
packet-paced presentation separately from rendering FPS; explicit pauses reset
the comparison and do not count as clock stalls.
`frame_timing.presentation_clock` records the same diagnostics for the cosmetic
animation clock after its update, making visible any difference between packet
arrival cadence and actual animation time. `frame_timing.window_focus` counts
focused/unfocused/unknown window states for exactly the accepted timing samples.

For paired timing runs, finish compilation first, keep the same binary profile
and scenario, and force `FISTFORCE_NO_SETTINGS_FILE=1`,
`FISTFORCE_DISPLAY_MODE=windowed`, `FISTFORCE_RESOLUTION=1600x900`,
`FISTFORCE_RENDER_SCALE=1`, `FISTFORCE_VSYNC=0` and `FISTFORCE_FRAME_CAP=0`.
Confirm actual render dimensions in the initial `.capture.json`: fullscreen on
Retina can use a larger native target than the requested logical resolution.
Compare matching phase/sample population rather than averaging overlapping
600-frame `ClientPerf` windows. For minimal extra diagnostics,
`FISTFORCE_PROFILE_HITCHES=1` enables frame summaries without the mesh census
enabled by `FISTFORCE_CLIENT_PERF=1`; leave detailed census/network tracing for a
separate diagnostic run. Record other machine workloads alongside results.

Use `FISTWORLD_BATTLE_BENCHMARK=1` for explicitly uncoupling a connected battle
test from Winit's unfocused-window wait policy. This flag applies continuous
event-loop updates only when a connected battle scenario is active, and the
summary records `benchmark: true`. It leaves ordinary VSync and frame-cap
settings in force, so an uncapped comparison also needs `FISTFORCE_VSYNC=0` and
`FISTFORCE_FRAME_CAP=0`. `FISTWORLD_BATTLE_METRICS_ONLY=1` by itself preserves
normal event-loop pacing. Compare focus counts and use the same benchmark flag
on both builds; a background-window timing result is not directly comparable to
a focused or continuous-mode run.

### Crowded combat and mid-fight orders

`battle-crowded.ron` starts three 50-person attacking battalions with overlapping
approaches to one defending battalion. The client reissues an army attack eight
seconds after first contact through the same mouse/order path. Scenario offsets
only stage the initial fixture; runtime movement and collision remain production
systems. Its summary also requires the mid-fight order to have been issued.

After a connected run, use:

```sh
python3 tools/analyze_battle_capture.py /path/to/capture-directory
```

`movement-analysis.json` records individual contact participation, travel before
contact, substantial direction reversals, close body overlaps, and unsupported
five-second idle windows. Waiting within three metres of an engaged ally and five
metres of an enemy is classified as support. These are diagnostics to inspect
alongside the PNG sequence, not blanket requirements for every rear-rank soldier
to fight or an FPS measurement. Compare the same scenario and observation window;
casualties and target availability can legitimately alter contact participation.

### Hands-on three-versus-three battle

For the larger **manual five-versus-five** fixture, use `./run.sh battle5v5`.
It builds the client/server pair, supplies `battle-5v5.ron` only to the server,
waits for the network listener and opens the client as `battlelab` with combat
mode and a hero. Closing the client or interrupting the launcher stops its own
server. Both logs are retained in `logs/battle5v5-*`; an already occupied server
port is reported without stopping that session. This is a hands-on launcher,
not an automatic verification run. The connected capture recipes above remain
the way to run assertions and record the full battle automatically.

`battle-3v3.ron` stages three 50-person battalions per side, 44 m apart. For
manual play, give **only the server** `FISTWORLD_ARMY_SCENARIO`; setting it on
the client runs the automated attack/capture/exit sequence instead. Start a fresh
server using the connected launch recipe above with this scenario, then launch:

```sh
env -u FISTWORLD_ARMY_SCENARIO CITYSIM_MAP_ID=battle_lab \
  FISTFORCE_AUTOCONNECT=battlelab FISTWORLD_AUTOSPAWN_HERO=1 \
  FISTWORLD_AUTOSPAWN_AT=-32,-72 FISTFORCE_COMBAT_MODE=1 \
  FISTFORCE_START_FOCUS=-20,-36 FISTFORCE_START_ZOOM=100 FISTFORCE_START_YAW=0 \
  BEVY_ASSET_ROOT="$PWD/client/assets" ./target/playtest/client
```

The server also needs `FISTWORLD_DEV=1` for the existing hero spawn hook. The
player receives 150 mustered soldiers and a hero behind the line; the 150 enemy
soldiers Hold until engaged. No client driver issues attacks, locks the camera
or exits the session. Normal Army-page membership/stance controls remain live.

## Connected catapult rehearsal

`capture/scenarios/catapult.ron` uses the connected army-lab runtime. Rebuild both
binaries after wire changes. Launch server and client with `CITYSIM_MAP_ID=battle_lab`
and `FISTWORLD_ARMY_SCENARIO="$PWD/capture/scenarios/catapult.ron"`; the server also
needs `FISTWORLD_VILLAGE_LAB_RUNTIME=1 FISTWORLD_LAB_SCENARIO=skirmish FISTWORLD_LAB_WARP=1`.
On the client set `FISTFORCE_AUTOCONNECT=battlelab`,
`BEVY_ASSET_ROOT="$PWD/client/assets"`, `FISTFORCE_NO_SETTINGS_FILE=1`,
`FISTFORCE_RESOLUTION=1600x900 FISTFORCE_RENDER_SCALE=1` and a fresh
`FISTWORLD_ARMY_CAPTURE_DIR`.

Readiness requires replicated actors, instantiated catapult rig, dressed people,
unblocked input and stable terrain chunks. The rehearsal uses normal RMB movement,
F + RMB bombardment, H, then RMB enemy attack. Scene captures record the moving carriage,
launch, flight and impact continuously; window captures include the aiming and status
panels. Inspect each PNG with its `.capture.json` and `.siege.json`. The summary checks
travel, authoritative velocity (network snapshot arrival intervals are not movement
speed), wind-up, two impacts, multiple damaged people, ammunition and a full held reload.

The second shot uses an opt-in cinematic camera that follows the elevated projectile;
ordinary RTS focus stays grounded. Capture metadata records the actual camera transform.
At most four screenshot tickets are outstanding, and every ticket must complete before
the rehearsal exits. The requested launch cadence is 24 Hz, but readback can reduce it;
encode recordings from the `.siege.json` timestamps rather than assuming a fixed frame
rate. This rehearsal is visual/functional evidence, not a frame-rate benchmark.

## Army management and standing-policy rehearsal

`capture/scenarios/ui-army.ron` renders the actual retained Army page with two battalions,
assigned and unassigned troops. Use `target: window`; inspect the PNG and `.capture.json`.
A smaller-window check can use `--resolution 1280x720 --out /tmp/army-ui-small` with the
same scenario. Health/stance/checkbox changes must bind without replacing controls.

`capture/scenarios/army-management.ron` requires the real connected lab. Rebuild both
binaries (the stance contract changes the protocol). Launch the server with:

```sh
CITYSIM_MAP_ID=battle_lab FISTWORLD_VILLAGE_LAB_RUNTIME=1 \
FISTWORLD_LAB_SCENARIO=skirmish FISTWORLD_LAB_WARP=1 \
FISTWORLD_ARMY_SCENARIO="$PWD/capture/scenarios/army-management.ron" \
./target/playtest/server
```

Launch the client with the same `CITYSIM_MAP_ID` and `FISTWORLD_ARMY_SCENARIO`, plus
`FISTFORCE_AUTOCONNECT=armylab`, `BEVY_ASSET_ROOT="$PWD/client/assets"`,
`FISTFORCE_NO_SETTINGS_FILE=1`, `FISTFORCE_RESOLUTION=1600x900`,
`FISTFORCE_RENDER_SCALE=1` and a fresh `FISTWORLD_ARMY_CAPTURE_DIR`.

After actors, UI text and terrain are ready, the rehearsal activates the production
Army buttons: bulk remove/refill, transfer, remove/reassign, and Hold line. It waits for
each server-replicated roster transition rather than assuming a network delay. The
server fixture then orders two real enemy catapults to fire once, one at each stance.
Fixture soldiers have 300 health so all survivors' positions can be compared. Damage,
responses, collision, navigation and movement use the production systems.

UI checkpoints use window captures; the bombardment and repositioning sequence uses
continuous scene captures at a modest cadence. `.army.json` records positions, health,
membership and world time beside each PNG/renderer `.capture.json`. `summary.json`
requires completed membership edits, policy replication, damage to both battalions,
at least 8 m travel by every Defensive troop and less than 0.05 m by every held troop.
The final capture must complete before exit. This is not a performance benchmark.

The connected presentation image follows physical window size and DPI, including the
launcher-to-fullscreen transition. Its dimensions can therefore differ from the requested
`FISTFORCE_RESOLUTION` in borderless mode; inspect the recorded dimensions. Use
`FISTFORCE_DISPLAY_MODE=windowed` when the connected run needs an exact output size.
Offline RON scenarios keep their explicitly requested artifact dimensions.

### Verified 2026-09-06

Inspected the Army page at 1600x900 and 1280x720, then ran the connected scenario
through the production button handlers and network messages. The passing run used
24 soldiers (12 per battalion) and two enemy catapults. Eight soldiers in each
battalion took splash damage; every Defensive soldier travelled at least 12.904 m,
and the maximum Hold line displacement was 0.000 m. The continuous captures show
the Defensive line reforming on new ground while the held line remains in place.
Bulk remove/refill, an inter-battalion transfer and its return, and policy replication
all passed. This exercises button actions, not native pointer hit-testing.

Local artifacts are under `logs/captures/army-management/`: `before/`, `after/`,
`small/` and the full passing `connected/` sequence with PNGs, capture/army sidecars
and `summary.json`. Readiness includes bound row labels and completed text updates;
the roster count alone can precede the deferred row binding. The rehearsal also
exposed a row-replacement/disabled-button command ordering bug. A regression test
now covers switching destination battalions while old row actions become invalid.

Verification completed with `cargo check --workspace --all-targets`,
`cargo test --workspace` (873 passed, 11 existing ignored),
`cargo build --workspace --profile playtest`, formatting and whitespace checks.


## Graphics controls and native display modes

`capture/scenarios/ui-graphics.ron` captures the real graphics panel at 1600x1000;
override `--resolution 1280x720` to check the smaller layout. The companion
`ui-graphics-confirmation.ron` checks the real Keep/Revert panel at 720p. Its offline
fixture holds the confirmation timer at 15 seconds without changing the native
window or graphics preferences. Both use `target: window` for the composed UI.

For native fullscreen verification, keep the computer unlocked, start an ordinary
local server, then run the connected client from the repo root with:

```sh
BEVY_ASSET_ROOT="$PWD/client/assets" \
FISTFORCE_NO_SETTINGS_FILE=1 FISTFORCE_AUTOCONNECT=DisplayReview \
FISTWORLD_AUTOSPAWN_HERO=1 FISTFORCE_DISPLAY_MODE=windowed \
FISTWORLD_DISPLAY_CAPTURE_DIR="$PWD/logs/captures/display-modes" \
  ./target/playtest/client
```

This opt-in driver activates the production UI button handlers, checks Windowed,
Borderless and Exclusive modes, adjusts scene and supported output resolution,
confirms changes, reverts to the confirmed borderless mode and finally returns to
Windowed. It exits with failure on timeout or invalid capture. Read `summary.json`,
the PNG/`.capture.json` pairs and `.display.json` native-mode evidence together.
The latter records winit's actual fullscreen state, monitor video mode, content size
and scene render target after 12 stable frames. On macOS, borderless content can be
shorter than the monitor's video mode because Cocoa excludes the notch safe area.
An output step is explicitly skipped if the monitor has only one eligible mode.
The driver waits before sending input when no active primary monitor is reported;
macOS reports no active displays while the session is locked.
These connected screenshots use `target: scene`; use the offline scenarios above
for UI layout. This exercises real action handlers, not native pointer hit-testing,
and `FISTFORCE_NO_SETTINGS_FILE=1` prevents any read or write of player preferences.

## Shared UI theme gallery and motion rehearsal

`capture/ui_gallery.py` writes named RON scenarios for the real encyclopedia tabs,
company controls (top/bottom), ledger, founding form, property board, market, compact
settlement panel, pause/settings, world map, developer panel, hero creator, front-end
menus and populated combat bar. It does not generate alternate UI layouts.

```sh
python3 capture/ui_gallery.py /tmp/fistworld-ui-gallery
BEVY_ASSET_ROOT="$PWD/client/assets" ./target/playtest/capture \
  --scenario /tmp/fistworld-ui-gallery/people.ron --out /tmp/ui-people
BEVY_ASSET_ROOT="$PWD/client/assets" ./target/playtest/capture \
  --scenario /tmp/fistworld-ui-gallery/army.ron --resolution 1280x720 --out /tmp/ui-army-small
BEVY_ASSET_ROOT="$PWD/client/assets" ./target/playtest/capture \
  --scenario capture/scenarios/ui-theme-tour.ron --out /tmp/ui-tour
```

The tour starts after terrain readiness, then captures an uninterrupted 661-frame
sequence at a fixed 60 Hz, with a window PNG and renderer JSON every three frames.
It closes/reopens the encyclopedia, hovers and activates Places/Army/Companies/People,
and asserts that the production action handlers show exactly one matching page.
`.ui.json` adds the tab, visible pages, panel size/translation and input method.
The offscreen presentation camera cannot use Bevy's window-only hit tester; the tour
injects semantic `Interaction` plus input edges after Focus, and does not overwrite
navigation state to fabricate success. It is motion and action coverage, not a claim
of native mouse automation or a performance benchmark. Membership/network behavior
remains covered by the connected army-management scenario above.

`FISTFORCE_CAPTURE_FRONTEND=menu|name` selects the real launcher/name-entry state;
these front-end captures require no terrain chunks. All other gallery pages retain
the normal terrain readiness requirement. Inspect each PNG and its JSON together.

### Theme verification, 2026-09-06

Inspected all 21 gallery views at 1600x900, plus Army, People, company controls,
ledger, market, graphics and controls at 1280x720. PNGs and renderer sidecars are in
`logs/captures/ui-theme/after/` and `small/`; five pre-change references are in `before/`.
The continuous tour passed all 221 capture probes and its navigation/hover assertions.
Inspected arrival, tab-change and reopened frames and encoded the sequence as
`logs/captures/ui-theme/ui-theme-tour.mp4`. The tour exercises production actions with
semantic input, not native pointer hit-testing.

The connected run exposed a stale presentation image after the launcher entered
Retina fullscreen. Fixed its size/DPI synchronization and added a regression test.
The repeat in `connected-retina/` has 22 inspected-metadata captures at 2940x1846;
the overview, transfer and Hold line PNGs now show the controls and scrollable rosters
inside the frame. The passing 24-soldier/two-catapult summary records bulk remove/refill,
transfer roundtrip and stance replication, eight damaged soldiers in each battalion,
12.904 m minimum Defensive travel and 0.000 m maximum Hold line travel. The earlier
`connected/` artifacts preserve the oversized mirror defect for comparison.

Final verification: `cargo check --workspace --all-targets`, `cargo test --workspace`
(880 passed, 11 existing ignored), `cargo build --workspace --profile playtest`,
formatting and whitespace checks. No frame-rate conclusions were drawn while the
machine was also rendering another project.

### Windmill asset inspection

`windmill.ron` isolates a staffed, supplied mill on leveled terrain and checks
front/rear, rotor-facing, gameplay, midnight and low roof/doorway views.
`windmill-motion.ron` runs 481 continuous frames, samples every 30 frames and
changes the replicated clock to exercise cap yaw as well as sail rotation. The
last section moves close to the doorway while `FISTFORCE_CAPTURE_DOORS=cycle`
drives the production door consumer. The fixture is selected with
`FISTFORCE_CAPTURE_SETTLEMENT=windmill`; it does not simulate connected NPC entry.
The free-look cameras use the fixture elevation 8.885553 m. Inspect each PNG
with its JSON, including the full rotation and the low threshold views.


### Civic hall progression

`civic-moot.ron`, `civic-village.ron` and `civic-town.ron` stage one actual
settlement root at its authored level. Each checks front/rear, normal zoom,
night glass, entrance and a low side view. `civic-lineup.ron` compares all three
at one camera scale. `civic-doors.ron` runs a continuous 271-frame open/close
cycle through `BuildingDoorDemand`, probing every 30 frames.

```sh
BEVY_ASSET_ROOT="$PWD/client/assets" target/playtest/capture   --scenario capture/scenarios/civic-lineup.ron --out logs/captures/civic-halls/lineup
```

The fixture reserves/levels the same largest-hall footprint as founding, and
uses the real settlement scene, material and door consumers. The human in the
individual art views is a scale reference. This offline fixture does not
simulate immigration, service queues or connected NPC threshold crossing.

### Rural workplaces and fields

`rural-farmstead.ron`, `rural-livestock.ron`, `rural-quarry.ron` and
`rural-church.ron` select `FISTFORCE_CAPTURE_RURAL` fixtures. They use the actual
building visual, window-light and door consumers, with real walkable wheat-field
or sheep-pasture entities where applicable. Plots use the shared flattening
bounds and grade; the supplied worker roster enables occupied night windows.
These fixtures have building entities rather than civic settlement roots, so
`settlements_at_least` does not describe their readiness.

Each scenario inspects day/night, rear, grounds, gameplay zoom, entry and roof
views. `rural-doors.ron` stages all four buildings and continuously drives the
normal `BuildingDoorDemand` consumer for 271 frames, probing every 30 frames.
Inspect the PNGs and JSON. These offline views do not simulate worker travel,
harvesting, services or connected NPC entry. Sources and budgets are documented
in [RURAL_BUILDINGS.md](../asset_creation/RURAL_BUILDINGS.md).

## Character animation and equipment review

`character-work-review.ron`, `character-motion-review.ron`,
`character-armour-review.ron`, `character-deaths-review.ron`,
`character-rest-review.ron` and `character-swim-review.ron` record uninterrupted
2.5-second sequences through the production character driver. They wait for
`HeroDressed` as well as terrain readiness. `character-creator.ron` checks the
composed equipment selector with the mail preset selected. Full asset rebuild,
clip ownership and stable wardrobe indices are documented in
[CHARACTER_HANDOVER.md](../asset_creation/CHARACTER_HANDOVER.md).

The connected Village Lab accepts `FISTWORLD_LAB_CAPTURE_ACTIVITY=rest` with its
usual capture flags. It waits for a **replicated** `LyingDown` resident and focuses
that person. Pair with `FISTFORCE_AUTOTIME_PRESET=night` in a local dev lab to
exercise the real nighttime routine. A work-pose fixture is not proof that a
civilian has reached its authoritative workplace, and the swim fixture does not
certify connected input/navigation; use the movement tests and connected lab too.

### Bow asset review

`character-archery-review.ron` stages three ordinary dressed characters with
`BowEquipped` and a single clock-sampled `BowShot`. The body and bow/string use
their production animation systems. Readiness includes both wardrobe and bow
graph setup. This is an asset presentation fixture; it does not assert ranged
combat or arrow damage. See `asset_creation/ARCHERY_HANDOVER.md`.

### Connected archery

`battle-archers.ron` requires arrows released, in-flight projectiles, active
body/bow animation and an archer sidearm transition during an infantry
countercharge. `battle-mixed-archers.ron`
checks three battalions per side with archers behind infantry. Their `.battle.json`
files include roles, remaining arrows, bow release times and sampled projectile
positions. `archery_visuals` records body clip selection, playhead, weight, bow
readiness and motion. `battle-archers-close.ron` frames the entire firing line;
`battle-archer-animation.ron` uses four archers and zoom 10 to make hands, draw
and release readable. Inspect these close PNGs as well as the wider fight.
`army-archers.ron` checks frontage/movement and the tactical HUD;
`army-archer-management.ron` checks the retained Army page, bulk membership,
transfers and bombardment stances with archer battalions.
The generic melee contact/casualty pass alone does not prove archery works.

### Horse model proportions

`horse-model.ron` inspects the current rest-pose horse GLB from above, front,
side, rear and three-quarter views. `FISTFORCE_CAPTURE_ASSET` is a pre-gameplay
asset review fixture: it loads the ordinary Bevy world asset, skin and materials,
requires loaded dependencies plus a ready scene instance, and fails on a load
error or readiness timeout. It does not assert animal behaviour, animations or
mounting. See `asset_creation/animals/HORSE_HANDOVER.md` for the explicit unfinished
integration boundary and source/export vertex counts.

The 2026-09-09 narrower-body pass was inspected in Blender and in all five real
Bevy PNG/JSON pairs under `logs/captures/horse-top-final/`.

For animated horse/rider inspection, generate continuous scenarios:

```sh
python3 capture/horse_animations.py logs/captures/horse-animation-scenarios
BEVY_ASSET_ROOT="$PWD/client/assets" target/playtest/capture --scenario logs/captures/horse-animation-scenarios/horse_graze.ron --out logs/captures/horse-animations/graze
```

These scenarios select named exported clips with `FISTFORCE_CAPTURE_ASSET_CLIP`.
`FISTFORCE_CAPTURE_RIDER_CLIP` attaches the exported default character outfit to
`Anchor_Rider`. Readiness waits for both scene instances and animation graphs.
The fixture advances clip time through continuous shots and samples rider gait
cycles using the horse's normalized phase. Mount/dismount hold their final pose.
This remains an offline asset review, not proof of connected mounting behavior.

### Town growth and fortifications

`fortifications.ron` reviews closed palisade/stone meshes, gate clearance and roof
undersides from five angles, with `fortification_sections_at_least` assertions.
`ui-civic-capacity.ron` photographs the Hall's population-based public positions
using an explicitly synthetic UI population; it is not a growth simulation.

`capture/town_growth.py` imports actual server snapshots, including residential
wards, defense reservations and completed sections. It adds a residential-quarter
view and, when available, a completed gateway view. Scene readiness includes
matching section meshes. Inspect the PNG and `.capture.json` for every view.

Connected labs accept `FISTWORLD_LAB_CAPTURE_FORTIFICATIONS=<minimum>` and
`FISTWORLD_LAB_CAPTURE_ACTIVITY=gate`. The latter waits for a rendered completed
gate and holds that framing during terrain streaming. Enable server
`FISTWORLD_LAB_DEFENSE_TRACE=1` to count observed embodied NPC/hero gate crossings;
teleports and representation changes do not count. Offline fixtures verify
rendering, while connected movement is separate behavioral evidence.

`civic-square.ron` reviews the terrain-painted public apron, Hall-facing Market
and adjoining house fronts from three angles. It deliberately stages presentation
inputs rather than pretending to grow a successful economy. A reserved square
also excludes decorative ground cover through the shared client build-zone index;
changed/removed square bounds dirty only their affected grass chunks.

## Wildlife captures

`capture/scenarios/wild-horses.ron` is a continuous four-horse fixture using the
production wildlife consumer. The `horses_at_least` and `horse_rigs_at_least`
assertions verify records and ready animation rigs; both counts are preserved in
`world.horses` and `world.horse_rigs` in capture metadata. The fixture waits for
all four rigs before its normal terrain readiness gate.

`FISTWORLD_WILDLIFE_CAPTURE_DIR` enables a connected natural-herd smoke test.
It observes grazing and authoritative position changes, captures the herd,
checks zero rigs at wide zoom, and returns to the same horse identities. See
[WILDLIFE.md](WILDLIFE.md) for commands and the boundary between offline
presentation proof and connected behavioral proof.

## Cavalry captures

`./run.sh cavalryworld` opens the manual mounted-unit lab. It supplies
`capture/scenarios/battle-cavalry.ron` only to the server: two eight-rider wings
and eight infantry against 32 infantry. For automatic verification, use the
connected battle recipe above with this scenario on both binaries,
`FISTFORCE_AUTOCONNECT=battlelab`, and a fresh ignored `FISTWORLD_ARMY_CAPTURE_DIR`.

The continuous connected run requires all 16 rider/horse visual pairs ready,
every rider travelling at least 10 m, mounted speed above 3.5 m/s, a mounted melee
impact, accepted ordinary attack input, whole-battalion selection, all three
battalions engaging and casualties. It records an initial close view and close
combat views. `.battle.json` adds `cavalry_visuals`; `summary.json` records paired
readiness, per-person travel, speed and riders with impacts. Inspect these along
with each selected PNG and `.capture.json`.

For deterministic presentation and the retained Army page:

```sh
BEVY_ASSET_ROOT="$PWD/client/assets" ./target/debug/capture --scenario capture/scenarios/cavalry-visuals.ron
BEVY_ASSET_ROOT="$PWD/client/assets" ./target/debug/capture --scenario capture/scenarios/ui-army-cavalry.ron
```

`cavalry-visuals.ron` stages two production-mounted riders, samples their gait,
zooms to the static mounted representation and returns to the same identities.
`horse_rigs_at_most(count: 0)` proves the wide view removed animated horse rigs;
the fixture also requires visible proxies and riders. Each probe adds a
`.cavalry.json` with pairing, representation and clip evidence. This is rendering
proof; it does not simulate horse movement or damage on the client.

`ui-army-cavalry.ron` uses a role-only roster fixture at 1280×720 to check cavalry
labels, equipment controls and compatible reinforcements in the actual Army page.
See [CAVALRY.md](CAVALRY.md) for gameplay scope and limitations.
