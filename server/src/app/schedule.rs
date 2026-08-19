//! Fixed-update schedule sets and system wiring.
//!
//! The full authoritative world, network, village and persistence schedule.

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
    configure_server_fixed_schedule(app);
}

#[derive(SystemSet, Debug, Clone, Copy, Eq, PartialEq, Hash)]
enum ServerSet {
    WorldTick,
    NetIngress,
    Indices,
    Persistence,
}

fn configure_server_fixed_schedule(app: &mut App) {
    app.add_systems(
        Startup,
        world::immigration::prepare_natural_immigration_coasts,
    );
    world::village::schedule::configure_shared_village_simulation(app, FixedUpdate);

    app.configure_sets(
        FixedUpdate,
        (
            ServerSet::WorldTick,
            ServerSet::NetIngress,
            ServerSet::Indices,
            ServerSet::Persistence,
        )
            .chain(),
    );
    app.configure_sets(
        FixedUpdate,
        (
            world::village::schedule::VillageSimulationSet::Time
                .in_set(ServerSet::WorldTick)
                .before(world::time::update_world_time)
                .run_if(server_is_started),
            world::village::schedule::VillageSimulationSet::Core
                .in_set(ServerSet::WorldTick)
                .after(world::navgrid::sync_obstacle_grid)
                .before(world::regions::tick_strategic_world)
                .run_if(server_is_started),
            world::village::schedule::VillageSimulationSet::Navigation
                .in_set(ServerSet::NetIngress)
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
            world::village::strategic::advance_strategic_travel,
            world::village::strategic::advance_strategic_company_deliveries,
            world::village::strategic::advance_strategic_villages,
            world::regions::log_region_telemetry,
        )
            .chain()
            .in_set(ServerSet::WorldTick)
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
            world::village::history::handle_company_history_requests,
            net::input::handle_client_input_messages,
            player::commander::sync_commander_views,
            player::hero::handle_unit_move_orders,
            player::permits::handle_hero_construction_orders,
            player::companies::handle_hero_company_founding,
            player::business::handle_hero_business_orders,
            player::business::handle_hero_company_orders,
            player::trade_routes::handle_hero_trade_route_orders,
            player::market::handle_hero_market_orders,
            player::hero::ensure_player_permit_ledgers,
            player::permits::handle_hero_permit_orders,
            world::regions::update_client_interest,
            world::regions::apply_region_visibility,
            world::regions::update_region_sim_levels,
        )
            .chain()
            .in_set(ServerSet::NetIngress)
            .run_if(server_is_started),
    );

    app.add_systems(
        FixedUpdate,
        world::immigration::plan_natural_immigration
            .after(world::time::update_world_time)
            .before(crate::city::buildings::sync_authored_plot_buildings)
            .in_set(ServerSet::WorldTick)
            .run_if(server_is_started),
    );

    app.add_systems(
        FixedUpdate,
        world::dev::handle_god_access_requests
            .after(player::spawn::handle_player_name_submission)
            .before(player::roster::handle_character_roster_requests)
            .in_set(ServerSet::NetIngress)
            .run_if(server_is_started),
    );

    // Opening-voyage systems are kept out of the already-large ingress tuple
    // so adding future vessel classes does not hit Bevy's tuple arity ceiling.
    app.add_systems(
        FixedUpdate,
        player::boat::handle_create_hero_requests
            .after(player::spawn::handle_player_name_submission)
            .before(world::regions::update_client_interest)
            .in_set(ServerSet::NetIngress)
            .run_if(server_is_started),
    );
    app.add_systems(
        FixedUpdate,
        (
            player::boat::handle_disembark_requests,
            player::boat::plan_vessel_routes,
            player::boat::step_boats,
            player::boat::sync_aboard_heroes,
            world::immigration::sync_natural_immigrant_passengers,
            world::immigration::finish_natural_immigrant_voyages,
        )
            .chain()
            .after(player::hero::handle_unit_move_orders)
            .before(world::regions::update_client_interest)
            .in_set(ServerSet::NetIngress)
            .run_if(server_is_started),
    );

    app.add_systems(
        FixedUpdate,
        persistence::autosave::update_periodic_player_save
            .in_set(ServerSet::Persistence)
            .run_if(server_is_started),
    );

    app.add_systems(
        FixedUpdate,
        (
            telemetry::perf::handle_perf_tick_begin.before(world::time::handle_set_time_of_day),
            telemetry::perf::handle_perf_core_phase_begin
                .after(world::navgrid::sync_obstacle_grid)
                .before(world::village::schedule::VillageSimulationSet::Core),
            // Bracket the exact sets. A mere `before(rebuild_graph)` constraint
            // can legally run at the start of NetIngress and accidentally
            // include the preceding village core, making an expensive permit
            // search look like a navigation stall.
            telemetry::perf::handle_perf_core_phase_end
                .after(world::village::schedule::VillageSimulationSet::Core)
                .before(world::regions::tick_strategic_world),
            telemetry::perf::handle_perf_navigation_phase_begin
                .after(player::hero::handle_unit_move_orders)
                .before(world::village::schedule::VillageSimulationSet::Navigation),
            telemetry::perf::handle_perf_navigation_phase_end
                .after(world::village::schedule::VillageSimulationSet::Navigation)
                .before(world::regions::update_client_interest),
            telemetry::perf::update_server_perf_log.after(ServerSet::Persistence),
            telemetry::network::sample_replication_change_pressure.after(ServerSet::Persistence),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn authoritative_schedules_initialize_without_query_conflicts() {
        // Query alias validation happens while Bevy initializes a schedule,
        // not during `cargo check`. With no GameServer entity, the shared
        // server_is_started condition prevents world logic from executing;
        // running the schedules still initializes every registered system and
        // catches B0001-style startup crashes before a playtest.
        let mut app = App::new();
        configure_fixed_schedule(&mut app);
        app.world_mut().run_schedule(FixedUpdate);
        app.world_mut().run_schedule(PostUpdate);
    }
}
