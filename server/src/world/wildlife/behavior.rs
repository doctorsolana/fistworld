//! Observation-budgeted ambient behavior; no offscreen pathfinding or ticking rigs.
use super::*;
use crate::world::simulation_time::SimulationTime;
use bevy::ecs::system::SystemState;
use bevy::prelude::*;
use shared::worldgen::splitmix64;
use shared::{components::*, region::RegionCoord, terrain::WorldTerrain};

#[derive(Default)]
pub struct ObservationScratch {
    elapsed: f32,
    observers: Vec<Vec3>,
    candidates: Vec<(Entity, f32, u64)>,
}

pub fn update_observation(
    mut commands: Commands,
    time: SimulationTime,
    players: Query<&PlayerPosition, With<Player>>,
    horses: Query<(Entity, &Horse, &PlayerPosition, Has<ActiveWildHorse>)>,
    clocks: Query<&WorldTime>,
    mut scratch: Local<ObservationScratch>,
) {
    scratch.elapsed += time.real_seconds();
    if scratch.elapsed < 0.5 {
        return;
    }
    scratch.elapsed = 0.0;
    scratch.observers.clear();
    scratch.observers.extend(players.iter().map(|p| p.0));
    scratch.candidates.clear();
    for (e, horse, at, active) in &horses {
        if horse.rider.is_some() {
            continue;
        }
        let distance = scratch
            .observers
            .iter()
            .map(|p| p.xz().distance_squared(at.0.xz()))
            .fold(f32::INFINITY, f32::min);
        let radius = OBSERVATION_RADIUS + if active { 25.0 } else { 0.0 };
        if distance <= radius * radius {
            scratch.candidates.push((e, distance, horse.id));
        }
    }
    scratch
        .candidates
        .sort_unstable_by(|a, b| a.1.total_cmp(&b.1).then(a.2.cmp(&b.2)));
    scratch.candidates.truncate(MAX_ACTIVE_HORSES);
    let now = clocks.iter().next().map_or(0., |c| {
        f64::from(c.day) * f64::from(c.cycle_duration()) + f64::from(c.seconds_in_cycle)
    });
    for (e, horse, _, active) in &horses {
        let wanted = scratch.candidates.iter().any(|candidate| candidate.0 == e);
        if wanted && !active {
            commands.entity(e).insert(ActiveWildHorse);
        }
        if !wanted && active {
            commands.entity(e).remove::<ActiveWildHorse>();
            // Keep the exact ground position. Demotion only clears velocity;
            // it never invents a replacement horse or teleports an old one.
            if horse.rider.is_none() {
                commands.entity(e).insert((
                    CharacterMotion::STATIONARY,
                    HorseAnimation {
                        activity: HorseActivity::Graze,
                        since: now,
                    },
                ));
            }
        }
    }
}

#[derive(Default)]
pub struct WildlifeScratch {
    time: Option<SystemState<SimulationTime<'static, 'static>>>,
    actors: Vec<(Entity, u64, Vec3)>,
    bodies: Vec<(Entity, Vec2)>,
}

pub fn tick(world: &mut World, mut scratch: Local<WildlifeScratch>) {
    let state = scratch.time.get_or_insert_with(|| SystemState::new(world));
    let dt = state.get(world).expect("wildlife simulation time");
    let real_dt = dt.real_seconds().max(1e-5);
    let dt = dt.world_seconds();
    let now = now(world);
    scratch.actors.clear();
    scratch.actors.extend(
        world
            .query_filtered::<(Entity, &Horse, &PlayerPosition), With<ActiveWildHorse>>()
            .iter(world)
            .filter(|(_, h, _)| h.rider.is_none())
            .map(|(e, h, p)| (e, h.id, p.0)),
    );
    if scratch.actors.is_empty() {
        return;
    }
    scratch.actors.sort_unstable_by_key(|(_, id, _)| *id);
    scratch.bodies.clear();
    scratch.bodies.extend(
        world
            .query::<(Entity, &Horse, &PlayerPosition)>()
            .iter(world)
            .map(|(e, _, p)| (e, p.0.xz())),
    );
    for index in 0..scratch.actors.len() {
        let (entity, id, at) = scratch.actors[index];
        // Activation asks the existing streamer for local blockers. Wait for
        // that chunk before taking the first physical step after promotion.
        if world
            .get_resource::<crate::collision::library::StaticColliders>()
            .is_some_and(|c| {
                !c.loaded_chunks
                    .contains(&shared::terrain::ChunkCoord::from_world_pos(at))
            })
        {
            continue;
        }
        let Some(wild) = world.get::<WildHorse>(entity) else {
            continue;
        };
        let (home, next_decision, serial, target) =
            (wild.home, wild.next_decision, wild.serial, wild.target);
        if let Some(target) = target {
            let delta = (target - at).xz();
            let distance = delta.length();
            let step = (HorseGait::Walk.speed() * dt).min(distance);
            let next = grounded(
                world,
                at + Vec3::new(delta.x, 0., delta.y).normalize_or_zero() * step,
            );
            let crowded = scratch.bodies.iter().any(|(other, p)| {
                *other != entity
                    && shared::components::distance_squared_to_segment(*p, at.xz(), next.xz())
                        < (HORSE_CLEARANCE * 2.).powi(2)
            });
            if distance < 0.04 || crowded || !clear(world, at, next, HORSE_CLEARANCE) {
                let mut wild = world.get_mut::<WildHorse>(entity).unwrap();
                wild.target = None;
                wild.next_decision = now + 3.0 + (id % 4) as f64;
                world
                    .get_mut::<CharacterMotion>(entity)
                    .unwrap()
                    .set_if_neq(CharacterMotion::STATIONARY);
                set_activity(world, entity, HorseActivity::Graze, now);
            } else {
                world
                    .get_mut::<PlayerPosition>(entity)
                    .unwrap()
                    .set_if_neq(PlayerPosition(next));
                world
                    .get_mut::<PlayerRotation>(entity)
                    .unwrap()
                    .set_if_neq(PlayerRotation((-delta.x).atan2(-delta.y)));
                world
                    .get_mut::<RegionCoord>(entity)
                    .unwrap()
                    .set_if_neq(RegionCoord::from_world_pos(next));
                world
                    .get_mut::<CharacterMotion>(entity)
                    .unwrap()
                    .set_if_neq(CharacterMotion::new((next - at) / real_dt));
                if let Some((_, p)) = scratch.bodies.iter_mut().find(|(e, _)| *e == entity) {
                    *p = next.xz();
                }
                set_activity(world, entity, HorseActivity::Moving(HorseGait::Walk), now);
            }
        } else if now >= next_decision {
            let serial = splitmix64(serial);
            let activity = match serial % 5 {
                0 => HorseActivity::Alert,
                1 => HorseActivity::Idle,
                _ => HorseActivity::Graze,
            };
            let mut destination = None;
            if serial % 3 == 0 {
                let angle = (serial >> 32) as f32 / u32::MAX as f32 * std::f32::consts::TAU;
                let candidate =
                    grounded(world, home + Vec3::new(angle.cos(), 0., angle.sin()) * 5.0);
                if world
                    .get_resource::<WorldTerrain>()
                    .is_none_or(|t| population::habitat(t, candidate))
                    && clear(world, at, candidate, HORSE_CLEARANCE)
                {
                    destination = Some(candidate);
                }
            }
            let mut wild = world.get_mut::<WildHorse>(entity).unwrap();
            wild.serial = serial;
            wild.target = destination;
            wild.next_decision = now + 5.0 + (serial % 7) as f64;
            set_activity(world, entity, activity, now);
        }
    }
}
