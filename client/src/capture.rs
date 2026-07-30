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
    app.add_systems(Update, (spawn_capture_heroes, drive_capture));

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
    if let Ok(mode) = std::env::var("FISTFORCE_CAPTURE_HUD") {
        if !mode.is_empty() {
            if mode == "god" {
                commands.insert_resource(crate::ui::hud::GodCapability(true));
                commands.insert_resource(crate::ui::hud::HudMode::God);
            }
        }
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
        if mode == "live" {
            commands.insert_resource(EncyclopediaOpen(true));
            commands.insert_resource(crate::ui::hud::GodCapability(true));
            return;
        }
        let sample = |name: &str, level, prestige, online, known, is_self, affiliation| {
            PersonRecord {
                name: name.to_string(),
                kind: PersonKind::Hero,
                affiliation,
                level,
                prestige,
                online,
                known,
                is_self,
            }
        };
        commands.insert_resource(KnownPeople {
            records: vec![
                sample("Aldric", 7, 2, true, true, true, Affiliation::Neutral),
                sample("Bryn", 4, 0, true, true, false, Affiliation::Neutral),
                sample("Cassia", 11, 5, false, true, false, Affiliation::Clan("HOLLOWMERE".into())),
                sample("Dunstan", 2, 0, false, true, false, Affiliation::Neutral),
                sample("Eirwen", 9, 3, true, true, false, Affiliation::Clan("HOLLOWMERE".into())),
                sample("Faelan", 1, 0, false, false, false, Affiliation::Neutral),
                sample("Gwyneth", 14, 8, false, false, false, Affiliation::Clan("BRACKWATER".into())),
                sample("Hollis", 5, 1, false, true, false, Affiliation::Neutral),
                sample("Ivo", 3, 0, false, false, false, Affiliation::Neutral),
                sample("Jorunn", 8, 4, true, true, false, Affiliation::Clan("BRACKWATER".into())),
                sample("Kelda", 6, 2, false, true, false, Affiliation::Neutral),
                sample("Lorcan", 12, 6, false, false, false, Affiliation::Neutral),
            ],
            requested: true,
        });
        commands.insert_resource(SelectedPerson(Some("Cassia".to_string())));
        commands.insert_resource(EncyclopediaOpen(true));
        // Mode picks which surface to photograph: a tab name, or "god" to
        // grant capability so the unknown-people view can be verified.
        commands.insert_resource(match mode.as_str() {
            "retinue" => crate::ui::encyclopedia::EncyclopediaTab::Retinue,
            "ledger" => crate::ui::encyclopedia::EncyclopediaTab::Ledger,
            _ => crate::ui::encyclopedia::EncyclopediaTab::People,
        });
        if mode == "god" {
            commands.insert_resource(crate::ui::hud::GodCapability(true));
        }
    }

    info!("capture: entering world offline (no server)");
}

/// FISTFORCE_CAPTURE_HERO spawns stand-in heroes (offline fakes of the
/// replicated entity) in a line at the first shot's focus, terrain-snapped, so
/// captures can verify the character model, wardrobe and pose without a
/// server.
///
/// Spec: `slot0,slot1,...,skin` per hero, semicolon-separated, all indices
/// into the manifest's slot items / skin tones (order as in voxel_boy.ron:
/// bottom, top, hair). Missing or unparsable fields use the manifest default.
/// `FISTFORCE_CAPTURE_HERO=default` spawns one hero in the declared default.
fn spawn_capture_heroes(
    mut commands: Commands,
    config: Res<CaptureConfig>,
    terrain: Option<Res<shared::terrain::WorldTerrain>>,
    mut spawned: Local<bool>,
) {
    if *spawned {
        return;
    }
    let hero_spec = std::env::var("FISTFORCE_CAPTURE_HERO").ok();
    let villager_count = std::env::var("FISTFORCE_CAPTURE_VILLAGERS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok());
    if hero_spec.is_none() && villager_count.is_none() {
        *spawned = true;
        return;
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
        let x = base.x + i as f32 * 1.4;
        let z = base.z;
        let pos = Vec3::new(x, terrain.get_height(x, z), z);
        commands.spawn((
            shared::components::Hero {
                owner: lightyear::prelude::PeerId::Netcode(1000 + i as u64),
            },
            shared::components::CharacterName(format!("Capture Hero {}", i + 1)),
            shared::components::CharacterKind::Hero,
            outfit,
            shared::components::PlayerPosition(pos),
            shared::components::PlayerRotation(std::f32::consts::PI),
        ));
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
            commands.spawn((
                shared::components::CharacterName(shared::names::person_name(seed)),
                shared::components::CharacterKind::Villager,
                shared::components::HeroOutfit::varied(seed),
                shared::components::PlayerPosition(pos),
                shared::components::PlayerRotation(std::f32::consts::PI),
            ));
        }
    }

    // FISTFORCE_CAPTURE_SELECT=1 selects the FIRST fake hero, so the ground ring
    // and the selected-unit plate can be verified without a server. Deferred by
    // a command so it runs after the spawns above are applied.
    if std::env::var("FISTFORCE_CAPTURE_SELECT").is_ok_and(|v| v == "1") {
        commands.queue(|world: &mut World| {
            let mut heroes = world
                .query_filtered::<(Entity, &shared::components::Hero), ()>();
            if let Some((first, hero)) = heroes.iter(world).next() {
                let owner = shared::player::peer_id_to_u64(hero.owner);
                world.resource_mut::<crate::selection::Selection>().entity = Some(first);
                // Claim ownership of it too, so the shot shows the state a real
                // player sees (ember mark, own name) rather than "NOT YOURS".
                world.insert_resource(crate::camera_rts::LocalPeerId(owner));
            }
        });
    }
    *spawned = true;
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
