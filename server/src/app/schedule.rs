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
    world::village::schedule::configure_shared_village_simulation(app, FixedUpdate);

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
    app.configure_sets(
        FixedUpdate,
        (
            world::village::schedule::VillageSimulationSet::Core
                .in_set(FpsServerSet::WorldTick)
                .after(world::navgrid::sync_obstacle_grid)
                .before(world::regions::tick_strategic_world)
                .run_if(server_is_started),
            world::village::schedule::VillageSimulationSet::Navigation
                .in_set(FpsServerSet::NetIngress)
                .after(player::hero::handle_unit_move_orders)
                .before(world::regions::update_client_interest)
                .run_if(server_is_started),
        ),
    );

    app.add_systems(
        FixedUpdate,
        (
            world::time::handle_set_time_of_day,
            world::dev::handle_dev_commands,
            world::village::strategic::update_person_simulation_lod,
            world::village::claim_settlement_hall_obstacles,
            world::time::update_world_time,
            crate::city::buildings::sync_authored_plot_buildings,
            collision::building_index::sync_building_spatial_index,
            collision::streaming::update_static_collider_streaming,
            world::navgrid::sync_obstacle_grid,
            world::regions::tick_strategic_world,
            world::village::strategic::advance_strategic_villages,
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
            player::roster::handle_character_roster_requests,
            world::village::history::handle_settlement_history_requests,
            world::village::history::handle_world_history_requests,
            net::input::handle_client_input_messages,
            player::commander::sync_commander_views,
            player::hero::handle_unit_move_orders,
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
            telemetry::perf::handle_perf_navigation_phase_begin
                .before(world::village_roads::rebuild_village_road_graph),
            telemetry::perf::handle_perf_navigation_phase_end
                .after(player::hero::step_units)
                .before(world::regions::update_client_interest),
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
