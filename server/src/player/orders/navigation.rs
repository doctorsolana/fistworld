//! Battalion route ownership. Open ground stays direct; obstructed formations
//! share one bounded field and feed certified routes to the existing mover.
use super::flow::FlowField;
use super::{CommandStance, MarchOrder};
use crate::collision::library::{DerivedColliderLibrary, StaticColliders};
use crate::player::{combat::AttackOrder, hero::MoveTarget};
use crate::world::village_roads::{
    navigation_geometry_version, NavigationRouteFailed, NavigationRoutePending, RouteWaypoint,
    TravelRoute,
};
use bevy::prelude::*;
use shared::{components::*, spatial::SpatialObstacleGrid, terrain::WorldTerrain};
use std::collections::{BTreeMap, HashSet};

struct FormationRoute {
    points: Vec<Vec2>,
    goal: Vec2,
    clearance: f32,
    field: Option<FlowField>,
    version: u64,
    failed: HashSet<Entity>,
}
#[derive(Resource, Default)]
pub struct FormationRoutes {
    next: u64,
    groups: BTreeMap<u64, FormationRoute>,
    cursor: usize,
}
impl FormationRoutes {
    pub fn register_with_clearance(
        &mut self,
        points: Vec<Vec2>,
        goal: Vec2,
        clearance: f32,
    ) -> u64 {
        self.next += 1;
        self.groups.insert(
            self.next,
            FormationRoute {
                points,
                goal,
                clearance,
                field: None,
                version: 0,
                failed: HashSet::new(),
            },
        );
        self.next
    }
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn advance_marches(
    mut commands: Commands,
    routes: Option<ResMut<FormationRoutes>>,
    terrain: Option<Res<WorldTerrain>>,
    buildings: Option<Res<SpatialObstacleGrid>>,
    colliders: Option<Res<StaticColliders>>,
    derived: Option<Res<DerivedColliderLibrary>>,
    mut active: Local<HashSet<u64>>,
    mut units: Query<(
        Entity,
        &MarchOrder,
        &PlayerPosition,
        &mut PlayerRotation,
        Option<&MoveTarget>,
        Option<&TravelRoute>,
        Has<NavigationRoutePending>,
        Has<AttackOrder>,
        Has<crate::player::combat::fronts::PausedFormationMarch>,
        Has<Mounted>,
        Has<crate::player::combat::SkirmishOrder>,
    )>,
) {
    let Some(mut routes) = routes else {
        return;
    };
    active.clear();
    for (_, march, ..) in &units {
        active.insert(march.group);
    }
    routes.groups.retain(|id, _| active.contains(id));
    if routes.groups.is_empty() {
        return;
    }
    let version = navigation_geometry_version(buildings.as_deref(), colliders.as_deref());
    let clear_at = |radius: f32, a: Vec2, b: Vec2| {
        crate::player::siege::ground_clear(
            a,
            b,
            radius,
            terrain.as_deref(),
            buildings.as_deref(),
            colliders.as_deref(),
            derived.as_deref(),
        )
    };
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(2);
    let mut route_budget = 32;
    let grounded = |p: Vec2| {
        Vec3::new(
            p.x,
            terrain.as_deref().map_or(0.0, |t| t.get_height(p.x, p.y)),
            p.y,
        )
    };
    // Round-robin batches prevent one obstructed battalion monopolising the
    // planner. This budget is shared by the army, never multiplied by soldiers.
    let count = routes.groups.len();
    let skip = routes.cursor % count;
    routes.cursor = (skip + 1) % count;
    let mut budget = 2048;
    for group in routes.groups.values_mut().skip(skip) {
        if let Some(field) = group.field.as_mut() {
            field.advance_until(&mut budget, deadline, &|a, b| {
                clear_at(group.clearance, a, b)
            });
        }
    }
    for group in routes.groups.values_mut().take(skip) {
        if let Some(field) = group.field.as_mut() {
            field.advance_until(&mut budget, deadline, &|a, b| {
                clear_at(group.clearance, a, b)
            });
        }
    }
    for (entity, march, position, mut rotation, target, route, pending, fighting, paused, mounted, skirmish) in
        &mut units
    {
        if (fighting && !(mounted && skirmish)) || paused {
            continue;
        }
        if target.is_none() && position.0.xz().distance_squared(march.destination.xz()) < 0.2 * 0.2
        {
            rotation.set_if_neq(PlayerRotation(f32::atan2(-march.facing.x, -march.facing.y)));
            commands
                .entity(entity)
                .remove::<(
                    MarchOrder,
                    MoveTarget,
                    TravelRoute,
                    NavigationRoutePending,
                    NavigationRouteFailed,
                )>()
                .insert(CommandStance::Guard)
                .remove::<crate::player::army::EvadingBombardment>();
            continue;
        }
        let Some(group) = routes.groups.get_mut(&march.group) else {
            continue;
        };
        if group.version != version {
            group.field = None;
            group.failed.clear();
            group.version = version;
        }
        if route.is_some_and(|r| r.geometry_version == version) {
            continue;
        }
        if route_budget == 0 || std::time::Instant::now() >= deadline {
            if target.is_some() || route.is_some() || !pending {
                commands
                    .entity(entity)
                    .remove::<(MoveTarget, TravelRoute)>()
                    .insert(NavigationRoutePending::new(march.destination));
            }
            continue;
        }
        route_budget -= 1;
        let start = position.0.xz();
        let goal = march.destination.xz();
        let clear = |a, b| clear_at(group.clearance, a, b);
        if clear(start, goal) {
            commands
                .entity(entity)
                .insert((
                    MoveTarget(march.destination),
                    TravelRoute {
                        goal: march.destination,
                        waypoints: vec![RouteWaypoint {
                            position: grounded(goal),
                            on_road: false,
                        }],
                        next: 0,
                        geometry_version: version,
                    },
                ))
                .remove::<(NavigationRoutePending, NavigationRouteFailed)>();
            continue;
        }
        let field = group
            .field
            .get_or_insert_with(|| FlowField::new(&group.points, group.goal));
        if !field.complete() {
            if target.is_some() || route.is_some() || !pending {
                commands
                    .entity(entity)
                    .remove::<(MoveTarget, TravelRoute)>()
                    .insert(NavigationRoutePending::new(march.destination));
            }
            continue;
        }
        if group.failed.contains(&entity) {
            continue;
        }
        if let Some(path) = field.path(start, goal, &clear) {
            commands
                .entity(entity)
                .insert((
                    MoveTarget(march.destination),
                    TravelRoute {
                        goal: march.destination,
                        waypoints: path
                            .into_iter()
                            .map(|p| RouteWaypoint {
                                position: grounded(p),
                                on_road: false,
                            })
                            .collect(),
                        next: 0,
                        geometry_version: version,
                    },
                ))
                .remove::<(NavigationRoutePending, NavigationRouteFailed)>();
        } else {
            if group.failed.is_empty() {
                warn!("Formation route could not reach {:?}; issue a nearer waypoint or a different frontage", march.destination);
            }
            group.failed.insert(entity);
            // Explicit failure, retried on a new command or geometry revision.
            commands
                .entity(entity)
                .remove::<(MoveTarget, TravelRoute, NavigationRoutePending)>()
                .insert(NavigationRouteFailed {
                    goal: march.destination,
                });
        }
    }
}
