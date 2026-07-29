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
    NetIngress,
    Indices,
    Persistence,
}

fn configure_fps_fixed_schedule(app: &mut App) {
    app.configure_sets(
        FixedUpdate,
        (
            FpsServerSet::WorldTick,
            FpsServerSet::NetIngress,
            FpsServerSet::Indices,
            FpsServerSet::Persistence,
        )
            .chain(),
    );

    app.add_systems(
        FixedUpdate,
        (
            world::time::handle_set_time_of_day,
            world::dev::handle_dev_commands,
            world::time::update_world_time,
            crate::city::buildings::sync_authored_plot_buildings,
            collision::building_index::sync_building_spatial_index,
            collision::streaming::update_static_collider_streaming,
            world::navgrid::sync_obstacle_grid,
            world::regions::tick_strategic_world,
            world::regions::log_region_telemetry,
        )
            .chain()
            .in_set(FpsServerSet::WorldTick)
            .run_if(server_is_started),
    );

    app.add_systems(
        FixedUpdate,
        (
            net::connection::handle_connections,
            player::spawn::handle_player_name_submission,
            player::roster::handle_player_roster_requests,
            net::input::handle_client_input_messages,
            player::commander::sync_commander_views,
            player::hero::handle_hero_move_orders,
            player::hero::step_heroes,
            world::regions::update_client_interest,
            world::regions::apply_region_visibility,
            world::regions::update_region_sim_levels,
        )
            .chain()
            .in_set(FpsServerSet::NetIngress)
            .run_if(server_is_started),
    );

    app.add_systems(
        FixedUpdate,
        (player::index::sync_player_entity_index,)
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
            telemetry::perf::handle_perf_core_phase_end.after(FpsServerSet::WorldTick),
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
