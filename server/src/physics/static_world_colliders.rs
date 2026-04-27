//! Static world colliders (props + buildings) in Rapier.

use bevy::prelude::*;
use bevy_rapier3d::prelude::{Collider, RigidBody};
use std::collections::{HashMap, HashSet};

use shared::building::{BuildingPosition, PlacedBuilding};
use shared::colliders::BakedCollider;

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

#[derive(Resource, Default)]
pub struct StaticWorldColliderRegistry {
    pub props: HashMap<u32, Entity>,
    pub buildings: HashMap<Entity, Entity>,
}

fn scaled_points(points: &[[f32; 3]], scale: f32) -> Vec<Vec3> {
    points
        .iter()
        .map(|p| Vec3::new(p[0] * scale, p[1] * scale, p[2] * scale))
        .collect()
}

fn collider_from_baked(shape: &BakedCollider, scale: f32) -> Option<Collider> {
    match shape {
        BakedCollider::ConvexHull { points } => {
            let points = scaled_points(points, scale);
            Collider::convex_hull(&points)
        }
        BakedCollider::CompoundConvex { hulls } => {
            let mut parts = Vec::new();
            for hull in hulls {
                let points = scaled_points(hull, scale);
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

pub fn sync_static_prop_colliders(
    mut commands: Commands,
    library: Option<Res<BakedColliderLibrary>>,
    static_colliders: Res<StaticColliders>,
    mut registry: ResMut<StaticWorldColliderRegistry>,
) {
    let Some(library) = library else { return };

    let mut live_ids = HashSet::with_capacity(static_colliders.instances.len());

    for (instance_id, instance) in &static_colliders.instances {
        live_ids.insert(*instance_id);
        if registry.props.contains_key(instance_id) {
            continue;
        }

        let collider = library
            .by_kind
            .get(&instance.kind)
            .and_then(|shape| collider_from_baked(shape, instance.scale))
            .unwrap_or_else(|| Collider::cuboid(0.5, 0.5, 0.5));
        let transform =
            Transform::from_translation(instance.position).with_rotation(instance.rotation);

        let entity = commands
            .spawn((
                StaticPropCollider {
                    instance_id: *instance_id,
                },
                RigidBody::Fixed,
                collider,
                layers::static_world_groups(),
                transform,
                GlobalTransform::from(transform),
            ))
            .id();

        registry.props.insert(*instance_id, entity);
    }

    let stale: Vec<u32> = registry
        .props
        .keys()
        .copied()
        .filter(|id| !live_ids.contains(id))
        .collect();
    for id in stale {
        if let Some(entity) = registry.props.remove(&id) {
            commands.entity(entity).despawn();
        }
    }
}

pub fn sync_static_building_colliders(
    mut commands: Commands,
    buildings: Query<(Entity, &PlacedBuilding, &BuildingPosition)>,
    mut registry: ResMut<StaticWorldColliderRegistry>,
) {
    let mut live_buildings = HashSet::with_capacity(buildings.iter().len());

    for (building_entity, building, position) in buildings.iter() {
        live_buildings.insert(building_entity);

        let def = building.building_type.definition();
        let half_extents = Vec3::new(
            def.footprint.x * 0.5,
            def.height * 0.5,
            def.footprint.y * 0.5,
        );
        let translation = position.0 + Vec3::Y * half_extents.y;
        let rotation = Quat::from_rotation_y(building.rotation);
        let transform = Transform::from_translation(translation).with_rotation(rotation);

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
}
