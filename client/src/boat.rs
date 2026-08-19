//! Client presentation for generic vessels and the opening dinghy voyage.
//!
//! Navigation and speed remain server-authoritative. This module smooths the
//! replicated hull, drives the authored sail rig from the same shared wind,
//! and owns the one-time face-to-RTS opening camera transition.

use bevy::audio::Volume;
use bevy::prelude::*;
use shared::components::{
    AboardBoat, CharacterMotion, CloudSeed, CommandedBy, Hero, PlayerBoat, PlayerPosition,
    PlayerRotation, TimeWarp, WorldTime, WreckedVessel,
};
use shared::terrain::WorldTerrain;
use shared::water::{water_swell_height, OCEAN_LOOP_SECONDS, WATER_SURFACE_OFFSET};

use crate::camera_rts::{CommanderCamera, LocalPeerId};
use crate::states::GameState;

const DINGHY_SCENE: &str = "game_assets/vehicles/boats/Dinghy.glb#Scene0";
const GAME_INTRO_AUDIO: &str = "audio/music/game_intro.ogg";
const CAMERA_HOLD_SECONDS: f32 = 3.5;
const CAMERA_TRAVEL_SECONDS: f32 = 4.4;
const PORTRAIT_PULLBACK_METERS: f32 = 1.35;
const PORTRAIT_RISE_METERS: f32 = 0.18;
const FINAL_RTS_ZOOM: f32 = 36.0;
/// Authored seated-helm anchor in Bevy/glTF coordinates.
const HELM_LOCAL: Vec3 = Vec3::new(0.0, 0.35, 1.24);
/// Water samples sit inside the hull ends so a short shore wave cannot make
/// the visual pose hinge on a point outside the boat.
const BUOYANCY_HALF_LENGTH: f32 = 1.65;
const BUOYANCY_HALF_BEAM: f32 = 0.65;

fn smoothstep01(raw: f32) -> f32 {
    let raw = raw.clamp(0.0, 1.0);
    raw * raw * (3.0 - 2.0 * raw)
}

pub struct BoatPlugin;

impl Plugin for BoatPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<OpeningCinematic>();
        app.add_systems(
            Update,
            (
                attach_boat_visuals,
                sync_boat_transforms,
                sync_aboard_hero_visuals.after(crate::hero::sync_hero_transforms),
                tag_boat_sail_nodes,
                drive_boat_sails,
                drive_opening_cinematic.after(crate::camera_rts::update_commander_camera),
                focus_disembarked_hero,
                sync_voyage_hint,
            )
                .chain()
                .run_if(in_state(GameState::Playing)),
        );
    }
}

#[derive(Component)]
struct BoatVisual {
    snapshot: Vec3,
}

/// The renderer advances its water clock from local real time. Latching one
/// offset from the replicated authoritative clock keeps hull sampling on the
/// same phase without reacting to ordinary snapshot latency every frame.
#[derive(Default)]
struct BoatWaveClockSync {
    world_time_entity: Option<Entity>,
    offset: f32,
}

#[derive(Component)]
struct BoatSailRig {
    boat: Entity,
    rest_rotation: Quat,
    initialized: bool,
}

#[derive(Component)]
struct BoatSailMorph {
    boat: Entity,
}

/// The authored dinghy hierarchy has instantiated and is safe to frame.
/// Without this gate a fast network snapshot can start the opening camera on
/// an invisible hull for one or two render frames.
#[derive(Component)]
pub(crate) struct BoatSceneReady;

#[derive(Component)]
struct VoyageHint;

#[derive(Debug, Clone)]
enum CinematicState {
    Idle,
    Armed,
    Running {
        elapsed: f32,
        boat: Entity,
        start: Transform,
        start_target: Vec3,
        end_target: Vec3,
        end: Transform,
    },
}

/// Client-only one-shot opening. It is armed only when the server says this
/// account needs a hero, so reconnecting never replays the cinematic.
#[derive(Resource, Debug, Clone)]
pub struct OpeningCinematic {
    state: CinematicState,
    intro_played: bool,
}

impl Default for OpeningCinematic {
    fn default() -> Self {
        Self {
            state: CinematicState::Idle,
            intro_played: false,
        }
    }
}

impl OpeningCinematic {
    pub fn arm(&mut self) {
        self.state = CinematicState::Armed;
    }

    pub fn is_active(&self) -> bool {
        !matches!(self.state, CinematicState::Idle)
    }

    pub fn cancel(&mut self) {
        self.state = CinematicState::Idle;
    }

    pub(crate) fn running_elapsed_secs(&self) -> Option<f32> {
        match self.state {
            CinematicState::Running { elapsed, .. } => Some(elapsed),
            CinematicState::Idle | CinematicState::Armed => None,
        }
    }
}

fn attach_boat_visuals(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    boats: Query<
        (Entity, &PlayerPosition, &PlayerRotation),
        (With<PlayerBoat>, Without<BoatVisual>),
    >,
) {
    for (entity, position, rotation) in boats.iter() {
        commands.entity(entity).insert((
            Name::new("Player Dinghy"),
            BoatVisual {
                snapshot: position.0,
            },
            // Do not instantiate at yaw zero while PlayerRotation is still in
            // flight from the server. Waiting for the authoritative heading
            // prevents the whole hull visibly turning around during the
            // opening shot as its first rotation snapshot arrives.
            Transform::from_translation(position.0)
                .with_rotation(Quat::from_rotation_y(rotation.0)),
            GlobalTransform::default(),
            Visibility::default(),
            InheritedVisibility::default(),
            WorldAssetRoot(asset_server.load(DINGHY_SCENE)),
        ));
    }
}

fn sync_boat_transforms(
    time: Res<Time>,
    terrain: Res<WorldTerrain>,
    world_time: Query<(Entity, &WorldTime)>,
    mut wave_clock: Local<BoatWaveClockSync>,
    mut boats: Query<(
        Ref<PlayerPosition>,
        &PlayerRotation,
        Has<WreckedVessel>,
        &mut Transform,
        &mut BoatVisual,
    )>,
) {
    let blend = 1.0 - (-time.delta_secs() / 0.075).exp();
    let ocean_seconds = synchronized_ocean_seconds(&time, &world_time, &mut wave_clock);
    for (position, rotation, wrecked, mut transform, mut visual) in boats.iter_mut() {
        if position.is_changed() {
            visual.snapshot = position.0;
        }
        let (surface_y, buoyancy_rotation) =
            boat_surface_pose(&terrain, visual.snapshot, rotation.0, ocean_seconds);
        let target_position = Vec3::new(
            visual.snapshot.x,
            surface_y + if wrecked { -0.16 } else { 0.0 },
            visual.snapshot.z,
        );
        if transform.translation.distance_squared(target_position) > 30.0 * 30.0 {
            transform.translation = target_position;
        } else {
            transform.translation = transform.translation.lerp(target_position, blend);
        }
        let wreck_tilt = if wrecked {
            Quat::from_rotation_x(0.10) * Quat::from_rotation_z(-0.07)
        } else {
            Quat::IDENTITY
        };
        transform.rotation = transform
            .rotation
            .slerp(buoyancy_rotation * wreck_tilt, blend);
    }
}

fn synchronized_ocean_seconds(
    time: &Time,
    world_time: &Query<(Entity, &WorldTime)>,
    sync: &mut BoatWaveClockSync,
) -> f32 {
    let local = time.elapsed_secs_wrapped().rem_euclid(OCEAN_LOOP_SECONDS);
    if let Ok((entity, world_time)) = world_time.single() {
        if sync.world_time_entity != Some(entity) {
            let half_loop = OCEAN_LOOP_SECONDS * 0.5;
            sync.offset = (world_time.ocean_seconds - local + half_loop)
                .rem_euclid(OCEAN_LOOP_SECONDS)
                - half_loop;
            sync.world_time_entity = Some(entity);
        }
    }
    (local + sync.offset).rem_euclid(OCEAN_LOOP_SECONDS)
}

fn sampled_surface_height(terrain: &WorldTerrain, point: Vec2, ocean_seconds: f32) -> f32 {
    let base = terrain
        .water_surface_height(point.x, point.y)
        .or_else(|| terrain.water_level())
        .unwrap_or(0.0);
    let depth = base - terrain.get_height(point.x, point.y);
    base + WATER_SURFACE_OFFSET + water_swell_height(point.x, point.y, depth, ocean_seconds)
}

fn boat_surface_pose(
    terrain: &WorldTerrain,
    centre: Vec3,
    yaw: f32,
    ocean_seconds: f32,
) -> (f32, Quat) {
    let yaw_rotation = Quat::from_rotation_y(yaw);
    let forward = (yaw_rotation * Vec3::NEG_Z).xz().normalize_or_zero();
    let right = (yaw_rotation * Vec3::X).xz().normalize_or_zero();
    let centre_xz = centre.xz();
    let bow = sampled_surface_height(
        terrain,
        centre_xz + forward * BUOYANCY_HALF_LENGTH,
        ocean_seconds,
    );
    let stern = sampled_surface_height(
        terrain,
        centre_xz - forward * BUOYANCY_HALF_LENGTH,
        ocean_seconds,
    );
    let starboard = sampled_surface_height(
        terrain,
        centre_xz + right * BUOYANCY_HALF_BEAM,
        ocean_seconds,
    );
    let port = sampled_surface_height(
        terrain,
        centre_xz - right * BUOYANCY_HALF_BEAM,
        ocean_seconds,
    );
    let centre_height = sampled_surface_height(terrain, centre_xz, ocean_seconds);

    (
        centre_height,
        orientation_from_surface_samples(yaw, bow, stern, starboard, port),
    )
}

/// Construct an authored-local (+X right, +Y up, -Z bow) orientation from
/// four water heights. Kept pure so pitch/roll sign regressions are testable.
fn orientation_from_surface_samples(
    yaw: f32,
    bow: f32,
    stern: f32,
    starboard: f32,
    port: f32,
) -> Quat {
    let yaw_rotation = Quat::from_rotation_y(yaw);
    let flat_forward = yaw_rotation * Vec3::NEG_Z;
    let flat_right = yaw_rotation * Vec3::X;
    let forward_hint =
        (flat_forward * (BUOYANCY_HALF_LENGTH * 2.0) + Vec3::Y * (bow - stern)).normalize_or_zero();
    let right_hint = (flat_right * (BUOYANCY_HALF_BEAM * 2.0) + Vec3::Y * (starboard - port))
        .normalize_or_zero();
    let up = right_hint.cross(forward_hint).normalize_or_zero();
    let right = forward_hint.cross(up).normalize_or_zero();
    let forward = up.cross(right).normalize_or_zero();

    Quat::from_mat3(&Mat3::from_cols(right, up, -forward)).normalize()
}

/// The replicated Hero stays yaw-only and server-authoritative. Its rendered
/// root is pinned to the smoothed buoyant hull afterward so the seated sailor
/// cannot visibly float through the bench as the Dinghy pitches and rolls.
fn sync_aboard_hero_visuals(
    boats: Query<(&CommandedBy, &Transform), (With<PlayerBoat>, With<BoatVisual>)>,
    all_boats: Query<
        (&PlayerPosition, &Transform),
        (With<PlayerBoat>, With<BoatVisual>, Without<Hero>),
    >,
    mut heroes: Query<
        (&CommandedBy, &mut Transform),
        (
            With<AboardBoat>,
            With<Hero>,
            With<crate::hero::HeroVisual>,
            Without<PlayerBoat>,
        ),
    >,
    mut npc_passengers: Query<
        (&PlayerPosition, &mut Transform),
        (
            With<AboardBoat>,
            With<crate::hero::HeroVisual>,
            Without<Hero>,
            Without<PlayerBoat>,
        ),
    >,
) {
    for (owner, mut transform) in heroes.iter_mut() {
        let Some((_, boat_transform)) =
            boats.iter().find(|(boat_owner, _)| boat_owner.0 == owner.0)
        else {
            continue;
        };
        transform.translation = boat_transform.translation + boat_transform.rotation * HELM_LOCAL;
        transform.rotation = boat_transform.rotation;
    }

    // Natural immigrant boats have no player account/CommandedBy authority.
    // Their replicated passenger position is pinned to the authoritative helm,
    // so the closest matching boat snapshot is an unambiguous, bounded visual
    // join (the server permits at most eight such voyages at once).
    for (position, mut transform) in npc_passengers.iter_mut() {
        let Some((_, boat_transform)) = all_boats
            .iter()
            .filter(|(boat_position, _)| boat_position.0.distance_squared(position.0) <= 4.0 * 4.0)
            .min_by(|(a, _), (b, _)| {
                a.0.distance_squared(position.0)
                    .total_cmp(&b.0.distance_squared(position.0))
            })
        else {
            continue;
        };
        transform.translation = boat_transform.translation + boat_transform.rotation * HELM_LOCAL;
        transform.rotation = boat_transform.rotation;
    }
}

fn player_boat_ancestor(
    entity: Entity,
    parents: &Query<&ChildOf>,
    boats: &Query<(), With<PlayerBoat>>,
) -> Option<Entity> {
    let mut ancestor = entity;
    loop {
        if boats.get(ancestor).is_ok() {
            return Some(ancestor);
        }
        ancestor = parents.get(ancestor).ok()?.parent();
    }
}

/// Discover authored nodes after the async glTF scene has instantiated.
fn tag_boat_sail_nodes(
    mut commands: Commands,
    world_time: Query<&WorldTime>,
    cloud_seed: Query<&CloudSeed>,
    warp: Query<&TimeWarp>,
    mut named: Query<
        (
            Entity,
            &Name,
            Option<&mut Transform>,
            Option<&mut MorphWeights>,
        ),
        Added<Name>,
    >,
    parents: Query<&ChildOf>,
    boat_markers: Query<(), With<PlayerBoat>>,
    boats: Query<
        (
            &PlayerRotation,
            Option<&CharacterMotion>,
            Has<WreckedVessel>,
        ),
        With<PlayerBoat>,
    >,
) {
    let wind = current_wind(&world_time, &cloud_seed);
    let time_warp = warp.iter().next().map_or(1.0, |warp| warp.0.max(1.0));
    for (entity, name, mut transform, mut morph) in named.iter_mut() {
        let Some(boat) = player_boat_ancestor(entity, &parents, &boat_markers) else {
            continue;
        };
        if name.as_str() == "DinghySailRig" {
            let rest_rotation = transform
                .as_ref()
                .map_or(Quat::IDENTITY, |transform| transform.rotation);
            let initial = boats
                .get(boat)
                .ok()
                .and_then(|(rotation, motion, wrecked)| {
                    if wrecked {
                        None
                    } else {
                        wind.map(|wind| sail_wind_state(rotation.0, motion, time_warp, wind))
                    }
                });
            if let (Some(transform), Some(initial)) = (transform.as_deref_mut(), initial) {
                transform.rotation = Quat::from_rotation_y(initial.local_yaw) * rest_rotation;
            }
            commands.entity(entity).insert(BoatSailRig {
                boat,
                rest_rotation,
                initialized: initial.is_some(),
            });
            if initial.is_none() {
                // Never expose the authored zero-yaw pose while the replicated
                // clock is still arriving. The first valid wind sample below
                // initializes and reveals it in one Update frame.
                commands.entity(entity).insert(Visibility::Hidden);
            }
        }
        if name.as_str() == "DinghySail" && morph.is_some() {
            if let Some(initial) = boats
                .get(boat)
                .ok()
                .and_then(|(rotation, motion, wrecked)| {
                    if wrecked {
                        None
                    } else {
                        wind.map(|wind| sail_wind_state(rotation.0, motion, time_warp, wind))
                    }
                })
            {
                if let Some(weight) = morph
                    .as_deref_mut()
                    .and_then(|weights| weights.weights_mut().first_mut())
                {
                    *weight = initial.fill;
                }
            }
            commands.entity(entity).insert(BoatSailMorph { boat });
        }
        if name.as_str() == "Dinghy" {
            commands.entity(boat).insert(BoatSceneReady);
        }
    }
}

fn drive_boat_sails(
    time: Res<Time>,
    world_time: Query<&WorldTime>,
    cloud_seed: Query<&CloudSeed>,
    warp: Query<&TimeWarp>,
    boats: Query<
        (
            &PlayerRotation,
            Option<&CharacterMotion>,
            Has<WreckedVessel>,
        ),
        With<PlayerBoat>,
    >,
    mut rigs: Query<(&mut BoatSailRig, &mut Transform, Option<&mut Visibility>)>,
    mut morphs: Query<(&BoatSailMorph, &mut MorphWeights)>,
) {
    let Some(wind) = current_wind(&world_time, &cloud_seed) else {
        return;
    };
    // CharacterMotion is replicated in visual/world-time metres per real
    // second. Normalize it before calculating apparent wind so a 10x or 100x
    // simulation does not visually flatten the sail or reverse it.
    let time_warp = warp.iter().next().map_or(1.0, |warp| warp.0.max(1.0));

    let turn_blend = 1.0 - (-time.delta_secs() / 0.28).exp();
    for (mut rig, mut transform, visibility) in rigs.iter_mut() {
        let Ok((rotation, motion, wrecked)) = boats.get(rig.boat) else {
            continue;
        };
        if wrecked {
            transform.rotation = rig.rest_rotation;
            if let Some(mut visibility) = visibility {
                *visibility = Visibility::Hidden;
            }
            continue;
        }
        if let Some(mut visibility) = visibility {
            *visibility = Visibility::Inherited;
        }
        let state = sail_wind_state(rotation.0, motion, time_warp, wind);
        let target = Quat::from_rotation_y(state.local_yaw) * rig.rest_rotation;
        transform.rotation = if rig.initialized {
            transform.rotation.slerp(target, turn_blend)
        } else {
            rig.initialized = true;
            target
        };
    }

    for (sail, mut weights) in morphs.iter_mut() {
        let Ok((rotation, motion, wrecked)) = boats.get(sail.boat) else {
            continue;
        };
        if wrecked {
            if let Some(weight) = weights.weights_mut().first_mut() {
                *weight = 0.0;
            }
            continue;
        }
        let state = sail_wind_state(rotation.0, motion, time_warp, wind);
        if let Some(weight) = weights.weights_mut().first_mut() {
            *weight = state.fill;
        }
    }
}

#[derive(Clone, Copy)]
struct CurrentWind {
    downwind: Vec2,
    speed: f32,
}

#[derive(Clone, Copy)]
struct SailWindState {
    local_yaw: f32,
    fill: f32,
}

fn current_wind(
    world_time: &Query<&WorldTime>,
    cloud_seed: &Query<&CloudSeed>,
) -> Option<CurrentWind> {
    let clock = world_time.iter().next()?;
    let absolute_seconds = clock.day as f32 * clock.cycle_duration() + clock.seconds_in_cycle;
    let seed_phase =
        shared::wind::wind_seed_phase(cloud_seed.iter().next().map_or(0, |seed| seed.seed));
    let downwind = shared::wind::wind_direction(absolute_seconds, seed_phase);
    let (_, speed) = shared::wind::wind_state(absolute_seconds, seed_phase);
    Some(CurrentWind { downwind, speed })
}

fn sail_wind_state(
    boat_yaw: f32,
    motion: Option<&CharacterMotion>,
    time_warp: f32,
    wind: CurrentWind,
) -> SailWindState {
    let boat_velocity = motion.map_or(Vec2::ZERO, |motion| motion.velocity.xz() / time_warp);
    let apparent = wind.downwind * wind.speed - boat_velocity;
    let apparent_speed = apparent.length();
    let local_yaw = if apparent_speed > 1.0e-4 {
        apparent.x.atan2(apparent.y) - boat_yaw
    } else {
        0.0
    };
    let forward = (Quat::from_rotation_y(boat_yaw) * Vec3::NEG_Z)
        .xz()
        .normalize_or_zero();
    let crosswind = if apparent_speed > 1.0e-4 {
        1.0 - forward.dot(apparent / apparent_speed).abs()
    } else {
        0.0
    };
    let fill = (0.2 + apparent_speed / shared::wind::WIND_SPEED_MAX * 0.45 + crosswind * 0.35)
        .clamp(0.0, 1.0);
    SailWindState { local_yaw, fill }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn drive_opening_cinematic(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    time: Res<Time>,
    terrain: Res<shared::terrain::WorldTerrain>,
    local: Option<Res<LocalPeerId>>,
    account: Option<Res<crate::ui::name_entry::PlayerNameInput>>,
    heroes: Query<
        (Entity, &Hero, &PlayerPosition, &PlayerRotation),
        (
            With<AboardBoat>,
            With<crate::hero::HeroVisual>,
            With<crate::hero::HeroDressed>,
        ),
    >,
    heads: Query<(&crate::hero::CharacterHead, &GlobalTransform)>,
    boats: Query<
        (Entity, &CommandedBy, &PlayerPosition, &PlayerRotation),
        (With<PlayerBoat>, With<BoatSceneReady>),
    >,
    mut cameras: Query<(&mut Transform, &mut CommanderCamera), With<Camera3d>>,
    mut opening: ResMut<OpeningCinematic>,
    mut selection: ResMut<crate::selection::Selection>,
) {
    if matches!(opening.state, CinematicState::Idle) {
        return;
    }
    let Ok((mut camera_transform, mut controller)) = cameras.single_mut() else {
        return;
    };

    if matches!(opening.state, CinematicState::Armed) {
        let Some(local) = local else { return };
        let Some((hero_entity, _, _, hero_rotation)) = heroes
            .iter()
            .find(|(_, hero, ..)| shared::player::peer_id_to_u64(hero.owner) == local.0)
        else {
            return;
        };
        let Some((_, head_transform)) = heads.iter().find(|(head, _)| head.owner == hero_entity)
        else {
            // The character scene is asynchronous. Waiting for its real head
            // transform also gives meshes/materials a render frame to settle.
            return;
        };
        let account = account
            .as_ref()
            .map(|account| account.name.trim().to_lowercase());
        let Some((boat, _, boat_position, _)) = boats
            .iter()
            .find(|(_, owner, ..)| account.as_deref() == Some(owner.0.as_str()))
        else {
            return;
        };

        let face = head_transform.translation() + Vec3::Y * 0.10;
        let hero_basis = Quat::from_rotation_y(hero_rotation.0);
        let hero_forward = hero_basis * Vec3::NEG_Z;
        let hero_right = hero_basis * Vec3::X;
        // Look back at the face from ahead and well off the starboard quarter.
        // The lateral clearance is intentional: the mast is directly ahead of
        // the helm, so a centred portrait shot would hide the sailor behind it.
        let start = Transform::from_translation(
            face + hero_forward * 3.15 + hero_right * 2.70 + Vec3::Y * 0.70,
        )
        .looking_at(face, Vec3::Y);

        controller.focus = boat_position.0;
        controller.focus_target = boat_position.0;
        controller.zoom = FINAL_RTS_ZOOM;
        controller.zoom_target = FINAL_RTS_ZOOM;
        controller.tilt = crate::camera_rts::commander_tilt_for_zoom(
            controller.zoom,
            controller.zoom_min,
            controller.zoom_max,
        );
        // Keep the final RTS camera on the portrait's bearing. Rotating to the
        // opposite side during a pull-back makes the subject leave frame.
        let portrait_offset = start.translation - face;
        controller.yaw = portrait_offset.x.atan2(portrait_offset.z);
        controller.yaw_target = controller.yaw;
        let mut end = Transform::IDENTITY;
        crate::camera_rts::apply_commander_transform(&mut end, &controller, Some(&terrain));
        let end_target = boat_position.0 + Vec3::Y * 0.45;
        *camera_transform = start;
        if !opening.intro_played {
            commands.spawn((
                Name::new("Opening voyage music"),
                AudioPlayer::new(asset_server.load(GAME_INTRO_AUDIO)),
                PlaybackSettings::DESPAWN.with_volume(Volume::Linear(0.82)),
            ));
            opening.intro_played = true;
            info!("opening cinematic: playing {GAME_INTRO_AUDIO}");
        }
        info!(
            "opening cinematic: hero={hero_entity:?} head={face:?} start={:?} end={:?}",
            start.translation, end.translation
        );
        opening.state = CinematicState::Running {
            elapsed: 0.0,
            boat,
            start,
            start_target: face,
            end_target,
            end,
        };
        return;
    }

    let CinematicState::Running {
        elapsed,
        boat,
        start,
        start_target,
        end_target,
        end,
    } = &mut opening.state
    else {
        return;
    };
    *elapsed += time.delta_secs();
    // The portrait is deliberately calm, but never frozen: it opens with a
    // slow dolly backward and slight rise before the wider crane begins.
    let away_from_face = (start.translation - *start_target).normalize_or_zero();
    let portrait_end = start.translation
        + away_from_face * PORTRAIT_PULLBACK_METERS
        + Vec3::Y * PORTRAIT_RISE_METERS;
    let hold_raw = (*elapsed / CAMERA_HOLD_SECONDS).clamp(0.0, 1.0);
    let hold_eased = smoothstep01(hold_raw);
    let raw = ((*elapsed - CAMERA_HOLD_SECONDS) / CAMERA_TRAVEL_SECONDS).clamp(0.0, 1.0);
    let eased = smoothstep01(raw);
    // Crane backward and upward while keeping the subject on-screen. As the
    // frame widens, attention shifts from the sailor's face to the full boat.
    let position = if *elapsed < CAMERA_HOLD_SECONDS {
        start.translation.lerp(portrait_end, hold_eased)
    } else {
        portrait_end.lerp(end.translation, eased)
    };
    let target = start_target.lerp(*end_target, eased);
    *camera_transform = Transform::from_translation(position).looking_at(target, Vec3::Y);
    if raw >= 1.0 {
        *camera_transform = *end;
        selection.set(vec![*boat]);
        opening.state = CinematicState::Idle;
    }
}

/// Once the authoritative aboard marker disappears, hand control cleanly from
/// the wreck to the hero. This makes shore arrival one interaction rather than
/// "disembark, then hunt for and reselect the tiny character".
fn focus_disembarked_hero(
    mut removed: RemovedComponents<AboardBoat>,
    local: Option<Res<LocalPeerId>>,
    heroes: Query<(&Hero, &PlayerPosition)>,
    mut selection: ResMut<crate::selection::Selection>,
    mut cameras: Query<&mut CommanderCamera>,
) {
    let Some(local) = local else { return };
    for entity in removed.read() {
        let Ok((hero, position)) = heroes.get(entity) else {
            continue;
        };
        if shared::player::peer_id_to_u64(hero.owner) != local.0 {
            continue;
        }
        selection.set(vec![entity]);
        for mut camera in cameras.iter_mut() {
            camera.focus_target = position.0;
        }
    }
}

fn sync_voyage_hint(
    mut commands: Commands,
    opening: Res<OpeningCinematic>,
    local: Option<Res<LocalPeerId>>,
    aboard: Query<&Hero, With<AboardBoat>>,
    existing: Query<Entity, With<VoyageHint>>,
) {
    let active = !opening.is_active()
        && local.as_ref().is_some_and(|local| {
            aboard
                .iter()
                .any(|hero| shared::player::peer_id_to_u64(hero.owner) == local.0)
        });
    if !active {
        for entity in existing.iter() {
            commands.entity(entity).despawn();
        }
        return;
    }
    if !existing.is_empty() {
        return;
    }
    commands
        .spawn((
            VoyageHint,
            Node {
                position_type: PositionType::Absolute,
                top: Val::Px(18.0),
                left: Val::Percent(50.0),
                margin: UiRect::left(Val::Px(-220.0)),
                width: Val::Px(440.0),
                padding: UiRect::axes(Val::Px(18.0), Val::Px(10.0)),
                justify_content: JustifyContent::Center,
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(7.0)),
                ..default()
            },
            BackgroundColor(crate::ui::styles::LIMEWASH),
            BorderColor::from(crate::ui::styles::PLATE_RULE),
            GlobalZIndex(60),
        ))
        .with_children(|root| {
            root.spawn((
                Text::new("RIGHT-CLICK WATER TO SAIL  |  RIGHT-CLICK NEARBY LAND TO DISEMBARK"),
                TextFont {
                    font_size: FontSize::Px(11.0),
                    ..default()
                },
                TextColor(crate::ui::styles::INK),
            ));
        });
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use bevy::prelude::*;

    use super::{orientation_from_surface_samples, smoothstep01, GAME_INTRO_AUDIO};

    #[test]
    fn opening_ease_starts_and_finishes_exactly() {
        for (raw, expected) in [(0.0_f32, 0.0_f32), (1.0, 1.0)] {
            let eased = smoothstep01(raw);
            assert!((eased - expected).abs() < f32::EPSILON);
        }
    }

    #[test]
    fn opening_music_ships_in_the_runtime_asset_tree() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("assets")
            .join(GAME_INTRO_AUDIO);
        assert!(path.is_file(), "missing opening music: {}", path.display());
    }

    #[test]
    fn flat_water_preserves_the_authoritative_yaw() {
        let yaw = 0.73;
        let pose = orientation_from_surface_samples(yaw, 4.0, 4.0, 4.0, 4.0);
        let expected = Quat::from_rotation_y(yaw);

        assert!((pose * Vec3::NEG_Z).distance(expected * Vec3::NEG_Z) < 1.0e-5);
        assert!((pose * Vec3::Y).distance(Vec3::Y) < 1.0e-5);
    }

    #[test]
    fn higher_bow_visually_lifts_the_authored_bow() {
        let pose = orientation_from_surface_samples(0.0, 0.4, -0.4, 0.0, 0.0);
        let bow = pose * Vec3::NEG_Z;

        assert!(bow.y > 0.0, "higher bow should pitch upward: {bow:?}");
        assert!(pose.is_finite());
    }

    #[test]
    fn higher_starboard_visually_lifts_the_authored_right_side() {
        let pose = orientation_from_surface_samples(0.0, 0.0, 0.0, 0.3, -0.3);
        let starboard = pose * Vec3::X;

        assert!(
            starboard.y > 0.0,
            "higher starboard should roll upward: {starboard:?}"
        );
        assert!(pose.is_finite());
    }
}
