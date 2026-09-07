//! Local body separation with reusable scratch buffers.
use super::*;
use std::collections::HashMap;
#[derive(Default)]
pub struct SeparationScratch {
    participants: Vec<(Entity, Vec2, bool, bool)>,
    cells: HashMap<(i32, i32), Vec<usize>>,
    pushes: HashMap<Entity, Vec2>,
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
    const CELL: f32 = 2.0;
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
    for (entity, _, position, _, war_party, commanded, health) in bodies.iter() {
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
        ));
    }
    if participants.len() < 2 {
        return;
    }
    for (index, (_, point, _, _)) in participants.iter().enumerate() {
        cells
            .entry((
                (point.x / CELL).floor() as i32,
                (point.y / CELL).floor() as i32,
            ))
            .or_default()
            .push(index);
    }

    let min_distance = BODY_RADIUS * 2.0;
    cells.retain(|_, indices| !indices.is_empty());
    for (index, (entity, point, fixed, engaged)) in participants.iter().enumerate() {
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
                    let (other, other_point, other_fixed, other_engaged) =
                        participants[*other_index];
                    // Arriving allies yield to an established fight. Two
                    // fighters still resolve overlap symmetrically.
                    let fixed = *fixed || (*engaged && !other_engaged && !other_fixed);
                    let other_fixed = other_fixed || (other_engaged && !*engaged && !fixed);
                    if fixed && other_fixed {
                        continue;
                    }
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

    for (entity, kind, mut position, mut region, _, _, _) in bodies.iter_mut() {
        let Some(push) = pushes.get(&entity) else {
            continue;
        };
        let push = push.clamp_length_max(MAX_PUSH_PER_TICK);
        if push.length_squared() < 1.0e-8 {
            continue;
        }
        let current = Vec2::new(position.0.x, position.0.z);
        let next = current + push;
        // A shove must not put a villager inside a wall - that would hand
        // them to the route-failure machinery mid-fight. Heroes are as
        // collision-exempt here as they are in step_units.
        if *kind == CharacterKind::Villager
            && !crate::player::hero::navigation_segment_clear(
                current,
                next,
                obstacles.as_deref(),
                colliders.as_deref(),
                derived.as_deref(),
            )
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
