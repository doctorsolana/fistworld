//! Bounded wild behaviour and mount cleanup. Wandering uses short, certified
//! ground segments; ridden routes use the ordinary shared tactical planner.
use super::*;
use crate::world::simulation_time::SimulationTime;
use bevy::ecs::system::SystemState;
use std::collections::HashMap;

pub fn tick(
    world: &mut World,
    mut timing: Local<Option<SystemState<SimulationTime<'static, 'static>>>>,
) {
    let state = timing.get_or_insert_with(|| SystemState::new(world));
    let time = state.get(world).expect("riding simulation time");
    let dt = time.world_seconds();
    let real_dt = time.real_seconds().max(1e-5);
    let now = now(world);
    let riders: HashMap<_, _> = world
        .query::<(Entity, &PersonId, &Mounted)>()
        .iter(world)
        .map(|(e, p, m)| (*p, (e, *m)))
        .collect();
    let horses: Vec<_> = world
        .query::<(Entity, &Horse, &PlayerPosition)>()
        .iter(world)
        .map(|(e, h, p)| (e, *h, p.0))
        .collect();
    // Replicated mounts are a pair. Never leave a rider suspended after a
    // horse despawn, and never leave a horse leased after death/disconnection.
    for &(rider, mounted) in riders.values() {
        if !horses.iter().any(|(_, h, _)| h.id == mounted.horse) {
            stop(world, rider);
            world
                .entity_mut(rider)
                .remove::<(Mounted, DismountLanding)>();
        }
    }
    for (entity, horse, at) in horses {
        if let Some(person) = horse.rider {
            let rider = riders.get(&person).copied().filter(|(e, m)| {
                m.horse == horse.id
                    && world.get::<OfflineHero>(*e).is_none()
                    && world.get::<Health>(*e).is_none_or(|h| !h.is_dead())
                    && world.get::<AboardBoat>(*e).is_none()
            });
            if let Some((rider, mut mounted)) = rider {
                if now - mounted.since >= HORSE_TRANSITION_SECONDS {
                    match mounted.phase {
                        RidingPhase::Mounting => {
                            mounted.phase = RidingPhase::Riding;
                            mounted.since = now;
                            world.entity_mut(rider).insert(mounted);
                        }
                        RidingPhase::Dismounting => {
                            let landing = world.get::<DismountLanding>(rider).map_or(at, |l| l.0);
                            if clear(world, at, landing, 0.24) {
                                let landing = grounded(world, landing);
                                world.entity_mut(rider).insert((
                                    PlayerPosition(landing),
                                    RegionCoord::from_world_pos(landing),
                                    CharacterMotion::STATIONARY,
                                ));
                                world
                                    .entity_mut(rider)
                                    .remove::<(Mounted, DismountLanding)>();
                                release(world, entity, now);
                                continue;
                            }
                            mounted.phase = RidingPhase::Riding;
                            mounted.since = now;
                            world
                                .entity_mut(rider)
                                .insert(mounted)
                                .remove::<DismountLanding>();
                        }
                        RidingPhase::Riding => {}
                    }
                }
                let position = world.get::<PlayerPosition>(rider).unwrap().0;
                let rotation = world.get::<PlayerRotation>(rider).unwrap().clone();
                let motion = *world
                    .get::<CharacterMotion>(rider)
                    .unwrap_or(&CharacterMotion::STATIONARY);
                world
                    .get_mut::<PlayerPosition>(entity)
                    .unwrap()
                    .set_if_neq(PlayerPosition(position));
                world
                    .get_mut::<PlayerRotation>(entity)
                    .unwrap()
                    .set_if_neq(rotation);
                world
                    .get_mut::<RegionCoord>(entity)
                    .unwrap()
                    .set_if_neq(RegionCoord::from_world_pos(position));
                world
                    .get_mut::<CharacterMotion>(entity)
                    .unwrap()
                    .set_if_neq(motion);
                set_activity(
                    world,
                    entity,
                    if mounted.phase == RidingPhase::Riding && motion.is_moving() {
                        HorseActivity::Moving(mounted.gait)
                    } else {
                        HorseActivity::Idle
                    },
                    now,
                );
                continue;
            }
            if let Some(&(rider, _)) = riders.get(&person) {
                stop(world, rider);
                world
                    .entity_mut(rider)
                    .remove::<(Mounted, DismountLanding)>();
            }
            release(world, entity, now);
        }
        let Some(wild) = world.get::<WildHorse>(entity) else {
            continue;
        };
        let (home, next, serial, target) =
            (wild.home, wild.next_decision, wild.serial, wild.target);
        if let Some(target) = target {
            let delta = (target - at).xz();
            let distance = delta.length();
            let step = (HorseGait::Walk.speed() * dt).min(distance);
            let next = grounded(
                world,
                at + Vec3::new(delta.x, 0., delta.y).normalize_or_zero() * step,
            );
            if distance < 0.04 || !clear(world, at, next, HORSE_CLEARANCE) {
                world.get_mut::<WildHorse>(entity).unwrap().target = None;
                world.entity_mut(entity).insert(CharacterMotion::STATIONARY);
                set_activity(world, entity, HorseActivity::Idle, now);
            } else {
                let yaw = (-delta.x).atan2(-delta.y);
                world.entity_mut(entity).insert((
                    PlayerPosition(next),
                    PlayerRotation(yaw),
                    RegionCoord::from_world_pos(next),
                ));
                world
                    .get_mut::<CharacterMotion>(entity)
                    .unwrap()
                    .set_if_neq(CharacterMotion::new((next - at) / real_dt));
                set_activity(world, entity, HorseActivity::Moving(HorseGait::Walk), now);
            }
        } else if now >= next {
            // Deterministic per-horse sequence; one candidate, no unbounded search.
            let serial = serial.wrapping_mul(6364136223846793005).wrapping_add(1);
            let activity = match serial % 4 {
                0 => HorseActivity::Alert,
                1 => HorseActivity::Idle,
                _ => HorseActivity::Graze,
            };
            let mut destination = None;
            if serial % 4 == 1 {
                let angle = (serial >> 32) as f32 / u32::MAX as f32 * std::f32::consts::TAU;
                let candidate = grounded(
                    world,
                    home + Vec3::new(angle.cos() * 4., 0., angle.sin() * 4.),
                );
                if clear(world, at, candidate, HORSE_CLEARANCE) {
                    destination = Some(candidate);
                }
            }
            let mut wild = world.get_mut::<WildHorse>(entity).unwrap();
            wild.serial = serial;
            wild.target = destination;
            wild.next_decision = now + 8. + (serial % 5) as f64;
            set_activity(world, entity, activity, now);
        }
    }
}
fn release(world: &mut World, horse: Entity, now: f64) {
    world.get_mut::<Horse>(horse).unwrap().rider = None;
    world.entity_mut(horse).insert(CharacterMotion::STATIONARY);
    if let Some(mut wild) = world.get_mut::<WildHorse>(horse) {
        wild.next_decision = now + 4.;
        wild.target = None;
    }
    set_activity(world, horse, HorseActivity::Idle, now);
}
