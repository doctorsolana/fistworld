//! Villages that run themselves.
//!
//! God mode introduces people and founds halls. Everything after that is the
//! villagers' own doing: they choose where to live, they decide what the place
//! needs next, and they site their own buildings. No player assigns a resident,
//! an occupation or a plot.
//!
//! The first local monetary loop is also server truth: villagers have
//! fixed-point wallets, the Moot quotes from physical stock and earmarked cash,
//! producers split realised sale proceeds with workplace owners, households buy
//! one daily ration, builders buy Wood or gather it themselves, and the Moot
//! pays its one road steward. Births, boats and remote trade remain deferred.
//!
//! Everything in this module is server truth. Clients receive settlements and
//! buildings and draw them; they never decide anything.

pub mod ambient;
pub mod history;
#[cfg(test)]
mod scale_lab;

use bevy::ecs::system::SystemParam;
use bevy::platform::collections::{HashMap, HashSet};
use bevy::prelude::*;
use lightyear::prelude::{NetworkTarget, Replicate};

use shared::components::{
    BuildingDoorDemand, BuildingDoorUse, CharacterActivity, CharacterAttributes, CharacterKind,
    CharacterName, FarmField, FishingPier, Household, MootAdministration, Nutrition, Occupation,
    PlayerPosition, PlayerRotation, Residence, Settlement, SettlementBuilding,
    SettlementBuildingKind, SettlementPolicies, VillageRoad, WorkStatus, WorldTime,
};
use shared::economy::{
    permit_price, BusinessAccount, BusinessSalePolicy, BusinessWagePolicy, CarriedLoad, Good,
    GoodsInventory, HouseholdEconomy, MootMarket, SettlementEconomy, Wallet, WorkforceRequirements,
    BUSINESS_WAGE_REVIEW_STEP, FOOD_SECURITY_TARGET_DAYS, FOUNDING_DAILY_WAGE,
    FULLY_STAFFED_DAYS_BEFORE_REVIEW, MAXIMUM_BUSINESS_DAILY_WAGE, MINIMUM_BUSINESS_DAILY_WAGE,
    PAYROLL_STRESS_DAYS_BEFORE_CUT, PENNIES_PER_COIN, STARTING_TREASURY_MONEY,
    VACANCY_DAYS_BEFORE_RAISE, VILLAGE_MIN_PROSPERITY, VILLAGE_MIN_RESIDENTS,
    VILLAGE_REQUIRED_SECURE_DAYS, WEALTHY_OWNER_MONEY,
};
use shared::region::{RegionCoord, SimLevel};
use shared::spatial::SpatialObstacleGrid;
use shared::terrain::{ChunkCoord, WorldTerrain};

use crate::collision::library::{DerivedColliderLibrary, StaticColliders};
use crate::player::hero::MoveTarget;
use crate::world::navgrid::VILLAGER_PROP_RADIUS;
use crate::world::regions::RegionRegistry;
use crate::world::village_roads::{
    NavigationRouteFailed, NavigationRoutePending, RoadBuilderRoutine, RoadRequest, RoadSteward,
    RouteWaypoint, TravelRoute,
};

/// Terrain and collision truth needed while choosing a plot. Keeping these
/// related resources in one system parameter leaves room for the rest of the
/// permit system's settlement queries within Bevy's system-parameter limit.
#[derive(SystemParam)]
pub struct PermitPlanningResources<'w, 's> {
    terrain: Option<Res<'w, WorldTerrain>>,
    colliders: Option<Res<'w, StaticColliders>>,
    derived: Option<Res<'w, DerivedColliderLibrary>>,
    permit_busy: Query<
        'w,
        's,
        (),
        Or<(
            With<FarmerRoutine>,
            With<FishingRoutine>,
            With<LumberjackRoutine>,
            With<MarketCollectionRoutine>,
            With<HouseholdShoppingRoutine>,
            With<WorkplaceDoorTransit>,
            With<PierTraversal>,
        )>,
    >,
}

/// Give the derived moot hall the same obstacle/build-zone identity as every
/// replicated settlement building. A settlement entity is the hall entity;
/// without this derivation roads and the nav grid see an empty plot here.
pub fn claim_settlement_hall_obstacles(
    mut commands: Commands,
    halls: Query<
        (Entity, &PlayerPosition, Option<&PlayerRotation>),
        (With<Settlement>, Without<shared::building::PlacedBuilding>),
    >,
) {
    for (hall, position, rotation) in halls.iter() {
        commands.entity(hall).insert((
            shared::building::PlacedBuilding {
                building_type: SettlementBuildingKind::Hall.art(),
                rotation: rotation.map_or(0.0, |rotation| rotation.0),
            },
            shared::building::BuildingPosition(position.0),
        ));
    }
}

/// How often an uncommitted villager looks for somewhere to live.
///
/// Seconds, not frames: this is a decision, and decisions should not get more
/// frequent because the server is running well.
const SEEK_INTERVAL: f32 = 3.0;

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

/// How close a villager must get to the hall to have arrived.
///
/// Generous, because arrival is the point rather than the precision: a villager
/// who stops a metre short and stands there forever is a bug the player will
/// read as the whole system being broken.
const ARRIVAL_RADIUS: f32 = 6.0;

/// Physical work-loop tuning. Prices decide whether a transfer can happen, but
/// walking, work duration and carried capacity still decide when it happens.
const WORK_REACH: f32 = 2.5;
const INDOOR_REST_SECONDS: f32 = 4.0;
// These are world-time work passes, not tiny transaction delays. Output rate is
// physical: field quality controls how much labour makes one Wheat, and a
// farmer works continuously until the shift ends instead of receiving a daily
// production allowance.
const CHOP_SECONDS: f32 = 90.0;
/// At 100% quality, one Wheat takes 2m50s of actual field work. The ordinary
/// 06:00-ish to 18:00 shift contains about 1,050 simulation seconds, so a
/// perfect field approaches six Wheat after allowing for short local trips.
/// A common 67% field takes about 4m14s and approaches four Wheat per shift.
const PERFECT_FIELD_SECONDS_PER_WHEAT: f32 = 170.0;
const FARM_CARRY_BATCH_UNITS: u32 = 2;
const FISH_CARRY_BATCH_UNITS: u32 = 2;
/// Daylight spans 06:00-22:00. A 75% cutoff ends ordinary work near 18:00,
/// leaving a visible evening for shopping, socialising and household tasks.
const WORKDAY_END_DAY_T: f32 = 0.75;
const TREE_MIN_DISTANCE: f32 = 10.0;
const TREE_MAX_DISTANCE: f32 = 120.0;
const DOOR_REACH: f32 = 0.4;
const DOOR_OPEN_SECONDS: f32 = 0.667;

/// How long a fully supplied building takes to raise, in seconds.
///
/// The timer represents builder work only. It cannot start until the site's
/// physical inventory contains the building's full wood requirement.
const BUILD_SECONDS: f32 = shared::components::SETTLEMENT_RAISE_SECONDS;

type FarmsteadPositionKey = (u32, u32, u32);

fn farmstead_position_key(position: Vec3) -> FarmsteadPositionKey {
    (
        position.x.to_bits(),
        position.y.to_bits(),
        position.z.to_bits(),
    )
}

/// Actual field labour required for one inventory unit. Progress is retained
/// between shifts, so short working days reduce current output without erasing
/// work or requiring a special per-day cap. Almost barren fields can still
/// yield eventually, but are proportionally unattractive places to build.
fn farmer_seconds_per_wheat(quality: f32) -> f32 {
    let quality = if quality.is_finite() {
        quality.clamp(0.01, 1.0)
    } else {
        0.01
    };
    PERFECT_FIELD_SECONDS_PER_WHEAT / quality
}

/// Pier quality uses the same readable scale as farmland: ideal water can
/// approach six catches per ordinary shift, while a two-thirds-quality shore
/// approaches four. Travel along the pier is real labour overhead rather than
/// being hidden inside a daily output table.
fn fisher_seconds_per_food(quality: f32) -> f32 {
    farmer_seconds_per_wheat(quality)
}

/// A completed interaction with a real tree creates one physical carried
/// bundle, whose size reflects the timber density sampled where the hut was
/// built. There is deliberately no daily ceiling.
fn lumber_tree_yield(quality: f32) -> u32 {
    if quality < 0.34 {
        1
    } else if quality < 0.67 {
        2
    } else {
        3
    }
}

/// Where a villager is in the business of joining somewhere.
#[derive(Component, Debug, Clone, PartialEq)]
pub enum VillagerIntent {
    /// Knows of nowhere to go. Re-checks on the seek tick.
    Idle,
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
}

impl MigrationCooldown {
    fn after_failure(previous: Option<Self>, settlement: Entity, now: f64) -> Self {
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
        }
    }
}

impl VillagerIntent {
    /// The settlement this villager belongs to, if any.
    pub fn settlement(&self) -> Option<Entity> {
        match self {
            VillagerIntent::Idle => None,
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
    /// Who is actually walking out there. Held as an entity rather than looked
    /// up by name because generated names repeat -- the first duplicate shows
    /// up around the fifty-first villager.
    pub builder: Option<Entity>,
    pub settlement: Entity,
    /// Where the builder stands to work. Arrival is judged against THIS, not
    /// the plot centre, or they would walk into the middle of the site.
    pub stand: Vec3,
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

impl ConstructionMaterialRoutine {
    pub(crate) fn is_waiting_for_materials(&self) -> bool {
        matches!(self.phase, ConstructionMaterialPhase::Seeking)
    }
}

#[derive(Debug, Clone, Copy)]
enum ConstructionMaterialPhase {
    Seeking,
    UnloadingAtHall { hall: Entity, entrance: Vec3 },
    CollectingFromStore { source: Entity, entrance: Vec3 },
    WalkingToTree { tree: Vec3, stand: Vec3 },
    Chopping { tree: Vec3, seconds_left: f32 },
    Delivering { destination: Vec3 },
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
    chop_seconds: f32,
    production_day: u32,
    produced_today: u32,
    phase: LumberjackPhase,
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
    /// Productive seconds already invested in the next Wheat. This is copied
    /// into FarmerHarvestProgress when the shift ends and restored tomorrow.
    harvest_seconds: f32,
    production_day: u32,
    produced_today: u32,
    phase: FarmerPhase,
}

#[cfg(test)]
impl FarmerRoutine {
    /// Exposes workplace identity to the deterministic Village Lab without
    /// publishing the rest of the routine's server-only state.
    pub(crate) fn farmstead(&self) -> Entity {
        self.farmstead
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
    production_day: u32,
    produced_today: u32,
    phase: FishingPhase,
}

#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct FishingWorkProgress {
    hut: Entity,
    pier: Entity,
    seconds: f32,
}

/// The founding Moot Hall's commercial worker. A porter is the sole early
/// long-distance hauler between private workplace stores and the public market.
#[derive(Component, Debug, Clone, Copy)]
pub struct MarketPorter {
    settlement: Entity,
}

#[derive(Component, Debug, Clone)]
pub struct MarketCollectionRoutine {
    business: Entity,
    hall: Entity,
    good: Good,
    reserved_units: u32,
    reserved_pennies: u64,
    phase: MarketCollectionPhase,
}

impl MarketCollectionRoutine {
    #[cfg(test)]
    pub(crate) fn reserved_pennies(&self) -> u64 {
        self.reserved_pennies
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MarketCollectionPhase {
    GoingToBusiness,
    ReturningToHall,
}

/// A tactical-region household's one visible restocking trip. Strategic
/// households settle the identical transaction directly at their daily tick.
#[derive(Component, Debug, Clone, Copy)]
pub struct HouseholdShoppingRoutine {
    home: Entity,
    hall: Entity,
    phase: HouseholdShoppingPhase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HouseholdShoppingPhase {
    GoingToMarket,
    ReturningHome,
}

fn business_output(kind: SettlementBuildingKind) -> Option<Good> {
    match kind {
        SettlementBuildingKind::Farmstead => Some(Good::Wheat),
        SettlementBuildingKind::FishermansHut => Some(Good::Food),
        SettlementBuildingKind::LumberjackHut => Some(Good::Wood),
        _ => None,
    }
}

/// Add the small ledgers used by every productive workplace. Founding working
/// capital is transferred from the owner when possible, never minted.
pub fn ensure_business_economies(
    mut commands: Commands,
    buildings: Query<(
        Entity,
        &SettlementBuilding,
        Option<&BusinessAccount>,
        Option<&BusinessSalePolicy>,
        Option<&BusinessWagePolicy>,
    )>,
    mut owners: Query<(&CharacterName, &mut Wallet)>,
) {
    for (entity, building, account, policy, wage_policy) in buildings.iter() {
        if business_output(building.kind).is_none() {
            continue;
        }
        let mut entity_commands = commands.entity(entity);
        if account.is_none() {
            let mut opening_cash = 0;
            if let Some(owner) = building.owner.as_deref() {
                if let Some((_, mut wallet)) = owners.iter_mut().find(|(name, _)| name.0 == owner) {
                    let wanted = 2 * PENNIES_PER_COIN;
                    if wallet.debit(wanted) {
                        opening_cash = wanted;
                    }
                }
            }
            entity_commands.insert(BusinessAccount {
                cash: opening_cash,
                ..default()
            });
        }
        if policy.is_none() {
            entity_commands.insert(BusinessSalePolicy::default());
        }
        if wage_policy.is_none() {
            entity_commands.insert(BusinessWagePolicy::default());
        }
    }
}

/// The Moot has three founding positions: Reeve, market porter and road
/// steward. The road system owns the steward; this pass fills the other two
/// without stealing business workers or residents who chose to chill.
pub fn staff_moot_hall_roles(
    mut commands: Commands,
    world_time: Query<&WorldTime>,
    mut last_payday: Local<HashMap<Entity, u32>>,
    buildings: Query<&SettlementBuilding>,
    mut halls: Query<(Entity, &mut Settlement, &mut MootAdministration)>,
    mut villagers: Query<(
        Entity,
        &CharacterName,
        &VillagerIntent,
        &mut Occupation,
        Option<&WorkStatus>,
        Option<&MarketPorter>,
    )>,
    mut wallets: Query<(&CharacterName, &mut Wallet)>,
) {
    let day = world_time.iter().next().map_or(0, |clock| clock.day);
    let business_workers: HashSet<&str> = buildings
        .iter()
        .flat_map(|building| building.workers.iter().map(String::as_str))
        .collect();
    for (hall, mut settlement, mut administration) in halls.iter_mut() {
        let valid = |wanted: &str| {
            villagers.iter().any(|(_, name, intent, _, _, _)| {
                name.0 == wanted && intent.settlement() == Some(hall) && intent.counts_as_resident()
            })
        };
        if administration
            .reeve
            .as_deref()
            .is_some_and(|name| !valid(name))
        {
            administration.reeve = None;
        }
        if administration
            .market_porter
            .as_deref()
            .is_some_and(|name| !valid(name))
        {
            administration.market_porter = None;
        }

        // Logistics comes before clerical comfort in a tiny foundation. Keep
        // at least one resident outside the Moot staff so three founders can
        // still apply for permits and operate the first workplace.
        for (title, porter_role) in [("Market Porter", true), ("Reeve", false)] {
            let founding_staff = usize::from(administration.road_steward.is_some())
                + usize::from(administration.market_porter.is_some())
                + usize::from(administration.reeve.is_some());
            if founding_staff >= settlement.residents.saturating_sub(1) as usize {
                continue;
            }
            let filled = if porter_role {
                administration.market_porter.is_some()
            } else {
                administration.reeve.is_some()
            };
            if filled {
                continue;
            }
            let occupied: HashSet<String> = administration
                .road_steward
                .iter()
                .chain(administration.reeve.iter())
                .chain(administration.market_porter.iter())
                .chain(administration.city_workers.iter())
                .chain(administration.guards.iter())
                .cloned()
                .collect();
            let candidate = villagers
                .iter()
                .filter(|(_, name, intent, occupation, status, _)| {
                    matches!(intent, VillagerIntent::Resident { settlement } if *settlement == hall)
                        && occupation.0.is_none()
                        && status.is_none_or(|status| *status == WorkStatus::LookingForWork)
                        && !business_workers.contains(name.0.as_str())
                        && !occupied.contains(&name.0)
                })
                .min_by(|a, b| a.1 .0.cmp(&b.1 .0))
                .map(|(entity, ..)| entity);
            let Some(candidate) = candidate else { continue };
            let Ok((_, name, _, mut occupation, _, marker)) = villagers.get_mut(candidate) else {
                continue;
            };
            occupation.0 = Some(title.to_string());
            commands.entity(candidate).insert(WorkStatus::Employed);
            if porter_role {
                administration.market_porter = Some(name.0.clone());
                if marker.is_none_or(|porter| porter.settlement != hall) {
                    commands
                        .entity(candidate)
                        .insert(MarketPorter { settlement: hall });
                }
            } else {
                administration.reeve = Some(name.0.clone());
            }
            info!(
                "Village '{}': {} took the {} position",
                settlement.name, name.0, title
            );
        }

        let previous = last_payday.entry(hall).or_insert(day);
        let elapsed = day.saturating_sub(*previous);
        if elapsed > 0 {
            *previous = day;
            let due = FOUNDING_DAILY_WAGE.saturating_mul(u64::from(elapsed));
            for employee in [
                administration.reeve.as_deref(),
                administration.market_porter.as_deref(),
            ]
            .into_iter()
            .flatten()
            {
                let payment = settlement.treasury.min(due);
                if payment == 0 {
                    break;
                }
                if let Some((_, mut wallet)) =
                    wallets.iter_mut().find(|(name, _)| name.0 == employee)
                {
                    settlement.treasury -= payment;
                    wallet.credit(payment);
                }
            }
        }
    }
}

/// Keep the compact work-state component aligned with real rosters. `Chilling`
/// is an intentional choice and is never silently converted back into job
/// seeking merely because the occupation title is empty.
pub fn reconcile_work_statuses(
    mut villagers: Query<(&Occupation, &mut WorkStatus), With<CharacterKind>>,
) {
    for (occupation, mut status) in villagers.iter_mut() {
        let next = if occupation.0.is_some() {
            WorkStatus::Employed
        } else if *status == WorkStatus::Chilling {
            WorkStatus::Chilling
        } else {
            WorkStatus::LookingForWork
        };
        if *status != next {
            *status = next;
        }
    }
}

/// Collect saleable stock with the Moot porter. The market reserves its exact
/// cash at dispatch, goods remain at the business until the porter reaches it,
/// and the business account is credited only after physical delivery.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn run_market_collections(
    mut commands: Commands,
    mut halls: Query<
        (
            &Settlement,
            &PlayerPosition,
            Option<&PlayerRotation>,
            &mut GoodsInventory,
            &mut MootMarket,
        ),
        (Without<SettlementBuilding>, Without<CharacterKind>),
    >,
    mut businesses: Query<
        (
            Entity,
            &SettlementBuilding,
            &PlayerPosition,
            &PlayerRotation,
            &mut GoodsInventory,
            &BusinessSalePolicy,
            &mut BusinessAccount,
        ),
        Without<CharacterKind>,
    >,
    mut porters: Query<
        (
            Entity,
            &MarketPorter,
            &PlayerPosition,
            &mut CharacterActivity,
            &mut GoodsInventory,
            Option<&MoveTarget>,
            Option<&mut MarketCollectionRoutine>,
            Option<&HomeRoutine>,
            Option<&RoadBuilderRoutine>,
            Option<&HouseholdShoppingRoutine>,
            Option<&NavigationRouteFailed>,
        ),
        With<CharacterKind>,
    >,
) {
    for (
        porter_entity,
        porter,
        position,
        mut activity,
        mut carrier,
        move_target,
        routine,
        home,
        road_work,
        shopping,
        route_failed,
    ) in porters.iter_mut()
    {
        if home.is_some() || road_work.is_some() || shopping.is_some() {
            continue;
        }
        let Ok((settlement, hall_position, hall_rotation, mut hall_store, mut market)) =
            halls.get_mut(porter.settlement)
        else {
            continue;
        };
        let hall_entrance = SettlementBuildingKind::Hall.entrance_position(
            hall_position.0,
            hall_rotation.map_or(0.0, |rotation| rotation.0),
        );

        if let Some(failed) = route_failed {
            match routine.as_deref() {
                Some(active) if active.phase == MarketCollectionPhase::GoingToBusiness => {
                    // The market reserved its cash at dispatch. If the porter
                    // cannot reach that seller, unwind the reservation instead
                    // of leaving the entire early economy permanently short of
                    // both its porter and those pennies.
                    warn!(
                        "Market porter could not reach business at {:.1},{:.1}; cancelling that collection so another offer can be tried",
                        failed.goal.x, failed.goal.z
                    );
                    market.cancel_producer_purchase(
                        active.good,
                        hall_store.amount(active.good),
                        shared::economy::MarketTrade {
                            units: active.reserved_units,
                            pennies: active.reserved_pennies,
                        },
                    );
                    commands
                        .entity(porter_entity)
                        .remove::<MarketCollectionRoutine>()
                        .remove::<MoveTarget>()
                        .remove::<TravelRoute>()
                        .remove::<NavigationRoutePending>()
                        .remove::<NavigationRouteFailed>();
                }
                Some(active) => {
                    // Once stock is physically aboard it must reach the hall.
                    // Clear the static failure and explicitly dirty the target
                    // so the bounded planner gets a fresh request next tick.
                    warn!(
                        "Market porter retrying a loaded return to the Moot Hall after route failure at {:.1},{:.1}",
                        failed.goal.x, failed.goal.z
                    );
                    debug_assert_eq!(active.phase, MarketCollectionPhase::ReturningToHall);
                    commands
                        .entity(porter_entity)
                        .remove::<TravelRoute>()
                        .remove::<NavigationRoutePending>()
                        .remove::<NavigationRouteFailed>()
                        .insert(MoveTarget(hall_entrance));
                }
                None => {
                    // A stale failure without a live transaction must never
                    // prevent this unique civic worker from accepting work.
                    let mut porter_commands = commands.entity(porter_entity);
                    porter_commands
                        .remove::<TravelRoute>()
                        .remove::<NavigationRoutePending>()
                        .remove::<NavigationRouteFailed>();
                    if carrier.used_bulk() > 0 {
                        porter_commands.insert(MoveTarget(hall_entrance));
                    } else {
                        porter_commands.remove::<MoveTarget>();
                    }
                }
            }
            continue;
        }

        let Some(mut routine) = routine else {
            if carrier.used_bulk() > 0 {
                // Recover an old interrupted load before reserving another.
                ensure_move_target(&mut commands, porter_entity, move_target, hall_entrance);
                continue;
            }
            let mut offers: Vec<(Entity, Good, u32, Vec3)> = businesses
                .iter()
                .filter_map(|(entity, building, at, rotation, inventory, policy, _)| {
                    if building.settlement != settlement.name || !policy.collection_enabled {
                        return None;
                    }
                    let good = business_output(building.kind)?;
                    if market.pool(good).bid < policy.minimum_unit_price {
                        return None;
                    }
                    let surplus = inventory.amount(good).saturating_sub(policy.keep_units);
                    let carrier_room = carrier.free_bulk() / good.bulk_per_unit();
                    let hall_room = hall_store.free_bulk() / good.bulk_per_unit();
                    let units = surplus
                        .min(policy.max_units_per_collection)
                        .min(carrier_room)
                        .min(hall_room);
                    (units > 0).then_some((
                        entity,
                        good,
                        units,
                        building.kind.entrance_position(at.0, rotation.0),
                    ))
                })
                .collect();
            offers.sort_unstable_by_key(|(entity, _, _, _)| entity.to_bits());
            let Some((business, good, offered, entrance)) = offers.into_iter().next() else {
                *activity = CharacterActivity::Indoors;
                commands.entity(porter_entity).remove::<MoveTarget>();
                continue;
            };
            let trade = market.buy_from_producer(good, hall_store.amount(good), offered);
            if trade.units == 0 {
                continue;
            }
            *activity = CharacterActivity::Idle;
            commands.entity(porter_entity).insert((
                MarketCollectionRoutine {
                    business,
                    hall: porter.settlement,
                    good,
                    reserved_units: trade.units,
                    reserved_pennies: trade.pennies,
                    phase: MarketCollectionPhase::GoingToBusiness,
                },
                MoveTarget(entrance),
            ));
            continue;
        };

        if routine.hall != porter.settlement {
            commands
                .entity(porter_entity)
                .remove::<MarketCollectionRoutine>()
                .remove::<MoveTarget>();
            continue;
        }
        match routine.phase {
            MarketCollectionPhase::GoingToBusiness => {
                let Ok((_, building, at, rotation, mut store, _, _)) =
                    businesses.get_mut(routine.business)
                else {
                    market.cancel_producer_purchase(
                        routine.good,
                        hall_store.amount(routine.good),
                        shared::economy::MarketTrade {
                            units: routine.reserved_units,
                            pennies: routine.reserved_pennies,
                        },
                    );
                    commands
                        .entity(porter_entity)
                        .remove::<MarketCollectionRoutine>()
                        .remove::<MoveTarget>();
                    continue;
                };
                let entrance = building.kind.entrance_position(at.0, rotation.0);
                if ground_distance(position.0, entrance) > WORK_REACH {
                    ensure_move_target(&mut commands, porter_entity, move_target, entrance);
                    continue;
                }
                let moved = store.transfer_to(&mut carrier, routine.good, routine.reserved_units);
                if moved != routine.reserved_units {
                    market.cancel_producer_purchase(
                        routine.good,
                        hall_store.amount(routine.good),
                        shared::economy::MarketTrade {
                            units: routine.reserved_units,
                            pennies: routine.reserved_pennies,
                        },
                    );
                    let replacement = market.buy_from_producer(
                        routine.good,
                        hall_store.amount(routine.good),
                        moved,
                    );
                    routine.reserved_units = replacement.units;
                    routine.reserved_pennies = replacement.pennies;
                }
                if routine.reserved_units == 0 {
                    commands
                        .entity(porter_entity)
                        .remove::<MarketCollectionRoutine>()
                        .remove::<MoveTarget>();
                    continue;
                }
                *activity = CharacterActivity::Idle;
                commands
                    .entity(porter_entity)
                    .insert(MoveTarget(hall_entrance));
                routine.phase = MarketCollectionPhase::ReturningToHall;
            }
            MarketCollectionPhase::ReturningToHall => {
                if ground_distance(position.0, hall_entrance) > WORK_REACH {
                    ensure_move_target(&mut commands, porter_entity, move_target, hall_entrance);
                    continue;
                }
                let delivered =
                    carrier.transfer_to(&mut hall_store, routine.good, routine.reserved_units);
                if let Ok((_, _, _, _, _, _, mut account)) = businesses.get_mut(routine.business) {
                    if delivered == routine.reserved_units {
                        account.cash = account.cash.saturating_add(routine.reserved_pennies);
                    }
                }
                *activity = CharacterActivity::Indoors;
                commands
                    .entity(porter_entity)
                    .remove::<MarketCollectionRoutine>()
                    .remove::<MoveTarget>();
            }
        }
    }
}

/// Pay daily wages from business cash, distribute only genuine surplus to the
/// owner, and let a wealthy owner leave hands-on work when a replacement is
/// ready. This is daily O(people + workplaces), not per-frame decision search.
pub fn run_business_payroll_and_owner_leisure(
    mut commands: Commands,
    world_time: Query<&WorldTime>,
    settlements: Query<(Entity, &Settlement)>,
    mut businesses: Query<(
        &mut SettlementBuilding,
        &mut BusinessAccount,
        &mut BusinessWagePolicy,
    )>,
    mut villagers: Query<(
        Entity,
        &CharacterName,
        &VillagerIntent,
        &mut Wallet,
        &mut Occupation,
        &mut WorkStatus,
    )>,
    mut processed_day: Local<Option<u32>>,
) {
    let Some(day) = world_time.iter().next().map(|clock| clock.day) else {
        return;
    };
    if *processed_day == Some(day) {
        return;
    }
    *processed_day = Some(day);
    let settlement_ids: HashMap<&str, Entity> = settlements
        .iter()
        .map(|(entity, settlement)| (settlement.name.as_str(), entity))
        .collect();
    // Build one daily index. Looking up every roster name by scanning all
    // villagers made payroll O(businesses * population), and the wealthy-owner
    // check repeated that cost even on ticks with no day boundary.
    let mut people_by_settlement: HashMap<Entity, HashMap<String, Entity>> = HashMap::new();
    let mut available_replacements: HashMap<Entity, usize> = HashMap::new();
    for (entity, name, intent, _, occupation, status) in villagers.iter() {
        let Some(settlement) = intent.settlement() else {
            continue;
        };
        people_by_settlement
            .entry(settlement)
            .or_default()
            .entry(name.0.clone())
            .or_insert(entity);
        if occupation.0.is_none() && *status == WorkStatus::LookingForWork {
            *available_replacements.entry(settlement).or_default() += 1;
        }
    }

    for (mut building, mut account, mut wage_policy) in businesses.iter_mut() {
        if business_output(building.kind).is_none() {
            continue;
        }
        let Some(settlement) = settlement_ids.get(building.settlement.as_str()).copied() else {
            continue;
        };
        let people = people_by_settlement.get(&settlement);
        if account.last_payroll_day == u32::MAX {
            account.last_payroll_day = day;
        }
        let elapsed = day.saturating_sub(account.last_payroll_day);
        if elapsed > 0 {
            account.last_payroll_day = day;
            wage_policy.daily_wage = wage_policy
                .daily_wage
                .clamp(MINIMUM_BUSINESS_DAILY_WAGE, MAXIMUM_BUSINESS_DAILY_WAGE);
            let per_worker = wage_policy.daily_wage.saturating_mul(u64::from(elapsed));
            account.wage_arrears = account
                .wage_arrears
                .saturating_add(per_worker.saturating_mul(building.workers.len() as u64));

            // Arrears are a real workplace liability, not merely a warning
            // counter. When later sales make cash available, distribute the
            // entire affordable obligation evenly so no alphabetically-early
            // worker is always paid while everybody else starves.
            let mut workers = building.workers.clone();
            workers.sort();
            let worker_entities: Vec<Entity> = workers
                .iter()
                .filter_map(|wanted| people.and_then(|people| people.get(wanted)).copied())
                .collect();
            if !worker_entities.is_empty() {
                let payment_budget = account.cash.min(account.wage_arrears);
                let worker_count = worker_entities.len() as u64;
                let equal_share = payment_budget / worker_count;
                let remainder = payment_budget % worker_count;
                let mut paid = 0_u64;
                for (index, worker) in worker_entities.into_iter().enumerate() {
                    let payment = equal_share + u64::from((index as u64) < remainder);
                    let Ok((_, _, _, mut wallet, _, _)) = villagers.get_mut(worker) else {
                        continue;
                    };
                    wallet.credit(payment);
                    paid = paid.saturating_add(payment);
                }
                account.cash = account.cash.saturating_sub(paid);
                account.wage_arrears = account.wage_arrears.saturating_sub(paid);
            }

            if let Some(owner) = building.owner.as_deref() {
                let payroll_reserve = wage_policy
                    .daily_wage
                    .saturating_mul(building.workers.len() as u64)
                    .saturating_mul(2)
                    .saturating_add(2 * PENNIES_PER_COIN)
                    .saturating_add(account.wage_arrears);
                let draw = account.cash.saturating_sub(payroll_reserve);
                if draw > 0 {
                    if let Some(owner_entity) = people.and_then(|people| people.get(owner)).copied()
                    {
                        let Ok((_, _, _, mut wallet, _, _)) = villagers.get_mut(owner_entity)
                        else {
                            continue;
                        };
                        account.cash -= draw;
                        wallet.credit(draw);
                    }
                }
            }

            review_automatic_wage_offer(
                &mut wage_policy,
                elapsed,
                building.workers.len(),
                building.kind.positions() as usize,
                account.cash,
                account.wage_arrears,
            );
        }

        let Some(owner) = building.owner.clone() else {
            continue;
        };
        if !building.workers.iter().any(|worker| worker == &owner) {
            continue;
        }
        let Some(owner_entity) = people.and_then(|people| people.get(&owner)).copied() else {
            continue;
        };
        let replacement_exists = available_replacements
            .get(&settlement)
            .copied()
            .unwrap_or(0)
            > 0;
        let owner_is_wealthy = villagers
            .get(owner_entity)
            .is_ok_and(|(_, _, _, wallet, _, _)| wallet.balance() >= WEALTHY_OWNER_MONEY);
        let payroll_secure = account.cash
            >= wage_policy
                .daily_wage
                .saturating_mul(building.workers.len() as u64)
                .saturating_mul(2);
        if !replacement_exists || !owner_is_wealthy || !payroll_secure {
            continue;
        }
        building.workers.retain(|worker| worker != &owner);
        if let Ok((_, _, _, _, mut occupation, mut status)) = villagers.get_mut(owner_entity) {
            occupation.0 = None;
            *status = WorkStatus::Chilling;
            commands
                .entity(owner_entity)
                .remove::<FarmerRoutine>()
                .remove::<FishingRoutine>()
                .remove::<LumberjackRoutine>()
                .remove::<WorkplaceDoorTransit>()
                .remove::<MoveTarget>();
            info!(
                "Village '{}': {} became a wealthy owner and left daily {} work",
                building.settlement,
                owner,
                building.kind.trade().unwrap_or("business")
            );
        }
    }
}

fn review_automatic_wage_offer(
    policy: &mut BusinessWagePolicy,
    elapsed_days: u32,
    filled_positions: usize,
    total_positions: usize,
    cash: u64,
    arrears: u64,
) {
    if !policy.automatic || elapsed_days == 0 || total_positions == 0 {
        return;
    }
    let elapsed = elapsed_days.min(u32::from(u16::MAX)) as u16;
    if filled_positions < total_positions {
        policy.vacancy_days = policy.vacancy_days.saturating_add(elapsed);
        policy.fully_staffed_days = 0;
        policy.payroll_stress_days = 0;
        let next = policy
            .daily_wage
            .saturating_add(BUSINESS_WAGE_REVIEW_STEP)
            .min(MAXIMUM_BUSINESS_DAILY_WAGE);
        let full_day_cost = next.saturating_mul(total_positions as u64);
        if policy.vacancy_days >= VACANCY_DAYS_BEFORE_RAISE
            && next > policy.daily_wage
            && cash >= full_day_cost
        {
            policy.daily_wage = next;
            policy.vacancy_days = 0;
        }
        return;
    }

    policy.vacancy_days = 0;
    policy.fully_staffed_days = policy.fully_staffed_days.saturating_add(elapsed);
    if arrears > 0 {
        policy.payroll_stress_days = policy.payroll_stress_days.saturating_add(elapsed);
    } else {
        policy.payroll_stress_days = 0;
    }
    let staffed_payroll = policy.daily_wage.saturating_mul(filled_positions as u64);
    let easy_to_fill_but_cash_tight = policy.fully_staffed_days >= FULLY_STAFFED_DAYS_BEFORE_REVIEW
        && cash < staffed_payroll.saturating_mul(2);
    if policy.payroll_stress_days >= PAYROLL_STRESS_DAYS_BEFORE_CUT || easy_to_fill_but_cash_tight {
        policy.daily_wage = policy
            .daily_wage
            .saturating_sub(BUSINESS_WAGE_REVIEW_STEP)
            .max(MINIMUM_BUSINESS_DAILY_WAGE);
        policy.payroll_stress_days = 0;
        policy.fully_staffed_days = 0;
    }
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

fn begin_workplace_entry(
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

fn begin_workplace_exit(
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
    time: Res<Time>,
    obstacles: Option<Res<SpatialObstacleGrid>>,
    warp: Query<&shared::components::TimeWarp>,
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
        With<CharacterKind>,
    >,
) {
    let dt = time.delta_secs() * time_warp_factor(&warp);
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
}

/// Server-side identity-safe link from one villager entity to one cabin.
/// Household names are for inspection; this entity link is what keeps two
/// people with the same generated name from stealing each other's bed.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct HomeAssignment {
    home: Entity,
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
    /// An unchanged village cannot make an unchanged failed plot search
    /// succeed. Remember that exact geometry until residents, buildings,
    /// roads, or edited terrain change instead of rescanning and logging the
    /// same failure every four simulated seconds at 100x.
    failed_site_searches: HashMap<Entity, FailedSiteSearch>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct FailedSiteSearch {
    kind: SettlementBuildingKind,
    residents: u32,
    occupied_plots: usize,
    roads: usize,
    terrain_version: u32,
}

impl Default for VillageClock {
    fn default() -> Self {
        Self {
            seek: 0.0,
            permit: 0.0,
            failed_site_searches: HashMap::new(),
        }
    }
}

const FOOD_HISTORY_DAYS: usize = 3;

#[derive(Debug)]
struct SettlementEconomyDay {
    last_world_day: u32,
    produced_today: u32,
    production_history: [u32; FOOD_HISTORY_DAYS],
    consumption_history: [u32; FOOD_HISTORY_DAYS],
    recorded_days: usize,
}

impl SettlementEconomyDay {
    fn new(day: u32) -> Self {
        Self {
            last_world_day: day,
            produced_today: 0,
            production_history: [0; FOOD_HISTORY_DAYS],
            consumption_history: [0; FOOD_HISTORY_DAYS],
            recorded_days: 0,
        }
    }

    fn finish_day(&mut self, consumed: u32) {
        self.production_history.rotate_right(1);
        self.consumption_history.rotate_right(1);
        self.production_history[0] = std::mem::take(&mut self.produced_today);
        self.consumption_history[0] = consumed;
        self.recorded_days = (self.recorded_days + 1).min(FOOD_HISTORY_DAYS);
    }

    fn recent_average(values: &[u32; FOOD_HISTORY_DAYS], days: usize) -> f32 {
        if days == 0 {
            return 0.0;
        }
        values[..days].iter().sum::<u32>() as f32 / days as f32
    }

    /// The just-finished day's production plus enough history to retain the
    /// same three-day window used by the public economy reading.
    fn recent_production_including_today(&self) -> f32 {
        let previous_days = self.recorded_days.min(FOOD_HISTORY_DAYS.saturating_sub(1));
        let total = self.produced_today.saturating_add(
            self.production_history[..previous_days]
                .iter()
                .copied()
                .sum(),
        );
        total as f32 / (previous_days + 1) as f32
    }
}

/// Server-only daily buckets behind the small replicated economy summary.
#[derive(Resource, Default)]
pub struct SettlementEconomyRuntime {
    by_settlement: HashMap<Entity, SettlementEconomyDay>,
}

/// Coin already removed from a market pool and waiting for a named owner's
/// wallet. Keeping it as an entity makes the conservation boundary explicit:
/// a deferred owner payment is still money, never a number hidden in a closure.
#[derive(Component, Debug, Clone)]
pub(crate) struct PendingMarketPayment {
    pub settlement: Entity,
    pub recipient: String,
    pub pennies: u64,
}

fn queue_credit(
    commands: &mut Commands,
    settlement: Entity,
    recipient: &str,
    wallet: Option<&mut Wallet>,
    pennies: u64,
) {
    if pennies == 0 {
        return;
    }
    if let Some(wallet) = wallet {
        wallet.credit(pennies);
    } else {
        commands.spawn(PendingMarketPayment {
            settlement,
            recipient: recipient.to_string(),
            pennies,
        });
    }
}

/// Pay realised output, not promised wages: 80% to the person who performed
/// and hauled the work, 20% to the permitted building's owner. An owner-worker
/// receives the whole sale and no deferred payment entity is needed.
fn distribute_sale_proceeds(
    commands: &mut Commands,
    settlement: Entity,
    worker_name: &str,
    worker_wallet: Option<&mut Wallet>,
    owner: Option<&str>,
    pennies: u64,
) {
    if pennies == 0 {
        return;
    }
    let Some(owner) = owner.filter(|owner| *owner != worker_name) else {
        queue_credit(commands, settlement, worker_name, worker_wallet, pennies);
        return;
    };
    let worker_share = pennies.saturating_mul(80) / 100;
    let owner_share = pennies.saturating_sub(worker_share);
    queue_credit(
        commands,
        settlement,
        worker_name,
        worker_wallet,
        worker_share,
    );
    queue_credit(commands, settlement, owner, None, owner_share);
}

fn sell_carried_to_moot(
    commands: &mut Commands,
    settlement: Entity,
    worker_name: &str,
    worker_wallet: Option<&mut Wallet>,
    owner: Option<&str>,
    good: Good,
    carrier: &mut GoodsInventory,
    hall: &mut GoodsInventory,
    market: &mut MootMarket,
) -> u32 {
    let room = hall.free_bulk() / good.bulk_per_unit();
    let offered = carrier.amount(good).min(room);
    let trade = market.buy_from_producer(good, hall.amount(good), offered);
    let moved = carrier.transfer_to(hall, good, trade.units);
    debug_assert_eq!(moved, trade.units);
    distribute_sale_proceeds(
        commands,
        settlement,
        worker_name,
        worker_wallet,
        owner,
        trade.pennies,
    );
    moved
}

fn buy_from_moot(
    good: Good,
    requested: u32,
    buyer: &mut Wallet,
    hall: &mut GoodsInventory,
    carrier: &mut GoodsInventory,
    market: &mut MootMarket,
) -> u32 {
    let room = carrier.free_bulk() / good.bulk_per_unit();
    let available = requested.min(room).min(hall.amount(good));
    let trade = market.sell_to_consumer(good, hall.amount(good), available, buyer.balance());
    if trade.units == 0 || !buyer.debit(trade.pennies) {
        return 0;
    }
    let moved = hall.transfer_to(carrier, good, trade.units);
    debug_assert_eq!(moved, trade.units);
    moved
}

/// Backfill monetary state on old/test entities and initialise new foundations.
pub fn ensure_village_finances(
    mut commands: Commands,
    mut settlements: Query<(Entity, &mut Settlement, Option<&MootMarket>)>,
    villagers: Query<(Entity, &CharacterKind, Option<&Wallet>)>,
) {
    for (entity, mut settlement, market) in settlements.iter_mut() {
        if market.is_none() {
            if settlement.treasury == 0 {
                settlement.treasury = STARTING_TREASURY_MONEY;
            }
            commands.entity(entity).insert(MootMarket::founding());
        }
    }
    for (entity, kind, wallet) in villagers.iter() {
        if *kind == CharacterKind::Villager && wallet.is_none() {
            commands.entity(entity).insert(Wallet::founding_villager());
        }
    }
}

/// Reprice each Moot against real inventory and measurable local demand.
pub fn update_moot_market_targets(
    mut halls: Query<(Entity, &Settlement, &GoodsInventory, &mut MootMarket)>,
    sites: Query<(&UnderConstruction, &GoodsInventory)>,
) {
    for (hall, settlement, inventory, mut market) in halls.iter_mut() {
        let outstanding_wood = sites
            .iter()
            .filter(|(site, _)| site.settlement == hall)
            .map(|(site, materials)| {
                site.kind
                    .construction_wood_required()
                    .saturating_sub(materials.amount(Good::Wood))
            })
            .fold(0u32, u32::saturating_add);
        market.set_targets(settlement.residents, outstanding_wood);
        market.refresh_all(inventory);
    }
}

/// Finish deferred owner shares after producer systems have queued them.
pub fn settle_pending_market_payments(
    mut commands: Commands,
    payments: Query<(Entity, &PendingMarketPayment)>,
    mut wallets: Query<(&CharacterName, &mut Wallet)>,
) {
    for (payment_entity, payment) in payments.iter() {
        let mut paid = false;
        for (name, mut wallet) in wallets.iter_mut() {
            if name.0 == payment.recipient {
                wallet.credit(payment.pennies);
                paid = true;
                break;
            }
        }
        if paid {
            commands.entity(payment_entity).despawn();
        }
    }
}

impl SettlementEconomyRuntime {
    fn record_food_production(&mut self, settlement: Entity, amount: u32) {
        if amount == 0 {
            return;
        }
        if let Some(day) = self.by_settlement.get_mut(&settlement) {
            day.produced_today = day.produced_today.saturating_add(amount);
        }
    }
}

/// Every hall owns one replicated economic reading; physical stock remains in
/// the inventories of the hall and its completed buildings.
pub fn ensure_settlement_economies(
    mut commands: Commands,
    settlements: Query<Entity, (With<Settlement>, Without<SettlementEconomy>)>,
) {
    for entity in settlements.iter() {
        commands.entity(entity).insert(SettlementEconomy::default());
    }
}

/// Consume one edible portion per resident at each world-day boundary, keep a
/// three-day production/consumption reading, derive prosperity from visible
/// facts, and promote food-secure Hamlets.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn update_settlement_economies(
    world_time: Query<&WorldTime>,
    mut runtime: ResMut<SettlementEconomyRuntime>,
    mut settlements: Query<(
        Entity,
        &mut Settlement,
        &mut SettlementEconomy,
        Option<&mut MootMarket>,
        Option<&SettlementPolicies>,
    )>,
    buildings: Query<(Entity, &SettlementBuilding)>,
    mut residents: Query<
        (
            Entity,
            &VillagerIntent,
            Option<&mut Wallet>,
            Option<&mut Nutrition>,
            Option<&HomeAssignment>,
        ),
        With<CharacterKind>,
    >,
    mut inventories: ParamSet<(
        Query<&mut GoodsInventory, (With<Settlement>, Without<SettlementBuilding>)>,
        Query<&mut GoodsInventory, With<SettlementBuilding>>,
    )>,
) {
    let Some(day) = world_time.iter().next().map(|time| time.day) else {
        return;
    };

    let mut stores: HashMap<&str, Vec<Entity>> = HashMap::new();
    let mut pantries: HashMap<&str, Vec<Entity>> = HashMap::new();
    let mut housing: HashMap<&str, u32> = HashMap::new();
    let mut employed: HashMap<&str, u32> = HashMap::new();
    for (entity, building) in buildings.iter() {
        stores
            .entry(building.settlement.as_str())
            .or_default()
            .push(entity);
        if building.kind == SettlementBuildingKind::House {
            pantries
                .entry(building.settlement.as_str())
                .or_default()
                .push(entity);
        }
        *housing.entry(building.settlement.as_str()).or_default() +=
            u32::from(building.kind.housing_capacity());
        *employed.entry(building.settlement.as_str()).or_default() += building.workers.len() as u32;
    }
    for store_entities in stores.values_mut() {
        store_entities.sort_unstable_by_key(|entity| entity.to_bits());
    }
    for pantry_entities in pantries.values_mut() {
        pantry_entities.sort_unstable_by_key(|entity| entity.to_bits());
    }

    for (entity, mut settlement, mut economy, mut market, policies) in settlements.iter_mut() {
        let day_state = runtime
            .by_settlement
            .entry(entity)
            .or_insert_with(|| SettlementEconomyDay::new(day));
        let elapsed_days = day.saturating_sub(day_state.last_world_day);
        let mut advanced_day = false;

        for _ in 0..elapsed_days {
            let demand = settlement.residents;
            let meal_day = day_state.last_world_day.saturating_add(1);
            let consumed = if let Some(market) = market.as_deref_mut() {
                // A ration is now a real purchase. Only goods the Moot bought
                // into its physical hall inventory are market stock; a farmer's
                // private shed is not silently raided at midnight.
                let mut consumed = 0u32;
                let mut unaffordable = Vec::new();
                let mut individual_buyers = Vec::new();
                // Housed residents eat from their shared physical pantry.
                // Unhoused residents still buy one ration personally at the
                // Moot because they have no household budget or storage.
                for (resident, intent, _, nutrition, home) in residents.iter_mut() {
                    if intent.settlement() != Some(entity) {
                        continue;
                    }
                    let fed = home.is_some_and(|home| {
                        inventories
                            .p1()
                            .get_mut(home.home)
                            .is_ok_and(|mut pantry| pantry.remove_edible(1) == 1)
                    });
                    if fed {
                        consumed = consumed.saturating_add(1);
                        if let Some(mut nutrition) = nutrition {
                            nutrition.record_meal(meal_day);
                        }
                    } else if home.is_some() {
                        if let Some(mut nutrition) = nutrition {
                            nutrition.record_missed_meal();
                        }
                        unaffordable.push(resident);
                    } else {
                        individual_buyers.push(resident);
                    }
                }
                if let Ok(mut hall) = inventories.p0().get_mut(entity) {
                    for resident in individual_buyers {
                        let Ok((_, _, wallet, nutrition, _)) = residents.get_mut(resident) else {
                            continue;
                        };
                        let Some(mut wallet) = wallet else {
                            if let Some(mut nutrition) = nutrition {
                                nutrition.record_missed_meal();
                            }
                            unaffordable.push(resident);
                            continue;
                        };
                        let mut fed = false;
                        for good in [Good::Food, Good::Wheat] {
                            let trade = market.sell_to_consumer(
                                good,
                                hall.amount(good),
                                1,
                                wallet.balance(),
                            );
                            if trade.units == 1 && wallet.debit(trade.pennies) {
                                debug_assert_eq!(hall.remove(good, 1), 1);
                                consumed = consumed.saturating_add(1);
                                fed = true;
                                break;
                            }
                        }
                        if let Some(mut nutrition) = nutrition {
                            if fed {
                                nutrition.record_meal(meal_day);
                            } else {
                                nutrition.record_missed_meal();
                            }
                        }
                        if !fed {
                            unaffordable.push(resident);
                        }
                    }

                    // Solvent residents buy first. Poor Relief then sees the
                    // real remaining surplus and cannot consume the emergency
                    // floor out from under ordinary daily demand.
                    if let Some(policy) = policies.filter(|policy| policy.poor_relief) {
                        let reserve_floor =
                            demand.saturating_mul(u32::from(policy.poor_relief_reserve_days));
                        let production_is_sustainable =
                            day_state.recent_production_including_today() >= demand as f32;

                        unaffordable.sort_unstable_by_key(|resident| resident.to_bits());
                        for resident in unaffordable {
                            // Public relief is a real market purchase, but only
                            // sustainable surplus is eligible: production must
                            // cover the roster and the purchase must leave the
                            // configured number of full resident-days intact.
                            if !production_is_sustainable
                                || hall.edible_amount().saturating_sub(1) < reserve_floor
                            {
                                break;
                            }
                            let mut relieved = false;
                            for good in [Good::Food, Good::Wheat] {
                                let trade = market.sell_to_consumer(
                                    good,
                                    hall.amount(good),
                                    1,
                                    settlement.treasury,
                                );
                                if trade.units == 1 && settlement.treasury >= trade.pennies {
                                    settlement.treasury -= trade.pennies;
                                    debug_assert_eq!(hall.remove(good, 1), 1);
                                    consumed = consumed.saturating_add(1);
                                    relieved = true;
                                    break;
                                }
                            }
                            if relieved {
                                if let Ok((_, _, _, Some(mut nutrition), _)) =
                                    residents.get_mut(resident)
                                {
                                    nutrition.record_meal(meal_day);
                                }
                            }
                        }
                    }
                }
                consumed.min(demand)
            } else {
                // Migration/test compatibility for a malformed pre-market
                // world. Production wiring always installs a market first.
                let mut remaining = demand;
                if remaining > 0 {
                    if let Ok(mut hall) = inventories.p0().get_mut(entity) {
                        remaining = remaining.saturating_sub(hall.remove_edible(remaining));
                    }
                    if remaining > 0 {
                        for store in stores.get(settlement.name.as_str()).into_iter().flatten() {
                            if let Ok(mut inventory) = inventories.p1().get_mut(*store) {
                                remaining =
                                    remaining.saturating_sub(inventory.remove_edible(remaining));
                            }
                            if remaining == 0 {
                                break;
                            }
                        }
                    }
                }
                let consumed = demand.saturating_sub(remaining);
                // Old/no-market worlds still produce an honest per-person
                // result. Entity order keeps the fallback deterministic; live
                // worlds use the wallet-aware branch above.
                let mut local_residents: Vec<Entity> = residents
                    .iter_mut()
                    .filter_map(|(resident, intent, _, _, _)| {
                        (intent.settlement() == Some(entity)).then_some(resident)
                    })
                    .collect();
                local_residents.sort_unstable_by_key(|resident| resident.to_bits());
                for (index, resident) in local_residents.into_iter().enumerate() {
                    if let Ok((_, _, _, Some(mut nutrition), _)) = residents.get_mut(resident) {
                        if index < consumed as usize {
                            nutrition.record_meal(meal_day);
                        } else {
                            nutrition.record_missed_meal();
                        }
                    }
                }
                consumed
            };
            economy.unmet_food = demand.saturating_sub(consumed);
            day_state.finish_day(consumed);
            day_state.last_world_day = day_state.last_world_day.saturating_add(1);
            advanced_day = true;
        }

        let mut stock = inventories
            .p0()
            .get_mut(entity)
            .map_or(0, |inventory| inventory.edible_amount());
        for pantry in pantries.get(settlement.name.as_str()).into_iter().flatten() {
            stock = stock.saturating_add(
                inventories
                    .p1()
                    .get_mut(*pantry)
                    .map_or(0, |inventory| inventory.edible_amount()),
            );
        }
        // Before finance initialisation, retain the old aggregate reading for
        // focused unit tests. In a live market, only Moot-owned stock is a
        // reserve residents can actually purchase.
        if market.is_none() {
            for store in stores.get(settlement.name.as_str()).into_iter().flatten() {
                if pantries
                    .get(settlement.name.as_str())
                    .is_some_and(|houses| houses.contains(store))
                {
                    continue;
                }
                stock = stock.saturating_add(
                    inventories
                        .p1()
                        .get_mut(*store)
                        .map_or(0, |inventory| inventory.edible_amount()),
                );
            }
        }

        let residents = settlement.residents;
        economy.edible_stock = stock;
        economy.reserve_days = if residents == 0 {
            0.0
        } else {
            stock as f32 / residents as f32
        };
        economy.recent_food_production = SettlementEconomyDay::recent_average(
            &day_state.production_history,
            day_state.recorded_days,
        );
        economy.recent_food_consumption = SettlementEconomyDay::recent_average(
            &day_state.consumption_history,
            day_state.recorded_days,
        );
        economy.observed_days = economy
            .observed_days
            .saturating_add(elapsed_days.min(u32::from(u16::MAX)) as u16);

        let demand = residents.max(1) as f32;
        economy.reserve_prosperity =
            (economy.reserve_days / FOOD_SECURITY_TARGET_DAYS).clamp(0.0, 1.0) * 40.0;
        economy.production_prosperity =
            (economy.recent_food_production / demand).clamp(0.0, 1.0) * 30.0;
        economy.housing_prosperity =
            (housing.get(settlement.name.as_str()).copied().unwrap_or(0) as f32 / demand)
                .clamp(0.0, 1.0)
                * 20.0;
        economy.employment_prosperity =
            (employed.get(settlement.name.as_str()).copied().unwrap_or(0) as f32 / demand)
                .clamp(0.0, 1.0)
                * 10.0;
        economy.hunger_penalty = -(economy.unmet_food as f32 / demand).clamp(0.0, 1.0) * 30.0;
        economy.prosperity = (economy.reserve_prosperity
            + economy.production_prosperity
            + economy.housing_prosperity
            + economy.employment_prosperity
            + economy.hunger_penalty)
            .clamp(0.0, 100.0);

        if advanced_day {
            let secure = residents > 0
                && economy.reserve_days >= FOOD_SECURITY_TARGET_DAYS
                && economy.recent_food_production >= residents as f32
                && economy.unmet_food == 0;
            economy.food_secure_days = if secure {
                economy.food_secure_days.saturating_add(1)
            } else {
                0
            };

            if settlement.tier == shared::components::SettlementTier::Hamlet
                && residents >= VILLAGE_MIN_RESIDENTS
                && economy.food_secure_days >= VILLAGE_REQUIRED_SECURE_DAYS
                && economy.prosperity >= VILLAGE_MIN_PROSPERITY
            {
                settlement.tier = shared::components::SettlementTier::Village;
                info!(
                    "Village '{}': advanced from Hamlet to Village with {:.0} prosperity and {:.1} reserve days",
                    settlement.name, economy.prosperity, economy.reserve_days
                );
            }
        }
    }
}

/// Give every villager an intent, so the rest of the module can assume one.
///
/// Polls rather than reacting to `Added`, for the reason this codebase has now
/// been bitten by repeatedly: components arrive in separate batches and a
/// one-shot on `Added` misses whoever was assembled late.
pub fn tag_villager_intent(
    mut commands: Commands,
    villagers: Query<
        (
            Entity,
            &CharacterKind,
            Option<&VillagerIntent>,
            Option<&Occupation>,
            Option<&GoodsInventory>,
            Option<&CharacterActivity>,
            Option<&CarriedLoad>,
            Option<&Nutrition>,
            Option<&WorkStatus>,
        ),
        (With<CharacterName>, With<PlayerPosition>),
    >,
) {
    for (entity, kind, intent, occupation, inventory, activity, carried, nutrition, work_status) in
        villagers.iter()
    {
        // Heroes are players' bodies and join nothing on their own.
        if *kind != CharacterKind::Villager {
            continue;
        }
        let mut entity_commands = commands.entity(entity);
        if intent.is_none() {
            entity_commands.insert(VillagerIntent::Idle);
        }
        // These are backfilled as well as attached by the normal spawn path so
        // old saves and focused tests cannot create bottomless or visually
        // ambiguous villagers.
        if occupation.is_none() {
            entity_commands.insert(Occupation::default());
        }
        if inventory.is_none() {
            entity_commands.insert(GoodsInventory::new(shared::economy::capacity::VILLAGER));
        }
        if activity.is_none() {
            entity_commands.insert(CharacterActivity::Idle);
        }
        if carried.is_none() {
            entity_commands.insert(CarriedLoad::default());
        }
        if nutrition.is_none() {
            entity_commands.insert(Nutrition::default());
        }
        if work_status.is_none() {
            entity_commands.insert(if occupation.is_some_and(|value| value.0.is_some()) {
                WorkStatus::Employed
            } else {
                WorkStatus::LookingForWork
            });
        }
    }
}

/// Uncommitted villagers pick somewhere to live and start walking.
pub fn seek_settlement(
    time: Res<Time>,
    warp: Query<&shared::components::TimeWarp>,
    mut clock: ResMut<VillageClock>,
    mut commands: Commands,
    settlements: Query<(
        Entity,
        &Settlement,
        &PlayerPosition,
        Option<&PlayerRotation>,
    )>,
    mut villagers: Query<(
        Entity,
        &PlayerPosition,
        &mut VillagerIntent,
        Option<&MigrationCooldown>,
    )>,
) {
    clock.seek += time.delta_secs() * time_warp_factor(&warp);
    if clock.seek < SEEK_INTERVAL {
        return;
    }
    clock.seek = 0.0;

    let now = time.elapsed_secs_f64();

    for (entity, position, mut intent, cooldown) in villagers.iter_mut() {
        if !matches!(*intent, VillagerIntent::Idle) {
            continue;
        }
        // Nearest non-ruined settlement. Ruins have no hall to walk to and
        // nobody to join.
        let nearest = settlements
            .iter()
            .filter(|(_, settlement, _, _)| {
                settlement.tier != shared::components::SettlementTier::Ruins
            })
            .filter(|(entity, ..)| {
                cooldown.is_none_or(|cooldown| {
                    cooldown.settlement != *entity || now >= cooldown.retry_after
                })
            })
            .min_by(|a, b| {
                a.2 .0
                    .distance_squared(position.0)
                    .total_cmp(&b.2 .0.distance_squared(position.0))
            });
        // No settlement anywhere: stay idle and look again next tick. A villager
        // with nowhere to go is a real state, not an error.
        let Some((settlement_entity, _, hall, rotation)) = nearest else {
            continue;
        };
        if cooldown.is_some_and(|cooldown| cooldown.settlement != settlement_entity) {
            commands.entity(entity).remove::<MigrationCooldown>();
        }
        *intent = VillagerIntent::Travelling {
            settlement: settlement_entity,
        };
        // The hall centre is inside its solid navigation footprint. Before
        // authored building obstacles existed, walking there happened to work;
        // now it correctly produces no route and can strand every founder in
        // `Travelling`. The authored entrance is both reachable and still
        // inside ARRIVAL_RADIUS of the settlement origin.
        let entrance = SettlementBuildingKind::Hall
            .entrance_position(hall.0, rotation.map_or(0.0, |rotation| rotation.0));
        commands.entity(entity).insert(MoveTarget(entrance));
    }
}

/// Villagers who reached their hall become residents of that settlement.
pub fn arrive_at_settlement(
    mut commands: Commands,
    time: Option<Res<Time>>,
    halls: Query<(&PlayerPosition, &Settlement, Option<&PlayerRotation>)>,
    mut villagers: Query<(
        Entity,
        &PlayerPosition,
        &mut VillagerIntent,
        Option<&MoveTarget>,
        Option<&NavigationRouteFailed>,
        Option<&MigrationCooldown>,
    )>,
) {
    let now = time.as_ref().map_or(0.0, |time| time.elapsed_secs_f64());
    for (entity, position, mut intent, move_target, route_failed, cooldown) in villagers.iter_mut()
    {
        let VillagerIntent::Travelling { settlement } = *intent else {
            continue;
        };
        // The settlement went away mid-journey: go back to looking.
        let Ok((hall, place, rotation)) = halls.get(settlement) else {
            *intent = VillagerIntent::Idle;
            commands
                .entity(entity)
                .remove::<MoveTarget>()
                .remove::<Residence>();
            continue;
        };
        if position.0.distance(hall.0) > ARRIVAL_RADIUS {
            if let Some(failed) = route_failed {
                debug!(
                    "Villager could not migrate to '{}' through {:.1},{:.1}; reconsidering after cooldown",
                    place.name, failed.goal.x, failed.goal.z
                );
                *intent = VillagerIntent::Idle;
                commands
                    .entity(entity)
                    .remove::<MoveTarget>()
                    .remove::<TravelRoute>()
                    .remove::<NavigationRoutePending>()
                    .remove::<NavigationRouteFailed>()
                    .insert(MigrationCooldown::after_failure(
                        cooldown.copied(),
                        settlement,
                        now,
                    ));
                continue;
            }
            // Repair old saves/live entities that still point at the solid hall
            // centre, and recover if any other system removed the journey. A
            // changed target also wakes the bounded route planner after its
            // previous blocked-route retry limit.
            let entrance = SettlementBuildingKind::Hall
                .entrance_position(hall.0, rotation.map_or(0.0, |rotation| rotation.0));
            ensure_move_target(&mut commands, entity, move_target, entrance);
            continue;
        }
        *intent = VillagerIntent::Resident { settlement };
        // Residence is the replicated half of the same fact, so a client can
        // name who lives where without knowing anything about intents.
        commands
            .entity(entity)
            .remove::<MoveTarget>()
            .remove::<TravelRoute>()
            .remove::<NavigationRoutePending>()
            .remove::<NavigationRouteFailed>()
            .remove::<MigrationCooldown>()
            .insert(Residence(place.name.clone()));
    }
}

/// Recount every settlement's residents from the villagers who live there.
///
/// DERIVED every tick rather than incremented on arrival, because a counter
/// that is nudged by events drifts the first time an event is missed -- and a
/// resident count that disagrees with the people standing there is exactly the
/// kind of lie the encyclopedia must never tell.
pub fn recount_residents(
    mut settlements: Query<(Entity, &mut Settlement)>,
    villagers: Query<&VillagerIntent>,
) {
    let mut counts: HashMap<Entity, u32> = HashMap::new();
    for intent in villagers.iter() {
        if !intent.counts_as_resident() {
            continue;
        }
        let settlement = intent
            .settlement()
            .expect("resident intents always name their settlement");
        *counts.entry(settlement).or_default() += 1;
    }

    for (entity, mut settlement) in settlements.iter_mut() {
        let count = counts.get(&entity).copied().unwrap_or(0);
        // Change detection drives replication; an idle village must not
        // re-send its count every tick.
        if settlement.residents != count {
            info!(
                "Village '{}': {} resident(s), was {}",
                settlement.name, count, settlement.residents
            );
            settlement.residents = count;
        }
    }
}

/// What a settlement wants next, counting completed and already-approved
/// structures alike.
///
/// The founding order remains legible, but needs no longer stop forever after
/// three buildings: housing follows real bed pressure and observed food
/// shortages can request more Farmsteads up to the available population.
pub fn next_need(
    existing: &HashMap<SettlementBuildingKind, usize>,
    residents: u32,
    economy: Option<&SettlementEconomy>,
) -> Option<SettlementBuildingKind> {
    let count = |kind| existing.get(&kind).copied().unwrap_or(0);
    shared::economy::next_settlement_building(
        count(SettlementBuildingKind::Farmstead),
        count(SettlementBuildingKind::FishermansHut),
        count(SettlementBuildingKind::LumberjackHut),
        count(SettlementBuildingKind::House),
        residents,
        economy,
    )
}

/// Tier infrastructure is requested only after survival/housing shortages are
/// satisfied. The charter influences where it goes, not whether demand and the
/// current tier justify it.
fn next_civic_need(
    tier: shared::components::SettlementTier,
    existing: &HashMap<SettlementBuildingKind, usize>,
) -> Option<SettlementBuildingKind> {
    let has = |kind| existing.get(&kind).copied().unwrap_or(0) > 0;
    match tier {
        shared::components::SettlementTier::Village => {
            if !has(SettlementBuildingKind::Market) {
                Some(SettlementBuildingKind::Market)
            } else if !has(SettlementBuildingKind::Tavern) {
                Some(SettlementBuildingKind::Tavern)
            } else {
                None
            }
        }
        shared::components::SettlementTier::Town => {
            (!has(SettlementBuildingKind::Church)).then_some(SettlementBuildingKind::Church)
        }
        _ => None,
    }
}

/// A resident applies for a permit, and it is approved if it is valid.
///
/// Distinct decisions may be in flight together. Planned kinds count as already
/// had, each applicant can hold only one active build, and pending plots reserve
/// their ground, so concurrency cannot duplicate or overlap construction.
/// Needed housing approval is free. A business permit always moves personal
/// coin into the settlement treasury, discounted when the settlement requested
/// that trade and progressively dearer for residents with several holdings.
#[allow(clippy::too_many_arguments)]
pub fn consider_permits(
    time: Res<Time>,
    warp: Query<&shared::components::TimeWarp>,
    mut clock: ResMut<VillageClock>,
    mut commands: Commands,
    planning: PermitPlanningResources,
    mut settlements: Query<(Entity, &mut Settlement, &PlayerPosition)>,
    economies: Query<&SettlementEconomy>,
    developments: Query<&shared::components::SettlementDevelopment>,
    buildings: Query<&SettlementBuilding>,
    pending: Query<&UnderConstruction>,
    placed: Query<(
        &SettlementBuilding,
        &PlayerPosition,
        Option<&PlayerRotation>,
    )>,
    roads: Query<&VillageRoad>,
    administrations: Query<&MootAdministration>,
    // ONE query, read then written. Two -- a read of `&VillagerIntent` and a
    // write of `&mut VillagerIntent` -- is a genuine conflict Bevy refuses at
    // runtime, and iterating a mutable query gives read-only items anyway.
    mut villagers: Query<(
        Entity,
        &CharacterName,
        &mut VillagerIntent,
        Option<&WorkStatus>,
    )>,
    road_stewards: Query<(), With<RoadSteward>>,
    mut wallets: Query<&mut Wallet>,
) {
    clock.permit += time.delta_secs() * time_warp_factor(&warp);
    if clock.permit < PERMIT_INTERVAL {
        return;
    }
    clock.permit = 0.0;

    let Some(terrain) = planning.terrain.as_deref() else {
        return;
    };

    for (settlement_entity, mut settlement, hall) in settlements.iter_mut() {
        // Count what stands AND what is already approved, or three residents
        // deciding on successive permit ticks all build the same thing.
        let mut have: HashMap<SettlementBuildingKind, usize> = HashMap::new();
        for building in buildings
            .iter()
            .filter(|building| building.settlement == settlement.name)
        {
            *have.entry(building.kind).or_default() += 1;
        }
        for under in pending.iter() {
            if under.settlement == settlement_entity {
                *have.entry(under.kind).or_default() += 1;
            }
        }
        // The hall is always there; it is the founding act, not a need.
        have.insert(SettlementBuildingKind::Hall, 1);

        let requested = next_need(
            &have,
            settlement.residents,
            economies.get(settlement_entity).ok(),
        )
        .or_else(|| next_civic_need(settlement.tier, &have));
        let planned_farms = have
            .get(&SettlementBuildingKind::Farmstead)
            .copied()
            .unwrap_or(0);
        let planned_fishers = have
            .get(&SettlementBuildingKind::FishermansHut)
            .copied()
            .unwrap_or(0);
        let initial_food_request = planned_farms + planned_fishers == 0;
        let may_add_complementary_fishing =
            requested.is_none() && planned_farms > 0 && planned_fishers == 0;

        if requested.is_none() && !may_add_complementary_fishing {
            continue;
        }

        // Occupied ground, so a new building does not land on an old one.
        let mut occupied: Vec<(Vec3, f32)> = placed
            .iter()
            .filter(|(building, _, _)| building.settlement == settlement.name)
            .map(|(building, position, _)| (position.0, building.kind.clearance()))
            .chain(std::iter::once((
                hall.0,
                SettlementBuildingKind::Hall.clearance(),
            )))
            .chain(pending.iter().filter_map(|under| {
                (under.settlement == settlement_entity)
                    .then_some((under.position, under.kind.clearance()))
            }))
            .collect();

        // A Farmstead owns more ground than its cabin. Reserve the separate
        // crop plot as well, including while construction is pending, so a
        // later building cannot be approved on top of the wheat rows.
        occupied.extend(
            placed
                .iter()
                .filter(|(building, _, _)| building.settlement == settlement.name)
                .flat_map(|(building, position, rotation)| {
                    let rotation = rotation.map_or(0.0, |rotation| rotation.0);
                    let radius = building
                        .kind
                        .field_half_extents()
                        .map(|half| half.length() + shared::components::FARM_FIELD_EDGE_CLEARANCE);
                    building
                        .kind
                        .field_positions(position.0, rotation)
                        .into_iter()
                        .flatten()
                        .filter_map(move |field| radius.map(|radius| (field, radius)))
                }),
        );
        occupied.extend(
            pending
                .iter()
                .filter(|under| under.settlement == settlement_entity)
                .flat_map(|under| {
                    let radius = under
                        .kind
                        .field_half_extents()
                        .map(|half| half.length() + shared::components::FARM_FIELD_EDGE_CLEARANCE);
                    under
                        .kind
                        .field_positions(under.position, under.rotation)
                        .into_iter()
                        .flatten()
                        .filter_map(move |field| radius.map(|radius| (field, radius)))
                }),
        );

        let village_roads: Vec<_> = roads
            .iter()
            .filter(|road| road.settlement == settlement.name)
            .collect();
        let missing = requested.unwrap_or(SettlementBuildingKind::FishermansHut);
        let search_signature = FailedSiteSearch {
            kind: missing,
            residents: settlement.residents,
            occupied_plots: occupied.len(),
            roads: village_roads.len(),
            terrain_version: terrain.modification_version(),
        };
        if clock
            .failed_site_searches
            .get(&settlement_entity)
            .is_some_and(|failed| *failed == search_signature)
        {
            continue;
        }
        // A food permit becomes a fishing workplace when the authored hut and
        // pier can form a convincing land-to-water pair. Inland settlements
        // keep the farmstead path unchanged.
        let coastal_site = (initial_food_request || may_add_complementary_fishing)
            .then(|| find_fishing_site(terrain, hall.0, &occupied, &village_roads))
            .flatten();
        let ordinary_site = requested.and_then(|kind| {
            find_site_with_plan(
                terrain,
                hall.0,
                kind,
                &occupied,
                &village_roads,
                developments.get(settlement_entity).ok(),
                planning.colliders.as_deref(),
                planning.derived.as_deref(),
            )
            .map(|(position, rotation)| (kind, position, rotation))
        });
        let mut approved_site = if let Some((position, rotation, quality)) =
            coastal_site.filter(|_| initial_food_request || may_add_complementary_fishing)
        {
            Some((
                SettlementBuildingKind::FishermansHut,
                position,
                rotation,
                quality,
            ))
        } else if let Some((kind, position, rotation)) = ordinary_site {
            Some((
                kind,
                position,
                rotation,
                site_quality(terrain, kind, position),
            ))
        } else {
            None
        };

        // One geographically impossible business must not freeze the entire
        // permit queue. Treat the unavailable preferred kind as provisionally
        // satisfied, ask the same shortage model what would come next, and
        // approve that real need if it has a viable plot. The unavailable kind
        // remains absent, so it will be reconsidered after settlement geometry
        // or terrain changes instead of being silently granted.
        if approved_site.is_none() {
            if let Some(unavailable) = requested {
                let mut assumed_have = have.clone();
                *assumed_have.entry(unavailable).or_default() += 1;
                let alternative = next_need(
                    &assumed_have,
                    settlement.residents,
                    economies.get(settlement_entity).ok(),
                )
                .or_else(|| next_civic_need(settlement.tier, &assumed_have));
                if let Some(alternative) = alternative.filter(|kind| *kind != unavailable) {
                    approved_site = find_site_with_plan(
                        terrain,
                        hall.0,
                        alternative,
                        &occupied,
                        &village_roads,
                        developments.get(settlement_entity).ok(),
                        planning.colliders.as_deref(),
                        planning.derived.as_deref(),
                    )
                    .map(|(position, rotation)| {
                        (
                            alternative,
                            position,
                            rotation,
                            site_quality(terrain, alternative, position),
                        )
                    });
                }
            }
        }

        let Some((kind, position, rotation, quality)) = approved_site else {
            info!(
                "Village '{}': nowhere to put a {} yet",
                settlement.name,
                missing.label()
            );
            clock
                .failed_site_searches
                .insert(settlement_entity, search_signature);
            continue;
        };
        clock.failed_site_searches.remove(&settlement_entity);

        // A resident applies only after geography has selected the actual
        // permit kind. This prevents an impossible lumber permit from applying
        // lumber prices or eligibility rules to a fallback house or farm.
        // The applicant is whoever holds the fewest completed and approved
        // plots, with stable name ordering for reproducible runs.
        let holdings = |who: &str| -> usize {
            buildings
                .iter()
                .filter(|building| {
                    building.settlement == settlement.name && building.owner.as_deref() == Some(who)
                })
                .count()
                + pending
                    .iter()
                    .filter(|under| {
                        under.settlement == settlement_entity && under.owner.as_deref() == Some(who)
                    })
                    .count()
        };
        let administration = administrations.get(settlement_entity).ok();
        let reeve = administration.and_then(|office| office.reeve.as_deref());
        let civic_staff: HashSet<&str> = administration
            .into_iter()
            .flat_map(|office| {
                office
                    .reeve
                    .iter()
                    .chain(office.market_porter.iter())
                    .chain(office.city_workers.iter())
                    .chain(office.guards.iter())
            })
            .map(String::as_str)
            .collect();
        let applicant = villagers
            .iter()
            .filter(|(_, _, intent, _)| {
                matches!(intent, VillagerIntent::Resident { settlement } if *settlement == settlement_entity)
            })
            .filter_map(|(entity, name, _, status)| {
                // Owning a building does not create a second job, but its
                // construction still needs the applicant's physical time.
                // Never tear somebody out of a field, workplace doorway or
                // fishing pier mid-shift. Employed residents become eligible
                // again after their bounded work routine ends for the day.
                if planning.permit_busy.get(entity).is_ok() {
                    return None;
                }
                if kind.is_civic() {
                    if reeve != Some(name.0.as_str()) {
                        return None;
                    }
                } else if road_stewards.get(entity).is_ok()
                    || civic_staff.contains(name.0.as_str())
                {
                    return None;
                }
                let holding_count = holdings(&name.0);
                let fee = permit_price(kind, holding_count, requested.is_some());
                let balance = wallets
                    .get(entity)
                    .map(|wallet| wallet.balance())
                    .unwrap_or(shared::economy::STARTING_VILLAGER_MONEY);
                let wealthy_investor = kind != SettlementBuildingKind::House
                    && status.is_some_and(|status| *status == WorkStatus::Chilling)
                    && holding_count > 0;
                (balance >= fee).then_some((
                    entity,
                    name.0.clone(),
                    holding_count,
                    u8::from(!wealthy_investor),
                ))
            })
            .min_by(|a, b| {
                a.3.cmp(&b.3)
                    .then_with(|| a.2.cmp(&b.2))
                    .then_with(|| a.1.cmp(&b.1))
            });
        let Some((builder, applicant, applicant_holdings, _)) = applicant else {
            info!(
                "Village '{}': nobody can afford its next {} permit",
                settlement.name,
                kind.label()
            );
            continue;
        };

        // Charge the actual approved kind. The coastal substitution has the
        // same base as a Farmstead today, but keeping this exact lets their
        // prices diverge later without charging for a building never granted.
        let fee = permit_price(kind, applicant_holdings, requested.is_some());
        if let Ok(mut wallet) = wallets.get_mut(builder) {
            if !wallet.debit(fee) {
                continue;
            }
        } else {
            // Old/test villagers without a wallet migrate into the live rule
            // with the same founding endowment, minus this real fee.
            commands.entity(builder).insert(Wallet::new(
                shared::economy::STARTING_VILLAGER_MONEY.saturating_sub(fee),
            ));
        }
        settlement.treasury = settlement.treasury.saturating_add(fee);

        // Auto-approved after payment. A permit still builds nothing: private
        // owners buy or gather the Wood, while civic work draws physical Wood
        // from the settlement's common hall inventory.
        // How good this ground is for this trade, sampled where it will stand
        // rather than at the hall. A farmstead on the settlement's best soil is
        // worth more than one behind the woodshed, and that has to be decided
        // by the plot, not the village.
        // Beside the plot, in front of it. The builder must not stand where the
        // building is about to rise.
        let stand = shared::components::builder_stand_position(
            position,
            rotation,
            kind.art().definition().footprint.y,
        );

        let site = commands
            .spawn((
                UnderConstruction {
                    kind,
                    position,
                    rotation,
                    // Progression amenities are public works. The applicant
                    // supplies builder time, while the settlement owns the
                    // completed structure and its common-stock material bill.
                    owner: (!kind.is_civic()).then_some(applicant.clone()),
                    builder: Some(builder),
                    settlement: settlement_entity,
                    stand,
                    stage: BuildStage::Supplying,
                    quality,
                },
                // The replicated half, so the panel can show it as approved.
                shared::components::ConstructionSite {
                    kind,
                    settlement: settlement.name.clone(),
                    raising: false,
                    stand,
                    rotation,
                },
                GoodsInventory::new(kind.construction_storage_bulk()),
                PlayerPosition(position),
                Replicate::to_clients(NetworkTarget::All),
            ))
            .id();

        // The permit does not build anything. Its builder first sources every
        // required wood bundle and carries it into this site's bounded pile.
        commands.entity(builder).remove::<MoveTarget>().insert((
            ConstructionMaterialRoutine {
                site,
                cycle: 0,
                failed_tree_routes: 0,
                failed_store_routes: 0,
                failed_delivery_routes: 0,
                tree_retry_after: 0.0,
                store_retry_after: 0.0,
                phase: ConstructionMaterialPhase::Seeking,
            },
            CharacterActivity::Idle,
        ));
        commands
            .entity(builder)
            .remove::<TravelRoute>()
            .remove::<NavigationRoutePending>()
            .remove::<NavigationRouteFailed>()
            .remove::<ambient::AmbientRoutine>();
        if let Ok(mut intent) = villagers.get_mut(builder).map(|(_, _, intent, _)| intent) {
            *intent = VillagerIntent::Building {
                settlement: settlement_entity,
                site,
            };
        }
        info!(
            "Village '{}': {applicant} paid {} coin for a {} permit at {:.0},{:.0}",
            settlement.name,
            shared::economy::format_money(fee),
            kind.label(),
            position.x,
            position.z
        );
    }
}

/// How well this ground suits what is being built, 0..1.
///
/// Reads the SAME `BiomeField::resources` the grass density and the economy
/// read, so a farmstead standing in thick grass really is standing on good
/// soil — the thing you can see is the thing the number says.
///
/// Slope is passed as zero deliberately. `find_site` already rejected anything
/// above `MAX_BUILD_SLOPE`, which is below every slope threshold inside
/// `biome()` and `resources()`, so at a legal plot the slope term cannot change
/// the answer. Inventing a second slope formula here would only create
/// something to disagree with the siting gate about.
pub fn site_quality(terrain: &WorldTerrain, kind: SettlementBuildingKind, at: Vec3) -> f32 {
    let Some(field) = terrain.generator.loaded_map().biome_field.as_deref() else {
        // Hand-authored maps carry no biome field. Neutral rather than zero: a
        // building that works nowhere is worse than one that works averagely.
        return 0.5;
    };
    let profile = field.resources(at.x, at.z, at.y, 0.0);
    kind.yield_quality(&profile)
}

/// Supply approved worksites with physical wood before construction begins.
///
/// Market Wood in the settlement hall is preferred and must be purchased by
/// the owner. If it is absent or unaffordable -- including the founding
/// deadlock before the first hut exists -- the builder walks to a real tree,
/// chops, carries what fits, and deposits it at the site.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn run_construction_material_logistics(
    time: Res<Time>,
    terrain: Option<Res<WorldTerrain>>,
    derived: Option<Res<DerivedColliderLibrary>>,
    obstacles: Option<Res<SpatialObstacleGrid>>,
    warp: Query<&shared::components::TimeWarp>,
    world_time: Query<&WorldTime>,
    mut commands: Commands,
    settlements: Query<
        (
            Entity,
            &Settlement,
            &PlayerPosition,
            Option<&PlayerRotation>,
        ),
        Without<CharacterKind>,
    >,
    sites: Query<(Entity, &UnderConstruction, &PlayerPosition), Without<CharacterKind>>,
    mut inventories: Query<&mut GoodsInventory>,
    mut markets: Query<&mut MootMarket>,
    mut builders: Query<
        (
            Entity,
            &CharacterName,
            &PlayerPosition,
            &VillagerIntent,
            Option<&HomeRoutine>,
            &mut PlayerRotation,
            &mut CharacterActivity,
            &mut ConstructionMaterialRoutine,
            Option<&MoveTarget>,
            Option<&NavigationRouteFailed>,
            Option<&mut Wallet>,
            Option<&ambient::AmbientRoutine>,
        ),
        With<CharacterKind>,
    >,
) {
    let Some(terrain) = terrain else {
        return;
    };
    if world_time
        .iter()
        .next()
        .is_some_and(|clock| !clock.is_day())
    {
        return;
    }
    let dt = time.delta_secs() * time_warp_factor(&warp);
    let now = time.elapsed_secs_f64();
    // A terrain proof is bounded but not free. Stagger simultaneous builders
    // across ticks instead of letting a migration wave perform the same
    // impossible-landmass search four times in one server frame.
    let mut tree_proof_used = false;
    // Scarce founding stock must finish useful buildings instead of being
    // smeared across every simultaneous permit. Prefer the site closest to
    // completion (then stable entity order) and advance to the next only after
    // it is fully supplied. Builders already carrying Wood may always finish
    // their delivery.
    let mut material_priority: HashMap<Entity, (u32, u64, Entity)> = HashMap::new();
    for (site_entity, site, _) in sites.iter() {
        if site.stage != BuildStage::Supplying {
            continue;
        }
        let delivered = inventories
            .get(site_entity)
            .map(|inventory| inventory.amount(Good::Wood))
            .unwrap_or(0);
        let remaining = site
            .kind
            .construction_wood_required()
            .saturating_sub(delivered);
        if remaining == 0 {
            continue;
        }
        let candidate = (remaining, site_entity.to_bits(), site_entity);
        let priority = material_priority
            .entry(site.settlement)
            .or_insert(candidate);
        if (candidate.0, candidate.1) < (priority.0, priority.1) {
            *priority = candidate;
        }
    }

    for (
        builder,
        name,
        position,
        intent,
        home_routine,
        mut facing,
        mut activity,
        mut routine,
        move_target,
        route_failed,
        mut wallet,
        ambient_routine,
    ) in builders.iter_mut()
    {
        if home_routine.is_some() {
            continue;
        }
        if !matches!(intent, VillagerIntent::Building { site, .. } if *site == routine.site) {
            commands
                .entity(builder)
                .remove::<ConstructionMaterialRoutine>()
                .remove::<MoveTarget>();
            *activity = CharacterActivity::Idle;
            continue;
        }
        let Ok((_, site, site_position)) = sites.get(routine.site) else {
            commands
                .entity(builder)
                .remove::<ConstructionMaterialRoutine>()
                .remove::<MoveTarget>();
            *activity = CharacterActivity::Idle;
            continue;
        };
        if site.stage != BuildStage::Supplying {
            commands
                .entity(builder)
                .remove::<ConstructionMaterialRoutine>();
            continue;
        }

        if let Some(failed) = route_failed {
            if matches!(
                routine.phase,
                ConstructionMaterialPhase::WalkingToTree { .. }
            ) {
                debug!(
                    "Village supplier {} could not reach tree stand {:.1},{:.1}; trying another tree",
                    name.0, failed.goal.x, failed.goal.z
                );
                let widened = postpone_construction_tree_search(&mut routine, now);
                if widened {
                    warn!(
                        "Village supplier {} exhausted twelve tree approaches; pausing the unavailable timber search",
                        name.0
                    );
                }
                commands
                    .entity(builder)
                    .remove::<NavigationRouteFailed>()
                    .remove::<NavigationRoutePending>()
                    .remove::<TravelRoute>()
                    .remove::<MoveTarget>();
            } else if matches!(routine.phase, ConstructionMaterialPhase::Delivering { .. }) {
                // A valid plot can still have one bad interaction side (a
                // steep rear bank, water edge or prop cluster). Delivery does
                // not require the builder to stand at the hammering anchor:
                // rotate around the site's accessible perimeter instead of
                // pinning the whole village to one failed endpoint forever.
                routine.failed_delivery_routes = routine.failed_delivery_routes.wrapping_add(1);
                let attempt = u32::from(routine.failed_delivery_routes);
                let angle = (attempt % 12) as f32 * std::f32::consts::TAU / 12.0;
                let radius = site.kind.clearance() + 1.5 + (attempt / 12) as f32 * 2.0;
                let x = site_position.0.x + angle.cos() * radius;
                let z = site_position.0.z + angle.sin() * radius;
                let destination = Vec3::new(x, terrain.get_height(x, z), z);
                debug!(
                    "Village supplier {} could not reach delivery anchor {:.1},{:.1}; trying perimeter approach {:.1},{:.1}",
                    name.0,
                    failed.goal.x,
                    failed.goal.z,
                    destination.x,
                    destination.z,
                );
                routine.phase = ConstructionMaterialPhase::Delivering { destination };
                commands
                    .entity(builder)
                    .remove::<NavigationRouteFailed>()
                    .remove::<NavigationRoutePending>()
                    .remove::<TravelRoute>()
                    .insert(MoveTarget(destination));
            } else if matches!(
                routine.phase,
                ConstructionMaterialPhase::CollectingFromStore { .. }
                    | ConstructionMaterialPhase::UnloadingAtHall { .. }
            ) {
                // The old implementation handled failed tree and site routes
                // but left a failed trip to the Moot store attached forever.
                // Back off from that entrance and gather timber directly so a
                // single inaccessible market approach cannot freeze a site.
                debug!(
                    "Village supplier {} could not reach the Moot store at {:.1},{:.1}; falling back to gathered timber",
                    name.0, failed.goal.x, failed.goal.z
                );
                postpone_construction_store_route(&mut routine, now);
                commands
                    .entity(builder)
                    .remove::<NavigationRouteFailed>()
                    .remove::<NavigationRoutePending>()
                    .remove::<TravelRoute>()
                    .remove::<MoveTarget>();
            } else {
                // Seeking and Chopping do not own navigation. Any failure seen
                // there belongs to an older task and must never become a
                // permanent guard that suppresses construction every tick.
                commands
                    .entity(builder)
                    .remove::<NavigationRouteFailed>()
                    .remove::<NavigationRoutePending>()
                    .remove::<TravelRoute>()
                    .remove::<MoveTarget>();
            }
            continue;
        }

        let required = site.kind.construction_wood_required();
        let delivered = inventories
            .get(routine.site)
            .map(|inventory| inventory.amount(Good::Wood))
            .unwrap_or(0);
        if delivered >= required {
            *activity = CharacterActivity::Idle;
            commands
                .entity(builder)
                .remove::<ConstructionMaterialRoutine>()
                .insert(MoveTarget(site.stand));
            continue;
        }

        let Some((hall_entity, settlement, hall_position, hall_rotation)) = settlements
            .iter()
            .find(|(entity, _, _, _)| *entity == site.settlement)
        else {
            continue;
        };
        let hall_entrance = SettlementBuildingKind::Hall.entrance_position(
            hall_position.0,
            hall_rotation.map(|rotation| rotation.0).unwrap_or(0.0),
        );
        let carried_wood = inventories
            .get(builder)
            .map(|inventory| inventory.amount(Good::Wood))
            .unwrap_or(0);
        let carries_other_goods = inventories.get(builder).is_ok_and(|inventory| {
            Good::ALL
                .iter()
                .any(|good| *good != Good::Wood && inventory.amount(*good) > 0)
        });
        let owns_material_priority = material_priority
            .get(&site.settlement)
            .is_none_or(|(_, _, priority)| *priority == routine.site);

        match routine.phase {
            ConstructionMaterialPhase::Seeking => {
                *activity = CharacterActivity::Idle;
                if carried_wood > 0 {
                    commands.entity(builder).remove::<ambient::AmbientRoutine>();
                    ensure_move_target(&mut commands, builder, move_target, site.stand);
                    routine.phase = ConstructionMaterialPhase::Delivering {
                        destination: site.stand,
                    };
                    continue;
                }
                if carries_other_goods {
                    commands.entity(builder).remove::<ambient::AmbientRoutine>();
                    ensure_move_target(&mut commands, builder, move_target, hall_entrance);
                    routine.phase = ConstructionMaterialPhase::UnloadingAtHall {
                        hall: hall_entity,
                        entrance: hall_entrance,
                    };
                    continue;
                }
                let affordable = site.owner.is_none()
                    || match (markets.get(hall_entity), wallet.as_deref()) {
                        (Ok(market), Some(wallet)) => {
                            wallet.can_afford(market.pool(Good::Wood).ask)
                        }
                        // Compatibility for focused tests assembled without the
                        // finance initialiser. A live village always has both.
                        _ => true,
                    };
                let source = inventories
                    .get(hall_entity)
                    .is_ok_and(|inventory| {
                        now >= routine.store_retry_after
                            && inventory.amount(Good::Wood) > 0
                            && affordable
                    })
                    .then_some((hall_entity, hall_entrance));
                if let Some((source, entrance)) = source {
                    if !owns_material_priority {
                        if ambient_routine.is_some() {
                            continue;
                        }
                        if ground_distance(position.0, hall_entrance) > WORK_REACH {
                            ensure_move_target(&mut commands, builder, move_target, hall_entrance);
                        }
                        continue;
                    }
                    commands.entity(builder).remove::<ambient::AmbientRoutine>();
                    ensure_move_target(&mut commands, builder, move_target, entrance);
                    routine.phase =
                        ConstructionMaterialPhase::CollectingFromStore { source, entrance };
                    continue;
                }

                if now < routine.tree_retry_after || tree_proof_used {
                    continue;
                }
                commands.entity(builder).remove::<ambient::AmbientRoutine>();
                tree_proof_used = true;

                let salt = stable_name_hash(&name.0) ^ routine.site.to_bits() as u32;
                let Some((tree, stand)) = find_tree_for_cycle(
                    &terrain,
                    derived.as_deref(),
                    obstacles.as_deref(),
                    site_position.0,
                    routine.cycle,
                    salt,
                ) else {
                    // No locally valid stand for this deterministic candidate.
                    // Apply the same real-time backoff as a failed global route,
                    // otherwise a sparse grove retries at the server tick rate.
                    postpone_construction_tree_search(&mut routine, now);
                    ensure_move_target(&mut commands, builder, move_target, hall_entrance);
                    continue;
                };
                // Reject the permanent failure before it enters the ordinary
                // navigation queue. The terrain-only proof deliberately omits
                // transient props/buildings; it answers the cheap structural
                // question of whether hall and tree share walkable land.
                if !crate::world::village_roads::embodied_land_route_exists(
                    &terrain,
                    hall_entrance,
                    stand,
                ) {
                    let widened = postpone_construction_tree_search(&mut routine, now);
                    if widened {
                        warn!(
                            "Village supplier {} found no reachable timber after twelve candidates; waiting for market stock or terrain change",
                            name.0
                        );
                    }
                    ensure_move_target(&mut commands, builder, move_target, hall_entrance);
                    continue;
                }
                routine.tree_retry_after = 0.0;
                ensure_move_target(&mut commands, builder, move_target, stand);
                routine.phase = ConstructionMaterialPhase::WalkingToTree { tree, stand };
            }
            ConstructionMaterialPhase::UnloadingAtHall { hall, entrance } => {
                *activity = CharacterActivity::Idle;
                if ground_distance(position.0, entrance) > WORK_REACH {
                    ensure_move_target(&mut commands, builder, move_target, entrance);
                    continue;
                }
                commands.entity(builder).remove::<MoveTarget>();
                if let Ok([mut carrier, mut hall_store]) = inventories.get_many_mut([builder, hall])
                {
                    for good in Good::ALL {
                        if good != Good::Wood {
                            if let Ok(mut market) = markets.get_mut(hall) {
                                sell_carried_to_moot(
                                    &mut commands,
                                    hall,
                                    &name.0,
                                    wallet.as_deref_mut(),
                                    None,
                                    good,
                                    &mut carrier,
                                    &mut hall_store,
                                    &mut market,
                                );
                                // A buying pool with no cash must not trap a
                                // temporary builder forever. Unsold goods are
                                // consigned to common storage without creating
                                // coin; they remain physical and marketable.
                                carrier.transfer_to(&mut hall_store, good, u32::MAX);
                            } else {
                                carrier.transfer_to(&mut hall_store, good, u32::MAX);
                            }
                        }
                    }
                }
                routine.phase = ConstructionMaterialPhase::Seeking;
            }
            ConstructionMaterialPhase::CollectingFromStore { source, entrance } => {
                *activity = CharacterActivity::Idle;
                if ground_distance(position.0, entrance) > WORK_REACH {
                    ensure_move_target(&mut commands, builder, move_target, entrance);
                    continue;
                }
                commands.entity(builder).remove::<MoveTarget>();
                let remaining = required.saturating_sub(delivered);
                let moved = if let Ok([mut source_store, mut carrier]) =
                    inventories.get_many_mut([source, builder])
                {
                    let carry_room = carrier.free_bulk() / Good::Wood.bulk_per_unit();
                    let requested = remaining.min(carry_room);
                    if site.owner.is_none() {
                        source_store.transfer_to(&mut carrier, Good::Wood, requested)
                    } else {
                        match (markets.get_mut(source), wallet.as_deref_mut()) {
                            (Ok(mut market), Some(wallet)) => buy_from_moot(
                                Good::Wood,
                                requested,
                                wallet,
                                &mut source_store,
                                &mut carrier,
                                &mut market,
                            ),
                            _ => source_store.transfer_to(&mut carrier, Good::Wood, requested),
                        }
                    }
                } else {
                    0
                };
                if moved > 0 {
                    routine.failed_store_routes = 0;
                    routine.store_retry_after = 0.0;
                    commands.entity(builder).insert(MoveTarget(site.stand));
                    routine.phase = ConstructionMaterialPhase::Delivering {
                        destination: site.stand,
                    };
                } else {
                    routine.phase = ConstructionMaterialPhase::Seeking;
                }
            }
            ConstructionMaterialPhase::WalkingToTree { tree, stand } => {
                *activity = CharacterActivity::Idle;
                if ground_distance(position.0, stand) <= WORK_REACH {
                    routine.failed_tree_routes = 0;
                    routine.tree_retry_after = 0.0;
                    commands.entity(builder).remove::<MoveTarget>();
                    let to_tree = tree - position.0;
                    if to_tree.length_squared() > 1e-4 {
                        facing.0 = f32::atan2(-to_tree.x, -to_tree.z);
                    }
                    *activity = CharacterActivity::Chopping;
                    routine.phase = ConstructionMaterialPhase::Chopping {
                        tree,
                        seconds_left: CHOP_SECONDS,
                    };
                } else {
                    ensure_move_target(&mut commands, builder, move_target, stand);
                }
            }
            ConstructionMaterialPhase::Chopping { tree, seconds_left } => {
                *activity = CharacterActivity::Chopping;
                let left = seconds_left - dt;
                if left > 0.0 {
                    routine.phase = ConstructionMaterialPhase::Chopping {
                        tree,
                        seconds_left: left,
                    };
                    continue;
                }
                let remaining = required.saturating_sub(delivered);
                if let Ok(mut carrier) = inventories.get_mut(builder) {
                    carrier.add(Good::Wood, remaining.min(3));
                }
                routine.cycle = routine.cycle.wrapping_add(1);
                *activity = CharacterActivity::Idle;
                commands.entity(builder).insert(MoveTarget(site.stand));
                routine.phase = ConstructionMaterialPhase::Delivering {
                    destination: site.stand,
                };
            }
            ConstructionMaterialPhase::Delivering { destination } => {
                *activity = CharacterActivity::Idle;
                if ground_distance(position.0, destination) > WORK_REACH {
                    ensure_move_target(&mut commands, builder, move_target, destination);
                    continue;
                }
                routine.failed_delivery_routes = 0;
                commands.entity(builder).remove::<MoveTarget>();
                let moved = if let Ok([mut carrier, mut site_store]) =
                    inventories.get_many_mut([builder, routine.site])
                {
                    carrier.transfer_to(&mut site_store, Good::Wood, required - delivered)
                } else {
                    0
                };
                let now_delivered = delivered.saturating_add(moved);
                info!(
                    "Village '{}': {} delivered wood to the {} ({now_delivered}/{required})",
                    settlement.name,
                    name.0,
                    site.kind.label(),
                );
                if now_delivered >= required {
                    commands
                        .entity(builder)
                        .remove::<ConstructionMaterialRoutine>()
                        .insert(MoveTarget(site.stand));
                } else {
                    routine.phase = ConstructionMaterialPhase::Seeking;
                }
            }
        }
    }
}

/// Drive every permitted building from grant to standing.
///
/// Three things happen in order, and the order is the point: the builder walks
/// out, the plot is cleared and levelled, and only then does the frame go up.
/// A building that simply materialised on a timer told you nothing about who
/// built it or what it cost.
#[allow(clippy::too_many_arguments)]
pub fn advance_construction(
    time: Res<Time>,
    warp: Query<&shared::components::TimeWarp>,
    world_time: Query<&WorldTime>,
    mut commands: Commands,
    mut terrain: Option<ResMut<WorldTerrain>>,
    mut deltas: ResMut<PublishedTerrainDeltas>,
    settlements: Query<&Settlement>,
    positions: Query<&PlayerPosition>,
    move_targets: Query<&MoveTarget>,
    home_routines: Query<(), With<HomeRoutine>>,
    mut intents: Query<&mut VillagerIntent>,
    mut pending: Query<(Entity, &mut UnderConstruction, &GoodsInventory)>,
    mut sites: Query<&mut shared::components::ConstructionSite>,
    mut facings: Query<&mut PlayerRotation>,
) {
    let warp = time_warp_factor(&warp);
    let daylight = world_time.iter().next().is_none_or(WorldTime::is_day);
    for (site, mut under, materials) in pending.iter_mut() {
        let Ok(settlement) = settlements.get(under.settlement) else {
            // Its settlement vanished; drop the site rather than leaving a
            // building belonging to nowhere.
            release_builder(&mut commands, &mut intents, under.builder, None);
            commands.entity(site).despawn();
            continue;
        };

        // People go home at night. An approved site remains exactly where it
        // was, but no ground is cleared and no raising timer advances without
        // daylight and its builder's time.
        if !daylight {
            continue;
        }
        if under
            .builder
            .is_some_and(|builder| home_routines.get(builder).is_ok())
        {
            continue;
        }

        match under.stage {
            BuildStage::Supplying => {
                let Some(builder) = under.builder else {
                    commands.entity(site).despawn();
                    continue;
                };
                if positions.get(builder).is_err() {
                    commands.entity(site).despawn();
                    continue;
                }
                let required = under.kind.construction_wood_required();
                if materials.amount(Good::Wood) < required {
                    continue;
                }
                under.stage = BuildStage::Walking;
                commands
                    .entity(builder)
                    .remove::<ConstructionMaterialRoutine>()
                    .insert(MoveTarget(under.stand));
                info!(
                    "Village '{}': {} fully supplied ({required} wood)",
                    settlement.name,
                    under.kind.label(),
                );
            }
            BuildStage::Walking => {
                // No builder left (they were despawned): the permit lapses
                // rather than the building appearing by itself.
                let Some(builder) = under.builder else {
                    commands.entity(site).despawn();
                    continue;
                };
                let Ok(at) = positions.get(builder) else {
                    commands.entity(site).despawn();
                    continue;
                };
                if at.0.distance(under.stand) > BUILD_REACH {
                    if move_targets.get(builder).is_err() {
                        commands.entity(builder).insert(MoveTarget(under.stand));
                    }
                    continue;
                }

                // On site. Clear the plot before anything is raised on it.
                if let Some(terrain) = terrain.as_mut() {
                    clear_and_level(terrain, &mut deltas, &mut commands, &under);
                }
                // Carrying these two is what makes the world treat the plot as
                // built-on: the client stops scattering props inside it, the
                // server drops the tree colliders and marks it as an obstacle.
                // Attached NOW, at clearing, not at completion -- that is the
                // difference between a site being cleared and a building
                // appearing on top of standing trees.
                commands.entity(site).insert((
                    shared::building::PlacedBuilding {
                        building_type: under.kind.art(),
                        rotation: under.rotation,
                    },
                    shared::building::BuildingPosition(under.position),
                ));
                under.stage = BuildStage::Raising {
                    seconds_left: BUILD_SECONDS,
                };
                // Construction owns the builder now. Leaving the walk target
                // attached lets `step_units` run later in the fixed schedule
                // and overwrite the inward-facing rotation with the final
                // approach direction -- exactly the "back to the house" bug.
                commands.entity(builder).remove::<MoveTarget>();
                // One flip, one replication. The client runs its own clock from
                // here so the frame can rise out of the ground without the
                // server streaming a progress float at tick rate.
                if let Ok(mut site_view) = sites.get_mut(site) {
                    site_view.raising = true;
                }
                // Turn them to face the work. They arrive facing whichever way
                // they were walking, which is away from the plot as often as
                // not, and a builder hammering with their back to the house
                // reads as broken.
                //
                // The shipped character faces local -Z. Face that rendered
                // front toward the plot, then keep movement from overwriting it
                // by removing the completed walk target above.
                if let Some(builder) = under.builder {
                    if let Ok(mut facing) = facings.get_mut(builder) {
                        let to_work = under.position - under.stand;
                        if to_work.length_squared() > 1e-4 {
                            facing.0 = build_clip_facing(to_work);
                        }
                    }
                }
                info!(
                    "Village '{}': ground cleared for a {}",
                    settlement.name,
                    under.kind.label()
                );
            }
            BuildStage::Raising { seconds_left } => {
                let left = seconds_left - time.delta_secs() * warp;
                if left > 0.0 {
                    under.stage = BuildStage::Raising { seconds_left: left };
                    continue;
                }
                let building_entity = commands
                    .spawn((
                        SettlementBuilding {
                            kind: under.kind,
                            settlement: settlement.name.clone(),
                            owner: under.owner.clone(),
                            quality: under.quality,
                            workers: Vec::new(),
                        },
                        shared::economy::GoodsInventory::new(under.kind.storage_bulk_capacity()),
                        PlayerPosition(under.position),
                        PlayerRotation(under.rotation),
                        // The finished building takes over the plot claim from the
                        // site, so the ground stays clear once the site despawns.
                        shared::building::PlacedBuilding {
                            building_type: under.kind.art(),
                            rotation: under.rotation,
                        },
                        shared::building::BuildingPosition(under.position),
                        // No RegionCoord: buildings are part of the map screen, like
                        // the settlements they belong to.
                        Replicate::to_clients(NetworkTarget::All),
                    ))
                    .id();
                info!(
                    "Village '{}': {} completed ({:.0}% ground)",
                    settlement.name,
                    under.kind.label(),
                    under.quality * 100.0
                );
                if let Some(builder) = under.builder {
                    // The person who raised the building owns the last piece of
                    // work too: joining its authored door to the village path
                    // network. `plan_requested_roads` adopts them next in the
                    // chained schedule and releases them if no route is viable.
                    commands.entity(building_entity).insert(RoadRequest {
                        builder,
                        settlement: under.settlement,
                        completed_site: site,
                        attempt: 0,
                    });
                    commands
                        .entity(builder)
                        .remove::<MoveTarget>()
                        .remove::<ConstructionMaterialRoutine>();
                } else {
                    release_builder(&mut commands, &mut intents, None, Some(under.settlement));
                }
                commands.entity(site).despawn();
            }
        }
    }
}

/// Put a builder back to ordinary residency.
fn release_builder(
    commands: &mut Commands,
    intents: &mut Query<&mut VillagerIntent>,
    builder: Option<Entity>,
    settlement: Option<Entity>,
) {
    let Some(builder) = builder else { return };
    commands
        .entity(builder)
        .remove::<MoveTarget>()
        .remove::<ConstructionMaterialRoutine>();
    if let Ok(mut intent) = intents.get_mut(builder) {
        *intent = match settlement {
            Some(settlement) => VillagerIntent::Resident { settlement },
            None => VillagerIntent::Idle,
        };
    }
}

/// Level the plot and publish the change so clients see the same ground.
///
/// The flatten itself already existed -- it is what the editor's terrain brush
/// uses -- and so did the client's ingest of replicated deltas. What did not
/// exist was anything on the SERVER writing one, so this is the missing half of
/// a road that was already built from both ends.
fn clear_and_level(
    terrain: &mut WorldTerrain,
    deltas: &mut PublishedTerrainDeltas,
    commands: &mut Commands,
    under: &UnderConstruction,
) {
    let def = under.kind.art().definition();
    // Level TO the plot's own height, so a building on a slope cuts a terrace
    // rather than the whole village drifting to one altitude.
    let ground = terrain.get_height(under.position.x, under.position.z);
    let centre = Vec3::new(under.position.x, ground, under.position.z);
    let affected = terrain.apply_flatten_rect(
        centre,
        def.footprint * 0.5,
        under.rotation,
        def.flatten_radius,
    );

    for coord in affected {
        let Some(data) = terrain.get_delta_chunk(coord) else {
            continue;
        };
        let chunk = shared::terrain::TerrainDeltaChunk::from_delta_data(coord, data);
        match deltas.by_chunk.get(&coord) {
            // Update in place: a second building in the same chunk must not
            // spawn a second authority for that chunk's heights.
            Some(entity) => {
                commands.entity(*entity).insert(chunk);
            }
            None => {
                let entity = commands
                    .spawn((chunk, Replicate::to_clients(NetworkTarget::All)))
                    .id();
                deltas.by_chunk.insert(coord, entity);
            }
        }
    }
}

/// Residents take vacant positions in their own settlement.
///
/// This is the smallest honest version of WORLD-DESIGN section 1a's rule that
/// production is people in jobs rather than population times a multiplier. A
/// position is held by a NAMED person, so when that person is gone the position
/// is vacant again and its physical work routine stops.
///
/// Founding workplaces have no skill minimum. If a future specialist building
/// carries [`WorkforceRequirements`], only matching residents can fill it.
/// Among jobs a resident can do, the highest daily wage recruits first and
/// distance breaks the choice of which resident takes that offer.
pub fn fill_vacancies(
    mut commands: Commands,
    mut buildings: Query<(
        Entity,
        &mut SettlementBuilding,
        &PlayerPosition,
        Option<&BusinessWagePolicy>,
        Option<&WorkforceRequirements>,
    )>,
    mut villagers: Query<(
        Entity,
        &CharacterName,
        &VillagerIntent,
        &PlayerPosition,
        &mut Occupation,
        Option<&WorkStatus>,
        Option<&CharacterAttributes>,
    )>,
    settlements: Query<(Entity, &Settlement)>,
    administrations: Query<&MootAdministration>,
    active_builders: Query<(), Or<(With<ConstructionMaterialRoutine>, With<RoadBuilderRoutine>)>>,
) {
    // Civic posts are jobs, not honorary labels. Give those explicit rosters
    // priority when repairing an old save that contains the same person in a
    // business roster as well.
    let mut employed: HashSet<String> = HashSet::new();
    for administration in administrations.iter() {
        if let Some(reeve) = &administration.reeve {
            employed.insert(reeve.clone());
        }
        if let Some(porter) = &administration.market_porter {
            employed.insert(porter.clone());
        }
        if let Some(steward) = &administration.road_steward {
            employed.insert(steward.clone());
        }
        employed.extend(administration.city_workers.iter().cloned());
        employed.extend(administration.guards.iter().cloned());
    }

    // Repair legacy or malformed double assignments as well as preventing new
    // ones. The first business roster keeps an ordinary worker; a civic worker
    // is removed from every business because their public post already owns
    // their working day.
    for (_, mut building, _, _, _) in buildings.iter_mut() {
        let settlement_name = building.settlement.clone();
        let trade = building.kind.trade().unwrap_or("workplace");
        building.workers.retain(|worker| {
            let keeps_job = employed.insert(worker.clone());
            if !keeps_job {
                warn!(
                    "Village '{}': removed {worker} from the {} roster because they already hold another job",
                    settlement_name, trade
                );
            }
            keeps_job
        });
    }
    if !villagers
        .iter()
        .any(|(entity, name, intent, _, occupation, status, _)| {
            intent.is_settled()
                && occupation.0.is_none()
                && !employed.contains(name.0.as_str())
                && active_builders.get(entity).is_err()
                && status.is_none_or(|status| *status == WorkStatus::LookingForWork)
        })
    {
        return;
    }

    for (settlement_entity, settlement) in settlements.iter() {
        loop {
            // Best offer in this settlement. Skill requirements remain absent
            // on every founding business, but are part of the vacancy now so
            // later specialist buildings do not need a parallel job system.
            let mut vacancies: Vec<_> = buildings
                .iter()
                .filter(|(_, building, _, _, _)| {
                    building.settlement == settlement.name
                        && (building.workers.len() as u8) < building.kind.positions()
                })
                .map(|(entity, building, at, wage, requirements)| {
                    (
                        entity,
                        building.kind,
                        at.0,
                        wage.map_or(FOUNDING_DAILY_WAGE, |policy| policy.daily_wage),
                        requirements.copied(),
                    )
                })
                .collect();
            vacancies.sort_by(|a, b| {
                b.3.cmp(&a.3)
                    .then_with(|| a.2.x.total_cmp(&b.2.x))
                    .then_with(|| a.2.z.total_cmp(&b.2.z))
            });

            // Usually the best-paid offer finds somebody in one people scan.
            // Only an unfillable specialist offer falls through to the next;
            // ordinary jobs never scan the whole population once per building.
            let mut placement = None;
            for (vacancy_entity, kind, plot, offered_wage, requirements) in vacancies {
                let taker = villagers
                    .iter()
                    .filter(
                        |(entity, name, intent, _, occupation, status, attributes)| {
                            intent.is_settled()
                                && intent.settlement() == Some(settlement_entity)
                                && occupation.0.is_none()
                                && !employed.contains(&name.0)
                                && active_builders.get(*entity).is_err()
                                && status.is_none_or(|status| *status == WorkStatus::LookingForWork)
                                && requirements.is_none_or(|requirements| {
                                    attributes.is_some_and(|attributes| {
                                        requirements.is_met_by(*attributes)
                                    })
                                })
                        },
                    )
                    .min_by(|a, b| {
                        a.3 .0
                            .distance_squared(plot)
                            .total_cmp(&b.3 .0.distance_squared(plot))
                    })
                    .map(|(entity, name, _, _, _, _, _)| (entity, name.0.clone()));
                if let Some((taker_entity, taker)) = taker {
                    placement = Some((vacancy_entity, kind, offered_wage, taker_entity, taker));
                    break;
                }
            }
            let Some((vacancy_entity, kind, offered_wage, taker_entity, taker)) = placement else {
                break;
            };

            let Ok((_, mut building, _, _, _)) = buildings.get_mut(vacancy_entity) else {
                break;
            };
            if (building.workers.len() as u8) >= building.kind.positions() {
                break;
            }
            building.workers.push(taker.clone());
            employed.insert(taker.clone());
            for (_, name, _, _, mut occupation, _, _) in villagers.iter_mut() {
                if name.0 == taker {
                    let title = kind.trade().unwrap_or("Villager").to_string();
                    if occupation.0.as_deref() != Some(title.as_str()) {
                        occupation.0 = Some(title);
                    }
                }
            }
            commands.entity(taker_entity).insert(WorkStatus::Employed);
            info!(
                "Village '{}': {taker} took work as a {} for {} coin/day",
                settlement.name,
                kind.trade().unwrap_or("hand"),
                shared::economy::format_money(offered_wage),
            );
        }
    }
}

/// Give every completed cabin a bounded, inspectable household roster.
pub fn ensure_households(
    mut commands: Commands,
    houses: Query<(
        Entity,
        &SettlementBuilding,
        Option<&Household>,
        Option<&HouseholdEconomy>,
    )>,
) {
    for (entity, building, household, economy) in houses.iter() {
        if building.kind == SettlementBuildingKind::House {
            let mut entity_commands = commands.entity(entity);
            if household.is_none() {
                entity_commands.insert(Household::default());
            }
            if economy.is_none() {
                entity_commands.insert(HouseholdEconomy::default());
            }
        }
    }
}

/// Fund each house's shared necessities purse and restock its bounded pantry.
///
/// This is intentionally one decision per household per world day. Tactical
/// households send the named shopper on a visible trip; strategic settlements
/// settle the same purchase directly so 5,000 people do not need 5,000 paths.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn update_household_budgets_and_pantries(
    mut commands: Commands,
    world_time: Query<&WorldTime>,
    regions: Option<Res<RegionRegistry>>,
    mut halls: Query<
        (
            Entity,
            &Settlement,
            &PlayerPosition,
            Option<&PlayerRotation>,
            &mut GoodsInventory,
            &mut MootMarket,
        ),
        (Without<SettlementBuilding>, Without<CharacterKind>),
    >,
    mut houses: Query<
        (
            Entity,
            &SettlementBuilding,
            &Household,
            &mut HouseholdEconomy,
            &mut GoodsInventory,
        ),
        Without<CharacterKind>,
    >,
    mut residents: Query<(
        Entity,
        &CharacterName,
        &mut Wallet,
        Option<&WorkStatus>,
        Option<&HomeAssignment>,
        Option<&RegionCoord>,
        Option<&HouseholdShoppingRoutine>,
    )>,
    busy: Query<
        (),
        Or<(
            With<ConstructionMaterialRoutine>,
            With<RoadBuilderRoutine>,
            With<HomeRoutine>,
            With<WorkplaceDoorTransit>,
            With<MarketCollectionRoutine>,
        )>,
    >,
) {
    let Some(clock) = world_time.iter().next() else {
        return;
    };
    let day = clock.day;
    let hall_by_name: HashMap<String, Entity> = halls
        .iter()
        .map(|(entity, settlement, _, _, _, _)| (settlement.name.clone(), entity))
        .collect();
    let resident_by_name: HashMap<String, Entity> = residents
        .iter()
        .map(|(entity, name, _, _, _, _, _)| (name.0.clone(), entity))
        .collect();
    for (house_entity, building, household, mut economy, mut pantry) in houses.iter_mut() {
        if building.kind != SettlementBuildingKind::House || economy.last_budget_day == day {
            continue;
        }
        let mut members: Vec<(String, WorkStatus)> = household
            .residents
            .iter()
            .filter_map(|member| {
                let entity = resident_by_name.get(member).copied()?;
                let Ok((_, name, _, status, assignment, _, _)) = residents.get(entity) else {
                    return None;
                };
                assignment
                    .is_some_and(|assignment| assignment.home == house_entity)
                    .then_some((name.0.clone(), status.copied().unwrap_or_default()))
            })
            .collect();
        members.sort_by(|a, b| {
            let rank = |status: WorkStatus| match status {
                WorkStatus::Chilling => 0,
                WorkStatus::LookingForWork => 1,
                WorkStatus::Employed => 2,
            };
            rank(a.1).cmp(&rank(b.1)).then_with(|| a.0.cmp(&b.0))
        });
        economy.shopper = members.first().map(|(name, _)| name.clone());
        if members.is_empty() {
            economy.last_budget_day = day;
            continue;
        }
        let target_units =
            (members.len() as u32).saturating_mul(u32::from(economy.pantry_target_days));
        let deficit = target_units.saturating_sub(pantry.edible_amount());
        if deficit == 0 {
            economy.last_budget_day = day;
            continue;
        }
        let Some(hall_entity) = hall_by_name.get(&building.settlement).copied() else {
            continue;
        };
        let Ok((_, _, hall_position, hall_rotation, mut hall_store, mut market)) =
            halls.get_mut(hall_entity)
        else {
            continue;
        };
        let estimated_unit_price = [Good::Food, Good::Wheat]
            .into_iter()
            .filter(|good| hall_store.amount(*good) > 0)
            .map(|good| market.pool(good).ask)
            .min()
            .unwrap_or(0);
        let wanted_budget = u64::from(deficit).saturating_mul(estimated_unit_price);
        let mut needed = wanted_budget.saturating_sub(economy.pennies);
        if needed > 0 {
            // All earners contribute toward the same concrete pantry target,
            // while retaining two coins as personal discretionary money.
            let mut contribution_order = members.clone();
            contribution_order.sort_by(|a, b| a.0.cmp(&b.0));
            for (member, _) in contribution_order {
                let Some(member_entity) = resident_by_name.get(&member).copied() else {
                    continue;
                };
                let Ok((_, _, mut wallet, _, _, _, _)) = residents.get_mut(member_entity) else {
                    continue;
                };
                let available = wallet.balance().saturating_sub(2 * PENNIES_PER_COIN);
                let contribution = available.min(needed);
                if contribution > 0 && wallet.debit(contribution) {
                    economy.pennies = economy.pennies.saturating_add(contribution);
                    needed -= contribution;
                }
                if needed == 0 {
                    break;
                }
            }
        }

        let shopper_entity = economy
            .shopper
            .as_ref()
            .and_then(|shopper| resident_by_name.get(shopper).copied());
        let tactical_shopper = shopper_entity.is_some_and(|shopper| {
            let Ok((_, _, _, _, _, region, routine)) = residents.get(shopper) else {
                return false;
            };
            routine.is_none()
                && busy.get(shopper).is_err()
                && region.is_some_and(|region| {
                    regions.as_ref().is_some_and(|registry| {
                        registry
                            .get(*region)
                            .is_some_and(|state| state.sim_level == SimLevel::Tactical)
                    })
                })
        });
        if tactical_shopper && clock.is_day() {
            if let Some(shopper) = shopper_entity {
                let entrance = SettlementBuildingKind::Hall.entrance_position(
                    hall_position.0,
                    hall_rotation.map_or(0.0, |rotation| rotation.0),
                );
                commands.entity(shopper).insert((
                    HouseholdShoppingRoutine {
                        home: house_entity,
                        hall: hall_entity,
                        phase: HouseholdShoppingPhase::GoingToMarket,
                    },
                    MoveTarget(entrance),
                ));
                economy.last_budget_day = day;
                continue;
            }
        }

        let mut remaining = deficit;
        for good in [Good::Food, Good::Wheat] {
            if remaining == 0 {
                break;
            }
            let room = pantry.free_bulk() / good.bulk_per_unit();
            let requested = remaining.min(room).min(hall_store.amount(good));
            let trade =
                market.sell_to_consumer(good, hall_store.amount(good), requested, economy.pennies);
            if trade.units == 0 {
                continue;
            }
            if economy.pennies < trade.pennies {
                continue;
            }
            economy.pennies -= trade.pennies;
            let moved = hall_store.transfer_to(&mut pantry, good, trade.units);
            debug_assert_eq!(moved, trade.units);
            remaining = remaining.saturating_sub(moved);
        }
        economy.last_budget_day = day;
    }
}

/// Animate the tactical half of household restocking. The shopper pays from
/// the house purse at the Moot, carries only what fits, and unloads at their
/// own cabin; off-screen households use the aggregate branch above.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn run_household_shopping(
    mut commands: Commands,
    mut halls: Query<
        (
            &PlayerPosition,
            Option<&PlayerRotation>,
            &mut GoodsInventory,
            &mut MootMarket,
        ),
        (
            With<Settlement>,
            Without<SettlementBuilding>,
            Without<CharacterKind>,
        ),
    >,
    mut houses: Query<
        (
            &SettlementBuilding,
            &PlayerPosition,
            &PlayerRotation,
            &Household,
            &mut HouseholdEconomy,
            &mut GoodsInventory,
        ),
        Without<CharacterKind>,
    >,
    mut shoppers: Query<
        (
            Entity,
            &PlayerPosition,
            &mut CharacterActivity,
            &mut GoodsInventory,
            &mut HouseholdShoppingRoutine,
            Option<&MoveTarget>,
            Option<&HomeRoutine>,
        ),
        With<CharacterKind>,
    >,
) {
    for (shopper, position, mut activity, mut carrier, mut routine, move_target, home) in
        shoppers.iter_mut()
    {
        if home.is_some() {
            continue;
        }
        let Ok((hall_position, hall_rotation, mut hall_store, mut market)) =
            halls.get_mut(routine.hall)
        else {
            commands
                .entity(shopper)
                .remove::<HouseholdShoppingRoutine>()
                .remove::<MoveTarget>();
            continue;
        };
        let Ok((building, home_position, home_rotation, household, mut economy, mut pantry)) =
            houses.get_mut(routine.home)
        else {
            commands
                .entity(shopper)
                .remove::<HouseholdShoppingRoutine>()
                .remove::<MoveTarget>();
            continue;
        };
        let hall_entrance = SettlementBuildingKind::Hall.entrance_position(
            hall_position.0,
            hall_rotation.map_or(0.0, |rotation| rotation.0),
        );
        let home_entrance = building
            .kind
            .entrance_position(home_position.0, home_rotation.0);
        *activity = CharacterActivity::Idle;
        match routine.phase {
            HouseholdShoppingPhase::GoingToMarket => {
                if ground_distance(position.0, hall_entrance) > WORK_REACH {
                    ensure_move_target(&mut commands, shopper, move_target, hall_entrance);
                    continue;
                }
                let target = (household.residents.len() as u32)
                    .saturating_mul(u32::from(economy.pantry_target_days));
                let mut remaining = target.saturating_sub(pantry.edible_amount());
                for good in [Good::Food, Good::Wheat] {
                    if remaining == 0 {
                        break;
                    }
                    let requested = remaining
                        .min(carrier.free_bulk() / good.bulk_per_unit())
                        .min(hall_store.amount(good));
                    let trade = market.sell_to_consumer(
                        good,
                        hall_store.amount(good),
                        requested,
                        economy.pennies,
                    );
                    if trade.units == 0 || economy.pennies < trade.pennies {
                        continue;
                    }
                    economy.pennies -= trade.pennies;
                    let moved = hall_store.transfer_to(&mut carrier, good, trade.units);
                    debug_assert_eq!(moved, trade.units);
                    remaining = remaining.saturating_sub(moved);
                }
                if carrier.edible_amount() == 0 {
                    commands
                        .entity(shopper)
                        .remove::<HouseholdShoppingRoutine>()
                        .remove::<MoveTarget>();
                    continue;
                }
                commands.entity(shopper).insert(MoveTarget(home_entrance));
                routine.phase = HouseholdShoppingPhase::ReturningHome;
            }
            HouseholdShoppingPhase::ReturningHome => {
                if ground_distance(position.0, home_entrance) > DOOR_REACH {
                    ensure_move_target(&mut commands, shopper, move_target, home_entrance);
                    continue;
                }
                for good in [Good::Food, Good::Wheat] {
                    carrier.transfer_to(&mut pantry, good, u32::MAX);
                }
                commands
                    .entity(shopper)
                    .remove::<HouseholdShoppingRoutine>()
                    .remove::<MoveTarget>();
            }
        }
    }
}

/// Assign every resident to one cabin in their settlement, up to its real bed
/// capacity. Nearest-first keeps a household spatially coherent without yet
/// inventing family relationships.
pub fn assign_households(
    mut commands: Commands,
    settlements: Query<(Entity, &Settlement)>,
    mut houses: Query<(Entity, &SettlementBuilding, &PlayerPosition, &mut Household)>,
    villagers: Query<(
        Entity,
        &CharacterName,
        &VillagerIntent,
        &PlayerPosition,
        Option<&HomeAssignment>,
    )>,
    changed_assignments: Query<(), Changed<HomeAssignment>>,
    changed_names: Query<(), Changed<CharacterName>>,
) {
    let house_info: Vec<(Entity, String, Vec3, usize)> = houses
        .iter()
        .filter(|(_, building, _, _)| building.kind == SettlementBuildingKind::House)
        .map(|(entity, building, position, _)| {
            (
                entity,
                building.settlement.clone(),
                position.0,
                building.kind.housing_capacity() as usize,
            )
        })
        .collect();
    let house_lookup: HashMap<Entity, usize> = house_info
        .iter()
        .enumerate()
        .map(|(index, (entity, _, _, _))| (*entity, index))
        .collect();
    let settlement_names: HashMap<Entity, String> = settlements
        .iter()
        .map(|(entity, settlement)| (entity, settlement.name.clone()))
        .collect();

    // The common case is a fully assigned, unchanged village. Validate it in
    // O(people + houses) and leave every replicated roster untouched. The old
    // implementation rebuilt names and searched every house for every person
    // on all 60 server ticks, even though housing changes only when a person or
    // cabin changes.
    let mut occupancy: HashMap<Entity, usize> = HashMap::new();
    let mut needs_rebuild = !changed_assignments.is_empty() || !changed_names.is_empty();
    for (_, _, intent, _, assignment) in villagers.iter() {
        if !intent.counts_as_resident() {
            // Travelling villagers used to consume beds before they arrived,
            // while recount_residents correctly excluded them. Remove any
            // legacy assignment and rebuild its old cabin roster.
            if assignment.is_some() {
                needs_rebuild = true;
            }
            continue;
        }
        let Some(assignment) = assignment else {
            needs_rebuild = true;
            continue;
        };
        let Some(index) = house_lookup.get(&assignment.home).copied() else {
            needs_rebuild = true;
            continue;
        };
        let (_, place, _, capacity) = &house_info[index];
        let same_settlement = intent
            .settlement()
            .and_then(|settlement| settlement_names.get(&settlement))
            == Some(place);
        let occupied = occupancy.entry(assignment.home).or_default();
        if !same_settlement || *occupied >= *capacity {
            needs_rebuild = true;
            continue;
        }
        *occupied += 1;
    }
    if !needs_rebuild {
        needs_rebuild = houses.iter().any(|(house, _, _, household)| {
            occupancy.get(&house).copied().unwrap_or(0) != household.residents.len()
        });
    }
    if !needs_rebuild {
        return;
    }

    let mut rosters: HashMap<Entity, Vec<String>> = HashMap::new();
    let mut assigned: HashSet<Entity> = HashSet::new();
    for (villager, name, intent, _, assignment) in villagers.iter() {
        let Some(assignment) = assignment else {
            continue;
        };
        let valid = intent.counts_as_resident()
            && house_lookup
                .get(&assignment.home)
                .and_then(|index| house_info.get(*index))
                .is_some_and(|(house, place, _, capacity)| {
                    intent
                        .settlement()
                        .and_then(|settlement| settlement_names.get(&settlement))
                        == Some(place)
                        && rosters.get(house).map_or(0, Vec::len) < *capacity
                });
        if valid {
            assigned.insert(villager);
            rosters
                .entry(assignment.home)
                .or_default()
                .push(name.0.clone());
        } else {
            commands.entity(villager).remove::<HomeAssignment>();
        }
    }

    for (house, place, house_position, capacity) in &house_info {
        while rosters.get(house).map_or(0, Vec::len) < *capacity {
            let resident = villagers
                .iter()
                .filter(|(entity, _, intent, _, _)| {
                    !assigned.contains(entity)
                        && intent.counts_as_resident()
                        && intent
                            .settlement()
                            .and_then(|settlement| settlement_names.get(&settlement))
                            == Some(place)
                })
                .min_by(|a, b| {
                    a.3 .0
                        .distance_squared(*house_position)
                        .total_cmp(&b.3 .0.distance_squared(*house_position))
                })
                .map(|(entity, name, _, _, _)| (entity, name.0.clone()));
            let Some((resident, name)) = resident else {
                break;
            };
            assigned.insert(resident);
            rosters.entry(*house).or_default().push(name.clone());
            commands
                .entity(resident)
                .insert(HomeAssignment { home: *house });
            info!("Village '{place}': {name} was assigned a bed in a cabin");
        }
    }

    for (house, _, _, mut household) in houses.iter_mut() {
        let next = rosters.remove(&house).unwrap_or_default();
        if household.residents != next {
            household.residents = next;
        }
    }
}

/// Send housed villagers through their cabin door at sunset and back out at
/// sunrise. Work state is reset to its workplace approach, so morning resumes
/// with a real journey instead of farming or chopping beside the bed.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn run_household_schedules(
    time: Res<Time>,
    obstacles: Option<Res<SpatialObstacleGrid>>,
    warp: Query<&shared::components::TimeWarp>,
    world_time: Query<&WorldTime>,
    mut commands: Commands,
    homes: Query<
        (
            Entity,
            &SettlementBuilding,
            &PlayerPosition,
            &PlayerRotation,
            &Household,
        ),
        Without<CharacterKind>,
    >,
    workplaces: Query<
        (&SettlementBuilding, &PlayerPosition, &PlayerRotation),
        Without<CharacterKind>,
    >,
    roads: Query<&shared::components::VillageRoad>,
    mut villagers: Query<
        (
            Entity,
            &CharacterName,
            &PlayerPosition,
            &mut PlayerRotation,
            &mut CharacterActivity,
            &HomeAssignment,
            Option<&MoveTarget>,
            Option<&mut HomeRoutine>,
            Option<&mut FarmerRoutine>,
            Option<&mut LumberjackRoutine>,
            Option<&mut FishingRoutine>,
            Option<&mut RoadBuilderRoutine>,
            Option<&WorkplaceDoorTransit>,
        ),
        With<CharacterKind>,
    >,
) {
    let Some(clock) = world_time.iter().next() else {
        return;
    };
    let is_day = clock.is_day();
    let dt = time.delta_secs() * time_warp_factor(&warp);

    for (
        villager,
        name,
        position,
        mut facing,
        mut activity,
        assignment,
        move_target,
        routine,
        mut farmer,
        mut lumberjack,
        mut fisher,
        mut road_builder,
        workplace_transit,
    ) in villagers.iter_mut()
    {
        let assigned_home = homes.get(assignment.home).ok();

        let Some(mut routine) = routine else {
            if is_day {
                continue;
            }
            let Some((home, _, home_position, home_rotation, _)) = assigned_home else {
                continue;
            };
            let door =
                SettlementBuildingKind::House.entrance_position(home_position.0, home_rotation.0);
            *activity = CharacterActivity::Idle;
            let workplace_threshold = workplace_transit
                .map(|transit| (transit.building, transit.door, transit.inside))
                .or_else(|| {
                    farmer.as_deref().and_then(|farmer| {
                        matches!(farmer.phase, FarmerPhase::Inside { .. })
                            .then(|| workplaces.get(farmer.farmstead).ok())
                            .flatten()
                            .map(|(building, position, rotation)| {
                                (
                                    position.0,
                                    building.kind.entrance_position(position.0, rotation.0),
                                    building.kind.interior_door_position(position.0, rotation.0),
                                )
                            })
                    })
                })
                .or_else(|| {
                    lumberjack.as_deref().and_then(|lumberjack| {
                        matches!(lumberjack.phase, LumberjackPhase::Inside { .. })
                            .then(|| workplaces.get(lumberjack.hut).ok())
                            .flatten()
                            .map(|(building, position, rotation)| {
                                (
                                    position.0,
                                    building.kind.entrance_position(position.0, rotation.0),
                                    building.kind.interior_door_position(position.0, rotation.0),
                                )
                            })
                    })
                })
                .or_else(|| {
                    fisher.as_deref().and_then(|fisher| {
                        matches!(fisher.phase, FishingPhase::Inside { .. })
                            .then(|| workplaces.get(fisher.hut).ok())
                            .flatten()
                            .map(|(building, position, rotation)| {
                                (
                                    position.0,
                                    building.kind.entrance_position(position.0, rotation.0),
                                    building.kind.interior_door_position(position.0, rotation.0),
                                )
                            })
                    })
                });

            // Unfinished work waits for morning. Resetting the spatial phase is
            // essential: otherwise an interrupted Chopping phase resumes at
            // the cabin and the villager swings an axe beside their bed.
            if let Some(farmer) = farmer.as_deref_mut() {
                farmer.phase = FarmerPhase::GoingToFarmstead;
            }
            if let Some(lumberjack) = lumberjack.as_deref_mut() {
                lumberjack.phase = LumberjackPhase::GoingToHut;
            }
            if let Some(fisher) = fisher.as_deref_mut() {
                fisher.phase = FishingPhase::GoingToHut;
                commands
                    .entity(villager)
                    .remove::<PierTraversal>()
                    .remove::<TravelRoute>()
                    .remove::<NavigationRoutePending>();
            }
            if let Some(road_builder) = road_builder.as_deref_mut() {
                if let Ok(road) = roads.get(road_builder.road) {
                    road_builder.restart_from_built_road(road);
                }
            }

            // Night takes ownership of the destination. A failed daytime
            // route must not remain attached: movement deliberately sleeps
            // while NavigationRouteFailed exists, including during a doorway
            // transit that is otherwise allowed to ignore the wall collider.
            commands
                .entity(villager)
                .remove::<TravelRoute>()
                .remove::<NavigationRoutePending>()
                .remove::<NavigationRouteFailed>();

            let phase =
                if let Some((building, workplace_door, workplace_inside)) = workplace_threshold {
                    begin_workplace_exit(
                        &mut commands,
                        villager,
                        building,
                        workplace_door,
                        workplace_inside,
                        door,
                    );
                    HomePhase::LeavingWorkplace
                } else {
                    commands.entity(villager).insert(MoveTarget(door));
                    HomePhase::GoingToDoor
                };
            commands
                .entity(villager)
                .insert(HomeRoutine { home, phase });
            continue;
        };

        let Ok((_, building, home_position, home_rotation, household)) = homes.get(routine.home)
        else {
            *activity = CharacterActivity::Idle;
            commands
                .entity(villager)
                .remove::<HomeRoutine>()
                .remove::<BuildingDoorUse>()
                .remove::<MoveTarget>()
                .remove::<TravelRoute>()
                .remove::<NavigationRoutePending>()
                .remove::<NavigationRouteFailed>();
            continue;
        };
        if assignment.home != routine.home
            || !household
                .residents
                .iter()
                .any(|resident| resident == &name.0)
        {
            *activity = CharacterActivity::Idle;
            commands
                .entity(villager)
                .remove::<HomeRoutine>()
                .remove::<BuildingDoorUse>()
                .remove::<MoveTarget>()
                .remove::<TravelRoute>()
                .remove::<NavigationRoutePending>()
                .remove::<NavigationRouteFailed>();
            continue;
        }

        let door = building
            .kind
            .entrance_position(home_position.0, home_rotation.0);
        let inside = building
            .kind
            .interior_door_position(home_position.0, home_rotation.0);
        let outside = exterior_door_clearance_position(home_position.0, door);
        let door_use = BuildingDoorUse {
            building: home_position.0,
        };

        if is_day {
            match routine.phase {
                HomePhase::LeavingWorkplace => {
                    // Finish crossing the workplace threshold first. Its
                    // transit changes this to GoingToDoor, which releases the
                    // villager back to work on the following daylight tick.
                }
                HomePhase::GoingToDoor | HomePhase::OpeningToEnter { .. } => {
                    *activity = CharacterActivity::Idle;
                    commands
                        .entity(villager)
                        .remove::<HomeRoutine>()
                        .remove::<BuildingDoorUse>()
                        .remove::<MoveTarget>()
                        .remove::<TravelRoute>()
                        .remove::<NavigationRoutePending>()
                        .remove::<NavigationRouteFailed>();
                }
                HomePhase::Entering => {
                    *activity = CharacterActivity::Idle;
                    commands
                        .entity(villager)
                        .remove::<TravelRoute>()
                        .remove::<NavigationRoutePending>()
                        .remove::<NavigationRouteFailed>()
                        .insert((door_use, MoveTarget(outside)));
                    routine.phase = HomePhase::Leaving;
                }
                HomePhase::Sleeping => {
                    *activity = CharacterActivity::Indoors;
                    commands
                        .entity(villager)
                        .insert(door_use)
                        .remove::<MoveTarget>();
                    routine.phase = HomePhase::OpeningToLeave {
                        seconds_left: DOOR_OPEN_SECONDS,
                    };
                }
                HomePhase::OpeningToLeave { seconds_left } => {
                    *activity = CharacterActivity::Indoors;
                    commands
                        .entity(villager)
                        .insert(door_use)
                        .remove::<MoveTarget>();
                    let left = seconds_left - dt;
                    if left > 0.0 {
                        routine.phase = HomePhase::OpeningToLeave { seconds_left: left };
                    } else {
                        *activity = CharacterActivity::Idle;
                        commands.entity(villager).insert(MoveTarget(outside));
                        routine.phase = HomePhase::Leaving;
                    }
                }
                HomePhase::Leaving => {
                    *activity = CharacterActivity::Idle;
                    commands.entity(villager).insert(door_use);
                    let still_inside_blocker = obstacles.as_deref().is_some_and(|grid| {
                        grid.point_blocked(Vec2::new(position.0.x, position.0.z))
                    });
                    if ground_distance(position.0, outside) <= DOOR_REACH && !still_inside_blocker {
                        commands
                            .entity(villager)
                            .remove::<HomeRoutine>()
                            .remove::<BuildingDoorUse>()
                            .remove::<MoveTarget>()
                            .remove::<TravelRoute>()
                            .remove::<NavigationRoutePending>()
                            .remove::<NavigationRouteFailed>();
                    } else {
                        ensure_move_target(&mut commands, villager, move_target, outside);
                    }
                }
            }
            continue;
        }

        match routine.phase {
            HomePhase::LeavingWorkplace => {
                // `run_workplace_door_transits` owns this threshold and changes
                // the phase after the worker reaches the exterior door.
                *activity = CharacterActivity::Idle;
            }
            HomePhase::GoingToDoor => {
                *activity = CharacterActivity::Idle;
                if ground_distance(position.0, door) <= DOOR_REACH {
                    commands
                        .entity(villager)
                        .remove::<MoveTarget>()
                        .insert(door_use);
                    let to_house = home_position.0 - door;
                    if to_house.length_squared() > 1e-4 {
                        facing.0 = build_clip_facing(to_house);
                    }
                    routine.phase = HomePhase::OpeningToEnter {
                        seconds_left: DOOR_OPEN_SECONDS,
                    };
                } else {
                    ensure_move_target(&mut commands, villager, move_target, door);
                }
            }
            HomePhase::OpeningToEnter { seconds_left } => {
                *activity = CharacterActivity::Idle;
                commands
                    .entity(villager)
                    .insert(door_use)
                    .remove::<MoveTarget>();
                let left = seconds_left - dt;
                if left > 0.0 {
                    routine.phase = HomePhase::OpeningToEnter { seconds_left: left };
                } else {
                    commands.entity(villager).insert(MoveTarget(inside));
                    routine.phase = HomePhase::Entering;
                }
            }
            HomePhase::Entering => {
                *activity = CharacterActivity::Idle;
                commands.entity(villager).insert(door_use);
                if ground_distance(position.0, inside) <= DOOR_REACH {
                    *activity = CharacterActivity::Indoors;
                    commands
                        .entity(villager)
                        .remove::<BuildingDoorUse>()
                        .remove::<MoveTarget>();
                    routine.phase = HomePhase::Sleeping;
                } else {
                    ensure_move_target(&mut commands, villager, move_target, inside);
                }
            }
            HomePhase::Sleeping => {
                *activity = CharacterActivity::Indoors;
                commands
                    .entity(villager)
                    .remove::<BuildingDoorUse>()
                    .remove::<MoveTarget>();
            }
            HomePhase::OpeningToLeave { .. } | HomePhase::Leaving => {
                // A debug jump back into night while leaving turns them around
                // cleanly rather than stranding them in an impossible phase.
                *activity = CharacterActivity::Idle;
                commands
                    .entity(villager)
                    .insert((door_use, MoveTarget(inside)));
                routine.phase = HomePhase::Entering;
            }
        }
    }
}

/// Plant the two authored wheat fields beside every completed Farmstead.
pub fn ensure_farm_fields(
    mut commands: Commands,
    terrain: Option<Res<WorldTerrain>>,
    farms: Query<(&SettlementBuilding, &PlayerPosition, &PlayerRotation)>,
    fields: Query<&FarmField>,
) {
    let Some(terrain) = terrain else {
        return;
    };
    let planted: HashSet<(FarmsteadPositionKey, u8)> = fields
        .iter()
        .map(|field| (farmstead_position_key(field.farmstead), field.plot_index))
        .collect();
    for (farm, position, rotation) in farms.iter() {
        if farm.kind != SettlementBuildingKind::Farmstead {
            continue;
        }
        let key = farmstead_position_key(position.0);
        for plot_index in 0..shared::components::FARM_FIELDS_PER_FARMSTEAD {
            if planted.contains(&(key, plot_index)) {
                continue;
            }
            let Some(mut field_position) = farm
                .kind
                .field_position_at(position.0, rotation.0, plot_index)
            else {
                continue;
            };
            field_position.y = terrain.get_height(field_position.x, field_position.z);
            commands.spawn((
                FarmField {
                    settlement: farm.settlement.clone(),
                    farmstead: position.0,
                    plot_index,
                    quality: farm.quality,
                },
                PlayerPosition(field_position),
                PlayerRotation(rotation.0),
                Replicate::to_clients(NetworkTarget::All),
            ));
            info!(
                "Village '{}': planted wheat field {}/{} beside its Farmstead",
                farm.settlement,
                plot_index + 1,
                shared::components::FARM_FIELDS_PER_FARMSTEAD,
            );
        }
    }
}

/// Place the separate collider-free pier behind every completed Fisherman's
/// Hut. The asset origin is its landward end; the authored piles extend below
/// that origin, so water level is the stable placement plane on every coast.
pub fn ensure_fishing_piers(
    mut commands: Commands,
    terrain: Option<Res<WorldTerrain>>,
    huts: Query<(&SettlementBuilding, &PlayerPosition, &PlayerRotation)>,
    piers: Query<&FishingPier>,
) {
    let Some(terrain) = terrain else {
        return;
    };
    let Some(water) = terrain.water_level() else {
        return;
    };
    for (hut, position, rotation) in huts.iter() {
        if hut.kind != SettlementBuildingKind::FishermansHut
            || piers.iter().any(|pier| pier.fishermans_hut == position.0)
        {
            continue;
        }
        let Some(mut pier_position) = hut.kind.pier_position(position.0, rotation.0) else {
            continue;
        };
        pier_position.y = water;
        commands.spawn((
            FishingPier {
                settlement: hut.settlement.clone(),
                fishermans_hut: position.0,
                quality: hut.quality,
            },
            PlayerPosition(pier_position),
            PlayerRotation(rotation.0),
            Replicate::to_clients(NetworkTarget::All),
        ));
        info!(
            "Village '{}': set a fishing pier behind its Fisherman's Hut",
            hut.settlement
        );
    }
}

pub fn assign_farmer_routines(
    mut commands: Commands,
    world_time: Query<&WorldTime>,
    farms: Query<(
        Entity,
        &SettlementBuilding,
        &PlayerPosition,
        &PlayerRotation,
    )>,
    fields: Query<(Entity, &FarmField)>,
    settlements: Query<(Entity, &Settlement)>,
    villagers: Query<
        (
            Entity,
            &CharacterName,
            &VillagerIntent,
            &PlayerPosition,
            Option<&WorkerOffDuty>,
            Option<&FarmerHarvestProgress>,
        ),
        (
            Without<FarmerRoutine>,
            Without<FishingRoutine>,
            Without<LumberjackRoutine>,
        ),
    >,
) {
    let Some(clock) = world_time.iter().next() else {
        return;
    };
    if villagers.is_empty() || !ordinary_workday(clock) {
        return;
    }
    let mut fields_by_farm: HashMap<FarmsteadPositionKey, Vec<(Entity, u8)>> = HashMap::new();
    for (field_entity, field) in fields.iter() {
        fields_by_farm
            .entry(farmstead_position_key(field.farmstead))
            .or_default()
            .push((field_entity, field.plot_index));
    }
    for farm_fields in fields_by_farm.values_mut() {
        farm_fields.sort_by_key(|(_, plot_index)| *plot_index);
    }
    let mut claimed = HashSet::new();
    for (farmstead, farm, position, rotation) in farms.iter() {
        if farm.kind != SettlementBuildingKind::Farmstead {
            continue;
        }
        let Some(farm_fields) = fields_by_farm.get(&farmstead_position_key(position.0)) else {
            continue;
        };
        let Some((hall, _)) = settlements
            .iter()
            .find(|(_, settlement)| settlement.name == farm.settlement)
        else {
            continue;
        };
        for (worker_index, worker_name) in farm.workers.iter().enumerate() {
            let field = farm_fields[worker_index % farm_fields.len()].0;
            let candidate = villagers
                .iter()
                .filter(|(entity, name, intent, _, off_duty, _)| {
                    !claimed.contains(entity)
                        && name.0 == *worker_name
                        && intent.is_settled()
                        && intent.settlement() == Some(hall)
                        && off_duty.is_none_or(|off_duty| off_duty.day != clock.day)
                })
                .min_by(|a, b| {
                    a.3 .0
                        .distance_squared(position.0)
                        .total_cmp(&b.3 .0.distance_squared(position.0))
                });
            let Some((worker, _, _, _, _, progress)) = candidate else {
                continue;
            };
            let harvest_seconds = progress
                .filter(|progress| progress.farmstead == farmstead && progress.field == field)
                .map_or(0.0, |progress| progress.seconds);
            claimed.insert(worker);
            commands
                .entity(worker)
                .remove::<WorkerOffDuty>()
                .remove::<FarmerHarvestProgress>()
                .remove::<TravelRoute>()
                .remove::<NavigationRoutePending>()
                .remove::<NavigationRouteFailed>()
                .insert((
                    FarmerRoutine {
                        farmstead,
                        field,
                        hall,
                        harvest_seconds,
                        production_day: u32::MAX,
                        produced_today: 0,
                        phase: FarmerPhase::GoingToFarmstead,
                    },
                    CharacterActivity::Idle,
                    MoveTarget(farm.kind.entrance_position(position.0, rotation.0)),
                ));
        }
    }
}

/// Run the producer-owned part of the physical wheat loop: field to Farmstead.
/// Farmers never sell or haul to the Moot Hall. The market porter independently
/// evaluates the Farmstead's sale policy and collects only approved surplus.
#[allow(clippy::too_many_arguments)]
pub fn run_farmer_routines(
    time: Res<Time>,
    warp: Query<&shared::components::TimeWarp>,
    world_time: Query<&WorldTime>,
    obstacles: Option<Res<SpatialObstacleGrid>>,
    colliders: Option<Res<StaticColliders>>,
    derived: Option<Res<DerivedColliderLibrary>>,
    mut economy_runtime: ResMut<SettlementEconomyRuntime>,
    mut commands: Commands,
    farms: Query<(&SettlementBuilding, &PlayerPosition, &PlayerRotation), Without<CharacterKind>>,
    fields: Query<(&FarmField, &PlayerPosition, &PlayerRotation), Without<CharacterKind>>,
    halls: Query<&Settlement, Without<CharacterKind>>,
    mut inventories: Query<&mut GoodsInventory>,
    mut workers: Query<
        (
            Entity,
            &CharacterName,
            &PlayerPosition,
            &VillagerIntent,
            Option<&HomeRoutine>,
            Option<&HouseholdShoppingRoutine>,
            Option<&RoadBuilderRoutine>,
            Option<&WorkplaceDoorTransit>,
            &mut CharacterActivity,
            Option<&mut CharacterAttributes>,
            &mut FarmerRoutine,
            Option<&MoveTarget>,
            Option<&NavigationRouteFailed>,
        ),
        With<CharacterKind>,
    >,
) {
    let factor = warp
        .iter()
        .next()
        .map(|time_warp| time_warp.0)
        .unwrap_or(1.0);
    let dt = time.delta_secs() * factor;
    let Some(clock) = world_time.iter().next() else {
        return;
    };
    let production_day = clock.day;
    let workday_active = ordinary_workday(clock);

    for (
        worker,
        name,
        position,
        intent,
        home_routine,
        shopping,
        road_builder,
        door_transit,
        mut activity,
        mut attributes,
        mut routine,
        move_target,
        route_failed,
    ) in workers.iter_mut()
    {
        if routine.production_day != production_day {
            routine.production_day = production_day;
            routine.produced_today = 0;
        }
        if home_routine.is_some()
            || shopping.is_some()
            || road_builder.is_some()
            || door_transit.is_some()
        {
            continue;
        }
        if !intent.is_settled() {
            if *activity != CharacterActivity::Idle {
                *activity = CharacterActivity::Idle;
            }
            continue;
        }
        let Ok((farm, farm_position, farm_rotation)) = farms.get(routine.farmstead) else {
            commands
                .entity(worker)
                .remove::<FarmerRoutine>()
                .remove::<WorkplaceDoorTransit>()
                .remove::<BuildingDoorUse>()
                .remove::<MoveTarget>();
            *activity = CharacterActivity::Idle;
            continue;
        };
        if farm.kind != SettlementBuildingKind::Farmstead
            || !farm
                .workers
                .iter()
                .any(|worker_name| worker_name == &name.0)
        {
            commands
                .entity(worker)
                .remove::<FarmerRoutine>()
                .remove::<WorkplaceDoorTransit>()
                .remove::<BuildingDoorUse>()
                .remove::<MoveTarget>();
            *activity = CharacterActivity::Idle;
            continue;
        }
        let Ok((field, field_position, field_rotation)) = fields.get(routine.field) else {
            continue;
        };
        let Ok(hall) = halls.get(routine.hall) else {
            continue;
        };
        if field.settlement != farm.settlement || hall.name != farm.settlement {
            continue;
        }
        let farm_entrance = farm
            .kind
            .entrance_position(farm_position.0, farm_rotation.0);
        let farm_inside = farm
            .kind
            .interior_door_position(farm_position.0, farm_rotation.0);

        if let Some(failed) = route_failed {
            let retry_target = match routine.phase {
                FarmerPhase::WalkingToField { stand } => Some(stand),
                FarmerPhase::GoingToFarmstead | FarmerPhase::ReturningToFarmstead => {
                    Some(farm_entrance)
                }
                FarmerPhase::Inside { .. } | FarmerPhase::Farming | FarmerPhase::EndingShift => {
                    None
                }
            };
            commands
                .entity(worker)
                .remove::<NavigationRouteFailed>()
                .remove::<NavigationRoutePending>()
                .remove::<TravelRoute>();
            if let Some(target) = retry_target {
                // One failed commute must not turn an employed farmer into an
                // ambient idler for the rest of the day. Keep the exact work
                // phase (and any carried Wheat), then let the route planner
                // try a different certified corridor.
                commands.entity(worker).insert(MoveTarget(target));
                *activity = CharacterActivity::Idle;
            } else {
                warn!(
                    "Farmer {} discarded stale route failure for {:.1},{:.1} while {:?}",
                    name.0, failed.goal.x, failed.goal.z, routine.phase
                );
            }
            continue;
        }

        if !workday_active {
            match routine.phase {
                FarmerPhase::Inside { .. } => {
                    *activity = CharacterActivity::Idle;
                    begin_workplace_exit(
                        &mut commands,
                        worker,
                        farm_position.0,
                        farm_entrance,
                        farm_inside,
                        farm_entrance,
                    );
                    routine.phase = FarmerPhase::EndingShift;
                    continue;
                }
                FarmerPhase::WalkingToField { .. } | FarmerPhase::Farming => {
                    *activity = CharacterActivity::Idle;
                    ensure_move_target(&mut commands, worker, move_target, farm_entrance);
                    routine.phase = FarmerPhase::ReturningToFarmstead;
                    continue;
                }
                FarmerPhase::GoingToFarmstead
                    if ground_distance(position.0, farm_entrance) <= DOOR_REACH =>
                {
                    finish_farmer_shift(
                        &mut commands,
                        worker,
                        production_day,
                        &routine,
                        &mut activity,
                    );
                    continue;
                }
                FarmerPhase::EndingShift => {
                    finish_farmer_shift(
                        &mut commands,
                        worker,
                        production_day,
                        &routine,
                        &mut activity,
                    );
                    continue;
                }
                FarmerPhase::GoingToFarmstead | FarmerPhase::ReturningToFarmstead => {}
            }
        }

        match routine.phase {
            FarmerPhase::GoingToFarmstead => {
                if ground_distance(position.0, farm_entrance) <= DOOR_REACH {
                    if !workday_active {
                        finish_farmer_shift(
                            &mut commands,
                            worker,
                            production_day,
                            &routine,
                            &mut activity,
                        );
                        continue;
                    }
                    *activity = CharacterActivity::Idle;
                    begin_workplace_entry(
                        &mut commands,
                        worker,
                        farm_position.0,
                        farm_entrance,
                        farm_inside,
                    );
                    routine.phase = FarmerPhase::Inside {
                        seconds_left: INDOOR_REST_SECONDS,
                    };
                } else {
                    ensure_move_target(&mut commands, worker, move_target, farm_entrance);
                }
            }
            FarmerPhase::Inside { seconds_left } => {
                *activity = CharacterActivity::Indoors;
                let left = seconds_left - dt;
                if left > 0.0 {
                    routine.phase = FarmerPhase::Inside { seconds_left: left };
                    continue;
                }
                let Some(stand) = farm_work_stand(
                    field_position.0,
                    field_rotation.0,
                    stable_name_hash(&name.0),
                    obstacles.as_deref(),
                    colliders.as_deref(),
                    derived.as_deref(),
                ) else {
                    // Legacy worlds can contain a field planted before crop
                    // plots reserved prop-free ground. Stay safely inside and
                    // periodically re-evaluate instead of walking into a tree
                    // or abandoning the entire paid shift.
                    routine.phase = FarmerPhase::Inside {
                        seconds_left: INDOOR_REST_SECONDS,
                    };
                    continue;
                };
                *activity = CharacterActivity::Idle;
                begin_workplace_exit(
                    &mut commands,
                    worker,
                    farm_position.0,
                    farm_entrance,
                    farm_inside,
                    stand,
                );
                routine.phase = FarmerPhase::WalkingToField { stand };
            }
            FarmerPhase::WalkingToField { stand } => {
                if ground_distance(position.0, stand) <= WORK_REACH {
                    commands.entity(worker).remove::<MoveTarget>();
                    *activity = CharacterActivity::Farming;
                    routine.phase = FarmerPhase::Farming;
                } else {
                    ensure_move_target(&mut commands, worker, move_target, stand);
                }
            }
            FarmerPhase::Farming => {
                *activity = CharacterActivity::Farming;
                routine.harvest_seconds += dt;
                let seconds_per_wheat = farmer_seconds_per_wheat(field.quality);
                if let Ok(mut carrier) = inventories.get_mut(worker) {
                    // The carried bundle is a completed field basket, not a
                    // frame-by-frame progress meter. Publishing the first of
                    // two Wheat immediately made the client replace the
                    // harvest animation with a stationary carry pose for the
                    // entire second production interval. Retain continuous
                    // labour internally, then materialise the full batch and
                    // leave the field on the same tick.
                    let carried = carrier.amount(Good::Wheat);
                    let needed = FARM_CARRY_BATCH_UNITS.saturating_sub(carried);
                    let batch_seconds = seconds_per_wheat * needed as f32;
                    if needed > 0 && routine.harvest_seconds < batch_seconds {
                        continue;
                    }
                    let first_harvest_today = routine.produced_today == 0;
                    let produced = carrier.add(Good::Wheat, needed);
                    if produced > 0 {
                        routine.harvest_seconds = (routine.harvest_seconds
                            - seconds_per_wheat * produced as f32)
                            .max(0.0);
                    }
                    routine.produced_today = routine.produced_today.saturating_add(produced);
                    economy_runtime.record_food_production(routine.hall, produced);
                    // Attribute progression remains once per productive day
                    // even though production itself has no daily cap.
                    if produced > 0 && first_harvest_today {
                        if let Some(attributes) = attributes.as_deref_mut() {
                            attributes.train_physique(1);
                        }
                    }
                    let batch_ready = carrier.amount(Good::Wheat) >= FARM_CARRY_BATCH_UNITS;
                    let can_keep_harvesting =
                        !batch_ready && carrier.free_bulk() >= Good::Wheat.bulk_per_unit();
                    if can_keep_harvesting {
                        continue;
                    }
                }
                *activity = CharacterActivity::Idle;
                commands.entity(worker).insert(MoveTarget(farm_entrance));
                routine.phase = FarmerPhase::ReturningToFarmstead;
            }
            FarmerPhase::ReturningToFarmstead => {
                if ground_distance(position.0, farm_entrance) > DOOR_REACH {
                    ensure_move_target(&mut commands, worker, move_target, farm_entrance);
                    continue;
                }
                commands.entity(worker).remove::<MoveTarget>();
                if let Ok([mut carrier, mut farm_store]) =
                    inventories.get_many_mut([worker, routine.farmstead])
                {
                    carrier.transfer_to(&mut farm_store, Good::Wheat, u32::MAX);
                }
                if !workday_active {
                    finish_farmer_shift(
                        &mut commands,
                        worker,
                        production_day,
                        &routine,
                        &mut activity,
                    );
                    continue;
                }
                *activity = CharacterActivity::Idle;
                begin_workplace_entry(
                    &mut commands,
                    worker,
                    farm_position.0,
                    farm_entrance,
                    farm_inside,
                );
                routine.phase = FarmerPhase::Inside {
                    seconds_left: INDOOR_REST_SECONDS,
                };
            }
            FarmerPhase::EndingShift => {
                finish_farmer_shift(
                    &mut commands,
                    worker,
                    production_day,
                    &routine,
                    &mut activity,
                );
            }
        }
    }
}

/// Attach the physical hut-to-pier loop to each named fisher.
pub fn assign_fishing_routines(
    mut commands: Commands,
    world_time: Query<&WorldTime>,
    huts: Query<(
        Entity,
        &SettlementBuilding,
        &PlayerPosition,
        &PlayerRotation,
    )>,
    piers: Query<(Entity, &FishingPier)>,
    settlements: Query<(Entity, &Settlement)>,
    villagers: Query<
        (
            Entity,
            &CharacterName,
            &VillagerIntent,
            &PlayerPosition,
            Option<&WorkerOffDuty>,
            Option<&FishingWorkProgress>,
        ),
        (
            Without<FarmerRoutine>,
            Without<FishingRoutine>,
            Without<LumberjackRoutine>,
        ),
    >,
) {
    let Some(clock) = world_time.iter().next() else {
        return;
    };
    if villagers.is_empty() || !ordinary_workday(clock) {
        return;
    }
    let mut claimed = HashSet::new();
    for (hut_entity, hut, hut_position, hut_rotation) in huts.iter() {
        if hut.kind != SettlementBuildingKind::FishermansHut {
            continue;
        }
        let Some((pier, _)) = piers
            .iter()
            .find(|(_, pier)| pier.fishermans_hut == hut_position.0)
        else {
            continue;
        };
        let Some((hall, _)) = settlements
            .iter()
            .find(|(_, settlement)| settlement.name == hut.settlement)
        else {
            continue;
        };
        for worker_name in &hut.workers {
            let candidate = villagers
                .iter()
                .filter(|(entity, name, intent, _, off_duty, _)| {
                    !claimed.contains(entity)
                        && name.0 == *worker_name
                        && intent.is_settled()
                        && intent.settlement() == Some(hall)
                        && off_duty.is_none_or(|off_duty| off_duty.day != clock.day)
                })
                .min_by(|a, b| {
                    a.3 .0
                        .distance_squared(hut_position.0)
                        .total_cmp(&b.3 .0.distance_squared(hut_position.0))
                });
            let Some((worker, _, _, _, _, progress)) = candidate else {
                continue;
            };
            let catch_seconds = progress
                .filter(|progress| progress.hut == hut_entity && progress.pier == pier)
                .map_or(0.0, |progress| progress.seconds);
            claimed.insert(worker);
            commands
                .entity(worker)
                .remove::<WorkerOffDuty>()
                .remove::<FishingWorkProgress>()
                .remove::<TravelRoute>()
                .remove::<NavigationRoutePending>()
                .remove::<NavigationRouteFailed>()
                .insert((
                    FishingRoutine {
                        hut: hut_entity,
                        pier,
                        hall,
                        catch_seconds,
                        production_day: u32::MAX,
                        produced_today: 0,
                        phase: FishingPhase::GoingToHut,
                    },
                    CharacterActivity::Idle,
                    MoveTarget(hut.kind.entrance_position(hut_position.0, hut_rotation.0)),
                ));
            info!(
                "Village '{}': {worker_name} began fishing from the new pier",
                hut.settlement
            );
        }
    }
}

fn fishing_land_point(terrain: &WorldTerrain, hut: Vec3, rotation: f32, local: Vec2) -> Vec3 {
    let offset = shared::rotation::local_to_world_xz(local, rotation);
    let x = hut.x + offset.x;
    let z = hut.z + offset.y;
    Vec3::new(x, terrain.get_height(x, z), z)
}

fn fishing_deck_points(pier: Vec3, rotation: f32) -> (Vec3, Vec3) {
    const DECK_HEIGHT: f32 = 0.52;
    let deck_start = Vec3::new(pier.x, pier.y + DECK_HEIGHT, pier.z);
    let offset = shared::rotation::local_to_world_xz(Vec2::new(0.0, 6.25), rotation);
    let fish_spot = Vec3::new(pier.x + offset.x, pier.y + DECK_HEIGHT, pier.z + offset.y);
    (deck_start, fish_spot)
}

fn install_fishing_route(
    commands: &mut Commands,
    worker: Entity,
    goal: Vec3,
    waypoints: impl IntoIterator<Item = Vec3>,
    traversal: PierTraversal,
) {
    commands
        .entity(worker)
        .remove::<NavigationRoutePending>()
        .remove::<NavigationRouteFailed>()
        .insert((
            MoveTarget(goal),
            TravelRoute {
                goal,
                waypoints: waypoints
                    .into_iter()
                    .map(|position| RouteWaypoint {
                        position,
                        on_road: false,
                    })
                    .collect(),
                next: 0,
            },
            traversal,
        ));
}

/// Run the visible direct-food loop:
/// hut -> safe side route -> pier -> fish until a carry batch is full -> hut.
/// The fisher deposits only into the private hut store; the Moot porter later
/// collects policy-approved surplus as a separate job.
#[allow(clippy::too_many_arguments)]
pub fn run_fishing_routines(
    time: Res<Time>,
    terrain: Option<Res<WorldTerrain>>,
    warp: Query<&shared::components::TimeWarp>,
    world_time: Query<&WorldTime>,
    mut economy_runtime: ResMut<SettlementEconomyRuntime>,
    mut commands: Commands,
    huts: Query<(&SettlementBuilding, &PlayerPosition, &PlayerRotation), Without<CharacterKind>>,
    piers: Query<(&FishingPier, &PlayerPosition, &PlayerRotation), Without<CharacterKind>>,
    halls: Query<&Settlement, Without<CharacterKind>>,
    mut inventories: Query<&mut GoodsInventory>,
    mut workers: Query<
        (
            Entity,
            &CharacterName,
            &PlayerPosition,
            &VillagerIntent,
            Option<&HomeRoutine>,
            Option<&HouseholdShoppingRoutine>,
            Option<&RoadBuilderRoutine>,
            Option<&WorkplaceDoorTransit>,
            &mut PlayerRotation,
            &mut CharacterActivity,
            &mut FishingRoutine,
            Option<&MoveTarget>,
        ),
        With<CharacterKind>,
    >,
) {
    let Some(terrain) = terrain else {
        return;
    };
    let dt = time.delta_secs() * time_warp_factor(&warp);
    let Some(clock) = world_time.iter().next() else {
        return;
    };
    let production_day = clock.day;
    let workday_active = ordinary_workday(clock);

    for (
        worker,
        name,
        position,
        intent,
        home_routine,
        shopping,
        road_builder,
        door_transit,
        mut facing,
        mut activity,
        mut routine,
        move_target,
    ) in workers.iter_mut()
    {
        if routine.production_day != production_day {
            routine.production_day = production_day;
            routine.produced_today = 0;
        }
        if home_routine.is_some()
            || shopping.is_some()
            || road_builder.is_some()
            || door_transit.is_some()
        {
            continue;
        }
        if !intent.is_settled() {
            *activity = CharacterActivity::Idle;
            continue;
        }
        let Ok((hut, hut_position, hut_rotation)) = huts.get(routine.hut) else {
            commands
                .entity(worker)
                .remove::<FishingRoutine>()
                .remove::<PierTraversal>()
                .remove::<WorkplaceDoorTransit>()
                .remove::<BuildingDoorUse>()
                .remove::<TravelRoute>()
                .remove::<NavigationRoutePending>()
                .remove::<MoveTarget>();
            *activity = CharacterActivity::Idle;
            continue;
        };
        if hut.kind != SettlementBuildingKind::FishermansHut
            || !hut.workers.iter().any(|worker_name| worker_name == &name.0)
        {
            commands
                .entity(worker)
                .remove::<FishingRoutine>()
                .remove::<PierTraversal>()
                .remove::<TravelRoute>()
                .remove::<NavigationRoutePending>()
                .remove::<MoveTarget>();
            *activity = CharacterActivity::Idle;
            continue;
        }
        let Ok((pier, pier_position, pier_rotation)) = piers.get(routine.pier) else {
            continue;
        };
        let Ok(hall) = halls.get(routine.hall) else {
            continue;
        };
        if pier.settlement != hut.settlement || hall.name != hut.settlement {
            continue;
        }

        let entrance = hut.kind.entrance_position(hut_position.0, hut_rotation.0);
        let inside = hut
            .kind
            .interior_door_position(hut_position.0, hut_rotation.0);
        let staging = fishing_land_point(
            &terrain,
            hut_position.0,
            hut_rotation.0,
            Vec2::new(-4.15, -4.45),
        );
        let nets = fishing_land_point(
            &terrain,
            hut_position.0,
            hut_rotation.0,
            Vec2::new(-4.15, -0.35),
        );
        let rear = fishing_land_point(
            &terrain,
            hut_position.0,
            hut_rotation.0,
            Vec2::new(-4.15, 2.85),
        );
        let (deck_start, fish_spot) = fishing_deck_points(pier_position.0, pier_rotation.0);
        let traversal = PierTraversal {
            deck_start,
            deck_end: fish_spot,
        };

        if !workday_active {
            match routine.phase {
                FishingPhase::Inside { .. } => {
                    *activity = CharacterActivity::Idle;
                    begin_workplace_exit(
                        &mut commands,
                        worker,
                        hut_position.0,
                        entrance,
                        inside,
                        entrance,
                    );
                    routine.phase = FishingPhase::EndingShift;
                    continue;
                }
                FishingPhase::Fishing | FishingPhase::WalkingToPier => {
                    *activity = CharacterActivity::Idle;
                    install_fishing_route(
                        &mut commands,
                        worker,
                        staging,
                        [deck_start, rear, nets, staging],
                        traversal,
                    );
                    routine.phase = FishingPhase::ReturningFromPier { staging };
                    continue;
                }
                FishingPhase::StagingForPier { .. } => {
                    commands
                        .entity(worker)
                        .remove::<PierTraversal>()
                        .remove::<TravelRoute>()
                        .remove::<NavigationRoutePending>();
                    ensure_move_target(&mut commands, worker, move_target, entrance);
                    routine.phase = FishingPhase::ReturningToHut;
                    continue;
                }
                FishingPhase::GoingToHut if ground_distance(position.0, entrance) <= DOOR_REACH => {
                    finish_fishing_shift(
                        &mut commands,
                        worker,
                        production_day,
                        &routine,
                        &mut activity,
                    );
                    continue;
                }
                FishingPhase::EndingShift => {
                    finish_fishing_shift(
                        &mut commands,
                        worker,
                        production_day,
                        &routine,
                        &mut activity,
                    );
                    continue;
                }
                FishingPhase::GoingToHut
                | FishingPhase::ReturningFromPier { .. }
                | FishingPhase::ReturningToHut => {}
            }
        }

        match routine.phase {
            FishingPhase::GoingToHut => {
                if ground_distance(position.0, entrance) <= DOOR_REACH {
                    if !workday_active {
                        finish_fishing_shift(
                            &mut commands,
                            worker,
                            production_day,
                            &routine,
                            &mut activity,
                        );
                        continue;
                    }
                    *activity = CharacterActivity::Idle;
                    begin_workplace_entry(&mut commands, worker, hut_position.0, entrance, inside);
                    routine.phase = FishingPhase::Inside {
                        seconds_left: INDOOR_REST_SECONDS,
                    };
                } else {
                    ensure_move_target(&mut commands, worker, move_target, entrance);
                }
            }
            FishingPhase::Inside { seconds_left } => {
                *activity = CharacterActivity::Indoors;
                let left = seconds_left - dt;
                if left > 0.0 {
                    routine.phase = FishingPhase::Inside { seconds_left: left };
                    continue;
                }
                *activity = CharacterActivity::Idle;
                begin_workplace_exit(
                    &mut commands,
                    worker,
                    hut_position.0,
                    entrance,
                    inside,
                    staging,
                );
                routine.phase = FishingPhase::StagingForPier { staging };
            }
            FishingPhase::StagingForPier { staging } => {
                if ground_distance(position.0, staging) > WORK_REACH {
                    ensure_move_target(&mut commands, worker, move_target, staging);
                    continue;
                }
                install_fishing_route(
                    &mut commands,
                    worker,
                    fish_spot,
                    [nets, rear, deck_start, fish_spot],
                    traversal,
                );
                routine.phase = FishingPhase::WalkingToPier;
            }
            FishingPhase::WalkingToPier => {
                if ground_distance(position.0, fish_spot) <= WORK_REACH {
                    commands
                        .entity(worker)
                        .remove::<MoveTarget>()
                        .remove::<TravelRoute>()
                        .remove::<NavigationRoutePending>()
                        .remove::<PierTraversal>();
                    let outward = Vec2::new(fish_spot.x - deck_start.x, fish_spot.z - deck_start.z);
                    if outward.length_squared() > 1e-4 {
                        facing.0 = f32::atan2(-outward.x, -outward.y);
                    }
                    *activity = CharacterActivity::Fishing;
                    routine.phase = FishingPhase::Fishing;
                }
            }
            FishingPhase::Fishing => {
                *activity = CharacterActivity::Fishing;
                routine.catch_seconds += dt;
                let seconds_per_food = fisher_seconds_per_food(pier.quality);
                if let Ok(mut carrier) = inventories.get_mut(worker) {
                    let carried = carrier.amount(Good::Food);
                    let needed = FISH_CARRY_BATCH_UNITS.saturating_sub(carried);
                    let batch_seconds = seconds_per_food * needed as f32;
                    if needed > 0 && routine.catch_seconds < batch_seconds {
                        continue;
                    }
                    let produced = carrier.add(Good::Food, needed);
                    if produced > 0 {
                        routine.catch_seconds =
                            (routine.catch_seconds - seconds_per_food * produced as f32).max(0.0);
                    }
                    routine.produced_today = routine.produced_today.saturating_add(produced);
                    economy_runtime.record_food_production(routine.hall, produced);
                    if carrier.amount(Good::Food) < FISH_CARRY_BATCH_UNITS
                        && carrier.free_bulk() >= Good::Food.bulk_per_unit()
                    {
                        continue;
                    }
                }
                *activity = CharacterActivity::Idle;
                install_fishing_route(
                    &mut commands,
                    worker,
                    staging,
                    [deck_start, rear, nets, staging],
                    traversal,
                );
                routine.phase = FishingPhase::ReturningFromPier { staging };
            }
            FishingPhase::ReturningFromPier { staging } => {
                if ground_distance(position.0, staging) > WORK_REACH {
                    continue;
                }
                commands
                    .entity(worker)
                    .remove::<PierTraversal>()
                    .remove::<TravelRoute>()
                    .remove::<NavigationRoutePending>()
                    .insert(MoveTarget(entrance));
                routine.phase = FishingPhase::ReturningToHut;
            }
            FishingPhase::ReturningToHut => {
                if ground_distance(position.0, entrance) > DOOR_REACH {
                    ensure_move_target(&mut commands, worker, move_target, entrance);
                    continue;
                }
                commands.entity(worker).remove::<MoveTarget>();
                if let Ok([mut carrier, mut hut_store]) =
                    inventories.get_many_mut([worker, routine.hut])
                {
                    carrier.transfer_to(&mut hut_store, Good::Food, u32::MAX);
                }
                if !workday_active {
                    finish_fishing_shift(
                        &mut commands,
                        worker,
                        production_day,
                        &routine,
                        &mut activity,
                    );
                    continue;
                }
                begin_workplace_entry(&mut commands, worker, hut_position.0, entrance, inside);
                routine.phase = FishingPhase::Inside {
                    seconds_left: INDOOR_REST_SECONDS,
                };
            }
            FishingPhase::EndingShift => {
                finish_fishing_shift(
                    &mut commands,
                    worker,
                    production_day,
                    &routine,
                    &mut activity,
                );
            }
        }
    }
}

/// Attach a physical work routine to every staffed lumberjack hut.
///
/// Employment remains the source of truth: the routine exists only while the
/// villager's name is actually in that hut's worker roster. Entity-backed
/// jobs replace this name join later, when people gain stable `PersonId`s.
pub fn assign_lumberjack_routines(
    mut commands: Commands,
    world_time: Query<&WorldTime>,
    huts: Query<(
        Entity,
        &SettlementBuilding,
        &PlayerPosition,
        &PlayerRotation,
    )>,
    settlements: Query<(Entity, &Settlement)>,
    villagers: Query<
        (
            Entity,
            &CharacterName,
            &VillagerIntent,
            &PlayerPosition,
            Option<&WorkerOffDuty>,
            Option<&LumberjackWorkProgress>,
        ),
        (
            Without<FarmerRoutine>,
            Without<FishingRoutine>,
            Without<LumberjackRoutine>,
        ),
    >,
) {
    let Some(clock) = world_time.iter().next() else {
        return;
    };
    if villagers.is_empty() || !ordinary_workday(clock) {
        return;
    }
    let mut claimed = HashSet::new();
    for (hut_entity, building, hut_position, hut_rotation) in huts.iter() {
        if building.kind != SettlementBuildingKind::LumberjackHut {
            continue;
        }
        let Some((hall, _)) = settlements
            .iter()
            .find(|(_, settlement)| settlement.name == building.settlement)
        else {
            continue;
        };
        for worker_name in &building.workers {
            let candidate = villagers
                .iter()
                .filter(|(entity, name, intent, _, off_duty, _)| {
                    !claimed.contains(entity)
                        && name.0 == *worker_name
                        && intent.is_settled()
                        && intent.settlement() == Some(hall)
                        && off_duty.is_none_or(|off_duty| off_duty.day != clock.day)
                })
                .min_by(|a, b| {
                    a.3 .0
                        .distance_squared(hut_position.0)
                        .total_cmp(&b.3 .0.distance_squared(hut_position.0))
                });
            let Some((worker, _, _, _, _, progress)) = candidate else {
                continue;
            };
            let (cycle, chop_seconds) = progress
                .filter(|progress| progress.hut == hut_entity)
                .map_or((0, 0.0), |progress| (progress.cycle, progress.chop_seconds));
            claimed.insert(worker);
            let entrance = building
                .kind
                .entrance_position(hut_position.0, hut_rotation.0);
            commands
                .entity(worker)
                .remove::<WorkerOffDuty>()
                .remove::<LumberjackWorkProgress>()
                .remove::<TravelRoute>()
                .remove::<NavigationRoutePending>()
                .remove::<NavigationRouteFailed>()
                .insert((
                    LumberjackRoutine {
                        hut: hut_entity,
                        hall,
                        cycle,
                        failed_tree_routes: 0,
                        chop_seconds,
                        production_day: u32::MAX,
                        produced_today: 0,
                        phase: LumberjackPhase::GoingToHut,
                    },
                    CharacterActivity::Idle,
                    MoveTarget(entrance),
                ));
            info!(
                "Village '{}': {worker_name} began working from the lumberjack hut",
                building.settlement
            );
        }
    }
}

/// Run the observed woodcutting loop:
/// hut -> tree -> chop -> carry -> hut, with periodic hauling to the hall.
///
/// Every transfer uses bounded inventories. A full destination leaves the
/// remainder in the source, so congestion is visible as a worker who cannot
/// complete the next leg rather than as deleted resources.
#[allow(clippy::too_many_arguments)]
pub fn run_lumberjack_routines(
    time: Res<Time>,
    terrain: Option<Res<WorldTerrain>>,
    derived: Option<Res<DerivedColliderLibrary>>,
    obstacles: Option<Res<SpatialObstacleGrid>>,
    warp: Query<&shared::components::TimeWarp>,
    world_time: Query<&WorldTime>,
    mut commands: Commands,
    huts: Query<(&SettlementBuilding, &PlayerPosition, &PlayerRotation), Without<CharacterKind>>,
    halls: Query<&Settlement, Without<CharacterKind>>,
    mut inventories: Query<&mut GoodsInventory>,
    mut workers: Query<
        (
            Entity,
            &CharacterName,
            &PlayerPosition,
            &VillagerIntent,
            Option<&HomeRoutine>,
            Option<&HouseholdShoppingRoutine>,
            Option<&RoadBuilderRoutine>,
            Option<&WorkplaceDoorTransit>,
            &mut PlayerRotation,
            &mut CharacterActivity,
            &mut LumberjackRoutine,
            Option<&MoveTarget>,
            Option<&NavigationRoutePending>,
            Option<&NavigationRouteFailed>,
        ),
        With<CharacterKind>,
    >,
) {
    let Some(terrain) = terrain else {
        return;
    };
    let factor = warp
        .iter()
        .next()
        .map(|time_warp| time_warp.0)
        .unwrap_or(1.0);
    let dt = time.delta_secs() * factor;
    let Some(clock) = world_time.iter().next() else {
        return;
    };
    let production_day = clock.day;
    let workday_active = ordinary_workday(clock);

    for (
        worker,
        name,
        position,
        intent,
        home_routine,
        shopping,
        road_builder,
        door_transit,
        mut facing,
        mut activity,
        mut routine,
        move_target,
        route_pending,
        route_failed,
    ) in workers.iter_mut()
    {
        if routine.production_day != production_day {
            routine.production_day = production_day;
            routine.produced_today = 0;
        }
        if home_routine.is_some()
            || shopping.is_some()
            || road_builder.is_some()
            || door_transit.is_some()
        {
            continue;
        }
        // A permit temporarily takes this person's builder time. Construction
        // owns their destination until it releases them back to residency;
        // their ordinary job then resumes from the same physical phase.
        if !intent.is_settled() {
            if *activity != CharacterActivity::Idle {
                *activity = CharacterActivity::Idle;
            }
            continue;
        }
        let Ok((hut, hut_position, hut_rotation)) = huts.get(routine.hut) else {
            commands
                .entity(worker)
                .remove::<LumberjackRoutine>()
                .remove::<WorkplaceDoorTransit>()
                .remove::<BuildingDoorUse>()
                .remove::<MoveTarget>();
            if *activity != CharacterActivity::Idle {
                *activity = CharacterActivity::Idle;
            }
            continue;
        };
        if hut.kind != SettlementBuildingKind::LumberjackHut
            || !hut.workers.iter().any(|worker_name| worker_name == &name.0)
        {
            commands
                .entity(worker)
                .remove::<LumberjackRoutine>()
                .remove::<WorkplaceDoorTransit>()
                .remove::<BuildingDoorUse>()
                .remove::<MoveTarget>();
            if *activity != CharacterActivity::Idle {
                *activity = CharacterActivity::Idle;
            }
            continue;
        }
        let Ok(hall) = halls.get(routine.hall) else {
            continue;
        };
        if hall.name != hut.settlement {
            continue;
        }

        let hut_entrance = hut.kind.entrance_position(hut_position.0, hut_rotation.0);
        let hut_inside = hut
            .kind
            .interior_door_position(hut_position.0, hut_rotation.0);

        if route_failed.is_some()
            && matches!(
                routine.phase,
                LumberjackPhase::GoingToHut | LumberjackPhase::ReturningToHut
            )
        {
            // A failed route used to be handled only while walking to a tree.
            // A woodcutter carrying Wood home could therefore retain the
            // terminal failure forever and silently disable the business.
            // Clear the terminal navigation state and submit the hut entrance
            // again; the planner can then use updated obstacle geometry or a
            // different A* corridor without losing the carried goods.
            commands
                .entity(worker)
                .remove::<NavigationRouteFailed>()
                .remove::<NavigationRoutePending>()
                .remove::<TravelRoute>()
                .insert(MoveTarget(hut_entrance));
            *activity = CharacterActivity::Idle;
            continue;
        }

        if !workday_active {
            match routine.phase {
                LumberjackPhase::Inside { .. } => {
                    *activity = CharacterActivity::Idle;
                    begin_workplace_exit(
                        &mut commands,
                        worker,
                        hut_position.0,
                        hut_entrance,
                        hut_inside,
                        hut_entrance,
                    );
                    routine.phase = LumberjackPhase::EndingShift;
                    continue;
                }
                LumberjackPhase::WalkingToTree { .. } | LumberjackPhase::Chopping => {
                    commands
                        .entity(worker)
                        .remove::<TravelRoute>()
                        .remove::<NavigationRoutePending>()
                        .remove::<NavigationRouteFailed>();
                    *activity = CharacterActivity::Idle;
                    ensure_move_target(&mut commands, worker, move_target, hut_entrance);
                    routine.phase = LumberjackPhase::ReturningToHut;
                    continue;
                }
                LumberjackPhase::GoingToHut
                    if ground_distance(position.0, hut_entrance) <= DOOR_REACH =>
                {
                    finish_lumberjack_shift(
                        &mut commands,
                        worker,
                        production_day,
                        &routine,
                        &mut activity,
                    );
                    continue;
                }
                LumberjackPhase::EndingShift => {
                    finish_lumberjack_shift(
                        &mut commands,
                        worker,
                        production_day,
                        &routine,
                        &mut activity,
                    );
                    continue;
                }
                LumberjackPhase::GoingToHut | LumberjackPhase::ReturningToHut => {}
            }
        }

        match routine.phase {
            LumberjackPhase::GoingToHut => {
                if ground_distance(position.0, hut_entrance) <= DOOR_REACH {
                    if !workday_active {
                        finish_lumberjack_shift(
                            &mut commands,
                            worker,
                            production_day,
                            &routine,
                            &mut activity,
                        );
                        continue;
                    }
                    *activity = CharacterActivity::Idle;
                    begin_workplace_entry(
                        &mut commands,
                        worker,
                        hut_position.0,
                        hut_entrance,
                        hut_inside,
                    );
                    routine.phase = LumberjackPhase::Inside {
                        seconds_left: INDOOR_REST_SECONDS,
                    };
                } else {
                    ensure_move_target(&mut commands, worker, move_target, hut_entrance);
                }
            }
            LumberjackPhase::Inside { seconds_left } => {
                if *activity != CharacterActivity::Indoors {
                    *activity = CharacterActivity::Indoors;
                }
                let left = seconds_left - dt;
                if left > 0.0 {
                    routine.phase = LumberjackPhase::Inside { seconds_left: left };
                    continue;
                }
                let salt = stable_name_hash(&name.0);
                let Some((tree, stand)) = find_tree_for_cycle(
                    &terrain,
                    derived.as_deref(),
                    obstacles.as_deref(),
                    hut_position.0,
                    routine.cycle,
                    salt,
                ) else {
                    // Move through the deterministic pool when a tree has no
                    // collision-free interaction point. Retrying the same
                    // cycle here previously created a permanent indoor loop.
                    routine.cycle = routine.cycle.wrapping_add(1);
                    routine.phase = LumberjackPhase::Inside {
                        seconds_left: INDOOR_REST_SECONDS,
                    };
                    continue;
                };
                *activity = CharacterActivity::Idle;
                begin_workplace_exit(
                    &mut commands,
                    worker,
                    hut_position.0,
                    hut_entrance,
                    hut_inside,
                    stand,
                );
                routine.phase = LumberjackPhase::WalkingToTree { tree, stand };
            }
            LumberjackPhase::WalkingToTree { tree, stand } => {
                if let Some(failed) = route_failed {
                    let (cycle, failed_routes, widened) =
                        advance_failed_tree_candidate(routine.cycle, routine.failed_tree_routes);
                    routine.cycle = cycle;
                    routine.failed_tree_routes = failed_routes;
                    if widened {
                        warn!(
                            "Woodcutter {} exhausted twelve tree approaches after {:.1},{:.1}; widening the search instead of stopping",
                            name.0, failed.goal.x, failed.goal.z
                        );
                    }
                    commands
                        .entity(worker)
                        .remove::<MoveTarget>()
                        .remove::<NavigationRouteFailed>()
                        .remove::<NavigationRoutePending>()
                        .remove::<TravelRoute>()
                        .insert(MoveTarget(hut_entrance));
                    routine.phase = LumberjackPhase::GoingToHut;
                    *activity = CharacterActivity::Idle;
                    continue;
                }
                if route_pending.is_some_and(NavigationRoutePending::exhausted) {
                    // A coastline or cliff can put the deterministic nearest
                    // tree on a different landmass. Abandon that target and
                    // advance the stable search salt instead of staring at an
                    // exhausted path request forever.
                    commands
                        .entity(worker)
                        .remove::<MoveTarget>()
                        .remove::<NavigationRoutePending>()
                        .remove::<TravelRoute>();
                    routine.cycle = routine.cycle.wrapping_add(1);
                    routine.phase = LumberjackPhase::GoingToHut;
                    commands.entity(worker).insert(MoveTarget(hut_entrance));
                    *activity = CharacterActivity::Idle;
                    continue;
                }
                if ground_distance(position.0, stand) <= WORK_REACH {
                    routine.failed_tree_routes = 0;
                    commands.entity(worker).remove::<MoveTarget>();
                    let to_tree = tree - position.0;
                    if to_tree.length_squared() > 1e-4 {
                        facing.0 = f32::atan2(-to_tree.x, -to_tree.z);
                    }
                    *activity = CharacterActivity::Chopping;
                    routine.phase = LumberjackPhase::Chopping;
                } else {
                    ensure_move_target(&mut commands, worker, move_target, stand);
                }
            }
            LumberjackPhase::Chopping => {
                if *activity != CharacterActivity::Chopping {
                    *activity = CharacterActivity::Chopping;
                }
                routine.chop_seconds += dt;
                if routine.chop_seconds < CHOP_SECONDS {
                    continue;
                }
                let yield_units = lumber_tree_yield(hut.quality);
                if let Ok(mut carrier) = inventories.get_mut(worker) {
                    let produced = carrier.add(Good::Wood, yield_units);
                    if produced > 0 {
                        routine.chop_seconds = (routine.chop_seconds - CHOP_SECONDS).max(0.0);
                    }
                    routine.produced_today = routine.produced_today.saturating_add(produced);
                }
                routine.cycle = routine.cycle.wrapping_add(1);
                *activity = CharacterActivity::Idle;
                commands.entity(worker).insert(MoveTarget(hut_entrance));
                routine.phase = LumberjackPhase::ReturningToHut;
            }
            LumberjackPhase::ReturningToHut => {
                if ground_distance(position.0, hut_entrance) > DOOR_REACH {
                    ensure_move_target(&mut commands, worker, move_target, hut_entrance);
                    continue;
                }
                commands.entity(worker).remove::<MoveTarget>();
                if let Ok([mut carrier, mut hut_store]) =
                    inventories.get_many_mut([worker, routine.hut])
                {
                    carrier.transfer_to(&mut hut_store, Good::Wood, u32::MAX);
                }
                if !workday_active {
                    finish_lumberjack_shift(
                        &mut commands,
                        worker,
                        production_day,
                        &routine,
                        &mut activity,
                    );
                    continue;
                }
                *activity = CharacterActivity::Idle;
                begin_workplace_entry(
                    &mut commands,
                    worker,
                    hut_position.0,
                    hut_entrance,
                    hut_inside,
                );
                routine.phase = LumberjackPhase::Inside {
                    seconds_left: INDOOR_REST_SECONDS,
                };
            }
            LumberjackPhase::EndingShift => {
                finish_lumberjack_shift(
                    &mut commands,
                    worker,
                    production_day,
                    &routine,
                    &mut activity,
                );
            }
        }
    }
}

/// Keep the small replicated carried-load view in sync with private inventory.
pub fn sync_carried_load(
    mut carriers: Query<(&GoodsInventory, &mut CarriedLoad), With<CharacterKind>>,
) {
    for (inventory, mut carried) in carriers.iter_mut() {
        let next = CarriedLoad::from_inventory(inventory);
        if *carried != next {
            *carried = next;
        }
    }
}

/// Pick a real deterministic tree prop and a collision-free chopping point.
///
/// Tree removal/regrowth needs stable resource-node ids and a sparse depletion
/// map. Until that layer exists, the worker uses the same authored tree the
/// client already draws but does not yet remove it after harvesting.
fn advance_failed_tree_candidate(mut cycle: u32, mut failed_routes: u8) -> (u32, u8, bool) {
    failed_routes = failed_routes.saturating_add(1);
    cycle = cycle.wrapping_add(1);
    if failed_routes < 12 {
        return (cycle, failed_routes, false);
    }
    // `cycle` is the deterministic search salt. Jump farther after a whole
    // failed batch so the next search does not immediately revisit the same
    // local candidate set. Resetting the diagnostic counter keeps failure
    // bounded without turning twelve bad trees into a terminal AI state.
    cycle = cycle.wrapping_add(12);
    failed_routes = 0;
    (cycle, failed_routes, true)
}

fn timber_retry_delay(failures: u8) -> f64 {
    let exponent = u32::from(failures.saturating_sub(1)).min(8);
    (TIMBER_RETRY_BASE_SECONDS * 2_f64.powi(exponent as i32)).min(TIMBER_RETRY_MAX_SECONDS)
}

fn postpone_construction_tree_search(routine: &mut ConstructionMaterialRoutine, now: f64) -> bool {
    let retry_failures = routine.failed_tree_routes.saturating_add(1);
    let (cycle, failed_routes, widened) =
        advance_failed_tree_candidate(routine.cycle, routine.failed_tree_routes);
    routine.cycle = cycle;
    routine.failed_tree_routes = failed_routes;
    routine.tree_retry_after = now + timber_retry_delay(retry_failures);
    routine.phase = ConstructionMaterialPhase::Seeking;
    widened
}

fn postpone_construction_store_route(routine: &mut ConstructionMaterialRoutine, now: f64) {
    routine.failed_store_routes = routine.failed_store_routes.saturating_add(1);
    routine.store_retry_after = now + timber_retry_delay(routine.failed_store_routes);
    routine.phase = ConstructionMaterialPhase::Seeking;
}

const TREE_APPROACH_ANGLES: [f32; 8] = [
    0.0,
    std::f32::consts::FRAC_PI_4,
    -std::f32::consts::FRAC_PI_4,
    std::f32::consts::FRAC_PI_2,
    -std::f32::consts::FRAC_PI_2,
    3.0 * std::f32::consts::FRAC_PI_4,
    -3.0 * std::f32::consts::FRAC_PI_4,
    std::f32::consts::PI,
];

/// A failed route must not recreate the same interaction goal on the next
/// visit. `cycle` advances once per failed goal; after the routine has tried
/// every candidate tree, the preferred side of each trunk rotates as well.
fn tree_approach_start(cycle: u32, choice_count: usize) -> usize {
    (cycle as usize / choice_count.max(1)) % TREE_APPROACH_ANGLES.len()
}

fn collect_tree_work_candidates(
    terrain: &WorldTerrain,
    hut: Vec3,
) -> (Vec<shared::props::PropSpawn>, Vec<usize>) {
    let origin = ChunkCoord::from_world_pos(hut);
    let spawns: Vec<_> = origin
        .chunks_in_radius(2)
        .into_iter()
        .flat_map(|chunk| shared::props::generate_chunk_prop_spawns(&terrain.generator, chunk))
        .collect();
    let mut trees: Vec<usize> = spawns
        .iter()
        .enumerate()
        .filter(|(_, spawn)| spawn.kind.is_some_and(|kind| kind.is_tree()))
        .filter(|(_, spawn)| {
            let distance = Vec2::new(spawn.position.x - hut.x, spawn.position.z - hut.z).length();
            (TREE_MIN_DISTANCE..=TREE_MAX_DISTANCE).contains(&distance)
        })
        .map(|(index, _)| index)
        .collect();
    trees.sort_by(|a, b| {
        let a = spawns[*a].position;
        let b = spawns[*b].position;
        a.distance_squared(hut)
            .total_cmp(&b.distance_squared(hut))
            .then_with(|| a.x.total_cmp(&b.x))
            .then_with(|| a.z.total_cmp(&b.z))
    });
    (spawns, trees)
}

fn find_tree_in_candidates(
    terrain: &WorldTerrain,
    derived: Option<&DerivedColliderLibrary>,
    obstacles: Option<&SpatialObstacleGrid>,
    hut: Vec3,
    cycle: u32,
    salt: u32,
    spawns: &[shared::props::PropSpawn],
    trees: &[usize],
) -> Option<(Vec3, Vec3)> {
    // Keep a comfortably large deterministic candidate pool. Sparse coastal
    // groves may only contain two usable trees, while a dense forest should
    // not trap all workers on the same nearest twelve trunks.
    let choice_count = trees.len().min(48);
    if choice_count == 0 {
        return None;
    }
    let tree_index = trees[(cycle.wrapping_add(salt) as usize) % choice_count];
    let tree_spawn = &spawns[tree_index];
    let tree = tree_spawn.position;
    let toward_hut = Vec2::new(hut.x - tree.x, hut.z - tree.z).normalize_or(Vec2::Y);
    let tree_radius = tree_spawn
        .kind
        .and_then(|kind| derived.and_then(|library| library.by_kind.get(&kind)))
        .map_or(0.75, |shape| shape.horizontal_radius)
        * tree_spawn.scale;
    let stand_distance = (tree_radius + VILLAGER_PROP_RADIUS + 0.25).max(2.4);

    // The natural first choice faces back toward the workplace. If its route
    // fails, the next pass starts on a different side of the trunk instead of
    // returning the identical locally-clear but globally-unreachable goal.
    // The ordering remains deterministic for reproducible simulation runs.
    let approach_start = tree_approach_start(cycle, choice_count);
    for step in 0..TREE_APPROACH_ANGLES.len() {
        let angle = TREE_APPROACH_ANGLES[(approach_start + step) % TREE_APPROACH_ANGLES.len()];
        let (sin, cos) = angle.sin_cos();
        let direction = Vec2::new(
            toward_hut.x * cos - toward_hut.y * sin,
            toward_hut.x * sin + toward_hut.y * cos,
        );
        let stand_xz = Vec2::new(tree.x, tree.z) + direction * stand_distance;
        if obstacles.is_some_and(|grid| grid.point_blocked(stand_xz)) {
            continue;
        }
        let overlaps_prop = spawns.iter().enumerate().any(|(index, spawn)| {
            if index == tree_index {
                return false;
            }
            let Some(kind) = spawn.kind.filter(|kind| kind.blocks_village_road()) else {
                return false;
            };
            let radius = derived
                .and_then(|library| library.by_kind.get(&kind))
                .map_or(0.75, |shape| shape.horizontal_radius)
                * spawn.scale
                + VILLAGER_PROP_RADIUS;
            Vec2::new(spawn.position.x, spawn.position.z).distance_squared(stand_xz)
                < radius * radius
        });
        if overlaps_prop {
            continue;
        }
        let stand = Vec3::new(
            stand_xz.x,
            terrain.get_height(stand_xz.x, stand_xz.y),
            stand_xz.y,
        );
        return Some((tree, stand));
    }
    None
}

fn find_tree_for_cycle(
    terrain: &WorldTerrain,
    derived: Option<&DerivedColliderLibrary>,
    obstacles: Option<&SpatialObstacleGrid>,
    hut: Vec3,
    cycle: u32,
    salt: u32,
) -> Option<(Vec3, Vec3)> {
    let (spawns, trees) = collect_tree_work_candidates(terrain, hut);
    find_tree_in_candidates(
        terrain, derived, obstacles, hut, cycle, salt, &spawns, &trees,
    )
}

/// Permit-time proof that a lumber workplace has at least one usable tree on
/// its own walkable landmass. Resource density alone is insufficient on a
/// coast: a tree 30 metres away across a channel is not timber supply.
///
/// Candidate props are generated once, and only the nearest bounded set pays a
/// terrain-only route check. This runs when the plot decision changes, not per
/// villager or simulation tick.
pub(crate) fn lumber_plot_has_reachable_tree(terrain: &WorldTerrain, hut: Vec3) -> bool {
    const PERMIT_TREE_CANDIDATES: usize = 12;
    let (spawns, trees) = collect_tree_work_candidates(terrain, hut);
    let attempts = trees.len().min(PERMIT_TREE_CANDIDATES);
    (0..attempts).any(|cycle| {
        find_tree_in_candidates(terrain, None, None, hut, cycle as u32, 0, &spawns, &trees)
            .is_some_and(|(_, stand)| {
                crate::world::village_roads::embodied_land_route_exists(terrain, hut, stand)
            })
    })
}

fn ground_distance(a: Vec3, b: Vec3) -> f32 {
    Vec2::new(a.x - b.x, a.z - b.z).length()
}

fn farm_work_stand(
    field: Vec3,
    rotation: f32,
    worker_salt: u32,
    obstacles: Option<&SpatialObstacleGrid>,
    colliders: Option<&StaticColliders>,
    derived: Option<&DerivedColliderLibrary>,
) -> Option<Vec3> {
    let side = if worker_salt & 1 == 0 { -1.5 } else { 1.5 };
    let candidates = [
        Vec2::new(side, 0.0),
        Vec2::new(-side, 0.0),
        Vec2::new(0.0, 2.0),
        Vec2::new(0.0, -2.0),
        Vec2::new(side, 2.5),
        Vec2::new(-side, 2.5),
        Vec2::new(side, -2.5),
        Vec2::new(-side, -2.5),
    ];
    candidates.into_iter().find_map(|local| {
        let offset = shared::rotation::local_to_world_xz(local, rotation);
        let point = Vec2::new(field.x + offset.x, field.z + offset.y);
        if obstacles.is_some_and(|obstacles| obstacles.point_blocked(point)) {
            return None;
        }
        if colliders.zip(derived).is_some_and(|(colliders, derived)| {
            !crate::world::village_roads::navigation_point_is_clear_of_props(
                point, colliders, derived,
            )
        }) {
            return None;
        }
        Some(Vec3::new(point.x, field.y, point.y))
    })
}

fn build_clip_facing(to_work: Vec3) -> f32 {
    f32::atan2(-to_work.x, -to_work.z)
}

/// A collision-safe point beyond an authored threshold.
///
/// The door anchor sits only a few centimetres outside some inflated building
/// footprints. Stopping merely within `DOOR_REACH` of that anchor can therefore
/// leave an actor inside the blocker forever. Door-controlled movement may
/// cross the wall, so finish the crossing a short distance beyond the anchor
/// before ordinary navigation takes ownership again.
fn exterior_door_clearance_position(building: Vec3, door: Vec3) -> Vec3 {
    let outward = Vec2::new(door.x - building.x, door.z - building.z).normalize_or_zero();
    Vec3::new(
        door.x + outward.x * (DOOR_REACH + 0.35),
        door.y,
        door.z + outward.y * (DOOR_REACH + 0.35),
    )
}

fn time_warp_factor(warp: &Query<&shared::components::TimeWarp>) -> f32 {
    warp.iter()
        .next()
        .map(|time_warp| time_warp.0)
        .unwrap_or(1.0)
}

fn ordinary_workday(clock: &WorldTime) -> bool {
    clock.is_day() && clock.day_t() < WORKDAY_END_DAY_T
}

fn finish_farmer_shift(
    commands: &mut Commands,
    worker: Entity,
    day: u32,
    routine: &FarmerRoutine,
    activity: &mut CharacterActivity,
) {
    *activity = CharacterActivity::Idle;
    commands
        .entity(worker)
        .remove::<FarmerRoutine>()
        .remove::<WorkplaceDoorTransit>()
        .remove::<BuildingDoorUse>()
        .remove::<MoveTarget>()
        .remove::<TravelRoute>()
        .remove::<NavigationRoutePending>()
        .remove::<NavigationRouteFailed>()
        .insert((
            WorkerOffDuty { day },
            FarmerHarvestProgress {
                farmstead: routine.farmstead,
                field: routine.field,
                seconds: routine.harvest_seconds,
            },
        ));
}

fn finish_fishing_shift(
    commands: &mut Commands,
    worker: Entity,
    day: u32,
    routine: &FishingRoutine,
    activity: &mut CharacterActivity,
) {
    *activity = CharacterActivity::Idle;
    commands
        .entity(worker)
        .remove::<FishingRoutine>()
        .remove::<PierTraversal>()
        .remove::<WorkplaceDoorTransit>()
        .remove::<BuildingDoorUse>()
        .remove::<MoveTarget>()
        .remove::<TravelRoute>()
        .remove::<NavigationRoutePending>()
        .remove::<NavigationRouteFailed>()
        .insert((
            WorkerOffDuty { day },
            FishingWorkProgress {
                hut: routine.hut,
                pier: routine.pier,
                seconds: routine.catch_seconds,
            },
        ));
}

fn finish_lumberjack_shift(
    commands: &mut Commands,
    worker: Entity,
    day: u32,
    routine: &LumberjackRoutine,
    activity: &mut CharacterActivity,
) {
    *activity = CharacterActivity::Idle;
    commands
        .entity(worker)
        .remove::<LumberjackRoutine>()
        .remove::<WorkplaceDoorTransit>()
        .remove::<BuildingDoorUse>()
        .remove::<MoveTarget>()
        .remove::<TravelRoute>()
        .remove::<NavigationRoutePending>()
        .remove::<NavigationRouteFailed>()
        .insert((
            WorkerOffDuty { day },
            LumberjackWorkProgress {
                hut: routine.hut,
                cycle: routine.cycle,
                chop_seconds: routine.chop_seconds,
            },
        ));
}

fn stable_name_hash(name: &str) -> u32 {
    name.bytes().fold(0_u32, |hash, byte| {
        hash.wrapping_mul(33).wrapping_add(byte as u32)
    })
}

fn ensure_move_target(
    commands: &mut Commands,
    entity: Entity,
    current: Option<&MoveTarget>,
    expected: Vec3,
) {
    if current.is_none_or(|target| ground_distance(target.0, expected) > 0.05) {
        commands.entity(entity).insert(MoveTarget(expected));
    }
}

/// Steepness at a point, as a rise over the sampling distance.
fn slope_at(terrain: &WorldTerrain, x: f32, z: f32) -> f32 {
    const STEP: f32 = 3.0;
    let here = terrain.get_height(x, z);
    let dx = (terrain.get_height(x + STEP, z) - here).abs();
    let dz = (terrain.get_height(x, z + STEP) - here).abs();
    dx.max(dz) / STEP
}

/// Steepest ground a building will accept.
const MAX_BUILD_SLOPE: f32 = 0.30;

/// How far above the waterline anything a settlement builds must stand, in metres.
///
/// Not zero: ground exactly at the waterline is shoreline, and a farmstead with
/// its doorstep in the lake reads as a bug even though the maths permitted it.
pub const FREEBOARD: f32 = 1.5;

/// Find a dry Fisherman's Hut plot whose authored rear pier reaches genuine
/// open water.
///
/// This is intentionally geometry-led rather than biome-led. A northern rock
/// coast and a southern dry coast are both viable if there is a safe hut pad,
/// a dry route around the hut, and water beneath the working end of the pier.
/// The returned rotation points the hut's local +Z (its `Anchor_Pier` side)
/// seaward.
pub fn find_fishing_site(
    terrain: &WorldTerrain,
    hall: Vec3,
    occupied: &[(Vec3, f32)],
    roads: &[&VillageRoad],
) -> Option<(Vec3, f32, f32)> {
    const BEARINGS: usize = 24;
    const FACINGS: usize = 24;
    const RING_STEP: f32 = 4.0;

    let water = terrain.water_level()?;
    let kind = SettlementBuildingKind::FishermansHut;
    let (min_radius, max_radius) = kind.preferred_ring();
    let clearance = kind.clearance();
    let mut radius = min_radius;

    while radius <= max_radius {
        let mut best_at_radius: Option<(Vec3, f32, f32)> = None;
        for i in 0..BEARINGS {
            let turn = (i as f32 + (radius / RING_STEP) * 0.5) / BEARINGS as f32;
            let angle = turn * std::f32::consts::TAU;
            let x = hall.x + angle.cos() * radius;
            let z = hall.z + angle.sin() * radius;
            if slope_at(terrain, x, z) > MAX_BUILD_SLOPE {
                continue;
            }
            let ground = terrain.get_height(x, z);
            let candidate = Vec3::new(x, ground, z);
            if occupied.iter().any(|(other, other_clearance)| {
                Vec2::new(candidate.x - other.x, candidate.z - other.z).length()
                    < clearance + other_clearance
            }) {
                continue;
            }
            let footprint_radius = kind.art().definition().footprint.length() * 0.5 + 0.45;
            if roads.iter().any(|road| {
                road.contains_reserved_point(Vec2::new(candidate.x, candidate.z), footprint_radius)
            }) {
                continue;
            }

            for facing in 0..FACINGS {
                let rotation = facing as f32 / FACINGS as f32 * std::f32::consts::TAU;
                if shared::components::minimum_building_water_clearance(
                    terrain, candidate, kind, rotation,
                ) < shared::components::SETTLEMENT_FREEBOARD
                {
                    continue;
                }
                if !crate::world::village_roads::doorway_road_apron_is_dry(
                    terrain, kind, candidate, rotation,
                ) {
                    continue;
                }

                // The side-route anchor is where a fisher rounds the solid
                // building on the way from its front door to its rear pier.
                // It must remain dry and reasonably level with the hut pad.
                let Some(nets) = kind.nets_position(candidate, rotation) else {
                    continue;
                };
                let nets_ground = terrain.get_height(nets.x, nets.z);
                if nets_ground < water + 0.15 || (nets_ground - ground).abs() > 1.6 {
                    continue;
                }

                let Some(fish_spot) = kind.fishing_position(candidate, rotation) else {
                    continue;
                };
                let quality = fishing_water_quality(terrain, fish_spot, rotation, water);
                if quality <= 0.0 {
                    continue;
                }
                let stand = shared::components::builder_stand_position(
                    candidate,
                    rotation,
                    kind.art().definition().footprint.y,
                );
                if !crate::world::village_roads::embodied_land_route_exists(terrain, hall, stand) {
                    continue;
                }
                let replace = best_at_radius
                    .as_ref()
                    .is_none_or(|(_, _, best_quality)| quality > *best_quality);
                if replace {
                    best_at_radius = Some((candidate, rotation, quality));
                }
            }
        }
        if best_at_radius.is_some() {
            return best_at_radius;
        }
        radius += RING_STEP;
    }
    None
}

/// Score water around the working end of the authored pier. Zero means the
/// three seaward samples are not all submerged, so the layout would visibly
/// terminate on land. Non-zero values reward deeper, broader water without
/// making oceans categorically better than rivers or lakes.
fn fishing_water_quality(
    terrain: &WorldTerrain,
    fish_spot: Vec3,
    rotation: f32,
    water: f32,
) -> f32 {
    let mut depth_score = 0.0;
    let mut samples = 0.0;
    for forward in [-1.0_f32, 0.75, 2.5] {
        for side in [-1.4_f32, 0.0, 1.4] {
            let offset = shared::rotation::local_to_world_xz(Vec2::new(side, forward), rotation);
            let ground = terrain.get_height(fish_spot.x + offset.x, fish_spot.z + offset.y);
            let depth = water - ground;
            // The outer row is load-bearing: a pier whose tip merely touches a
            // shallow puddle is not a fishing site.
            if forward >= 2.5 && depth < 0.18 {
                return 0.0;
            }
            depth_score += (depth / 2.5).clamp(0.0, 1.0);
            samples += 1.0;
        }
    }
    // Require the actual authored standing point to be above water, too.
    let tip_depth = water - terrain.get_height(fish_spot.x, fish_spot.z);
    if tip_depth < 0.12 {
        return 0.0;
    }
    (0.35 + 0.65 * depth_score / samples).clamp(0.35, 1.0)
}

/// Deterministic ring search for somewhere to put a building.
///
/// Walks outward in rings from the hall, sampling a fixed number of bearings
/// per ring, and takes the first spot that is flat enough and clear of what is
/// already there. Deterministic on purpose: the same village in the same state
/// makes the same choice, so a bug is reproducible rather than a story about
/// what happened once.
///
/// This is emphatically NOT the settlement planner. It treats completed roads
/// as occupied infrastructure so later buildings cannot overwrite them, but it
/// has no frontage scoring, farmland quality or forest proximity; it gets
/// buildings onto sensible ground in roughly the right relationship to the
/// hall, and the real planner replaces it wholesale.
pub fn find_site(
    terrain: &WorldTerrain,
    hall: Vec3,
    kind: SettlementBuildingKind,
    occupied: &[(Vec3, f32)],
    roads: &[&VillageRoad],
) -> Option<(Vec3, f32)> {
    find_site_with_plan(terrain, hall, kind, occupied, roads, None, None, None)
}

#[derive(Clone, Copy, Debug)]
struct PlannedPlotCandidate {
    /// Building centre relative to the Moot Hall.
    local: Vec2,
    /// Point on the intended lane, street, avenue, or neighbourhood green that
    /// the building's door should face.
    frontage: Vec2,
}

fn plan_axis(plan: &shared::components::SettlementDevelopment) -> (Vec2, Vec2) {
    let fraction = (plan.plan_seed.rotate_right(29) & 0xffff) as f32 / 65_535.0;
    let angle = fraction * std::f32::consts::TAU;
    let axis = Vec2::new(angle.cos(), angle.sin());
    (axis, Vec2::new(-axis.y, axis.x))
}

fn plan_center_offset(
    plan: &shared::components::SettlementDevelopment,
    axis: Vec2,
    side: Vec2,
) -> Vec2 {
    use shared::components::SettlementCenterStyle as Center;
    let handedness = if plan.plan_seed.rotate_right(17) & 1 == 0 {
        1.0
    } else {
        -1.0
    };
    match plan.center {
        Center::Green => side * handedness * 8.0,
        Center::Square => Vec2::ZERO,
        Center::Avenue => axis * 6.0,
        Center::Courtyard => (axis + side * handedness) * 5.0,
    }
}

/// Convert one deterministic search sample into a recognisable piece of the
/// settlement's street grammar. Buildings are placed BESIDE an implied street
/// and face that street. The road builder later surveys from the authored door
/// to the existing network, turning this inexpensive plan into real terrain-
/// aware paths without pre-baking a whole city.
fn planned_plot_candidate(
    plan: &shared::components::SettlementDevelopment,
    kind: SettlementBuildingKind,
    radius: f32,
    bearing: usize,
    base: Vec2,
) -> PlannedPlotCandidate {
    use shared::components::SettlementLayoutStyle as Style;

    let (axis, side) = plan_axis(plan);
    let centre = plan_center_offset(plan, axis, side);
    let handedness = if plan.plan_seed & 1 == 0 { 1.0 } else { -1.0 };
    let side_sign = if bearing & 1 == 0 { 1.0 } else { -1.0 };

    match plan.layout {
        Style::Organic => {
            // Three gently wandering lanes. Houses occupy alternating verges
            // instead of forming a ring around the hall.
            let branch = bearing % 3;
            let branch_angle = (plan.plan_seed.rotate_right(7) & 0xffff) as f32 / 65_535.0
                * std::f32::consts::TAU
                + branch as f32 * std::f32::consts::TAU / 3.0
                + (radius * 0.085 + branch as f32).sin() * 0.18;
            let direction = Vec2::new(branch_angle.cos(), branch_angle.sin());
            let normal = Vec2::new(-direction.y, direction.x);
            let bend = normal
                * (radius * 0.11 + bearing as f32 + (plan.plan_seed & 31) as f32 * 0.03).sin()
                * 5.5;
            let frontage = centre + direction * radius + bend;
            let setback = 8.5 + (bearing / 6) as f32 * 3.0;
            PlannedPlotCandidate {
                local: frontage + normal * side_sign * setback,
                frontage,
            }
        }
        Style::Radial => {
            // Buildings front the sides of several spokes, not the Moot Hall.
            let branches = 5 + (plan.plan_seed.rotate_right(13) & 1) as usize;
            let branch = (bearing / 2) % branches;
            let angle =
                axis.y.atan2(axis.x) + branch as f32 * std::f32::consts::TAU / branches as f32;
            let direction = Vec2::new(angle.cos(), angle.sin());
            let normal = Vec2::new(-direction.y, direction.x);
            let frontage = centre + direction * radius;
            PlannedPlotCandidate {
                local: frontage + normal * side_sign * 9.0,
                frontage,
            }
        }
        Style::Grid => {
            // Seed-rotated orthogonal streets. Each sample is a building set
            // back from the nearest grid line with its facade parallel to it.
            const BLOCK: f32 = 26.0;
            const FRONTAGE_STEP: f32 = 11.0;
            const SETBACK: f32 = 9.0;
            let along = base.dot(axis);
            let across = base.dot(side);
            if bearing & 1 == 0 {
                let street_across = (across / BLOCK).round() * BLOCK;
                let frontage = centre
                    + axis * ((along / FRONTAGE_STEP).round() * FRONTAGE_STEP)
                    + side * street_across;
                let verge = if (across - street_across).abs() > 0.5 {
                    (across - street_across).signum()
                } else {
                    side_sign
                };
                PlannedPlotCandidate {
                    local: frontage + side * verge * SETBACK,
                    frontage,
                }
            } else {
                let street_along = (along / BLOCK).round() * BLOCK;
                let frontage = centre
                    + axis * street_along
                    + side * ((across / FRONTAGE_STEP).round() * FRONTAGE_STEP);
                let verge = if (along - street_along).abs() > 0.5 {
                    (along - street_along).signum()
                } else {
                    side_sign
                };
                PlannedPlotCandidate {
                    local: frontage + axis * verge * SETBACK,
                    frontage,
                }
            }
        }
        Style::Avenue => {
            // A long civic spine with buildings on both sides. Farther rings
            // extend the avenue rather than inflating another circle.
            let mut along = base.dot(axis) * 1.35;
            if along.abs() < 10.0 {
                along = side_sign * radius * 0.8;
            }
            let avenue = centre + axis * along;
            let verge = if base.dot(side).abs() > 0.5 {
                base.dot(side).signum()
            } else {
                side_sign * handedness
            };
            PlannedPlotCandidate {
                local: avenue + side * verge * (12.0 + (bearing / 8) as f32 * 5.0),
                frontage: avenue,
            }
        }
        Style::Polycentric => {
            // Three persistent neighbourhood centres. Workplaces sit in the
            // looser outer clusters while homes/civic buildings fill the near
            // neighbourhoods.
            let district = bearing % 3;
            let district_angle =
                axis.y.atan2(axis.x) + handedness * district as f32 * std::f32::consts::TAU / 3.0;
            let district_direction = Vec2::new(district_angle.cos(), district_angle.sin());
            let district_distance = match kind {
                SettlementBuildingKind::Farmstead
                | SettlementBuildingKind::LumberjackHut
                | SettlementBuildingKind::FishermansHut => 44.0,
                _ => 30.0,
            };
            let district_centre = centre + district_direction * district_distance;
            let orbit_angle = district_angle
                + std::f32::consts::FRAC_PI_2
                + (bearing / 3) as f32 * 0.75
                + radius * 0.035;
            let orbit = Vec2::new(orbit_angle.cos(), orbit_angle.sin())
                * (9.0 + ((radius / 6.0) as usize & 1) as f32 * 5.0);
            PlannedPlotCandidate {
                local: district_centre + orbit,
                frontage: district_centre,
            }
        }
    }
}

fn rotation_facing_frontage(building: Vec2, frontage: Vec2) -> f32 {
    // Building doors are authored on local -Z. Rotating local -Z toward the
    // target requires the vector FROM the target back to the building.
    let outward = building - frontage;
    outward.x.atan2(outward.y)
}

fn closest_point_on_segment(point: Vec2, start: Vec2, end: Vec2) -> Vec2 {
    let segment = end - start;
    let length_squared = segment.length_squared();
    if length_squared <= 1e-6 {
        return start;
    }
    let t = ((point - start).dot(segment) / length_squared).clamp(0.0, 1.0);
    start + segment * t
}

fn nearest_completed_road_frontage(candidate: Vec2, roads: &[&VillageRoad]) -> Option<Vec2> {
    roads
        .iter()
        .filter(|road| road.is_complete())
        .flat_map(|road| road.built_points().windows(2))
        .map(|pair| closest_point_on_segment(candidate, pair[0], pair[1]))
        .min_by(|a, b| {
            a.distance_squared(candidate)
                .total_cmp(&b.distance_squared(candidate))
        })
        .filter(|frontage| frontage.distance_squared(candidate) <= 48.0 * 48.0)
}

fn find_site_with_plan(
    terrain: &WorldTerrain,
    hall: Vec3,
    kind: SettlementBuildingKind,
    occupied: &[(Vec3, f32)],
    roads: &[&VillageRoad],
    development: Option<&shared::components::SettlementDevelopment>,
    colliders: Option<&StaticColliders>,
    derived: Option<&DerivedColliderLibrary>,
) -> Option<(Vec3, f32)> {
    const BEARINGS: usize = 12;
    const RING_STEP: f32 = 6.0;
    const RESOURCE_PLOT_SHORTLIST: usize = 6;

    let (mut min_radius, max_radius) = kind.preferred_ring();
    if let Some(plan) = development {
        use shared::components::SettlementCenterStyle as Center;
        if kind == SettlementBuildingKind::House {
            // Preserve the selected civic centre from the first cabin onward.
            min_radius = min_radius.max(match plan.center {
                Center::Green => 24.0,
                Center::Square => 27.0,
                Center::Avenue => 19.0,
                Center::Courtyard => 26.0,
            });
        }
    }
    let clearance = kind.clearance();
    let resource_scored = matches!(
        kind,
        SettlementBuildingKind::Farmstead | SettlementBuildingKind::LumberjackHut
    );
    let mut best_resource_plots: Vec<(f32, Vec3, f32)> = Vec::new();

    let mut radius = min_radius;
    while radius <= max_radius {
        'candidate: for i in 0..BEARINGS {
            // Offset each ring's bearings so successive rings do not line every
            // building up on the same spokes.
            let seeded_turn = development.map_or(0.0, |plan| {
                ((plan.plan_seed.rotate_right(9) & 0xffff) as f32 / 65_535.0) * 0.92
            });
            let turn = (i as f32 + (radius / RING_STEP) * 0.5 + seeded_turn) / BEARINGS as f32;
            let angle = turn * std::f32::consts::TAU;
            let base = Vec2::new(angle.cos(), angle.sin()) * radius;
            let planned = development.map_or(
                PlannedPlotCandidate {
                    local: base,
                    frontage: Vec2::ZERO,
                },
                |plan| planned_plot_candidate(plan, kind, radius, i, base),
            );
            let local = planned.local;
            let x = hall.x + local.x;
            let z = hall.z + local.y;

            if slope_at(terrain, x, z) > MAX_BUILD_SLOPE {
                continue;
            }
            let ground = terrain.get_height(x, z);
            let candidate = Vec3::new(x, ground, z);
            // Prefer real completed frontage once a street exists. Before
            // that, face the implied street from the seed grammar. This is the
            // rotation used by water, field, collision, door, and road checks.
            let candidate2 = Vec2::new(x, z);
            let planned_frontage =
                Vec2::new(hall.x + planned.frontage.x, hall.z + planned.frontage.y);
            let frontage =
                nearest_completed_road_frontage(candidate2, roads).unwrap_or(planned_frontage);
            let rotation = rotation_facing_frontage(candidate2, frontage);
            // Test every part of the rotated footprint and the authored door
            // against the LOCAL water surface. Comparing the centre to the
            // global ocean plane misses inland rivers entirely.
            if shared::components::minimum_building_water_clearance(
                terrain, candidate, kind, rotation,
            ) < FREEBOARD
            {
                continue;
            }
            if !crate::world::village_roads::doorway_road_apron_is_dry(
                terrain, kind, candidate, rotation,
            ) {
                continue;
            }
            if colliders.zip(derived).is_some_and(|(colliders, derived)| {
                !crate::world::village_roads::doorway_road_apron_is_clear_of_props(
                    kind, candidate, rotation, colliders, derived,
                )
            }) {
                continue;
            }
            let clashes = occupied.iter().any(|(other, other_clearance)| {
                let flat = Vec2::new(candidate.x - other.x, candidate.z - other.z).length();
                flat < clearance + other_clearance
            });
            if clashes {
                continue;
            }
            if let (Some(field_positions), Some(field_half)) = (
                kind.field_positions(candidate, rotation),
                kind.field_half_extents(),
            ) {
                for field in field_positions {
                    if shared::components::minimum_rotated_rect_water_clearance(
                        terrain,
                        field,
                        field_half + Vec2::splat(shared::components::FARM_FIELD_EDGE_CLEARANCE),
                        rotation,
                    ) < FREEBOARD
                    {
                        continue 'candidate;
                    }
                    if colliders.zip(derived).is_some_and(|(colliders, derived)| {
                        !crate::world::village_roads::rotated_rect_is_clear_of_props(
                            Vec2::new(field.x, field.z),
                            field_half,
                            rotation,
                            shared::components::FARM_FIELD_EDGE_CLEARANCE,
                            colliders,
                            derived,
                        )
                    }) {
                        continue 'candidate;
                    }
                    let field_clearance =
                        field_half.length() + shared::components::FARM_FIELD_EDGE_CLEARANCE;
                    if occupied.iter().any(|(other, other_clearance)| {
                        Vec2::new(field.x - other.x, field.z - other.z).length()
                            < field_clearance + other_clearance
                    }) {
                        continue 'candidate;
                    }
                }
            }
            let footprint_radius = kind.art().definition().footprint.length() * 0.5 + 0.45;
            if roads.iter().any(|road| {
                road.contains_reserved_point(Vec2::new(candidate.x, candidate.z), footprint_radius)
            }) {
                continue;
            }
            if let (Some(field_positions), Some(field_half)) = (
                kind.field_positions(candidate, rotation),
                kind.field_half_extents(),
            ) {
                for field in field_positions {
                    let field_center = Vec2::new(field.x, field.z);
                    if roads.iter().any(|road| {
                        road.intersects_rotated_rect(
                            field_center,
                            field_half,
                            rotation,
                            shared::components::FARM_FIELD_EDGE_CLEARANCE,
                        )
                    }) {
                        continue 'candidate;
                    }
                }
            }
            if resource_scored {
                // A layout grammar says which street this plot belongs to;
                // geography still decides whether a farm or timber workplace
                // is worth building there. A small travel penalty prevents a
                // negligible quality gain from sending the very first worker
                // to the edge of the full 120 m search band.
                let quality = site_quality(terrain, kind, candidate);
                let travel = candidate2.distance(Vec2::new(hall.x, hall.z));
                let score = quality * 100.0 - travel / max_radius.max(1.0) * 9.0;
                best_resource_plots.push((score, candidate, rotation));
                best_resource_plots.sort_by(|a, b| {
                    b.0.total_cmp(&a.0)
                        .then_with(|| a.1.x.total_cmp(&b.1.x))
                        .then_with(|| a.1.z.total_cmp(&b.1.z))
                });
                best_resource_plots.truncate(RESOURCE_PLOT_SHORTLIST);
            } else {
                return Some((candidate, rotation));
            }
        }
        radius += RING_STEP;
    }
    best_resource_plots
        .into_iter()
        .find(|(_, candidate, rotation)| {
            let builder_stand = shared::components::builder_stand_position(
                *candidate,
                *rotation,
                kind.art().definition().footprint.y,
            );
            if !crate::world::village_roads::embodied_land_route_exists(
                terrain,
                hall,
                builder_stand,
            ) {
                return false;
            }
            kind != SettlementBuildingKind::LumberjackHut
                || lumber_plot_has_reachable_tree(
                    terrain,
                    kind.entrance_position(*candidate, *rotation),
                )
        })
        .map(|(_, candidate, rotation)| (candidate, rotation))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_quality_controls_continuous_wheat_rate() {
        assert!((farmer_seconds_per_wheat(1.0) - 170.0).abs() < 0.01);
        assert!((farmer_seconds_per_wheat(2.0 / 3.0) - 255.0).abs() < 0.01);
        assert!((farmer_seconds_per_wheat(0.5) - 340.0).abs() < 0.01);
        assert!(farmer_seconds_per_wheat(0.1) > farmer_seconds_per_wheat(0.5));

        let ordinary_shift_seconds = WorldTime::DEFAULT_DAY_DURATION * WORKDAY_END_DAY_T
            - WorldTime::DEFAULT_START_SECONDS_IN_DAY;
        assert!((ordinary_shift_seconds / farmer_seconds_per_wheat(1.0) - 6.0).abs() < 0.2);
        assert!((ordinary_shift_seconds / farmer_seconds_per_wheat(2.0 / 3.0) - 4.0).abs() < 0.2);
    }

    #[test]
    fn adaptive_wages_raise_for_vacancies_and_cut_under_payroll_stress() {
        let mut policy = BusinessWagePolicy::default();
        review_automatic_wage_offer(&mut policy, 2, 0, 2, 4 * PENNIES_PER_COIN, 0);
        assert_eq!(
            policy.daily_wage,
            FOUNDING_DAILY_WAGE + BUSINESS_WAGE_REVIEW_STEP
        );

        review_automatic_wage_offer(&mut policy, 2, 2, 2, 0, PENNIES_PER_COIN);
        assert_eq!(policy.daily_wage, FOUNDING_DAILY_WAGE);

        policy.automatic = false;
        review_automatic_wage_offer(&mut policy, 100, 0, 2, 10_000, 0);
        assert_eq!(policy.daily_wage, FOUNDING_DAILY_WAGE);
    }

    #[test]
    fn higher_wage_and_skill_requirements_shape_recruitment() {
        let mut app = App::new();
        app.add_systems(Update, fill_vacancies);
        let settlement = app
            .world_mut()
            .spawn(Settlement {
                name: "Skillford".to_string(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 2,
                treasury: 0,
            })
            .id();
        let specialist = app
            .world_mut()
            .spawn((
                SettlementBuilding {
                    kind: SettlementBuildingKind::Farmstead,
                    settlement: "Skillford".to_string(),
                    owner: None,
                    quality: 0.8,
                    workers: Vec::new(),
                },
                PlayerPosition(Vec3::new(20.0, 0.0, 0.0)),
                BusinessWagePolicy {
                    daily_wage: 2 * PENNIES_PER_COIN,
                    automatic: false,
                    ..default()
                },
                WorkforceRequirements {
                    minimum_physique: 50,
                    ..default()
                },
            ))
            .id();
        let ordinary = app
            .world_mut()
            .spawn((
                SettlementBuilding {
                    kind: SettlementBuildingKind::LumberjackHut,
                    settlement: "Skillford".to_string(),
                    owner: None,
                    quality: 0.8,
                    workers: Vec::new(),
                },
                PlayerPosition(Vec3::new(10.0, 0.0, 0.0)),
                BusinessWagePolicy::default(),
            ))
            .id();
        for (name, physique, position) in [
            ("Untrained", 20, Vec3::new(19.0, 0.0, 0.0)),
            ("Skilled", 60, Vec3::new(0.0, 0.0, 0.0)),
        ] {
            app.world_mut().spawn((
                CharacterName(name.to_string()),
                VillagerIntent::Resident { settlement },
                PlayerPosition(position),
                Occupation::default(),
                WorkStatus::LookingForWork,
                CharacterAttributes::new(physique, 10, 10),
            ));
        }

        app.update();
        assert_eq!(
            app.world()
                .get::<SettlementBuilding>(specialist)
                .unwrap()
                .workers,
            ["Skilled"]
        );
        assert_eq!(
            app.world()
                .get::<SettlementBuilding>(ordinary)
                .unwrap()
                .workers,
            ["Untrained"]
        );
    }

    #[test]
    fn payroll_catches_up_arrears_instead_of_stranding_them() {
        let mut app = App::new();
        app.add_systems(Update, run_business_payroll_and_owner_leisure);
        let mut clock = WorldTime::new_default();
        clock.day = 2;
        app.world_mut().spawn(clock);
        let settlement = app
            .world_mut()
            .spawn(Settlement {
                name: "Payford".to_string(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 1,
                treasury: 0,
            })
            .id();
        let business = app
            .world_mut()
            .spawn((
                SettlementBuilding {
                    kind: SettlementBuildingKind::Farmstead,
                    settlement: "Payford".to_string(),
                    owner: None,
                    quality: 0.8,
                    workers: vec!["Ada".to_string()],
                },
                BusinessAccount {
                    cash: 3 * PENNIES_PER_COIN,
                    wage_arrears: 2 * PENNIES_PER_COIN,
                    last_payroll_day: 1,
                },
                BusinessWagePolicy::default(),
            ))
            .id();
        let worker = app
            .world_mut()
            .spawn((
                CharacterName("Ada".to_string()),
                VillagerIntent::Resident { settlement },
                Wallet::default(),
                Occupation(Some("Farmer".to_string())),
                WorkStatus::Employed,
            ))
            .id();

        app.update();
        assert_eq!(app.world().get::<Wallet>(worker).unwrap().balance(), 300);
        let account = app.world().get::<BusinessAccount>(business).unwrap();
        assert_eq!(account.cash, 0);
        assert_eq!(account.wage_arrears, 0);
    }

    #[test]
    fn transient_actor_requests_are_aggregated_on_the_stable_building() {
        let mut app = App::new();
        app.add_systems(Update, sync_building_door_demands);
        let position = Vec3::new(12.0, 3.0, -8.0);
        let hall = app
            .world_mut()
            .spawn((
                Settlement {
                    name: "Doorford".to_string(),
                    tier: shared::components::SettlementTier::Hamlet,
                    residents: 1,
                    treasury: 0,
                },
                PlayerPosition(position),
            ))
            .id();
        let visitor = app
            .world_mut()
            .spawn(BuildingDoorUse { building: position })
            .id();

        app.update();
        assert!(
            app.world()
                .entity(hall)
                .get::<BuildingDoorDemand>()
                .unwrap()
                .open
        );

        app.world_mut()
            .entity_mut(visitor)
            .remove::<BuildingDoorUse>();
        app.update();
        assert!(
            !app.world()
                .entity(hall)
                .get::<BuildingDoorDemand>()
                .unwrap()
                .open
        );
    }

    #[test]
    fn later_buildings_do_not_overwrite_completed_village_paths() {
        let terrain = WorldTerrain::default();
        let hall = Vec3::new(1720.0, terrain.get_height(1720.0, 0.0), 0.0);
        let kind = SettlementBuildingKind::House;
        let (first, _) = find_site(&terrain, hall, kind, &[], &[]).unwrap();
        let road = VillageRoad {
            settlement: "Oakmead".into(),
            builder: "Mara".into(),
            points: vec![
                Vec2::new(first.x - 12.0, first.z),
                Vec2::new(first.x + 12.0, first.z),
            ],
            built_through: 2,
            width: crate::world::village_roads::VILLAGE_ROAD_WIDTH,
            reserved_width: shared::components::RoadClass::Lane.initial_reserved_width(),
            surface: default(),
            class: default(),
            stone_committed: 0,
        };

        let (replacement, _) = find_site(&terrain, hall, kind, &[], &[&road]).unwrap();
        let footprint_radius = kind.art().definition().footprint.length() * 0.5 + 0.45;
        assert!(first.distance_squared(replacement) > 1.0);
        assert!(!road
            .contains_reserved_point(Vec2::new(replacement.x, replacement.z), footprint_radius,));
    }

    #[test]
    fn seeded_layouts_face_streets_and_produce_distinct_first_plots() {
        use shared::components::{
            SettlementCenterStyle, SettlementDevelopment, SettlementLayoutStyle,
        };

        let terrain = WorldTerrain::default();
        let hall = Vec3::new(1720.0, terrain.get_height(1720.0, 0.0), 0.0);
        let kind = SettlementBuildingKind::House;
        let styles = [
            SettlementLayoutStyle::Organic,
            SettlementLayoutStyle::Radial,
            SettlementLayoutStyle::Grid,
            SettlementLayoutStyle::Avenue,
            SettlementLayoutStyle::Polycentric,
        ];
        let mut first_plots = Vec::new();

        for style in styles {
            let mut plan = SettlementDevelopment::from_foundation("Planford", hall, 0);
            plan.layout = style;
            plan.center = SettlementCenterStyle::Green;
            let mut occupied = vec![(hall, SettlementBuildingKind::Hall.clearance())];
            let mut hall_facing = 0;

            for index in 0..4 {
                let (plot, rotation) = find_site_with_plan(
                    &terrain,
                    hall,
                    kind,
                    &occupied,
                    &[],
                    Some(&plan),
                    None,
                    None,
                )
                .unwrap_or_else(|| panic!("{style:?} must find plot {index}"));
                if index == 0 {
                    first_plots.push(Vec2::new(plot.x, plot.z));
                }
                let door = kind.entrance_position(plot, rotation);
                let door_direction = Vec2::new(door.x - plot.x, door.z - plot.z).normalize();
                let hall_direction = Vec2::new(hall.x - plot.x, hall.z - plot.z).normalize();
                if door_direction.dot(hall_direction) > 0.985 {
                    hall_facing += 1;
                }
                occupied.push((plot, kind.clearance()));
            }

            assert!(
                hall_facing < 4,
                "{style:?} must use street frontage instead of making every door face the hall"
            );
        }

        let mut distinct = 0;
        for (index, plot) in first_plots.iter().enumerate() {
            if first_plots[..index]
                .iter()
                .all(|other| other.distance_squared(*plot) > 4.0)
            {
                distinct += 1;
            }
        }
        assert!(
            distinct >= 4,
            "the five layout grammars must not collapse to the same first plot: {first_plots:?}"
        );
    }

    #[test]
    fn unseeded_fallback_points_the_authored_door_toward_the_hall() {
        let terrain = WorldTerrain::default();
        let hall = Vec3::new(1720.0, terrain.get_height(1720.0, 0.0), 0.0);
        let kind = SettlementBuildingKind::House;
        let (plot, rotation) = find_site(&terrain, hall, kind, &[], &[]).unwrap();
        let door = kind.entrance_position(plot, rotation);
        let door_direction = Vec2::new(door.x - plot.x, door.z - plot.z).normalize();
        let hall_direction = Vec2::new(hall.x - plot.x, hall.z - plot.z).normalize();
        assert!(
            door_direction.dot(hall_direction) > 0.999,
            "the old sign pointed the authored -Z door away from the Moot Hall"
        );
    }

    #[test]
    fn farmstead_siting_reserves_its_future_wheat_field_from_roads() {
        let terrain = WorldTerrain::default();
        let hall = Vec3::new(1720.0, terrain.get_height(1720.0, 0.0), 0.0);
        let kind = SettlementBuildingKind::Farmstead;
        let (first, first_rotation) = find_site(&terrain, hall, kind, &[], &[]).unwrap();
        let first_field = kind.field_position(first, first_rotation).unwrap();
        let road = VillageRoad {
            settlement: "Oakmead".into(),
            builder: "Mara".into(),
            points: vec![
                Vec2::new(first_field.x - 12.0, first_field.z),
                Vec2::new(first_field.x + 12.0, first_field.z),
            ],
            // Even an unbuilt plan is committed ground and must be reserved.
            built_through: 1,
            width: crate::world::village_roads::VILLAGE_ROAD_WIDTH,
            reserved_width: shared::components::RoadClass::Lane.initial_reserved_width(),
            surface: default(),
            class: default(),
            stone_committed: 0,
        };

        let (replacement, replacement_rotation) =
            find_site(&terrain, hall, kind, &[], &[&road]).unwrap();
        assert!(first.distance_squared(replacement) > 1.0);
        for replacement_field in kind
            .field_positions(replacement, replacement_rotation)
            .unwrap()
        {
            assert!(!road.intersects_rotated_rect(
                Vec2::new(replacement_field.x, replacement_field.z),
                kind.field_half_extents().unwrap(),
                replacement_rotation,
                shared::components::FARM_FIELD_EDGE_CLEARANCE,
            ));
        }
    }

    #[test]
    fn an_inland_river_cannot_be_mistaken_for_a_dry_building_plot() {
        let terrain = WorldTerrain::default();
        let ocean = terrain.water_level().expect("generated world has water");
        let river_point = terrain
            .rivers()
            .iter()
            .flatten()
            .find(|point| {
                terrain
                    .water_surface_height(point.x, point.z)
                    .is_some_and(|surface| {
                        surface > ocean + 0.2 && terrain.get_height(point.x, point.z) < surface
                    })
            })
            .expect("generated world has an inland river");
        let centre = Vec3::new(
            river_point.x,
            terrain.get_height(river_point.x, river_point.z),
            river_point.z,
        );

        assert!(
            shared::components::minimum_building_water_clearance(
                &terrain,
                centre,
                SettlementBuildingKind::Farmstead,
                0.0,
            ) < FREEBOARD
        );
    }

    #[test]
    fn builder_rendered_front_faces_the_building() {
        let toward_building = Vec3::new(4.0, 0.0, 3.0).normalize();
        let yaw = build_clip_facing(toward_building);
        let rendered_front = Quat::from_rotation_y(yaw) * Vec3::NEG_Z;

        assert!(
            rendered_front.dot(toward_building) > 0.999,
            "the character asset's local -Z front must face the work"
        );
    }

    #[test]
    fn twelve_failed_tree_routes_widen_the_search_instead_of_stopping_work() {
        let mut cycle = 7;
        let mut failed = 0;
        for _ in 0..11 {
            let (next_cycle, next_failed, widened) = advance_failed_tree_candidate(cycle, failed);
            cycle = next_cycle;
            failed = next_failed;
            assert!(!widened);
        }
        assert_eq!(failed, 11);
        let before_widening = cycle;
        let (next_cycle, next_failed, widened) = advance_failed_tree_candidate(cycle, failed);
        cycle = next_cycle;
        failed = next_failed;
        assert!(widened);
        assert_eq!(failed, 0);
        assert_eq!(cycle, before_widening.wrapping_add(13));

        let (_cycle, failed, widened) = advance_failed_tree_candidate(cycle, failed);
        assert!(!widened);
        assert_eq!(failed, 1, "the routine must remain live after widening");
    }

    #[test]
    fn sparse_grove_retries_rotate_around_each_tree() {
        let choice_count = 2;
        let starts: Vec<_> = (0..16)
            .map(|cycle| tree_approach_start(cycle, choice_count))
            .collect();

        assert_eq!(&starts[..4], &[0, 0, 1, 1]);
        assert_eq!(
            starts
                .into_iter()
                .collect::<std::collections::BTreeSet<_>>(),
            (0..TREE_APPROACH_ANGLES.len()).collect(),
            "two sparse trees must eventually be tried from all eight sides"
        );
    }

    #[test]
    fn settlement_seek_targets_the_authored_hall_entrance() {
        let mut app = App::new();
        app.init_resource::<Time>();
        app.init_resource::<VillageClock>();
        app.add_systems(Update, seek_settlement);

        let hall_position = Vec3::new(120.0, 8.0, -40.0);
        let hall_rotation = 0.73;
        app.world_mut().spawn((
            Settlement {
                name: "Doorstead".to_string(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 0,
                treasury: 0,
            },
            PlayerPosition(hall_position),
            PlayerRotation(hall_rotation),
        ));
        let villager = app
            .world_mut()
            .spawn((
                PlayerPosition(hall_position + Vec3::X * 40.0),
                VillagerIntent::Idle,
            ))
            .id();

        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs_f32(SEEK_INTERVAL + 0.1));
        app.update();

        let target = app.world().get::<MoveTarget>(villager).unwrap().0;
        let expected = SettlementBuildingKind::Hall.entrance_position(hall_position, hall_rotation);
        assert!(
            target.distance(expected) < 0.01,
            "migration must target the reachable authored door, not the solid hall centre: {target:?}"
        );
        assert!(target.distance(hall_position) > 1.0);
    }

    #[test]
    fn migration_repairs_an_obsolete_hall_centre_target() {
        let mut app = App::new();
        app.add_systems(Update, arrive_at_settlement);

        let hall_position = Vec3::new(120.0, 8.0, -40.0);
        let hall_rotation = 0.73;
        let settlement = app
            .world_mut()
            .spawn((
                Settlement {
                    name: "Doorstead".to_string(),
                    tier: shared::components::SettlementTier::Hamlet,
                    residents: 0,
                    treasury: 0,
                },
                PlayerPosition(hall_position),
                PlayerRotation(hall_rotation),
            ))
            .id();
        let villager = app
            .world_mut()
            .spawn((
                PlayerPosition(hall_position + Vec3::X * 40.0),
                VillagerIntent::Travelling { settlement },
                MoveTarget(hall_position),
            ))
            .id();

        app.update();

        let target = app.world().get::<MoveTarget>(villager).unwrap().0;
        let expected = SettlementBuildingKind::Hall.entrance_position(hall_position, hall_rotation);
        assert!(
            target.distance(expected) < 0.01,
            "a live villager stranded by the old centre target must be woken and redirected"
        );
    }

    #[test]
    fn thirty_then_thirty_immigrants_recover_across_day_two_and_warp_changes() {
        use shared::components::TimeWarp;

        let mut app = App::new();
        app.init_resource::<Time>();
        app.init_resource::<VillageClock>();
        app.add_systems(
            Update,
            (seek_settlement, arrive_at_settlement, recount_residents).chain(),
        );

        let first_hall_position = Vec3::ZERO;
        let second_hall_position = Vec3::new(100.0, 0.0, 0.0);
        let first_hall = app
            .world_mut()
            .spawn((
                Settlement {
                    name: "Nearford".into(),
                    tier: shared::components::SettlementTier::Hamlet,
                    residents: 0,
                    treasury: 0,
                },
                PlayerPosition(first_hall_position),
                PlayerRotation(0.0),
            ))
            .id();
        let second_hall = app
            .world_mut()
            .spawn((
                Settlement {
                    name: "Farford".into(),
                    tier: shared::components::SettlementTier::Hamlet,
                    residents: 0,
                    treasury: 0,
                },
                PlayerPosition(second_hall_position),
                PlayerRotation(0.0),
            ))
            .id();
        let clock = app
            .world_mut()
            .spawn((WorldTime::new_default(), TimeWarp::clamped(100.0)))
            .id();
        let failed_goal = SettlementBuildingKind::Hall.entrance_position(first_hall_position, 0.0);

        let mut first_wave = Vec::new();
        for index in 0..30 {
            let position = Vec3::new(20.0, 0.0, index as f32 * 0.05);
            first_wave.push(
                app.world_mut()
                    .spawn((
                        PlayerPosition(position),
                        VillagerIntent::Travelling {
                            settlement: first_hall,
                        },
                        MoveTarget(failed_goal),
                        NavigationRouteFailed { goal: failed_goal },
                    ))
                    .id(),
            );
        }

        // The failed cohort must return to decision-making, not remain counted
        // as embodied-but-not-resident travellers forever.
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs_f32(3.1));
        app.update();
        assert!(first_wave.iter().all(|entity| matches!(
            app.world().get::<VillagerIntent>(*entity),
            Some(VillagerIntent::Idle)
        )));

        // At 10x the next seek tick excludes Nearford only for these people,
        // so they choose the other viable town. Warp changes alter wall time,
        // never the state transition.
        app.world_mut().get_mut::<TimeWarp>(clock).unwrap().0 = 10.0;
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs_f32(3.1));
        app.update();
        assert!(first_wave.iter().all(|entity| matches!(
            app.world().get::<VillagerIntent>(*entity),
            Some(VillagerIntent::Travelling { settlement }) if *settlement == second_hall
        )));
        for entity in &first_wave {
            app.world_mut()
                .get_mut::<PlayerPosition>(*entity)
                .unwrap()
                .0 = second_hall_position;
        }
        app.update();

        // Day two receives a fresh cohort. Their choices are independent of
        // the first cohort's cooldown, so they can join the nearer town.
        app.world_mut().get_mut::<WorldTime>(clock).unwrap().day = 2;
        app.world_mut().get_mut::<TimeWarp>(clock).unwrap().0 = 100.0;
        for index in 0..30 {
            let position = first_hall_position + Vec3::new(0.0, 0.0, index as f32 * 0.01);
            app.world_mut()
                .spawn((PlayerPosition(position), VillagerIntent::Idle));
        }
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs_f32(3.1));
        app.update();

        assert_eq!(
            app.world().get::<Settlement>(first_hall).unwrap().residents,
            30
        );
        assert_eq!(
            app.world()
                .get::<Settlement>(second_hall)
                .unwrap()
                .residents,
            30
        );
        let (resident_intents, failed_routes) = {
            let world = app.world_mut();
            let resident_intents = world
                .query::<&VillagerIntent>()
                .iter(world)
                .filter(|intent| intent.counts_as_resident())
                .count();
            let failed_routes = world.query::<&NavigationRouteFailed>().iter(world).count();
            (resident_intents, failed_routes)
        };
        assert_eq!(resident_intents, 60);
        assert_eq!(failed_routes, 0);
    }

    #[test]
    fn one_hundred_twenty_real_routes_join_without_queue_starvation_at_100x() {
        use crate::player::hero::step_units;
        use crate::world::pathfinding::PathfindingBudgetSettings;
        use shared::components::{CharacterKind, TimeWarp};
        use shared::region::RegionCoord;

        let mut app = App::new();
        app.init_resource::<Time>();
        app.init_resource::<VillageClock>();
        app.init_resource::<crate::world::village_roads::VillageRoadGraph>();
        app.insert_resource(PathfindingBudgetSettings {
            max_requests_per_tick: 16,
            ..default()
        });
        app.insert_resource(WorldTerrain::default());
        app.add_systems(
            Update,
            (
                claim_settlement_hall_obstacles,
                tag_villager_intent,
                seek_settlement,
                arrive_at_settlement,
                recount_residents,
                crate::world::village_roads::rebuild_village_road_graph,
                crate::world::village_roads::queue_villager_travel_routes,
                crate::world::village_roads::plan_villager_travel_routes,
                step_units,
            )
                .chain(),
        );

        let hall_y = app
            .world()
            .resource::<WorldTerrain>()
            .get_height(1_700.0, 0.0);
        let hall_position = Vec3::new(1_700.0, hall_y, 0.0);
        let hall = app
            .world_mut()
            .spawn((
                Settlement {
                    name: "Crowdford".into(),
                    tier: shared::components::SettlementTier::Hamlet,
                    residents: 0,
                    treasury: 0,
                },
                PlayerPosition(hall_position),
                PlayerRotation(0.0),
            ))
            .id();
        app.world_mut()
            .spawn((WorldTime::new_default(), TimeWarp::clamped(100.0)));
        for index in 0..120 {
            let x = 1_738.0 + (index % 6) as f32 * 0.35;
            let z = (index / 6) as f32 * 0.08 - 0.8;
            let y = app.world().resource::<WorldTerrain>().get_height(x, z);
            let position = Vec3::new(x, y, z);
            app.world_mut().spawn((
                CharacterName(format!("CrowdImmigrant{index:03}")),
                CharacterKind::Villager,
                CharacterActivity::Idle,
                PlayerPosition(position),
                PlayerRotation(0.0),
                RegionCoord::from_world_pos(position),
            ));
        }

        let tick = std::time::Duration::from_secs_f32(1.0 / 60.0);
        let started = std::time::Instant::now();
        for _ in 0..180 {
            app.world_mut().resource_mut::<Time>().advance_by(tick);
            app.update();
        }
        let elapsed = started.elapsed();

        let counted_residents = app.world().get::<Settlement>(hall).unwrap().residents;
        let (idle, travelling, residents, pending, failed) = {
            let world = app.world_mut();
            let mut totals = (0, 0, 0, 0, 0);
            for (intent, route_pending, route_failed) in world
                .query::<(
                    &VillagerIntent,
                    Has<NavigationRoutePending>,
                    Has<NavigationRouteFailed>,
                )>()
                .iter(world)
            {
                totals.0 += usize::from(matches!(intent, VillagerIntent::Idle));
                totals.1 += usize::from(matches!(intent, VillagerIntent::Travelling { .. }));
                totals.2 += usize::from(intent.counts_as_resident());
                totals.3 += usize::from(route_pending);
                totals.4 += usize::from(route_failed);
            }
            totals
        };
        assert_eq!(
            (
                counted_residents,
                idle,
                travelling,
                residents,
                pending,
                failed
            ),
            (120, 0, 0, 120, 0, 0),
            "120-person migration failed after {elapsed:?}"
        );
        assert!(
            elapsed < std::time::Duration::from_secs(8),
            "cached shared-destination migration took {elapsed:?}"
        );
    }

    #[test]
    fn construction_stops_walking_before_it_turns_the_builder_inward() {
        let mut app = App::new();
        app.init_resource::<Time>();
        app.init_resource::<PublishedTerrainDeltas>();
        app.insert_resource(WorldTerrain::default());
        app.add_systems(Update, advance_construction);

        let settlement = app
            .world_mut()
            .spawn(Settlement {
                name: "Facing Test".to_string(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 1,
                treasury: 0,
            })
            .id();
        let plot = Vec3::new(40.0, 5.0, 10.0);
        let stand = Vec3::new(40.0, 5.0, 4.0);
        let builder = app
            .world_mut()
            .spawn((
                PlayerPosition(stand),
                PlayerRotation(0.0),
                VillagerIntent::Resident { settlement },
                MoveTarget(stand),
                HomeRoutine {
                    home: settlement,
                    phase: HomePhase::Leaving,
                },
            ))
            .id();
        app.world_mut().spawn((
            UnderConstruction {
                kind: SettlementBuildingKind::House,
                position: plot,
                rotation: 0.0,
                owner: Some("Ada".to_string()),
                builder: Some(builder),
                settlement,
                stand,
                stage: BuildStage::Walking,
                quality: 0.5,
            },
            shared::components::ConstructionSite {
                kind: SettlementBuildingKind::House,
                settlement: "Facing Test".to_string(),
                raising: false,
                stand,
                rotation: 0.0,
            },
            GoodsInventory::new(SettlementBuildingKind::House.construction_storage_bulk()),
            PlayerPosition(plot),
        ));

        app.update();
        assert!(
            app.world().entity(builder).contains::<MoveTarget>(),
            "construction must wait until its builder has finished leaving home"
        );
        app.world_mut().entity_mut(builder).remove::<HomeRoutine>();
        app.update();

        let builder = app.world().entity(builder);
        assert!(
            builder.get::<MoveTarget>().is_none(),
            "the completed approach target must not overwrite construction facing"
        );
        let yaw = builder.get::<PlayerRotation>().unwrap().0;
        let rendered_front = Quat::from_rotation_y(yaw) * Vec3::NEG_Z;
        let toward_building = (plot - stand).normalize();
        assert!(rendered_front.dot(toward_building) > 0.999);
    }

    #[test]
    fn the_first_worksite_chops_its_own_wood_when_no_lumber_hut_exists() {
        use crate::player::hero::step_units;
        use shared::components::{CharacterKind, TimeWarp};
        use shared::region::RegionCoord;

        let mut app = App::new();
        app.init_resource::<Time>();
        app.insert_resource(WorldTerrain::default());
        app.add_systems(
            Update,
            (
                run_construction_material_logistics,
                sync_carried_load,
                step_units,
            )
                .chain(),
        );

        let hall_position = {
            let terrain = app.world().resource::<WorldTerrain>();
            Vec3::new(1720.0, terrain.get_height(1720.0, 0.0), 0.0)
        };
        app.world_mut()
            .spawn((WorldTime::new_default(), TimeWarp::clamped(100.0)));
        let settlement = app
            .world_mut()
            .spawn((
                Settlement {
                    name: "Firstwood".to_string(),
                    tier: shared::components::SettlementTier::Hamlet,
                    residents: 1,
                    treasury: 0,
                },
                PlayerPosition(hall_position),
                PlayerRotation(0.0),
                GoodsInventory::new(shared::economy::capacity::HALL),
            ))
            .id();
        let kind = SettlementBuildingKind::Farmstead;
        let site_position = hall_position + Vec3::new(30.0, 0.0, 0.0);
        let stand = shared::components::builder_stand_position(
            site_position,
            0.0,
            kind.art().definition().footprint.y,
        );
        let builder = app
            .world_mut()
            .spawn((
                CharacterName("Ada".to_string()),
                CharacterKind::Villager,
                PlayerPosition(hall_position),
                PlayerRotation(0.0),
                RegionCoord::from_world_pos(hall_position),
                CharacterActivity::Idle,
                GoodsInventory::new(shared::economy::capacity::VILLAGER),
                CarriedLoad::default(),
                VillagerIntent::Resident { settlement },
            ))
            .id();
        let site = app
            .world_mut()
            .spawn((
                UnderConstruction {
                    kind,
                    position: site_position,
                    rotation: 0.0,
                    owner: Some("Ada".to_string()),
                    builder: Some(builder),
                    settlement,
                    stand,
                    stage: BuildStage::Supplying,
                    quality: 0.5,
                },
                shared::components::ConstructionSite {
                    kind,
                    settlement: "Firstwood".to_string(),
                    raising: false,
                    stand,
                    rotation: 0.0,
                },
                GoodsInventory::new(kind.construction_storage_bulk()),
                PlayerPosition(site_position),
            ))
            .id();
        app.world_mut().entity_mut(builder).insert((
            VillagerIntent::Building { settlement, site },
            ConstructionMaterialRoutine {
                site,
                cycle: 0,
                failed_tree_routes: 0,
                failed_store_routes: 0,
                failed_delivery_routes: 0,
                tree_retry_after: 0.0,
                store_retry_after: 0.0,
                phase: ConstructionMaterialPhase::Seeking,
            },
        ));

        let step = std::time::Duration::from_secs_f32(1.0 / 60.0);
        let mut saw_chopping = false;
        let mut saw_carried_wood = false;
        for _ in 0..480 {
            app.world_mut().resource_mut::<Time>().advance_by(step);
            app.update();
            let builder_ref = app.world().entity(builder);
            saw_chopping |= builder_ref
                .get::<CharacterActivity>()
                .is_some_and(|activity| *activity == CharacterActivity::Chopping);
            saw_carried_wood |= builder_ref
                .get::<CarriedLoad>()
                .is_some_and(|load| load.good == Some(Good::Wood) && load.amount > 0);
            if app
                .world()
                .entity(site)
                .get::<GoodsInventory>()
                .is_some_and(|inventory| {
                    inventory.amount(Good::Wood) >= kind.construction_wood_required()
                })
            {
                break;
            }
        }

        assert!(
            saw_chopping,
            "the founding builder must visibly chop a real tree"
        );
        assert!(
            saw_carried_wood,
            "chopped wood must travel in the builder's arms"
        );
        assert_eq!(
            app.world()
                .entity(site)
                .get::<GoodsInventory>()
                .unwrap()
                .amount(Good::Wood),
            kind.construction_wood_required()
        );
        assert_eq!(
            app.world()
                .entity(settlement)
                .get::<GoodsInventory>()
                .unwrap()
                .amount(Good::Wood),
            0,
            "this test has no market stock and no lumber hut to source from"
        );
    }

    #[test]
    fn failed_market_route_releases_a_construction_supplier_to_gather_wood() {
        use shared::components::CharacterKind;

        let mut app = App::new();
        app.init_resource::<Time>();
        app.insert_resource(WorldTerrain::default());
        app.add_systems(Update, run_construction_material_logistics);
        app.world_mut().spawn(WorldTime::new_default());

        let hall_position = Vec3::new(1720.0, 0.0, 0.0);
        let settlement = app
            .world_mut()
            .spawn((
                Settlement {
                    name: "Routeford".to_string(),
                    tier: shared::components::SettlementTier::Hamlet,
                    residents: 1,
                    treasury: 0,
                },
                PlayerPosition(hall_position),
                PlayerRotation(0.0),
                {
                    let mut stock = GoodsInventory::new(shared::economy::capacity::HALL);
                    stock.add(Good::Wood, 20);
                    stock
                },
                MootMarket::founding(),
            ))
            .id();
        let entrance = SettlementBuildingKind::Hall.entrance_position(hall_position, 0.0);
        let site_position = hall_position + Vec3::new(30.0, 0.0, 0.0);
        let stand = shared::components::builder_stand_position(
            site_position,
            0.0,
            SettlementBuildingKind::House.art().definition().footprint.y,
        );
        let builder = app
            .world_mut()
            .spawn((
                CharacterName("Ada".to_string()),
                CharacterKind::Villager,
                PlayerPosition(hall_position + Vec3::X * 12.0),
                PlayerRotation(0.0),
                CharacterActivity::Idle,
                GoodsInventory::new(shared::economy::capacity::VILLAGER),
                Wallet::default(),
                VillagerIntent::Resident { settlement },
            ))
            .id();
        let site = app
            .world_mut()
            .spawn((
                UnderConstruction {
                    kind: SettlementBuildingKind::House,
                    position: site_position,
                    rotation: 0.0,
                    owner: Some("Ada".to_string()),
                    builder: Some(builder),
                    settlement,
                    stand,
                    stage: BuildStage::Supplying,
                    quality: 0.5,
                },
                GoodsInventory::new(SettlementBuildingKind::House.construction_storage_bulk()),
                PlayerPosition(site_position),
            ))
            .id();
        app.world_mut().entity_mut(builder).insert((
            VillagerIntent::Building { settlement, site },
            ConstructionMaterialRoutine {
                site,
                cycle: 0,
                failed_tree_routes: 0,
                failed_store_routes: 0,
                failed_delivery_routes: 0,
                tree_retry_after: 0.0,
                store_retry_after: 0.0,
                phase: ConstructionMaterialPhase::CollectingFromStore {
                    source: settlement,
                    entrance,
                },
            },
            MoveTarget(entrance),
            NavigationRouteFailed { goal: entrance },
        ));

        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs(1));
        app.update();

        let builder_ref = app.world().entity(builder);
        assert!(builder_ref.get::<NavigationRouteFailed>().is_none());
        assert!(builder_ref.get::<MoveTarget>().is_none());
        let routine = builder_ref.get::<ConstructionMaterialRoutine>().unwrap();
        assert!(matches!(routine.phase, ConstructionMaterialPhase::Seeking));
        assert_eq!(routine.failed_store_routes, 1);
        assert!(
            routine.store_retry_after > app.world().resource::<Time>().elapsed_secs_f64(),
            "the inaccessible entrance must be backed off instead of retried every tick"
        );
    }

    #[test]
    fn construction_waits_for_the_last_required_wood_bundle() {
        let mut app = App::new();
        app.init_resource::<Time>();
        app.init_resource::<PublishedTerrainDeltas>();
        app.insert_resource(WorldTerrain::default());
        app.add_systems(Update, advance_construction);

        let settlement = app
            .world_mut()
            .spawn(Settlement {
                name: "Tenwood".to_string(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 1,
                treasury: 0,
            })
            .id();
        let kind = SettlementBuildingKind::House;
        let plot = Vec3::new(40.0, 5.0, 10.0);
        let stand = shared::components::builder_stand_position(
            plot,
            0.0,
            kind.art().definition().footprint.y,
        );
        let builder = app
            .world_mut()
            .spawn((
                PlayerPosition(stand),
                PlayerRotation(0.0),
                VillagerIntent::Resident { settlement },
            ))
            .id();
        let mut materials = GoodsInventory::new(kind.construction_storage_bulk());
        materials.add(Good::Wood, kind.construction_wood_required() - 1);
        let site = app
            .world_mut()
            .spawn((
                UnderConstruction {
                    kind,
                    position: plot,
                    rotation: 0.0,
                    owner: Some("Ada".to_string()),
                    builder: Some(builder),
                    settlement,
                    stand,
                    stage: BuildStage::Supplying,
                    quality: 0.5,
                },
                shared::components::ConstructionSite {
                    kind,
                    settlement: "Tenwood".to_string(),
                    raising: false,
                    stand,
                    rotation: 0.0,
                },
                materials,
                PlayerPosition(plot),
            ))
            .id();
        *app.world_mut()
            .entity_mut(builder)
            .get_mut::<VillagerIntent>()
            .unwrap() = VillagerIntent::Building { settlement, site };

        app.update();
        assert_eq!(
            app.world()
                .entity(site)
                .get::<UnderConstruction>()
                .unwrap()
                .stage,
            BuildStage::Supplying
        );
        assert!(
            !app.world()
                .entity(site)
                .get::<shared::components::ConstructionSite>()
                .unwrap()
                .raising
        );

        app.world_mut()
            .entity_mut(site)
            .get_mut::<GoodsInventory>()
            .unwrap()
            .add(Good::Wood, 1);
        app.update();
        assert_eq!(
            app.world()
                .entity(site)
                .get::<UnderConstruction>()
                .unwrap()
                .stage,
            BuildStage::Walking
        );
        app.update();
        assert!(matches!(
            app.world()
                .entity(site)
                .get::<UnderConstruction>()
                .unwrap()
                .stage,
            BuildStage::Raising { .. }
        ));
    }

    #[test]
    fn housed_villager_walks_through_the_door_at_night_and_back_out_at_dawn() {
        use crate::player::hero::step_units;
        use shared::components::{CharacterKind, TimeWarp};
        use shared::region::RegionCoord;

        let mut app = App::new();
        app.init_resource::<Time>();
        app.insert_resource(WorldTerrain::default());
        app.add_systems(
            Update,
            (
                ensure_households,
                assign_households,
                run_household_schedules,
                step_units,
            )
                .chain(),
        );

        let (hall_position, house_position) = {
            let terrain = app.world().resource::<WorldTerrain>();
            let hall = Vec3::new(1720.0, terrain.get_height(1720.0, 0.0), 0.0);
            let house = Vec3::new(1702.0, terrain.get_height(1702.0, -5.0), -5.0);
            (hall, house)
        };
        let clock = app
            .world_mut()
            .spawn((WorldTime::new(100.0, 20.0, 100.0), TimeWarp::clamped(100.0)))
            .id();
        let settlement = app
            .world_mut()
            .spawn(Settlement {
                name: "Nightford".to_string(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 1,
                treasury: 0,
            })
            .id();
        let house = app
            .world_mut()
            .spawn((
                SettlementBuilding {
                    kind: SettlementBuildingKind::House,
                    settlement: "Nightford".to_string(),
                    owner: Some("Ada".to_string()),
                    quality: 0.5,
                    workers: Vec::new(),
                },
                PlayerPosition(house_position),
                PlayerRotation(0.0),
            ))
            .id();
        let villager = app
            .world_mut()
            .spawn((
                CharacterName("Ada".to_string()),
                CharacterKind::Villager,
                VillagerIntent::Resident { settlement },
                PlayerPosition(hall_position),
                PlayerRotation(0.0),
                RegionCoord::from_world_pos(hall_position),
                CharacterActivity::Idle,
            ))
            .id();

        let step = std::time::Duration::from_secs_f32(1.0 / 60.0);
        let mut door_request_ticks = 0;
        for _ in 0..60 {
            app.world_mut().resource_mut::<Time>().advance_by(step);
            app.update();
            door_request_ticks +=
                usize::from(app.world().entity(villager).contains::<BuildingDoorUse>());
        }

        let door = SettlementBuildingKind::House.entrance_position(house_position, 0.0);
        let inside = SettlementBuildingKind::House.interior_door_position(house_position, 0.0);
        let household = app.world().entity(house).get::<Household>().unwrap();
        assert_eq!(household.residents, vec!["Ada"]);
        assert!(
            door_request_ticks > 0,
            "the villager must request the door while crossing at full time warp"
        );
        assert_eq!(
            *app.world()
                .entity(villager)
                .get::<CharacterActivity>()
                .unwrap(),
            CharacterActivity::Indoors
        );
        let sleeping_at = app
            .world()
            .entity(villager)
            .get::<PlayerPosition>()
            .unwrap()
            .0;
        assert!(ground_distance(sleeping_at, inside) <= DOOR_REACH);
        assert!(
            ground_distance(sleeping_at, house_position) < ground_distance(door, house_position),
            "the villager must cross the wall plane instead of vanishing outside"
        );

        // Reproduce the real navigation footprint only after the villager is
        // asleep. The old DOOR_REACH completion released BuildingDoorUse a
        // fraction before this blocker ended, so the first route to work began
        // from an impossible point inside the cabin.
        let mut obstacles = SpatialObstacleGrid::default();
        let definition = shared::building::BuildingType::LogCabin.definition();
        obstacles.insert(shared::spatial::ObstacleEntry {
            center: Vec2::new(house_position.x, house_position.z),
            half_extents: definition.footprint * 0.5
                + Vec2::splat(crate::world::navgrid::VILLAGER_NAV_RADIUS),
            rotation: 0.0,
            obstacle_type: shared::building::BuildingType::LogCabin as u32,
        });
        app.insert_resource(obstacles);

        app.world_mut()
            .entity_mut(clock)
            .get_mut::<WorldTime>()
            .unwrap()
            .seconds_in_cycle = 0.0;
        let mut exit_door_request_ticks = 0;
        for _ in 0..30 {
            app.world_mut().resource_mut::<Time>().advance_by(step);
            app.update();
            exit_door_request_ticks +=
                usize::from(app.world().entity(villager).contains::<BuildingDoorUse>());
        }

        let villager_ref = app.world().entity(villager);
        assert!(
            exit_door_request_ticks > 0,
            "the villager must request the door while leaving at full time warp"
        );
        assert!(!villager_ref.contains::<HomeRoutine>());
        assert!(!villager_ref.contains::<BuildingDoorUse>());
        assert_eq!(
            *villager_ref.get::<CharacterActivity>().unwrap(),
            CharacterActivity::Idle
        );
        let outside = exterior_door_clearance_position(house_position, door);
        assert!(
            ground_distance(villager_ref.get::<PlayerPosition>().unwrap().0, outside) <= DOOR_REACH,
            "the morning crossing must finish beyond the barely-clear door anchor"
        );
        let exited = villager_ref.get::<PlayerPosition>().unwrap().0;
        assert!(
            !app.world()
                .resource::<SpatialObstacleGrid>()
                .point_blocked(Vec2::new(exited.x, exited.z)),
            "morning must not release the villager while still inside the cabin blocker"
        );
    }

    #[test]
    fn household_capacity_is_entity_safe_when_names_repeat() {
        let mut app = App::new();
        app.add_systems(Update, (ensure_households, assign_households).chain());

        let settlement = app
            .world_mut()
            .spawn(Settlement {
                name: "Twinstead".to_string(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 5,
                treasury: 0,
            })
            .id();
        let houses: Vec<_> = [Vec3::ZERO, Vec3::new(30.0, 0.0, 0.0)]
            .into_iter()
            .map(|position| {
                app.world_mut()
                    .spawn((
                        SettlementBuilding {
                            kind: SettlementBuildingKind::House,
                            settlement: "Twinstead".to_string(),
                            owner: None,
                            quality: 0.5,
                            workers: Vec::new(),
                        },
                        PlayerPosition(position),
                    ))
                    .id()
            })
            .collect();
        let villagers: Vec<_> = (0..5)
            .map(|index| {
                app.world_mut()
                    .spawn((
                        CharacterName("Robin".to_string()),
                        VillagerIntent::Resident { settlement },
                        PlayerPosition(Vec3::new(index as f32, 0.0, 0.0)),
                    ))
                    .id()
            })
            .collect();

        app.update();

        let mut assignments = HashMap::<Entity, usize>::new();
        for villager in villagers {
            let assignment = app
                .world()
                .entity(villager)
                .get::<HomeAssignment>()
                .expect("every resident fits across the two cabins");
            *assignments.entry(assignment.home).or_default() += 1;
        }
        assert_eq!(assignments.values().sum::<usize>(), 5);
        assert!(
            assignments.values().all(|used| *used <= 4),
            "a repeated display name must not overbook a four-bed cabin: {assignments:?}"
        );
        let rostered: usize = houses
            .iter()
            .map(|house| {
                app.world()
                    .entity(*house)
                    .get::<Household>()
                    .unwrap()
                    .residents
                    .len()
            })
            .sum();
        assert_eq!(rostered, 5);
    }

    #[test]
    fn travelling_villagers_do_not_occupy_resident_beds() {
        let mut app = App::new();
        app.add_systems(Update, (ensure_households, assign_households).chain());

        let settlement = app
            .world_mut()
            .spawn(Settlement {
                name: "Arrival".to_string(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 7,
                treasury: 0,
            })
            .id();
        let houses: Vec<_> = [Vec3::ZERO, Vec3::new(30.0, 0.0, 0.0)]
            .into_iter()
            .map(|position| {
                app.world_mut()
                    .spawn((
                        SettlementBuilding {
                            kind: SettlementBuildingKind::House,
                            settlement: "Arrival".to_string(),
                            owner: None,
                            quality: 0.5,
                            workers: Vec::new(),
                        },
                        PlayerPosition(position),
                    ))
                    .id()
            })
            .collect();
        let traveller = app
            .world_mut()
            .spawn((
                CharacterName("Still On The Road".to_string()),
                VillagerIntent::Travelling { settlement },
                PlayerPosition(Vec3::ZERO),
            ))
            .id();
        let residents: Vec<_> = (0..7)
            .map(|index| {
                app.world_mut()
                    .spawn((
                        CharacterName(format!("Resident {index}")),
                        VillagerIntent::Resident { settlement },
                        PlayerPosition(Vec3::new(10.0 + index as f32, 0.0, 0.0)),
                    ))
                    .id()
            })
            .collect();

        app.update();

        assert!(app.world().get::<HomeAssignment>(traveller).is_none());
        assert!(residents
            .iter()
            .all(|resident| app.world().get::<HomeAssignment>(*resident).is_some()));
        let occupied: usize = houses
            .iter()
            .map(|house| {
                app.world()
                    .get::<Household>(*house)
                    .unwrap()
                    .residents
                    .len()
            })
            .sum();
        assert_eq!(occupied, 7, "occupied beds must equal actual residents");
    }

    #[test]
    fn needs_are_taken_in_order_and_stop_when_met() {
        let mut have = HashMap::new();
        have.insert(SettlementBuildingKind::Hall, 1);
        assert_eq!(
            next_need(&have, 4, None),
            Some(SettlementBuildingKind::Farmstead)
        );

        have.insert(SettlementBuildingKind::Farmstead, 1);
        assert_eq!(
            next_need(&have, 4, None),
            Some(SettlementBuildingKind::LumberjackHut)
        );

        have.insert(SettlementBuildingKind::LumberjackHut, 1);
        assert_eq!(
            next_need(&have, 4, None),
            Some(SettlementBuildingKind::House)
        );

        have.insert(SettlementBuildingKind::House, 1);
        assert_eq!(
            next_need(&have, 4, None),
            None,
            "a fed, timbered, housed village wants nothing more yet"
        );

        assert_eq!(
            next_need(&have, 9, None),
            Some(SettlementBuildingKind::House),
            "missing beds must request repeated Houses"
        );

        have.insert(SettlementBuildingKind::House, 3);
        let shortage = SettlementEconomy {
            observed_days: 1,
            reserve_days: 0.0,
            recent_food_production: 0.0,
            ..default()
        };
        assert_eq!(
            next_need(&have, 9, Some(&shortage)),
            Some(SettlementBuildingKind::Farmstead),
            "measured food shortage must request another supported Farmstead"
        );
    }

    #[test]
    fn residents_eat_food_then_wheat_once_per_world_day() {
        let mut app = App::new();
        app.init_resource::<SettlementEconomyRuntime>();
        app.add_systems(
            Update,
            (ensure_settlement_economies, update_settlement_economies).chain(),
        );
        let clock = app.world_mut().spawn(WorldTime::new_default()).id();
        let mut stock = GoodsInventory::new(shared::economy::capacity::HALL);
        assert_eq!(stock.add(Good::Food, 2), 2);
        assert_eq!(stock.add(Good::Wheat, 3), 3);
        assert_eq!(stock.add(Good::Wood, 1), 1);
        let hall = app
            .world_mut()
            .spawn((
                Settlement {
                    name: "Dailybread".to_string(),
                    tier: shared::components::SettlementTier::Hamlet,
                    residents: 4,
                    treasury: 0,
                },
                stock,
            ))
            .id();

        app.update();
        app.world_mut().get_mut::<WorldTime>(clock).unwrap().day = 1;
        app.update();

        let inventory = app.world().get::<GoodsInventory>(hall).unwrap();
        assert_eq!(inventory.amount(Good::Food), 0);
        assert_eq!(inventory.amount(Good::Wheat), 1);
        assert_eq!(inventory.amount(Good::Wood), 1);
        let economy = app.world().get::<SettlementEconomy>(hall).unwrap();
        assert_eq!(economy.edible_stock, 1);
        assert_eq!(economy.recent_food_consumption, 4.0);
        assert_eq!(economy.unmet_food, 0);

        app.world_mut().get_mut::<WorldTime>(clock).unwrap().day = 2;
        app.update();
        let economy = app.world().get::<SettlementEconomy>(hall).unwrap();
        assert_eq!(economy.edible_stock, 0);
        assert_eq!(economy.recent_food_consumption, 2.5);
        assert_eq!(economy.unmet_food, 3);
    }

    #[test]
    fn a_daily_market_ration_moves_food_and_exactly_conserves_coin() {
        let mut app = App::new();
        app.init_resource::<SettlementEconomyRuntime>();
        app.add_systems(
            Update,
            (
                ensure_settlement_economies,
                update_moot_market_targets,
                update_settlement_economies,
            )
                .chain(),
        );
        let clock = app.world_mut().spawn(WorldTime::new_default()).id();
        let mut stock = GoodsInventory::new(shared::economy::capacity::HALL);
        assert_eq!(stock.add(Good::Food, 2), 2);
        let hall = app
            .world_mut()
            .spawn((
                Settlement {
                    name: "Coinbread".to_string(),
                    tier: shared::components::SettlementTier::Hamlet,
                    residents: 1,
                    treasury: STARTING_TREASURY_MONEY,
                },
                stock,
                MootMarket::founding(),
            ))
            .id();
        let resident = app
            .world_mut()
            .spawn((
                CharacterName("Ada".to_string()),
                CharacterKind::Villager,
                VillagerIntent::Resident { settlement: hall },
                Wallet::founding_villager(),
            ))
            .id();

        app.update();
        let wallet_before = app.world().get::<Wallet>(resident).unwrap().balance();
        let market_before = app
            .world()
            .get::<MootMarket>(hall)
            .unwrap()
            .total_liquidity();
        app.world_mut().get_mut::<WorldTime>(clock).unwrap().day = 1;
        app.update();

        let wallet_after = app.world().get::<Wallet>(resident).unwrap().balance();
        let market = app.world().get::<MootMarket>(hall).unwrap();
        assert!(wallet_after < wallet_before, "the household did not pay");
        assert_eq!(
            wallet_before + market_before,
            wallet_after + market.total_liquidity(),
            "a ration must transfer coin rather than create or destroy it"
        );
        assert_eq!(market.pool(Good::Food).units_sold, 1);
        assert_eq!(
            app.world()
                .get::<GoodsInventory>(hall)
                .unwrap()
                .amount(Good::Food),
            1
        );
    }

    #[test]
    fn poor_relief_buys_a_ration_from_public_money() {
        let mut app = App::new();
        app.init_resource::<SettlementEconomyRuntime>();
        app.add_systems(
            Update,
            (
                ensure_settlement_economies,
                update_moot_market_targets,
                update_settlement_economies,
            )
                .chain(),
        );
        let clock = app.world_mut().spawn(WorldTime::new_default()).id();
        let mut stock = GoodsInventory::new(shared::economy::capacity::HALL);
        assert_eq!(stock.add(Good::Wheat, 5), 5);
        let hall = app
            .world_mut()
            .spawn((
                Settlement {
                    name: "Almsford".to_string(),
                    tier: shared::components::SettlementTier::Hamlet,
                    residents: 1,
                    treasury: STARTING_TREASURY_MONEY,
                },
                stock,
                MootMarket::founding(),
                SettlementPolicies::poor_relief(),
            ))
            .id();
        let poor = app
            .world_mut()
            .spawn((
                CharacterName("Ada".to_string()),
                CharacterKind::Villager,
                VillagerIntent::Resident { settlement: hall },
                Wallet::new(0),
            ))
            .id();

        app.update();
        app.world_mut()
            .resource_mut::<SettlementEconomyRuntime>()
            .record_food_production(hall, 1);
        let money_before = app.world().get::<Settlement>(hall).unwrap().treasury
            + app
                .world()
                .get::<MootMarket>(hall)
                .unwrap()
                .total_liquidity();
        app.world_mut().get_mut::<WorldTime>(clock).unwrap().day = 1;
        app.update();

        let economy = app.world().get::<SettlementEconomy>(hall).unwrap();
        assert_eq!(economy.unmet_food, 0);
        assert_eq!(economy.recent_food_consumption, 1.0);
        assert_eq!(economy.edible_stock, 4);
        assert_eq!(app.world().get::<Wallet>(poor).unwrap().balance(), 0);
        assert!(
            app.world().get::<Settlement>(hall).unwrap().treasury < STARTING_TREASURY_MONEY,
            "the policy must spend public money rather than mint a meal"
        );
        let money_after = app.world().get::<Settlement>(hall).unwrap().treasury
            + app
                .world()
                .get::<MootMarket>(hall)
                .unwrap()
                .total_liquidity();
        assert_eq!(money_after, money_before);
    }

    #[test]
    fn poor_relief_protects_an_unsustainable_or_thin_reserve() {
        fn run_case(stock: u32, production: u32) -> (u32, u32, u64) {
            let mut app = App::new();
            app.init_resource::<SettlementEconomyRuntime>();
            app.add_systems(
                Update,
                (
                    ensure_settlement_economies,
                    update_moot_market_targets,
                    update_settlement_economies,
                )
                    .chain(),
            );
            let clock = app.world_mut().spawn(WorldTime::new_default()).id();
            let mut inventory = GoodsInventory::new(shared::economy::capacity::HALL);
            assert_eq!(inventory.add(Good::Food, stock), stock);
            let hall = app
                .world_mut()
                .spawn((
                    Settlement {
                        name: "Reserveford".to_string(),
                        tier: shared::components::SettlementTier::Hamlet,
                        residents: 1,
                        treasury: STARTING_TREASURY_MONEY,
                    },
                    inventory,
                    MootMarket::founding(),
                    SettlementPolicies::poor_relief(),
                ))
                .id();
            app.world_mut().spawn((
                CharacterKind::Villager,
                VillagerIntent::Resident { settlement: hall },
                Wallet::new(0),
            ));

            app.update();
            app.world_mut()
                .resource_mut::<SettlementEconomyRuntime>()
                .record_food_production(hall, production);
            app.world_mut().get_mut::<WorldTime>(clock).unwrap().day = 1;
            app.update();

            (
                app.world()
                    .get::<SettlementEconomy>(hall)
                    .unwrap()
                    .unmet_food,
                app.world()
                    .get::<GoodsInventory>(hall)
                    .unwrap()
                    .edible_amount(),
                app.world().get::<Settlement>(hall).unwrap().treasury,
            )
        }

        assert_eq!(
            run_case(8, 0),
            (1, 8, STARTING_TREASURY_MONEY),
            "stock alone must not disguise failed production"
        );
        assert_eq!(
            run_case(3, 1),
            (1, 3, STARTING_TREASURY_MONEY),
            "relief must not spend the last three reserve days"
        );
    }

    #[test]
    fn a_broke_resident_goes_hungry_when_relief_is_disabled() {
        let mut app = App::new();
        app.init_resource::<SettlementEconomyRuntime>();
        app.add_systems(
            Update,
            (
                ensure_settlement_economies,
                update_moot_market_targets,
                update_settlement_economies,
            )
                .chain(),
        );
        let clock = app.world_mut().spawn(WorldTime::new_default()).id();
        let mut stock = GoodsInventory::new(shared::economy::capacity::HALL);
        stock.add(Good::Food, 1);
        let hall = app
            .world_mut()
            .spawn((
                Settlement {
                    name: "Hardmarket".to_string(),
                    tier: shared::components::SettlementTier::Hamlet,
                    residents: 1,
                    treasury: STARTING_TREASURY_MONEY,
                },
                stock,
                MootMarket::founding(),
                SettlementPolicies::default(),
            ))
            .id();
        let resident = app
            .world_mut()
            .spawn((
                CharacterKind::Villager,
                VillagerIntent::Resident { settlement: hall },
                Wallet::new(0),
                Nutrition::default(),
            ))
            .id();

        app.update();
        app.world_mut().get_mut::<WorldTime>(clock).unwrap().day = 1;
        app.update();

        assert_eq!(
            app.world()
                .get::<SettlementEconomy>(hall)
                .unwrap()
                .unmet_food,
            1
        );
        assert_eq!(
            app.world()
                .get::<GoodsInventory>(hall)
                .unwrap()
                .amount(Good::Food),
            1,
            "an unaffordable ration must remain real market stock"
        );
        assert_eq!(
            app.world().get::<Settlement>(hall).unwrap().treasury,
            STARTING_TREASURY_MONEY,
            "disabled relief must not spend public money"
        );
        let nutrition = app.world().get::<Nutrition>(resident).unwrap();
        assert!(nutrition.is_hungry());
        assert_eq!(nutrition.consecutive_missed_meals, 1);
        assert_eq!(nutrition.last_meal_day, None);
    }

    #[test]
    fn three_secure_days_advance_a_hamlet_to_village() {
        let mut app = App::new();
        app.init_resource::<SettlementEconomyRuntime>();
        app.add_systems(
            Update,
            (ensure_settlement_economies, update_settlement_economies).chain(),
        );
        let clock = app.world_mut().spawn(WorldTime::new_default()).id();
        let mut stock = GoodsInventory::new(shared::economy::capacity::HALL);
        assert_eq!(stock.add(Good::Wheat, 40), 40);
        let hall = app
            .world_mut()
            .spawn((
                Settlement {
                    name: "Plenty".to_string(),
                    tier: shared::components::SettlementTier::Hamlet,
                    residents: 4,
                    treasury: 0,
                },
                stock,
            ))
            .id();

        app.update();
        for day in 1..=3 {
            app.world_mut()
                .resource_mut::<SettlementEconomyRuntime>()
                .record_food_production(hall, 4);
            app.world_mut().get_mut::<WorldTime>(clock).unwrap().day = day;
            app.update();
        }

        let settlement = app.world().get::<Settlement>(hall).unwrap();
        let economy = app.world().get::<SettlementEconomy>(hall).unwrap();
        assert_eq!(settlement.tier, shared::components::SettlementTier::Village);
        assert_eq!(economy.food_secure_days, VILLAGE_REQUIRED_SECURE_DAYS);
        assert!(economy.prosperity >= VILLAGE_MIN_PROSPERITY);
    }

    /// The "already planned" half is what stops three residents all deciding
    /// the village needs a farm at the same instant.
    #[test]
    fn a_planned_building_counts_as_had() {
        let mut have = HashMap::new();
        have.insert(SettlementBuildingKind::Hall, 1);
        // Nothing built, but a farm already approved.
        have.insert(SettlementBuildingKind::Farmstead, 1);
        assert_eq!(
            next_need(&have, 4, None),
            Some(SettlementBuildingKind::LumberjackHut),
            "a planned farm must not be requested twice"
        );
    }

    #[test]
    fn distinct_permits_are_approved_without_waiting_for_construction() {
        let mut app = App::new();
        app.init_resource::<Time>();
        app.init_resource::<VillageClock>();
        app.insert_resource(WorldTerrain::default());
        app.add_systems(Update, consider_permits);

        let hall_position = {
            let terrain = app.world().resource::<WorldTerrain>();
            Vec3::new(1720.0, terrain.get_height(1720.0, 0.0), 0.0)
        };
        let settlement = app
            .world_mut()
            .spawn((
                Settlement {
                    name: "Quickstead".to_string(),
                    tier: shared::components::SettlementTier::Hamlet,
                    residents: 3,
                    treasury: 0,
                },
                PlayerPosition(hall_position),
            ))
            .id();
        for name in ["Ada", "Bea", "Cy"] {
            app.world_mut().spawn((
                CharacterName(name.to_string()),
                VillagerIntent::Resident { settlement },
            ));
        }

        for expected_sites in 1..=3 {
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(std::time::Duration::from_secs_f32(PERMIT_INTERVAL + 0.1));
            app.update();
            let site_count = {
                let world = app.world_mut();
                let mut query = world.query::<&UnderConstruction>();
                query.iter(world).count()
            };
            assert_eq!(
                site_count, expected_sites,
                "the next distinct permit must not wait for earlier construction"
            );
        }

        let mut world = std::mem::take(&mut *app.world_mut());
        let sites: Vec<_> = world
            .query::<&UnderConstruction>()
            .iter(&world)
            .map(|site| (site.kind, site.owner.clone().unwrap(), site.position))
            .collect();
        let kinds: HashSet<_> = sites.iter().map(|(kind, _, _)| *kind).collect();
        assert_eq!(
            kinds,
            HashSet::from([
                SettlementBuildingKind::Farmstead,
                SettlementBuildingKind::LumberjackHut,
                SettlementBuildingKind::House,
            ]),
            "planned kinds must suppress duplicate permits"
        );
        let owners: HashSet<_> = sites.iter().map(|(_, owner, _)| owner.as_str()).collect();
        assert_eq!(
            owners.len(),
            3,
            "zero-holding residents must receive their first permit before repeat owners"
        );
        for (index, (_, _, position)) in sites.iter().enumerate() {
            for (_, _, other) in sites.iter().skip(index + 1) {
                assert!(
                    position.distance(*other) > 1.0,
                    "pending plots must reserve their ground"
                );
            }
        }
    }

    #[test]
    fn a_permit_does_not_interrupt_an_active_fisher_mid_shift() {
        let mut app = App::new();
        app.init_resource::<Time>();
        app.init_resource::<VillageClock>();
        app.insert_resource(WorldTerrain::default());
        app.add_systems(Update, consider_permits);

        let hall_position = {
            let terrain = app.world().resource::<WorldTerrain>();
            Vec3::new(1720.0, terrain.get_height(1720.0, 0.0), 0.0)
        };
        let settlement = app
            .world_mut()
            .spawn((
                Settlement {
                    name: "Shiftstead".to_string(),
                    tier: shared::components::SettlementTier::Hamlet,
                    residents: 2,
                    treasury: 0,
                },
                PlayerPosition(hall_position),
            ))
            .id();
        let active_fisher = app
            .world_mut()
            .spawn((
                CharacterName("Ada".to_string()),
                VillagerIntent::Resident { settlement },
                FishingRoutine {
                    hut: settlement,
                    pier: settlement,
                    hall: settlement,
                    catch_seconds: 0.0,
                    production_day: 0,
                    produced_today: 0,
                    phase: FishingPhase::Fishing,
                },
            ))
            .id();
        let available = app
            .world_mut()
            .spawn((
                CharacterName("Bea".to_string()),
                VillagerIntent::Resident { settlement },
            ))
            .id();

        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs_f32(PERMIT_INTERVAL + 0.1));
        app.update();

        let world = app.world_mut();
        let site = world
            .query::<&UnderConstruction>()
            .single(world)
            .expect("an available resident should still receive the needed permit");
        assert_eq!(site.builder, Some(available));
        assert_ne!(site.builder, Some(active_fisher));
        assert!(
            world.entity(active_fisher).contains::<FishingRoutine>(),
            "granting another resident's permit must not interrupt active fishing"
        );
    }

    #[test]
    fn the_reeve_builds_public_progression_without_stopping_essential_trades() {
        let mut app = App::new();
        app.init_resource::<Time>();
        app.init_resource::<VillageClock>();
        app.insert_resource(WorldTerrain::default());
        app.add_systems(Update, consider_permits);

        let hall_position = {
            let terrain = app.world().resource::<WorldTerrain>();
            Vec3::new(1720.0, terrain.get_height(1720.0, 0.0), 0.0)
        };
        let settlement = app
            .world_mut()
            .spawn((
                Settlement {
                    name: "Civicstead".to_string(),
                    tier: shared::components::SettlementTier::Village,
                    residents: 4,
                    treasury: 0,
                },
                PlayerPosition(hall_position),
                MootAdministration {
                    reeve: Some("Ada".to_string()),
                    ..default()
                },
            ))
            .id();
        for kind in [
            SettlementBuildingKind::Farmstead,
            SettlementBuildingKind::LumberjackHut,
            SettlementBuildingKind::House,
        ] {
            app.world_mut().spawn(SettlementBuilding {
                kind,
                settlement: "Civicstead".to_string(),
                owner: Some("Founder".to_string()),
                quality: 0.5,
                workers: Vec::new(),
            });
        }
        for (name, occupation) in [
            ("Ada", "Reeve"),
            ("Bea", "Farmer"),
            ("Cy", "Woodcutter"),
            ("Dee", "Fisher"),
        ] {
            app.world_mut().spawn((
                CharacterName(name.to_string()),
                VillagerIntent::Resident { settlement },
                Occupation(Some(occupation.to_string())),
                WorkStatus::Employed,
                Wallet::new(shared::economy::STARTING_VILLAGER_MONEY),
            ));
        }

        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs_f32(PERMIT_INTERVAL + 0.1));
        app.update();

        let world = app.world_mut();
        let (site_entity, site) = world
            .query::<(Entity, &UnderConstruction)>()
            .iter(world)
            .next()
            .expect("the Village should request its Marketplace");
        assert_eq!(site.kind, SettlementBuildingKind::Market);
        assert_eq!(site.owner, None, "the Marketplace is a public work");
        let reeve = world
            .query::<(&CharacterName, &VillagerIntent)>()
            .iter(world)
            .find(|(name, _)| name.0 == "Ada")
            .unwrap();
        assert!(
            matches!(reeve.1, VillagerIntent::Building { site: active, .. } if *active == site_entity)
        );
        for (name, intent) in world
            .query::<(&CharacterName, &VillagerIntent)>()
            .iter(world)
        {
            if name.0 != "Ada" {
                assert!(
                    matches!(intent, VillagerIntent::Resident { .. }),
                    "{name:?} was pulled away from essential work"
                );
            }
        }
    }

    #[test]
    fn a_market_porter_collects_a_bounded_load_while_the_woodcutter_keeps_working() {
        let mut app = App::new();
        app.init_resource::<Time>();
        app.insert_resource(WorldTerrain::default());
        app.add_systems(Update, (run_market_collections, sync_carried_load).chain());

        let hall_position = Vec3::new(0.0, 5.0, 0.0);
        let hall = app
            .world_mut()
            .spawn((
                Settlement {
                    name: "Yewcrag".to_string(),
                    tier: shared::components::SettlementTier::Hamlet,
                    residents: 1,
                    treasury: 0,
                },
                PlayerPosition(hall_position),
                PlayerRotation(0.0),
                GoodsInventory::new(shared::economy::capacity::HALL),
                MootMarket::founding(),
            ))
            .id();

        let hut_position = Vec3::new(25.0, 5.0, 0.0);
        let mut hut_store = GoodsInventory::new(shared::economy::capacity::LUMBERJACK_HUT);
        assert_eq!(hut_store.add(Good::Wood, 45), 45, "45 bundles is 75% full");
        let hut = app
            .world_mut()
            .spawn((
                SettlementBuilding {
                    kind: SettlementBuildingKind::LumberjackHut,
                    settlement: "Yewcrag".to_string(),
                    owner: Some("Ada".to_string()),
                    quality: 0.5,
                    workers: vec!["Ada".to_string()],
                },
                PlayerPosition(hut_position),
                PlayerRotation(0.0),
                hut_store,
                BusinessSalePolicy::default(),
                BusinessAccount::default(),
                BusinessWagePolicy::default(),
            ))
            .id();
        let porter = app
            .world_mut()
            .spawn((
                CharacterKind::Villager,
                MarketPorter { settlement: hall },
                PlayerPosition(hall_position),
                PlayerRotation(0.0),
                CharacterActivity::Indoors,
                GoodsInventory::new(shared::economy::capacity::VILLAGER),
                CarriedLoad::default(),
            ))
            .id();

        app.update();
        assert!(matches!(
            app.world()
                .entity(porter)
                .get::<MarketCollectionRoutine>()
                .unwrap()
                .phase,
            MarketCollectionPhase::GoingToBusiness
        ));
        let hut_entrance =
            SettlementBuildingKind::LumberjackHut.entrance_position(hut_position, 0.0);
        app.world_mut()
            .entity_mut(porter)
            .get_mut::<PlayerPosition>()
            .unwrap()
            .0 = hut_entrance;
        app.update();
        let world = app.world();
        assert_eq!(
            world
                .entity(porter)
                .get::<GoodsInventory>()
                .unwrap()
                .amount(Good::Wood),
            3
        );
        assert_eq!(
            world
                .entity(hut)
                .get::<GoodsInventory>()
                .unwrap()
                .amount(Good::Wood),
            42
        );
        assert_eq!(world.entity(porter).get::<CarriedLoad>().unwrap().amount, 3);
        assert!(matches!(
            world
                .entity(porter)
                .get::<MarketCollectionRoutine>()
                .unwrap()
                .phase,
            MarketCollectionPhase::ReturningToHall
        ));

        let hall_entrance = SettlementBuildingKind::Hall.entrance_position(hall_position, 0.0);
        app.world_mut()
            .entity_mut(porter)
            .insert(NavigationRouteFailed {
                goal: hall_entrance,
            });
        app.update();
        assert!(
            app.world()
                .entity(porter)
                .get::<NavigationRouteFailed>()
                .is_none(),
            "one failed return route must not permanently disable the settlement's only porter"
        );
        assert!(matches!(
            app.world()
                .entity(porter)
                .get::<MarketCollectionRoutine>()
                .unwrap()
                .phase,
            MarketCollectionPhase::ReturningToHall
        ));
        assert_eq!(
            app.world()
                .entity(porter)
                .get::<GoodsInventory>()
                .unwrap()
                .amount(Good::Wood),
            3,
            "a retry must retain the physical load and reserved market cash"
        );
        app.world_mut()
            .entity_mut(porter)
            .get_mut::<PlayerPosition>()
            .unwrap()
            .0 = hall_entrance;
        app.update();
        let world = app.world();
        assert_eq!(
            world
                .entity(porter)
                .get::<GoodsInventory>()
                .unwrap()
                .amount(Good::Wood),
            0
        );
        assert_eq!(
            world
                .entity(hall)
                .get::<GoodsInventory>()
                .unwrap()
                .amount(Good::Wood),
            3
        );
        assert!(world
            .entity(porter)
            .get::<MarketCollectionRoutine>()
            .is_none());
        assert!(world.entity(hut).get::<BusinessAccount>().unwrap().cash > 0);
    }

    #[test]
    fn a_farmer_carries_wheat_only_to_the_farmstead_store() {
        let mut app = App::new();
        app.init_resource::<Time>();
        app.init_resource::<SettlementEconomyRuntime>();
        app.add_systems(Update, run_farmer_routines);

        let hall = app
            .world_mut()
            .spawn((
                Settlement {
                    name: "Barleywick".to_string(),
                    tier: shared::components::SettlementTier::Hamlet,
                    residents: 1,
                    treasury: 0,
                },
                GoodsInventory::new(shared::economy::capacity::HALL),
            ))
            .id();
        let clock = app
            .world_mut()
            .spawn((
                WorldTime::new_default(),
                shared::components::TimeWarp::clamped(1.0),
            ))
            .id();

        let farm_position = Vec3::new(20.0, 4.0, 0.0);
        let farm = app
            .world_mut()
            .spawn((
                SettlementBuilding {
                    kind: SettlementBuildingKind::Farmstead,
                    settlement: "Barleywick".to_string(),
                    owner: Some("Ada".to_string()),
                    quality: 0.9,
                    workers: vec!["Ada".to_string()],
                },
                PlayerPosition(farm_position),
                PlayerRotation(0.0),
                GoodsInventory::new(shared::economy::capacity::FARMSTEAD),
            ))
            .id();
        let field_position = SettlementBuildingKind::Farmstead
            .field_position_at(farm_position, 0.0, 0)
            .unwrap();
        let field = app
            .world_mut()
            .spawn((
                FarmField {
                    settlement: "Barleywick".to_string(),
                    farmstead: farm_position,
                    plot_index: 0,
                    quality: 0.9,
                },
                PlayerPosition(field_position),
                PlayerRotation(0.0),
            ))
            .id();
        let farmer = app
            .world_mut()
            .spawn((
                CharacterKind::Villager,
                CharacterName("Ada".to_string()),
                CharacterAttributes::default(),
                VillagerIntent::Resident { settlement: hall },
                PlayerPosition(field_position),
                CharacterActivity::Farming,
                GoodsInventory::new(shared::economy::capacity::VILLAGER),
                FarmerRoutine {
                    farmstead: farm,
                    field,
                    hall,
                    harvest_seconds: farmer_seconds_per_wheat(0.9),
                    production_day: u32::MAX,
                    produced_today: 0,
                    phase: FarmerPhase::Farming,
                },
            ))
            .id();

        app.update();
        assert_eq!(
            app.world()
                .entity(farmer)
                .get::<GoodsInventory>()
                .unwrap()
                .amount(Good::Wheat),
            0,
            "partial basket progress must keep the visible harvest animation active"
        );
        assert_eq!(
            app.world()
                .entity(farm)
                .get::<GoodsInventory>()
                .unwrap()
                .amount(Good::Wheat),
            0
        );

        // Completing the second unit materialises the whole field basket and
        // begins the return trip on that same simulation tick; it is not a
        // daily production ceiling.
        app.world_mut()
            .entity_mut(farmer)
            .get_mut::<FarmerRoutine>()
            .unwrap()
            .harvest_seconds = farmer_seconds_per_wheat(0.9) * 2.0;
        app.update();
        assert_eq!(
            app.world()
                .entity(farmer)
                .get::<GoodsInventory>()
                .unwrap()
                .amount(Good::Wheat),
            2,
            "the farmer should fill a two-Wheat carrying batch"
        );

        let farm_entrance = SettlementBuildingKind::Farmstead.entrance_position(farm_position, 0.0);
        app.world_mut()
            .entity_mut(farmer)
            .insert(NavigationRouteFailed {
                goal: farm_entrance,
            });
        app.update();
        assert!(app.world().entity(farmer).get::<FarmerRoutine>().is_some());
        assert!(app.world().entity(farmer).get::<WorkerOffDuty>().is_none());
        assert!(app
            .world()
            .entity(farmer)
            .get::<NavigationRouteFailed>()
            .is_none());
        assert_eq!(
            app.world()
                .entity(farmer)
                .get::<GoodsInventory>()
                .unwrap()
                .amount(Good::Wheat),
            2,
            "a failed loaded return must retain the basket and retry this shift"
        );
        app.world_mut()
            .entity_mut(farmer)
            .get_mut::<PlayerPosition>()
            .unwrap()
            .0 = farm_entrance;
        app.update();

        assert_eq!(
            app.world()
                .entity(farmer)
                .get::<GoodsInventory>()
                .unwrap()
                .amount(Good::Wheat),
            0
        );
        assert_eq!(
            app.world()
                .entity(farm)
                .get::<GoodsInventory>()
                .unwrap()
                .amount(Good::Wheat),
            2
        );
        assert_eq!(
            app.world()
                .entity(hall)
                .get::<GoodsInventory>()
                .unwrap()
                .amount(Good::Wheat),
            0,
            "the farmer must never bypass the Farmstead to sell at the hall"
        );
        assert!(app.world().entity(farmer).get::<FarmerRoutine>().is_some());
        assert!(app.world().entity(farmer).get::<WorkerOffDuty>().is_none());
        assert!(app
            .world()
            .entity(farmer)
            .get::<MarketCollectionRoutine>()
            .is_none());

        // The same loop keeps running until the shift boundary. At that point
        // it releases the employed villager to evening priorities while
        // preserving partial labour toward the next Wheat.
        app.world_mut()
            .entity_mut(farmer)
            .remove::<WorkplaceDoorTransit>()
            .remove::<BuildingDoorUse>()
            .remove::<MoveTarget>()
            .get_mut::<FarmerRoutine>()
            .unwrap()
            .phase = FarmerPhase::ReturningToFarmstead;
        {
            let mut time = app.world_mut().get_mut::<WorldTime>(clock).unwrap();
            time.seconds_in_cycle = time.day_duration * 0.8;
        }
        app.update();
        assert!(app.world().entity(farmer).get::<FarmerRoutine>().is_none());
        assert!(app.world().entity(farmer).get::<WorkerOffDuty>().is_some());
        assert!(app
            .world()
            .entity(farmer)
            .get::<FarmerHarvestProgress>()
            .is_some());
    }

    #[test]
    fn a_fisher_fills_a_two_food_batch_at_the_pier_then_deposits_at_the_hut() {
        let mut app = App::new();
        app.init_resource::<Time>();
        app.init_resource::<SettlementEconomyRuntime>();
        app.insert_resource(WorldTerrain::default());
        app.add_systems(Update, run_fishing_routines);

        let hall = app
            .world_mut()
            .spawn(Settlement {
                name: "Nethercove".to_string(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 1,
                treasury: 0,
            })
            .id();
        app.world_mut().spawn((
            WorldTime::new_default(),
            shared::components::TimeWarp::clamped(1.0),
        ));
        let hut_position = Vec3::new(20.0, 2.0, 0.0);
        let hut = app
            .world_mut()
            .spawn((
                SettlementBuilding {
                    kind: SettlementBuildingKind::FishermansHut,
                    settlement: "Nethercove".to_string(),
                    owner: Some("Ada".to_string()),
                    quality: 0.67,
                    workers: vec!["Ada".to_string()],
                },
                PlayerPosition(hut_position),
                PlayerRotation(0.0),
                GoodsInventory::new(shared::economy::capacity::FISHERMANS_HUT),
            ))
            .id();
        let pier_position = Vec3::new(20.0, 0.0, 8.0);
        let pier = app
            .world_mut()
            .spawn((
                FishingPier {
                    settlement: "Nethercove".to_string(),
                    fishermans_hut: hut_position,
                    quality: 0.67,
                },
                PlayerPosition(pier_position),
                PlayerRotation(0.0),
            ))
            .id();
        let (_, fish_spot) = fishing_deck_points(pier_position, 0.0);
        let fisher = app
            .world_mut()
            .spawn((
                CharacterKind::Villager,
                CharacterName("Ada".to_string()),
                VillagerIntent::Resident { settlement: hall },
                PlayerPosition(fish_spot),
                PlayerRotation(0.0),
                CharacterActivity::Fishing,
                GoodsInventory::new(shared::economy::capacity::VILLAGER),
                FishingRoutine {
                    hut,
                    pier,
                    hall,
                    catch_seconds: fisher_seconds_per_food(0.67),
                    production_day: u32::MAX,
                    produced_today: 0,
                    phase: FishingPhase::Fishing,
                },
            ))
            .id();

        app.update();
        assert_eq!(
            app.world()
                .entity(fisher)
                .get::<GoodsInventory>()
                .unwrap()
                .amount(Good::Food),
            0,
            "partial catch progress must keep the visible fishing animation active"
        );
        {
            let mut entity = app.world_mut().entity_mut(fisher);
            let mut routine = entity.get_mut::<FishingRoutine>().unwrap();
            routine.catch_seconds = fisher_seconds_per_food(0.67) * 2.0;
        }
        app.update();
        assert_eq!(
            app.world()
                .entity(fisher)
                .get::<GoodsInventory>()
                .unwrap()
                .amount(Good::Food),
            2,
            "the fisher should stay at the pier until the carrying batch is full"
        );

        let entrance = SettlementBuildingKind::FishermansHut.entrance_position(hut_position, 0.0);
        app.world_mut()
            .entity_mut(fisher)
            .remove::<MoveTarget>()
            .remove::<TravelRoute>()
            .remove::<PierTraversal>()
            .get_mut::<FishingRoutine>()
            .unwrap()
            .phase = FishingPhase::ReturningToHut;
        app.world_mut()
            .entity_mut(fisher)
            .get_mut::<PlayerPosition>()
            .unwrap()
            .0 = entrance;
        app.update();
        assert_eq!(
            app.world()
                .entity(hut)
                .get::<GoodsInventory>()
                .unwrap()
                .amount(Good::Food),
            2
        );
        assert_eq!(
            app.world()
                .entity(fisher)
                .get::<GoodsInventory>()
                .unwrap()
                .amount(Good::Food),
            0
        );
    }

    #[test]
    fn completed_tree_interactions_keep_producing_without_a_daily_cap() {
        let mut app = App::new();
        app.init_resource::<Time>();
        app.insert_resource(WorldTerrain::default());
        app.add_systems(Update, run_lumberjack_routines);

        let hall = app
            .world_mut()
            .spawn(Settlement {
                name: "Pinewatch".to_string(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 1,
                treasury: 0,
            })
            .id();
        app.world_mut().spawn((
            WorldTime::new_default(),
            shared::components::TimeWarp::clamped(1.0),
        ));
        let hut_position = Vec3::new(20.0, 2.0, 0.0);
        let hut = app
            .world_mut()
            .spawn((
                SettlementBuilding {
                    kind: SettlementBuildingKind::LumberjackHut,
                    settlement: "Pinewatch".to_string(),
                    owner: Some("Ada".to_string()),
                    quality: 0.9,
                    workers: vec!["Ada".to_string()],
                },
                PlayerPosition(hut_position),
                PlayerRotation(0.0),
                GoodsInventory::new(shared::economy::capacity::LUMBERJACK_HUT),
            ))
            .id();
        let woodcutter = app
            .world_mut()
            .spawn((
                CharacterKind::Villager,
                CharacterName("Ada".to_string()),
                VillagerIntent::Resident { settlement: hall },
                PlayerPosition(Vec3::new(30.0, 2.0, 0.0)),
                PlayerRotation(0.0),
                CharacterActivity::Chopping,
                GoodsInventory::new(shared::economy::capacity::VILLAGER),
                LumberjackRoutine {
                    hut,
                    hall,
                    cycle: 0,
                    failed_tree_routes: 0,
                    chop_seconds: CHOP_SECONDS,
                    production_day: u32::MAX,
                    produced_today: 0,
                    phase: LumberjackPhase::Chopping,
                },
            ))
            .id();

        app.update();
        assert_eq!(
            app.world()
                .entity(woodcutter)
                .get::<GoodsInventory>()
                .unwrap()
                .amount(Good::Wood),
            3,
            "Wood is created only when the chopping interaction completes"
        );
        let entrance = SettlementBuildingKind::LumberjackHut.entrance_position(hut_position, 0.0);
        app.world_mut()
            .entity_mut(woodcutter)
            .insert(NavigationRouteFailed { goal: entrance });
        app.update();
        assert!(
            app.world()
                .entity(woodcutter)
                .get::<NavigationRouteFailed>()
                .is_none(),
            "a failed loaded return route must be retried instead of disabling the woodcutter"
        );
        assert_eq!(
            app.world()
                .entity(woodcutter)
                .get::<GoodsInventory>()
                .unwrap()
                .amount(Good::Wood),
            3,
            "route recovery must retain physically carried Wood"
        );
        app.world_mut()
            .entity_mut(woodcutter)
            .get_mut::<PlayerPosition>()
            .unwrap()
            .0 = entrance;
        app.update();
        assert_eq!(
            app.world()
                .entity(hut)
                .get::<GoodsInventory>()
                .unwrap()
                .amount(Good::Wood),
            3
        );

        app.world_mut()
            .entity_mut(woodcutter)
            .remove::<WorkplaceDoorTransit>()
            .remove::<BuildingDoorUse>()
            .remove::<MoveTarget>();
        {
            let mut entity = app.world_mut().entity_mut(woodcutter);
            let mut routine = entity.get_mut::<LumberjackRoutine>().unwrap();
            routine.chop_seconds = CHOP_SECONDS;
            routine.phase = LumberjackPhase::Chopping;
        }
        app.update();
        let total_wood = app
            .world()
            .entity(hut)
            .get::<GoodsInventory>()
            .unwrap()
            .amount(Good::Wood)
            + app
                .world()
                .entity(woodcutter)
                .get::<GoodsInventory>()
                .unwrap()
                .amount(Good::Wood);
        assert_eq!(total_wood, 6);
        assert!(
            total_wood > 4,
            "a legacy four-Wood daily ceiling must not stop a valid second tree interaction"
        );
    }

    #[test]
    fn a_wealthy_owner_leaves_daily_work_only_when_payroll_and_a_replacement_are_ready() {
        let mut app = App::new();
        app.add_systems(
            Update,
            (run_business_payroll_and_owner_leisure, fill_vacancies).chain(),
        );
        app.world_mut().spawn({
            let mut clock = WorldTime::new_default();
            clock.day = 2;
            clock
        });
        let settlement = app
            .world_mut()
            .spawn(Settlement {
                name: "Richford".to_string(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 2,
                treasury: 0,
            })
            .id();
        let business = app
            .world_mut()
            .spawn((
                SettlementBuilding {
                    kind: SettlementBuildingKind::LumberjackHut,
                    settlement: "Richford".to_string(),
                    owner: Some("Ada".to_string()),
                    quality: 0.8,
                    workers: vec!["Ada".to_string()],
                },
                PlayerPosition(Vec3::new(10.0, 0.0, 0.0)),
                BusinessAccount {
                    cash: 20 * PENNIES_PER_COIN,
                    wage_arrears: 0,
                    last_payroll_day: 1,
                },
                BusinessWagePolicy::default(),
            ))
            .id();
        let owner = app
            .world_mut()
            .spawn((
                CharacterName("Ada".to_string()),
                VillagerIntent::Resident { settlement },
                PlayerPosition(Vec3::new(10.0, 0.0, 0.0)),
                Wallet::new(WEALTHY_OWNER_MONEY),
                Occupation(Some("Woodcutter".to_string())),
                WorkStatus::Employed,
            ))
            .id();
        let replacement = app
            .world_mut()
            .spawn((
                CharacterName("Bea".to_string()),
                VillagerIntent::Resident { settlement },
                PlayerPosition(Vec3::ZERO),
                Wallet::default(),
                Occupation::default(),
                WorkStatus::LookingForWork,
            ))
            .id();

        app.update();

        let world = app.world();
        let building = world.get::<SettlementBuilding>(business).unwrap();
        assert_eq!(building.workers, ["Bea"]);
        assert_eq!(
            *world.get::<WorkStatus>(owner).unwrap(),
            WorkStatus::Chilling
        );
        assert_eq!(world.get::<Occupation>(owner).unwrap().0, None);
        assert_eq!(
            *world.get::<WorkStatus>(replacement).unwrap(),
            WorkStatus::Employed
        );
        assert_eq!(
            world.get::<Occupation>(replacement).unwrap().0.as_deref(),
            Some("Woodcutter")
        );
        let account = world.get::<BusinessAccount>(business).unwrap();
        assert!(
            account.cash >= 2 * FOUNDING_DAILY_WAGE,
            "the owner must leave a real payroll reserve behind"
        );
    }

    /// A short headless soak of the same world the player watches, with every
    /// village clock running at 100x. This is deliberately not a mocked
    /// production calculation: villagers still migrate, request permits, walk,
    /// build, enter workplaces, animate work and physically haul each load.
    #[test]
    fn hundred_x_world_runs_complete_visible_supply_loops() {
        use crate::player::hero::step_units;
        use crate::world::pathfinding::PathfindingBudgetSettings;
        use shared::components::{CharacterKind, TimeWarp};
        use shared::region::RegionCoord;

        let mut app = App::new();
        app.init_resource::<Time>();
        app.init_resource::<VillageClock>();
        app.init_resource::<SettlementEconomyRuntime>();
        app.init_resource::<PublishedTerrainDeltas>();
        app.init_resource::<crate::world::village_roads::VillageRoadGraph>();
        app.insert_resource(PathfindingBudgetSettings {
            max_requests_per_tick: 8,
            ..default()
        });
        app.insert_resource(WorldTerrain::default());
        app.add_systems(
            Update,
            (
                claim_settlement_hall_obstacles,
                tag_villager_intent,
                seek_settlement,
                arrive_at_settlement,
                recount_residents,
                consider_permits,
                run_construction_material_logistics,
                advance_construction,
                ensure_farm_fields,
                crate::world::village_roads::plan_requested_roads,
                fill_vacancies,
                ensure_households,
                assign_households,
                (
                    run_household_schedules,
                    run_workplace_door_transits,
                    crate::world::village_roads::build_village_roads,
                    assign_farmer_routines,
                    assign_lumberjack_routines,
                    run_farmer_routines,
                    run_lumberjack_routines,
                    sync_carried_load,
                )
                    .chain(),
                // Match the live server's navigation phase. This is
                // intentionally after decisions: changed destinations are
                // queued, planned around solid buildings, then moved. The
                // following tick observes arrivals.
                crate::world::village_roads::rebuild_village_road_graph,
                crate::world::village_roads::queue_villager_travel_routes,
                crate::world::village_roads::plan_villager_travel_routes,
                step_units,
            )
                .chain(),
        );

        let hall_position = {
            let terrain = app.world().resource::<WorldTerrain>();
            let water = terrain.water_level().unwrap_or(f32::NEG_INFINITY);
            (0..400)
                .find_map(|step| {
                    let x = step as f32 * 40.0;
                    let height = terrain.get_height(x, 0.0);
                    (height > water + FREEBOARD + 4.0 && slope_at(terrain, x, 0.0) < 0.1)
                        .then_some(Vec3::new(x, height, 0.0))
                })
                .expect("the test map must contain dry, flat settlement ground")
        };
        app.world_mut().spawn((
            Settlement {
                name: "Fast Yewcrag".to_string(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 0,
                treasury: 0,
            },
            GoodsInventory::new(shared::economy::capacity::HALL),
            PlayerPosition(hall_position),
            PlayerRotation(0.0),
            WorldTime::new_default(),
            TimeWarp::clamped(100.0),
        ));
        for index in 0..3 {
            let spot = hall_position + Vec3::new(40.0 + index as f32 * 5.0, 0.0, 25.0);
            app.world_mut().spawn((
                CharacterName(format!("FastVillager{index}")),
                CharacterKind::Villager,
                CharacterAttributes::default(),
                PlayerPosition(spot),
                PlayerRotation(0.0),
                RegionCoord::from_world_pos(spot),
            ));
        }

        let step = std::time::Duration::from_secs_f32(1.0 / 60.0);
        let mut saw_indoors = false;
        let mut saw_farming = false;
        let mut saw_chopping = false;
        let mut saw_wheat_carried = false;
        let mut saw_wood_carried = false;
        let mut saw_partially_supplied_site = false;
        let mut saw_fully_supplied_site = false;
        // Twenty-four wall-clock seconds represent forty simulated minutes.
        // Material delivery adds several real journeys before work can begin.
        for _ in 0..(60 * 24) {
            app.world_mut().resource_mut::<Time>().advance_by(step);
            app.update();
            let world = app.world_mut();
            saw_indoors |= world
                .query::<&CharacterActivity>()
                .iter(world)
                .any(|activity| *activity == CharacterActivity::Indoors);
            saw_farming |= world
                .query::<&CharacterActivity>()
                .iter(world)
                .any(|activity| *activity == CharacterActivity::Farming);
            saw_chopping |= world
                .query::<&CharacterActivity>()
                .iter(world)
                .any(|activity| *activity == CharacterActivity::Chopping);
            saw_wheat_carried |= world
                .query::<&CarriedLoad>()
                .iter(world)
                .any(|load| load.good == Some(Good::Wheat) && load.amount > 0);
            saw_wood_carried |= world
                .query::<&CarriedLoad>()
                .iter(world)
                .any(|load| load.good == Some(Good::Wood) && load.amount > 0);
            for (site, inventory) in world
                .query::<(&shared::components::ConstructionSite, &GoodsInventory)>()
                .iter(world)
            {
                let required = site.kind.construction_wood_required();
                let delivered = inventory.amount(Good::Wood);
                saw_partially_supplied_site |= delivered > 0 && delivered < required;
                saw_fully_supplied_site |= required > 0 && delivered >= required;
            }
        }

        let mut world = std::mem::take(&mut *app.world_mut());
        let built: HashSet<_> = world
            .query::<&SettlementBuilding>()
            .iter(&world)
            .map(|building| building.kind)
            .collect();
        let pending_sites: Vec<_> = world
            .query::<&UnderConstruction>()
            .iter(&world)
            .map(|site| (site.kind, site.stage, site.position, site.builder))
            .collect();
        let supplier_states: Vec<_> = world
            .query::<(
                Entity,
                &PlayerPosition,
                Option<&MoveTarget>,
                Option<&crate::world::village_roads::NavigationRoutePending>,
                Option<&ConstructionMaterialRoutine>,
                &GoodsInventory,
            )>()
            .iter(&world)
            .map(|(entity, position, target, pending, routine, inventory)| {
                (
                    entity,
                    position.0,
                    target.map(|target| target.0),
                    pending.map(|pending| (pending.goal, pending.exhausted())),
                    routine.map(|routine| format!("{:?}", routine.phase)),
                    inventory.amount(Good::Wood),
                )
            })
            .collect();
        assert!(
            built.contains(&SettlementBuildingKind::Farmstead),
            "built={built:?} pending={pending_sites:?} suppliers={supplier_states:?}"
        );
        assert!(
            built.contains(&SettlementBuildingKind::LumberjackHut),
            "{built:?}"
        );
        assert!(built.contains(&SettlementBuildingKind::House), "{built:?}");
        assert_eq!(world.query::<&FarmField>().iter(&world).count(), 2);
        let households: Vec<_> = world.query::<&Household>().iter(&world).collect();
        assert_eq!(households.len(), 1);
        assert_eq!(households[0].residents.len(), 3);
        assert!(
            households[0].residents.len()
                <= SettlementBuildingKind::House.housing_capacity() as usize
        );
        let people_states: Vec<_> = world
            .query::<(
                &CharacterName,
                &VillagerIntent,
                &CharacterActivity,
                &PlayerPosition,
                Option<&MoveTarget>,
                Option<&FarmerRoutine>,
                Option<&LumberjackRoutine>,
            )>()
            .iter(&world)
            .map(
                |(name, intent, activity, position, target, farmer, lumberjack)| {
                    (
                        name.0.clone(),
                        format!("{intent:?}"),
                        *activity,
                        position.0,
                        target.map(|target| target.0),
                        farmer.map(|routine| format!("{:?}", routine.phase)),
                        lumberjack.map(|routine| format!("{:?}", routine.phase)),
                    )
                },
            )
            .collect();
        assert!(
            saw_indoors,
            "workers should disappear into their workplaces: {people_states:?}"
        );
        assert!(saw_farming, "farm work must remain observable at 100x");
        assert!(saw_chopping, "tree work must remain observable at 100x");
        assert!(saw_wheat_carried, "wheat must be physically hauled at 100x");
        assert!(saw_wood_carried, "wood must be physically hauled at 100x");
        assert!(
            saw_partially_supplied_site,
            "construction wood must accumulate at a worksite at 100x"
        );
        assert!(
            saw_fully_supplied_site,
            "a worksite must receive its complete wood requirement at 100x"
        );

        let (wheat, wood) = world.query::<&GoodsInventory>().iter(&world).fold(
            (0, 0),
            |(wheat, wood), inventory| {
                (
                    wheat + inventory.amount(Good::Wheat),
                    wood + inventory.amount(Good::Wood),
                )
            },
        );
        assert!(wheat > 0, "the accelerated farm loop must retain wheat");
        assert!(wood > 0, "the accelerated lumber loop must retain wood");
        assert!(
            world
                .query::<&CharacterAttributes>()
                .iter(&world)
                .any(|attributes| attributes.physique() > 10),
            "a successful farm cycle must train physique even at 100x"
        );
        assert!(world
            .query::<&GoodsInventory>()
            .iter(&world)
            .all(|inventory| inventory.used_bulk() <= inventory.bulk_capacity()));
    }

    /// The whole loop, driven by the real systems.
    ///
    /// This is the acceptance test the design was written against, run as code
    /// rather than as a person watching: found a settlement, put three people
    /// on the map some way off, and let the server do everything else. Nothing
    /// here assigns a resident, an occupation or a plot.
    ///
    /// It runs the ACTUAL scheduled systems, including `step_units`, so the
    /// walking, the arrival radius and the permit clock are all under test. A
    /// test that called the decision functions directly would pass while the
    /// villagers stood still forever.
    #[test]
    fn three_villagers_settle_and_build_a_village_unaided() {
        use crate::player::hero::step_units;
        use shared::components::CharacterKind;
        use shared::region::RegionCoord;

        let mut app = App::new();
        app.init_resource::<Time>();
        app.init_resource::<VillageClock>();
        app.init_resource::<SettlementEconomyRuntime>();
        app.init_resource::<PublishedTerrainDeltas>();
        app.insert_resource(WorldTerrain::default());
        app.add_systems(
            Update,
            (
                tag_villager_intent,
                seek_settlement,
                step_units,
                arrive_at_settlement,
                recount_residents,
                consider_permits,
                run_construction_material_logistics,
                advance_construction,
                ensure_farm_fields,
                crate::world::village_roads::plan_requested_roads,
                fill_vacancies,
                ensure_households,
                assign_households,
                (
                    run_household_schedules,
                    run_workplace_door_transits,
                    crate::world::village_roads::build_village_roads,
                    assign_farmer_routines,
                    assign_lumberjack_routines,
                    run_farmer_routines,
                    run_lumberjack_routines,
                    sync_carried_load,
                )
                    .chain(),
            )
                .chain(),
        );
        app.world_mut().spawn(WorldTime::new_default());

        // A hall on DRY, buildable land. Searched for rather than hardcoded,
        // because the origin of this map happens to be underwater -- and a test
        // that founded there would fail for a reason that has nothing to do
        // with villager autonomy.
        let hall_position = {
            let terrain = app.world().resource::<WorldTerrain>();
            let water = terrain.water_level().unwrap_or(f32::NEG_INFINITY);
            (0..400)
                .find_map(|step| {
                    let x = step as f32 * 40.0;
                    let height = terrain.get_height(x, 0.0);
                    (height > water + FREEBOARD + 4.0 && slope_at(terrain, x, 0.0) < 0.1)
                        .then_some(Vec3::new(x, height, 0.0))
                })
                .expect("the map has dry, flat ground somewhere along the x axis")
        };
        app.world_mut().spawn((
            Settlement {
                name: "Yewcrag".to_string(),
                tier: shared::components::SettlementTier::Hamlet,
                residents: 0,
                treasury: 0,
            },
            GoodsInventory::new(shared::economy::capacity::HALL),
            PlayerPosition(hall_position),
            PlayerRotation(0.0),
        ));

        // Three people, dropped well clear of the hall so they have to walk.
        for index in 0..3 {
            let offset = Vec3::new(40.0 + index as f32 * 5.0, 0.0, 25.0);
            let spot = hall_position + offset;
            app.world_mut().spawn((
                CharacterName(format!("Villager{index}")),
                CharacterKind::Villager,
                PlayerPosition(spot),
                PlayerRotation(0.0),
                RegionCoord::from_world_pos(spot),
            ));
        }

        // Twenty minutes of simulated time. It used to be forty seconds, which
        // was ample when a permit became a building on a timer. Now somebody has
        // to WALK to each plot -- up to 60 m at 3.2 m/s -- and then spend ten
        // seconds raising it, so a village of three buildings needs roughly
        // much longer now that builders must chop and carry every wood bundle.
        // The remaining time lets the newly employed workers complete observed
        // work cycles after the last material-heavy building finishes.
        let step = std::time::Duration::from_secs_f32(1.0 / 60.0);
        let mut saw_indoors = false;
        let mut saw_chopping = false;
        let mut saw_farming = false;
        let mut saw_carrying = false;
        let mut saw_wheat_carrying = false;
        for _ in 0..(60 * 1_200) {
            app.world_mut().resource_mut::<Time>().advance_by(step);
            app.update();
            let world = app.world_mut();
            saw_indoors |= world
                .query::<&CharacterActivity>()
                .iter(world)
                .any(|activity| *activity == CharacterActivity::Indoors);
            saw_chopping |= world
                .query::<&CharacterActivity>()
                .iter(world)
                .any(|activity| *activity == CharacterActivity::Chopping);
            saw_farming |= world
                .query::<&CharacterActivity>()
                .iter(world)
                .any(|activity| *activity == CharacterActivity::Farming);
            saw_carrying |= world
                .query::<&CarriedLoad>()
                .iter(world)
                .any(|load| !load.is_empty());
            saw_wheat_carrying |= world
                .query::<&CarriedLoad>()
                .iter(world)
                .any(|load| load.good == Some(Good::Wheat) && load.amount > 0);
        }

        let mut world = std::mem::take(&mut *app.world_mut());

        let settlement = world
            .query::<&Settlement>()
            .iter(&world)
            .next()
            .cloned()
            .expect("the settlement still exists");
        assert_eq!(
            settlement.residents, 3,
            "all three walked in and joined of their own accord"
        );

        let homes: Vec<String> = world
            .query::<&Residence>()
            .iter(&world)
            .map(|home| home.0.clone())
            .collect();
        assert_eq!(homes.len(), 3, "each of them is on record as living there");
        assert!(homes.iter().all(|home| home == "Yewcrag"));

        let built: HashSet<SettlementBuildingKind> = world
            .query::<&SettlementBuilding>()
            .iter(&world)
            .map(|building| building.kind)
            .collect();
        assert!(
            built.contains(&SettlementBuildingKind::Farmstead),
            "a farm went up first, because food comes first: {built:?}"
        );
        assert!(
            built.contains(&SettlementBuildingKind::LumberjackHut),
            "then timber: {built:?}"
        );
        assert!(
            built.contains(&SettlementBuildingKind::House),
            "then somewhere to live: {built:?}"
        );
        let households: Vec<_> = world.query::<&Household>().iter(&world).collect();
        assert_eq!(households.len(), 1, "the completed cabin needs a household");
        assert_eq!(
            households[0].residents.len(),
            3,
            "all three residents should have a designated bed"
        );
        assert!(
            households[0].residents.len()
                <= SettlementBuildingKind::House.housing_capacity() as usize
        );

        // Every building belongs to a named person. A village of anonymous
        // structures is exactly what this design refuses.
        let ownerless = world
            .query::<&SettlementBuilding>()
            .iter(&world)
            .filter(|building| building.owner.is_none())
            .count();
        assert_eq!(ownerless, 0, "somebody applied for every one of them");

        // THREE buildings, THREE owners. Not incidental: if one villager ends
        // up holding the whole village the other two are decoration, and
        // "the wheat farm stopped because the farmer died" stops meaning
        // anything -- one death would take everything with it.
        let owners: HashSet<String> = world
            .query::<&SettlementBuilding>()
            .iter(&world)
            .filter_map(|building| building.owner.clone())
            .collect();
        assert_eq!(
            owners.len(),
            3,
            "each resident has a stake of their own: {owners:?}"
        );

        // Nothing was built in the water. Flat ground is where a village
        // wants to build and a lake bed is the flattest ground there is, so
        // this is the failure the siting rule exists to prevent.
        let water = world
            .resource::<WorldTerrain>()
            .water_level()
            .unwrap_or(f32::NEG_INFINITY);
        let drowned: Vec<_> = world
            .query::<(&SettlementBuilding, &PlayerPosition)>()
            .iter(&world)
            .filter(|(_, at)| at.0.y < water)
            .map(|(building, at)| (building.kind, at.0))
            .collect();
        assert!(
            drowned.is_empty(),
            "nothing was built in the lake: {drowned:?}"
        );

        // Housing approval is free, while the two business permits move coin
        // from their applicants into the public treasury.
        assert!(
            settlement.treasury > 0,
            "business permits must fund the moot"
        );
        let wallet_total = world
            .query::<&Wallet>()
            .iter(&world)
            .map(|wallet| wallet.balance())
            .sum::<u64>();
        assert_eq!(
            wallet_total + settlement.treasury,
            3 * shared::economy::STARTING_VILLAGER_MONEY,
            "permit approval must transfer rather than create or destroy coin"
        );

        // Somebody WALKED to each plot. If construction still completed on a
        // timer alone this would pass with the builders standing at the hall,
        // so it checks the distance from the hall rather than merely that
        // buildings exist.
        let sites: Vec<Vec3> = world
            .query::<(&SettlementBuilding, &PlayerPosition)>()
            .iter(&world)
            .map(|(_, at)| at.0)
            .collect();
        assert!(
            sites.iter().all(|at| at.distance(hall_position) > 8.0),
            "buildings should stand out on their own plots, not on the hall: {sites:?}"
        );

        // The ground under every building was levelled and published. An empty
        // map here means the terrain edit never happened or never left the
        // server, and the client would draw buildings floating over a hillside.
        let published = world.resource::<PublishedTerrainDeltas>().by_chunk.len();
        assert!(
            published > 0,
            "clearing a plot must publish a terrain delta for its chunk"
        );

        // Every building claims its plot, which is what stops trees being drawn
        // inside it and what makes it a navigation obstacle.
        let claimed = world
            .query::<(&SettlementBuilding, &shared::building::PlacedBuilding)>()
            .iter(&world)
            .count();
        assert_eq!(claimed, 3, "each building must claim its ground");

        // Work exists and named people hold it. A Farmstead seats two and a
        // Lumberjack Hut one, so with three residents every position that can
        // be filled is filled -- and the House seats nobody, which is the point
        // of homes and workplaces being different things.
        let staffed: usize = world
            .query::<&SettlementBuilding>()
            .iter(&world)
            .map(|building| building.workers.len())
            .sum();
        assert!(
            staffed >= 2,
            "residents should have taken the vacant positions, got {staffed}"
        );

        let hut_points: Vec<_> = world
            .query::<(
                Entity,
                &SettlementBuilding,
                &PlayerPosition,
                &PlayerRotation,
            )>()
            .iter(&world)
            .filter(|(_, building, _, _)| building.kind == SettlementBuildingKind::LumberjackHut)
            .map(|(entity, building, position, rotation)| {
                (
                    entity,
                    building.kind.entrance_position(position.0, rotation.0),
                )
            })
            .collect();
        let routine_states: Vec<_> = world
            .query::<(
                &CharacterName,
                &LumberjackRoutine,
                &CharacterActivity,
                &PlayerPosition,
                Option<&MoveTarget>,
            )>()
            .iter(&world)
            .map(|(name, routine, activity, position, target)| {
                let door = hut_points
                    .iter()
                    .find(|(entity, _)| *entity == routine.hut)
                    .map(|(_, point)| *point);
                (
                    name.0.clone(),
                    format!("{:?}", routine.phase),
                    *activity,
                    position.0,
                    target.map(|target| target.0),
                    door,
                )
            })
            .collect();
        assert!(
            saw_indoors,
            "the woodcutter should rest inside their hut: {routine_states:?}"
        );
        assert!(
            saw_chopping,
            "the woodcutter should visibly work a real tree: {routine_states:?}"
        );
        assert!(
            saw_farming,
            "a farmer should visibly work the farmstead's wheat field"
        );
        assert!(
            saw_carrying,
            "harvested wood should travel in a bounded carried load: {routine_states:?}"
        );
        assert!(
            saw_wheat_carrying,
            "harvested wheat should travel in a bounded carried load"
        );
        let total_wood: u32 = world
            .query::<&GoodsInventory>()
            .iter(&world)
            .map(|inventory| inventory.amount(Good::Wood))
            .sum();
        assert!(total_wood > 0, "physical work should create stored wood");
        let total_wheat: u32 = world
            .query::<&GoodsInventory>()
            .iter(&world)
            .map(|inventory| inventory.amount(Good::Wheat))
            .sum();
        assert!(
            total_wheat > 0,
            "physical farm work should create stored wheat"
        );
        let field_count = world.query::<&FarmField>().iter(&world).count();
        assert_eq!(
            field_count, 2,
            "each farmstead should create two wheat fields"
        );
        let overfilled: Vec<_> = world
            .query::<&GoodsInventory>()
            .iter(&world)
            .filter(|inventory| inventory.used_bulk() > inventory.bulk_capacity())
            .map(|inventory| (inventory.used_bulk(), inventory.bulk_capacity()))
            .collect();
        assert!(
            overfilled.is_empty(),
            "no inventory may exceed capacity: {overfilled:?}"
        );
        let titles: Vec<String> = world
            .query::<&Occupation>()
            .iter(&world)
            .filter_map(|job| job.0.clone())
            .collect();
        assert!(
            titles.iter().any(|t| t == "Farmer"),
            "somebody works the farm: {titles:?}"
        );

        // Ground quality was sampled where each building stands, not defaulted.
        let qualities: Vec<f32> = world
            .query::<&SettlementBuilding>()
            .iter(&world)
            .map(|building| building.quality)
            .collect();
        assert!(
            qualities.iter().all(|q| (0.0..=1.0).contains(q)),
            "quality must be a real 0..1 sample: {qualities:?}"
        );

        // A workplace is a bounded physical store, not an infinite production
        // counter. The worker loop added next must have somewhere finite to
        // deposit its output, and every completed building must receive it.
        let stores: Vec<(SettlementBuildingKind, u32)> = world
            .query::<(&SettlementBuilding, &shared::economy::GoodsInventory)>()
            .iter(&world)
            .map(|(building, inventory)| (building.kind, inventory.bulk_capacity()))
            .collect();
        assert_eq!(stores.len(), 3, "every completed building needs storage");
        assert!(stores
            .iter()
            .all(|(kind, capacity)| { *capacity == kind.storage_bulk_capacity() }));

        // Where the test actually founded, so a failure elsewhere is diagnosable.
        println!("founded at {hall_position:?}, waterline {water}");
        println!("plots {sites:?}");
        println!("delta chunks published: {published}");
        println!("occupations: {titles:?}  ground quality: {qualities:?}");
    }

    #[test]
    fn houses_sit_closer_to_the_hall_than_workplaces() {
        let (house_min, house_max) = SettlementBuildingKind::House.preferred_ring();
        let (farm_min, farm_max) = SettlementBuildingKind::Farmstead.preferred_ring();
        let (wood_min, wood_max) = SettlementBuildingKind::LumberjackHut.preferred_ring();
        assert!(house_min < farm_min);
        assert!(house_min < wood_min);
        assert!(house_max < farm_max);
        assert!(house_max < wood_max);
    }
}
