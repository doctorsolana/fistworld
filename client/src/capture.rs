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

use std::path::PathBuf;

use bevy::prelude::*;
use bevy::render::view::screenshot::{save_to_disk, Screenshot};
use shared::components::WorldTime;

use crate::camera_rts::CommanderCamera;
use crate::states::GameState;
use crate::terrain::LoadedChunks;

/// Opt-in capture state for a real connected Village Lab run. This is kept in
/// the normal client rather than an external screen-grabber so the image is
/// taken from the actual render target at an exact authoritative world day.
#[derive(Default)]
pub(crate) enum LiveLabCaptureState {
    #[default]
    Uninitialized,
    Disabled,
    Waiting {
        day: u32,
        path: PathBuf,
        zoom: f32,
    },
    Settling {
        path: PathBuf,
        frames_left: u32,
    },
    AwaitingFile {
        path: PathBuf,
        frames_waited: u32,
    },
    Done,
}

#[derive(Resource, Default)]
pub(crate) struct LiveVoyageCaptureState {
    initialized: bool,
    enabled: bool,
    out_dir: PathBuf,
    stage: u8,
    awaiting: Option<(PathBuf, u32)>,
    sail_origin: Option<Vec3>,
    click_phase: u8,
}

/// Drive a real press/release through the ordinary right-click order system.
/// This is intentionally separate from the screenshot state machine so the
/// input lands before `issue_order_on_right_click` in the same Update frame.
pub(crate) fn drive_live_voyage_click_input(
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    mut state: ResMut<LiveVoyageCaptureState>,
) {
    if !state.enabled || state.stage != 3 {
        return;
    }
    match state.click_phase {
        1 => {
            // Let the deferred CursorTerrainOverride become visible to the
            // picker before the synthetic gesture begins.
            state.click_phase = 2;
        }
        2 => {
            mouse.press(MouseButton::Right);
            state.click_phase = 3;
            info!("live voyage capture: pressed ordinary right-click");
        }
        3 => {
            mouse.release(MouseButton::Right);
            state.click_phase = 4;
            info!("live voyage capture: released ordinary right-click");
        }
        _ => {}
    }
}

/// Capture the real connected new-player voyage at four points: face hold,
/// camera travel, final RTS framing, and authoritative sailing. Unlike
/// `FISTFORCE_CAPTURE_DINGHY`,
/// this hook creates no local fixtures; every photographed hero and vessel has
/// travelled through name submission, CreateHero, server spawning, interest
/// management and network replication.
///
/// Enable with `FISTWORLD_VOYAGE_CAPTURE_DIR=/absolute/output/directory`.
/// `FISTWORLD_VOYAGE_CAPTURE_EXIT=1` exits after all four PNGs reach disk.
pub(crate) fn drive_live_voyage_capture(
    mut commands: Commands,
    scene_target: Option<Res<crate::render::systems::scaled_target::SceneRenderTarget>>,
    opening: Res<crate::boat::OpeningCinematic>,
    local: Option<Res<crate::camera_rts::LocalPeerId>>,
    account: Option<Res<crate::ui::name_entry::PlayerNameInput>>,
    boats: Query<
        (
            Entity,
            &shared::components::CommandedBy,
            &shared::components::PlayerPosition,
            &shared::components::PlayerRotation,
        ),
        With<shared::components::PlayerBoat>,
    >,
    terrain: Res<shared::terrain::WorldTerrain>,
    selection: Res<crate::selection::Selection>,
    heroes: Query<
        (
            &shared::components::Hero,
            &shared::components::PlayerPosition,
        ),
        (
            With<shared::components::Hero>,
            With<shared::components::AboardBoat>,
        ),
    >,
    mut state: ResMut<LiveVoyageCaptureState>,
    mut app_exit: MessageWriter<AppExit>,
) {
    if !state.initialized {
        state.initialized = true;
        let Ok(raw) = std::env::var("FISTWORLD_VOYAGE_CAPTURE_DIR") else {
            return;
        };
        let out_dir = PathBuf::from(raw);
        if let Err(error) = std::fs::create_dir_all(&out_dir) {
            error!(
                "live voyage capture: cannot create {}: {error}",
                out_dir.display()
            );
            return;
        }
        state.enabled = true;
        state.out_dir = out_dir;
        info!("live voyage capture: armed for {}", state.out_dir.display());
    }
    if !state.enabled {
        return;
    }

    if let Some((path, frames_waited)) = state.awaiting.as_mut() {
        let written = std::fs::metadata(&*path).is_ok_and(|metadata| metadata.len() > 0);
        *frames_waited += 1;
        if !written && *frames_waited <= 600 {
            return;
        }
        if !written {
            error!("live voyage capture: {} never reached disk", path.display());
            state.enabled = false;
            app_exit.write(AppExit::error());
            return;
        }
        info!("live voyage capture: wrote {}", path.display());
        state.awaiting = None;
        state.stage += 1;
        if state.stage >= 4 {
            state.enabled = false;
            commands.remove_resource::<crate::camera_rts::CursorTerrainOverride>();
            if std::env::var("FISTWORLD_VOYAGE_CAPTURE_EXIT")
                .is_ok_and(|value| matches!(value.as_str(), "1" | "true" | "yes" | "on"))
            {
                app_exit.write(AppExit::Success);
            }
            return;
        }
    }

    let local_id = local.as_ref().map(|local| local.0);
    let account = account
        .as_ref()
        .map(|account| account.name.trim().to_lowercase());
    let local_boat = boats
        .iter()
        .find_map(|(entity, owner, position, rotation)| {
            (account.as_deref() == Some(owner.0.as_str()))
                .then_some((entity, position.0, rotation.0))
        });
    let boat_position = local_boat.map(|(_, position, _)| position);
    let hero_position = heroes.iter().find_map(|(hero, position)| {
        (local_id == Some(shared::player::peer_id_to_u64(hero.owner))).then_some(position.0)
    });
    let running = opening.running_elapsed_secs();
    let ready = match state.stage {
        0 => running.is_some_and(|elapsed| elapsed >= 2.25),
        1 => running.is_some_and(|elapsed| elapsed >= 5.70),
        2 => !opening.is_active() && boat_position.is_some() && hero_position.is_some(),
        3 => {
            let Some((boat, position, rotation)) = local_boat else {
                return;
            };
            if state.sail_origin.is_none() {
                // Exercise ordinary right-click movement without making the
                // photographed voyage perform an artificial U-turn. The
                // starter hull is already aimed down its certified approach
                // corridor, so test progressively nearer points straight off
                // the authored -Z bow.
                let forward = (Quat::from_rotation_y(rotation) * Vec3::NEG_Z)
                    .xz()
                    .normalize_or_zero();
                let Some(goal) = [30.0_f32, 24.0, 18.0, 12.0]
                    .into_iter()
                    .find_map(|distance| {
                        let xz = position.xz() + forward * distance;
                        terrain
                            .get_water_height(xz.x, xz.y)
                            .map(|water| Vec3::new(xz.x, water, xz.y))
                    })
                else {
                    error!("live voyage capture: no nearby water goal for sailing check");
                    state.enabled = false;
                    app_exit.write(AppExit::error());
                    return;
                };
                if !selection.is_selected(boat) {
                    error!(
                        "live voyage capture: opening cinematic did not leave the Dinghy selected"
                    );
                    state.enabled = false;
                    app_exit.write(AppExit::error());
                    return;
                }
                state.sail_origin = Some(position);
                state.click_phase = 1;
                commands.insert_resource(crate::camera_rts::CursorTerrainOverride(goal.xz()));
                info!(
                    "live voyage capture: armed ordinary water right-click boat={boat:?} from={position:?} to={goal:?}"
                );
                return;
            }
            state
                .sail_origin
                .is_some_and(|origin| origin.distance(position) >= 7.0)
        }
        _ => false,
    };
    if !ready {
        return;
    }
    let Some(scene_target) = scene_target else {
        return;
    };

    let filename = match state.stage {
        0 => "01_face.png",
        1 => "02_transition.png",
        2 => "03_rts.png",
        3 => "04_sailing.png",
        _ => return,
    };
    let path = state.out_dir.join(filename);
    let _ = std::fs::remove_file(&path);
    info!(
        "live voyage capture: shooting stage={} boat={boat_position:?} hero={hero_position:?} -> {}",
        state.stage,
        path.display()
    );
    // Capture the actual 3D scene target. The primary-window screenshot can
    // be an all-black swapchain image on macOS while entering fullscreen or
    // changing physical resolution, even though the presented scene is fine.
    // The cinematic itself lives in this offscreen target, so photographing it
    // is both deterministic and independent of the player's display mode.
    commands
        .spawn(Screenshot::image(scene_target.image.clone()))
        .observe(save_to_disk(path.clone()));
    state.awaiting = Some((path, 0));
}

/// Capture the connected, fully simulated Village Lab on a requested HUD day.
///
/// Environment variables:
/// - `FISTWORLD_LAB_CAPTURE_DAY` enables the hook.
/// - `FISTWORLD_LAB_CAPTURE_PATH` selects the PNG path.
/// - `FISTWORLD_LAB_CAPTURE_ZOOM` controls the survey framing (default 430m).
/// - `FISTWORLD_LAB_CAPTURE_SETTLE_FRAMES` controls render warmup (default 180).
/// - `FISTWORLD_LAB_CAPTURE_EXIT=1` closes the client after the file is written.
pub(crate) fn drive_live_lab_capture(
    mut commands: Commands,
    world_time: Query<&WorldTime>,
    mut cameras: Query<&mut CommanderCamera>,
    mut state: Local<LiveLabCaptureState>,
    mut app_exit: MessageWriter<AppExit>,
) {
    if matches!(*state, LiveLabCaptureState::Uninitialized) {
        let Some(day) = std::env::var("FISTWORLD_LAB_CAPTURE_DAY")
            .ok()
            .and_then(|raw| raw.parse::<u32>().ok())
        else {
            *state = LiveLabCaptureState::Disabled;
            return;
        };
        let path = std::env::var("FISTWORLD_LAB_CAPTURE_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(format!("logs/village-lab-day-{day}.png")));
        let zoom = std::env::var("FISTWORLD_LAB_CAPTURE_ZOOM")
            .ok()
            .and_then(|raw| raw.parse::<f32>().ok())
            .filter(|zoom| zoom.is_finite() && *zoom > 0.0)
            .unwrap_or(430.0);
        *state = LiveLabCaptureState::Waiting { day, path, zoom };
    }

    match &mut *state {
        LiveLabCaptureState::Uninitialized | LiveLabCaptureState::Disabled => {}
        LiveLabCaptureState::Waiting { day, path, zoom } => {
            let Some(clock) = world_time.iter().next() else {
                return;
            };
            if clock.day < *day {
                return;
            }
            for mut camera in cameras.iter_mut() {
                camera.zoom = *zoom;
                camera.zoom_target = *zoom;
            }
            info!(
                "live Village Lab capture: reached HUD day {}, settling zoom {} for {}",
                clock.day,
                zoom,
                path.display()
            );
            let frames_left = std::env::var("FISTWORLD_LAB_CAPTURE_SETTLE_FRAMES")
                .ok()
                .and_then(|raw| raw.parse::<u32>().ok())
                .unwrap_or(180);
            *state = LiveLabCaptureState::Settling {
                path: path.clone(),
                frames_left,
            };
        }
        LiveLabCaptureState::Settling { path, frames_left } => {
            if *frames_left > 0 {
                *frames_left -= 1;
                return;
            }
            if let Some(parent) = path.parent() {
                if let Err(error) = std::fs::create_dir_all(parent) {
                    error!(
                        "live Village Lab capture: cannot create {}: {error}",
                        parent.display()
                    );
                    *state = LiveLabCaptureState::Done;
                    return;
                }
            }
            let _ = std::fs::remove_file(&*path);
            commands
                .spawn(Screenshot::primary_window())
                .observe(save_to_disk(path.clone()));
            info!("live Village Lab capture: shooting {}", path.display());
            *state = LiveLabCaptureState::AwaitingFile {
                path: path.clone(),
                frames_waited: 0,
            };
        }
        LiveLabCaptureState::AwaitingFile {
            path,
            frames_waited,
        } => {
            let written = std::fs::metadata(&*path).is_ok_and(|metadata| metadata.len() > 0);
            *frames_waited += 1;
            if !written && *frames_waited <= 600 {
                return;
            }
            if written {
                info!("live Village Lab capture: wrote {}", path.display());
            } else {
                error!(
                    "live Village Lab capture: {} never reached disk",
                    path.display()
                );
            }
            if std::env::var("FISTWORLD_LAB_CAPTURE_EXIT").is_ok_and(|raw| {
                matches!(
                    raw.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on"
                )
            }) {
                app_exit.write(if written {
                    AppExit::Success
                } else {
                    AppExit::error()
                });
            }
            *state = LiveLabCaptureState::Done;
        }
        LiveLabCaptureState::Done => {}
    }
}

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
    pub out_dir: PathBuf,
    pub shots: Vec<Shot>,
    /// Frames to render before the first shot, so terrain chunks, props and textures
    /// have time to stream in. Streaming is async — too few frames photographs a
    /// half-loaded world, which looks like a rendering bug but is not one.
    pub warmup_frames: u32,
    /// Frames between moving the camera and taking the shot.
    pub settle_frames: u32,
    /// Fly the camera continuously: one shot per rendered frame, never
    /// pausing in `AwaitingFile` between shots. Screenshot probes are fired
    /// asynchronously and awaited only after the final shot, so streaming
    /// gets no stationary frames in which to catch up — the whole point of a
    /// fast-pan regression capture.
    pub continuous: bool,
    /// In continuous mode, request a screenshot every N shots. Never exceeds
    /// one request per frame: Bevy silently despawns a same-frame duplicate
    /// screenshot of the same window and its PNG would never land.
    pub probe_every: u32,
}

/// Where we are in the capture sequence.
#[derive(Resource, Debug)]
enum CaptureState {
    /// Letting the world stream in before the first shot.
    Warmup {
        frames_left: u32,
    },
    /// Camera moved, waiting for the world to settle at the new location.
    Settling {
        shot: usize,
        frames_left: u32,
    },
    /// Screenshot requested; waiting for the file to actually exist on disk.
    ///
    /// This is the latch that matters: the render-to-disk round trip is async, so
    /// exiting on a frame count instead races the writer and truncates the last image.
    AwaitingFile {
        shot: usize,
        path: PathBuf,
        frames_waited: u32,
    },
    /// Continuous flight finished; waiting for every probe PNG to land.
    AwaitingAll {
        paths: Vec<PathBuf>,
        frames_waited: u32,
    },
    Done,
}

pub fn run(config: CaptureConfig) {
    // Captures must not inherit the user's saved settings file.
    std::env::set_var("FISTFORCE_NO_SETTINGS_FILE", "1");
    if let Err(e) = std::fs::create_dir_all(&config.out_dir) {
        eprintln!(
            "capture: cannot create output dir {}: {e}",
            config.out_dir.display()
        );
        std::process::exit(1);
    }

    let asset_path = crate::get_asset_path();
    let mut app = App::new();
    crate::app_wiring::setup_plugins(&mut app, asset_path);
    crate::app_wiring::setup_resources(&mut app);
    crate::app_wiring::setup_systems(&mut app);

    app.init_resource::<CaptureFreeLook>();
    app.insert_resource(CaptureState::Warmup {
        frames_left: config.warmup_frames,
    });
    app.insert_resource(config);

    app.add_systems(Startup, enter_world_offline);
    app.add_systems(
        Update,
        (
            spawn_capture_dinghy,
            drive_capture_dinghy,
            spawn_capture_heroes,
            stage_capture_permit_placement,
            exercise_capture_door,
            select_capture_person,
            select_capture_place,
            open_capture_business_management,
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

    app.run();
}

/// `FISTFORCE_CAPTURE_WORLD_MAP=1` opens the real modal world map during an
/// offline capture. Combine it with `FISTFORCE_CAPTURE_HERO=default` and
/// `FISTFORCE_CAPTURE_SELECT=1` to verify the owned-hero arrow and camera
/// viewport without mouse automation or a live server.
fn open_capture_world_map(
    mut map_open: ResMut<crate::ui::world_map::MapOpen>,
    mut handled: Local<bool>,
) {
    if *handled {
        return;
    }
    *handled = true;
    if std::env::var("FISTFORCE_CAPTURE_WORLD_MAP").is_ok_and(|value| value == "1") {
        map_open.0 = true;
    }
}

/// `FISTFORCE_CAPTURE_DINGHY=underway|sailing|wreck` stages the real runtime
/// scene on water at the first shot's focus. This is presentation-only—the
/// live server remains the authority for navigation and disembarkation—but it
/// exercises the identical named sail nodes, morph and wreck state without a
/// login. `sailing` additionally advances the hull along its velocity so the
/// wake foam trail behind a genuinely moving boat can be photographed.
fn spawn_capture_dinghy(
    mut commands: Commands,
    mut config: ResMut<CaptureConfig>,
    terrain: Option<Res<shared::terrain::WorldTerrain>>,
    mut spawned: Local<bool>,
) {
    if *spawned {
        return;
    }
    let Ok(mode) = std::env::var("FISTFORCE_CAPTURE_DINGHY") else {
        *spawned = true;
        return;
    };
    let Some(terrain) = terrain else { return };
    let focus = config.shots.first().map_or(Vec3::ZERO, |shot| shot.focus);
    let Some(water) = terrain.get_water_height(focus.x, focus.z) else {
        error!(
            "capture: FISTFORCE_CAPTURE_DINGHY needs water at ({:.1}, {:.1})",
            focus.x, focus.z
        );
        *spawned = true;
        return;
    };
    let position = Vec3::new(focus.x, water, focus.z);
    // CLI captures commonly specify only X,Z and therefore default Y to zero.
    // Ocean height is map-authored, so that can put the review camera below
    // the surface. Keep every requested angle centred on the actual hull
    // waterline while preserving its XZ composition.
    for shot in config.shots.iter_mut() {
        shot.focus.y = water + 0.55;
    }
    let wrecked = mode.trim().eq_ignore_ascii_case("wreck");
    let sailing = mode.trim().eq_ignore_ascii_case("sailing");
    // Match the heading to the velocity so a sailing hull moves bow-first
    // (authored bow is -Z: yaw for velocity v is atan2(-v.x, -v.z)).
    let velocity = Vec3::new(2.0, 0.0, -1.0);
    let yaw = (-velocity.x).atan2(-velocity.z);
    let mut vessel = commands.spawn((
        shared::components::PlayerBoat,
        shared::components::Vessel,
        shared::components::CommandedBy("capture-sailor".into()),
        shared::components::PlayerPosition(position),
        shared::components::PlayerRotation(yaw),
        shared::components::CharacterMotion::new(velocity),
    ));
    if wrecked {
        vessel.insert(shared::components::WreckedVessel);
    }
    if sailing {
        vessel.insert(CaptureSailing);
    }
    if !wrecked {
        let manifest = match shared::character::CharacterManifest::load() {
            Ok(manifest) => manifest,
            Err(error) => {
                error!("capture: character manifest unavailable: {error}");
                *spawned = true;
                return;
            }
        };
        let helm = position + Quat::from_rotation_y(yaw) * Vec3::new(0.0, 0.35, 1.24);
        commands.spawn((
            shared::components::Hero {
                owner: lightyear::prelude::PeerId::Netcode(9_999),
            },
            shared::components::CharacterName("Capture Sailor".into()),
            shared::components::CharacterKind::Hero,
            shared::components::CharacterAffiliation::default(),
            shared::components::PersonId(99_999),
            shared::components::HeroOutfit::from_manifest(&manifest),
            shared::components::CommandedBy("capture-sailor".into()),
            shared::components::AboardBoat,
            shared::components::CharacterActivity::Sitting,
            shared::components::PlayerPosition(helm),
            shared::components::PlayerRotation(yaw),
            shared::components::CharacterMotion::new(velocity),
        ));
    }
    *spawned = true;
}

/// Marks the staged capture dinghy that should genuinely travel, so the wake
/// breadcrumb trail forms exactly as it does behind a live sailing boat.
#[derive(Component)]
struct CaptureSailing;

fn drive_capture_dinghy(
    time: Res<Time>,
    mut boats: Query<
        (
            &mut shared::components::PlayerPosition,
            &shared::components::CharacterMotion,
        ),
        With<CaptureSailing>,
    >,
) {
    for (mut position, motion) in boats.iter_mut() {
        position.0 += motion.velocity * time.delta_secs();
    }
}

/// `FISTFORCE_CAPTURE_DOOR=open` holds the offline settlement's town-hall
/// door open through the same stable building-side state used in a live game.
/// This is intentionally independent of villager AI and networking: a capture
/// made with it is a smoke test for scene instantiation, graph wiring, the
/// replicated demand consumer, and the authored glTF clip in one repeatable run.
fn exercise_capture_door(
    mut commands: Commands,
    settlements: Query<Entity, With<shared::components::Settlement>>,
    mut applied: Local<bool>,
) {
    if std::env::var("FISTFORCE_CAPTURE_DOOR").as_deref() != Ok("open") {
        return;
    }
    if *applied {
        return;
    }
    let Some(settlement) = settlements.iter().next() else {
        return;
    };
    commands
        .entity(settlement)
        .insert(shared::components::BuildingDoorDemand { open: true });
    *applied = true;
}

/// Jump straight into the world and provide the world state the server normally sends.
fn enter_world_offline(mut commands: Commands, mut next_state: ResMut<NextState<GameState>>) {
    // No connection, no name entry — the map comes off disk.
    next_state.set(GameState::Playing);

    // Stand in for the replicated WorldTime, otherwise day/night never advances past
    // "waiting for server" and every shot is unlit.
    commands.spawn(WorldTime {
        seconds_in_cycle: 0.0,
        day_duration: 600.0,
        night_duration: 300.0,
        ocean_seconds: 0.0,
        day: 0,
    });
    // Same stand-in for CloudSeed — the cloud plane waits for it. Fixed seed so
    // shots are reproducible; FISTFORCE_CAPTURE_CLOUDS=clear|cloudy overrides
    // the weather roll for guaranteed cloud coverage in verification shots.
    commands.spawn(shared::components::CloudSeed { seed: 7 });
    if let Ok(forced) = std::env::var("FISTFORCE_CAPTURE_CLOUDS") {
        use crate::render::systems::{CloudCover, CloudCoverMode, CloudCoverOverride};
        let mode = match forced.as_str() {
            "cloudy" => Some(CloudCoverMode::Cloudy),
            "clear" => Some(CloudCoverMode::Clear),
            "storm" => Some(CloudCoverMode::Storm),
            _ => None,
        };
        if let Some(mode) = mode {
            commands.insert_resource(CloudCoverOverride { mode });
            // Snap: a capture's few warmup seconds can't ride the ~90s lerp.
            commands.insert_resource(CloudCover::snapped(mode));
        }
    }

    // FISTFORCE_CAPTURE_HUD=play|god draws the persistent HUD, which is
    // otherwise suppressed so world shots stay clean. `god` also grants the god
    // capability and switches mode, so the god plate is visible.
    if std::env::var("FISTFORCE_CAPTURE_HUD").is_ok_and(|mode| mode == "god") {
        commands.insert_resource(crate::ui::hud::GodCapability(true));
        commands.insert_resource(crate::ui::hud::HudMode::God);
    }

    // FISTFORCE_CAPTURE_DEBUG_MENU=god|access opens the real J menu without
    // synthesizing keyboard input. Keeping this as a first-class capture target
    // prevents developer-only screens from escaping the ordinary UI visual audit.
    if let Ok(mode) = std::env::var("FISTFORCE_CAPTURE_DEBUG_MENU") {
        let god_access = mode.trim().eq_ignore_ascii_case("god");
        commands.insert_resource(crate::ui::debug_time_menu::DebugTimeMenuOpen(true));
        commands.insert_resource(crate::ui::hud::GodCapability(god_access));
        if god_access {
            commands.insert_resource(crate::ui::hud::HudMode::God);
        }
    }

    // FISTFORCE_CAPTURE_ROADS=compare stages the same completed main road
    // before and after its real Dirt -> Stone upgrade. It deliberately spawns
    // only replicated road data: the ordinary terrain compositor must produce
    // the image, so this remains a regression fixture for the shipping path.
    if std::env::var("FISTFORCE_CAPTURE_ROADS").is_ok_and(|value| value == "compare") {
        commands.queue(|world: &mut World| {
            let focus = world
                .get_resource::<CaptureConfig>()
                .and_then(|config| config.shots.first().map(|shot| shot.focus))
                .unwrap_or_default();
            let relative = [
                Vec2::new(-34.0, 0.0),
                Vec2::new(-12.0, -2.0),
                Vec2::new(8.0, 1.0),
                Vec2::new(34.0, 0.0),
            ];
            let points_at = |z: f32| {
                relative
                    .iter()
                    .map(|point| Vec2::new(focus.x + point.x, focus.z + point.y + z))
                    .collect::<Vec<_>>()
            };
            world.spawn(shared::components::VillageRoad {
                settlement: "Capture Roads".into(),
                builder: "Capture Road Steward".into(),
                points: points_at(-7.0),
                built_through: relative.len() as u16,
                width: 2.6,
                reserved_width: shared::components::RoadClass::Main.initial_reserved_width(),
                surface: shared::components::RoadSurface::Dirt,
                class: shared::components::RoadClass::Main,
                stone_committed: 0,
            });
            let mut stone = shared::components::VillageRoad {
                settlement: "Capture Roads".into(),
                builder: "Capture Road Steward".into(),
                points: points_at(7.0),
                built_through: relative.len() as u16,
                width: 4.0,
                reserved_width: shared::components::RoadClass::Main.initial_reserved_width(),
                surface: shared::components::RoadSurface::Stone,
                class: shared::components::RoadClass::Main,
                stone_committed: 0,
            };
            stone.stone_committed = stone.stone_required();
            world.spawn(stone);
        });
    }

    // FISTFORCE_CAPTURE_PAUSE=main|graphics|controls photographs the real ESC
    // menu and its expanded settings wells without needing keyboard input.
    if let Ok(panel) = std::env::var("FISTFORCE_CAPTURE_PAUSE") {
        crate::ui::pause_menu::open_for_capture(&mut commands, panel.trim());
    }

    // FISTFORCE_CAPTURE_SETTLEMENT=1 founds a settlement at the shot's focus so
    // the moot hall can be photographed without a server.
    // "village" additionally populates the first one; "coast" stages the
    // deterministic Village Lab hut/pier pair for shoreline inspection; and
    // "industries" frames the authored founding production buildings closely;
    // "bakery" isolates a staffed bakery for chimney, lighting and stock-art QA;
    // "market" and "market_paved" isolate the two identically sized square levels.
    if std::env::var("FISTFORCE_CAPTURE_SETTLEMENT").is_ok_and(|v| {
        matches!(
            v.as_str(),
            "1" | "village" | "coast" | "industries" | "bakery" | "market" | "market_paved"
        )
    }) {
        commands.queue(|world: &mut World| {
            // The FIRST SHOT's focus, not the camera's: this runs in Startup,
            // before `apply_shot` has moved the camera, so reading the camera
            // here plants the settlement at the origin and photographs empty
            // ground two kilometres away from it.
            let focus = world
                .get_resource::<CaptureConfig>()
                .and_then(|c| c.shots.first().map(|s| s.focus))
                .unwrap_or_default();
            let mode = std::env::var("FISTFORCE_CAPTURE_SETTLEMENT").unwrap_or_default();
            if matches!(mode.as_str(), "market" | "market_paved") {
                // Reproduce the authoritative construction earthwork in this
                // network-free visual fixture. This makes the capture useful
                // for spotting terrain triangles through the 12 m ground slab,
                // not merely for checking the GLB in isolation.
                if let Some(mut terrain) =
                    world.get_resource_mut::<shared::terrain::WorldTerrain>()
                {
                    let def = shared::components::SettlementBuildingKind::Market
                        .art()
                        .definition();
                    let target = terrain.get_height(focus.x, focus.z);
                    terrain.apply_flatten_rect(
                        Vec3::new(focus.x, target, focus.z),
                        def.terrain_flat_half_extents(),
                        0.0,
                        def.terrain_blend_width(),
                    );
                }
            }
            let settlement_focus = match mode.as_str() {
                "coast" => {
                    // The requested focus is the hut. This is its deterministic
                    // offset from the lab hall selected on village_lab seed 3.
                    focus - Vec3::new(53.94803, 0.0, -41.39578)
                }
                // Centre the authored production cluster rather than its Hall.
                // This keeps close asset-validation shots reusable as the Hall
                // ladder grows substantially taller than founding industries.
                "industries" | "bakery" | "market" | "market_paved" => {
                    focus + Vec3::new(0.0, 0.0, 140.0)
                }
                _ if std::env::var("FISTFORCE_CAPTURE_PERMIT_PLACEMENT").is_ok() => {
                    focus + Vec3::new(0.0, 0.0, -50.0)
                }
                _ => focus,
            };
            let ground = world
                .get_resource::<shared::terrain::WorldTerrain>()
                .map(|t| t.get_height(settlement_focus.x, settlement_focus.z))
                .unwrap_or(settlement_focus.y);
            use shared::components::SettlementTier as T;
            // A spread of rungs, so the list's ordering and the detail pane's
            // per-rung wording can both be photographed.
            for (i, (name, tier, offset)) in [
                ("Brackwater", T::Town, Vec3::new(0.0, 0.0, 0.0)),
                ("Ashfell", T::Hamlet, Vec3::new(-1400.0, 0.0, -1900.0)),
                ("Millhollow", T::Village, Vec3::new(900.0, 0.0, 1500.0)),
                ("Coldbarrow", T::Hamlet, Vec3::new(1800.0, 0.0, -600.0)),
            ]
            .into_iter()
            .enumerate()
            {
                let at = settlement_focus + offset;
                let y = if i == 0 { ground } else { at.y };
                let hall = world
                    .spawn((
                        shared::components::Settlement {
                            name: name.to_string(),
                            tier,
                            residents: (i as u32) * 3,
                            treasury: 0,
                        },
                        shared::components::SettlementId(i as u64 + 1),
                        shared::components::PlayerPosition(Vec3::new(at.x, y, at.z)),
                    ))
                    .id();
                if i == 0 && mode == "village" {
                    let mut store = shared::economy::GoodsInventory::new_partitioned(
                        shared::economy::capacity::HALL,
                    );
                    store.add(shared::economy::Good::Food, 9);
                    store.add(shared::economy::Good::Wheat, 18);
                    store.add(shared::economy::Good::Wood, 7);
                    store.add(shared::economy::Good::Stone, 2);
                    let mut market = shared::economy::MootMarket::founding();
                    market.refresh_all(&store);
                    world.entity_mut(hall).insert((
                        store,
                        market,
                        shared::components::MootAdministration {
                            lead_steward: Some(shared::names::person_name(7_002)),
                            roadless_buildings: 1,
                            disconnected_buildings: 0,
                            last_road_audit_day: 12,
                            ..default()
                        },
                        shared::components::SettlementPolicies::poor_relief(),
                        shared::components::SettlementOpportunityBoard {
                            opportunities: vec![
                                shared::components::PermitMarketOpportunity {
                                    kind: shared::components::SettlementBuildingKind::House,
                                    score: 94,
                                    subsidized: true,
                                    requires_independent_owner: false,
                                },
                                shared::components::PermitMarketOpportunity {
                                    kind: shared::components::SettlementBuildingKind::Farmstead,
                                    score: 78,
                                    subsidized: true,
                                    requires_independent_owner: false,
                                },
                                shared::components::PermitMarketOpportunity {
                                    kind: shared::components::SettlementBuildingKind::Windmill,
                                    score: 46,
                                    subsidized: false,
                                    requires_independent_owner: true,
                                },
                                shared::components::PermitMarketOpportunity {
                                    kind: shared::components::SettlementBuildingKind::Bakery,
                                    score: 31,
                                    subsidized: false,
                                    requires_independent_owner: false,
                                },
                            ],
                        },
                        shared::components::SettlementPropertyBoard {
                            listings: vec![
                                shared::components::PropertyMarketListing {
                                    kind: shared::components::SettlementBuildingKind::Bakery,
                                    stage: shared::components::PropertyListingStage::CompletedBusiness,
                                    asking_price: 425,
                                    listed_day: 0,
                                    reason: shared::economy::BusinessSaleReason::Insolvent,
                                    position: settlement_focus + Vec3::new(46.0, 0.0, 34.0),
                                },
                                shared::components::PropertyMarketListing {
                                    kind: shared::components::SettlementBuildingKind::Windmill,
                                    stage: shared::components::PropertyListingStage::UnfinishedWorksite,
                                    asking_price: 250,
                                    listed_day: 0,
                                    reason: shared::economy::BusinessSaleReason::OwnerDied,
                                    position: settlement_focus + Vec3::new(-38.0, 0.0, 26.0),
                                },
                            ],
                        },
                        shared::economy::SettlementEconomy {
                            edible_stock: 27,
                            reserve_days: 4.5,
                            recent_food_production: 3.7,
                            recent_food_consumption: 3.0,
                            unmet_food: 0,
                            observed_days: 8,
                            food_secure_days: 5,
                            reserve_prosperity: 22.0,
                            production_prosperity: 20.0,
                            housing_prosperity: 18.0,
                            employment_prosperity: 18.0,
                            hunger_penalty: 0.0,
                            prosperity: 78.0,
                            private_job_positions: 8,
                            private_filled_jobs: 6,
                            private_vacant_jobs: 2,
                            civic_job_positions: 3,
                            civic_filled_jobs: 2,
                            civic_vacant_jobs: 1,
                            job_seekers: 1,
                            best_open_private_wage: 120,
                            housing_capacity: 8,
                            homeless_residents: 0,
                            unpaid_workers: 0,
                            unrest: 8.0,
                            unrest_target: 0.0,
                            unrest_change: -5.0,
                            unrest_hunger_pressure: 0.0,
                            unrest_housing_pressure: 0.0,
                            unrest_wage_pressure: 0.0,
                        },
                    ));
                }
            }

            // FISTFORCE_CAPTURE_SETTLEMENT=village also populates the FIRST
            // settlement: residents on record, buildings standing, one going
            // up. These are offline stand-ins for what the server's autonomy
            // produces, so the settlement panel can be photographed without
            // waiting out a live village.
            if std::env::var("FISTFORCE_CAPTURE_SETTLEMENT")
                .is_ok_and(|v| {
                    matches!(
                        v.as_str(),
                        "village" | "industries" | "bakery" | "market" | "market_paved"
                    )
                })
            {
                use shared::components::SettlementBuildingKind as K;
                let people: Vec<String> = (0..3)
                    .map(|i| shared::names::person_name(7_000 + i))
                    .collect();
                for (index, name) in people.iter().enumerate() {
                    world.spawn((
                        shared::components::CharacterName(name.clone()),
                        shared::components::CharacterKind::Villager,
                        shared::components::Residence("Brackwater".to_string()),
                        shared::components::Occupation(Some(
                            ["Farmer", "Lumberjack", "Road Steward"][index].to_string(),
                        )),
                        shared::economy::Wallet::new((650 + index as u64 * 275) * 100),
                        shared::components::Nutrition {
                            last_meal_day: Some(12),
                            consecutive_missed_meals: if index == 1 { 1 } else { 0 },
                            total_meals: 12,
                            total_missed_meals: if index == 1 { 1 } else { 0 },
                        },
                        shared::components::CharacterActivity::Indoors,
                    ));
                }
                let kinds: &[K] = if mode == "bakery" {
                    &[K::Bakery]
                } else if matches!(mode.as_str(), "market" | "market_paved") {
                    &[K::Market]
                } else {
                    &[K::Farmstead, K::LumberjackHut, K::Windmill, K::Bakery]
                };
                for (index, kind) in kinds.iter().copied().enumerate() {
                    let at = if matches!(mode.as_str(), "bakery" | "market" | "market_paved") {
                        focus
                    } else if mode == "industries" {
                        // One authored comparison line: equal frontage,
                        // spacing and rotation make scale/anchor mistakes
                        // obvious in a single frame.
                        focus + Vec3::new(-27.0 + index as f32 * 18.0, 0.0, 0.0)
                    } else {
                        let column = index % 2;
                        let row = index / 2;
                        settlement_focus
                            + Vec3::new(30.0 + column as f32 * 16.0, 0.0, 18.0 + row as f32 * 17.0)
                    };
                    let ground = world
                        .get_resource::<shared::terrain::WorldTerrain>()
                        .map(|t| t.get_height(at.x, at.z))
                        .unwrap_or(at.y);
                    let mut store =
                        shared::economy::GoodsInventory::new(kind.storage_bulk_capacity());
                    match kind {
                        K::Farmstead => {
                            store.add(shared::economy::Good::Wheat, 11);
                        }
                        K::LumberjackHut => {
                            store.add(shared::economy::Good::Wood, 6);
                        }
                        K::Windmill => {
                            store.add(shared::economy::Good::Wheat, 8);
                            store.add(shared::economy::Good::Flour, 3);
                        }
                        K::Bakery => {
                            store.add(shared::economy::Good::Flour, 6);
                            store.add(shared::economy::Good::Bread, 160);
                        }
                        K::Market => {
                            store.add(shared::economy::Good::Food, 18);
                            store.add(shared::economy::Good::Wood, 10);
                        }
                        _ => unreachable!(),
                    }
                    let operator = &people[index % people.len()];
                    let mut business = shared::economy::BusinessAccount::with_capital(2_000);
                    business.record_sale(11, 800 + index as u64 * 125, 40, 4);
                    business.incur_wages(11, 200);
                    business.settle_wage_claim(200);
                    business.roll_to_day(12);
                    let building_entity = world
                        .spawn((
                        shared::components::SettlementBuilding {
                            kind,
                            settlement: "Brackwater".to_string(),
                            owner: Some(operator.clone()),
                            // Stand-ins, like the resident names above. The
                            // client has no BiomeField truth to sample and must
                            // not invent one -- these numbers exist so the panel
                            // has something to lay out, nothing more.
                            quality: if index == 0 { 0.82 } else { 0.41 },
                            workers: vec![operator.clone()],
                        },
                        shared::components::PlayerPosition(Vec3::new(at.x, ground, at.z)),
                        shared::components::PlayerRotation(0.0),
                        shared::components::BuildingId(100 + index as u64),
                        store,
                        business,
                        shared::economy::BusinessCondition::default(),
                        shared::economy::BusinessManagementPolicy::default(),
                        shared::economy::BusinessProcurementPolicy::default(),
                        shared::economy::BusinessSalePolicy::for_good(match kind {
                            K::Farmstead => shared::economy::Good::Wheat,
                            K::LumberjackHut => shared::economy::Good::Wood,
                            K::Windmill => shared::economy::Good::Flour,
                            K::Bakery => shared::economy::Good::Bread,
                            K::Market => shared::economy::Good::Wood,
                            _ => unreachable!(),
                        }),
                        shared::economy::BusinessWagePolicy::default(),
                        shared::components::BuildingDoorDemand {
                            open: std::env::var("FISTFORCE_CAPTURE_DOORS")
                                .is_ok_and(|value| value == "open"),
                        },
                    ))
                        .id();
                    if kind == K::Bakery {
                        // Visual fixture equivalent of a staffed, supplied
                        // bakery: exercise the same replicated transition that
                        // live server production drives.
                        world.entity_mut(building_entity).insert(
                            shared::components::WorkplaceOperation { active_workers: 1 },
                        );
                    }
                    if kind == K::Market {
                        world.entity_mut(building_entity).insert(
                            if mode == "market_paved" {
                                shared::components::MarketLevel::Paved
                            } else {
                                shared::components::MarketLevel::Earthen
                            },
                        );
                    }
                }
                if mode == "village" {
                    let house_at = focus + Vec3::new(15.0, 0.0, -12.0);
                    let house_ground = world
                        .get_resource::<shared::terrain::WorldTerrain>()
                        .map(|t| t.get_height(house_at.x, house_at.z))
                        .unwrap_or(house_at.y);
                    world.spawn((
                        shared::components::SettlementBuilding {
                            kind: K::House,
                            settlement: "Brackwater".to_string(),
                            owner: Some(people[2].clone()),
                            quality: 0.5,
                            workers: Vec::new(),
                        },
                        shared::components::Household {
                            residents: people.clone(),
                            ..default()
                        },
                        shared::components::PlayerPosition(Vec3::new(
                            house_at.x,
                            house_ground,
                            house_at.z,
                        )),
                        // Broadside to the default capture camera so the authored
                        // pane, rather than only its edge, is available for visual
                        // day/night comparison.
                        shared::components::PlayerRotation(1.02),
                    ));
                    // A site needs a POSITION as well as its record -- the raise
                    // visual is placed from it, and without one the frame has
                    // nowhere to come out of.
                    let site_at = focus + Vec3::new(-14.0, 0.0, 10.0);
                    let site_ground = world
                        .get_resource::<shared::terrain::WorldTerrain>()
                        .map(|t| t.get_height(site_at.x, site_at.z))
                        .unwrap_or(site_at.y);
                    world.spawn((
                        shared::components::ConstructionSite {
                            kind: K::House,
                            settlement: "Brackwater".to_string(),
                            // Mid-raise, so a screenshot catches the frame partly
                            // out of the ground rather than an empty plot.
                            raising: true,
                            stand: shared::components::builder_stand_position(
                                site_at,
                                0.9,
                                K::House.art().definition().footprint.y,
                            ),
                            // Deliberately NOT zero: a rotated site is the case
                            // where a mismatch between the rising frame and the
                            // finished building would show.
                            rotation: 0.9,
                        },
                        {
                            let mut materials = shared::economy::GoodsInventory::new(
                                K::House.construction_storage_bulk(),
                            );
                            materials.add(
                                shared::economy::Good::Wood,
                                K::House.construction_wood_required(),
                            );
                            materials
                        },
                        shared::components::PlayerPosition(Vec3::new(
                            site_at.x,
                            site_ground,
                            site_at.z,
                        )),
                    ));
                    // A completed connector through the staged plot also makes
                    // this fixture useful for ground-cover exclusion captures.
                    world.spawn(shared::components::VillageRoad {
                        settlement: "Brackwater".to_string(),
                        builder: "Capture Road Steward".to_string(),
                        points: vec![
                            Vec2::new(focus.x - 35.0, focus.z - 3.0),
                            Vec2::new(focus.x + 35.0, focus.z - 3.0),
                        ],
                        built_through: 2,
                        width: 3.0,
                        reserved_width: 4.0,
                        surface: default(),
                        class: default(),
                        stone_committed: 0,
                    });
                }
                if let Some(mut settlement) = world
                    .query::<&mut shared::components::Settlement>()
                    .iter_mut(world)
                    .find(|s| s.name == "Brackwater")
                {
                    settlement.residents = people.len() as u32;
                    settlement.treasury = 2_750;
                }
                if std::env::var("FISTFORCE_CAPTURE_TRADE").is_ok_and(|value| value == "1") {
                    let hall = world
                        .query_filtered::<Entity, With<shared::components::Settlement>>()
                        .iter(world)
                        .find(|entity| {
                            world
                                .get::<shared::components::Settlement>(*entity)
                                .is_some_and(|settlement| settlement.name == "Brackwater")
                        });
                    if let Some(hall) = hall {
                        world.insert_resource(crate::ui::settlement_panel::TradePanelTarget(Some(
                            hall,
                        )));
                    }
                }
                if std::env::var("FISTFORCE_CAPTURE_PROPERTY").is_ok_and(|value| value == "1") {
                    let hall = world
                        .query_filtered::<Entity, With<shared::components::Settlement>>()
                        .iter(world)
                        .find(|entity| {
                            world
                                .get::<shared::components::Settlement>(*entity)
                                .is_some_and(|settlement| settlement.name == "Brackwater")
                        });
                    if let Some(hall) = hall {
                        world.insert_resource(
                            crate::ui::property_market::PropertyMarketTarget(Some(hall)),
                        );
                        world.resource_mut::<crate::ui::player_permits::PendingPermitQuote>().0 =
                            Some(shared::protocol::HeroPermitQuote {
                                settlement: shared::components::SettlementId(1),
                                settlement_name: "Brackwater".into(),
                                kind: shared::components::SettlementBuildingKind::Farmstead,
                                fee: 165,
                                recommended_working_capital: 0,
                                wallet_balance: 1_000,
                                company: None,
                                company_cash: 0,
                            });
                    }
                }
                if let Ok(mode) = std::env::var("FISTFORCE_CAPTURE_HISTORY") {
                    let hall = world
                        .query_filtered::<Entity, With<shared::components::Settlement>>()
                        .iter(world)
                        .find(|entity| {
                            world
                                .get::<shared::components::Settlement>(*entity)
                                .is_some_and(|settlement| settlement.name == "Brackwater")
                        });
                    if let Some(hall) = hall {
                        if mode != "empty" {
                            let mut cache =
                                world.resource_mut::<crate::ui::history::SettlementHistoryCache>();
                            cache.archives.insert(
                                "Brackwater".to_string(),
                                synthetic_settlement_history("Brackwater"),
                            );
                            cache.world = Some(synthetic_world_history());
                        }
                        let _ = hall;
                    }
                }
            } else if mode == "coast" {
                use shared::components::SettlementBuildingKind as K;
                let rotation = 1.309_f32;
                let hut_ground = world
                    .get_resource::<shared::terrain::WorldTerrain>()
                    .map(|terrain| terrain.get_height(focus.x, focus.z))
                    .unwrap_or(focus.y);
                let hut = Vec3::new(focus.x, hut_ground, focus.z);
                let fisher = shared::names::person_name(8_001);
                let mut hut_store =
                    shared::economy::GoodsInventory::new(K::FishermansHut.storage_bulk_capacity());
                hut_store.add(shared::economy::Good::Food, 14);
                world.spawn((
                    shared::components::SettlementBuilding {
                        kind: K::FishermansHut,
                        settlement: "Brackwater".to_string(),
                        owner: Some(fisher.clone()),
                        quality: 0.43,
                        workers: vec![fisher],
                    },
                    hut_store,
                    shared::components::PlayerPosition(hut),
                    shared::components::PlayerRotation(rotation),
                ));

                let mut pier_at = K::FishermansHut
                    .pier_position(hut, rotation)
                    .expect("fisherman's hut has a pier anchor");
                pier_at.y = world
                    .get_resource::<shared::terrain::WorldTerrain>()
                    .and_then(|terrain| terrain.water_level())
                    .unwrap_or(0.0);
                world.spawn((
                    shared::components::FishingPier {
                        settlement: "Brackwater".to_string(),
                        fishermans_hut: hut,
                        quality: 0.43,
                    },
                    shared::components::PlayerPosition(pier_at),
                    shared::components::PlayerRotation(rotation),
                ));
            }
        });
    }

    // FISTFORCE_CAPTURE_HERO_CREATOR=1: open the character-creator modal so
    // captures can verify the live preview + selector UI without a server.
    if std::env::var("FISTFORCE_CAPTURE_HERO_CREATOR").is_ok_and(|v| v == "1") {
        commands.insert_resource(crate::ui::hero_creator::HeroCreatorOpen(true));
    }

    // FISTFORCE_CAPTURE_ENCYCLOPEDIA=1 opens the encyclopedia and seeds a
    // sample cast, so the window can be verified without a server (there is no
    // roster offline, and an empty list photographs nothing).
    if let Ok(mode) = std::env::var("FISTFORCE_CAPTURE_ENCYCLOPEDIA") {
        use crate::ui::encyclopedia::{
            Affiliation, EncyclopediaOpen, KnownPeople, PersonKind, PersonRecord, SelectedPerson,
        };
        // "live" opens the window with NO sample cast, so it fills from the
        // characters actually spawned in the world. That is the only way to
        // verify the real path -- a seeded list proves the layout renders and
        // nothing about whether characters reach it.
        if mode == "live" || mode == "places" {
            commands.insert_resource(EncyclopediaOpen(true));
            commands.insert_resource(crate::ui::hud::GodCapability(true));
            if mode == "places" {
                commands.insert_resource(crate::ui::encyclopedia::EncyclopediaTab::Places);
            }
            return;
        }
        let sample =
            |name: &str, level, prestige, online, known, is_self, affiliation| PersonRecord {
                id: shared::components::PersonId::UNASSIGNED,
                name: name.to_string(),
                kind: PersonKind::Hero,
                affiliation,
                level,
                prestige,
                online,
                alive: true,
                health: Some(shared::components::Health::default()),
                death_day: None,
                death_cause: None,
                known,
                is_self,
                commanded_by: None,
                residence: None,
                home: None,
                occupation: None,
                workplace: None,
                wallet: None,
                nutrition: None,
                activity: None,
                objective: None,
                day_plan: None,
                navigation: None,
                attributes: Some(shared::components::CharacterAttributes::default()),
                work_status: Some(shared::components::WorkStatus::LookingForWork),
                daily_wage: None,
                workforce_requirements: None,
                inventory: Some(shared::economy::GoodsInventory::new(
                    shared::economy::capacity::VILLAGER,
                )),
                carried: Some(shared::economy::CarriedLoad::default()),
            };
        let mut records = vec![
            sample("Aldric", 7, 2, true, true, true, Affiliation::default()),
            sample("Bryn", 4, 0, true, true, false, Affiliation::default()),
            sample(
                "Cassia",
                11,
                5,
                false,
                true,
                false,
                shared::components::CharacterAffiliation(Some(0)),
            ),
            sample("Dunstan", 2, 0, false, true, false, Affiliation::default()),
            sample(
                "Eirwen",
                9,
                3,
                true,
                true,
                false,
                shared::components::CharacterAffiliation(Some(0)),
            ),
            sample("Faelan", 1, 0, false, false, false, Affiliation::default()),
            sample(
                "Gwyneth",
                14,
                8,
                false,
                false,
                false,
                shared::components::CharacterAffiliation(Some(0)),
            ),
            sample("Hollis", 5, 1, false, true, false, Affiliation::default()),
            sample("Ivo", 3, 0, false, false, false, Affiliation::default()),
            sample(
                "Jorunn",
                8,
                4,
                true,
                true,
                false,
                shared::components::CharacterAffiliation(Some(0)),
            ),
            sample("Kelda", 6, 2, false, true, false, Affiliation::default()),
            sample("Lorcan", 12, 6, false, false, false, Affiliation::default()),
        ];
        for (index, record) in records.iter_mut().enumerate() {
            record.id = shared::components::PersonId(index as u64 + 1);
            if record.is_self {
                record.wallet = Some(2_750);
            }
        }
        commands.insert_resource(KnownPeople {
            records,
            requested: true,
        });
        commands.insert_resource(SelectedPerson(Some("Cassia".to_string())));
        commands.insert_resource(EncyclopediaOpen(true));
        // Mode picks which surface to photograph: a tab name, or "god" to
        // grant capability so the unknown-people view can be verified.
        commands.insert_resource(match mode.as_str() {
            "retinue" => crate::ui::encyclopedia::EncyclopediaTab::Retinue,
            "ledger" | "companies" | "company-stock" | "business" => {
                crate::ui::encyclopedia::EncyclopediaTab::Companies
            }
            _ => crate::ui::encyclopedia::EncyclopediaTab::People,
        });
        if matches!(
            mode.as_str(),
            "ledger" | "companies" | "company-stock" | "business"
        ) {
            stage_capture_companies(&mut commands);
            commands.insert_resource(crate::ui::encyclopedia::companies::SelectedCompany(Some(
                shared::components::CompanyId(501),
            )));
        }
        if mode == "god" {
            commands.insert_resource(crate::ui::hud::GodCapability(true));
        }
    }

    info!("capture: entering world offline (no server)");
}

/// Put the capture camera on the local branch controls without baking a
/// special layout into the real encyclopedia. This keeps the scrollable
/// directory itself under visual regression coverage.
fn position_capture_company_scroll(
    mut viewports: Query<
        &mut ScrollPosition,
        With<crate::ui::encyclopedia::companies::CompanyDetailViewport>,
    >,
) {
    if std::env::var("FISTFORCE_CAPTURE_ENCYCLOPEDIA").as_deref() != Ok("company-stock") {
        return;
    }
    for mut position in viewports.iter_mut() {
        position.y = 410.0;
    }
}

fn stage_capture_companies(commands: &mut Commands) {
    use shared::components::{
        BuildingId, BuildingOf, Company, CompanyId, CompanyLeadership, CompanyOwnership,
        CompanyShare, CompanyShareMarket, OperatedBy, PersonId, SettlementBuilding,
        SettlementBuildingKind, SettlementId,
    };
    use shared::economy::{
        BusinessAccount, BusinessCondition, BusinessInputRule, BusinessManagementPolicy,
        BusinessPrivateInputRule, BusinessProcurementPolicy, BusinessSalePolicy,
        BusinessStaffingPolicy, BusinessSupplyPolicy, BusinessWagePolicy, CompanyAccount,
        CompanyBranchPolicies, CompanyDayLedger, CompanyDecisionHistory, CompanyDecisionReason,
        CompanyDecisionRecord, CompanyManagementPolicy, CompanyResourcePolicy, Good,
        GoodsInventory,
    };

    let aldric =
        if std::env::var("FISTFORCE_CAPTURE_ENCYCLOPEDIA").is_ok_and(|mode| mode == "business") {
            // `spawn_capture_heroes` gives the first local stand-in this stable id.
            // Making that person the Master exposes the real authoritative controls.
            PersonId(10_000)
        } else {
            PersonId(1)
        };
    let bryn = PersonId(2);
    let cassia = PersonId(3);
    let first_ownership = CompanyOwnership::from_shares(vec![
        CompanyShare {
            shareholder: aldric,
            shares: 720,
        },
        CompanyShare {
            shareholder: bryn,
            shares: 280,
        },
    ])
    .expect("capture cap table totals 1,000");
    let mut first_market = CompanyShareMarket::default();
    assert!(first_market.list(&first_ownership, bryn, 80, 145, 11));
    let mut decisions = CompanyDecisionHistory::default();
    decisions.push(CompanyDecisionRecord {
        day: 8,
        master: aldric,
        from: shared::economy::BusinessStrategy::Balanced,
        to: shared::economy::BusinessStrategy::Growth,
        reason: CompanyDecisionReason::ProfitableExpansion,
    });
    let brackwater = SettlementId(701);
    let high_meadow = SettlementId(702);
    let rivermeet = SettlementId(703);
    let mut first_branches = CompanyBranchPolicies::default();
    first_branches.set_resource(
        brackwater,
        Good::Wheat,
        CompanyResourcePolicy {
            retain_units: 24,
            sell_excess: true,
        },
    );
    first_branches.set_resource(
        brackwater,
        Good::Flour,
        CompanyResourcePolicy {
            retain_units: 18,
            sell_excess: true,
        },
    );
    first_branches.set_resource(
        high_meadow,
        Good::Bread,
        CompanyResourcePolicy {
            retain_units: 8,
            sell_excess: false,
        },
    );
    commands.spawn((
        CompanyId(501),
        Company {
            name: "Aldric Grain & Bread".to_string(),
            founded_day: 2,
        },
        first_ownership,
        CompanyLeadership { master: aldric },
        first_market,
        CompanyAccount {
            cash: 8_950,
            contributed_capital: 4_000,
            capital_expenditures: 2_700,
            book_value: 2_700,
            owner_withdrawals: 1_250,
            current_day: CompanyDayLedger {
                day: 12,
                external_revenue: 2_480,
                wage_expense: 700,
                external_input_expense: 220,
                market_fees: 124,
                delivery_fees: 40,
                profit_taxes: 140,
                owner_withdrawals: 350,
                capital_expenditures: 0,
                internal_revenue: 1_100,
                internal_input_expense: 1_100,
            },
            previous_day: CompanyDayLedger {
                day: 11,
                external_revenue: 2_150,
                wage_expense: 700,
                external_input_expense: 180,
                market_fees: 108,
                delivery_fees: 40,
                profit_taxes: 112,
                owner_withdrawals: 250,
                capital_expenditures: 0,
                internal_revenue: 900,
                internal_input_expense: 900,
            },
            ..default()
        },
        CompanyManagementPolicy {
            strategy: shared::economy::BusinessStrategy::Growth,
            ..default()
        },
        first_branches,
        decisions,
    ));

    let second_ownership = CompanyOwnership::from_shares(vec![
        CompanyShare {
            shareholder: cassia,
            shares: 880,
        },
        CompanyShare {
            shareholder: aldric,
            shares: 120,
        },
    ])
    .expect("capture cap table totals 1,000");
    commands.spawn((
        CompanyId(502),
        Company {
            name: "Cassia River Fish".to_string(),
            founded_day: 5,
        },
        second_ownership,
        CompanyLeadership { master: cassia },
        CompanyShareMarket::default(),
        CompanyAccount {
            cash: 2_240,
            book_value: 900,
            wage_arrears: 125,
            current_day: CompanyDayLedger {
                day: 12,
                external_revenue: 650,
                wage_expense: 300,
                market_fees: 32,
                delivery_fees: 25,
                ..CompanyDayLedger::empty(12)
            },
            ..default()
        },
        CompanyManagementPolicy::default(),
        CompanyBranchPolicies::default(),
        CompanyDecisionHistory::default(),
    ));

    let sites = [
        (
            BuildingId(601),
            SettlementBuildingKind::Farmstead,
            "Brackwater",
            brackwater,
            Good::Wheat,
            18,
            4_100,
        ),
        (
            BuildingId(602),
            SettlementBuildingKind::Windmill,
            "Brackwater",
            brackwater,
            Good::Flour,
            50,
            2_450,
        ),
        (
            BuildingId(603),
            SettlementBuildingKind::Bakery,
            "High Meadow",
            high_meadow,
            Good::Bread,
            12,
            2_400,
        ),
    ];
    for (index, (id, kind, settlement, settlement_id, output, stock, cash)) in
        sites.into_iter().enumerate()
    {
        let mut inventory = GoodsInventory::new(kind.storage_bulk_capacity());
        inventory.add(output, stock);
        let mut procurement = BusinessProcurementPolicy::none();
        let mut supply = BusinessSupplyPolicy::none();
        if let Some(recipe) = match kind {
            SettlementBuildingKind::Windmill => Some((Good::Wheat, 9, 18, 250)),
            SettlementBuildingKind::Bakery => Some((Good::Flour, 15, 30, 300)),
            _ => None,
        } {
            procurement.set_rule(
                recipe.0,
                BusinessInputRule {
                    enabled: true,
                    coverage_days: 2,
                    reorder_below: recipe.1,
                    target_units: recipe.2,
                    maximum_unit_price: recipe.3,
                },
            );
            supply.set_rule(
                recipe.0,
                BusinessPrivateInputRule {
                    enabled: true,
                    ..default()
                },
            );
            inventory.add(recipe.0, recipe.1);
        }
        let mut account = BusinessAccount::with_capital(cash);
        account.current_day = shared::economy::BusinessDayLedger {
            day: 12,
            gross_revenue: 700 + index as u64 * 240,
            internal_revenue: if index < 2 { 350 } else { 0 },
            wage_expense: 200,
            market_fees: 35,
            profit_taxes: 45,
            ..shared::economy::BusinessDayLedger::empty(12)
        };
        let sale = BusinessSalePolicy::for_good(output);
        commands.spawn((
            id,
            BuildingOf(settlement_id),
            OperatedBy(CompanyId(501)),
            SettlementBuilding {
                kind,
                settlement: settlement.to_string(),
                owner: Some("Aldric".to_string()),
                quality: 0.8,
                workers: vec!["Worker".to_string(); usize::from(kind.positions())],
            },
            inventory,
            account,
            BusinessCondition {
                state: shared::economy::BusinessState::Operating,
                ..default()
            },
            sale,
            BusinessManagementPolicy::default(),
            BusinessWagePolicy::default(),
            BusinessStaffingPolicy::new(kind.positions()),
            procurement,
            supply,
        ));
    }

    let mut depot_stock = GoodsInventory::new(shared::economy::capacity::STORAGE_HALL);
    depot_stock.add(Good::Wheat, 70);
    depot_stock.add(Good::Flour, 32);
    commands.spawn((
        BuildingId(605),
        BuildingOf(brackwater),
        OperatedBy(CompanyId(501)),
        SettlementBuilding {
            kind: SettlementBuildingKind::StorageHall,
            settlement: "Brackwater".to_string(),
            owner: Some("Aldric".to_string()),
            quality: 0.5,
            workers: vec!["Company Porter".to_string()],
        },
        depot_stock,
        BusinessAccount::with_capital(0),
        BusinessCondition::default(),
        BusinessSalePolicy::default(),
        BusinessManagementPolicy::default(),
        BusinessWagePolicy::default(),
        BusinessStaffingPolicy::new(1),
        BusinessProcurementPolicy::none(),
        BusinessSupplyPolicy::none(),
    ));

    let mut fish =
        GoodsInventory::new(SettlementBuildingKind::FishermansHut.storage_bulk_capacity());
    fish.add(Good::Food, 9);
    commands.spawn((
        BuildingId(604),
        BuildingOf(rivermeet),
        OperatedBy(CompanyId(502)),
        SettlementBuilding {
            kind: SettlementBuildingKind::FishermansHut,
            settlement: "Rivermeet".to_string(),
            owner: Some("Cassia".to_string()),
            quality: 0.7,
            workers: vec!["Fisher".to_string()],
        },
        fish,
        BusinessAccount::with_capital(2_240),
        BusinessCondition::default(),
        BusinessSalePolicy::for_good(Good::Food),
        BusinessManagementPolicy::default(),
        BusinessWagePolicy::default(),
        BusinessStaffingPolicy::new(1),
        BusinessProcurementPolicy::none(),
        BusinessSupplyPolicy::default(),
    ));

    commands.queue(|world: &mut World| {
        world
            .resource_mut::<crate::ui::history::SettlementHistoryCache>()
            .companies
            .insert(CompanyId(501), synthetic_company_history(CompanyId(501)));
    });
}

/// Open the actual site-management modal over the staged company directory.
/// This is a rendering fixture only; it does not invent a second UI model.
fn open_capture_business_management(
    heroes: Query<(&shared::components::Hero, &shared::components::PersonId)>,
    sites: Query<(Entity, &shared::components::BuildingId)>,
    mut target: ResMut<crate::ui::business_management::BusinessManagementTarget>,
    mut return_to: ResMut<crate::ui::business_management::BusinessManagementReturn>,
    mut encyclopedia: ResMut<crate::ui::encyclopedia::EncyclopediaOpen>,
    mut opened: Local<bool>,
    mut commands: Commands,
) {
    if *opened
        || !std::env::var("FISTFORCE_CAPTURE_ENCYCLOPEDIA").is_ok_and(|mode| mode == "business")
    {
        return;
    }
    let Some((hero, _)) = heroes.iter().next() else {
        return;
    };
    let Some((site, _)) = sites.iter().find(|(_, id)| id.0 == 602) else {
        return;
    };
    commands.insert_resource(crate::camera_rts::LocalPeerId(
        shared::player::peer_id_to_u64(hero.owner),
    ));
    target.0 = Some(site);
    return_to.0 = Some(shared::components::CompanyId(501));
    encyclopedia.0 = false;
    *opened = true;
}

/// Open history after the commander camera has rendered ordinary world frames.
/// A modal present on the very first offline capture frame prevents the capture
/// harness's camera from completing its initial convergence; real players can
/// only open this after entering the world, so the delay mirrors actual use.
fn open_capture_history(
    settlements: Query<(Entity, &shared::components::Settlement)>,
    mut target: ResMut<crate::ui::history::HistoryPanelTarget>,
    mut frames: Local<u8>,
) {
    let Ok(mode) = std::env::var("FISTFORCE_CAPTURE_HISTORY") else {
        return;
    };
    if target.0.is_some() {
        return;
    }
    *frames = frames.saturating_add(1);
    if *frames < 30 {
        return;
    }
    let hall = settlements
        .iter()
        .find(|(_, settlement)| settlement.name == "Brackwater")
        .map(|(entity, _)| entity);
    let view = match mode.as_str() {
        "market" => crate::ui::history::HistoryView::Market(shared::economy::Good::Wood),
        "business" => {
            crate::ui::history::HistoryView::Business(shared::components::BuildingId(100))
        }
        "world" => crate::ui::history::HistoryView::World,
        "company" => crate::ui::history::HistoryView::Company(shared::components::CompanyId(501)),
        _ => crate::ui::history::HistoryView::Village,
    };
    let global_view = matches!(
        view,
        crate::ui::history::HistoryView::World | crate::ui::history::HistoryView::Company(_)
    );
    let settlement = if global_view { None } else { hall };
    if !global_view && settlement.is_none() {
        return;
    }
    target.0 = Some(crate::ui::history::HistoryTarget {
        settlement,
        place: match view {
            crate::ui::history::HistoryView::World => "World".to_string(),
            crate::ui::history::HistoryView::Company(_) => "Aldric Grain & Bread".to_string(),
            _ => "Brackwater".to_string(),
        },
        view,
        return_to_trade: false,
    });
}

/// FISTFORCE_CAPTURE_HERO spawns stand-in heroes (offline fakes of the
/// replicated entity) in a line at the first shot's focus, terrain-snapped, so
/// captures can verify the character model, wardrobe and pose without a
/// server.
///
/// Spec: `slot0,slot1,...,skin` per hero, semicolon-separated, all indices
/// into the manifest's slot items / skin tones (order as in Humanoid.ron:
/// bottom, top, hair). Missing or unparsable fields use the manifest default.
/// `FISTFORCE_CAPTURE_HERO=default` spawns one hero in the declared default.
/// `FISTFORCE_CAPTURE_HERO_OFFSET=x,z` offsets those heroes from the shot focus,
/// which is useful for verifying world-space UI such as the minimap marker.
/// `FISTFORCE_CAPTURE_PORTER_CART=0|1|2` gives that fixture an empty, half or
/// full cart while `FISTFORCE_CAPTURE_CARRIED` chooses its visible cargo.
fn spawn_capture_heroes(
    mut commands: Commands,
    config: Res<CaptureConfig>,
    terrain: Option<Res<shared::terrain::WorldTerrain>>,
    mut spawned: Local<bool>,
) {
    if *spawned {
        return;
    }
    let mut hero_spec = std::env::var("FISTFORCE_CAPTURE_HERO").ok();
    let hero_offset = std::env::var("FISTFORCE_CAPTURE_HERO_OFFSET")
        .ok()
        .and_then(|raw| {
            let mut parts = raw.split(',').map(|part| part.trim().parse::<f32>());
            match (parts.next(), parts.next(), parts.next()) {
                (Some(Ok(x)), Some(Ok(z)), None) if x.is_finite() && z.is_finite() => {
                    Some(Vec2::new(x, z))
                }
                _ => None,
            }
        })
        .unwrap_or(Vec2::ZERO);
    let villager_count = std::env::var("FISTFORCE_CAPTURE_VILLAGERS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok());
    let cart_slots = std::env::var("FISTFORCE_CAPTURE_PORTER_CART")
        .ok()
        .and_then(|value| value.parse::<u8>().ok())
        .map(|slots| slots.min(2));
    // Comma-separated authored bundle appearances. Supplying this alone
    // creates one default hero, making the first-integration WoodBundle shot a
    // one-flag exercise rather than a bespoke capture path.
    let carried = std::env::var("FISTFORCE_CAPTURE_CARRIED")
        .ok()
        .map(|spec| {
            spec.split(',')
                .filter_map(|raw| match raw.trim().to_ascii_lowercase().as_str() {
                    "wood" => Some((
                        shared::economy::Good::Wood,
                        shared::economy::CarriedAppearance::WoodBundle,
                    )),
                    "wheat" => Some((
                        shared::economy::Good::Wheat,
                        shared::economy::CarriedAppearance::WheatSheaf,
                    )),
                    "fish" | "food" => Some((
                        shared::economy::Good::Food,
                        shared::economy::CarriedAppearance::FishBasket,
                    )),
                    "stone" => Some((
                        shared::economy::Good::Stone,
                        shared::economy::CarriedAppearance::StoneBundle,
                    )),
                    "iron" => Some((
                        shared::economy::Good::Iron,
                        shared::economy::CarriedAppearance::IronBundle,
                    )),
                    _ => None,
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    // Comma-separated visible work states. This drives the same replicated
    // activity component as a live village, so authored clips and hand tools
    // can be reviewed offline without waiting for a worker cycle.
    let activities = std::env::var("FISTFORCE_CAPTURE_ACTIVITY")
        .ok()
        .map(|spec| {
            spec.split(',')
                .filter_map(|raw| match raw.trim().to_ascii_lowercase().as_str() {
                    "build" | "building" => Some(shared::components::CharacterActivity::Building),
                    "chop" | "chopping" => Some(shared::components::CharacterActivity::Chopping),
                    "farm" | "farming" | "harvest" => {
                        Some(shared::components::CharacterActivity::Farming)
                    }
                    "fish" | "fishing" => Some(shared::components::CharacterActivity::Fishing),
                    "mine" | "mining" | "quarrying" => {
                        Some(shared::components::CharacterActivity::Mining)
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if hero_spec.is_none()
        && villager_count.is_none()
        && carried.is_empty()
        && activities.is_empty()
        && cart_slots.is_none()
    {
        *spawned = true;
        return;
    }
    if hero_spec.is_none()
        && (!carried.is_empty() || !activities.is_empty() || cart_slots.is_some())
    {
        let fixture_count = carried.len().max(activities.len()).max(1);
        hero_spec = Some(vec!["default"; fixture_count].join(";"));
    }
    let Some(terrain) = terrain else {
        return;
    };
    let base = config.shots.first().map(|s| s.focus).unwrap_or(Vec3::ZERO);
    let spec = hero_spec.unwrap_or_default();
    // Indices are validated against the same manifest the renderer uses.
    let manifest = match shared::character::CharacterManifest::load() {
        Ok(manifest) => manifest,
        Err(e) => {
            error!("capture: character manifest unavailable: {e}");
            *spawned = true;
            return;
        }
    };
    let default_outfit = shared::components::HeroOutfit::from_manifest(&manifest);
    for (i, outfit_spec) in spec.split(';').filter(|s| !s.is_empty()).enumerate() {
        // Positional parse: a bad token falls back to the default for THAT
        // field instead of shifting later fields left.
        let parts: Vec<Option<u8>> = outfit_spec
            .split(',')
            .map(|p| p.trim().parse().ok())
            .collect();
        let mut outfit = default_outfit;
        for (slot_index, _) in manifest.slots.iter().enumerate() {
            if let Some(Some(value)) = parts.get(slot_index) {
                outfit.slots[slot_index] = *value;
            }
        }
        if let Some(Some(skin)) = parts.get(manifest.slots.len()) {
            outfit.skin = *skin;
        }
        let x = base.x + hero_offset.x + i as f32 * 1.4;
        let z = base.z + hero_offset.y;
        let pos = Vec3::new(x, terrain.get_height(x, z), z);
        let entity = commands
            .spawn((
                shared::components::Hero {
                    owner: lightyear::prelude::PeerId::Netcode(1000 + i as u64),
                },
                shared::components::CharacterName(format!("Capture Hero {}", i + 1)),
                shared::components::CharacterKind::Hero,
                shared::components::CharacterAffiliation::default(),
                shared::components::PersonId(10_000 + i as u64),
                outfit,
                shared::economy::Wallet::new(1_000),
                shared::components::PlayerPermitLedger::default(),
                shared::components::PlayerPosition(pos),
                shared::components::PlayerRotation(std::f32::consts::PI),
            ))
            .id();
        if let Some((good, appearance)) = carried.get(i % carried.len().max(1)).copied() {
            let amount = if cart_slots == Some(2) {
                shared::economy::capacity::PORTER / good.bulk_per_unit()
            } else {
                1
            };
            commands
                .entity(entity)
                .insert(shared::economy::CarriedLoad {
                    good: Some(good),
                    amount,
                    appearance: Some(appearance),
                });
        }
        if let Some(load_slots) = cart_slots {
            commands
                .entity(entity)
                .insert(shared::economy::PorterCartState { load_slots });
        }
        if let Some(activity) = activities.get(i % activities.len().max(1)).copied() {
            commands.entity(entity).insert(activity);
        }
    }
    // FISTFORCE_CAPTURE_VILLAGERS=<n> drops n named villagers in a row behind the
    // heroes, so the encyclopedia and the character visuals can be verified
    // without a server. Names come from the same generator the server uses.
    if let Some(count) = villager_count {
        for i in 0..count {
            let x = base.x + i as f32 * 1.5 - (count as f32 * 0.75);
            let z = base.z + 3.0;
            let pos = Vec3::new(x, terrain.get_height(x, z), z);
            let seed = 1_000 + i as u64;
            let entity = commands
                .spawn((
                    shared::components::CharacterName(shared::names::person_name(seed)),
                    shared::components::CharacterKind::Villager,
                    shared::components::CharacterAffiliation::default(),
                    shared::components::HeroOutfit::varied(seed),
                    shared::components::PlayerPosition(pos),
                    shared::components::PlayerRotation(std::f32::consts::PI),
                ))
                .id();
            if let Some((good, appearance)) = carried.get(i % carried.len().max(1)).copied() {
                commands
                    .entity(entity)
                    .insert(shared::economy::CarriedLoad {
                        good: Some(good),
                        amount: 1,
                        appearance: Some(appearance),
                    });
            }
            if let Some(activity) = activities.get(i % activities.len().max(1)).copied() {
                commands.entity(entity).insert(activity);
            }
        }
    }

    // FISTFORCE_CAPTURE_SELECT=1 selects the FIRST fake hero, so the ground ring
    // and the selected-unit plate can be verified without a server. Deferred by
    // a command so it runs after the spawns above are applied.
    // FISTFORCE_CAPTURE_SELECT=all force-selects EVERY character, including ones
    // you do not own. This is a stress harness for the ring pool and the group
    // HUD, NOT a picture of what a box-drag produces: a real drag filters to
    // your own units (see selection::pick). Do not read it as the game's rule.
    if std::env::var("FISTFORCE_CAPTURE_SELECT").is_ok_and(|v| v == "all") {
        commands.queue(|world: &mut World| {
            let mut all = world.query_filtered::<Entity, With<shared::components::CharacterName>>();
            let entities: Vec<Entity> = all.iter(world).collect();
            if let Some((first, hero)) = world
                .query::<(Entity, &shared::components::Hero)>()
                .iter(world)
                .next()
            {
                let owner = shared::player::peer_id_to_u64(hero.owner);
                let _ = first;
                world.insert_resource(crate::camera_rts::LocalPeerId(owner));
            }
            world.resource_mut::<crate::selection::Selection>().entities = entities;
        });
    } else if std::env::var("FISTFORCE_CAPTURE_SELECT").is_ok_and(|v| v == "market") {
        // Prefer the hall carrying the actual market components. Useful when a
        // staged capture also contains several distant list-only settlements.
        commands.queue(|world: &mut World| {
            let entity = world
                .query_filtered::<Entity, (
                    With<shared::components::Settlement>,
                    With<shared::economy::MootMarket>,
                )>()
                .iter(world)
                .next();
            if let Some(entity) = entity {
                world.resource_mut::<crate::selection::Selection>().entities = vec![entity];
            }
        });
    } else if std::env::var("FISTFORCE_CAPTURE_SELECT").is_ok_and(|v| v == "hall") {
        // Selects the first SETTLEMENT rather than a person, so the settlement
        // panel can be photographed. A place is never commandable, so this is
        // always a single selection.
        commands.queue(|world: &mut World| {
            let entity = world
                .query_filtered::<Entity, With<shared::components::Settlement>>()
                .iter(world)
                .next();
            if let Some(entity) = entity {
                world.resource_mut::<crate::selection::Selection>().entities = vec![entity];
            }
        });
    } else if std::env::var("FISTFORCE_CAPTURE_SELECT").is_ok_and(|v| v == "1") {
        commands.queue(|world: &mut World| {
            let mut heroes = world.query_filtered::<(Entity, &shared::components::Hero), ()>();
            if let Some((first, hero)) = heroes.iter(world).next() {
                let owner = shared::player::peer_id_to_u64(hero.owner);
                world.resource_mut::<crate::selection::Selection>().entities = vec![first];
                let _ = &owner;
                // Claim ownership of it too, so the shot shows the state a real
                // player sees (ember mark, own name) rather than "NOT YOURS".
                world.insert_resource(crate::camera_rts::LocalPeerId(owner));
            }
        });
    }
    *spawned = true;
}

/// Reproducible offline fixture for the actual permit placement presentation.
///
/// `FISTFORCE_CAPTURE_PERMIT_PLACEMENT=farm|lumber|house` uses the real
/// placement systems, terrain and building assets. Only the server response is
/// staged; no separate mock ghost or mock quality calculation exists here.
fn stage_capture_permit_placement(
    mut commands: Commands,
    config: Res<CaptureConfig>,
    settlements: Query<(
        &shared::components::SettlementId,
        &shared::components::PlayerPosition,
    )>,
    mut placement: ResMut<crate::hero::control::WorldPlacementMode>,
    mut staged: Local<bool>,
) {
    if *staged {
        return;
    }
    let Ok(raw) = std::env::var("FISTFORCE_CAPTURE_PERMIT_PLACEMENT") else {
        *staged = true;
        return;
    };
    let kind = match raw.trim().to_ascii_lowercase().as_str() {
        "farm" | "farmstead" => shared::components::SettlementBuildingKind::Farmstead,
        "lumber" | "lumberjack" => shared::components::SettlementBuildingKind::LumberjackHut,
        "house" | "cabin" => shared::components::SettlementBuildingKind::House,
        other => {
            warn!("capture: unknown permit-placement kind '{other}'");
            *staged = true;
            return;
        }
    };
    let Some((settlement_id, hall)) = settlements.iter().next() else {
        return;
    };
    let focus = config.shots.first().map_or(Vec3::ZERO, |shot| shot.focus);
    let cursor = Vec2::new(focus.x, focus.z);
    commands.insert_resource(crate::camera_rts::CursorTerrainOverride(cursor));
    let hall_door = shared::components::SettlementBuildingKind::Hall.entrance_position(hall.0, 0.0);
    commands.spawn((
        shared::components::VillageRoad {
            settlement: "Brackwater".into(),
            builder: "Capture Road Steward".into(),
            points: vec![Vec2::new(hall_door.x, hall_door.z), cursor],
            built_through: 2,
            width: 3.0,
            reserved_width: 4.0,
            surface: shared::components::RoadSurface::Dirt,
            class: shared::components::RoadClass::Lane,
            stone_committed: 0,
        },
        shared::components::RoadOf(*settlement_id),
    ));
    let permit = shared::components::PlayerPermit {
        id: shared::components::PermitId(900),
        settlement: *settlement_id,
        kind,
        fee_escrow: if kind == shared::components::SettlementBuildingKind::House {
            0
        } else {
            165
        },
        purchased_day: 1,
        company: None,
    };
    *placement = crate::hero::control::WorldPlacementMode::Permit {
        permit,
        settlement_name: "Brackwater".into(),
        rotation: 0.0,
    };
    *staged = true;
}

/// FISTFORCE_CAPTURE_DRAG_BOX="x0,y0,x1,y1[,ui_scale]" pins the drag-select
/// marquee to a known rectangle in WINDOW pixels, so where it actually lands on
/// screen can be measured instead of eyeballed.
///
/// The optional ui_scale reproduces the macOS setup, where the window takes a
/// scale-factor override of 1.0 and the Retina factor lives in `UiScale` -- the
/// exact condition under which cursor pixels and UI pixels stop being the same
/// unit, which is what put the marquee in the wrong place.
fn force_capture_drag_box(
    mut drag: ResMut<crate::selection::DragBox>,
    mut ui_scale: ResMut<bevy::ui::UiScale>,
) {
    let Ok(spec) = std::env::var("FISTFORCE_CAPTURE_DRAG_BOX") else {
        return;
    };
    let parts: Vec<f32> = spec
        .split(',')
        .filter_map(|p| p.trim().parse().ok())
        .collect();
    if parts.len() < 4 {
        return;
    }
    if let Some(scale) = parts.get(4) {
        if ui_scale.0 != *scale {
            ui_scale.0 = *scale;
        }
    }
    drag.start = Some(Vec2::new(parts[0], parts[1]));
    drag.current = Vec2::new(parts[2], parts[3]);
    drag.active = true;
}

/// FISTFORCE_CAPTURE_SELECT_PERSON=<name> selects that person in the
/// encyclopedia once they actually exist.
///
/// Retried rather than set once: `rebuild_people_list` drops a selection that is
/// not in the visible list, and at startup the list is empty, so a one-shot set
/// is cleared before the characters have even been learned.
/// FISTFORCE_CAPTURE_SELECT_PLACE=<name>, retried for the same reason as the
/// person selector: the list is empty at startup and drops a selection it does
/// not contain.
fn select_capture_place(
    places: Res<crate::ui::encyclopedia::places::KnownPlaces>,
    mut selected: ResMut<crate::ui::encyclopedia::places::SelectedPlace>,
    mut entry: ResMut<crate::ui::encyclopedia::places::SelectedPlaceEntry>,
) {
    let Ok(wanted) = std::env::var("FISTFORCE_CAPTURE_SELECT_PLACE") else {
        return;
    };
    let Some(place) = places.find(&wanted) else {
        return;
    };
    if selected.0.as_deref() != Some(wanted.as_str()) {
        selected.0 = Some(wanted);
        *entry = crate::ui::encyclopedia::places::SelectedPlaceEntry::Overview;
    }
    let Ok(building) = std::env::var("FISTFORCE_CAPTURE_SELECT_BUILDING") else {
        return;
    };
    if building.eq_ignore_ascii_case("overview") {
        *entry = crate::ui::encyclopedia::places::SelectedPlaceEntry::Overview;
    } else if building.eq_ignore_ascii_case("hall")
        || building.eq_ignore_ascii_case(shared::components::SettlementBuildingKind::Hall.label())
    {
        *entry = crate::ui::encyclopedia::places::SelectedPlaceEntry::Hall;
    } else if let Some((index, _)) = place
        .buildings
        .iter()
        .enumerate()
        .find(|(_, record)| record.kind.label().eq_ignore_ascii_case(&building))
    {
        *entry = crate::ui::encyclopedia::places::SelectedPlaceEntry::Building(index);
    }
}

fn select_capture_person(
    people: Res<crate::ui::encyclopedia::KnownPeople>,
    mut selected: ResMut<crate::ui::encyclopedia::SelectedPerson>,
) {
    if selected.0.is_some() {
        return;
    }
    let Ok(wanted) = std::env::var("FISTFORCE_CAPTURE_SELECT_PERSON") else {
        return;
    };
    if wanted.eq_ignore_ascii_case("first") {
        if let Some(record) = people.records.iter().find(|record| record.known) {
            selected.0 = Some(record.name.clone());
        }
        return;
    }
    if wanted.eq_ignore_ascii_case("hungry") {
        if let Some(record) = people.records.iter().find(|record| {
            record
                .nutrition
                .is_some_and(|nutrition| nutrition.is_hungry())
        }) {
            selected.0 = Some(record.name.clone());
        }
        return;
    }
    if people.find(&wanted).is_some() {
        selected.0 = Some(wanted);
    }
}

#[allow(clippy::too_many_arguments)]
fn drive_capture(
    mut commands: Commands,
    config: Res<CaptureConfig>,
    mut state: ResMut<CaptureState>,
    mut cameras: Query<&mut CommanderCamera>,
    mut world_time: Query<&mut WorldTime>,
    loaded_chunks: Option<Res<LoadedChunks>>,
    mut free_look: ResMut<CaptureFreeLook>,
    mut pending_probes: Local<Vec<PathBuf>>,
    mut app_exit: MessageWriter<AppExit>,
) {
    match &mut *state {
        CaptureState::Warmup { frames_left } => {
            // Park the camera on the first shot during warmup so streaming loads the
            // right chunks rather than whatever is around the origin.
            if let Some(shot) = config.shots.first() {
                apply_shot(shot, &mut cameras, &mut world_time, &mut free_look);
            }

            if *frames_left > 0 {
                *frames_left -= 1;
                return;
            }
            info!(
                "capture: warmup complete, {} shot(s) queued",
                config.shots.len()
            );
            *state = CaptureState::Settling {
                shot: 0,
                frames_left: config.settle_frames,
            };
        }

        CaptureState::Settling { shot, frames_left } => {
            let index = *shot;
            let Some(current) = config.shots.get(index) else {
                *state = CaptureState::Done;
                return;
            };
            apply_shot(current, &mut cameras, &mut world_time, &mut free_look);

            if *frames_left > 0 {
                *frames_left -= 1;
                return;
            }

            if config.continuous {
                // One shot per rendered frame: fire the probe screenshot
                // asynchronously and keep flying. Parking in AwaitingFile
                // here is exactly what used to let streaming catch up and
                // hide every fast-pan artifact.
                let probe_every = config.probe_every.max(1) as usize;
                let last = index + 1 == config.shots.len();
                if index % probe_every == 0 || last {
                    let path = config.out_dir.join(format!("{}.png", current.name));
                    let _ = std::fs::remove_file(&path);
                    commands
                        .spawn(Screenshot::primary_window())
                        .observe(save_to_disk(path.clone()));
                    info!(
                        "capture: probe '{}' at focus={:?} | {} terrain chunks loaded",
                        current.name,
                        current.focus,
                        loaded_chunks.map(|c| c.chunks.len()).unwrap_or(0),
                    );
                    pending_probes.push(path);
                }
                *state = if last {
                    CaptureState::AwaitingAll {
                        paths: std::mem::take(&mut *pending_probes),
                        frames_waited: 0,
                    }
                } else {
                    CaptureState::Settling {
                        shot: index + 1,
                        frames_left: 0,
                    }
                };
                return;
            }

            info!(
                "capture: '{}' focus={:?} zoom={} time={} | {} terrain chunks loaded",
                current.name,
                current.focus,
                current.zoom,
                current.time_of_day,
                loaded_chunks.map(|c| c.chunks.len()).unwrap_or(0),
            );
            let path = config.out_dir.join(format!("{}.png", current.name));
            let _ = std::fs::remove_file(&path);
            commands
                .spawn(Screenshot::primary_window())
                .observe(save_to_disk(path.clone()));
            info!("capture: shooting '{}' -> {}", current.name, path.display());
            *state = CaptureState::AwaitingFile {
                shot: index,
                path,
                frames_waited: 0,
            };
        }

        CaptureState::AwaitingFile {
            shot,
            path,
            frames_waited,
        } => {
            // Latch on the file existing rather than a frame count, so a slow write
            // cannot leave us with a truncated PNG.
            let written = std::fs::metadata(&*path)
                .map(|m| m.len() > 0)
                .unwrap_or(false);
            *frames_waited += 1;

            if !written {
                if *frames_waited > 600 {
                    error!("capture: '{}' never hit disk, giving up", path.display());
                } else {
                    return;
                }
            } else {
                info!("capture: wrote {}", path.display());
            }

            let next = *shot + 1;
            if next >= config.shots.len() {
                info!("capture: all {} shot(s) complete", config.shots.len());
                *state = CaptureState::Done;
            } else {
                *state = CaptureState::Settling {
                    shot: next,
                    frames_left: config.settle_frames,
                };
            }
        }

        CaptureState::AwaitingAll {
            paths,
            frames_waited,
        } => {
            *frames_waited += 1;
            paths.retain(|path| {
                let written = std::fs::metadata(path)
                    .map(|m| m.len() > 0)
                    .unwrap_or(false);
                if written {
                    info!("capture: wrote {}", path.display());
                }
                !written
            });
            if paths.is_empty() {
                info!("capture: all probe shot(s) complete");
                *state = CaptureState::Done;
            } else if *frames_waited > 1_200 {
                error!(
                    "capture: {} probe(s) never hit disk, giving up",
                    paths.len()
                );
                *state = CaptureState::Done;
            }
        }

        CaptureState::Done => {
            app_exit.write(AppExit::Success);
        }
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
/// That function maps the internal "day first, then night" layout onto a clock where
/// 0.0 = midnight, 0.25 = sunrise, **0.5 = noon**, 0.75 = sunset. Getting this backwards
/// silently photographs the world at dusk, which reads as "the renderer is broken".
fn normalized_to_seconds(normalized: f32, time: &WorldTime) -> f32 {
    // Delegate to the shared inverse so capture `--time` always agrees with
    // the game's display clock (now asymmetric summer hours, sunset 20:00) —
    // a hand-rolled copy here silently drifted once before.
    let mut scratch = time.clone();
    scratch.set_normalized_time(normalized);
    scratch.seconds_in_cycle
}

fn synthetic_settlement_history(name: &str) -> shared::economy::SettlementHistoryArchive {
    use shared::economy::{Good, MarketGoodHistoryDay, SettlementHistoryDay};

    let mut days = Vec::with_capacity(shared::economy::SETTLEMENT_HISTORY_DAYS);
    for day in 1..=shared::economy::SETTLEMENT_HISTORY_DAYS as u32 {
        let mut market = [MarketGoodHistoryDay::default(); Good::COUNT];
        let mut physical_stock = [0u32; Good::COUNT];
        for good in Good::ALL {
            let wave =
                ((day as f32 * 0.071 + good.index() as f32).sin() * 0.18 + 1.0).clamp(0.6, 1.4);
            let midpoint = (good.base_price() as f32 * wave) as u64;
            let producer_units = if (day + good.index() as u32).is_multiple_of(3) {
                0
            } else {
                2 + u64::from(day % 5)
            };
            let consumer_units = 1 + u64::from((day + good.index() as u32) % 4);
            let producer_price = midpoint.saturating_mul(92) / 100;
            let consumer_price = midpoint.saturating_mul(108).div_ceil(100);
            let stock = 5 + ((day * (good.index() as u32 + 2)) % 24);
            let target = match good {
                Good::Food | Good::Flour | Good::Bread | Good::Meat => 14,
                Good::Wheat => 10,
                Good::Wood => 20,
                Good::Stone | Good::Iron | Good::Wool => 5,
            };
            market[good.index()] = MarketGoodHistoryDay {
                opening_bid: producer_price.saturating_sub(3),
                opening_ask: consumer_price.saturating_sub(2),
                closing_bid: producer_price,
                closing_ask: consumer_price,
                high_bid: producer_price.saturating_add(8),
                low_bid: producer_price.saturating_sub(9),
                high_ask: consumer_price.saturating_add(10),
                low_ask: consumer_price.saturating_sub(7),
                producer_units,
                producer_coin: producer_price.saturating_mul(producer_units),
                consumer_units,
                consumer_coin: consumer_price.saturating_mul(consumer_units),
                unavailable_units: u64::from((day + good.index() as u32) % 3),
                unaffordable_units: u64::from((day + good.index() as u32 + 1) % 2),
                funded_unmet_units: u64::from((day + good.index() as u32) % 2),
                closing_stock: stock,
                target_stock: target,
                listed_units: stock.saturating_sub(2),
            };
            physical_stock[good.index()] = stock + 3;
        }
        let population = 3 + day / 38;
        let employed = population.saturating_sub(if day % 47 < 8 { 2 } else { 1 });
        let hungry = u32::from(day % 53 < 5);
        let prosperity =
            (55.0 + day as f32 * 0.085 + (day as f32 * 0.12).sin() * 8.0).clamp(0.0, 100.0);
        let resident_wallets = u64::from(population) * (900 + u64::from(day) * 4);
        let business_cash = 8_000 + u64::from(day) * 27;
        let household_cash = u64::from(population) * 160;
        let treasury = 2_000 + u64::from(day) * 12;
        let liquidation = Good::ALL
            .into_iter()
            .map(|good| u64::from(physical_stock[good.index()]) * market[good.index()].closing_bid)
            .sum();
        days.push(SettlementHistoryDay {
            day,
            market,
            civic_treasury: treasury,
            resident_wallet_money: resident_wallets,
            household_cash,
            business_cash,
            business_wage_arrears: if day % 29 == 0 { 150 } else { 0 },
            business_tax_arrears: if day % 37 == 0 { 80 } else { 0 },
            civic_wage_arrears: if day % 11 == 0 { 125 } else { 0 },
            civic: shared::economy::CivicHistoryDay::default(),
            physical_stock,
            stock_liquidation_value: liquidation,
            total_local_coin: treasury + resident_wallets + household_cash + business_cash,
            population,
            employed,
            hungry,
            job_seekers: population.saturating_sub(employed),
            homeless: u32::from(day % 71 < 4),
            unpaid_workers: u32::from(day % 29 == 0),
            unrest: (18.0 + (day as f32 * 0.07).sin() * 10.0).clamp(0.0, 100.0),
            unrest_target: (20.0 + (day as f32 * 0.05).sin() * 12.0).clamp(0.0, 100.0),
            food_reserves: physical_stock[Good::Food.index()]
                + physical_stock[Good::Flour.index()]
                + physical_stock[Good::Bread.index()],
            purchasable_food: market[Good::Food.index()].listed_units
                + market[Good::Flour.index()].listed_units
                + market[Good::Bread.index()].listed_units,
            unlisted_business_food: day % 9,
            food_produced: 3 + day % 7,
            food_consumed: population,
            buildings: (4 + day / 55) as u16,
            productive_buildings: (2 + day / 100) as u16,
            work_positions: (4 + day / 45) as u16,
            filled_jobs: employed.min(u16::MAX as u32) as u16,
            prosperity,
            reserve_prosperity: (prosperity * 0.38).min(40.0),
            production_prosperity: (prosperity * 0.29).min(30.0),
            housing_prosperity: (prosperity * 0.2).min(20.0),
            employment_prosperity: (prosperity * 0.1).min(10.0),
            hunger_penalty: -(hungry as f32 * 4.0),
        });
    }
    shared::economy::SettlementHistoryArchive {
        settlement: name.to_string(),
        days,
        businesses: vec![
            synthetic_business_history(
                shared::components::BuildingId(100),
                shared::components::SettlementBuildingKind::Farmstead,
                shared::economy::Good::Wheat,
                shared::names::person_name(7_000),
            ),
            synthetic_business_history(
                shared::components::BuildingId(101),
                shared::components::SettlementBuildingKind::LumberjackHut,
                shared::economy::Good::Wood,
                shared::names::person_name(7_001),
            ),
        ],
    }
}

fn synthetic_business_history(
    id: shared::components::BuildingId,
    kind: shared::components::SettlementBuildingKind,
    output: shared::economy::Good,
    owner: String,
) -> shared::economy::BusinessHistoryArchive {
    use shared::economy::{BusinessHistoryDay, BusinessState, BusinessStrategy, Good};

    let days = (1..=shared::economy::SETTLEMENT_HISTORY_DAYS as u32)
        .map(|day| {
            let produced = 2 + (day % 5);
            let sold = produced.saturating_sub(u32::from(day % 7 == 0));
            let asking = output.base_price().saturating_mul(90 + u64::from(day % 24)) / 100;
            let revenue = asking.saturating_mul(u64::from(sold));
            let wages = 100 + u64::from(day % 3) * 25;
            let fees = revenue * 5 / 100;
            let levy = revenue.saturating_sub(wages + fees) / 10;
            let costs = wages.saturating_add(fees).saturating_add(levy);
            let mut stock = [0; Good::COUNT];
            stock[output.index()] = 3 + day % 14;
            BusinessHistoryDay {
                day,
                observed: true,
                cash: 1_500 + u64::from(day) * 9,
                protected_working_capital: 600,
                withdrawable_profit: 300 + u64::from(day),
                wage_arrears: if day % 61 == 0 { 100 } else { 0 },
                tax_arrears: if day % 79 == 0 { 75 } else { 0 },
                gross_revenue: revenue,
                internal_revenue: if matches!(
                    kind,
                    shared::components::SettlementBuildingKind::Windmill
                        | shared::components::SettlementBuildingKind::Bakery
                ) {
                    revenue / 6
                } else {
                    0
                },
                wage_expense: wages,
                input_expense: 0,
                internal_input_expense: 0,
                market_fees: fees,
                delivery_fees: 0,
                profit_taxes: levy,
                owner_withdrawals: if day % 4 == 0 { 125 } else { 0 },
                capital_expenditures: if day == 1 { 450 } else { 0 },
                book_value: 450,
                profit: revenue as i64 - costs as i64,
                produced_units: produced,
                sold_units: sold,
                purchased_input_units: 0,
                workplace_stock: stock,
                listed_output_units: stock[output.index()].saturating_sub(2),
                asking_unit_price: asking,
                daily_wage: wages,
                strategy: BusinessStrategy::Balanced,
                autopilot: true,
                state: if day % 61 == 0 {
                    BusinessState::CashTight
                } else {
                    BusinessState::Operating
                },
            }
        })
        .collect();

    shared::economy::BusinessHistoryArchive {
        id,
        settlement: shared::components::SettlementId::UNASSIGNED,
        company_id: Some(shared::components::CompanyId(100 + id.0)),
        kind,
        owner_id: None,
        owner_name: Some(owner),
        output_good: Some(output),
        days,
    }
}

fn synthetic_company_history(
    company: shared::components::CompanyId,
) -> shared::economy::CompanyHistoryArchive {
    let mut farm = synthetic_business_history(
        shared::components::BuildingId(601),
        shared::components::SettlementBuildingKind::Farmstead,
        shared::economy::Good::Wheat,
        "Aldric".to_string(),
    );
    farm.company_id = Some(company);
    farm.settlement = shared::components::SettlementId(41);
    let mut mill = synthetic_business_history(
        shared::components::BuildingId(602),
        shared::components::SettlementBuildingKind::Windmill,
        shared::economy::Good::Flour,
        "Aldric".to_string(),
    );
    mill.company_id = Some(company);
    mill.settlement = shared::components::SettlementId(41);
    let mut bakery = synthetic_business_history(
        shared::components::BuildingId(603),
        shared::components::SettlementBuildingKind::Bakery,
        shared::economy::Good::Bread,
        "Aldric".to_string(),
    );
    bakery.company_id = Some(company);
    bakery.settlement = shared::components::SettlementId(52);
    shared::economy::CompanyHistoryArchive {
        company,
        businesses: vec![farm, mill, bakery],
    }
}

fn synthetic_world_history() -> shared::economy::WorldHistoryArchive {
    use shared::economy::{Good, WorldHistoryDay};

    let days = (1..=shared::economy::SETTLEMENT_HISTORY_DAYS as u32)
        .map(|day| {
            let settlements = 2 + day / 90;
            let population = 18 + day / 8 + (day / 70) * 5;
            let employed = population.saturating_sub(3 + day % 4);
            let hungry = if day % 61 < 8 { 2 + day % 3 } else { day % 2 };
            let mut physical_stock = [0u32; Good::COUNT];
            for good in Good::ALL {
                physical_stock[good.index()] = 15 + day / 5 + good.index() as u32 * 9 + day % 13;
            }
            let business_cash = 28_000 + u64::from(day) * 37;
            let household_cash = u64::from(population) * 175;
            let wallets = u64::from(population) * (850 + u64::from(day) * 3);
            let treasury = u64::from(settlements) * 2_500 + u64::from(day) * 18;
            WorldHistoryDay {
                day,
                settlements,
                population,
                employed,
                hungry,
                civic_treasury: treasury,
                resident_wallet_money: wallets,
                household_cash,
                business_cash,
                business_wage_arrears: if day % 29 == 0 { 300 } else { 0 },
                business_tax_arrears: if day % 41 == 0 { 175 } else { 0 },
                civic_wage_arrears: if day % 17 == 0 { 220 } else { 0 },
                total_local_coin: treasury + business_cash + household_cash + wallets,
                stock_liquidation_value: 18_000 + u64::from(day) * 91,
                physical_stock,
                food_reserves: physical_stock[Good::Food.index()]
                    + physical_stock[Good::Flour.index()]
                    + physical_stock[Good::Bread.index()],
                purchasable_food: physical_stock[Good::Food.index()]
                    + physical_stock[Good::Bread.index()],
                unlisted_business_food: day % 13,
                food_produced: population + 5 + day % 12,
                food_consumed: population.saturating_sub(hungry),
                buildings: 8 + day / 17,
                productive_buildings: 4 + day / 43,
                prosperity: (48.0 + day as f32 * 0.1 + (day as f32 * 0.085).sin() * 6.0)
                    .clamp(0.0, 100.0),
            }
        })
        .collect();
    shared::economy::WorldHistoryArchive { days }
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

    /// Display noon is NOT mid-day-portion since the summer clock (sunrise
    /// 06:00, sunset 22:00): it lands at the clock's fraction of daylight.
    #[test]
    fn noon_lands_at_the_summer_clock_fraction_of_daylight() {
        let t = probe();
        let frac = (0.5 - WorldTime::SUNRISE_NORMALIZED)
            / (WorldTime::SUNSET_NORMALIZED - WorldTime::SUNRISE_NORMALIZED);
        assert!((normalized_to_seconds(0.5, &t) - t.day_duration * frac).abs() < 1e-3);
    }
}
