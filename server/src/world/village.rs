//! Villages that run themselves.
//!
//! God mode introduces people and founds halls. Everything after that is the
//! villagers' own doing: they choose where to live, they decide what the place
//! needs next, and they site their own buildings. No player assigns a resident,
//! an occupation or a plot.
//!
//! The first local monetary loop is also server truth: villagers have
//! fixed-point wallets, businesses consign physically delivered stock under a
//! stable seller identity, buyers pay the firm only when a sale clears, households
//! buy daily provisions, builders buy Wood or gather it themselves, and the Moot
//! earns fees while paying civic workers. Births, boats and remote trade remain
//! deferred.
//!
//! Everything in this module is server truth. Clients receive settlements and
//! buildings and draw them; they never decide anything.

pub mod ambient;
mod businesses;
pub(crate) mod civic;
mod commerce;
mod companies;
mod construction;
mod development_market;
mod economy;
mod employment;
pub mod history;
mod households;
mod moot_services;
pub(crate) mod mortality;
mod movement;
mod objectives;
mod planning;
mod population;
mod processing;
mod production;
mod property_market;
mod quarry;
#[cfg(test)]
mod scale_lab;
pub mod schedule;
mod settlement_economy;
pub mod strategic;
mod tavern;
mod trade_routes;
mod trades;

pub use businesses::{apply_business_events, review_business_management, BusinessEventQueue};
pub use civic::{
    collect_business_profit_taxes, ensure_civic_accounts, review_civic_policies, run_civic_payroll,
    sync_civic_market_policy,
};
pub(crate) use commerce::{automatic_owner_strategy, business_output, is_private_business};
pub use commerce::{
    ensure_business_economies, reconcile_work_statuses, run_business_payroll_and_owner_leisure,
    run_internal_deliveries, run_market_collections, staff_moot_hall_roles,
    sync_porter_cargo_capacity,
};
pub(crate) use companies::new_company_bundle;
pub use companies::{
    cleanup_empty_companies, ensure_companies, ensure_company_branches,
    post_site_capital_to_company, refresh_company_accounts, refund_company_escrows,
    review_company_finance, review_company_strategies, CompanyDividendQueue,
    CompanyEscrowRefundQueue,
};
pub(crate) use construction::ensure_market_ground_is_level;
pub use construction::{advance_construction, run_construction_material_logistics};
pub(crate) use development_market::minimum_startup_capital;
pub(crate) use economy::review_automatic_wage_offer;
pub use employment::{
    enforce_staffing_targets, fill_vacancies, review_automatic_staffing, review_worker_job_choices,
    sync_company_porters,
};
pub use households::{
    assign_households, ensure_households, run_household_schedules, run_household_shopping,
    update_household_budgets_and_pantries,
};
#[cfg(test)]
pub(crate) use moot_services::PermitPickupRoutine;
pub(crate) use moot_services::{
    advance_moot_service_queues, complete_moot_permit_pickups, run_moot_meal_collections,
    MootMealRoutine, MootQueueClock, MootQueueTicket, MootQueueTransit, MootServiceKind,
};
pub use mortality::{
    acquire_businesses_for_sale, advance_nutrition_health, apply_nutrition_condition,
    ensure_character_vitals, process_character_deaths, recover_orphaned_construction,
    MortalityLedger,
};
pub(crate) use movement::ensure_move_target;
use movement::stable_name_hash;
pub use objectives::sync_character_objectives;
#[cfg(test)]
pub(crate) use planning::find_site;
pub use planning::{consider_permits, find_fishing_site, PermitPlanningDiagnostics, FREEBOARD};
#[cfg(test)]
use planning::{find_site_with_plan, planned_road_access_path, slope_at};
pub(crate) use planning::{
    road_access_blockers_for_plot, validate_manual_plot, ManualPlotApproval,
};
pub use population::{
    arrive_at_settlement, recount_residents, seek_settlement, tag_villager_intent,
};
pub(crate) use processing::ProcessorWorkProgress;
pub use processing::{
    assign_processing_routines, run_processing_routines, sync_workplace_operations,
    ProcessingRoutine,
};
pub use production::sync_business_stock_targets;
pub(crate) use production::{
    automatic_opening_positions, estimated_staffed_unit_cost, farmer_seconds_per_wheat,
    fisher_seconds_per_food, livestock_seconds_per_meat, lumber_seconds_per_tree,
    lumber_tree_yield, maximum_viable_input_unit_price, process_available_cycles,
    processing_recipe, produce_livestock_cycles, quarry_seconds_per_stone, rated_daily_production,
    viable_processing_input_purchase, BusinessOperatingPlan, ProcessingRecipe,
    SELF_SUPPLY_TREE_YIELD,
};
pub use property_market::{publish_property_boards, remove_abandoned_businesses};
pub(crate) use quarry::QuarryWorkProgress;
pub use quarry::{assign_quarry_routines, run_quarry_routines, QuarryRoutine};
use settlement_economy::{buy_from_moot, sell_carried_to_moot};
pub use settlement_economy::{
    ensure_settlement_economies, ensure_village_finances, sync_public_market_storage,
    update_moot_market_targets, update_settlement_economies, SettlementEconomyRuntime,
};
pub use tavern::{
    assign_tavern_routines, ensure_tavern_services, refresh_character_day_plans,
    review_tavern_businesses, run_strategic_tavern_visits, run_tavern_routines, TavernVisitRoutine,
    TavernWorkerRoutine,
};
pub use trade_routes::{
    manage_company_trade_routes, post_civic_import_contracts, review_autonomous_merchant_trade,
    run_company_trade_routes, run_merchant_trade_routes, RegionalTradeIntelligence,
    TradeRouteRoutine,
};
pub(crate) use trades::lumber_plot_has_reachable_tree;
#[cfg(test)]
use trades::{
    advance_failed_tree_candidate, fishing_deck_points, tree_approach_start, TREE_APPROACH_ANGLES,
};
pub use trades::{
    assign_farmer_routines, assign_fishing_routines, assign_lumberjack_routines,
    ensure_farm_fields, ensure_fishing_piers, ensure_livestock_pastures, run_farmer_routines,
    run_fishing_routines, run_lumberjack_routines, sync_carried_load, sync_porter_cart_state,
};
use trades::{
    build_clip_facing, exterior_door_clearance_position, find_tree_for_cycle_cached,
    ground_distance, postpone_construction_store_route, postpone_construction_tree_search,
    TreeCandidateLookup, TreeWorkCandidateCache,
};

use bevy::ecs::system::SystemParam;
use bevy::platform::collections::{HashMap, HashSet};
use bevy::prelude::*;
use lightyear::prelude::{NetworkTarget, Replicate};

use shared::components::{
    BuildingDoorDemand, BuildingDoorUse, CharacterActivity, CharacterAttributes, CharacterDayPlan,
    CharacterKind, CharacterName, FarmField, FishingPier, Household, LivestockPasture,
    MootAdministration, Nutrition, Occupation, PlannedLeisure, PlannedLeisureStatus,
    PlayerPosition, PlayerRotation, Residence, RoadClass, Settlement, SettlementBuilding,
    SettlementBuildingKind, SettlementPolicies, VillageRoad, WorkStatus, WorkplaceOperation,
    WorldTime,
};
use shared::economy::{
    permit_price_with_subsidy, BusinessAccount, BusinessCondition, BusinessForSale,
    BusinessInputRule, BusinessLiquidation, BusinessManagementPolicy, BusinessPrivateInputRule,
    BusinessProcurementPolicy, BusinessSalePolicy, BusinessSourcingMode, BusinessStaffingPolicy,
    BusinessState, BusinessSupplyPolicy, BusinessWageClaim, BusinessWagePolicy, CarriedLoad, Good,
    GoodsInventory, HouseholdEconomy, MarketSeller, MootMarket, SettlementEconomy, TavernService,
    Wallet, WorkforceRequirements, BASIS_POINTS, FOOD_SECURITY_TARGET_DAYS, FOUNDING_DAILY_WAGE,
    MAXIMUM_BUSINESS_DAILY_WAGE, MINIMUM_BUSINESS_DAILY_WAGE, PENNIES_PER_COIN,
    PROPERTY_MARKET_EXPOSURE_DAYS, STARTING_TREASURY_MONEY,
};
use shared::region::{RegionCoord, SimLevel};
use shared::spatial::SpatialObstacleGrid;
use shared::terrain::{ChunkCoord, WorldTerrain};

use crate::collision::library::{DerivedColliderLibrary, StaticColliders};
use crate::player::hero::MoveTarget;
use crate::world::navgrid::VILLAGER_PROP_RADIUS;
use crate::world::regions::RegionRegistry;
use crate::world::village_roads::{
    NavigationRouteFailed, NavigationRoutePending, PlannedRoadAccess, RoadBuilderRoutine,
    RoadRequest, RouteWaypoint, TravelRoute, VillageRoadGraph,
};

type PermitBusyFilter = Or<(
    With<FarmerRoutine>,
    With<FishingRoutine>,
    With<LumberjackRoutine>,
    With<QuarryRoutine>,
    With<ProcessingRoutine>,
    With<MarketCollectionRoutine>,
    With<InternalDeliveryRoutine>,
    With<TradeRouteRoutine>,
    With<HouseholdShoppingRoutine>,
    With<MootQueueTicket>,
    With<MootMealRoutine>,
    With<TavernVisitRoutine>,
    With<TavernWorkerRoutine>,
    With<WorkplaceDoorTransit>,
    With<PierTraversal>,
)>;

/// Terrain and collision truth needed while choosing a plot. Keeping these
/// related resources in one system parameter leaves room for the rest of the
/// permit system's settlement queries within Bevy's system-parameter limit.
#[derive(SystemParam)]
pub struct PermitPlanningResources<'w, 's> {
    ids: ResMut<'w, crate::world::identity::WorldIdAllocator>,
    terrain: Option<Res<'w, WorldTerrain>>,
    colliders: Option<Res<'w, StaticColliders>>,
    derived: Option<Res<'w, DerivedColliderLibrary>>,
    diagnostics: Option<ResMut<'w, PermitPlanningDiagnostics>>,
    planned_road_accesses: Query<'w, 's, &'static PlannedRoadAccess>,
    hall_upgrades: Query<
        'w,
        's,
        (
            &'static shared::components::CivicHallUpgradeWorksite,
            &'static shared::components::BuildingOf,
            &'static GoodsInventory,
        ),
    >,
    trade_contracts: Query<'w, 's, &'static shared::components::CivicTradeContract>,
    merchant_demand: Option<Res<'w, trade_routes::RegionalMerchantDemand>>,
    permit_busy: Query<'w, 's, (), PermitBusyFilter>,
    portfolios: Query<
        'w,
        's,
        (
            &'static shared::components::OwnedBy,
            Option<&'static BusinessCondition>,
            Option<&'static BusinessForSale>,
        ),
    >,
    companies: Query<
        'w,
        's,
        (
            &'static shared::components::CompanyId,
            &'static shared::components::CompanyLeadership,
            &'static shared::economy::CompanyManagementPolicy,
            Option<&'static shared::components::CompanyOwnership>,
        ),
    >,
}

/// Give the derived moot hall the same obstacle/build-zone identity as every
/// replicated settlement building. A settlement entity is the hall entity;
/// without this derivation roads and the nav grid see an empty plot here.
pub fn claim_settlement_hall_obstacles(
    mut commands: Commands,
    halls: Query<(
        Entity,
        &Settlement,
        &PlayerPosition,
        Option<&PlayerRotation>,
        Option<&shared::components::CivicHallLevel>,
        Option<&shared::building::PlacedBuilding>,
        Option<&shared::building::BuildingPosition>,
    )>,
) {
    for (hall, settlement, position, rotation, level, placed, building_position) in halls.iter() {
        let level = level
            .copied()
            .unwrap_or_else(|| shared::components::CivicHallLevel::for_tier(settlement.tier));
        let desired = shared::building::PlacedBuilding {
            building_type: level.building_type(),
            rotation: rotation.map_or(0.0, |rotation| rotation.0),
        };
        if placed != Some(&desired) {
            commands.entity(hall).insert(desired);
        }
        if building_position.is_none_or(|current| current.0 != position.0) {
            commands
                .entity(hall)
                .insert(shared::building::BuildingPosition(position.0));
        }
    }
}

/// Real-time cadence for admitting a bounded batch of uncommitted villagers
/// into migration. Using world time here made unpausing at 25x hand hundreds
/// of people the same hall destination on one server tick.
const SEEK_INTERVAL: f32 = 0.25;
const MAX_MIGRATION_ADMISSIONS_PER_PASS: usize = 8;

/// Failed migration is route-planner work, so retry pacing follows real time,
/// not warped world time. Otherwise 100x turns a world-minute cooldown into a
/// new expensive route every 0.6 real seconds for every unreachable migrant.
const MIGRATION_RETRY_BASE_SECONDS: f64 = 10.0;
const MIGRATION_RETRY_MAX_SECONDS: f64 = 120.0;

/// Construction timber searches can fail permanently on islands and opposite
/// river banks. Their retry clock is real time so 100x never turns one missing
/// resource into hundreds of full route searches per second.
const TIMBER_RETRY_BASE_SECONDS: f64 = 2.0;
const TIMBER_RETRY_MAX_SECONDS: f64 = 60.0;

/// How often a settlement considers what it needs next.
const PERMIT_INTERVAL: f32 = 4.0;

/// How close a villager must get to the Hall forecourt before the visible line
/// takes ownership of their movement.
///
/// This is deliberately wider than the exact door offset. Requiring every
/// migrant to solve a route to one point on the threshold made the occasional
/// person on the far side of a crowded forecourt fail and cool down while
/// everyone beside them visibly queued. The queue already owns collision-aware
/// last-metre movement and its bounded counter fallback.
const ARRIVAL_RADIUS: f32 = 12.0;

/// Physical work-loop tuning. Prices decide whether a transfer can happen, but
/// walking, work duration and carried capacity still decide when it happens.
const WORK_REACH: f32 = 2.5;
const INDOOR_REST_SECONDS: f32 = 4.0;
// These are world-time work passes, not tiny transaction delays. Output rate is
// physical: field quality controls how much labour makes one Wheat, and a
// farmer works continuously until the shift ends instead of receiving a daily
// production allowance.
pub(crate) const CHOP_SECONDS: f32 = 40.0;
/// A worker preserves their job and carried cargo across transient commute
/// failures, but one unreachable hut must not pin them outside a cabin for an
/// entire day. A later shift can retry after roads or obstacles change.
const MAX_WORKPLACE_ROUTE_FAILURES: u8 = 3;
/// At 100% quality, one Wheat takes 2m50s of actual field work. The ordinary
/// 06:00-ish to 18:00 shift contains about 1,050 simulation seconds, so a
/// perfect field approaches six Wheat after allowing for short local trips.
/// A common 67% field takes about 4m14s and approaches four Wheat per shift.
const PERFECT_FIELD_SECONDS_PER_WHEAT: f32 = 170.0;
const FARM_CARRY_BATCH_UNITS: u32 = 2;
const FISH_CARRY_BATCH_UNITS: u32 = 2;
/// Daylight spans 06:00-22:00. A 75% cutoff ends ordinary work near 18:00,
/// leaving a visible evening for shopping, socialising and household tasks.
pub(crate) const WORKDAY_END_DAY_T: f32 = WorldTime::WORKDAY_END_DAY_T;
const TREE_MIN_DISTANCE: f32 = 10.0;
const TREE_MAX_DISTANCE: f32 = 120.0;
const DOOR_REACH: f32 = 0.4;
const DOOR_OPEN_SECONDS: f32 = 0.667;

/// Select the closest physical counter backed by one settlement-owned market
/// inventory. Callers provide only completed Marketplace entrances belonging
/// to that settlement; the Hall is always the safe fallback.
pub(crate) fn nearest_public_market_entrance(
    origin: Vec3,
    hall_entrance: Vec3,
    marketplace_entrances: impl IntoIterator<Item = Vec3>,
) -> Vec3 {
    marketplace_entrances
        .into_iter()
        .fold(hall_entrance, |nearest, candidate| {
            if ground_distance(origin, candidate) < ground_distance(origin, nearest) {
                candidate
            } else {
                nearest
            }
        })
}

/// How long a fully supplied building takes to raise, in seconds.
///
/// The timer represents builder work only. It cannot start until the site's
/// physical inventory contains the building's full wood requirement.
const BUILD_SECONDS: f32 = shared::components::SETTLEMENT_RAISE_SECONDS;

/// Where a villager is in the business of joining somewhere.
#[derive(Component, Debug, Clone, PartialEq)]
pub enum VillagerIntent {
    /// Knows of nowhere to go. Re-checks on the seek tick.
    Idle,
    /// Has chosen a settlement and is physically sailing into the world. This
    /// is not residence and does not enter land pathfinding until landfall.
    ArrivingBySea { settlement: Entity },
    /// Walking to a settlement's hall.
    Travelling { settlement: Entity },
    /// Lives somewhere. The hall is their lodging until houses exist.
    Resident { settlement: Entity },
    /// Living there AND away raising something they were granted a permit for.
    ///
    /// Carries the settlement as well as the site so a builder still counts as
    /// a resident while they are out working. Without that the population dips
    /// by one every time somebody starts a building, which would be a lie the
    /// panel tells for ten seconds at a time.
    Building { settlement: Entity, site: Entity },
    /// Building the physical path requested by a newly completed building.
    RoadBuilding { settlement: Entity, road: Entity },
}

/// One villager's most recent unreachable migration destination.
///
/// Server-only and deliberately bounded to one settlement: after a failure the
/// villager can immediately consider every other town, while the failed town
/// becomes eligible again after an exponentially bounded real-time cooldown.
#[derive(Component, Debug, Clone, Copy)]
pub struct MigrationCooldown {
    settlement: Entity,
    retry_after: f64,
    failures: u8,
    /// Cohort-route proof visible when this attempt failed. A larger value
    /// means somebody else has since established a fresh approach to the same
    /// hall and this villager should reconsider immediately.
    cohort_opportunity_version: u64,
}

impl MigrationCooldown {
    fn after_failure(
        previous: Option<Self>,
        settlement: Entity,
        now: f64,
        cohort_opportunity_version: u64,
    ) -> Self {
        let failures = previous
            .filter(|previous| previous.settlement == settlement)
            .map_or(1, |previous| previous.failures.saturating_add(1));
        let exponent = u32::from(failures.saturating_sub(1)).min(10);
        let delay = (MIGRATION_RETRY_BASE_SECONDS * 2_f64.powi(exponent as i32))
            .min(MIGRATION_RETRY_MAX_SECONDS);
        Self {
            settlement,
            retry_after: now + delay,
            failures,
            cohort_opportunity_version,
        }
    }

    fn blocks(self, settlement: Entity, now: f64, current_cohort_opportunity_version: u64) -> bool {
        self.settlement == settlement
            && now < self.retry_after
            && current_cohort_opportunity_version <= self.cohort_opportunity_version
    }
}

impl VillagerIntent {
    /// The settlement this villager belongs to, if any.
    pub fn settlement(&self) -> Option<Entity> {
        match self {
            VillagerIntent::Idle => None,
            VillagerIntent::ArrivingBySea { settlement } => Some(*settlement),
            VillagerIntent::Travelling { settlement } => Some(*settlement),
            VillagerIntent::Resident { settlement } => Some(*settlement),
            VillagerIntent::Building { settlement, .. } => Some(*settlement),
            VillagerIntent::RoadBuilding { settlement, .. } => Some(*settlement),
        }
    }

    /// Whether this villager is available to take on a new job or permit.
    pub fn is_settled(&self) -> bool {
        matches!(self, VillagerIntent::Resident { .. })
    }

    /// Whether this person is already part of a settlement's population.
    ///
    /// A builder or road builder is still a resident while away at work, but
    /// somebody merely travelling toward the hall is not one yet. Housing and
    /// the public population count must use this same boundary or cabins can
    /// become occupied by people the settlement does not count.
    pub fn counts_as_resident(&self) -> bool {
        matches!(
            self,
            VillagerIntent::Resident { .. }
                | VillagerIntent::Building { .. }
                | VillagerIntent::RoadBuilding { .. }
        )
    }
}

/// How far along a permitted building is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum BuildStage {
    /// Granted, but the material pile is not full. The assigned builder hauls
    /// stored wood or chops directly from a real nearby tree.
    Supplying,
    /// Granted. The builder is walking out to the plot, and nothing has
    /// happened to the ground yet. This begins only after supply is complete.
    Walking,
    /// The builder is on site. The plot has been cleared and levelled, and the
    /// frame is going up.
    Raising { seconds_left: f32 },
}

/// A building a settlement has approved and is waiting on.
#[derive(Component, Debug, Clone)]
pub struct UnderConstruction {
    pub kind: SettlementBuildingKind,
    pub position: Vec3,
    pub rotation: f32,
    pub owner: Option<String>,
    /// Stable private owner. `owner` is only the readable permit label.
    pub owner_id: Option<shared::components::PersonId>,
    /// Who is actually walking out there. Held as an entity rather than looked
    /// up by name because generated names repeat -- the first duplicate shows
    /// up around the fifty-first villager.
    pub builder: Option<Entity>,
    pub settlement: Entity,
    /// Durable settlement membership for save/region boundaries.
    pub settlement_id: shared::components::SettlementId,
    /// Where the builder stands to work. Arrival is judged against THIS, not
    /// the plot centre, or they would walk into the middle of the site.
    pub stand: Vec3,
    /// Failed final approaches rotate around the plot instead of pinning a
    /// fully supplied worksite to one obstructed hammering point forever.
    pub failed_stand_routes: u8,
    pub stage: BuildStage,
    /// How good this ground is for what is being built, 0..1. Sampled once,
    /// where it is built. See `site_quality`.
    pub quality: f32,
}

/// The assigned builder's physical material run for one approved worksite.
#[derive(Component, Debug, Clone)]
pub struct ConstructionMaterialRoutine {
    site: Entity,
    cycle: u32,
    failed_tree_routes: u8,
    failed_store_routes: u8,
    failed_delivery_routes: u8,
    tree_retry_after: f64,
    store_retry_after: f64,
    phase: ConstructionMaterialPhase,
}

/// A directly controlled hero's current private construction command.
///
/// Villagers continue to express construction through [`VillagerIntent`]. A
/// hero is not a settlement resident or municipal crew member, so forcing the
/// same intent onto them would make household and employment systems adopt the
/// player accidentally. Construction accepts either state at its narrow work
/// boundary instead.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerConstructionAssignment {
    pub site: Entity,
    pub settlement: Entity,
}

impl ConstructionMaterialRoutine {
    pub(crate) const fn new(site: Entity) -> Self {
        Self {
            site,
            cycle: 0,
            failed_tree_routes: 0,
            failed_store_routes: 0,
            failed_delivery_routes: 0,
            tree_retry_after: 0.0,
            store_retry_after: 0.0,
            phase: ConstructionMaterialPhase::Seeking,
        }
    }

    pub(crate) fn is_waiting_for_materials(&self) -> bool {
        matches!(self.phase, ConstructionMaterialPhase::Seeking)
    }
}

/// Working capital reserved for an unfinished private business. Permit
/// applicants fund it at approval and takeover buyers inherit it; the component
/// follows the worksite and becomes the completed firm's opening account.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct InheritedBusinessCapital(pub u64);

/// Accounting provenance carried by an unfinished private business. Opening
/// cash may be an owner's new contribution or a transfer from an existing
/// company's retained cash; the permit itself is a capitalised company asset,
/// never an operating expense.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct BusinessProjectAccounting {
    pub company: Option<shared::components::CompanyId>,
    pub contributed_capital: u64,
    pub capital_expenditure: u64,
}

#[derive(Debug, Clone, Copy)]
enum ConstructionMaterialPhase {
    Seeking,
    UnloadingAtHall { hall: Entity, entrance: Vec3 },
    CollectingFromStore { source: Entity, entrance: Vec3 },
    WalkingToTree { tree: Vec3, stand: Vec3 },
    Chopping { tree: Vec3, seconds_left: f32 },
    ApproachingDeliveryAccess { entry: Vec3 },
    Delivering { destination: Vec3 },
    LeavingDeliveryAccess { exit: Vec3 },
}

/// Server-only detail for one woodcutter's routine.
///
/// The client receives only [`CharacterActivity`] and [`CarriedLoad`]. Timers,
/// destinations and chosen trees are simulation truth and do not belong on the
/// network.
#[derive(Component, Debug, Clone)]
pub struct LumberjackRoutine {
    hut: Entity,
    hall: Entity,
    cycle: u32,
    failed_tree_routes: u8,
    failed_hut_routes: u8,
    chop_seconds: f32,
    production_day: u32,
    produced_today: u32,
    phase: LumberjackPhase,
}

impl LumberjackRoutine {
    pub(crate) const fn workplace(&self) -> Entity {
        self.hut
    }

    pub(crate) const fn hall(&self) -> Entity {
        self.hall
    }
}

#[derive(Debug, Clone, Copy)]
enum LumberjackPhase {
    GoingToHut,
    Inside { seconds_left: f32 },
    WalkingToTree { tree: Vec3, stand: Vec3 },
    Chopping,
    ReturningToHut,
    EndingShift,
}

#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct LumberjackWorkProgress {
    hut: Entity,
    cycle: u32,
    chop_seconds: f32,
}

#[derive(Component, Debug, Clone)]
pub struct FarmerRoutine {
    farmstead: Entity,
    field: Entity,
    hall: Entity,
    /// Door-to-field certified standing point selected once per shift.
    work_stand: Vec3,
    /// Productive seconds already invested in the next Wheat. This is copied
    /// into FarmerHarvestProgress when the shift ends and restored tomorrow.
    harvest_seconds: f32,
    failed_workplace_routes: u8,
    production_day: u32,
    produced_today: u32,
    phase: FarmerPhase,
}

impl FarmerRoutine {
    /// Exposes workplace identity to the deterministic Village Lab without
    /// publishing the rest of the routine's server-only state.
    pub(crate) fn farmstead(&self) -> Entity {
        self.farmstead
    }

    pub(crate) const fn hall(&self) -> Entity {
        self.hall
    }
}

#[derive(Debug, Clone, Copy)]
enum FarmerPhase {
    GoingToFarmstead,
    Inside { seconds_left: f32 },
    WalkingToField { stand: Vec3 },
    Farming,
    ReturningToFarmstead,
    EndingShift,
}

/// The unfinished part of a harvest survives evenings and days off without
/// keeping the active work routine attached (which would suppress cheap
/// ambient and household behaviour).
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct FarmerHarvestProgress {
    farmstead: Entity,
    field: Entity,
    seconds: f32,
}

/// A rostered worker who has completed today's job remains employed, but is
/// released to cheap ambient/household behaviour until the next workday.
#[derive(Component, Debug, Clone, Copy)]
pub struct WorkerOffDuty {
    day: u32,
}

/// Server-only fisher state. The replicated activity and inventory make every
/// important outcome visible without putting these implementation phases on
/// the wire.
#[derive(Component, Debug, Clone)]
pub struct FishingRoutine {
    hut: Entity,
    pier: Entity,
    hall: Entity,
    catch_seconds: f32,
    failed_workplace_routes: u8,
    production_day: u32,
    produced_today: u32,
    phase: FishingPhase,
}

impl FishingRoutine {
    pub(crate) const fn workplace(&self) -> Entity {
        self.hut
    }

    pub(crate) const fn hall(&self) -> Entity {
        self.hall
    }
}

#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct FishingWorkProgress {
    hut: Entity,
    pier: Entity,
    seconds: f32,
}

/// A founding public worker who collects market goods and maintains roads.
/// A solvent Hamlet can staff two people in this combined role.
#[derive(Component, Debug, Clone, Copy)]
pub struct MootSteward {
    pub(crate) settlement: Entity,
}

/// A private porter employed by a Storage Hall. It may move only its own
/// company's goods inside this settlement; unlike a Moot Steward it does not
/// maintain roads or collect freight for unrelated firms.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompanyPorter {
    pub(crate) settlement: Entity,
    pub(crate) settlement_id: shared::components::SettlementId,
    pub(crate) company: shared::components::CompanyId,
    pub(crate) storage_hall: shared::components::BuildingId,
}

/// Server-side marker for a character currently carrying a porter's cart
/// allowance. It remains briefly after an abnormal job removal if necessary
/// so reducing capacity can never destroy an in-flight load.
#[derive(Component, Debug, Clone, Copy, Default)]
pub(crate) struct PorterCargoCapacity;

#[derive(Component, Debug, Clone)]
pub struct MarketCollectionRoutine {
    business: Entity,
    seller: shared::components::BuildingId,
    hall: Entity,
    /// Physical Hall or Marketplace counter used for this trip. Economic
    /// authority remains `hall`; pinning the entrance prevents route flapping
    /// halfway through a delivery.
    counter: Vec3,
    good: Good,
    reserved_units: u32,
    unit_price: u64,
    phase: MarketCollectionPhase,
}

/// One private same-company shipment performed by a municipal Moot Steward.
/// Goods travel directly between workplaces and never become Hall inventory or
/// a public listing.
#[derive(Component, Debug, Clone)]
pub struct InternalDeliveryRoutine {
    supplier: Entity,
    supplier_id: shared::components::BuildingId,
    receiver: Entity,
    receiver_id: shared::components::BuildingId,
    company: shared::components::CompanyId,
    hall: Entity,
    good: Good,
    reserved_units: u32,
    unit_value: u64,
    municipal_fee: bool,
    phase: InternalDeliveryPhase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum InternalDeliveryPhase {
    GoingToSupplier,
    Delivering,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MarketCollectionPhase {
    GoingToBusiness,
    ReturningToHall,
    DeliveringInput,
    ReturningFailedInput,
}

/// A tactical-region household's one visible restocking trip. Strategic
/// households settle the identical transaction directly at their daily tick.
#[derive(Component, Debug, Clone, Copy)]
pub struct HouseholdShoppingRoutine {
    home: Entity,
    hall: Entity,
    /// Hall or Marketplace entrance chosen when this shopping trip begins.
    counter: Vec3,
    phase: HouseholdShoppingPhase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HouseholdShoppingPhase {
    GoingToMarket,
    ReturningHome,
}

#[derive(Debug, Clone, Copy)]
enum FishingPhase {
    GoingToHut,
    Inside {
        seconds_left: f32,
    },
    /// First reaches the safe front-side staging point using ordinary land
    /// navigation, then installs the authored route around the hut and pier.
    StagingForPier {
        staging: Vec3,
    },
    WalkingToPier,
    Fishing,
    ReturningFromPier {
        staging: Vec3,
    },
    ReturningToHut,
    EndingShift,
}

/// Temporarily marks an authored route that is allowed to cross the fishing
/// pier over water. Ordinary path planning ignores these movers, and movement
/// uses the deck plane instead of snapping their feet to the lake bed.
#[derive(Component, Debug, Clone, Copy)]
pub struct PierTraversal {
    deck_start: Vec3,
    deck_end: Vec3,
}

impl PierTraversal {
    pub fn deck_height_at(self, point: Vec2) -> Option<f32> {
        let start = Vec2::new(self.deck_start.x, self.deck_start.z);
        let end = Vec2::new(self.deck_end.x, self.deck_end.z);
        let axis = end - start;
        let length_squared = axis.length_squared();
        if length_squared <= 1e-4 {
            return None;
        }
        let t = (point - start).dot(axis) / length_squared;
        if !(-0.04..=1.08).contains(&t) {
            return None;
        }
        let closest = start + axis * t.clamp(0.0, 1.0);
        (point.distance(closest) <= 1.05).then_some(
            self.deck_start.y + (self.deck_end.y - self.deck_start.y) * t.clamp(0.0, 1.0),
        )
    }
}

/// Shared threshold choreography for staffed buildings. Job routines choose
/// what comes after the threshold; this component owns only opening the leaf
/// and walking the short exterior/interior segment without teleporting.
#[derive(Component, Debug, Clone, Copy)]
pub struct WorkplaceDoorTransit {
    building: Vec3,
    door: Vec3,
    inside: Vec3,
    direction: WorkplaceDoorDirection,
    phase: WorkplaceDoorPhase,
    destination_after_exit: Option<Vec3>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum WorkplaceDoorDirection {
    Entering,
    Leaving,
}

#[derive(Debug, Clone, Copy)]
enum WorkplaceDoorPhase {
    Opening { seconds_left: f32 },
    Crossing,
}

pub(super) fn begin_workplace_entry(
    commands: &mut Commands,
    worker: Entity,
    building: Vec3,
    door: Vec3,
    inside: Vec3,
) {
    commands
        .entity(worker)
        .remove::<MoveTarget>()
        .remove::<TravelRoute>()
        .remove::<NavigationRoutePending>()
        .remove::<NavigationRouteFailed>()
        .insert(WorkplaceDoorTransit {
            building,
            door,
            inside,
            direction: WorkplaceDoorDirection::Entering,
            phase: WorkplaceDoorPhase::Opening {
                seconds_left: DOOR_OPEN_SECONDS,
            },
            destination_after_exit: None,
        });
}

pub(super) fn begin_workplace_exit(
    commands: &mut Commands,
    worker: Entity,
    building: Vec3,
    door: Vec3,
    inside: Vec3,
    destination: Vec3,
) {
    commands
        .entity(worker)
        .remove::<MoveTarget>()
        .remove::<TravelRoute>()
        .remove::<NavigationRoutePending>()
        .remove::<NavigationRouteFailed>()
        .insert(WorkplaceDoorTransit {
            building,
            door,
            inside,
            direction: WorkplaceDoorDirection::Leaving,
            phase: WorkplaceDoorPhase::Opening {
                seconds_left: DOOR_OPEN_SECONDS,
            },
            destination_after_exit: Some(destination),
        });
}

/// Keep the job system paused while the worker crosses the threshold, so its
/// destination cannot drag them through the wall while the door is opening.
pub fn run_workplace_door_transits(
    simulation_time: crate::world::simulation_time::SimulationTime,
    obstacles: Option<Res<SpatialObstacleGrid>>,
    mut commands: Commands,
    mut workers: Query<
        (
            Entity,
            &PlayerPosition,
            &VillagerIntent,
            Option<&mut HomeRoutine>,
            &mut PlayerRotation,
            &mut CharacterActivity,
            &mut WorkplaceDoorTransit,
            Option<&MoveTarget>,
        ),
        (With<CharacterKind>, Without<strategic::StrategicPerson>),
    >,
) {
    let dt = simulation_time.world_seconds();
    for (worker, position, intent, mut home, mut facing, mut activity, mut transit, move_target) in
        workers.iter_mut()
    {
        let leaving_for_home = home
            .as_deref()
            .is_some_and(|home| home.phase == HomePhase::LeavingWorkplace);
        // A resident can receive a construction or road task while still
        // crossing a workplace threshold. Cancelling the paired transit just
        // because their intent is no longer the idle Resident variant leaves
        // HomeRoutine::LeavingWorkplace with no system able to finish it.
        let invalid_commitment = if leaving_for_home {
            !intent.counts_as_resident()
        } else {
            !intent.is_settled()
        };
        if (home.is_some() && !leaving_for_home) || invalid_commitment {
            commands
                .entity(worker)
                .remove::<WorkplaceDoorTransit>()
                .remove::<BuildingDoorUse>();
            continue;
        }

        let door_use = BuildingDoorUse {
            building: transit.building,
        };
        commands.entity(worker).insert(door_use);
        match transit.phase {
            WorkplaceDoorPhase::Opening { seconds_left } => {
                let target = match transit.direction {
                    WorkplaceDoorDirection::Entering => transit.inside,
                    WorkplaceDoorDirection::Leaving => {
                        exterior_door_clearance_position(transit.building, transit.door)
                    }
                };
                let to_target = target - position.0;
                if to_target.length_squared() > 1e-4 {
                    facing.0 = build_clip_facing(to_target);
                }
                let left = seconds_left - dt;
                if left > 0.0 {
                    transit.phase = WorkplaceDoorPhase::Opening { seconds_left: left };
                    continue;
                }
                *activity = CharacterActivity::Idle;
                commands.entity(worker).insert(MoveTarget(target));
                transit.phase = WorkplaceDoorPhase::Crossing;
            }
            WorkplaceDoorPhase::Crossing => {
                let target = match transit.direction {
                    WorkplaceDoorDirection::Entering => transit.inside,
                    WorkplaceDoorDirection::Leaving => {
                        exterior_door_clearance_position(transit.building, transit.door)
                    }
                };
                *activity = CharacterActivity::Idle;
                // The entrance marker itself is outside the inflated building
                // blocker, but DOOR_REACH extends slightly back through the
                // wall. Do not release collision immunity from a leaving
                // worker merely because they are close to the door: at that
                // point their next ordinary route would begin inside the
                // blocker and can never certify. Walk the final few
                // centimetres until their actual position is outside too.
                let still_inside_blocker = transit.direction == WorkplaceDoorDirection::Leaving
                    && obstacles.as_deref().is_some_and(|grid| {
                        grid.point_blocked(Vec2::new(position.0.x, position.0.z))
                    });
                if ground_distance(position.0, target) > DOOR_REACH || still_inside_blocker {
                    ensure_move_target(&mut commands, worker, move_target, target);
                    continue;
                }

                let destination = transit.destination_after_exit;
                if transit.direction == WorkplaceDoorDirection::Entering {
                    *activity = CharacterActivity::Indoors;
                }
                let mut worker_commands = commands.entity(worker);
                worker_commands
                    .remove::<WorkplaceDoorTransit>()
                    .remove::<BuildingDoorUse>()
                    .remove::<MoveTarget>();
                if let Some(destination) = destination {
                    worker_commands.insert(MoveTarget(destination));
                }
                if leaving_for_home {
                    home.as_deref_mut().expect("checked above").phase = HomePhase::GoingToDoor;
                }
            }
        }
    }
}

/// Fold short-lived actor requests into one stable replicated value per door.
///
/// Replicating only [`BuildingDoorUse`] made the visual depend on the client
/// observing a transient component on an actor at exactly the right network
/// snapshot. A door belongs to its building, so its aggregate demand lives on
/// that stable entity and remains present even while its boolean is false.
pub fn sync_building_door_demands(
    mut commands: Commands,
    uses: Query<&BuildingDoorUse>,
    mut buildings: Query<
        (Entity, &PlayerPosition, Option<&mut BuildingDoorDemand>),
        Or<(With<Settlement>, With<SettlementBuilding>)>,
    >,
) {
    let requested: HashSet<[u32; 3]> = uses
        .iter()
        .map(|request| request.building.to_array().map(f32::to_bits))
        .collect();
    for (entity, position, demand) in buildings.iter_mut() {
        let open = requested.contains(&position.0.to_array().map(f32::to_bits));
        if let Some(mut demand) = demand {
            if demand.open != open {
                debug!(
                    "Building door at {:.1},{:.1} demand {}",
                    position.0.x,
                    position.0.z,
                    if open { "OPEN" } else { "CLOSED" }
                );
                demand.open = open;
            }
        } else {
            if open {
                debug!(
                    "Building door at {:.1},{:.1} demand OPEN",
                    position.0.x, position.0.z
                );
            }
            commands.entity(entity).insert(BuildingDoorDemand { open });
        }
    }
}

/// Night temporarily owns a villager's destination while preserving their
/// employment routine for morning.
#[derive(Component, Debug, Clone, Copy)]
pub struct HomeRoutine {
    home: Entity,
    phase: HomePhase,
    /// Consecutive terminal routes while satisfying this night's shelter need.
    /// Ordinary retries remain embodied; the bounded fallback prevents one
    /// impossible local route from leaving a resident outdoors forever.
    failed_routes: u8,
}

/// Server-side identity-safe link from one villager entity to one cabin.
/// Household names are for inspection; this entity link is what keeps two
/// people with the same generated name from stealing each other's bed.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct HomeAssignment {
    home: Entity,
}

impl HomeAssignment {
    pub(crate) const fn home(&self) -> Entity {
        self.home
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum HomePhase {
    /// Sunset found this villager inside (or crossing) a workplace. The
    /// workplace threshold owns movement until they are outside; only then may
    /// the ordinary route to the cabin begin.
    LeavingWorkplace,
    GoingToDoor,
    OpeningToEnter {
        seconds_left: f32,
    },
    Entering,
    Sleeping,
    OpeningToLeave {
        seconds_left: f32,
    },
    Leaving,
}

/// How close the builder must get to their plot before work starts.
const BUILD_REACH: f32 = 4.0;

/// Which chunk each published terrain delta belongs to.
///
/// One replicated entity per chunk, reused. Spawning a fresh one per edit would
/// give a chunk two competing authorities and the client would apply whichever
/// arrived last.
#[derive(Resource, Default)]
pub struct PublishedTerrainDeltas {
    pub by_chunk: bevy::platform::collections::HashMap<shared::terrain::ChunkCoord, Entity>,
}

/// Paces the two decision ticks.
#[derive(Resource)]
pub struct VillageClock {
    seek: f32,
    permit: f32,
    /// Monotonic permit-review sequence. Resource searches defer themselves
    /// for a few reviews after spending their bounded terrain budget so one
    /// difficult farm plot cannot monopolise every development decision.
    permit_round: u64,
    deferred_opportunities: HashMap<(Entity, SettlementBuildingKind), u64>,
    /// An unchanged village cannot make an unchanged failed plot search
    /// succeed. Remember that exact geometry until buildings, roads, or edited
    /// terrain change instead of rescanning and logging the
    /// same failure every four simulated seconds at 100x. Population is not
    /// geometry: immigration alone must not invalidate this cache.
    failed_site_searches: HashMap<Entity, FailedSiteSearch>,
    /// Last successful outward ring for each settlement/building kind.
    /// Completed plots never relocate or free their ground, so restarting a
    /// mature Farmstead search at its founding ring is pure repeated work.
    site_search_radii: HashMap<(Entity, SettlementBuildingKind), f32>,
    /// A coastline that has been exhausted ring by ring cannot become a
    /// fishing site merely because another inland house was completed. Retry
    /// only after edited terrain changes the physical shoreline.
    failed_fishing_terrain_versions: HashMap<Entity, u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FailedSiteSearch {
    kind: SettlementBuildingKind,
    occupied_plots: usize,
    roads: usize,
    terrain_version: u32,
}

impl Default for VillageClock {
    fn default() -> Self {
        Self {
            seek: 0.0,
            permit: 0.0,
            permit_round: 0,
            deferred_opportunities: HashMap::new(),
            failed_site_searches: HashMap::new(),
            site_search_radii: HashMap::new(),
            failed_fishing_terrain_versions: HashMap::new(),
        }
    }
}
#[cfg(test)]
mod tests;
