//! Shared Rapier query helpers used by gameplay systems.

use bevy::prelude::*;
use bevy_rapier3d::prelude::{QueryFilter, RapierContext, RayIntersection};

use crate::physics::layers;

/// Raycast against streamed rapier colliders. Currently unused (its only caller was
/// bullet simulation) but kept for unit line-of-sight: see also
/// `collision::raycast`, which queries terrain/prop/building data directly and so
/// works beyond the streamed-collider radius.
#[allow(dead_code)]
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
            .groups(layers::los_query_groups())
            .exclude_sensors(),
    )
}
