//! Offscreen visual capture — screenshots of the real renderer, without a server.
//!
//! Compiling is not proof. Terrain winding, shader artifacts, foliage orientation,
//! lighting and material bugs all build clean and only show up on screen, so this binary
//! exists to make "does it actually look right?" answerable without a human watching.
//!
//! It boots the client's real rendering stack (terrain, water, props, sky, atmosphere)
//! but skips networking entirely: `WorldTerrain` loads the map from disk, and the
//! commander camera is the streaming anchor, so no server is needed. A local `WorldTime`
//! entity stands in for the replicated one so the time of day is controllable.
//!
//! Run it via `cargo run -p client --bin capture -- --help`.

mod history_fixtures;
mod inspection;
mod live;
mod performance;
mod presentation;
mod scene_fixtures;
mod ui_fixtures;
mod world_fixture;

pub(crate) use live::{
    drive_live_lab_capture, drive_live_voyage_capture, drive_live_voyage_click_input,
    LiveVoyageCaptureState,
};

use crate::camera_rts::CommanderCamera;
use crate::capture_artifact::{
    git_commit, request_capture, CaptureAssertion, CaptureAssertionResult, CaptureCameraMetadata,
    CaptureComparisonConfig, CaptureCompletions, CaptureMetadata, CaptureReadiness, CaptureTarget,
    CaptureWorldSnapshot, CaptureWriteRequest, CAPTURE_METADATA_VERSION,
};
use bevy::prelude::*;
use bevy::render::view::screenshot::Screenshot;
use inspection::CaptureInspection;
use scene_fixtures::{
    drive_capture_dinghy, spawn_capture_dinghy, spawn_capture_heroes, spawn_capture_isolated_prop,
    stage_capture_permit_placement,
};
use shared::components::WorldTime;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use ui_fixtures::{
    force_capture_drag_box, open_capture_business_management, open_capture_history,
    open_capture_world_map, position_capture_company_scroll, scroll_capture_business_page,
    select_capture_person, select_capture_place,
};
use world_fixture::{enter_world_offline, exercise_capture_door};

/// One camera placement to photograph.
#[derive(Debug, Clone)]
pub struct Shot {
    /// Used as the output filename stem.
    pub name: String,
    /// World-space point the camera centers on.
    pub focus: Vec3,
    /// Camera yaw in radians.
    pub yaw: f32,
    /// Distance from the focus point.
    pub zoom: f32,
    /// Time of day in `0.0..=1.0` (0.5 = noon).
    pub time_of_day: f32,
    /// Free-look pitch in radians below the horizon (0.0 = level, negative
    /// looks up). The RTS camera derives its tilt from zoom every frame and
    /// can never frame the horizon or sky; when `pitch` is set the
    /// baked transform is overwritten after the camera update — the same
    /// escape the opening cinematic uses. `focus`/`zoom` still park the
    /// controller so streaming loads the right chunks.
    pub pitch: Option<f32>,
    /// Camera height in metres above the water surface for free-look shots.
    pub eye: f32,
    /// Per-shot streaming readiness contract.
    pub readiness: CaptureReadiness,
    /// Semantic invariants checked and recorded before this shot.
    pub assertions: Vec<CaptureAssertion>,
}

impl Default for Shot {
    fn default() -> Self {
        Self {
            name: "shot".to_string(),
            focus: Vec3::ZERO,
            yaw: -0.45,
            zoom: 220.0,
            time_of_day: 0.5,
            pitch: None,
            eye: 1.7,
            readiness: CaptureReadiness::default(),
            assertions: Vec::new(),
        }
    }
}

/// Free-look pose of the active shot, applied by [`apply_capture_free_look`]
/// AFTER the RTS camera bake each frame.
#[derive(Resource, Default)]
struct CaptureFreeLook(Option<FreeLookPose>);

struct FreeLookPose {
    focus: Vec3,
    yaw: f32,
    pitch: f32,
    eye: f32,
}

/// Overwrite the baked commander transform for free-look shots. Must run
/// after `update_commander_camera`, which unconditionally rewrites tilt from
/// zoom and bakes the transform (the opening cinematic escapes the same way).
fn apply_capture_free_look(
    free_look: Res<CaptureFreeLook>,
    terrain: Option<Res<shared::terrain::WorldTerrain>>,
    mut cameras: Query<&mut Transform, With<CommanderCamera>>,
) {
    let Some(pose) = &free_look.0 else { return };
    let surface = terrain
        .as_deref()
        .and_then(|t| t.get_water_height(pose.focus.x, pose.focus.z))
        .unwrap_or(pose.focus.y);
    for mut transform in cameras.iter_mut() {
        transform.translation = Vec3::new(pose.focus.x, surface + pose.eye, pose.focus.z);
        transform.rotation = Quat::from_rotation_y(pose.yaw) * Quat::from_rotation_x(-pose.pitch);
    }
}

#[derive(Resource, Debug, Clone)]
pub struct CaptureConfig {
    pub scenario_name: String,
    pub out_dir: PathBuf,
    pub shots: Vec<Shot>,
    pub resolution: [u32; 2],
    pub fixed_delta_seconds: f64,
    pub target: CaptureTarget,
    /// Hide the OS window while continuing to render the `Scene` image.
    pub show_window: bool,
    pub comparison: Option<CaptureComparisonConfig>,
    pub recording: Option<crate::capture_artifact::CaptureRecordingConfig>,
    pub diagnostics: crate::capture_artifact::CaptureDiagnosticsConfig,
    /// Frames to render before the first shot, so terrain chunks, props and textures
    /// have time to stream in. Streaming is async — too few frames photographs a
    /// half-loaded world, which looks like a rendering bug but is not one.
    pub warmup_frames: u32,
    /// Frames between moving the camera and taking the shot.
    pub settle_frames: u32,
    /// Fly the camera continuously: one shot per rendered frame, never
    /// pausing in `AwaitingCapture` between shots. Screenshot probes are fired
    /// asynchronously and awaited only after the final shot, so streaming
    /// gets no stationary frames in which to catch up — the whole point of a
    /// fast-pan regression capture.
    pub continuous: bool,
    /// In continuous mode, request a screenshot every N shots. Never exceeds
    /// one request per frame: Bevy silently despawns a same-frame duplicate
    /// screenshot of the same window and its PNG would never land.
    pub probe_every: u32,
    /// Measure real frame intervals on a continuous path without screenshot readbacks.
    pub benchmark: bool,
}

/// Where we are in the capture sequence.
#[derive(Resource, Debug)]
enum CaptureState {
    /// Letting the world stream in before the first shot.
    Warmup {
        progress: ReadinessProgress,
    },
    /// Camera moved, waiting for the world to settle at the new location.
    Settling {
        shot: usize,
        progress: ReadinessProgress,
    },
    /// Screenshot requested; waiting for Bevy's `ScreenshotCaptured` observer.
    AwaitingCapture {
        shot: usize,
        ticket: u64,
        frames_waited: u32,
    },
    /// Continuous flight finished; waiting for every observer to finish.
    AwaitingAll {
        tickets: Vec<u64>,
        frames_waited: u32,
    },
    FinishingVideo {
        started: Instant,
    },
    Done,
}

#[derive(Debug, Default)]
pub(crate) struct ReadinessProgress {
    frames: u32,
    stable_frames: u32,
    last_loaded_chunks: Option<usize>,
}

#[derive(Resource, Default)]
struct CaptureRunStatus {
    failed: bool,
}

#[derive(Resource, Default)]
struct CaptureVideoControl {
    enabled: bool,
    start_requested: bool,
    stop_requested: bool,
}

pub fn run(mut config: CaptureConfig) {
    if config.benchmark
        && (!config.continuous
            || config.shots.is_empty()
            || config.comparison.is_some()
            || config
                .recording
                .as_ref()
                .is_some_and(|recording| recording.enabled))
    {
        eprintln!("capture: --benchmark requires a nonempty continuous flight without comparison or recording");
        std::process::exit(2);
    }
    // Captures must not inherit the user's saved settings file.
    std::env::set_var("FISTFORCE_NO_SETTINGS_FILE", "1");
    std::env::set_var("FISTFORCE_CAPTURE_SYNC_PIPELINES", "1");
    if config.benchmark {
        config.show_window = false;
        std::env::set_var("FISTFORCE_FRAME_CAP", "0");
        std::env::set_var("FISTFORCE_VSYNC", "0");
    }
    if config.diagnostics.render_timings {
        std::env::set_var("FISTFORCE_RENDER_DIAG", "1");
        std::env::set_var("FISTFORCE_LOG_DIAGNOSTICS", "1");
    }
    if let Err(e) = std::fs::create_dir_all(&config.out_dir) {
        eprintln!(
            "capture: cannot create output dir {}: {e}",
            config.out_dir.display()
        );
        std::process::exit(1);
    }

    let recording_enabled = config
        .recording
        .as_ref()
        .is_some_and(|recording| recording.enabled);
    if recording_enabled && config.target != CaptureTarget::Scene {
        eprintln!(
            "capture: recording requires --target scene so video and artifact screenshots cannot race for the primary window"
        );
        std::process::exit(2);
    }
    #[cfg(not(feature = "capture-video"))]
    if recording_enabled {
        eprintln!(
            "capture: recording requires --features capture-video (screenshots are available without it)"
        );
        std::process::exit(2);
    }

    let asset_path = crate::get_asset_path();
    let mut app = App::new();
    crate::app_wiring::setup_plugins(&mut app, asset_path);
    crate::app_wiring::setup_resources(&mut app);
    crate::app_wiring::setup_systems(&mut app);

    app.world_mut()
        .resource_mut::<crate::perf_overlay::PerfOverlayEnabled>()
        .0 = config.diagnostics.performance_overlay;
    app.world_mut()
        .resource_mut::<shared::debug::DebugGizmoMode>()
        .0 = config.diagnostics.gizmos;

    {
        let mut settings = app
            .world_mut()
            .resource_mut::<crate::render::systems::GraphicsSettings>();
        // A capture resolution is an artifact contract, not a suggestion to
        // the user's current monitor. Keep the harness windowed even when the
        // shipped game defaults to borderless fullscreen.
        settings.fullscreen_enabled = false;
        settings.exclusive_fullscreen_enabled = false;
        settings.display_resolution = crate::render::systems::DisplayResolution::new(
            config.resolution[0],
            config.resolution[1],
        );
        if config.target == CaptureTarget::Scene {
            settings.render_scale = 1.0;
        }
    }
    app.insert_resource(bevy::time::TimeUpdateStrategy::ManualDuration(
        Duration::from_secs_f64(config.fixed_delta_seconds),
    ));

    #[cfg(feature = "capture-video")]
    if recording_enabled {
        use bevy_dev_tools::{EasyScreenRecordPlugin, Preset, Tune};
        let recording = config.recording.as_ref().expect("checked above");
        app.add_plugins(EasyScreenRecordPlugin {
            toggle: KeyCode::F12,
            preset: Preset::Medium,
            tune: Tune::Animation,
            frame_time: Duration::from_secs_f64(1.0 / f64::from(recording.frame_rate.max(1))),
            output_dir: Some(config.out_dir.join("video")),
        });
        app.add_systems(Update, drive_capture_video);
    }

    app.init_resource::<CaptureFreeLook>();
    app.init_resource::<CaptureCompletions>();
    app.init_resource::<CaptureRunStatus>();
    app.insert_resource(CaptureVideoControl {
        enabled: recording_enabled,
        ..default()
    });
    app.insert_resource(CaptureState::Warmup {
        progress: ReadinessProgress::default(),
    });
    if config.benchmark {
        // Winit's default game policy uses a reactive 60 Hz event loop when
        // unfocused, independently of vsync/the software cap. Hidden timing
        // runs must exercise the same continuous loop as focused gameplay.
        app.insert_resource(bevy::winit::WinitSettings::continuous());
        app.insert_resource(performance::CapturePerformance::new(config.shots.len()));
        app.add_systems(First, performance::measure_frames);
    }
    app.insert_resource(config);

    app.add_systems(PreStartup, configure_capture_window);
    app.add_systems(Startup, enter_world_offline);
    app.add_systems(PostStartup, presentation::setup_capture_presentation);
    app.add_systems(
        Update,
        (
            spawn_capture_isolated_prop,
            spawn_capture_dinghy,
            drive_capture_dinghy,
            spawn_capture_heroes,
            stage_capture_permit_placement,
            exercise_capture_door,
            select_capture_person,
            select_capture_place,
            open_capture_business_management,
            scroll_capture_business_page,
            open_capture_history,
            open_capture_world_map,
            position_capture_company_scroll,
            force_capture_drag_box,
            // Before the camera bake, so the pose photographed in frame N is
            // the pose this system applied in frame N — a continuous flight
            // must not trail its own screenshots by a frame.
            drive_capture.before(crate::camera_rts::update_commander_camera),
            apply_capture_free_look.after(crate::camera_rts::update_commander_camera),
        ),
    );

    let exit = app.run();
    if let AppExit::Error(code) = exit {
        // Winit returns Bevy's error value to the caller but does not convert
        // it into the capture process' exit status. CI and scripts need a
        // failed assertion/comparison to be an actual failed command.
        std::process::exit(i32::from(code.get()));
    }
}

fn configure_capture_window(
    config: Res<CaptureConfig>,
    mut windows: Query<&mut Window, With<bevy::window::PrimaryWindow>>,
) {
    for mut window in &mut windows {
        window
            .resolution
            .set_physical_resolution(config.resolution[0], config.resolution[1]);
        window.visible = config.show_window;
        window.title = format!("FistForce Capture — {}", config.scenario_name);
    }
}

#[cfg(feature = "capture-video")]
fn drive_capture_video(
    mut control: ResMut<CaptureVideoControl>,
    mut messages: MessageWriter<bevy_dev_tools::RecordScreen>,
) {
    if control.start_requested {
        control.start_requested = false;
        messages.write(bevy_dev_tools::RecordScreen::Start);
    }
    if control.stop_requested {
        control.stop_requested = false;
        messages.write(bevy_dev_tools::RecordScreen::Stop);
    }
}

#[allow(clippy::too_many_arguments)]
fn drive_capture(
    mut commands: Commands,
    config: Res<CaptureConfig>,
    mut state: ResMut<CaptureState>,
    mut cameras: Query<&mut CommanderCamera>,
    mut world_time: Query<&mut WorldTime>,
    mut free_look: ResMut<CaptureFreeLook>,
    inspection: CaptureInspection,
    mut completions: ResMut<CaptureCompletions>,
    mut run_status: ResMut<CaptureRunStatus>,
    mut video: ResMut<CaptureVideoControl>,
    mut pending_probes: Local<Vec<u64>>,
    mut app_exit: MessageWriter<AppExit>,
) {
    let chunk_count = inspection
        .loaded_chunks
        .as_ref()
        .map(|chunks| chunks.chunks.len())
        .unwrap_or(0);
    match &mut *state {
        CaptureState::Warmup { progress } => {
            // Park the camera on the first shot during warmup so streaming loads the
            // right chunks rather than whatever is around the origin.
            if let Some(shot) = config.shots.first() {
                apply_shot(shot, &mut cameras, &mut world_time, &mut free_look);
            }

            let mut readiness = config
                .shots
                .first()
                .map(|shot| shot.readiness.clone())
                .unwrap_or_default();
            readiness.minimum_frames = readiness.minimum_frames.max(config.warmup_frames);
            readiness.maximum_frames = readiness.maximum_frames.max(readiness.minimum_frames);
            match advance_readiness(progress, &readiness, chunk_count) {
                ReadinessOutcome::Waiting => return,
                ReadinessOutcome::TimedOut(reason) => {
                    error!("capture: warmup readiness timed out: {reason}");
                    run_status.failed = true;
                }
                ReadinessOutcome::Ready => {}
            }
            info!(
                "capture: warmup ready after {} frames with {} chunks; {} shot(s) queued",
                progress.frames,
                chunk_count,
                config.shots.len(),
            );
            if video.enabled {
                video.start_requested = true;
            }
            *state = CaptureState::Settling {
                shot: 0,
                progress: ReadinessProgress::default(),
            };
        }

        CaptureState::Settling { shot, progress } => {
            let index = *shot;
            let Some(current) = config.shots.get(index) else {
                finish_capture_run(&config, &mut state, &mut video);
                return;
            };
            apply_shot(current, &mut cameras, &mut world_time, &mut free_look);

            if config.continuous {
                // One shot per rendered frame: fire the probe screenshot
                // asynchronously and keep flying. Parking in AwaitingCapture
                // here is exactly what used to let streaming catch up and
                // hide every fast-pan artifact.
                let probe_every = config.probe_every.max(1) as usize;
                let last = index + 1 == config.shots.len();
                if config.benchmark && !current.assertions.is_empty() {
                    let snapshot = inspection.world_snapshot(world_time.iter().next());
                    for result in evaluate_assertions(&current.assertions, &snapshot) {
                        if !result.passed {
                            error!("capture: '{}' assertion failed: {:?}", current.name, result);
                            run_status.failed = true;
                        }
                    }
                }
                if !config.benchmark && (index % probe_every == 0 || last) {
                    let path = config.out_dir.join(format!("{}.png", current.name));
                    let snapshot = inspection.world_snapshot(world_time.iter().next());
                    let assertion_results = evaluate_assertions(&current.assertions, &snapshot);
                    if assertion_results.iter().any(|result| !result.passed) {
                        run_status.failed = true;
                    }
                    let Some(screenshot) = screenshot_for_target(
                        config.target,
                        inspection.scene_target.as_deref(),
                        inspection.presentation_target.as_deref(),
                    ) else {
                        error!("capture: render target is not ready for '{}'", current.name);
                        run_status.failed = true;
                        return;
                    };
                    let ticket = request_capture(
                        &mut commands,
                        screenshot,
                        capture_request(&config, current, path, snapshot, assertion_results, 0),
                        &mut completions,
                    );
                    info!(
                        "capture: probe '{}' at focus={:?} | {} terrain chunks loaded",
                        current.name, current.focus, chunk_count,
                    );
                    pending_probes.push(ticket);
                }
                *state = if last {
                    CaptureState::AwaitingAll {
                        tickets: std::mem::take(&mut *pending_probes),
                        frames_waited: 0,
                    }
                } else {
                    CaptureState::Settling {
                        shot: index + 1,
                        progress: ReadinessProgress::default(),
                    }
                };
                return;
            }

            let mut readiness = current.readiness.clone();
            readiness.minimum_frames = readiness.minimum_frames.max(config.settle_frames);
            readiness.maximum_frames = readiness.maximum_frames.max(readiness.minimum_frames);
            match advance_readiness(progress, &readiness, chunk_count) {
                ReadinessOutcome::Waiting => return,
                ReadinessOutcome::TimedOut(reason) => {
                    error!("capture: '{}' readiness timed out: {reason}", current.name);
                    run_status.failed = true;
                }
                ReadinessOutcome::Ready => {}
            }

            info!(
                "capture: '{}' ready after {} frames focus={:?} zoom={} time={} | {} terrain chunks loaded",
                current.name,
                progress.frames,
                current.focus,
                current.zoom,
                current.time_of_day,
                chunk_count,
            );
            let path = config.out_dir.join(format!("{}.png", current.name));
            let snapshot = inspection.world_snapshot(world_time.iter().next());
            let assertion_results = evaluate_assertions(&current.assertions, &snapshot);
            for failure in assertion_results.iter().filter(|result| !result.passed) {
                error!(
                    "capture: '{}' assertion failed: {:?} (observed {})",
                    current.name, failure.assertion, failure.observed
                );
                run_status.failed = true;
            }
            let Some(screenshot) = screenshot_for_target(
                config.target,
                inspection.scene_target.as_deref(),
                inspection.presentation_target.as_deref(),
            ) else {
                error!("capture: render target is not ready for '{}'", current.name);
                run_status.failed = true;
                return;
            };
            let ticket = request_capture(
                &mut commands,
                screenshot,
                capture_request(
                    &config,
                    current,
                    path.clone(),
                    snapshot,
                    assertion_results,
                    progress.frames,
                ),
                &mut completions,
            );
            info!("capture: shooting '{}' -> {}", current.name, path.display());
            *state = CaptureState::AwaitingCapture {
                shot: index,
                ticket,
                frames_waited: 0,
            };
        }

        CaptureState::AwaitingCapture {
            shot,
            ticket,
            frames_waited,
        } => {
            *frames_waited += 1;
            let Some(completion) = completions.take(*ticket) else {
                if *frames_waited > 1_200 {
                    error!(
                        "capture: screenshot observer timed out for ticket {}",
                        ticket
                    );
                    run_status.failed = true;
                } else {
                    return;
                }
                let next = *shot + 1;
                if next >= config.shots.len() {
                    finish_capture_run(&config, &mut state, &mut video);
                } else {
                    *state = CaptureState::Settling {
                        shot: next,
                        progress: ReadinessProgress::default(),
                    };
                }
                return;
            };
            if let Some(error) = completion.error {
                error!("capture: {} failed: {error}", completion.path.display());
                run_status.failed = true;
            } else {
                info!("capture: wrote {} and metadata", completion.path.display());
            }
            if completion.comparison_failed {
                error!(
                    "capture: visual baseline comparison failed for {}",
                    completion.path.display()
                );
                run_status.failed = true;
            }

            let next = *shot + 1;
            if next >= config.shots.len() {
                info!("capture: all {} shot(s) complete", config.shots.len());
                finish_capture_run(&config, &mut state, &mut video);
            } else {
                *state = CaptureState::Settling {
                    shot: next,
                    progress: ReadinessProgress::default(),
                };
            }
        }

        CaptureState::AwaitingAll {
            tickets,
            frames_waited,
        } => {
            *frames_waited += 1;
            tickets.retain(|ticket| {
                let Some(completion) = completions.take(*ticket) else {
                    return true;
                };
                if let Some(error) = completion.error {
                    error!("capture: {} failed: {error}", completion.path.display());
                    run_status.failed = true;
                } else {
                    info!("capture: wrote {} and metadata", completion.path.display());
                }
                if completion.comparison_failed {
                    error!(
                        "capture: visual baseline comparison failed for {}",
                        completion.path.display()
                    );
                    run_status.failed = true;
                }
                false
            });
            if tickets.is_empty() {
                info!("capture: all probe shot(s) complete");
                finish_capture_run(&config, &mut state, &mut video);
            } else if *frames_waited > 1_200 {
                error!(
                    "capture: {} screenshot observer(s) timed out",
                    tickets.len()
                );
                run_status.failed = true;
                finish_capture_run(&config, &mut state, &mut video);
            }
        }

        CaptureState::FinishingVideo { started } => {
            let flush_seconds = config
                .recording
                .as_ref()
                .map(|recording| recording.flush_seconds.max(0.0))
                .unwrap_or(0.0);
            if started.elapsed().as_secs_f32() >= flush_seconds {
                *state = CaptureState::Done;
            }
        }
        CaptureState::Done => {
            app_exit.write(if run_status.failed {
                AppExit::error()
            } else {
                AppExit::Success
            });
        }
    }
}

enum ReadinessOutcome {
    Waiting,
    Ready,
    TimedOut(String),
}

fn advance_readiness(
    progress: &mut ReadinessProgress,
    readiness: &CaptureReadiness,
    loaded_chunks: usize,
) -> ReadinessOutcome {
    progress.frames = progress.frames.saturating_add(1);
    if progress.last_loaded_chunks == Some(loaded_chunks) {
        progress.stable_frames = progress.stable_frames.saturating_add(1);
    } else {
        progress.last_loaded_chunks = Some(loaded_chunks);
        progress.stable_frames = 0;
    }
    let ready = progress.frames >= readiness.minimum_frames
        && loaded_chunks >= readiness.minimum_loaded_chunks
        && progress.stable_frames >= readiness.stable_loaded_chunk_frames;
    if ready {
        ReadinessOutcome::Ready
    } else if progress.frames >= readiness.maximum_frames {
        ReadinessOutcome::TimedOut(format!(
            "loaded_chunks={loaded_chunks}/{} stable_frames={}/{}",
            readiness.minimum_loaded_chunks,
            progress.stable_frames,
            readiness.stable_loaded_chunk_frames,
        ))
    } else {
        ReadinessOutcome::Waiting
    }
}

fn screenshot_for_target(
    target: CaptureTarget,
    scene_target: Option<&crate::render::systems::scaled_target::SceneRenderTarget>,
    presentation_target: Option<&presentation::CapturePresentationTarget>,
) -> Option<Screenshot> {
    match target {
        CaptureTarget::Window => {
            presentation_target.map(|target| Screenshot::image(target.image.clone()))
        }
        CaptureTarget::Scene => scene_target.map(|target| Screenshot::image(target.image.clone())),
    }
}

fn evaluate_assertions(
    assertions: &[CaptureAssertion],
    snapshot: &CaptureWorldSnapshot,
) -> Vec<CaptureAssertionResult> {
    assertions
        .iter()
        .cloned()
        .map(|assertion| {
            let (observed, passed) = match assertion {
                CaptureAssertion::LoadedChunksAtLeast { count } => {
                    (snapshot.loaded_chunks, snapshot.loaded_chunks >= count)
                }
                CaptureAssertion::EntitiesAtLeast { count } => {
                    (snapshot.entity_count, snapshot.entity_count >= count)
                }
                CaptureAssertion::VillagersAtLeast { count } => {
                    (snapshot.villagers, snapshot.villagers >= count)
                }
                CaptureAssertion::SettlementsAtLeast { count } => {
                    (snapshot.settlements, snapshot.settlements >= count)
                }
                CaptureAssertion::PlanningRoutesAtMost { count } => {
                    (snapshot.planning_routes, snapshot.planning_routes <= count)
                }
                CaptureAssertion::BlockedRoutesAtMost { count } => {
                    (snapshot.blocked_routes, snapshot.blocked_routes <= count)
                }
            };
            CaptureAssertionResult {
                assertion,
                passed,
                observed,
            }
        })
        .collect()
}

fn capture_request(
    config: &CaptureConfig,
    shot: &Shot,
    path: PathBuf,
    world: CaptureWorldSnapshot,
    assertions: Vec<CaptureAssertionResult>,
    readiness_frames: u32,
) -> CaptureWriteRequest {
    CaptureWriteRequest {
        path: path.clone(),
        metadata: CaptureMetadata {
            schema_version: CAPTURE_METADATA_VERSION,
            scenario: config.scenario_name.clone(),
            shot: shot.name.clone(),
            map: std::env::var("CITYSIM_MAP_ID").unwrap_or_else(|_| "default".to_owned()),
            git_commit: git_commit(),
            target: config.target,
            fixed_delta_seconds: config.fixed_delta_seconds,
            output: path.display().to_string(),
            width: 0,
            height: 0,
            readiness_frames,
            camera: CaptureCameraMetadata {
                focus: shot.focus.to_array(),
                yaw: shot.yaw,
                zoom: shot.zoom,
                time_of_day: shot.time_of_day,
                pitch: shot.pitch,
                eye: shot.eye,
                position: None,
                rotation: None,
            },
            world,
            assertions,
            comparison: None,
            comparison_error: None,
        },
        comparison: config.comparison.clone(),
    }
}

fn finish_capture_run(
    config: &CaptureConfig,
    state: &mut CaptureState,
    video: &mut CaptureVideoControl,
) {
    if video.enabled {
        video.stop_requested = true;
        *state = CaptureState::FinishingVideo {
            started: Instant::now(),
        };
        info!(
            "capture: recording stopped; flushing for {:.1}s",
            config
                .recording
                .as_ref()
                .map(|recording| recording.flush_seconds)
                .unwrap_or(0.0)
        );
    } else {
        *state = CaptureState::Done;
    }
}

fn apply_shot(
    shot: &Shot,
    cameras: &mut Query<&mut CommanderCamera>,
    world_time: &mut Query<&mut WorldTime>,
    free_look: &mut CaptureFreeLook,
) {
    free_look.0 = shot.pitch.map(|pitch| FreeLookPose {
        focus: shot.focus,
        yaw: shot.yaw,
        pitch,
        eye: shot.eye,
    });
    for mut camera in cameras.iter_mut() {
        // Set BOTH the rendered value and the target. The commander camera eases
        // toward its targets every frame, so writing only the rendered value
        // would have the camera spring straight back to wherever the target
        // still pointed -- a capture harness that silently framed the wrong shot.
        camera.focus = shot.focus;
        camera.focus_target = shot.focus;
        camera.yaw = shot.yaw;
        camera.yaw_target = shot.yaw;
        camera.zoom = shot.zoom;
        camera.zoom_target = shot.zoom;
    }

    for mut time in world_time.iter_mut() {
        let cycle = time.day_duration + time.night_duration;
        time.seconds_in_cycle = normalized_to_seconds(shot.time_of_day, &time);
        time.ocean_seconds = shot.time_of_day * cycle;
    }
}

/// Invert `WorldTime::normalized_time()`.
///
/// The internal cycle begins at sunrise while the public clock begins at
/// midnight. Getting that offset backwards silently photographs the world at
/// dusk, which reads as "the renderer is broken".
fn normalized_to_seconds(normalized: f32, time: &WorldTime) -> f32 {
    // Delegate to the shared inverse so capture `--time` always agrees with
    // the game's linear display clock; a hand-rolled copy here silently drifted
    // once before.
    let mut scratch = time.clone();
    scratch.set_normalized_time(normalized);
    scratch.seconds_in_cycle
}

#[cfg(test)]
mod tests {
    use super::*;

    fn probe() -> WorldTime {
        WorldTime {
            seconds_in_cycle: 0.0,
            day_duration: 600.0,
            night_duration: 300.0,
            ocean_seconds: 0.0,
            day: 0,
        }
    }

    /// Round-trip against the real accessor so a change to either side is caught.
    #[test]
    fn time_of_day_round_trips() {
        for target in [0.0_f32, 0.25, 0.5, 0.75, 0.9] {
            let mut t = probe();
            t.seconds_in_cycle = normalized_to_seconds(target, &t);
            let got = t.normalized_time();
            assert!(
                (got - target).abs() < 1e-3 || (got - target).abs() > 0.999,
                "time {target} round-tripped to {got}"
            );
        }
    }

    /// Capture time is a clock reading, not a fraction of the daylight arc.
    #[test]
    fn noon_lands_at_the_linear_clock_offset_from_sunrise() {
        let t = probe();
        let expected = t.cycle_duration() * (0.5 - WorldTime::SUNRISE_NORMALIZED).rem_euclid(1.0);
        assert!((normalized_to_seconds(0.5, &t) - expected).abs() < 1e-3);
    }
}
