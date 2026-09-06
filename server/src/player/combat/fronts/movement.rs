use super::*;
use crate::player::{
    combat::AttackOrder,
    hero::{navigation_segment_clear, MoveTarget},
    orders::{CommandStance, FormationRoutes, MarchOrder},
};
use crate::world::village_roads::{NavigationRouteFailed, NavigationRoutePending, TravelRoute};
#[derive(Default)]
pub struct PlanningScratch {
    reservations: Vec<(BattalionId, Face)>,
    plans: Vec<(u64, Option<Footprint>)>,
}

/// Slow formation decisions, cheap continuous movement through the existing
/// authoritative mover. File queues survive losses; only the affected file
/// advances. An explicit individual order detaches that soldier immediately.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn advance_battle_fronts(
    mut commands: Commands,
    mut formations: ResMut<CombatFormations>,
    space: Res<CombatSpace>,
    mut routes: Option<ResMut<FormationRoutes>>,
    mut scratch: Local<PlanningScratch>,
    clock: Query<&WorldTime>,
    mut last: Local<f64>,
    terrain: Option<Res<shared::terrain::WorldTerrain>>,
    buildings: Option<Res<shared::spatial::SpatialObstacleGrid>>,
    colliders: Option<Res<crate::collision::library::StaticColliders>>,
    derived: Option<Res<crate::collision::library::DerivedColliderLibrary>>,
    policies: Query<&BattalionStance>,
    mut units: Query<(
        &FormationMember,
        Option<&MemberOfBattalion>,
        &PlayerPosition,
        Option<&MoveTarget>,
        Has<MarchOrder>,
        Has<AttackOrder>,
        &mut PlayerRotation,
        &mut CharacterMotion,
        &mut CharacterActivity,
    )>,
) {
    let Some(clock) = clock.iter().next() else {
        return;
    };
    let now = super::super::world_clock_seconds(clock);
    if now >= *last && now - *last < 0.1 {
        return;
    }
    *last = now;
    // Bounded by the retained tactical roster, not the world's population.
    for (&id, front) in &mut formations.fronts {
        for column in &mut front.columns {
            column.retain(|e| {
                units.get(*e).is_ok_and(|(member, bat, ..)| {
                    member.group == id && member.battalion == bat.map(|m| m.0)
                }) && space.body(*e).is_some()
            });
        }
    }
    formations
        .fronts
        .retain(|_, f| f.columns.iter().any(|c| !c.is_empty()));
    let PlanningScratch {
        reservations,
        plans,
    } = &mut *scratch;
    reservations.clear();
    plans.clear();
    for front in formations.fronts.values() {
        if let (Intent::Attack(Enemy::Battalion(target)), Some(face)) = (front.intent, front.sector)
        {
            reservations.push((target, face));
        }
    }
    // Decisions use a stable snapshot before any front's desired anchor moves.
    plans.extend(formations.fronts.iter().map(|(&id, f)| {
        let target = if let Intent::Attack(enemy) = f.intent {
            space.footprint(enemy, &formations)
        } else {
            None
        };
        (id, target)
    }));
    // Allocate the closest approach first. Entity/command order must not let
    // a far wing steal the front from the centre and send the other wing on
    // a needlessly long trip around the rear.
    let approach_cost = |(id, target): &(u64, Option<Footprint>)| {
        target.map_or(f32::INFINITY, |shape| {
            let from = formations.fronts[id].footprint().centre;
            Face::ALL
                .into_iter()
                .map(|face| from.distance_squared(shape.contact(face)))
                .min_by(f32::total_cmp)
                .unwrap()
        })
    };
    plans.sort_by(|a, b| {
        approach_cost(a)
            .total_cmp(&approach_cost(b))
            .then(a.0.cmp(&b.0))
    });
    for &(id, target) in plans.iter() {
        let front = formations.fronts.get_mut(&id).unwrap();
        let members = front.columns.iter().flatten();
        let marching = members
            .clone()
            .any(|e| units.get(*e).is_ok_and(|(_, _, _, _, march, ..)| march));
        if let Intent::March { engage } = front.intent {
            if !marching {
                front.intent = Intent::Hold;
                front.march_paused = false;
            } else if front.march_paused {
                let hostile_near = members
                    .clone()
                    .filter_map(|e| space.body(*e))
                    .any(|b| space.nearby(b.point).any(|other| other.side != b.side));
                if !hostile_near {
                    front.active = false;
                    front.march_paused = false;
                    front.anchor = front.march_anchor;
                    for e in members {
                        commands.entity(*e).remove::<(
                            CombatReady,
                            PausedFormationMarch,
                            AttackOrder,
                            EngagedWith,
                        )>();
                    }
                    continue;
                }
            } else {
                if !engage {
                    continue;
                }
                let contact = members.clone().filter_map(|e| space.body(*e)).any(|b| {
                    space.nearby(b.point).any(|other| {
                        other.side != b.side && b.point.distance_squared(other.point) < 3.0 * 3.0
                    })
                });
                if !contact {
                    continue;
                }
                let mut sum = Vec2::ZERO;
                let mut count = 0;
                for (rank, e) in front.columns.iter().flat_map(|c| c.iter().enumerate()) {
                    if let Some(b) = space.body(*e) {
                        sum += b.point + front.facing * rank as f32 * RANK_SPACING;
                        count += 1;
                    }
                    commands.entity(*e).insert(PausedFormationMarch).remove::<(
                        MoveTarget,
                        TravelRoute,
                        NavigationRoutePending,
                        NavigationRouteFailed,
                    )>();
                }
                front.anchor = sum / count.max(1) as f32;
                front.active = true;
                front.march_paused = true;
            }
        }
        if let Intent::Attack(enemy) = front.intent {
            if let Some(enemy_shape) = target {
                if front.sector.is_none() {
                    let from = front.footprint().centre;
                    let chosen = Face::ALL
                        .into_iter()
                        .filter(|face| match enemy {
                            Enemy::Battalion(b) => !reservations.contains(&(b, *face)),
                            _ => true,
                        })
                        .min_by(|a, b| {
                            from.distance_squared(enemy_shape.contact(*a))
                                .total_cmp(&from.distance_squared(enemy_shape.contact(*b)))
                        });
                    if let Some(face) = chosen {
                        front.sector = Some(face);
                        if let Enemy::Battalion(b) = enemy {
                            reservations.push((b, face));
                        }
                        // A flank has less frontage than the enemy's front.
                        // Re-form once during approach, never on every death.
                        let files = if matches!(enemy, Enemy::Battalion(_)) {
                            (1 + (enemy_shape.half_length(face) * 2.0 / FILE_SPACING).floor()
                                as usize)
                                .clamp(1, front.columns.len())
                        } else {
                            front.columns.len()
                        };
                        let facing = -enemy_shape.normal(face);
                        if files != front.columns.len() || facing.dot(front.facing) < 0.99 {
                            let right = Vec2::new(facing.y, -facing.x);
                            let mut roster: Vec<_> =
                                front.columns.iter().flatten().copied().collect();
                            roster.sort_by(|a, b| {
                                let a = space.body(*a).unwrap();
                                let b = space.body(*b).unwrap();
                                b.point
                                    .dot(facing)
                                    .total_cmp(&a.point.dot(facing))
                                    .then(a.entity.cmp(&b.entity))
                            });
                            front.columns = vec![Vec::new(); files];
                            for row in roster.chunks_mut(files) {
                                row.sort_by(|a, b| {
                                    space
                                        .body(*a)
                                        .unwrap()
                                        .point
                                        .dot(right)
                                        .total_cmp(&space.body(*b).unwrap().point.dot(right))
                                        .then(a.cmp(b))
                                });
                                for (file, e) in row.iter().enumerate() {
                                    front.columns[file].push(*e);
                                }
                            }
                            front.facing = facing;
                        }
                    } else {
                        continue;
                    } // All four faces occupied: remain a reserve.
                }
                let face = front.sector.unwrap();
                let actual = front
                    .columns
                    .iter()
                    .flatten()
                    .filter_map(|e| space.body(*e))
                    .map(|b| b.point)
                    .sum::<Vec2>()
                    / front.columns.iter().map(Vec::len).sum::<usize>().max(1) as f32;
                let half = (front.columns.len() - 1) as f32 * FILE_SPACING * 0.5;
                if let Some(staging) = enemy_shape.staging_point(face, actual, half) {
                    front.anchor = staging;
                    front.staging = true;
                    // Face the enemy only after reaching the outside corridor.
                } else {
                    front.anchor = enemy_shape.contact(face);
                    front.facing = -enemy_shape.normal(face);
                    front.staging = false;
                }
            } else {
                front.intent = Intent::Hold;
                front.sector = None;
                // Keep the last occupied front; do not chase a despawned person.
            }
        }
        if !front.active {
            front.active = front
                .columns
                .iter()
                .flatten()
                .filter_map(|e| space.body(*e))
                .any(|b| space.nearby(b.point).any(|o| o.side != b.side));
        }
        if !front.active {
            continue;
        }
        let mut route_group = None;
        for (entity, home, exposed) in front.posts() {
            if front.intent == Intent::Hold
                && policies
                    .get(entity)
                    .is_ok_and(|policy| *policy == BattalionStance::HoldLine)
            {
                commands.entity(entity).remove::<MoveTarget>();
                if let Ok((_, _, _, _, _, fighting, _, mut motion, mut activity)) =
                    units.get_mut(entity)
                {
                    motion.set_if_neq(CharacterMotion::STATIONARY);
                    if !fighting {
                        activity.set_if_neq(CharacterActivity::Idle);
                    }
                }
                continue;
            }
            // The rank is a home position, not a rail. Only exposed soldiers
            // step out to meet a local threat; supporting ranks retain space
            // behind them. A lone flanker never turns the whole battalion.
            let mut slot = home;
            if exposed && !front.staging {
                if let Some(body) = space.body(entity) {
                    if body.point.distance_squared(home) < 3.0 * 3.0 {
                        if let Some(enemy) = space
                            .nearby(body.point)
                            .filter(|e| {
                                e.side != body.side && e.point.distance_squared(home) < 3.1 * 3.1
                            })
                            .min_by(|a, b| {
                                a.point
                                    .distance_squared(body.point)
                                    .total_cmp(&b.point.distance_squared(body.point))
                            })
                        {
                            let outward = (enemy.point - home).normalize_or_zero();
                            let approach = enemy.point - outward * geometry::CONTACT_DISTANCE;
                            slot = home + (approach - home).clamp_length_max(1.1);
                        }
                    }
                }
            }
            let Ok((
                _,
                _,
                position,
                target,
                march,
                fighting,
                mut rotation,
                mut motion,
                mut activity,
            )) = units.get_mut(entity)
            else {
                continue;
            };
            if (march && !front.march_paused) || fighting {
                continue;
            }
            let distance = position.0.xz().distance(slot);
            if distance > 0.18 {
                let clear = navigation_segment_clear(
                    position.0.xz(),
                    slot,
                    buildings.as_deref(),
                    colliders.as_deref(),
                    derived.as_deref(),
                ) && terrain.as_deref().is_none_or(|t| {
                    crate::player::hero::terrain_segment_walkable(t, position.0.xz(), slot)
                });
                if !clear && !front.march_paused {
                    if let Some(routes) = routes.as_mut() {
                        // All obstructed files share one bounded reverse field.
                        // The march system owns certification and retries; keep
                        // the battle intent so local contact can interrupt it.
                        let group = *route_group.get_or_insert_with(|| {
                            routes.register(
                                front
                                    .columns
                                    .iter()
                                    .flatten()
                                    .filter_map(|e| space.body(*e))
                                    .map(|b| b.point)
                                    .collect(),
                                front.anchor,
                            )
                        });
                        let destination = Vec3::new(
                            slot.x,
                            terrain
                                .as_deref()
                                .map_or(position.0.y, |t| t.get_height(slot.x, slot.y)),
                            slot.y,
                        );
                        commands
                            .entity(entity)
                            .insert(MarchOrder {
                                destination,
                                facing: front.facing,
                                group,
                            })
                            .remove::<(
                                MoveTarget,
                                TravelRoute,
                                NavigationRoutePending,
                                NavigationRouteFailed,
                            )>();
                    }
                }
                if clear && target.is_none_or(|t| t.0.xz().distance_squared(slot) > 0.04) {
                    let point = Vec3::new(
                        slot.x,
                        terrain
                            .as_deref()
                            .map_or(position.0.y, |t| t.get_height(slot.x, slot.y)),
                        slot.y,
                    );
                    commands.entity(entity).insert(MoveTarget(point)).remove::<(
                        TravelRoute,
                        NavigationRoutePending,
                        NavigationRouteFailed,
                    )>();
                }
            } else {
                if target.is_some() {
                    commands.entity(entity).remove::<MoveTarget>();
                }
                rotation.set_if_neq(PlayerRotation(f32::atan2(-front.facing.x, -front.facing.y)));
                motion.set_if_neq(CharacterMotion::STATIONARY);
                activity.set_if_neq(CharacterActivity::Idle);
            }
            commands.entity(entity).insert_if_new(CommandStance::Guard);
        }
    }
}
