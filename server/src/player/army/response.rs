//! Standing policies and impact-driven defensive repositioning. No projectile
//! prediction: a nearby impact alerts idle troops, who keep their new ground.
use crate::player::{
    combat::{fronts, AttackOrder, SkirmishOrder},
    hero::MoveTarget,
    orders::{CommandStance, MarchOrder},
};
use crate::world::village_roads::{NavigationRouteFailed, NavigationRoutePending, TravelRoute};
use bevy::prelude::*;
use shared::{components::*, formation::FormationSoldier, protocol::*};
use std::collections::BTreeMap;

#[derive(Component)]
pub(crate) struct DirectedAttack;
#[derive(Component)]
pub(crate) struct UnansweredBombardment;
#[derive(Component)]
pub struct EvadingBombardment;
#[derive(Component)]
struct BombardmentCooldown(f64);

fn directed(world: &World, entity: Entity) -> bool {
    world.get::<DirectedAttack>(entity).is_some()
        || world.get::<SkirmishOrder>(entity).is_some()
        || (world.get::<MarchOrder>(entity).is_some()
            && world.get::<EvadingBombardment>(entity).is_none())
        || world
            .get::<fronts::FormationMember>(entity)
            .is_some_and(|member| {
                world
                    .get_resource::<fronts::CombatFormations>()
                    .and_then(|f| f.fronts.get(&member.group))
                    .is_some_and(|f| matches!(f.intent, fronts::Intent::Attack(_)))
            })
}

pub(super) fn apply_policy(world: &mut World, entity: Entity, stance: BattalionStance) {
    if world.get::<BattalionStance>(entity) == Some(&stance) {
        return;
    }
    world.entity_mut(entity).insert(stance);
    if directed(world, entity) {
        return;
    }
    // Changing standing policy cancels a previous automatic response / chase.
    // Direct move and attack objectives have already returned above.
    world
        .entity_mut(entity)
        .remove::<(
            EvadingBombardment,
            MarchOrder,
            MoveTarget,
            TravelRoute,
            NavigationRoutePending,
            NavigationRouteFailed,
            AttackOrder,
            EngagedWith,
            CombatSwing,
            fronts::FormationMember,
            fronts::PausedFormationMarch,
        )>()
        .insert(CommandStance::Guard);
    if let Some(mut motion) = world.get_mut::<CharacterMotion>(entity) {
        motion.set_if_neq(CharacterMotion::STATIONARY);
    }
}

pub(crate) fn set_stance(
    world: &mut World,
    battalion: Entity,
    id: BattalionId,
    stance: BattalionStance,
) -> usize {
    if world.get::<BattalionStance>(battalion) != Some(&stance) {
        world.entity_mut(battalion).insert(stance);
    }
    let members: Vec<_> = world
        .query::<(Entity, &MemberOfBattalion)>()
        .iter(world)
        .filter(|(_, m)| m.0 == id)
        .map(|(e, _)| e)
        .collect();
    for &entity in &members {
        apply_policy(world, entity, stance);
    }
    members.len()
}

/// One bounded roster pass only when an impact arrives. Whole battalions move
/// together; any active member objective prevents an unsolicited split.
pub fn react_to_bombardment(world: &mut World) {
    let impacts: Vec<_> = world
        .query_filtered::<(Entity, &SiegeImpact), With<UnansweredBombardment>>()
        .iter(world)
        .map(|(e, i)| (e, *i))
        .collect();
    if impacts.is_empty() {
        return;
    }
    for &(entity, _) in &impacts {
        world.entity_mut(entity).remove::<UnansweredBombardment>();
    }
    let mut groups: BTreeMap<(String, Option<BattalionId>, u64), Vec<Entity>> = BTreeMap::new();
    for (entity, owner, membership, health) in world
        .query_filtered::<(Entity, &CommandedBy, Option<&MemberOfBattalion>, &Health), (
            With<CharacterKind>,
            Without<crate::player::hero::OfflineHero>,
            Without<AboardBoat>,
        )>()
        .iter(world)
    {
        if health.is_dead() {
            continue;
        }
        let id = membership.map(|m| m.0);
        groups
            .entry((
                owner.0.clone(),
                id,
                if id.is_some() { 0 } else { entity.to_bits() },
            ))
            .or_default()
            .push(entity);
    }
    for ((account, id, _), mut units) in groups {
        units.sort_by_key(|e| world.get::<PersonId>(*e).map_or(e.to_bits(), |id| id.0));
        if units.iter().any(|&e| {
            directed(world, e)
                || world.get::<BattalionStance>(e) == Some(&BattalionStance::HoldLine)
                || world.get::<AttackOrder>(e).is_some()
                || (world.get::<MoveTarget>(e).is_some()
                    && world.get::<EvadingBombardment>(e).is_none())
        }) {
            continue;
        }
        let mut positions: Vec<_> = units
            .iter()
            .filter_map(|&e| world.get::<PlayerPosition>(e).map(|p| (e, p.0)))
            .collect();
        if positions.is_empty() {
            continue;
        }
        let Some((_, impact)) = impacts.iter().find(|(_, i)| {
            positions.iter().any(|(_, p)| {
                p.xz().distance_squared(i.position.xz()) < (CATAPULT_BLAST_RADIUS + 4.0).powi(2)
            }) && units.iter().all(|e| {
                world
                    .get::<BombardmentCooldown>(*e)
                    .is_none_or(|c| i.at - c.0 >= 6.0)
            })
        }) else {
            continue;
        };
        // A roster transfer need not mean physical reunion yet. Keep an idle
        // detachment local instead of pulling distant members across the map.
        positions.retain(|(_, p)| p.xz().distance_squared(impact.position.xz()) < 24.0 * 24.0);
        units.retain(|e| positions.iter().any(|(local, _)| local == e));
        let soldiers: Vec<_> = positions
            .iter()
            .map(|(entity, position)| FormationSoldier {
                entity: *entity,
                position: *position,
                seat: None,
                identity: world
                    .get::<PersonId>(*entity)
                    .map_or(entity.to_bits(), |id| id.0),
            })
            .collect();
        let block = fronts::current_block(
            world,
            id.map_or(units[0].to_bits(), |id| id.0),
            &soldiers,
            default(),
        );
        let centre = shared::formation::centre(positions.iter().map(|(_, p)| *p));
        let away = (centre - impact.position)
            .xz()
            .try_normalize()
            .unwrap_or_else(|| {
                let angle = (impact.seed % 8) as f32 * std::f32::consts::FRAC_PI_4;
                Vec2::new(angle.cos(), angle.sin())
            });
        let offset = [
            0.0_f32,
            0.8,
            -0.8,
            1.6,
            -1.6,
            2.4,
            -2.4,
            std::f32::consts::PI,
        ]
        .into_iter()
        .map(|a| {
            Vec2::new(
                away.x * a.cos() - away.y * a.sin(),
                away.x * a.sin() + away.y * a.cos(),
            ) * 14.0
        })
        .find(|offset| {
            positions.iter().all(|(_, p)| {
                let to = p.xz() + *offset;
                escape_clear(world, p.xz(), to)
                    && world
                        .get_resource::<fronts::CombatSpace>()
                        .is_none_or(|space| {
                            !space.nearby(to).any(|other| {
                                !units.contains(&other.entity)
                                    && other.point.distance_squared(to) < 1.0
                            })
                        })
            })
        });
        let Some(offset) = offset else {
            continue;
        };
        let order = UnitOrder {
            selection: UnitSelection {
                units: units.clone(),
                battalions: vec![],
            },
            command: UnitCommand::Move {
                target: block.centre + Vec3::new(offset.x, 0.0, offset.y),
                frontage: Some(FormationFrontage {
                    facing: block.facing,
                    width: (block.files as f32 * shared::formation::FILE_SPACING).max(1.0),
                }),
                mode: MovementMode::Move,
            },
        };
        let (accepted, _) = crate::player::orders::apply_local_unit_order(
            world,
            &account,
            order.selection.units,
            order.command,
        );
        if accepted > 0 {
            for entity in units {
                world
                    .entity_mut(entity)
                    .insert((EvadingBombardment, BombardmentCooldown(impact.at)));
            }
        }
    }
}

fn escape_clear(world: &World, from: Vec2, to: Vec2) -> bool {
    let terrain = world.get_resource::<shared::terrain::WorldTerrain>();
    if terrain.is_some_and(|t| {
        let b = t.generator.active_map_bounds();
        to.cmplt(Vec2::from_array(b.min)).any() || to.cmpgt(Vec2::from_array(b.max)).any()
    }) {
        return false;
    }
    crate::player::siege::ground_clear(
        from,
        to,
        0.0,
        terrain,
        world.get_resource::<shared::spatial::SpatialObstacleGrid>(),
        world.get_resource::<crate::collision::library::StaticColliders>(),
        world.get_resource::<crate::collision::library::DerivedColliderLibrary>(),
    )
}

#[cfg(test)]
mod tests;
