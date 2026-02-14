//! Fixed-update schedule sets and system wiring.

use bevy::ecs::schedule::SystemSet;
use bevy::prelude::*;
use lightyear::connection::ConnectionSystems;
use lightyear::link::LinkSystems;

use crate::ai;
use crate::collision;
use crate::combat;
use crate::inventory;
use crate::net;
use crate::persistence;
use crate::player;
use crate::telemetry;
use crate::vehicle;
use crate::world;

use super::bootstrap::server_is_started;

#[derive(SystemSet, Debug, Clone, Copy, Eq, PartialEq, Hash)]
enum FixedServerSet {
    WorldTick,
    NetIngress,
    VehicleSim,
    PlayerSim,
    Indices,
    Persistence,
    AISim,
    Collision,
    Inventory,
    Combat,
}

pub(crate) fn configure_fixed_schedule(app: &mut App) {
    app.configure_sets(
        FixedUpdate,
        (
            FixedServerSet::WorldTick,
            FixedServerSet::NetIngress,
            FixedServerSet::VehicleSim,
            FixedServerSet::PlayerSim,
            FixedServerSet::Indices,
            FixedServerSet::Persistence,
            FixedServerSet::AISim,
            FixedServerSet::Collision,
            FixedServerSet::Inventory,
            FixedServerSet::Combat,
        )
            .chain(),
    );

    app.add_systems(
        FixedUpdate,
        (
            world::time::handle_set_time_of_day,
            world::time::update_world_time,
            collision::building_index::sync_building_spatial_index,
            collision::streaming::update_static_collider_streaming,
        )
            .chain()
            .in_set(FixedServerSet::WorldTick)
            .run_if(server_is_started),
    );

    app.add_systems(
        FixedUpdate,
        (
            net::connection::handle_connections,
            player::spawn::handle_player_name_submission,
            player::spawn::handle_set_player_character,
            ai::spawn::handle_spawn_oilman_debug,
            player::roster::handle_player_roster_requests,
            net::input::handle_client_input_messages,
        )
            .chain()
            .in_set(FixedServerSet::NetIngress)
            .run_if(server_is_started),
    );

    app.add_systems(
        FixedUpdate,
        (
            vehicle::simulation::ensure_car_suspension_state,
            vehicle::interaction::handle_vehicle_interaction_requests,
            vehicle::simulation::update_vehicles,
        )
            .chain()
            .in_set(FixedServerSet::VehicleSim)
            .run_if(server_is_started),
    );

    app.add_systems(
        FixedUpdate,
        (
            player::movement::update_players,
            player::lifecycle::handle_player_deaths,
            player::lifecycle::update_respawn_timers,
        )
            .chain()
            .in_set(FixedServerSet::PlayerSim)
            .run_if(server_is_started),
    );

    app.add_systems(
        FixedUpdate,
        (
            player::index::sync_player_entity_index,
            player::spatial::sync_player_spatial_index,
        )
            .chain()
            .in_set(FixedServerSet::Indices)
            .run_if(server_is_started),
    );

    app.add_systems(
        FixedUpdate,
        (
            persistence::autosave::update_periodic_player_save,
            persistence::io_queue::update_profile_io_acks,
        )
            .chain()
            .in_set(FixedServerSet::Persistence)
            .run_if(server_is_started),
    );

    app.add_systems(
        FixedUpdate,
        (
            ai::obstacles::sync_obstacle_grid,
            ai::tick::handle_npc_damage_events,
            ai::tick::update_npc_ai,
            ai::death_cleanup::ensure_dead_npc_despawn_timers,
            ai::death_cleanup::update_dead_npc_despawn_timers,
        )
            .chain()
            .in_set(FixedServerSet::AISim)
            .run_if(server_is_started),
    );

    app.add_systems(
        FixedUpdate,
        (
            collision::resolve_vehicle::handle_vehicle_static_collisions,
            collision::resolve_player::handle_player_static_collisions,
            collision::resolve_npc::handle_npc_static_collisions,
        )
            .chain()
            .in_set(FixedServerSet::Collision)
            .run_if(server_is_started),
    );

    app.add_systems(
        FixedUpdate,
        (
            inventory::hotbar::handle_hotbar_selection_requests,
            inventory::hotbar::handle_inventory_move_requests,
            inventory::ground_items::handle_pickup_requests,
            inventory::ground_items::handle_drop_requests,
            inventory::hotbar::sync_equipped_weapon_from_hotbar,
            inventory::chest::handle_open_chest_requests,
            inventory::chest::handle_close_chest_requests,
            inventory::chest::handle_chest_transfer_requests,
            inventory::chest::update_distant_chest_auto_close,
        )
            .chain()
            .in_set(FixedServerSet::Inventory)
            .run_if(server_is_started),
    );

    app.add_systems(
        FixedUpdate,
        (
            combat::target_index::sync_hittable_spatial_index,
            combat::reload::update_reload_timers,
            combat::reload::handle_reload_request,
            combat::fire::handle_shoot_requests,
            combat::bullet_sim::update_bullets,
            combat::hit_characters::handle_bullet_character_hits,
            combat::hit_world::handle_bullet_world_hits,
            combat::cleanup::cleanup_bullets,
            inventory::death_drop::handle_inventory_drop_on_death,
        )
            .chain()
            .in_set(FixedServerSet::Combat)
            .run_if(server_is_started),
    );

    app.add_systems(
        FixedUpdate,
        (
            telemetry::perf::handle_perf_tick_begin.before(world::time::handle_set_time_of_day),
            telemetry::perf::handle_perf_core_phase_begin
                .before(world::time::handle_set_time_of_day),
            telemetry::perf::handle_perf_core_phase_end
                .after(persistence::autosave::update_periodic_player_save),
            telemetry::perf::handle_perf_npc_inventory_build_phase_begin
                .before(ai::obstacles::sync_obstacle_grid),
            telemetry::perf::handle_perf_npc_inventory_build_phase_end
                .after(inventory::chest::update_distant_chest_auto_close),
            telemetry::perf::handle_perf_weapons_phase_begin
                .before(combat::reload::update_reload_timers),
            telemetry::perf::handle_perf_weapons_phase_end
                .after(inventory::death_drop::handle_inventory_drop_on_death),
            telemetry::perf::update_server_perf_log
                .after(inventory::death_drop::handle_inventory_drop_on_death),
            telemetry::network::sample_replication_change_pressure
                .after(inventory::death_drop::handle_inventory_drop_on_death),
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
