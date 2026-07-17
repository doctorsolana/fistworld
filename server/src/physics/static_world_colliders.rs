//! Static world colliders (props + buildings) in Rapier.

use bevy::prelude::*;
use bevy_rapier3d::prelude::{Collider, RigidBody};
use std::collections::{HashMap, HashSet};

use shared::building::{building_rotation_quat, BuildingPosition, PlacedBuilding};
use shared::colliders::BakedCollider;
use shared::props::PropKind;

use crate::collision::building_index::BuildingSpatialIndex;
use crate::collision::library::{BakedColliderLibrary, StaticColliders};
use crate::physics::layers;

#[derive(Component, Clone, Copy, Debug)]
pub struct StaticPropCollider {
    pub instance_id: u32,
}

#[derive(Component, Clone, Copy, Debug)]
pub struct StaticBuildingCollider {
    pub building_entity: Entity,
}

const DEFAULT_MAX_PROP_COLLIDERS_PER_TICK: usize = 96;

#[derive(Resource)]
pub struct StaticWorldColliderRegistry {
    pub props: HashMap<u32, Entity>,
    pub buildings: HashMap<Entity, Entity>,
    pub props_synced_version: u64,
    pub buildings_synced_version: u64,
    base_colliders: HashMap<PropKind, Collider>,
    max_prop_colliders_per_tick: usize,
}

impl Default for StaticWorldColliderRegistry {
    fn default() -> Self {
        let max_prop_colliders_per_tick = std::env::var("CITYSIM_PROP_COLLIDERS_PER_TICK")
            .ok()
            .and_then(|raw| raw.parse::<usize>().ok())
            .unwrap_or(DEFAULT_MAX_PROP_COLLIDERS_PER_TICK)
            .clamp(8, 2048);
        Self {
            props: HashMap::new(),
            buildings: HashMap::new(),
            props_synced_version: 0,
            buildings_synced_version: 0,
            base_colliders: HashMap::new(),
            max_prop_colliders_per_tick,
        }
    }
}

fn collider_points(points: &[[f32; 3]]) -> Vec<Vec3> {
    points.iter().map(|p| Vec3::new(p[0], p[1], p[2])).collect()
}

fn collider_from_baked(shape: &BakedCollider) -> Option<Collider> {
    match shape {
        BakedCollider::ConvexHull { points } => {
            let points = collider_points(points);
            Collider::convex_hull(&points)
        }
        BakedCollider::CompoundConvex { hulls } => {
            let mut parts = Vec::new();
            for hull in hulls {
                let points = collider_points(hull);
                let Some(convex) = Collider::convex_hull(&points) else {
                    continue;
                };
                parts.push((Vec3::ZERO, Quat::IDENTITY, convex));
            }
            if parts.is_empty() {
                None
            } else {
                Some(Collider::compound(parts))
            }
        }
    }
}

fn building_box_transform(building: &PlacedBuilding, position: Vec3) -> (Vec3, Transform) {
    let def = building.building_type.definition();
    let half_extents = Vec3::new(
        def.footprint.x * 0.5,
        def.height * 0.5,
        def.footprint.y * 0.5,
    );
    let translation = position + Vec3::Y * half_extents.y;
    let rotation = building_rotation_quat(building.rotation);

    (
        half_extents,
        Transform::from_translation(translation).with_rotation(rotation),
    )
}

pub fn sync_static_prop_colliders(
    mut commands: Commands,
    library: Option<Res<BakedColliderLibrary>>,
    mut static_colliders: ResMut<StaticColliders>,
    mut registry: ResMut<StaticWorldColliderRegistry>,
) {
    let Some(library) = library else { return };

    for instance_id in static_colliders.pending_removed.drain(..) {
        if let Some(entity) = registry.props.remove(&instance_id) {
            commands.entity(entity).despawn();
        }
    }

    let budget = registry.max_prop_colliders_per_tick;
    for _ in 0..budget {
        let Some(instance_id) = static_colliders.pending_added.pop_front() else {
            break;
        };
        if registry.props.contains_key(&instance_id) {
            continue;
        }
        let Some(instance) = static_colliders.instances.get(&instance_id) else {
            continue;
        };

        let collider = if let Some(collider) = registry.base_colliders.get(&instance.kind) {
            collider.clone()
        } else {
            let collider = library
                .by_kind
                .get(&instance.kind)
                .and_then(collider_from_baked)
                .unwrap_or_else(|| Collider::cuboid(0.5, 0.5, 0.5));
            registry
                .base_colliders
                .insert(instance.kind, collider.clone());
            collider
        };
        let transform = Transform::from_translation(instance.position)
            .with_rotation(instance.rotation)
            .with_scale(Vec3::splat(instance.scale));

        let entity = commands
            .spawn((
                StaticPropCollider { instance_id },
                RigidBody::Fixed,
                collider,
                layers::static_world_groups(),
                transform,
                GlobalTransform::from(transform),
            ))
            .id();

        registry.props.insert(instance_id, entity);
    }
    registry.props_synced_version = static_colliders.version;
}

pub fn sync_static_building_colliders(
    mut commands: Commands,
    building_index: Res<BuildingSpatialIndex>,
    buildings: Query<(Entity, &PlacedBuilding, &BuildingPosition)>,
    mut registry: ResMut<StaticWorldColliderRegistry>,
) {
    if registry.buildings_synced_version == building_index.version {
        return;
    }
    let mut live_buildings = HashSet::with_capacity(buildings.iter().len());

    for (building_entity, building, position) in buildings.iter() {
        live_buildings.insert(building_entity);

        let (half_extents, transform) = building_box_transform(building, position.0);

        if let Some(collider_entity) = registry.buildings.get(&building_entity).copied() {
            commands.entity(collider_entity).insert((
                Collider::cuboid(half_extents.x, half_extents.y, half_extents.z),
                transform,
                GlobalTransform::from(transform),
            ));
            continue;
        }

        let collider_entity = commands
            .spawn((
                StaticBuildingCollider { building_entity },
                RigidBody::Fixed,
                Collider::cuboid(half_extents.x, half_extents.y, half_extents.z),
                layers::static_world_groups(),
                transform,
                GlobalTransform::from(transform),
            ))
            .id();
        registry.buildings.insert(building_entity, collider_entity);
    }

    let stale: Vec<Entity> = registry
        .buildings
        .keys()
        .copied()
        .filter(|entity| !live_buildings.contains(entity))
        .collect();
    for building_entity in stale {
        if let Some(collider_entity) = registry.buildings.remove(&building_entity) {
            commands.entity(collider_entity).despawn();
        }
    }
    registry.buildings_synced_version = building_index.version;
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::building::BuildingType;
    use shared::colliders::BakedCollider;

    use crate::collision::library::StaticColliderInstance;

    #[test]
    fn building_box_uses_city_yaw_convention() {
        let yaw = 37.0_f32.to_radians();
        let building = PlacedBuilding {
            building_type: BuildingType::Multistory03,
            rotation: yaw,
        };
        let position = Vec3::new(12.0, 4.0, -8.0);

        let (half_extents, transform) = building_box_transform(&building, position);
        let world_x = transform.rotation * Vec3::X;
        let expected_x = Vec3::new(yaw.cos(), 0.0, yaw.sin());

        assert!(world_x.distance(expected_x) < 1.0e-5);
        assert_eq!(transform.translation, position + Vec3::Y * half_extents.y);
    }

    #[test]
    fn prop_collider_creation_respects_tick_budget_and_instance_scale() {
        let kind = PropKind::Rock_1;
        let mut static_colliders = StaticColliders::default();
        for instance_id in 1..=2 {
            static_colliders.instances.insert(
                instance_id,
                StaticColliderInstance {
                    kind,
                    position: Vec3::new(instance_id as f32, 2.0, 3.0),
                    rotation: Quat::IDENTITY,
                    scale: 1.5,
                    cell: (0, 0),
                },
            );
            static_colliders.pending_added.push_back(instance_id);
        }

        let mut registry = StaticWorldColliderRegistry::default();
        registry.max_prop_colliders_per_tick = 1;
        let mut app = App::new();
        app.insert_resource(BakedColliderLibrary {
            by_kind: HashMap::from([(
                kind,
                BakedCollider::ConvexHull {
                    points: vec![
                        [0.0, 0.0, 0.0],
                        [1.0, 0.0, 0.0],
                        [0.0, 1.0, 0.0],
                        [0.0, 0.0, 1.0],
                    ],
                },
            )]),
        });
        app.insert_resource(static_colliders);
        app.insert_resource(registry);
        app.add_systems(Update, sync_static_prop_colliders);

        app.update();
        assert_eq!(
            app.world()
                .resource::<StaticWorldColliderRegistry>()
                .props
                .len(),
            1
        );
        let first_entity = app.world().resource::<StaticWorldColliderRegistry>().props[&1];
        assert_eq!(
            app.world().get::<Transform>(first_entity).unwrap().scale,
            Vec3::splat(1.5)
        );

        app.update();
        assert_eq!(
            app.world()
                .resource::<StaticWorldColliderRegistry>()
                .props
                .len(),
            2
        );
    }
}
