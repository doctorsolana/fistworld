//! Fixed-update schedule sets and system wiring.
//!
//! The full simulation schedule (physics, AI, persistence).

use bevy::ecs::schedule::SystemSet;
use bevy::prelude::*;
use lightyear::connection::ConnectionSystems;
use lightyear::link::LinkSystems;

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
            world::navgrid::sync_obstacle_grid,
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
            physics::dynamic_actors::sync_player_bodies_from_authoritative_state,
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
            physics::dynamic_actors::apply_player_controls,
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
