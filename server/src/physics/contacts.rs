//! Contact/grounding helpers for dynamic actors.

use bevy::prelude::*;
use bevy_rapier3d::prelude::{QueryFilter, ReadRapierContext};

use shared::components::{Player, PlayerGrounded};
use shared::physics::WALKABLE_THRESHOLD;
use shared::player::PLAYER_HEIGHT;
use shared::vehicle::{Vehicle, VehicleState};

use crate::physics::dynamic_actors::{PlayerPhysicsBody, VehiclePhysicsBody};
use crate::physics::static_world_colliders::{StaticBuildingCollider, StaticPropCollider};
use crate::physics::terrain_colliders::TerrainColliderChunk;

pub fn update_player_grounding_from_queries(
    time: Res<Time>,
    rapier: ReadRapierContext,
    terrain_colliders: Query<(), With<TerrainColliderChunk>>,
    _prop_colliders: Query<(), With<StaticPropCollider>>,
    _building_colliders: Query<(), With<StaticBuildingCollider>>,
    mut players: Query<
        (Entity, &Transform, &mut PlayerGrounded),
        (With<Player>, With<PlayerPhysicsBody>),
    >,
) {
    let Ok(ctx) = rapier.single() else {
        return;
    };

    let dt = time.delta_secs();

    for (entity, transform, mut grounded) in players.iter_mut() {
        let hit = ctx.cast_ray_and_get_normal(
            transform.translation,
            Vec3::NEG_Y,
            PLAYER_HEIGHT * 0.6 + 0.35,
            true,
            QueryFilter::new().exclude_rigid_body(entity),
        );

        grounded.on_terrain = false;
        grounded.on_static = false;

        if let Some((hit_entity, hit_data)) = hit {
            if hit_data.normal.y > WALKABLE_THRESHOLD {
                if terrain_colliders.get(hit_entity).is_ok() {
                    grounded.on_terrain = true;
                } else {
                    grounded.on_static = true;
                }
            }
        }

        if grounded.is_grounded() {
            grounded.time_since_grounded = 0.0;
        } else {
            grounded.time_since_grounded += dt;
        }
    }
}

pub fn update_vehicle_grounded_from_queries(
    rapier: ReadRapierContext,
    mut vehicles: Query<
        (Entity, &Transform, &mut VehicleState),
        (With<Vehicle>, With<VehiclePhysicsBody>),
    >,
) {
    let Ok(ctx) = rapier.single() else {
        return;
    };

    for (entity, transform, mut state) in vehicles.iter_mut() {
        state.grounded = ctx
            .cast_ray_and_get_normal(
                transform.translation,
                Vec3::NEG_Y,
                1.8,
                true,
                QueryFilter::new().exclude_rigid_body(entity),
            )
            .map(|(_, hit)| hit.normal.y > 0.25)
            .unwrap_or(false);
    }
}
