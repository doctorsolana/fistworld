//! Fixed-update schedule sets and system wiring.
//!
//! Default: the full FistForce simulation (physics, AI, inventory,
//! persistence). Set `FISTFORCE_RAIL=1` (same flag as the client) to run the
//! rail-tycoon prototype schedule instead; the two consume the same network
//! messages (e.g. `SubmitPlayerName`), so they are wired mutually exclusively.

use bevy::ecs::schedule::SystemSet;
use bevy::prelude::*;
use lightyear::connection::ConnectionSystems;
use lightyear::link::LinkSystems;

use crate::ai;
use crate::collision;
use crate::inventory;
use crate::net;
use crate::persistence;
use crate::physics;
use crate::player;
use crate::rail;
use crate::telemetry;
use crate::vehicle;
use crate::world;

use super::bootstrap::server_is_started;

/// `FISTFORCE_RAIL=1`: run the rail-tycoon prototype instead of the shooter.
pub(crate) fn rail_mode_enabled() -> bool {
    std::env::var("FISTFORCE_RAIL")
        .map(|value| matches!(value.trim(), "1" | "true" | "yes" | "on"))
        .unwrap_or(false)
}

pub(crate) fn configure_fixed_schedule(app: &mut App) {
    if rail_mode_enabled() {
        info!("FISTFORCE_RAIL=1: wiring rail-tycoon server schedule");
        configure_rail_fixed_schedule(app);
    } else {
        configure_fps_fixed_schedule(app);
    }
}

#[derive(SystemSet, Debug, Clone, Copy, Eq, PartialEq, Hash)]
enum FpsServerSet {
    WorldTick,
    PhysicsWorld,
    NetIngress,
    VehicleSim,
    AISim,
    PhysicsControl,
    PhysicsPost,
    PlayerSim,
    Indices,
    Persistence,
    Inventory,
}

fn configure_fps_fixed_schedule(app: &mut App) {
    app.configure_sets(
        FixedUpdate,
        (
            FpsServerSet::WorldTick,
            FpsServerSet::PhysicsWorld,
            FpsServerSet::NetIngress,
            FpsServerSet::VehicleSim,
            FpsServerSet::AISim,
            FpsServerSet::PhysicsControl,
            FpsServerSet::PhysicsPost,
            FpsServerSet::PlayerSim,
            FpsServerSet::Indices,
            FpsServerSet::Persistence,
            FpsServerSet::Inventory,
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
            vehicle::interaction::handle_vehicle_interaction_requests,
            vehicle::simulation::ensure_car_suspension_state,
            vehicle::simulation::update_vehicles,
            collision::resolve_vehicle::handle_vehicle_static_collisions,
        )
            .chain()
            .in_set(FpsServerSet::VehicleSim)
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
            inventory::hotbar::handle_hotbar_selection_requests,
            inventory::hotbar::handle_inventory_move_requests,
            inventory::ground_items::handle_pickup_requests,
            inventory::ground_items::handle_drop_requests,
            inventory::chest::handle_open_chest_requests,
            inventory::chest::handle_close_chest_requests,
            inventory::chest::handle_chest_transfer_requests,
            inventory::chest::update_distant_chest_auto_close,
        )
            .chain()
            .in_set(FpsServerSet::Inventory)
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
                .after(FpsServerSet::Inventory),
            telemetry::perf::update_server_perf_log.after(FpsServerSet::Inventory),
            telemetry::network::sample_replication_change_pressure.after(FpsServerSet::Inventory),
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

#[derive(SystemSet, Debug, Clone, Copy, Eq, PartialEq, Hash)]
enum RailServerSet {
    WorldTick,
    NetIngress,
    RailCommands,
    RailSim,
    Telemetry,
}

fn configure_rail_fixed_schedule(app: &mut App) {
    app.configure_sets(
        FixedUpdate,
        (
            RailServerSet::WorldTick,
            RailServerSet::NetIngress,
            RailServerSet::RailCommands,
            RailServerSet::RailSim,
            RailServerSet::Telemetry,
        )
            .chain(),
    );

    app.add_systems(
        FixedUpdate,
        (
            world::time::handle_set_time_of_day,
            world::time::update_world_time,
        )
            .chain()
            .in_set(RailServerSet::WorldTick)
            .run_if(server_is_started),
    );

    app.add_systems(
        FixedUpdate,
        (
            net::connection::handle_connections,
            rail::handle_company_name_submission,
            rail::handle_create_company_requests,
        )
            .chain()
            .in_set(RailServerSet::NetIngress)
            .run_if(server_is_started),
    );

    app.add_systems(
        FixedUpdate,
        (
            rail::handle_build_track_requests,
            rail::handle_build_station_requests,
            rail::handle_buy_train_requests,
            rail::handle_assign_route_requests,
            rail::handle_set_train_cargo_policy_requests,
            rail::handle_demolish_rail_requests,
        )
            .chain()
            .in_set(RailServerSet::RailCommands)
            .run_if(server_is_started),
    );

    app.add_systems(
        FixedUpdate,
        (rail::tick_economy, rail::update_train_movement)
            .chain()
            .in_set(RailServerSet::RailSim)
            .run_if(server_is_started),
    );

    app.add_systems(
        FixedUpdate,
        (
            telemetry::network::sample_replication_change_pressure,
            telemetry::perf::update_server_perf_log,
        )
            .chain()
            .in_set(RailServerSet::Telemetry)
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
