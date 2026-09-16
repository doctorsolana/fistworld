//! Authoritative wildlife behavior, independent of camera and client presence.
use super::*;
use crate::world::simulation_time::SimulationTime;
use bevy::ecs::system::SystemState;
use bevy::prelude::*;
use shared::worldgen::splitmix64;
use shared::{components::*, region::RegionCoord, terrain::WorldTerrain};

#[derive(Default)]
pub struct WildlifeScratch {
    time: Option<SystemState<SimulationTime<'static, 'static>>>,
    actors: Vec<(Entity, u64, Vec3)>,
    bodies: Vec<(Entity, Vec2)>,
}

/// A grazing step may straddle a chunk boundary. Missing blockers mean wait,
/// not clear ground; retain the same decision and destination until ready.
fn blockers_ready(world: &World, from: Vec3, to: Vec3) -> bool {
    let Some(colliders) = world.get_resource::<crate::collision::library::StaticColliders>() else {
        return true;
    };
    let min = shared::terrain::ChunkCoord::from_world_pos(
        from.min(to) - Vec3::new(HORSE_CLEARANCE, 0., HORSE_CLEARANCE),
    );
    let max = shared::terrain::ChunkCoord::from_world_pos(
        from.max(to) + Vec3::new(HORSE_CLEARANCE, 0., HORSE_CLEARANCE),
    );
    (min.x..=max.x).all(|x| {
        (min.z..=max.z).all(|z| {
            colliders
                .loaded_chunks
                .contains(&shared::terrain::ChunkCoord::new(x, z))
        })
    })
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
            .query_filtered::<(Entity, &Horse, &PlayerPosition), With<WildHorse>>()
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
        // Every wild horse requests a small local blocker footprint. Wait for
        // its chunk before moving; cameras never control this readiness.
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
            if !blockers_ready(world, at, next) {
                continue;
            }
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
                if !blockers_ready(world, at, candidate) {
                    continue;
                }
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
