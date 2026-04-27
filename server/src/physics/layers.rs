//! Collision layers/groups for the server-authoritative Rapier world.

use bevy_rapier3d::prelude::{CollisionGroups, Group};

pub const GROUP_TERRAIN: Group = Group::GROUP_1;
pub const GROUP_STATIC_WORLD: Group = Group::GROUP_2;
pub const GROUP_PLAYER: Group = Group::GROUP_3;
pub const GROUP_NPC: Group = Group::GROUP_4;
pub const GROUP_VEHICLE: Group = Group::GROUP_5;
pub const GROUP_RAGDOLL: Group = Group::GROUP_6;
pub const GROUP_DEBUG_BOX: Group = Group::GROUP_7;
pub const GROUP_BULLET_QUERY: Group = Group::GROUP_8;

#[inline]
pub fn world_solid_mask() -> Group {
    Group::GROUP_1
        | Group::GROUP_2
        | Group::GROUP_3
        | Group::GROUP_4
        | Group::GROUP_5
        | Group::GROUP_6
        | Group::GROUP_7
}

#[inline]
pub fn dynamic_actor_mask() -> Group {
    Group::GROUP_3 | Group::GROUP_4 | Group::GROUP_5 | Group::GROUP_6 | Group::GROUP_7
}

#[inline]
pub fn world_query_mask() -> Group {
    dynamic_actor_mask() | GROUP_BULLET_QUERY
}

#[inline]
pub fn terrain_groups() -> CollisionGroups {
    CollisionGroups::new(GROUP_TERRAIN, world_query_mask())
}

#[inline]
pub fn static_world_groups() -> CollisionGroups {
    CollisionGroups::new(GROUP_STATIC_WORLD, world_query_mask())
}

#[inline]
pub fn player_groups() -> CollisionGroups {
    CollisionGroups::new(
        GROUP_PLAYER,
        GROUP_TERRAIN
            | GROUP_STATIC_WORLD
            | GROUP_NPC
            | GROUP_VEHICLE
            | GROUP_RAGDOLL
            | GROUP_DEBUG_BOX,
    )
}

#[inline]
pub fn player_seated_groups() -> CollisionGroups {
    CollisionGroups::new(GROUP_PLAYER, Group::NONE)
}

#[inline]
pub fn npc_groups() -> CollisionGroups {
    CollisionGroups::new(
        GROUP_NPC,
        GROUP_TERRAIN
            | GROUP_STATIC_WORLD
            | GROUP_PLAYER
            | GROUP_NPC
            | GROUP_VEHICLE
            | GROUP_RAGDOLL
            | GROUP_DEBUG_BOX,
    )
}

#[inline]
pub fn vehicle_groups() -> CollisionGroups {
    CollisionGroups::new(
        GROUP_VEHICLE,
        GROUP_TERRAIN
            | GROUP_STATIC_WORLD
            | GROUP_PLAYER
            | GROUP_NPC
            | GROUP_VEHICLE
            | GROUP_RAGDOLL
            | GROUP_DEBUG_BOX,
    )
}

#[inline]
pub fn ragdoll_groups() -> CollisionGroups {
    CollisionGroups::new(
        GROUP_RAGDOLL,
        GROUP_TERRAIN
            | GROUP_STATIC_WORLD
            | GROUP_PLAYER
            | GROUP_NPC
            | GROUP_VEHICLE
            | GROUP_RAGDOLL
            | GROUP_DEBUG_BOX
            | GROUP_BULLET_QUERY,
    )
}

#[inline]
pub fn ragdoll_no_self_groups() -> CollisionGroups {
    CollisionGroups::new(
        GROUP_RAGDOLL,
        GROUP_TERRAIN
            | GROUP_STATIC_WORLD
            | GROUP_PLAYER
            | GROUP_NPC
            | GROUP_VEHICLE
            | GROUP_DEBUG_BOX
            | GROUP_BULLET_QUERY,
    )
}

#[inline]
pub fn debug_box_groups() -> CollisionGroups {
    CollisionGroups::new(
        GROUP_DEBUG_BOX,
        GROUP_TERRAIN
            | GROUP_STATIC_WORLD
            | GROUP_PLAYER
            | GROUP_NPC
            | GROUP_VEHICLE
            | GROUP_RAGDOLL
            | GROUP_DEBUG_BOX
            | GROUP_BULLET_QUERY,
    )
}

#[inline]
pub fn bullet_world_query_groups() -> CollisionGroups {
    CollisionGroups::new(
        GROUP_BULLET_QUERY,
        GROUP_TERRAIN | GROUP_STATIC_WORLD | GROUP_DEBUG_BOX | GROUP_RAGDOLL,
    )
}
