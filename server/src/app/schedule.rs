//! Fixed-update schedule sets and system wiring.
//!
//! The full simulation schedule (physics, AI, persistence).

use bevy::ecs::schedule::SystemSet;
use bevy::prelude::*;
use lightyear::connection::ConnectionSystems;
use lightyear::link::LinkSystems;

use crate::ai;
use crate::collision;
use crate::net;
use crate::persistence;
use crate::physics;
use crate::player;
use crate::telemetry;
use crate::world;

use super::bootstrap::server_is_started;

pub(crate) fn configure_fixed_schedule(app: &mut App) {
    configure_fps_fixed_schedule(app);
}

#[derive(SystemSet, Debug, Clone, Copy, Eq, PartialEq, Hash)]
enum FpsServerSet {
    WorldTick,
    PhysicsWorld,
    NetIngress,
    AISim,
    PhysicsControl,
    PhysicsPost,
    PlayerSim,
    Indices,
    Persistence,
}

fn configure_fps_fixed_schedule(app: &mut App) {
    app.configure_sets(
        FixedUpdate,
        (
            FpsServerSet::WorldTick,
            FpsServerSet::PhysicsWorld,
            FpsServerSet::NetIngress,
            FpsServerSet::AISim,
            FpsServerSet::PhysicsControl,
            FpsServerSet::PhysicsPost,
            FpsServerSet::PlayerSim,
            FpsServerSet::Indices,
            FpsServerSet::Persistence,
        )
            .chain(),
    );

    app.add_systems(
        FixedUpdate,
        (
            world::time::handle_set_time_of_day,
            world::time::update_world_time,
            crate::city::buildings::sync_authored_plot_buildings,
            collision::building_index::sync_building_spatial_index,
            collision::streaming::update_static_collider_streaming,
        )
            .chain()
            .in_set(FpsServerSet::WorldTick)
            .run_if(server_is_started),
    );

    app.add_systems(
        FixedUpdate,
        (
            physics::terrain_colliders::sync_terrain_colliders,
            physics::static_world_colliders::sync_static_prop_colliders,
            physics::static_world_colliders::sync_static_building_colliders,
            physics::dynamic_actors::ensure_player_physics_bodies,
            physics::dynamic_actors::ensure_npc_physics_bodies,
            physics::dynamic_actors::cleanup_npc_physics_when_ragdoll_activates,
            physics::dynamic_actors::sync_player_bodies_from_authoritative_state,
            physics::dynamic_actors::sync_npcs_from_physics_before_ai,
        )
            .chain()
            .in_set(FpsServerSet::PhysicsWorld)
            .run_if(server_is_started),
    );

    app.add_systems(
        FixedUpdate,
        (
            net::connection::handle_connections,
            player::spawn::handle_player_name_submission,
            player::spawn::handle_set_player_character,
            ai::spawn::handle_spawn_oilman_debug,
            ai::spawn::handle_spawn_physics_box_debug,
            player::roster::handle_player_roster_requests,
            net::input::handle_client_input_messages,
        )
            .chain()
            .in_set(FpsServerSet::NetIngress)
            .run_if(server_is_started),
    );

    app.add_systems(
        FixedUpdate,
        (
            ai::obstacles::sync_obstacle_grid,
            ai::tick::update_npc_ai,
            ai::ragdoll::debug_auto_kill_npcs,
            ai::ragdoll::activate_npc_ragdolls,
            ai::ragdoll::evict_excess_corpses,
            ai::death_cleanup::ensure_dead_npc_despawn_timers,
            ai::death_cleanup::update_dead_npc_despawn_timers,
        )
            .chain()
            .in_set(FpsServerSet::AISim)
            .run_if(server_is_started),
    );

    app.add_systems(
        FixedUpdate,
        (
            physics::dynamic_actors::apply_player_controls,
            physics::dynamic_actors::apply_npc_controls_from_ai,
        )
            .chain()
            .before(bevy_rapier3d::plugin::PhysicsSet::SyncBackend)
            .in_set(FpsServerSet::PhysicsControl)
            .run_if(server_is_started),
    );

    app.add_systems(
        FixedUpdate,
        (
            physics::dynamic_actors::clamp_players_to_map_bounds,
            physics::dynamic_actors::sync_players_from_physics,
            physics::contacts::update_player_grounding_from_queries,
            physics::dynamic_actors::sync_npcs_from_physics_after_writeback,
            physics::dynamic_actors::sync_debug_boxes_from_physics,
            ai::ragdoll::stabilize_soft_ragdoll_bodies,
            ai::ragdoll::sync_npc_roots_from_ragdolls,
            ai::ragdoll::sync_corpse_collision_index,
            ai::ragdoll::send_ragdoll_pose_snapshots,
        )
            .chain()
            .after(bevy_rapier3d::plugin::PhysicsSet::Writeback)
            .in_set(FpsServerSet::PhysicsPost)
            .run_if(server_is_started),
    );

    app.add_systems(
        FixedUpdate,
        (
            physics::dynamic_actors::tick_player_jump_timers,
            player::lifecycle::handle_player_deaths,
            player::lifecycle::update_respawn_timers,
        )
            .chain()
            .in_set(FpsServerSet::PlayerSim)
            .run_if(server_is_started),
    );

    app.add_systems(
        FixedUpdate,
        (
            player::index::sync_player_entity_index,
            player::spatial::sync_player_spatial_index,
            ai::relevance::update_npc_network_visibility,
        )
            .chain()
            .in_set(FpsServerSet::Indices)
            .run_if(server_is_started),
    );

    app.add_systems(
        FixedUpdate,
        (
            persistence::autosave::update_periodic_player_save,
            persistence::io_queue::update_profile_io_acks,
        )
            .chain()
            .in_set(FpsServerSet::Persistence)
            .run_if(server_is_started),
    );

    app.add_systems(
        FixedUpdate,
        (
            telemetry::perf::handle_perf_tick_begin.before(world::time::handle_set_time_of_day),
            telemetry::perf::handle_perf_core_phase_begin
                .before(world::time::handle_set_time_of_day),
            // Phase brackets anchor on SystemSets, not individual systems, so that
            // deleting any single gameplay system cannot silently skew the timings.
            telemetry::perf::handle_perf_core_phase_end.after(FpsServerSet::PhysicsPost),
            telemetry::perf::handle_perf_npc_inventory_build_phase_begin
                .before(FpsServerSet::AISim),
            telemetry::perf::handle_perf_npc_inventory_build_phase_end
                .after(FpsServerSet::Persistence),
            telemetry::perf::update_server_perf_log.after(FpsServerSet::Persistence),
            telemetry::network::sample_replication_change_pressure.after(FpsServerSet::Persistence),
        )
            .run_if(server_is_started),
    );

    app.add_systems(
        PostUpdate,
        telemetry::network::sample_link_flow_post_send
            .after(ConnectionSystems::Send)
            .before(LinkSystems::Send)
            .run_if(server_is_started),
    );
}
