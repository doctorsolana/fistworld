//! Sparse authoritative firing events; flight is analytic and damage resolves once.
use super::*;
use crate::player::combat::world_clock_seconds;
use crate::world::{simulation_time::SimulationTime, village::mortality::PendingDeathCause};
use lightyear::prelude::{NetworkTarget, Replicate};

#[allow(clippy::type_complexity)]
pub fn advance_catapults(
    mut commands: Commands,
    clock: Query<&WorldTime>,
    time: SimulationTime,
    terrain: Option<Res<WorldTerrain>>,
    targets: Query<(&PlayerPosition, &Health), Without<crate::player::hero::OfflineHero>>,
    mut machines: Query<(
        Entity,
        &PlayerPosition,
        &mut PlayerRotation,
        &Health,
        &mut Catapult,
        &mut CatapultStatus,
        &mut CharacterMotion,
        Option<&SiegeTarget>,
        Has<MarchOrder>,
        Has<MoveTarget>,
        Has<NavigationRouteFailed>,
        Option<&CatapultWreck>,
    )>,
) {
    let Some(clock) = clock.iter().next() else {
        return;
    };
    let now = world_clock_seconds(clock);
    for (
        entity,
        position,
        mut rotation,
        health,
        mut catapult,
        mut status,
        mut motion,
        target,
        marching,
        moving,
        failed,
        wreck,
    ) in &mut machines
    {
        let mut next = *status;
        if health.is_dead() {
            if let Some(wreck) = wreck {
                if now - wreck.0 > 3.0 {
                    commands.entity(entity).despawn();
                }
            } else {
                commands
                    .entity(entity)
                    .remove::<(SiegeTarget, MarchOrder, MoveTarget, TravelRoute)>()
                    .insert(CatapultWreck(now));
            }
            next.phase = SiegePhase::Destroyed;
            motion.set_if_neq(CharacterMotion::STATIONARY);
        } else if failed {
            next.phase = SiegePhase::Blocked;
            motion.set_if_neq(CharacterMotion::STATIONARY);
        } else if marching || moving {
            next.phase = SiegePhase::Moving;
        } else {
            motion.set_if_neq(CharacterMotion::STATIONARY);
            if now < next.ready_at {
                next.phase = SiegePhase::Reloading;
            } else if catapult.ammunition == 0 {
                next.phase = SiegePhase::Empty;
            } else if let Some(target) = target {
                let aim = match *target {
                    SiegeTarget::Ground(point) => Some(point),
                    SiegeTarget::Person(person) => targets
                        .get(person)
                        .ok()
                        .filter(|(_, health)| !health.is_dead())
                        .map(|(p, _)| p.0),
                };
                if let Some(mut aim) = aim {
                    if let Some(terrain) = terrain.as_deref() {
                        aim.y = terrain.get_height(aim.x, aim.z);
                    }
                    if next.phase == SiegePhase::Winding {
                        // Target is deliberately locked for the windup: running
                        // soldiers can evade the stone; no homing correction.
                        if now >= next.fire_at {
                            let aim = next.aim.expect("windup has an aim");
                            let launch = next.fire_at + 0.14;
                            let socket = catapult_stone_socket(&next, launch);
                            let origin = position.0 + Quat::from_rotation_y(rotation.0) * socket;
                            let projectile = trajectory(
                                origin,
                                aim,
                                launch,
                                entity.to_bits() ^ next.fire_at.to_bits(),
                                terrain.as_deref(),
                            );
                            commands.spawn((
                                projectile,
                                RegionCoord::from_world_pos(origin),
                                Replicate::to_clients(NetworkTarget::All),
                            ));
                            catapult.ammunition -= 1;
                            next.ready_at = now + CATAPULT_RELOAD;
                            next.phase = SiegePhase::Reloading;
                        }
                    } else if !siege_in_range(position.0, aim) {
                        next.phase = SiegePhase::OutOfRange;
                        next.aim = Some(aim);
                    } else {
                        let (yaw, aligned) = turn_siege_towards(
                            rotation.0,
                            (aim - position.0).xz(),
                            time.world_seconds(),
                        );
                        rotation.set_if_neq(PlayerRotation(yaw));
                        next.aim = Some(aim);
                        if aligned {
                            next.phase = SiegePhase::Winding;
                            next.cycle_at = now;
                            next.fire_at = now + CATAPULT_WINDUP;
                        } else {
                            next.phase = SiegePhase::Turning;
                        }
                    }
                } else {
                    commands.entity(entity).remove::<SiegeTarget>();
                    next.phase = SiegePhase::Ready;
                    next.aim = None;
                    if status.phase == SiegePhase::Winding {
                        next.fire_at = 0.0;
                    }
                }
            } else {
                next.phase = SiegePhase::Ready;
            }
        }
        status.set_if_neq(next);
    }
}

pub(super) fn trajectory(
    origin: Vec3,
    aim: Vec3,
    launch: f64,
    seed: u64,
    terrain: Option<&WorldTerrain>,
) -> SiegeProjectile {
    let duration = (origin.xz().distance(aim.xz()) / 23.0).clamp(1.8, 5.2);
    let mut p = SiegeProjectile {
        origin,
        aim,
        impact: aim,
        launched_at: launch,
        flight_seconds: duration,
        impact_at: launch + f64::from(duration),
        seed,
    };
    if let Some(terrain) = terrain {
        let below = |time| {
            let point = p.position(time);
            point.y <= terrain.get_height(point.x, point.z)
        };
        // Find the first terrain crossing, including a ridge before the aim.
        // Retaining full flight_seconds keeps client and server arcs identical.
        for step in 1..=96 {
            let mut hi = launch + f64::from(duration) * f64::from(step) / 96.0;
            if !below(hi) {
                continue;
            }
            let mut lo = launch + f64::from(duration) * f64::from(step - 1) / 96.0;
            for _ in 0..10 {
                let mid = (lo + hi) * 0.5;
                if below(mid) {
                    hi = mid;
                } else {
                    lo = mid;
                }
            }
            p.impact_at = hi;
            p.impact = p.position(hi);
            p.impact.y = terrain.get_height(p.impact.x, p.impact.z);
            break;
        }
    }
    p
}

#[allow(clippy::type_complexity)]
pub fn resolve_siege_projectiles(
    mut commands: Commands,
    clock: Query<&WorldTime>,
    mut stones: Query<(Entity, &SiegeProjectile, &mut RegionCoord)>,
    impacts: Query<(Entity, &SiegeImpact)>,
    mut bodies: Query<
        (Entity, &PlayerPosition, &mut Health, Has<CharacterKind>),
        (
            Or<(With<CharacterKind>, With<Catapult>)>,
            Without<crate::player::hero::OfflineHero>,
            Without<AboardBoat>,
        ),
    >,
    mut landed: Local<Vec<SiegeImpact>>,
) {
    let Some(clock) = clock.iter().next() else {
        return;
    };
    let now = world_clock_seconds(clock);
    landed.clear();
    for (entity, stone, mut region) in &mut stones {
        if now >= stone.impact_at {
            let impact = SiegeImpact {
                position: stone.impact,
                at: stone.impact_at,
                seed: stone.seed,
            };
            landed.push(impact);
            region.set_if_neq(RegionCoord::from_world_pos(impact.position));
            commands
                .entity(entity)
                .remove::<SiegeProjectile>()
                .insert((impact, crate::player::army::UnansweredBombardment));
        } else {
            region.set_if_neq(RegionCoord::from_world_pos(stone.position(now)));
        }
    }
    // One victim pass only on impact ticks, independent of the number of
    // airborne stones. No global scan or damage traffic during flight.
    if !landed.is_empty() {
        for (entity, position, mut health, person) in &mut bodies {
            if health.is_dead() {
                continue;
            }
            let damage: f32 = landed
                .iter()
                .map(|impact| siege_splash_damage(position.0.distance(impact.position)))
                .sum();
            if damage <= 0.0 {
                continue;
            }
            let fatal = health.take_damage(damage);
            if person {
                commands
                    .entity(entity)
                    .insert(CombatReaction { at: now, fatal });
                if fatal {
                    commands
                        .entity(entity)
                        .insert(PendingDeathCause(DeathCause::Combat));
                }
            }
        }
    }
    for (entity, impact) in &impacts {
        if now - impact.at > 2.4 {
            commands.entity(entity).despawn();
        }
    }
}
