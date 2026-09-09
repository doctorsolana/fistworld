//! Local body separation with reusable scratch buffers.
use super::*;
use std::collections::HashMap;
#[derive(Default)]
pub struct SeparationScratch {
    participants: Vec<(Entity, Vec2, bool, bool, f32)>,
    cells: HashMap<(i32, i32), Vec<usize>>,
    pushes: HashMap<Entity, Vec2>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn heroes_cannot_be_shoved_through_defenses_but_gate_and_building_rules_survive() {
        use shared::components::{
            FortificationKind, FortificationMaterial, FortificationSegment, SettlementId,
        };
        let mut app = App::new();
        let mut grid = shared::spatial::SpatialObstacleGrid::default();
        for (start_z, end_z) in [(-4.0, 4.0), (12.0, 16.0), (24.0, 28.0)] {
            let wall = FortificationSegment {
                settlement_id: SettlementId(1),
                circuit: 0,
                start: Vec3::new(0.0, 0.0, start_z),
                end: Vec3::new(0.0, 0.0, end_z),
                kind: FortificationKind::Wall,
                material: FortificationMaterial::Palisade,
                complete: true,
            };
            grid.insert(wall.navigation_obstacle().unwrap());
        }
        grid.insert(shared::spatial::ObstacleEntry {
            center: Vec2::new(0.0, -20.0),
            half_extents: Vec2::new(0.5, 4.0),
            rotation: 0.0,
            obstacle_type: 0,
        });
        app.insert_resource(grid);
        app.add_systems(Update, separate_melee_bodies);
        let mut heroes = Vec::new();
        for z in [0.0, 20.0, -20.0] {
            for (x, hero) in [(-0.55, true), (-0.65, false)] {
                let entity = app
                    .world_mut()
                    .spawn((
                        if hero {
                            CharacterKind::Hero
                        } else {
                            CharacterKind::Villager
                        },
                        PlayerPosition(Vec3::new(x, 0.0, z)),
                        shared::region::RegionCoord::default(),
                        CommandedBy("test".into()),
                    ))
                    .id();
                if hero {
                    heroes.push(entity);
                }
            }
        }
        app.update();
        assert_eq!(
            app.world().get::<PlayerPosition>(heroes[0]).unwrap().0.x,
            -0.55
        );
        assert!(app.world().get::<PlayerPosition>(heroes[1]).unwrap().0.x > -0.55);
        assert!(app.world().get::<PlayerPosition>(heroes[2]).unwrap().0.x > -0.55);
    }
}

/// Melee bodies never overlap: any two combatants closer than two body radii
/// are pushed apart, half each, capped per tick. This is what makes a fight
/// a FRONT LINE - the second rank physically cannot occupy the first rank's
/// ground, so it holds behind or slides around the flanks - and it is scoped
/// to combatants so the tuned civilian flows (queues, doorways, markets) are
/// never disturbed.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn separate_melee_bodies(
    terrain: Option<Res<shared::terrain::WorldTerrain>>,
    obstacles: Option<Res<shared::spatial::SpatialObstacleGrid>>,
    colliders: Option<Res<crate::collision::library::StaticColliders>>,
    derived: Option<Res<crate::collision::library::DerivedColliderLibrary>>,
    mut scratch: Local<SeparationScratch>,
    policies: Query<(
        Option<&shared::components::BattalionStance>,
        Has<crate::player::army::DirectedAttack>,
        Has<crate::player::orders::MarchOrder>,
        Has<super::SkirmishOrder>,
        Option<&super::fronts::FormationMember>,
        Has<shared::components::EngagedWith>,
    )>,
    formations: Option<Res<super::fronts::CombatFormations>>,
    mut bodies: Query<
        (
            Entity,
            &CharacterKind,
            &mut PlayerPosition,
            &mut shared::region::RegionCoord,
            Option<&WarParty>,
            Option<&CommandedBy>,
            Option<&Health>,
            Has<shared::components::Mounted>,
        ),
        (
            With<CharacterKind>,
            Without<OfflineHero>,
            Without<AboardBoat>,
            Without<crate::world::village::strategic::StrategicPerson>,
        ),
    >,
) {
    // Combatants only, hashed into coarse cells so the pair pass is local.
    const CELL: f32 = 3.2;
    let SeparationScratch {
        participants,
        cells,
        pushes,
    } = &mut *scratch;
    participants.clear();
    for indices in cells.values_mut() {
        indices.clear();
    }
    pushes.clear();
    for (entity, _, position, _, war_party, commanded, health, mounted) in bodies.iter() {
        if health.is_some_and(|h| h.is_dead()) || (war_party.is_none() && commanded.is_none()) {
            continue;
        }
        let fixed =
            policies
                .get(entity)
                .is_ok_and(|(policy, directed, marching, skirmish, member, _)| {
                    policy == Some(&shared::components::BattalionStance::HoldLine)
                        && !directed
                        && !marching
                        && !skirmish
                        && !member.is_some_and(|m| {
                            formations
                                .as_ref()
                                .and_then(|f| f.fronts.get(&m.group))
                                .is_some_and(|f| {
                                    matches!(f.intent, super::fronts::Intent::Attack(_))
                                })
                        })
                });
        let engaged = policies
            .get(entity)
            .is_ok_and(|(_, _, _, _, _, engaged)| engaged);
        participants.push((
            entity,
            Vec2::new(position.0.x, position.0.z),
            fixed,
            engaged,
            if mounted {
                shared::components::HORSE_BODY_RADIUS
            } else {
                BODY_RADIUS
            },
        ));
    }
    if participants.len() < 2 {
        return;
    }
    for (index, (_, point, _, _, _)) in participants.iter().enumerate() {
        cells
            .entry((
                (point.x / CELL).floor() as i32,
                (point.y / CELL).floor() as i32,
            ))
            .or_default()
            .push(index);
    }

    cells.retain(|_, indices| !indices.is_empty());
    for (index, (entity, point, fixed, engaged, radius)) in participants.iter().enumerate() {
        let cell = (
            (point.x / CELL).floor() as i32,
            (point.y / CELL).floor() as i32,
        );
        for dx in -1..=1 {
            for dz in -1..=1 {
                let Some(neighbors) = cells.get(&(cell.0 + dx, cell.1 + dz)) else {
                    continue;
                };
                for other_index in neighbors {
                    // Each pair once.
                    if *other_index <= index {
                        continue;
                    }
                    let (other, other_point, other_fixed, other_engaged, other_radius) =
                        participants[*other_index];
                    // Arriving allies yield to an established fight. Two
                    // fighters still resolve overlap symmetrically.
                    let fixed = *fixed || (*engaged && !other_engaged && !other_fixed);
                    let other_fixed = other_fixed || (other_engaged && !*engaged && !fixed);
                    if fixed && other_fixed {
                        continue;
                    }
                    let min_distance = radius + other_radius;
                    let offset = *point - other_point;
                    let distance = offset.length();
                    if distance >= min_distance - SEPARATION_SLACK {
                        continue;
                    }
                    // Exactly coincident bodies fan out along a direction
                    // derived from the pair, deterministic across both ends.
                    let axis = if distance > 1.0e-4 {
                        offset / distance
                    } else {
                        let a = entity.to_bits().min(other.to_bits());
                        let b = entity.to_bits().max(other.to_bits());
                        let mixed = (a ^ (b.rotate_left(17))).wrapping_mul(0x9E37_79B9_7F4A_7C15);
                        let angle = (mixed as u32) as f32 / u32::MAX as f32 * std::f32::consts::TAU;
                        Vec2::new(angle.cos(), angle.sin())
                    };
                    let correction = ((min_distance - distance) * 0.5).min(MAX_PUSH_PER_TICK);
                    if !fixed {
                        *pushes.entry(*entity).or_default() +=
                            axis * correction * if other_fixed { 2.0 } else { 1.0 };
                    }
                    if !other_fixed {
                        *pushes.entry(other).or_default() -=
                            axis * correction * if fixed { 2.0 } else { 1.0 };
                    }
                }
            }
        }
    }
    if pushes.is_empty() {
        return;
    }

    for (entity, kind, mut position, mut region, _, _, _, mounted) in bodies.iter_mut() {
        let Some(push) = pushes.get(&entity) else {
            continue;
        };
        let push = push.clamp_length_max(MAX_PUSH_PER_TICK);
        if push.length_squared() < 1.0e-8 {
            continue;
        }
        let current = Vec2::new(position.0.x, position.0.z);
        let next = current + push;
        // Heroes retain their ordinary-building exception. Defenses are solid
        // for every combatant, including displacement by neighboring bodies.
        let defense_blocked = obstacles.as_deref().is_some_and(|grid| {
            grid.segment_blocked_by_type(current, next, shared::components::DEFENSE_OBSTACLE_TYPE)
        });
        if (mounted
            && !crate::player::siege::ground_clear(
                current,
                next,
                shared::components::HORSE_CLEARANCE,
                terrain.as_deref(),
                obstacles.as_deref(),
                colliders.as_deref(),
                derived.as_deref(),
            ))
            || defense_blocked
            || (*kind == CharacterKind::Villager
                && !crate::player::hero::navigation_segment_clear(
                    current,
                    next,
                    obstacles.as_deref(),
                    colliders.as_deref(),
                    derived.as_deref(),
                ))
        {
            continue;
        }
        let y = terrain
            .as_deref()
            .map(|terrain| terrain.get_height(next.x, next.y))
            .unwrap_or(position.0.y);
        let next_position = Vec3::new(next.x, y, next.y);
        if position.0 != next_position {
            position.0 = next_position;
        }
        let next_region = shared::region::RegionCoord::from_world_pos(next_position);
        if *region != next_region {
            *region = next_region;
        }
    }
}
