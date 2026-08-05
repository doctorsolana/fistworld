//! Shared registration for the live server and deterministic Village Lab.
//!
//! Behavioural tests must execute the same ordered systems as production. The
//! schedule label differs (`FixedUpdate` live, `Update` in the headless lab),
//! but this is the only list of village and local-navigation systems.

use bevy::ecs::schedule::ScheduleLabel;
use bevy::prelude::*;

use crate::{player, world};

#[derive(SystemSet, Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum VillageSimulationSet {
    Time,
    Core,
    Navigation,
}

pub fn configure_shared_village_simulation<M: ScheduleLabel + Clone>(app: &mut App, schedule: M) {
    app.init_resource::<world::simulation_time::SimulationDelta>();
    app.configure_sets(
        schedule.clone(),
        (
            VillageSimulationSet::Time,
            VillageSimulationSet::Core,
            VillageSimulationSet::Navigation,
        )
            .chain(),
    );

    app.add_systems(
        schedule.clone(),
        world::simulation_time::refresh_simulation_delta.in_set(VillageSimulationSet::Time),
    );

    app.add_systems(
        schedule.clone(),
        (
            (
                player::hero::ensure_character_attributes,
                world::identity::assign_stable_world_ids,
                world::identity::rebuild_world_identity_index,
                super::tag_villager_intent,
                super::seek_settlement,
                super::arrive_at_settlement,
                world::identity::reconcile_stable_world_relationships,
                world::identity::reconcile_stable_adjunct_relationships,
                world::identity::reconcile_stable_road_relationships,
                super::recount_residents,
                super::ensure_village_finances,
                super::ensure_settlement_economies,
                super::ensure_business_economies,
            )
                .chain(),
            (
                world::village_roads::ensure_moot_administrations,
                world::settlement_development::ensure_settlement_developments,
                // Migrate old named civic rosters before staffing validates
                // them against durable assignments.
                world::identity::reconcile_stable_civic_employment,
                world::village_roads::staff_and_pay_road_stewards,
                super::staff_moot_hall_roles,
                world::village_roads::staff_public_positions,
                super::reconcile_work_statuses,
                world::village_roads::audit_village_roads,
            )
                .chain(),
            (
                super::update_moot_market_targets,
                super::ensure_households,
                super::assign_households,
                super::update_household_budgets_and_pantries,
                super::update_settlement_economies,
                super::run_business_payroll_and_owner_leisure,
                super::history::capture_settlement_history,
                super::consider_permits,
            )
                .chain(),
            super::run_construction_material_logistics,
            super::advance_construction,
            super::ensure_farm_fields,
            world::village_roads::plan_requested_roads,
            super::fill_vacancies,
            super::run_household_schedules,
            super::run_workplace_door_transits,
            world::village_roads::build_village_roads,
            world::settlement_development::upgrade_town_roads,
            world::settlement_development::update_settlement_developments,
            (
                super::ensure_fishing_piers,
                super::assign_farmer_routines,
                super::assign_fishing_routines,
                super::assign_lumberjack_routines,
                super::run_household_shopping,
                super::run_farmer_routines,
                super::run_fishing_routines,
                super::run_lumberjack_routines,
                super::run_market_collections,
                super::ambient::run_ambient_routines,
                super::settle_pending_market_payments,
                player::hero::sync_hero_attributes_to_player_progression,
                (super::sync_carried_load, super::sync_building_door_demands).chain(),
            )
                .chain(),
            (
                world::settlement_directory::tag_settlement_detail_regions,
                world::settlement_directory::sync_settlement_directory,
            )
                .chain(),
        )
            .chain()
            .in_set(VillageSimulationSet::Core),
    );

    app.add_systems(
        schedule,
        (
            world::village_roads::rebuild_village_road_graph,
            world::village_roads::queue_villager_travel_routes,
            world::village_roads::retry_failed_routes_after_obstacle_change,
            world::village_roads::plan_villager_travel_routes,
            player::hero::step_units,
        )
            .chain()
            .in_set(VillageSimulationSet::Navigation),
    );
}
