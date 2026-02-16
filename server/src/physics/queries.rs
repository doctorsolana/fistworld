//! Shared Rapier query helpers used by gameplay systems.

use bevy::prelude::*;
use bevy_rapier3d::prelude::{QueryFilter, RapierContext, RayIntersection};

use crate::physics::layers;

#[inline]
pub fn cast_world_impact(
    context: &RapierContext<'_>,
    origin: Vec3,
    direction: Vec3,
    max_distance: f32,
) -> Option<(Entity, RayIntersection)> {
    context.cast_ray_and_get_normal(
        origin,
        direction,
        max_distance,
        true,
        QueryFilter::new()
            .groups(layers::bullet_world_query_groups())
            .exclude_sensors(),
    )
}
