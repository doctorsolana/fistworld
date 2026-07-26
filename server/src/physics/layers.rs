//! Collision layers/groups for the server-authoritative Rapier world.
//!
//! The world currently contains only static geometry — terrain heightfields and baked
//! prop/building colliders — plus ray queries against them for line of sight. Groups for
//! dynamic actors (players/NPCs/vehicles/ragdolls) died with the FPS embodiment; add a
//! `GROUP_UNIT` here when units get physics bodies.

use bevy_rapier3d::prelude::{CollisionGroups, Group};

pub const GROUP_TERRAIN: Group = Group::GROUP_1;
pub const GROUP_STATIC_WORLD: Group = Group::GROUP_2;
/// Ray/shape queries used for line-of-sight and ground probing.
pub const GROUP_LOS_QUERY: Group = Group::GROUP_8;

/// What the static world is visible to.
#[inline]
fn world_query_mask() -> Group {
    GROUP_LOS_QUERY
}

#[inline]
pub fn terrain_groups() -> CollisionGroups {
    CollisionGroups::new(GROUP_TERRAIN, world_query_mask())
}

#[inline]
pub fn static_world_groups() -> CollisionGroups {
    CollisionGroups::new(GROUP_STATIC_WORLD, world_query_mask())
}

/// Groups for a line-of-sight query: collides with the static world only.
#[inline]
pub fn los_query_groups() -> CollisionGroups {
    CollisionGroups::new(GROUP_LOS_QUERY, GROUP_TERRAIN | GROUP_STATIC_WORLD)
}
