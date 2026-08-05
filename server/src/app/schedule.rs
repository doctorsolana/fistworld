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
            world::village::claim_settlement_hall_obstacles,
            world::time::update_world_time,
            crate::city::buildings::sync_authored_plot_buildings,
            collision::building_index::sync_building_spatial_index,
            collision::streaming::update_static_collider_streaming,
            world::navgrid::sync_obstacle_grid,
            // Villages run themselves: tag, seek, arrive, recount, decide, build.
            // Chained because each step reads what the previous one wrote, and
            // a resident who arrives must be counted before anyone applies for
            // a permit on their behalf.
            (
                (
                    player::hero::ensure_character_attributes,
                    world::village::tag_villager_intent,
                    world::village::seek_settlement,
                    world::village::arrive_at_settlement,
                    world::village::recount_residents,
                    world::village::ensure_village_finances,
                    world::village::ensure_settlement_economies,
                    world::village::ensure_business_economies,
                )
                    .chain(),
                (
                    world::village_roads::ensure_moot_administrations,
                    world::settlement_development::ensure_settlement_developments,
                    world::village_roads::staff_and_pay_road_stewards,
                    world::village::staff_moot_hall_roles,
                    world::village_roads::staff_public_positions,
                    world::village::reconcile_work_statuses,
                    world::village_roads::audit_village_roads,
                )
                    .chain(),
                (
                    world::village::update_moot_market_targets,
                    world::village::ensure_households,
                    world::village::assign_households,
                    world::village::update_household_budgets_and_pantries,
                    world::village::update_settlement_economies,
                    world::village::run_business_payroll_and_owner_leisure,
                    world::village::history::capture_settlement_history,
                    world::village::consider_permits,
                )
                    .chain(),
                world::village::run_construction_material_logistics,
                world::village::advance_construction,
                // Publish the separate crop footprint before surveying the
                // completed Farmstead's road.
                world::village::ensure_farm_fields,
                world::village_roads::plan_requested_roads,
                world::village::fill_vacancies,
                world::village::run_household_schedules,
                world::village::run_workplace_door_transits,
                world::village_roads::build_village_roads,
                world::settlement_development::upgrade_town_roads,
                world::settlement_development::update_settlement_developments,
                (
                    world::village::ensure_fishing_piers,
                    world::village::assign_farmer_routines,
                    world::village::assign_fishing_routines,
                    world::village::assign_lumberjack_routines,
                    world::village::run_household_shopping,
                    world::village::run_farmer_routines,
                    world::village::run_fishing_routines,
                    world::village::run_lumberjack_routines,
                    world::village::run_market_collections,
                    world::village::ambient::run_ambient_routines,
                    world::village::settle_pending_market_payments,
                    player::hero::sync_hero_attributes_to_player_progression,
                    (
                        world::village::sync_carried_load,
                        world::village::sync_building_door_demands,
                    )
                        .chain(),
                )
                    .chain(),
            )
                .chain(),
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
            player::roster::handle_character_roster_requests,
            world::village::history::handle_settlement_history_requests,
            world::village::history::handle_world_history_requests,
            net::input::handle_client_input_messages,
            player::commander::sync_commander_views,
            player::hero::handle_unit_move_orders,
            world::village_roads::rebuild_village_road_graph,
            world::village_roads::queue_villager_travel_routes,
            world::village_roads::retry_failed_routes_after_obstacle_change,
            world::village_roads::plan_villager_travel_routes,
            player::hero::step_units,
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
