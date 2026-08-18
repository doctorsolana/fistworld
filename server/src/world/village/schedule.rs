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

/// Ordered subdivisions of the authoritative village core. Besides making the
/// large domain schedule readable, these boundaries let the lab and live
/// telemetry identify a slow economy, construction or activity pass without
/// changing the production system list.
#[derive(SystemSet, Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum VillageCoreSet {
    IdentityPopulation,
    Civic,
    EconomyPlanning,
    Construction,
    Activity,
    Directory,
}

#[derive(SystemSet, Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum VillageEconomySet {
    MarketsBusinesses,
    Households,
    SettlementAccounts,
    Permits,
}

#[derive(SystemSet, Debug, Clone, Copy, Eq, PartialEq, Hash)]
pub enum VillageConstructionSet {
    MootServices,
    MaterialLogistics,
    BuildingProgress,
    Fields,
    RoadPlanning,
    Employment,
}

pub fn configure_shared_village_simulation<M: ScheduleLabel + Clone>(app: &mut App, schedule: M) {
    app.init_resource::<world::simulation_time::SimulationDelta>();
    app.init_resource::<super::BusinessEventQueue>();
    app.init_resource::<super::CompanyDividendQueue>();
    app.init_resource::<super::CompanyEscrowRefundQueue>();
    app.init_resource::<super::MootQueueClock>();
    app.init_resource::<super::MortalityLedger>();
    app.init_resource::<super::RegionalTradeIntelligence>();
    app.init_resource::<super::trade_routes::RegionalMerchantDemand>();
    app.configure_sets(
        schedule.clone(),
        (
            VillageSimulationSet::Time,
            VillageSimulationSet::Core,
            VillageSimulationSet::Navigation,
        )
            .chain(),
    );
    app.configure_sets(
        schedule.clone(),
        (
            VillageConstructionSet::MootServices,
            VillageConstructionSet::MaterialLogistics,
            VillageConstructionSet::BuildingProgress,
            VillageConstructionSet::Fields,
            VillageConstructionSet::RoadPlanning,
            VillageConstructionSet::Employment,
        )
            .chain()
            .in_set(VillageCoreSet::Construction),
    );
    app.configure_sets(
        schedule.clone(),
        (
            VillageCoreSet::IdentityPopulation,
            VillageCoreSet::Civic,
            VillageCoreSet::EconomyPlanning,
            VillageCoreSet::Construction,
            VillageCoreSet::Activity,
            VillageCoreSet::Directory,
        )
            .chain()
            .in_set(VillageSimulationSet::Core),
    );
    app.configure_sets(
        schedule.clone(),
        (
            VillageEconomySet::MarketsBusinesses,
            VillageEconomySet::Households,
            VillageEconomySet::SettlementAccounts,
            VillageEconomySet::Permits,
        )
            .chain()
            .in_set(VillageCoreSet::EconomyPlanning),
    );

    app.add_systems(
        schedule.clone(),
        world::simulation_time::refresh_simulation_delta.in_set(VillageSimulationSet::Time),
    );

    app.add_systems(
        schedule.clone(),
        (
            (
                (
                    player::hero::ensure_character_attributes,
                    super::ensure_character_vitals,
                ),
                world::identity::assign_stable_world_ids,
                world::identity::rebuild_world_identity_index,
                world::village_lab_scenario::ensure_merchant_beacon_marketplace,
                super::tag_villager_intent,
                super::seek_settlement,
                super::arrive_at_settlement,
                world::identity::reconcile_stable_world_relationships,
                world::identity::reconcile_stable_adjunct_relationships,
                world::identity::reconcile_stable_road_relationships,
                super::recount_residents,
                super::ensure_village_finances,
                super::ensure_civic_accounts,
                super::ensure_settlement_economies,
                super::ensure_companies,
                super::ensure_business_economies,
                super::ensure_tavern_services,
                super::post_site_capital_to_company,
                super::ensure_company_branches,
                super::cleanup_empty_companies,
            )
                .chain()
                .in_set(VillageCoreSet::IdentityPopulation),
            (
                world::village_roads::ensure_moot_administrations,
                world::settlement_development::ensure_settlement_developments,
                // Migrate old named civic rosters before staffing validates
                // them against durable assignments.
                world::identity::reconcile_stable_civic_employment,
                world::village_roads::staff_moot_stewards,
                super::staff_moot_hall_roles,
                world::village_roads::staff_public_positions,
                super::run_civic_payroll,
                super::reconcile_work_statuses,
                world::village_roads::audit_village_roads,
            )
                .chain()
                .in_set(VillageCoreSet::Civic),
            (
                (
                    super::sync_civic_market_policy,
                    super::sync_public_market_storage,
                    super::update_moot_market_targets,
                    super::refund_company_escrows,
                    super::run_business_payroll_and_owner_leisure,
                    super::collect_business_profit_taxes,
                    super::review_company_strategies,
                    world::village_lab_scenario::maintain_merchant_beacon_supply,
                    super::review_autonomous_merchant_trade,
                    super::review_tavern_businesses,
                    super::review_business_management,
                    super::review_company_finance,
                    super::acquire_businesses_for_sale,
                    super::remove_abandoned_businesses,
                )
                    .chain()
                    .in_set(VillageEconomySet::MarketsBusinesses),
                (
                    super::ensure_households,
                    super::assign_households,
                    super::update_household_budgets_and_pantries,
                )
                    .chain()
                    .in_set(VillageEconomySet::Households),
                (
                    super::apply_business_events,
                    super::refresh_company_accounts,
                    super::update_settlement_economies,
                    super::apply_nutrition_condition,
                    super::advance_nutrition_health,
                    super::process_character_deaths,
                    super::publish_property_boards,
                    super::review_civic_policies,
                    super::history::capture_settlement_history,
                )
                    .chain()
                    .in_set(VillageEconomySet::SettlementAccounts),
                super::consider_permits.in_set(VillageEconomySet::Permits),
            )
                .chain()
                .in_set(VillageCoreSet::EconomyPlanning),
            (
                (
                    super::advance_moot_service_queues,
                    super::complete_moot_permit_pickups,
                    super::run_moot_meal_collections,
                )
                    .chain()
                    .in_set(VillageConstructionSet::MootServices),
                (
                    super::recover_orphaned_construction,
                    super::run_construction_material_logistics,
                )
                    .chain()
                    .in_set(VillageConstructionSet::MaterialLogistics),
                super::advance_construction.in_set(VillageConstructionSet::BuildingProgress),
                (super::ensure_farm_fields, super::ensure_livestock_pastures)
                    .in_set(VillageConstructionSet::Fields),
                world::village_roads::plan_requested_roads
                    .in_set(VillageConstructionSet::RoadPlanning),
                (
                    super::review_automatic_staffing,
                    super::enforce_staffing_targets,
                    super::review_worker_job_choices,
                    super::fill_vacancies,
                    super::sync_company_porters,
                )
                    .chain()
                    .in_set(VillageConstructionSet::Employment),
            )
                .in_set(VillageCoreSet::Construction),
            (
                super::sync_porter_cargo_capacity,
                super::refresh_character_day_plans,
                super::run_household_schedules,
                super::run_workplace_door_transits,
                world::village_roads::build_village_roads,
                world::settlement_development::upgrade_town_roads,
                world::settlement_development::run_civic_hall_upgrade_projects,
                super::post_civic_import_contracts,
                world::settlement_development::update_settlement_developments,
                world::settlement_development::sync_civic_hall_levels,
                world::settlement_development::sync_market_levels,
                super::ensure_market_ground_is_level,
                (
                    (
                        super::ensure_fishing_piers,
                        super::assign_farmer_routines,
                        super::assign_fishing_routines,
                        super::assign_lumberjack_routines,
                        super::assign_quarry_routines,
                        super::assign_processing_routines,
                        super::assign_tavern_routines,
                    )
                        .chain(),
                    (
                        super::run_household_shopping,
                        super::run_farmer_routines,
                        super::run_fishing_routines,
                        super::run_lumberjack_routines,
                        super::run_quarry_routines,
                        super::run_processing_routines,
                        super::run_tavern_routines,
                        super::run_strategic_tavern_visits,
                        super::sync_workplace_operations,
                        super::sync_business_stock_targets,
                    )
                        .chain(),
                    (
                        super::manage_company_trade_routes,
                        super::run_company_trade_routes,
                        super::run_merchant_trade_routes,
                        super::run_internal_deliveries,
                        super::run_market_collections,
                    )
                        .chain(),
                    super::ambient::run_ambient_routines,
                    super::apply_business_events,
                    player::hero::sync_hero_attributes_to_player_progression,
                    (
                        super::sync_carried_load,
                        super::sync_porter_cart_state,
                        super::sync_building_door_demands,
                    )
                        .chain(),
                )
                    .chain(),
                super::sync_character_objectives,
            )
                .chain()
                .in_set(VillageCoreSet::Activity),
            (
                world::settlement_directory::tag_settlement_detail_regions,
                world::settlement_directory::sync_settlement_directory,
            )
                .chain()
                .in_set(VillageCoreSet::Directory),
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
