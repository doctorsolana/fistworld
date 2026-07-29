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
    /// Downward tilt in radians. Lower = more horizon, higher = more top-down.
    pub tilt: f32,
    /// Time of day in `0.0..=1.0` (0.5 = noon).
    pub time_of_day: f32,
}

impl Default for Shot {
    fn default() -> Self {
        Self {
            name: "shot".to_string(),
            focus: Vec3::ZERO,
            yaw: -0.45,
            zoom: 220.0,
            tilt: 0.75,
            time_of_day: 0.5,
        }
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

    app.insert_resource(CaptureState::Warmup {
        frames_left: config.warmup_frames,
    });
    app.insert_resource(config);

    app.add_systems(Startup, enter_world_offline);
    app.add_systems(Update, drive_capture);

    app.run();
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
    match std::env::var("FISTFORCE_CAPTURE_CLOUDS").as_deref() {
        Ok("cloudy") => {
            commands.insert_resource(crate::render::systems::CloudCoverOverride {
                mode: crate::render::systems::CloudCoverMode::Cloudy,
            });
        }
        Ok("clear") => {
            commands.insert_resource(crate::render::systems::CloudCoverOverride {
                mode: crate::render::systems::CloudCoverMode::Clear,
            });
        }
        _ => {}
    }

    info!("capture: entering world offline (no server)");
}

#[allow(clippy::too_many_arguments)]
fn drive_capture(
    mut commands: Commands,
    config: Res<CaptureConfig>,
    mut state: ResMut<CaptureState>,
    mut cameras: Query<&mut CommanderCamera>,
    mut world_time: Query<&mut WorldTime>,
    loaded_chunks: Option<Res<LoadedChunks>>,
    mut app_exit: MessageWriter<AppExit>,
) {
    match &mut *state {
        CaptureState::Warmup { frames_left } => {
            // Park the camera on the first shot during warmup so streaming loads the
            // right chunks rather than whatever is around the origin.
            if let Some(shot) = config.shots.first() {
                apply_shot(shot, &mut cameras, &mut world_time);
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
            apply_shot(current, &mut cameras, &mut world_time);

            if *frames_left > 0 {
                *frames_left -= 1;
                return;
            }

            info!(
                "capture: '{}' focus={:?} zoom={} tilt={} time={} | {} terrain chunks loaded",
                current.name,
                current.focus,
                current.zoom,
                current.tilt,
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

        CaptureState::Done => {
            app_exit.write(AppExit::Success);
        }
    }
}

fn apply_shot(
    shot: &Shot,
    cameras: &mut Query<&mut CommanderCamera>,
    world_time: &mut Query<&mut WorldTime>,
) {
    for mut camera in cameras.iter_mut() {
        camera.focus = shot.focus;
        camera.yaw = shot.yaw;
        camera.zoom = shot.zoom;
        camera.tilt = shot.tilt;
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

    #[test]
    fn noon_lands_in_the_middle_of_the_day_portion() {
        let t = probe();
        assert!((normalized_to_seconds(0.5, &t) - t.day_duration / 2.0).abs() < 1e-3);
    }
}
