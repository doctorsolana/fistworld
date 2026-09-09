//! Mounted-pair lifecycle. Ambient animals belong to world::wildlife.
use super::*;
use shared::region::RegionCoord;
use std::collections::{HashMap, HashSet};

#[derive(Default)]
pub struct MountScratch {
    riders: HashMap<PersonId, (Entity, Mounted)>,
    horses: Vec<(Entity, Horse, Vec3)>,
    ids: HashSet<u64>,
}

pub fn tick(world: &mut World, mut scratch: Local<MountScratch>) {
    // Wildlife is independent of mounted-pair reconciliation. The usual case
    // (no riders yet) needs no scratch allocations or per-animal updates.
    if world.query::<&Mounted>().iter(world).next().is_none()
        && world
            .query::<&Horse>()
            .iter(world)
            .all(|h| h.rider.is_none())
        && world.query::<&super::cavalry::CavalryMount>().iter(world).next().is_none()
    {
        return;
    }
    let now = now(world);
    let MountScratch {
        riders,
        horses,
        ids,
    } = &mut *scratch;
    riders.clear();
    horses.clear();
    ids.clear();
    riders.extend(
        world
            .query::<(Entity, &PersonId, &Mounted)>()
            .iter(world)
            .map(|(e, p, m)| (*p, (e, *m))),
    );
    horses.extend(
        world
            .query::<(Entity, &Horse, &PlayerPosition)>()
            .iter(world)
            .map(|(e, h, p)| (e, *h, p.0)),
    );
    ids.extend(horses.iter().map(|(_, h, _)| h.id));
    // Replicated mounts are a pair. Never leave a rider suspended after a
    // horse despawn, and never leave a horse leased after death/disconnection.
    for &(rider, mounted) in riders.values() {
        if !ids.contains(&mounted.horse) {
            stop(world, rider);
            world
                .entity_mut(rider)
                .remove::<(Mounted, DismountLanding)>();
            if world.get::<SoldierRole>(rider) == Some(&SoldierRole::Cavalry) {
                world.entity_mut(rider).insert(SoldierRole::Infantry);
            }
        }
    }
    for &(entity, horse, at) in horses.iter() {
        if let Some(person) = horse.rider {
            let rider = riders.get(&person).copied().filter(|(e, m)| {
                m.horse == horse.id
                    && world.get::<OfflineHero>(*e).is_none()
                    && world.get::<Health>(*e).is_none_or(|h| !h.is_dead())
                    && world.get::<AboardBoat>(*e).is_none()
                    && (world.get::<super::cavalry::CavalryMount>(entity).is_none() || world.get::<CommandedBy>(*e).is_some())
            });
            if let Some((rider, mut mounted)) = rider {
                if world.get::<SoldierRole>(rider) == Some(&SoldierRole::Cavalry) {
                    world.entity_mut(rider).insert_if_new(CombatReady);
                }
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
        } else if world.get::<super::cavalry::CavalryMount>(entity).is_some() {
            world.despawn(entity);
        }
    }
}

fn release(world: &mut World, horse: Entity, now: f64) {
    if world.get::<super::cavalry::CavalryMount>(horse).is_some() {
        world.despawn(horse);
        return;
    }
    world.get_mut::<Horse>(horse).unwrap().rider = None;
    world.entity_mut(horse).insert(CharacterMotion::STATIONARY);
    if let Some(mut wild) = world.get_mut::<WildHorse>(horse) {
        wild.next_decision = now + 4.;
        wild.target = None;
    }
    set_activity(world, horse, HorseActivity::Idle, now);
}
