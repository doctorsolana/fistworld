//! Connected client/server capture hooks for the opening voyage and Village Lab.

use super::inspection::CaptureInspection;
use super::{advance_readiness, ReadinessOutcome, ReadinessProgress};
use crate::camera_rts::CommanderCamera;
use crate::capture_artifact::{
    git_commit, request_capture, CaptureCameraMetadata, CaptureCompletions, CaptureMetadata,
    CaptureReadiness, CaptureTarget, CaptureWorldSnapshot, CaptureWriteRequest,
    CAPTURE_METADATA_VERSION,
};
use bevy::prelude::*;
use bevy::render::view::screenshot::Screenshot;
use shared::components::WorldTime;
use std::path::PathBuf;

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
        zoom: f32,
        frames_left: u32,
    },
    AwaitingCapture {
        ticket: u64,
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
    awaiting: Option<(u64, u32)>,
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
    inspection: CaptureInspection,
    world_time: Query<&WorldTime>,
    cameras: Query<&CommanderCamera>,
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
    mut completions: ResMut<CaptureCompletions>,
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

    if let Some((ticket, frames_waited)) = state.awaiting.as_mut() {
        *frames_waited += 1;
        let Some(completion) = completions.take(*ticket) else {
            if *frames_waited <= 600 {
                return;
            }
            error!("live voyage capture: screenshot observer timed out");
            state.enabled = false;
            app_exit.write(AppExit::error());
            return;
        };
        if let Some(error) = completion.error {
            error!(
                "live voyage capture: {} failed: {error}",
                completion.path.display()
            );
            state.enabled = false;
            app_exit.write(AppExit::error());
            return;
        }
        info!(
            "live voyage capture: wrote {} and metadata",
            completion.path.display()
        );
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
    let Some(scene_target) = inspection.scene_target.as_ref() else {
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
    let mut request = live_capture_request(path, "live-voyage", filename, CaptureTarget::Scene);
    if let Err(error) = inspection.complete_live_request(
        &mut request,
        cameras.iter().next(),
        world_time.iter().next(),
        0,
    ) {
        error!("live voyage capture: {error}");
        state.enabled = false;
        app_exit.write(AppExit::error());
        return;
    }
    let ticket = request_capture(
        &mut commands,
        Screenshot::image(scene_target.image.clone()),
        request,
        &mut completions,
    );
    state.awaiting = Some((ticket, 0));
}

/// Capture the connected, fully simulated Village Lab on a requested HUD day.
///
/// Environment variables:
/// - `FISTWORLD_LAB_CAPTURE_DAY` enables the hook.
/// - `FISTWORLD_LAB_CAPTURE_PATH` selects the PNG path.
/// - `FISTWORLD_LAB_CAPTURE_ZOOM` controls the survey framing (default 430m).
/// - `FISTWORLD_LAB_CAPTURE_TARGET=scene` reads the offscreen 3D image; the default is window.
/// - `FISTWORLD_LAB_CAPTURE_SETTLE_FRAMES` controls render warmup (default 180).
/// - `FISTWORLD_LAB_CAPTURE_EXIT=1` closes the client after the file is written.
pub(crate) fn drive_live_lab_capture(
    mut commands: Commands,
    world_time: Query<&WorldTime>,
    resting_people: Query<
        (
            &shared::components::PersonId,
            &shared::components::CharacterActivity,
            &GlobalTransform,
        ),
        With<crate::hero::HeroDressed>,
    >,
    mut cameras: Query<&mut CommanderCamera>,
    mut state: Local<LiveLabCaptureState>,
    inspection: CaptureInspection,
    mut readiness: Local<ReadinessProgress>,
    mut completions: ResMut<CaptureCompletions>,
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

    let follow_rest = std::env::var("FISTWORLD_LAB_CAPTURE_ACTIVITY").as_deref() == Ok("rest");
    let resting = follow_rest
        .then(|| {
            resting_people
                .iter()
                .filter(|(_, activity, _)| {
                    **activity == shared::components::CharacterActivity::LyingDown
                })
                .min_by_key(|(person, _, _)| person.0)
                .map(|(person, _, at)| (person.0, at.translation()))
        })
        .flatten();
    match &mut *state {
        LiveLabCaptureState::Uninitialized | LiveLabCaptureState::Disabled => {}
        LiveLabCaptureState::Waiting { day, path, zoom } => {
            let Some(clock) = world_time.iter().next() else {
                return;
            };
            if clock.day < *day {
                return;
            }
            let Ok(mut camera) = cameras.single_mut() else {
                return;
            };
            if follow_rest {
                let Some((person, point)) = resting else {
                    return;
                };
                camera.focus = point;
                camera.focus_target = point;
                if matches!(*readiness, ReadinessProgress { frames: 0, .. }) {
                    info!("live character rest capture: PersonId({person}) at {point:?}");
                }
            }
            camera.zoom = *zoom;
            camera.zoom_target = *zoom;
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
                zoom: *zoom,
                frames_left,
            };
            *readiness = ReadinessProgress::default();
        }
        LiveLabCaptureState::Settling {
            path,
            zoom,
            frames_left,
        } => {
            let Ok(mut camera) = cameras.single_mut() else {
                return;
            };
            // A replicated/restored commander view may arrive after the clock.
            // Keep the requested framing throughout warmup and terrain streaming.
            if follow_rest {
                let Some((person, point)) = resting else {
                    return;
                };
                camera.focus = point;
                camera.focus_target = point;
                if matches!(*readiness, ReadinessProgress { frames: 0, .. }) {
                    info!("live character rest capture: PersonId({person}) at {point:?}");
                }
            }
            camera.zoom = *zoom;
            camera.zoom_target = *zoom;
            if *frames_left > 0 {
                *frames_left -= 1;
                return;
            }
            let chunks = inspection
                .loaded_chunks
                .as_ref()
                .map_or(0, |loaded| loaded.chunks.len());
            match advance_readiness(
                &mut readiness,
                &CaptureReadiness {
                    minimum_frames: 0,
                    maximum_frames: 1200,
                    minimum_loaded_chunks: 25,
                    stable_loaded_chunk_frames: 20,
                },
                chunks,
            ) {
                ReadinessOutcome::Waiting => return,
                ReadinessOutcome::TimedOut(reason) => {
                    error!("live Village Lab capture: readiness timed out: {reason}");
                    *state = LiveLabCaptureState::Done;
                    app_exit.write(AppExit::error());
                    return;
                }
                ReadinessOutcome::Ready => {}
            }
            let (screenshot, target) =
                if std::env::var("FISTWORLD_LAB_CAPTURE_TARGET").as_deref() == Ok("scene") {
                    let Some(scene_target) = inspection.scene_target.as_ref() else {
                        return;
                    };
                    (
                        Screenshot::image(scene_target.image.clone()),
                        CaptureTarget::Scene,
                    )
                } else {
                    (Screenshot::primary_window(), CaptureTarget::Window)
                };
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
            let mut request = live_capture_request(
                path.clone(),
                "live-village-lab",
                path.file_stem()
                    .and_then(|stem| stem.to_str())
                    .unwrap_or("village-lab"),
                target,
            );
            if let Err(error) = inspection.complete_live_request(
                &mut request,
                Some(&camera),
                world_time.iter().next(),
                readiness.frames,
            ) {
                error!("live Village Lab capture: {error}");
                *state = LiveLabCaptureState::Done;
                app_exit.write(AppExit::error());
                return;
            }
            let ticket = request_capture(&mut commands, screenshot, request, &mut completions);
            info!("live Village Lab capture: shooting {}", path.display());
            *state = LiveLabCaptureState::AwaitingCapture {
                ticket,
                frames_waited: 0,
            };
        }
        LiveLabCaptureState::AwaitingCapture {
            ticket,
            frames_waited,
        } => {
            *frames_waited += 1;
            let completion = completions.take(*ticket);
            if completion.is_none() && *frames_waited <= 600 {
                return;
            }
            let success = completion
                .as_ref()
                .is_some_and(|completion| completion.error.is_none());
            match completion {
                Some(completion) if success => info!(
                    "live Village Lab capture: wrote {} and metadata",
                    completion.path.display()
                ),
                Some(completion) => error!(
                    "live Village Lab capture: {} failed: {}",
                    completion.path.display(),
                    completion.error.as_deref().unwrap_or("unknown error")
                ),
                None => error!("live Village Lab capture: screenshot observer timed out"),
            }
            if std::env::var("FISTWORLD_LAB_CAPTURE_EXIT").is_ok_and(|raw| {
                matches!(
                    raw.trim().to_ascii_lowercase().as_str(),
                    "1" | "true" | "yes" | "on"
                )
            }) {
                app_exit.write(if success {
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

pub(super) fn live_capture_request(
    path: PathBuf,
    scenario: &str,
    shot: &str,
    target: CaptureTarget,
) -> CaptureWriteRequest {
    CaptureWriteRequest {
        path: path.clone(),
        metadata: CaptureMetadata {
            schema_version: CAPTURE_METADATA_VERSION,
            scenario: scenario.to_owned(),
            shot: shot.to_owned(),
            map: std::env::var("CITYSIM_MAP_ID").unwrap_or_else(|_| "default".to_owned()),
            git_commit: git_commit(),
            target,
            fixed_delta_seconds: 0.0,
            output: path.display().to_string(),
            width: 0,
            height: 0,
            readiness_frames: 0,
            camera: CaptureCameraMetadata {
                focus: [0.0; 3],
                yaw: 0.0,
                zoom: 0.0,
                time_of_day: 0.0,
                pitch: None,
                eye: 0.0,
                position: None,
                rotation: None,
            },
            world: CaptureWorldSnapshot::default(),
            assertions: Vec::new(),
            comparison: None,
            comparison_error: None,
        },
        comparison: None,
    }
}
