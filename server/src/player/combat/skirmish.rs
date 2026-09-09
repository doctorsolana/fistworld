//! Unassigned troops retain individual control. They choose an
//! exposed opponent and a free approach, rather than inheriting a battalion's
//! frontage or making every ally chase the clicked person through the ranks.
use super::{
    fronts::{CombatSpace, Enemy},
    AttackOrder,
};
use crate::player::hero::{navigation_segment_clear, MoveTarget};
use crate::player::orders::{FormationRoutes, MarchOrder};
use crate::world::village_roads::{TravelRoute, NavigationRoutePending, NavigationRouteFailed};
use bevy::prelude::*;
use shared::components::*;
use std::collections::HashMap;
#[derive(Component, Clone, Copy, Debug)]
pub struct SkirmishOrder {
    pub target: Enemy,
}
/// A certified, unobstructed local manoeuvre. Longer or obstructed approaches
/// instead use the existing budgeted and cached tactical route planner.
#[derive(Component)]
pub struct DirectCombatApproach;

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn steer_skirmishers(
    mut commands: Commands,
    space: Res<CombatSpace>,
    clock: Query<&WorldTime>,
    health: Query<&Health>,
    bows: Query<(), With<BowEquipped>>,
    mut last: Local<f64>,
    mut routes: Option<ResMut<FormationRoutes>>,
    units: Query<(
        Entity,
        &SkirmishOrder,
        &PlayerPosition,
        Option<&AttackOrder>,
        Option<&MoveTarget>,
        Has<Mounted>,
        Option<&MarchOrder>,
    )>,
    terrain: Option<Res<shared::terrain::WorldTerrain>>,
    buildings: Option<Res<shared::spatial::SpatialObstacleGrid>>,
    colliders: Option<Res<crate::collision::library::StaticColliders>>,
    derived: Option<Res<crate::collision::library::DerivedColliderLibrary>>,
    mut loads: Local<HashMap<Entity, usize>>,
    mut candidates: Local<Vec<(Entity, f32)>>,
) {
    let Some(clock) = clock.iter().next() else {
        return;
    };
    let now = super::world_clock_seconds(clock);
    if now >= *last && now - *last < 0.12 {
        return;
    }
    *last = now;
    loads.clear();
    for (_, _, _, attack, _, _, _) in &units {
        if let Some(a) = attack {
            *loads.entry(a.target).or_default() += 1;
        }
    }
    for (entity, order, position, attack, moving, mounted, march) in &units {
        if bows.contains(entity) {
            continue;
        }
        let Some(body) = space.body(entity) else {
            continue;
        };
        if attack.is_some_and(|a| space.clear_strike(entity, a.target)) {
            if mounted && march.is_some() { commands.entity(entity).remove::<(MarchOrder, TravelRoute, NavigationRoutePending, NavigationRouteFailed)>(); }
            continue;
        }
        candidates.clear();
        for enemy in space.enemy_members(order.target) {
            if enemy.side != body.side {
                candidates.push((
                    enemy.entity,
                    enemy.point.distance_squared(body.point)
                        + loads.get(&enemy.entity).copied().unwrap_or(0) as f32 * 6.0,
                ));
            }
        }
        if candidates.is_empty() {
            // Explicit attacks on an uncommanded person retain the existing
            // single-person pursuit path; neutral villagers are not indexed.
            if let Enemy::Person(target) = order.target {
                if health.get(target).is_ok_and(|h| !h.is_dead()) {
                    continue;
                }
            }
            commands.entity(entity).remove::<(
                SkirmishOrder,
                DirectCombatApproach,
                AttackOrder,
                EngagedWith,
                MoveTarget,
                CombatReady,
            )>();
            continue;
        }
        candidates.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
        let mut chosen = None;
        'opponents: for (enemy, _) in candidates.iter().take(8) {
            let opponent = space.body(*enemy).unwrap();
            let direction = (body.point - opponent.point)
                .try_normalize()
                .unwrap_or(Vec2::X);
            for angle in [0.0_f32, 0.65, -0.65, 1.25, -1.25] {
                let side = Vec2::new(
                    direction.x * angle.cos() - direction.y * angle.sin(),
                    direction.x * angle.sin() + direction.y * angle.cos(),
                );
                let goal = opponent.point + side * body.contact_distance(opponent);
                if !space.approach_clear(entity, body.point, goal) {
                    continue;
                }
                let clear = if mounted {
                    crate::player::siege::ground_clear(body.point, goal, HORSE_CLEARANCE, terrain.as_deref(), buildings.as_deref(), colliders.as_deref(), derived.as_deref())
                } else {
                    navigation_segment_clear(body.point, goal, buildings.as_deref(), colliders.as_deref(), derived.as_deref())
                        && terrain.as_deref().is_none_or(|t| crate::player::hero::terrain_segment_walkable(t, body.point, goal))
                };
                if !clear { continue; }

                chosen = Some((*enemy, goal));
                break 'opponents;
            }
        }
        let direct = chosen.is_some();
        // The local body search is deliberately bounded. Do not turn that
        // bound into a maximum attack range: route towards the nearest exposed
        // edge through the ordinary planner when no direct approach exists.
        let (enemy, goal) = chosen.unwrap_or_else(|| {
            let enemy = candidates[0].0;
            let opponent = space.body(enemy).unwrap();
            let outward = (body.point - opponent.point)
                .try_normalize()
                .unwrap_or(Vec2::X);
            (
                enemy,
                opponent.point + outward * body.contact_distance(opponent),
            )
        });
        {
            if direct {
                if mounted && march.is_some() { commands.entity(entity).remove::<MarchOrder>(); }
                commands
                    .entity(entity)
                    .insert_if_new(DirectCombatApproach)
                    .remove::<(
                        crate::world::village_roads::TravelRoute,
                        crate::world::village_roads::NavigationRoutePending,
                    )>();
            } else {
                commands.entity(entity).remove::<DirectCombatApproach>();
            }
            if attack.is_none_or(|a| a.target != enemy) {
                commands
                    .entity(entity)
                    .insert(AttackOrder { target: enemy })
                    .remove::<EngagedWith>();
                *loads.entry(enemy).or_default() += 1;
            }
            if body
                .point
                .distance_squared(space.body(enemy).unwrap().point)
                > body.melee_reach(space.body(enemy).unwrap()).powi(2)
                || !space.clear_strike(entity, enemy)
            {
                if moving.is_none_or(|m| m.0.xz().distance_squared(goal) > 0.1) {
                    commands.entity(entity).insert(MoveTarget(Vec3::new(
                        goal.x,
                        terrain
                            .as_deref()
                            .map_or(position.0.y, |t| t.get_height(goal.x, goal.y)),
                        goal.y,
                    )));
                }
            }
            if mounted && !direct && march.is_none_or(|m| m.destination.xz().distance_squared(goal) > 0.75 * 0.75) {
                if let Some(routes) = routes.as_mut() {
                    let group = routes.register_with_clearance(vec![body.point, goal], goal, HORSE_CLEARANCE);
                    let destination = Vec3::new(goal.x, terrain.as_deref().map_or(position.0.y, |t| t.get_height(goal.x, goal.y)), goal.y);
                    commands.entity(entity).insert(MarchOrder { destination, facing: (goal - body.point).normalize_or_zero(), group });
                }
            }
            commands.entity(entity).insert_if_new(CombatReady);
        }
    }
}
