use super::*;
use crate::player::{
    combat::AttackOrder,
    hero::MoveTarget,
    orders::{CommandStance, FormationRoutes, MarchOrder},
};
use crate::world::village_roads::{NavigationRouteFailed, NavigationRoutePending, TravelRoute};
#[derive(Default)]
pub struct PlanningScratch {
    steering: std::collections::HashMap<(u64, Entity), steering::Steering>,
    plans: Vec<(u64, Option<Footprint>)>,
}

/// Slow formation decisions, cheap continuous movement through the existing
/// authoritative mover. File queues survive losses; only the affected file
/// advances. Individual control requires an explicit membership removal.
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
    bows: Query<(), With<BowEquipped>>,
    targets: Query<(&PlayerPosition, &Health)>,
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
        Has<CommandStance>,
    )>,
) {
    let Some(clock) = clock.iter().next() else {
        return;
    };
    let now = super::super::world_clock_seconds(clock);
    if now >= *last && now - *last < 0.1 {
        return;
    }
    let dt = (now - *last).clamp(0.0, 0.25) as f32;
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
    let PlanningScratch { steering, plans } = &mut *scratch;
    steering.retain(|(id, entity), _| {
        formations.fronts.contains_key(id)
            && units.get(*entity).is_ok_and(|(m, ..)| m.group == *id)
            && space.body(*entity).is_some()
    });
    plans.clear();
    // One enemy outline per battalion; local choices use the shared body grid.
    plans.extend(formations.fronts.iter().map(|(&id, f)| {
        (
            id,
            if let Intent::Attack(enemy) = f.intent {
                space.footprint(enemy, &formations).or_else(|| {
                    let Enemy::Person(target) = enemy else {
                        return None;
                    };
                    targets
                        .get(target)
                        .ok()
                        .filter(|(_, h)| !h.is_dead())
                        .map(|(p, _)| Footprint {
                            centre: p.0.xz(),
                            facing: Vec2::Y,
                            half_width: 0.5,
                            half_depth: 0.5,
                        })
                })
            } else {
                None
            },
        )
    }));
    for &(id, target) in plans.iter() {
        let front = formations.fronts.get_mut(&id).unwrap();
        let members = front.columns.iter().flatten();
        let ranged = members.clone().any(|e| bows.contains(*e));
        let marching = members
            .clone()
            .any(|e| units.get(*e).is_ok_and(|(_, _, _, _, march, ..)| march));
        if let Intent::March { engage } = front.intent {
            if !marching {
                front.intent = Intent::Hold;
                front.march_paused = false;
            } else if front.march_paused {
                let hostile_near = members.clone().filter_map(|e| space.body(*e)).any(|b| {
                    space
                        .within(b.point, if ranged { BOW_RANGE } else { 3. })
                        .any(|other| {
                            other.side != b.side
                                && other.point.distance_squared(b.point)
                                    < if ranged { BOW_RANGE * BOW_RANGE } else { 9. }
                        })
                });
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
                    space
                        .within(b.point, if ranged { BOW_ADVANCE_RANGE } else { 3. })
                        .any(|other| {
                            other.side != b.side
                                && b.point.distance_squared(other.point)
                                    < if ranged {
                                        BOW_ADVANCE_RANGE * BOW_ADVANCE_RANGE
                                    } else {
                                        9.
                                    }
                        })
                });
                if !contact {
                    continue;
                }
                let occupied = front.occupied_anchor(&space);
                for e in front.columns.iter().flatten() {
                    commands.entity(*e).insert(PausedFormationMarch).remove::<(
                        MoveTarget,
                        TravelRoute,
                        NavigationRoutePending,
                        NavigationRouteFailed,
                    )>();
                }
                front.anchor = occupied;
                front.active = true;
                front.march_paused = true;
            }
        }
        let threatened = front
            .columns
            .iter()
            .flatten()
            .filter_map(|e| space.body(*e))
            .any(|b| {
                space
                    .within(b.point, 6.0)
                    .any(|o| o.side != b.side && o.point.distance_squared(b.point) < 36.0)
            });
        if threatened {
            front.last_threat = now;
        }
        let was_active = front.active;
        front.active = threatened
            || now - front.last_threat < 2.0
            || matches!(front.intent, Intent::Attack(_));
        if matches!(front.intent, Intent::Attack(_)) {
            if let Some(shape) = target {
                let actual = front.occupied_anchor(&space);
                if !threatened && !(ranged && front.ranged_deployed) {
                    let facing = (shape.centre - actual)
                        .try_normalize()
                        .unwrap_or(front.facing);
                    let turn = front.facing.angle_to(facing).clamp(-dt * 1.5, dt * 1.5);
                    front.facing = Mat2::from_angle(turn) * front.facing;
                }
                if ranged {
                    let edge = shape.approach(actual);
                    let distance = actual.distance(edge);
                    if distance > BOW_RANGE - 5. {
                        front.ranged_deployed = false;
                    }
                    if !front.ranged_deployed {
                        if distance > BOW_ADVANCE_RANGE + 3. {
                            front.anchor =
                                edge + (actual - edge).normalize_or_zero() * BOW_ADVANCE_RANGE;
                        } else {
                            front.anchor = actual;
                            front.ranged_deployed = true;
                        }
                    }
                } else {
                    front.ranged_deployed = false;
                    front.anchor = shape.approach(actual);
                }
            } else {
                front.intent = Intent::Hold;
                front.active = threatened || now - front.last_threat < 2.0;
            }
        }
        if was_active && !front.active {
            // Re-form around the ground actually occupied, not the pre-battle
            // anchor. Never send survivors marching back through the battlefield.
            front.anchor = front.occupied_anchor(&space);
        }
        let depth = front
            .columns
            .iter()
            .map(Vec::len)
            .max()
            .unwrap_or(1)
            .saturating_sub(1) as f32
            * front.rank_spacing;
        let defended_ground = Footprint {
            centre: front.anchor - front.facing * depth * 0.5,
            facing: front.facing,
            half_width: front.columns.len().saturating_sub(1) as f32 * front.spacing * 0.5,
            half_depth: depth * 0.5,
        };
        let mut route_group = None;
        for (entity, home, ahead) in front.posts() {
            if front.intent == Intent::Hold
                && policies
                    .get(entity)
                    .is_ok_and(|policy| *policy == BattalionStance::HoldLine)
            {
                commands.entity(entity).remove::<MoveTarget>();
                if let Ok((_, _, _, _, _, fighting, _, mut motion, mut activity, _)) =
                    units.get_mut(entity)
                {
                    motion.set_if_neq(CharacterMotion::STATIONARY);
                    if !fighting {
                        activity.set_if_neq(CharacterActivity::Idle);
                    }
                }
                continue;
            }
            if units
                .get(entity)
                .is_ok_and(|(_, _, _, _, march, fighting, ..)| {
                    (march && !front.march_paused) || fighting
                })
            {
                continue;
            }
            let Some(body) = space.body(entity) else {
                continue;
            };
            let approach = steering.entry((id, entity)).or_default();
            let bow = bows.contains(entity);
            let local = if front.active && !bow {
                approach.local_goal(&space, entity, now)
            } else {
                approach.released = false;
                None
            };
            let loose = !bow && (matches!(front.intent, Intent::Attack(_)) || local.is_some());
            let mut keep_file = local.is_none();
            let mut slot = home;
            if let Some(goal) = local {
                // Release reserves when contact is close or an approach opens.
                // Otherwise follow the actual soldier ahead: a blocked rear
                // rank must not fan out merely to overtake its own moving file.
                let leader = ahead.and_then(|e| space.body(e));
                approach.released = approach.released
                    || leader.is_none()
                    || body.point.distance_squared(goal) <= 2.5 * 2.5
                    || space.approach_clear(entity, body.point, goal);
                if let Some(leader) = leader.filter(|_| !approach.released) {
                    slot = leader.point - front.facing * front.rank_spacing;
                    // A reserve follows meaningful progress, not every tiny
                    // sideways correction made by the person ahead.
                    if slot.distance_squared(body.point) < 0.55 * 0.55 {
                        slot = body.point;
                    }
                    keep_file = true;
                } else {
                    slot = goal;
                }
                if front.intent == Intent::Hold {
                    // Defend the whole battalion's ground, not each home slot.
                    slot = defended_ground.clamp(slot, 6.0);
                }
            } else if let Some(shape) = target.filter(|_| approach.released) {
                slot = shape.approach(body.point);
                keep_file = false;
            } else if target.is_none() && front.active && !bow {
                // A supporting rank without a reachable local threat waits
                // where it is, rather than tugging towards an occupied slot.
                slot = body.point;
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
                has_stance,
            )) = units.get_mut(entity)
            else {
                continue;
            };
            if (march && !front.march_paused) || fighting {
                continue;
            }
            let distance = position.0.xz().distance(slot);
            if distance > 0.18 {
                let clear = crate::player::siege::ground_clear(
                    position.0.xz(),
                    slot,
                    front.clearance,
                    terrain.as_deref(),
                    buildings.as_deref(),
                    colliders.as_deref(),
                    derived.as_deref(),
                );
                if !clear && !front.march_paused {
                    if let Some(routes) = routes.as_mut() {
                        // All obstructed files share one bounded reverse field.
                        // The march system owns certification and retries; keep
                        // the battle intent so local contact can interrupt it.
                        let group = *route_group.get_or_insert_with(|| {
                            routes.register_with_clearance(
                                front
                                    .columns
                                    .iter()
                                    .flatten()
                                    .filter_map(|e| space.body(*e))
                                    .map(|b| b.point)
                                    .collect(),
                                front.anchor,
                                front.clearance,
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
                if clear && loose {
                    let next = steering.entry((id, entity)).or_default().step(
                        &space,
                        entity,
                        position.0.xz(),
                        slot,
                        now,
                        keep_file,
                        |end| {
                            crate::player::siege::ground_clear(
                                position.0.xz(),
                                end,
                                front.clearance,
                                terrain.as_deref(),
                                buildings.as_deref(),
                                colliders.as_deref(),
                                derived.as_deref(),
                            )
                        },
                    );
                    let Some(next) = next else {
                        if target.is_some() {
                            commands.entity(entity).remove::<MoveTarget>();
                        }
                        motion.set_if_neq(CharacterMotion::STATIONARY);
                        activity.set_if_neq(CharacterActivity::Idle);
                        continue;
                    };
                    slot = next;
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
                if !front.active {
                    rotation
                        .set_if_neq(PlayerRotation(f32::atan2(-front.facing.x, -front.facing.y)));
                }
                motion.set_if_neq(CharacterMotion::STATIONARY);
                activity.set_if_neq(CharacterActivity::Idle);
            }
            if !has_stance {
                commands.entity(entity).insert(CommandStance::Guard);
            }
        }
    }
}
