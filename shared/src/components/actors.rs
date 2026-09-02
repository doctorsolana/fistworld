use bevy::prelude::*;
use lightyear::prelude::PeerId;
use serde::{Deserialize, Serialize};

/// Marker component for player entities.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Player {
    pub client_id: PeerId,
}

/// Player progression data (replicated + persisted).
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct PlayerProgression {
    /// Main level (starts at 0)
    pub level: u32,
    /// Prestige count (starts at 0)
    pub prestige: u32,
    /// Reputation can be negative or positive
    pub reputation: i32,
    /// Persisted physique value for the player's embodied character.
    ///
    /// The field keeps its old wire/profile name so existing saves can be
    /// migrated without confusing stamina with a second progression system.
    pub stamina: u32,
    /// Persisted intelligence value for the player's embodied character.
    pub intelligence: u32,
    /// Persisted charm value for the player's embodied character.
    pub charm: u32,
}

/// The three broad aptitudes shared by every embodied character.
///
/// These are deliberately aptitudes rather than job-specific XP. A future
/// bakery can ask for an intelligent worker and a future shop can value charm
/// without every person carrying a sparse map of every trade in the game.
/// Values are only changed through the methods below, which enforce the hard
/// 0..=100 gameplay range at the authoritative data boundary.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct CharacterAttributes {
    physique: u8,
    intelligence: u8,
    charm: u8,
}

impl CharacterAttributes {
    pub const MAX: u8 = 100;

    pub fn new(physique: u8, intelligence: u8, charm: u8) -> Self {
        Self {
            physique: physique.min(Self::MAX),
            intelligence: intelligence.min(Self::MAX),
            charm: charm.min(Self::MAX),
        }
    }

    /// Stable starting variation for generated people (8..=20 in each stat).
    pub fn from_seed(seed: u64) -> Self {
        fn mixed(mut value: u64) -> u64 {
            value ^= value >> 30;
            value = value.wrapping_mul(0xbf58_476d_1ce4_e5b9);
            value ^= value >> 27;
            value = value.wrapping_mul(0x94d0_49bb_1331_11eb);
            value ^ (value >> 31)
        }
        let physique = 8 + (mixed(seed) % 13) as u8;
        let intelligence = 8 + (mixed(seed ^ 0xa076_1d64_78bd_642f) % 13) as u8;
        let charm = 8 + (mixed(seed ^ 0xe703_7ed1_a0b4_28db) % 13) as u8;
        Self::new(physique, intelligence, charm)
    }

    pub const fn physique(self) -> u8 {
        self.physique
    }

    pub const fn intelligence(self) -> u8 {
        self.intelligence
    }

    pub const fn charm(self) -> u8 {
        self.charm
    }

    /// Returns the amount actually gained (zero at the cap).
    pub fn train_physique(&mut self, amount: u8) -> u8 {
        let before = self.physique;
        self.physique = self.physique.saturating_add(amount).min(Self::MAX);
        self.physique - before
    }

    pub fn train_intelligence(&mut self, amount: u8) -> u8 {
        let before = self.intelligence;
        self.intelligence = self.intelligence.saturating_add(amount).min(Self::MAX);
        self.intelligence - before
    }

    pub fn train_charm(&mut self, amount: u8) -> u8 {
        let before = self.charm;
        self.charm = self.charm.saturating_add(amount).min(Self::MAX);
        self.charm - before
    }
}

impl Default for CharacterAttributes {
    fn default() -> Self {
        Self::new(10, 10, 10)
    }
}

/// The player's embodied character in the world (one per player, by design —
/// "you ARE somewhere"). Server-authoritative: the server owns its position
/// and steps it toward move targets; clients only send intent.
///
/// Deliberately NOT `Player`: the commander entity is a bodiless camera focus
/// that several server systems (view sync, autosave, interest anchoring)
/// query by `Player`, and a second match would fight them.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct Hero {
    /// Peer that owns this hero.
    pub owner: PeerId,
}

/// The small vessel that carries a newly created hero into the world.
///
/// Ownership and command authority live in the accompanying [`CommandedBy`]
/// component, exactly as they do for a hero or retinue.  Keeping this marker
/// data-free means reconnecting only has to repair the hero's session-local
/// [`PeerId`]; the boat remains attached to the stable account name.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct PlayerBoat;

/// A temporary dinghy carrying a naturally arriving villager.
///
/// This replicated marker lets clients present and follow the real physical
/// immigration journey without inferring ownership from the more general
/// [`PlayerBoat`] vessel marker. The server removes the whole boat at landfall.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct ImmigrantArrivalBoat;

/// Any water-going vehicle using the shared vessel navigation/sailing stack.
/// Starter dinghies, merchant ships and warships differ in their additional
/// components and hull parameters, not in whether water is navigable.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Vessel;

/// A vessel that can no longer receive movement orders.
///
/// The opening dinghy becomes a small shoreline wreck when its hero lands.
/// Keeping wreckage as an explicit state, rather than pretending it is still
/// navigable or deleting it instantly, also gives future ships one shared seam
/// for sinking, salvage and repair presentation.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct WreckedVessel;

/// Marks a hero who is currently riding their starter boat.
///
/// While present, the server pins the hero to the helm and rejects ordinary
/// walking orders.  Removing it is the single authoritative transition from
/// the opening voyage to normal on-foot play.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct AboardBoat;

/// A person in the world, with a name.
///
/// Every character carries one -- a player's hero and a villager alike -- so the
/// encyclopedia can list PEOPLE rather than accounts. This is the thing that
/// separates "who is in this world" from "who has a login".
///
/// Replicated, and the server is the only writer. Names are generated from a
/// stable seed (see [`crate::names`]) so a villager keeps their name across a
/// restart; a player's hero takes the player's chosen profile name instead,
/// because that is the identity they already picked.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct CharacterName(pub String);

/// What kind of person this is.
///
/// The encyclopedia shows both, and needs to say which is which -- "a hero
/// belonging to a player" and "a villager who lives here" are different things
/// to the player even when they look identical on the ground.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CharacterKind {
    /// The embodied character of a player account.
    #[default]
    Hero,
    /// A world inhabitant. Not owned by anyone.
    Villager,
}

/// The coarse, visible activity a character is currently performing.
///
/// This is intentionally smaller than the server's full AI state machine. A
/// client needs to know that a villager is chopping or is behind a building's
/// exterior shell; it does not need the worker's timers, chosen tree, or next
/// destination.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CharacterActivity {
    #[default]
    Idle,
    Building,
    Chopping,
    Farming,
    /// Working at the end of a fishing pier. Until a dedicated cast/net clip
    /// exists, the client deliberately maps this to the same visible work
    /// motion used by farming and construction.
    Fishing,
    /// Resting at a roadside or gathering place. The server chooses the place;
    /// clients only need this coarse state to play the authored seated loop.
    Sitting,
    Indoors,
    /// Extracting Stone at an outdoor quarry face. Until dedicated pickaxe
    /// art exists, clients reuse the construction work motion.
    Mining,
    /// Trading blows in melee. Until dedicated combat art exists, clients
    /// reuse the most physical work motion available.
    Fighting,
}

impl CharacterActivity {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Idle => "Idle",
            Self::Building => "Building",
            Self::Chopping => "Cutting timber",
            Self::Farming => "Working the fields",
            Self::Fishing => "Fishing",
            Self::Sitting => "Taking a rest",
            Self::Indoors => "Indoors",
            Self::Mining => "Quarrying stone",
            Self::Fighting => "Fighting",
        }
    }
}

/// The current purpose behind a villager's visible activity.
///
/// [`CharacterActivity`] remains the small animation state (idle, walking,
/// farming, and so on). This component answers the more useful inspection
/// question: *why* is this person standing or walking? It is deliberately a
/// compact enum rather than a replicated debug string, so thousands of NPCs
/// only send a few bytes when their actual objective changes.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CharacterObjective {
    #[default]
    Idle,
    LookingForSettlement,
    SailingToSettlement,
    TravellingToSettlement,
    WaitingToRetryMigration,
    QueuedForImmigration,
    RegisteringImmigration,
    LeavingImmigrationCounter,
    QueuedForPermit,
    CollectingPermit,
    QueuedForHouseholdFood,
    QueuedForPersonalFood,
    QueuedForPoorRelief,
    CollectingFood,
    Eating,
    FindingConstructionWood,
    QueuedForConstructionWood,
    CollectingConstructionWood,
    CarryingConstructionWood,
    ConstructingBuilding,
    BuildingRoad,
    ClearingRoadTree,
    GoingHome,
    EnteringHome,
    Sleeping,
    LeavingHome,
    GoingHouseholdShopping,
    ReturningWithHouseholdFood,
    CollectingMarketGoods,
    DeliveringMarketGoods,
    GoingToFarm,
    Farming,
    ReturningHarvest,
    GoingFishing,
    Fishing,
    ReturningCatch,
    GoingToLumberWork,
    ChoppingTimber,
    ReturningTimber,
    GoingToProcessingWork,
    MillingFlour,
    BakingBread,
    GoingToQuarryWork,
    QuarryingStone,
    ReturningStone,
    GoingToLivestockWork,
    TendingLivestock,
    ReturningLivestockProducts,
    GoingToTavern,
    WaitingForTavernService,
    EatingAtTavern,
    LeavingTavern,
    OpeningTavern,
    ServingAtTavern,
    EndingWorkShift,
    SettlingIntoTown,
    WalkingAroundTown,
    Resting,
    ShelteringAtMoot,
    OffDuty,
    LookingForWork,
    WalkingToDestination,
    CollectingCompanyInputs,
    DeliveringCompanyInputs,
    GoingToTradeRoutePickup,
    HaulingInterSettlementCargo,
    ReturningFromTradeRoute,
}

impl CharacterObjective {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Idle => "Idle",
            Self::LookingForSettlement => "Looking for a settlement",
            Self::SailingToSettlement => "Sailing to a chosen settlement",
            Self::TravellingToSettlement => "Going to register at the Moot Hall",
            Self::WaitingToRetryMigration => "Waiting to retry settlement travel",
            Self::QueuedForImmigration => "In line to register as a resident",
            Self::RegisteringImmigration => "Registering at the Moot Hall",
            Self::LeavingImmigrationCounter => "Leaving the Moot Hall after registration",
            Self::QueuedForPermit => "In line to collect a permit",
            Self::CollectingPermit => "Collecting a permit",
            Self::QueuedForHouseholdFood => "In line for household food",
            Self::QueuedForPersonalFood => "In line to buy food",
            Self::QueuedForPoorRelief => "In line for Poor Relief",
            Self::CollectingFood => "Carrying food from the Moot Hall",
            Self::Eating => "Eating at the Moot commons",
            Self::FindingConstructionWood => "Finding Wood for a building site",
            Self::QueuedForConstructionWood => "In line to collect Wood for a building site",
            Self::CollectingConstructionWood => "Collecting Wood for a building site",
            Self::CarryingConstructionWood => "Delivering Wood to a building site",
            Self::ConstructingBuilding => "Constructing a building",
            Self::BuildingRoad => "Building a road",
            Self::ClearingRoadTree => "Clearing a tree from a road",
            Self::GoingHome => "Going home for the night",
            Self::EnteringHome => "Entering home",
            Self::Sleeping => "Sleeping at home",
            Self::LeavingHome => "Leaving home",
            Self::GoingHouseholdShopping => "Going to buy household food",
            Self::ReturningWithHouseholdFood => "Taking food home",
            Self::CollectingMarketGoods => "Collecting goods for the market",
            Self::DeliveringMarketGoods => "Delivering goods to the Moot Hall",
            Self::GoingToFarm => "Going to farm work",
            Self::Farming => "Working the wheat field",
            Self::ReturningHarvest => "Taking Wheat to the farmstead",
            Self::GoingFishing => "Going to the fishing grounds",
            Self::Fishing => "Fishing",
            Self::ReturningCatch => "Taking the catch to the fishing hut",
            Self::GoingToLumberWork => "Going to lumber work",
            Self::ChoppingTimber => "Cutting timber",
            Self::ReturningTimber => "Taking Wood to the lumber hut",
            Self::GoingToProcessingWork => "Going to processing work",
            Self::MillingFlour => "Milling Wheat into Flour",
            Self::BakingBread => "Baking Bread",
            Self::GoingToQuarryWork => "Going to quarry work",
            Self::QuarryingStone => "Quarrying Stone",
            Self::ReturningStone => "Taking Stone to the quarry store",
            Self::GoingToLivestockWork => "Going to the livestock pasture",
            Self::TendingLivestock => "Tending livestock",
            Self::ReturningLivestockProducts => "Taking Meat and Wool to the livestock farm",
            Self::GoingToTavern => "Going to the tavern",
            Self::WaitingForTavernService => "Waiting to order at the tavern",
            Self::EatingAtTavern => "Eating at the tavern",
            Self::LeavingTavern => "Leaving the tavern",
            Self::OpeningTavern => "Going to open the tavern",
            Self::ServingAtTavern => "Serving guests at the tavern",
            Self::EndingWorkShift => "Finishing the work shift",
            Self::SettlingIntoTown => "Getting settled after immigration",
            Self::WalkingAroundTown => "Walking around town",
            Self::Resting => "Resting",
            Self::ShelteringAtMoot => "Sheltering at the Moot Hall",
            Self::OffDuty => "Off duty",
            Self::LookingForWork => "Looking for work",
            Self::WalkingToDestination => "Walking to a destination",
            Self::CollectingCompanyInputs => "Collecting an internal company shipment",
            Self::DeliveringCompanyInputs => "Delivering goods between company workplaces",
            Self::GoingToTradeRoutePickup => "Going to collect inter-settlement cargo",
            Self::HaulingInterSettlementCargo => "Hauling cargo between settlements",
            Self::ReturningFromTradeRoute => "Returning to the company warehouse",
        }
    }
}

/// Inspection-only route state paired with [`CharacterObjective`]. This is
/// kept separate so a blocked farmer still reads as “going to farm work” while
/// the UI also exposes that route planning—not job selection—is the blocker.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CharacterNavigationStatus {
    #[default]
    Stationary,
    Walking,
    PlanningRoute,
    RouteBlocked,
}

impl CharacterNavigationStatus {
    pub const fn label(self) -> Option<&'static str> {
        match self {
            Self::Stationary => None,
            Self::Walking => Some("walking"),
            Self::PlanningRoute => Some("finding a route"),
            Self::RouteBlocked => Some("route blocked"),
        }
    }
}

impl CharacterKind {
    pub fn label(self) -> &'static str {
        match self {
            CharacterKind::Hero => "HERO",
            CharacterKind::Villager => "VILLAGER",
        }
    }
}

/// A settlement: the economic atom, and a PLACE rather than a set of buildings.
///
/// Buildings are how its plan gets expressed; this is what constitutes it. See
/// WORLD-DESIGN section 1.
///
/// The hall entity is region-scoped detail. A separate [`super::SettlementSummary`]
/// carries the small globally visible map record, joined through [`super::SettlementId`].
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Settlement {
    /// Chosen by whoever founded it. Naming a place is the first act of
    /// ownership the game offers, so a generator only ever SUGGESTS.
    pub name: String,
    pub tier: SettlementTier,
    /// How many people live here.
    ///
    /// A COUNT on the wire, not the roster itself: the roster is server truth
    /// and can be long, while every client needs the number for the map screen.
    /// Nobody sets this by hand -- residents arrive on their own feet and the
    /// server counts them.
    pub residents: u32,
    /// Local coin. Permit fees land here, including for independent
    /// settlements: a place's income is its own, and belongs to nobody else.
    /// Market fees, positive-profit levies and public-sale receipts join that
    /// income; wages, relief and public procurement spend the same real cash.
    pub treasury: u64,
}

/// The physical civic building standing on the settlement entity.
///
/// This is intentionally distinct from [`SettlementTier`]. Today promotion
/// completes the matching Hall level immediately; keeping the physical state
/// explicit lets a later treasury-funded construction project delay that
/// completion without replacing the settlement, its inventory, queues or
/// stable identity.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum CivicHallLevel {
    #[default]
    Moot,
    Village,
    Town,
}

/// Stable, replicated description of an in-place civic Hall upgrade.
///
/// The worksite is a separate entity at the Hall root. Its bounded
/// [`GoodsInventory`](crate::economy::GoodsInventory) contains the material pile;
/// [`ConstructionSite::raising`] flips only after all purchased material is
/// physically staged. Per-tick timers remain server-local to avoid needless
/// replication churn.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct CivicHallUpgradeWorksite {
    pub target: CivicHallLevel,
    pub material: crate::economy::Good,
    pub material_required: u32,
}

impl CivicHallLevel {
    /// Current automatic ladder. A City retains its Town Hall until authored
    /// City Hall art and construction rules exist.
    pub const fn for_tier(tier: SettlementTier) -> Self {
        match tier {
            SettlementTier::Ruins | SettlementTier::Hamlet => Self::Moot,
            SettlementTier::Village => Self::Village,
            SettlementTier::Town | SettlementTier::City => Self::Town,
        }
    }

    pub const fn building_type(self) -> crate::building::BuildingType {
        match self {
            Self::Moot => crate::building::BuildingType::MootHall,
            Self::Village => crate::building::BuildingType::VillageHall,
            Self::Town => crate::building::BuildingType::TownHall,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Moot => "MOOT HALL",
            Self::Village => "VILLAGE HALL",
            Self::Town => "TOWN HALL",
        }
    }

    /// Largest authored rung that every new civic centre must reserve.
    pub const fn largest_supported() -> Self {
        Self::Town
    }

    /// Centre of the permanent, largest-supported Hall footprint in world X/Z.
    pub fn reserved_world_center(root: Vec3, rotation_y: f32) -> Vec2 {
        Self::largest_supported()
            .building_type()
            .definition()
            .world_footprint_center(root, rotation_y)
    }

    pub fn reserved_half_extents() -> Vec2 {
        Self::largest_supported()
            .building_type()
            .definition()
            .footprint
            * 0.5
    }
}

/// The two authored house families. A line is permanent identity: upgrades
/// stay within it so the entrance never jumps to another wall.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum HouseLine {
    #[default]
    Cabin,
    LongCabin,
}

/// Physical rung of a house, independent of settlement tier after building.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum HouseLevel {
    #[default]
    Ground,
    UpperStorey,
}

/// Stable replicated art identity for one house or house worksite.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct HouseAppearance {
    pub line: HouseLine,
    pub level: HouseLevel,
}

impl HouseAppearance {
    /// Pick a repeatable line from the approved plot rather than iteration or
    /// RNG order, so reloads and high-speed simulations cannot re-skin homes.
    pub fn for_new_house(tier: SettlementTier, plot: Vec3) -> Self {
        let x = (plot.x * 4.0).round() as i64 as u64;
        let z = (plot.z * 4.0).round() as i64 as u64;
        let mut hash = x ^ z.rotate_left(32) ^ 0x9e37_79b9_7f4a_7c15;
        hash ^= hash >> 30;
        hash = hash.wrapping_mul(0xbf58_476d_1ce4_e5b9);
        hash ^= hash >> 27;
        hash = hash.wrapping_mul(0x94d0_49bb_1331_11eb);
        hash ^= hash >> 31;
        Self {
            line: if hash & 1 == 0 {
                HouseLine::Cabin
            } else {
                HouseLine::LongCabin
            },
            level: if tier >= SettlementTier::Village {
                HouseLevel::UpperStorey
            } else {
                HouseLevel::Ground
            },
        }
    }

    pub const fn building_type(self) -> crate::building::BuildingType {
        use crate::building::BuildingType as Art;
        match (self.line, self.level) {
            (HouseLine::Cabin, HouseLevel::Ground) => Art::LogCabin,
            (HouseLine::Cabin, HouseLevel::UpperStorey) => Art::CabinL2,
            (HouseLine::LongCabin, HouseLevel::Ground) => Art::LongCabin,
            (HouseLine::LongCabin, HouseLevel::UpperStorey) => Art::LongCabinL2,
        }
    }

    /// Largest authored rung in this same line, used when reserving a plot.
    pub const fn reserved_building_type(self) -> crate::building::BuildingType {
        use crate::building::BuildingType as Art;
        match self.line {
            HouseLine::Cabin => Art::CabinL2,
            HouseLine::LongCabin => Art::LongCabinL2,
        }
    }
}

/// The physical finish of a settlement's marketplace.
///
/// The two authored scenes have an identical 12 x 12 metre footprint and the
/// same service anchors, so this can upgrade the existing entity in place
/// without invalidating roads, inventories, ownership or visitor targets.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum MarketLevel {
    #[default]
    Earthen,
    Paved,
}

impl MarketLevel {
    /// Village markets begin as packed earth and receive paving when their
    /// settlement reaches Town. A future art rung can extend this ladder
    /// without changing the semantic `SettlementBuildingKind::Market`.
    pub const fn for_tier(tier: SettlementTier) -> Self {
        match tier {
            SettlementTier::Ruins | SettlementTier::Hamlet | SettlementTier::Village => {
                Self::Earthen
            }
            SettlementTier::Town | SettlementTier::City => Self::Paved,
        }
    }

    pub const fn building_type(self) -> crate::building::BuildingType {
        match self {
            Self::Earthen => crate::building::BuildingType::Market,
            Self::Paved => crate::building::BuildingType::MarketPaved,
        }
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Earthen => "EARTHEN MARKET",
            Self::Paved => "PAVED MARKET",
        }
    }
}

/// The small public office operated from a settlement's Moot Hall.
///
/// This is separate from [`Settlement`] because administration is optional
/// state that can grow without making every old settlement constructor and
/// save record know about future civic jobs. The founding roster has a Reeve
/// and up to two combined Moot Stewards; later tier and policy targets can
/// advertise Guards. It is replicated so clicking the hall exposes who
/// holds each job and what the public purse owes them.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct MootAdministration {
    /// Senior civic clerk. The Reeve remains accountable for permits and the
    /// public purse while the other two founding positions have physical jobs.
    #[serde(default)]
    pub reeve: Option<String>,
    /// Readable alias for the first combined Moot Steward. Durable employment
    /// remains authoritative and supports more than one steward.
    pub lead_steward: Option<String>,
    /// Public safety positions. Guards are real named jobs even before guard
    /// patrol/combat behaviour is implemented.
    #[serde(default)]
    pub guards: Vec<String>,
    /// Public works positions. Founding worker slots are combined Moot
    /// Stewards: both haul goods and either can accept road repairs. The
    /// lead alias above remains the readable primary steward.
    #[serde(default)]
    pub city_workers: Vec<String>,
    /// One payable per present or former public employee. Inactive entries
    /// remain until their arrears are actually paid, so changing jobs cannot
    /// erase a municipal debt.
    #[serde(default)]
    pub payroll: Vec<CivicPayrollEntry>,
    pub steward_daily_salary: u64,
    pub wage_arrears: u64,
    pub roadless_buildings: u16,
    pub disconnected_buildings: u16,
    /// Completed buildings whose live builder still owns an unfinished
    /// connector. Kept separate so the UI never calls pending work healthy.
    pub pending_road_buildings: u16,
    pub last_road_audit_day: u32,
}

impl Default for MootAdministration {
    fn default() -> Self {
        Self {
            reeve: None,
            lead_steward: None,
            guards: Vec::new(),
            city_workers: Vec::new(),
            payroll: Vec::new(),
            steward_daily_salary: crate::economy::MOOT_STEWARD_DAILY_SALARY,
            wage_arrears: 0,
            roadless_buildings: 0,
            disconnected_buildings: 0,
            pending_road_buildings: 0,
            last_road_audit_day: 0,
        }
    }
}

/// A durable public wage claim. Civic positions use the same explicit cash
/// and arrears rules as private businesses rather than silently volunteering
/// whenever the treasury is empty.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct CivicPayrollEntry {
    pub person_id: super::PersonId,
    pub name: String,
    pub role: super::CivicRole,
    pub daily_wage: u64,
    pub arrears: u64,
    pub last_accrual_day: u32,
    pub active: bool,
}

/// What a building in a settlement IS, as distinct from what it looks like.
///
/// Semantic rather than artistic on purpose. `BuildingType` names a glTF file;
/// this names a role in the economy, and the two are deliberately separable so
/// a Farmstead can be re-skinned without touching a single rule. Today several
/// of these borrow art that was modelled for something else.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SettlementBuildingKind {
    /// The founding act. One per settlement, and its position IS the
    /// settlement's position.
    Hall,
    /// Grows food.
    Farmstead,
    /// Cuts timber.
    LumberjackHut,
    /// Lands edible fish from an attached walkable pier.
    FishermansHut,
    /// Somewhere to live.
    House,
    /// Tier-two civic/commercial buildings. These stay semantic even as their
    /// physical art gains levels or regional variants.
    Market,
    Tavern,
    Church,
    /// Buys Wheat and mills it into household-edible Flour.
    Windmill,
    /// Buys Flour and bakes higher-efficiency Bread.
    Bakery,
    /// Private local depot. Its workers move company goods; it does not pool
    /// physical inventory with branches in other settlements.
    StorageHall,
    /// Extracts Stone from rocky ground. Appended to keep existing replicated
    /// enum discriminants stable.
    StoneQuarry,
    /// Raises grazing animals for edible Meat and Wool. Appended so existing
    /// replicated building discriminants remain stable.
    LivestockFarm,
}

impl SettlementBuildingKind {
    /// Complete player-facing permit catalogue in notice-board order.
    ///
    /// Keep this as the single source for permit menus. Tier rules below hide
    /// entries which are not yet legal; economic demand only changes their
    /// score and price. The Hall is deliberately absent because it is civic.
    pub const PLAYER_PERMIT_KINDS: [Self; 12] = [
        Self::House,
        Self::Farmstead,
        Self::FishermansHut,
        Self::LivestockFarm,
        Self::Windmill,
        Self::Bakery,
        Self::StorageHall,
        Self::LumberjackHut,
        Self::StoneQuarry,
        Self::Market,
        Self::Tavern,
        Self::Church,
    ];

    pub const fn is_civic(self) -> bool {
        matches!(
            self,
            SettlementBuildingKind::Hall
                | SettlementBuildingKind::Market
                | SettlementBuildingKind::Church
        )
    }

    /// First settlement rung at which a private person may purchase this land
    /// use. Economic demand affects the price and the notice-board signal, not
    /// legality: a founder may speculate on any unlocked use and bear the
    /// consequences. The Hall itself is never a private permit.
    pub const fn minimum_player_permit_tier(self) -> Option<SettlementTier> {
        match self {
            SettlementBuildingKind::Hall => None,
            SettlementBuildingKind::House
            | SettlementBuildingKind::Farmstead
            | SettlementBuildingKind::LumberjackHut
            | SettlementBuildingKind::FishermansHut
            | SettlementBuildingKind::Windmill
            | SettlementBuildingKind::Bakery
            | SettlementBuildingKind::StorageHall
            | SettlementBuildingKind::StoneQuarry
            | SettlementBuildingKind::LivestockFarm => Some(SettlementTier::Hamlet),
            SettlementBuildingKind::Market | SettlementBuildingKind::Tavern => {
                Some(SettlementTier::Village)
            }
            SettlementBuildingKind::Church => Some(SettlementTier::Town),
        }
    }

    pub fn is_player_permit_available_at(self, tier: SettlementTier) -> bool {
        self.minimum_player_permit_tier()
            .is_some_and(|minimum| tier >= minimum)
    }

    pub fn label(self) -> &'static str {
        match self {
            SettlementBuildingKind::Hall => "MOOT HALL",
            SettlementBuildingKind::Farmstead => "FARMSTEAD",
            SettlementBuildingKind::LumberjackHut => "LUMBERJACK HUT",
            SettlementBuildingKind::FishermansHut => "FISHERMAN'S HUT",
            SettlementBuildingKind::House => "HOUSE",
            SettlementBuildingKind::Market => "MARKETPLACE",
            SettlementBuildingKind::Tavern => "TAVERN",
            SettlementBuildingKind::Church => "CHURCH",
            SettlementBuildingKind::Windmill => "WINDMILL",
            SettlementBuildingKind::Bakery => "BAKERY",
            SettlementBuildingKind::StorageHall => "STORAGE HALL",
            SettlementBuildingKind::StoneQuarry => "STONE QUARRY",
            SettlementBuildingKind::LivestockFarm => "LIVESTOCK FARM",
        }
    }

    /// The art used for this semantic role. Keeping this mapping separate from
    /// the economy means future regional skins remain visual-only changes.
    pub fn art(self) -> crate::building::BuildingType {
        use crate::building::BuildingType as Art;
        match self {
            SettlementBuildingKind::Hall => Art::MootHall,
            SettlementBuildingKind::Farmstead => Art::Farmstead,
            SettlementBuildingKind::LumberjackHut => Art::LumberjackHut,
            SettlementBuildingKind::FishermansHut => Art::FishermansHut,
            SettlementBuildingKind::House => Art::LogCabin,
            SettlementBuildingKind::Market => Art::Market,
            SettlementBuildingKind::Tavern => Art::PlaceholderTavern,
            SettlementBuildingKind::Church => Art::PlaceholderChurch,
            SettlementBuildingKind::Windmill => Art::Windmill,
            SettlementBuildingKind::Bakery => Art::Bakery,
            // Dedicated art can replace this semantic mapping without a save
            // migration. Keep its temporary solid box distinct from the
            // walkable open-air marketplace.
            SettlementBuildingKind::StorageHall => Art::PlaceholderStorageHall,
            SettlementBuildingKind::StoneQuarry => Art::PlaceholderStoneQuarry,
            SettlementBuildingKind::LivestockFarm => Art::LivestockFarm,
        }
    }

    /// Resolve per-instance house art while preserving the simple semantic
    /// mapping for every other building kind and old replicated houses.
    pub fn art_with_house(self, house: Option<&HouseAppearance>) -> crate::building::BuildingType {
        if self == SettlementBuildingKind::House {
            house.copied().unwrap_or_default().building_type()
        } else {
            self.art()
        }
    }

    /// Conservative siting envelope. Houses reserve the union of both L2
    /// lines because the deterministic line pick happens at the approved plot.
    pub fn placement_definition(self) -> crate::building::BuildingDef {
        if self != SettlementBuildingKind::House {
            return self.art().definition();
        }
        let mut definition = crate::building::BuildingType::LongCabinL2.definition();
        definition.footprint = Vec2::new(8.6866, 7.3600);
        definition.footprint_center = Vec2::ZERO;
        definition
    }

    /// What this building makes its owner, given the ground it stands on.
    ///
    /// Reads the same `ResourceProfile` the vegetation density reads, so the
    /// answer is legible from the window: a farmstead standing in thick grass
    /// really is on good soil, and a lumberjack hut among dense trees really is
    /// in good timber. A player should be able to site a building well by
    /// LOOKING, without opening a heatmap.
    pub fn yield_quality(self, profile: &crate::worldgen::ResourceProfile) -> f32 {
        match self {
            SettlementBuildingKind::Farmstead => profile.farmland,
            SettlementBuildingKind::LivestockFarm => profile.farmland,
            SettlementBuildingKind::LumberjackHut => profile.wood,
            SettlementBuildingKind::StoneQuarry => profile.stone,
            // Fishing quality is geometry rather than a land resource: the
            // server measures navigable open water around the authored pier.
            SettlementBuildingKind::FishermansHut => 0.5,
            // These buildings transform supplied goods or provide services;
            // the soil beneath them does not change their output. Keep the
            // shared storage field neutral while omitting it from their UI.
            SettlementBuildingKind::Hall
            | SettlementBuildingKind::House
            | SettlementBuildingKind::Market
            | SettlementBuildingKind::Tavern
            | SettlementBuildingKind::Church
            | SettlementBuildingKind::Windmill
            | SettlementBuildingKind::Bakery
            | SettlementBuildingKind::StorageHall => 0.5,
        }
    }

    /// Geographic preference used while choosing a plot. This is deliberately
    /// separate from [`Self::yield_quality`]: a Windmill belongs on open ground
    /// visually and for wind access, but its actual Flour rate is controlled
    /// by Wheat, workers and elapsed mill time rather than a land multiplier.
    pub fn placement_suitability(self, profile: &crate::worldgen::ResourceProfile) -> f32 {
        match self {
            SettlementBuildingKind::Farmstead => profile.farmland,
            SettlementBuildingKind::LivestockFarm => {
                (profile.farmland * (1.0 - profile.wood * 0.55)).clamp(0.0, 1.0)
            }
            SettlementBuildingKind::LumberjackHut => profile.wood,
            SettlementBuildingKind::StoneQuarry => profile.stone,
            SettlementBuildingKind::Windmill => (1.0 - profile.wood * 0.75).clamp(0.0, 1.0),
            SettlementBuildingKind::Hall
            | SettlementBuildingKind::FishermansHut
            | SettlementBuildingKind::House
            | SettlementBuildingKind::Market
            | SettlementBuildingKind::Tavern
            | SettlementBuildingKind::Church
            | SettlementBuildingKind::Bakery
            | SettlementBuildingKind::StorageHall => 0.5,
        }
    }

    /// Player-facing site-yield label, present only where geography changes
    /// physical output. Processor throughput instead depends on inputs,
    /// staffing and work time.
    pub const fn site_quality_label(self) -> Option<&'static str> {
        match self {
            SettlementBuildingKind::Farmstead => Some("FARMLAND QUALITY"),
            SettlementBuildingKind::LivestockFarm => Some("PASTURE QUALITY"),
            SettlementBuildingKind::LumberjackHut => Some("TIMBER QUALITY"),
            SettlementBuildingKind::StoneQuarry => Some("STONE QUALITY"),
            SettlementBuildingKind::FishermansHut => Some("FISHING QUALITY"),
            SettlementBuildingKind::Hall
            | SettlementBuildingKind::House
            | SettlementBuildingKind::Market
            | SettlementBuildingKind::Tavern
            | SettlementBuildingKind::Church
            | SettlementBuildingKind::Windmill
            | SettlementBuildingKind::Bakery
            | SettlementBuildingKind::StorageHall => None,
        }
    }

    /// What someone working here is called, if anyone works here at all.
    pub fn trade(self) -> Option<&'static str> {
        match self {
            SettlementBuildingKind::Farmstead => Some("Farmer"),
            SettlementBuildingKind::LumberjackHut => Some("Woodcutter"),
            SettlementBuildingKind::FishermansHut => Some("Fisher"),
            SettlementBuildingKind::Hall => Some("Reeve"),
            SettlementBuildingKind::House => None,
            // The Marketplace is currently a second physical counter for the
            // Hall's shared store. It deliberately creates no civic job until
            // staffed market roles have real behaviour and payroll.
            SettlementBuildingKind::Market => None,
            SettlementBuildingKind::Tavern => Some("Innkeeper"),
            SettlementBuildingKind::Church => Some("Cleric"),
            SettlementBuildingKind::Windmill => Some("Miller"),
            SettlementBuildingKind::Bakery => Some("Baker"),
            SettlementBuildingKind::StorageHall => Some("Company Porter"),
            SettlementBuildingKind::StoneQuarry => Some("Quarrier"),
            SettlementBuildingKind::LivestockFarm => Some("Herder"),
        }
    }

    /// How many people this building has room to employ.
    ///
    /// A house has none on purpose: it is where people live, not where they
    /// work, and conflating the two is how population quietly becomes a
    /// multiplier again.
    pub fn positions(self) -> u8 {
        match self {
            SettlementBuildingKind::Farmstead => 2,
            SettlementBuildingKind::LumberjackHut => 1,
            SettlementBuildingKind::FishermansHut => 2,
            // The founding hall employs a Reeve and up to two combined Moot
            // Stewards. Each steward both collects consignments and maintains
            // roads; those duties must never become separate jobs.
            SettlementBuildingKind::Hall => 3,
            SettlementBuildingKind::House => 0,
            SettlementBuildingKind::Market => 0,
            SettlementBuildingKind::Tavern => 2,
            SettlementBuildingKind::Church => 1,
            SettlementBuildingKind::Windmill | SettlementBuildingKind::Bakery => 2,
            SettlementBuildingKind::StorageHall => 4,
            SettlementBuildingKind::StoneQuarry => 2,
            SettlementBuildingKind::LivestockFarm => 2,
        }
    }

    /// Bounded bulk storage physically available at this place. Hall and
    /// Marketplace values are applied independently to each public resource
    /// compartment; private buildings use one combined allowance.
    ///
    /// The hall is attached to the [`Settlement`] entity rather than spawned as
    /// a `SettlementBuilding`, but keeping its capacity in this semantic table
    /// gives every building role one source of truth.
    pub const fn storage_bulk_capacity(self) -> u32 {
        match self {
            SettlementBuildingKind::Hall => crate::economy::capacity::HALL,
            SettlementBuildingKind::Farmstead => crate::economy::capacity::FARMSTEAD,
            SettlementBuildingKind::LumberjackHut => crate::economy::capacity::LUMBERJACK_HUT,
            SettlementBuildingKind::FishermansHut => crate::economy::capacity::FISHERMANS_HUT,
            SettlementBuildingKind::House => crate::economy::capacity::HOUSE,
            SettlementBuildingKind::Market => crate::economy::capacity::MARKET,
            SettlementBuildingKind::Tavern => crate::economy::capacity::TAVERN,
            SettlementBuildingKind::Church => crate::economy::capacity::CHURCH,
            SettlementBuildingKind::Windmill => crate::economy::capacity::WINDMILL,
            SettlementBuildingKind::Bakery => crate::economy::capacity::BAKERY,
            SettlementBuildingKind::StorageHall => crate::economy::capacity::STORAGE_HALL,
            SettlementBuildingKind::StoneQuarry => crate::economy::capacity::STONE_QUARRY,
            SettlementBuildingKind::LivestockFarm => crate::economy::capacity::LIVESTOCK_FARM,
        }
    }

    /// How many permanent residents this building can house.
    ///
    /// Only houses add normal capacity. The hall remains emergency shelter for
    /// founders, but it does not let a settlement claim to be properly housed.
    pub const fn housing_capacity(self) -> u8 {
        match self {
            SettlementBuildingKind::House => 4,
            SettlementBuildingKind::Hall
            | SettlementBuildingKind::Farmstead
            | SettlementBuildingKind::LumberjackHut
            | SettlementBuildingKind::FishermansHut
            | SettlementBuildingKind::Market
            | SettlementBuildingKind::Tavern
            | SettlementBuildingKind::Church
            | SettlementBuildingKind::Windmill
            | SettlementBuildingKind::Bakery
            | SettlementBuildingKind::StorageHall => 0,
            SettlementBuildingKind::StoneQuarry | SettlementBuildingKind::LivestockFarm => 0,
        }
    }

    /// Wood bundles that must physically reach an approved worksite before
    /// its builder may clear the plot and raise the frame.
    ///
    /// These first village buildings deliberately use one material so the
    /// hauling loop can be judged before stone, tools or market prices exist.
    /// A log cabin is the tuning anchor requested by the design: ten bundles.
    pub const fn construction_wood_required(self) -> u32 {
        match self {
            SettlementBuildingKind::Hall => 0,
            SettlementBuildingKind::Farmstead => 12,
            SettlementBuildingKind::LumberjackHut => 10,
            SettlementBuildingKind::FishermansHut => 12,
            SettlementBuildingKind::House => 10,
            SettlementBuildingKind::Market => 14,
            SettlementBuildingKind::Tavern => 12,
            SettlementBuildingKind::Church => 16,
            // Small enough to bootstrap from a founder's ten coins while
            // retaining some working capital for the first input purchase.
            SettlementBuildingKind::Windmill | SettlementBuildingKind::Bakery => 8,
            SettlementBuildingKind::StorageHall => 14,
            SettlementBuildingKind::StoneQuarry => 12,
            SettlementBuildingKind::LivestockFarm => 12,
        }
    }

    /// Exact bulk capacity of a worksite's material pile.
    pub const fn construction_storage_bulk(self) -> u32 {
        self.construction_wood_required() * crate::economy::Good::Wood.bulk_per_unit()
    }

    /// Door anchor in building-local X/Z metres.
    ///
    /// These values come from the authored `Anchor_Door` nodes in the current
    /// building assets. Keeping them beside the semantic building kind lets AI
    /// and future interaction code use the same entrance as the art.
    pub const fn door_offset(self) -> Vec2 {
        match self {
            SettlementBuildingKind::Hall => Vec2::new(0.0, -5.20),
            SettlementBuildingKind::Farmstead => Vec2::new(0.0, -3.95),
            SettlementBuildingKind::LumberjackHut => Vec2::new(0.0, -3.40),
            SettlementBuildingKind::FishermansHut => Vec2::new(0.0, -4.45),
            SettlementBuildingKind::House => Vec2::new(0.0, -3.80),
            SettlementBuildingKind::Market => Vec2::new(0.0, -6.5),
            SettlementBuildingKind::Tavern => Vec2::new(0.0, -4.0),
            SettlementBuildingKind::Church => Vec2::new(0.0, -6.5),
            SettlementBuildingKind::Windmill | SettlementBuildingKind::Bakery => {
                Vec2::new(0.0, -4.0)
            }
            SettlementBuildingKind::StorageHall => Vec2::new(0.0, -4.0),
            SettlementBuildingKind::StoneQuarry => Vec2::new(0.0, -4.8),
            SettlementBuildingKind::LivestockFarm => Vec2::new(0.0, -3.8),
        }
    }

    /// World-space point where a character enters or leaves this building.
    pub fn entrance_position(self, plot: Vec3, rotation_y: f32) -> Vec3 {
        let offset = crate::rotation::local_to_world_xz(self.door_offset(), rotation_y);
        Vec3::new(plot.x + offset.x, plot.y, plot.z + offset.y)
    }

    /// A point just through the doorway, used for visible threshold crossing.
    ///
    /// Interiors are still exterior shells, so this is deliberately shallow:
    /// far enough past the wall for a villager to walk through the open leaf,
    /// not an invented room layout that future interiors would have to keep.
    pub fn interior_door_position(self, plot: Vec3, rotation_y: f32) -> Vec3 {
        let entrance = self.entrance_position(plot, rotation_y);
        let inward = Vec2::new(plot.x - entrance.x, plot.z - entrance.z).normalize_or_zero();
        Vec3::new(
            entrance.x + inward.x * 1.35,
            plot.y,
            entrance.z + inward.y * 1.35,
        )
    }

    /// The centres of the two crop plots authored behind a Farmstead.
    ///
    /// A Farmstead needs both plots for full production. They sit beside one
    /// another so the farmhouse remains the obvious shared workplace while
    /// each of its two farmers has a distinct field to work.
    pub fn field_positions(self, plot: Vec3, rotation_y: f32) -> Option<[Vec3; 2]> {
        (self == SettlementBuildingKind::Farmstead).then(|| {
            [-FARM_FIELD_LATERAL_OFFSET, FARM_FIELD_LATERAL_OFFSET].map(|side| {
                let offset = crate::rotation::local_to_world_xz(Vec2::new(side, 9.0), rotation_y);
                Vec3::new(plot.x + offset.x, plot.y, plot.z + offset.y)
            })
        })
    }

    /// Centre of one numbered crop plot authored beside a Farmstead.
    pub fn field_position_at(self, plot: Vec3, rotation_y: f32, plot_index: u8) -> Option<Vec3> {
        self.field_positions(plot, rotation_y)?
            .get(plot_index as usize)
            .copied()
    }

    /// Centre of the first crop plot.
    ///
    /// Kept as a compatibility convenience for callers that only need a
    /// representative Farmstead field location.
    pub fn field_position(self, plot: Vec3, rotation_y: f32) -> Option<Vec3> {
        self.field_position_at(plot, rotation_y, 0)
    }

    /// Centre of the fenced grazing plot behind a Livestock Farm.
    pub fn pasture_position(self, plot: Vec3, rotation_y: f32) -> Option<Vec3> {
        (self == SettlementBuildingKind::LivestockFarm).then(|| {
            let offset = crate::rotation::local_to_world_xz(Vec2::new(0.0, 12.0), rotation_y);
            Vec3::new(plot.x + offset.x, plot.y, plot.z + offset.y)
        })
    }

    /// Half-size of the separate walkable livestock pasture.
    pub const fn pasture_half_extents(self) -> Option<Vec2> {
        match self {
            SettlementBuildingKind::LivestockFarm => Some(Vec2::new(8.0, 7.0)),
            _ => None,
        }
    }

    /// Half-size of the separate wheat plot in its rendered local X/Z frame.
    ///
    /// `WheatField.glb` measures 8 x 11 metres after the authoring export turn.
    /// Keeping that footprint beside [`Self::field_position`] lets settlement
    /// planning reserve the crop while still leaving it collider-free for the
    /// farmers who must walk among the rows.
    pub const fn field_half_extents(self) -> Option<Vec2> {
        match self {
            SettlementBuildingKind::Farmstead => Some(Vec2::new(4.0, 5.5)),
            _ => None,
        }
    }

    /// The authored landward origin of the separate walkable fishing pier.
    pub fn pier_position(self, plot: Vec3, rotation_y: f32) -> Option<Vec3> {
        (self == SettlementBuildingKind::FishermansHut).then(|| {
            let offset = crate::rotation::local_to_world_xz(Vec2::new(0.0, 2.85), rotation_y);
            Vec3::new(plot.x + offset.x, plot.y, plot.z + offset.y)
        })
    }

    /// The authored net-mending point that safely routes a fisher around the
    /// solid hut instead of asking them to walk from its front door through it.
    pub fn nets_position(self, plot: Vec3, rotation_y: f32) -> Option<Vec3> {
        (self == SettlementBuildingKind::FishermansHut).then(|| {
            let offset = crate::rotation::local_to_world_xz(Vec2::new(-4.15, -0.35), rotation_y);
            Vec3::new(plot.x + offset.x, plot.y, plot.z + offset.y)
        })
    }

    /// Where a fisher stands at the seaward end of `FishingPier.glb`.
    pub fn fishing_position(self, plot: Vec3, rotation_y: f32) -> Option<Vec3> {
        let pier = self.pier_position(plot, rotation_y)?;
        let offset = crate::rotation::local_to_world_xz(Vec2::new(0.0, 6.25), rotation_y);
        Some(Vec3::new(pier.x + offset.x, pier.y, pier.z + offset.y))
    }

    /// How far from the hall this belongs, in metres.
    ///
    /// Houses cluster around the hall because that is what a village looks
    /// like; workplaces sit out where their work is. Real siting against
    /// farmland and forest comes with the settlement planner -- this is the
    /// crude version that gets the shape right.
    pub fn preferred_ring(self) -> (f32, f32) {
        match self {
            SettlementBuildingKind::Hall => (0.0, 0.0),
            // Several concentric residential lanes leave enough frontage for
            // a growing hamlet even after its radial roads claim real space.
            SettlementBuildingKind::House => (12.0, 54.0),
            // Workplaces start on the old close working lane so a new hamlet
            // does not add a long first supply journey. Their expanded outer
            // search can still reach meadow or woodland when the inner lanes
            // fill. It deliberately overlaps the outer housing band: occupied
            // footprints and roads, not an arbitrary zoning wall, decide the
            // final village shape.
            SettlementBuildingKind::Farmstead => (30.0, 120.0),
            SettlementBuildingKind::LumberjackHut => (30.0, 120.0),
            // The coastal search uses this wide band to find the actual bank;
            // it still requires the whole hut to remain safely on dry land.
            SettlementBuildingKind::FishermansHut => (12.0, 120.0),
            // Civic amenities occupy valuable inner frontage. Seeded layout
            // scoring varies their exact centre geometry in the server.
            SettlementBuildingKind::Market => (18.0, 54.0),
            SettlementBuildingKind::Tavern => (18.0, 66.0),
            SettlementBuildingKind::Church => (24.0, 78.0),
            SettlementBuildingKind::Windmill => (30.0, 96.0),
            SettlementBuildingKind::Bakery => (18.0, 60.0),
            SettlementBuildingKind::StorageHall => (22.0, 78.0),
            SettlementBuildingKind::StoneQuarry => (36.0, 150.0),
            SettlementBuildingKind::LivestockFarm => (32.0, 120.0),
        }
    }

    /// Ground a building of this kind needs to itself, in metres.
    pub fn clearance(self) -> f32 {
        match self {
            // The Moot is also the founding market, permit office and relief
            // counter. Reserve a real civic forecourt for its visible service
            // line and commons instead of allowing later cabins to pinch the
            // authored doorway down to a single overlapping navigation point.
            SettlementBuildingKind::Hall => 16.0,
            SettlementBuildingKind::Farmstead => 12.0,
            SettlementBuildingKind::LumberjackHut => 9.0,
            SettlementBuildingKind::FishermansHut => 11.0,
            // A 6.0 x 6.94 m cabin does not need the old sixteen-metre
            // centre-to-centre exclusion. Twelve metres still leaves useful
            // yards between cabins while allowing seeded lanes and grid
            // frontages to read as an actual neighbourhood. Door aprons,
            // roads, prop collision and the authored footprints remain
            // separate hard constraints.
            SettlementBuildingKind::House => 6.0,
            SettlementBuildingKind::Market => 13.0,
            SettlementBuildingKind::Tavern => 10.0,
            SettlementBuildingKind::Church => 12.0,
            SettlementBuildingKind::Windmill => 11.0,
            SettlementBuildingKind::Bakery => 9.0,
            SettlementBuildingKind::StorageHall => 12.0,
            SettlementBuildingKind::StoneQuarry => 12.0,
            SettlementBuildingKind::LivestockFarm => 14.0,
        }
    }
}

#[cfg(test)]
mod settlement_building_kind_tests {
    use super::*;

    #[test]
    fn only_extractive_workplaces_expose_site_quality() {
        assert_eq!(
            SettlementBuildingKind::Farmstead.site_quality_label(),
            Some("FARMLAND QUALITY")
        );
        assert_eq!(
            SettlementBuildingKind::LumberjackHut.site_quality_label(),
            Some("TIMBER QUALITY")
        );
        assert_eq!(
            SettlementBuildingKind::FishermansHut.site_quality_label(),
            Some("FISHING QUALITY")
        );
        assert_eq!(SettlementBuildingKind::Windmill.site_quality_label(), None);
        assert_eq!(SettlementBuildingKind::Bakery.site_quality_label(), None);
        assert_eq!(
            SettlementBuildingKind::StorageHall.site_quality_label(),
            None
        );
        assert_eq!(SettlementBuildingKind::StorageHall.positions(), 4);
        assert_eq!(
            SettlementBuildingKind::StorageHall.storage_bulk_capacity(),
            crate::economy::capacity::STORAGE_HALL
        );
        assert!(
            SettlementBuildingKind::StorageHall.storage_bulk_capacity()
                > SettlementBuildingKind::Bakery.storage_bulk_capacity()
        );

        let dense_forest = crate::worldgen::ResourceProfile {
            wood: 1.0,
            stone: 0.0,
            iron: 0.0,
            farmland: 0.0,
        };
        assert_eq!(
            SettlementBuildingKind::Windmill.yield_quality(&dense_forest),
            0.5,
            "processor output must not change with the land resource profile"
        );
        assert_eq!(
            SettlementBuildingKind::Bakery.yield_quality(&dense_forest),
            0.5
        );
        let open_ground = crate::worldgen::ResourceProfile {
            wood: 0.0,
            ..dense_forest
        };
        assert!(
            SettlementBuildingKind::Windmill.placement_suitability(&open_ground)
                > SettlementBuildingKind::Windmill.placement_suitability(&dense_forest),
            "Windmills should prefer open plots without turning that preference into output quality"
        );
        assert_eq!(
            SettlementBuildingKind::Bakery.placement_suitability(&open_ground),
            SettlementBuildingKind::Bakery.placement_suitability(&dense_forest)
        );
    }

    #[test]
    fn market_plot_contract_matches_the_authored_open_square() {
        let market = SettlementBuildingKind::Market;
        assert_eq!(market.art(), crate::building::BuildingType::Market);
        assert_eq!(market.positions(), 0, "market civic jobs remain disabled");
        assert_eq!(market.trade(), None);
        assert_eq!(market.door_offset(), Vec2::new(0.0, -6.5));
        assert_eq!(market.clearance(), 13.0);
        assert_eq!(
            MarketLevel::for_tier(SettlementTier::Village),
            MarketLevel::Earthen
        );
        assert_eq!(
            MarketLevel::for_tier(SettlementTier::Town),
            MarketLevel::Paved
        );
    }
}

/// Seed-selected settlement morphology. It biases future candidate plots but
/// never relocates a structure that is already built.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SettlementLayoutStyle {
    #[default]
    Organic,
    Radial,
    Grid,
    Avenue,
    Polycentric,
}

impl SettlementLayoutStyle {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Organic => "ORGANIC LANES",
            Self::Radial => "RADIAL COMMONS",
            Self::Grid => "ORDERED GRID",
            Self::Avenue => "GREAT AVENUE",
            Self::Polycentric => "NEIGHBOURHOOD CLUSTERS",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SettlementCenterStyle {
    #[default]
    Green,
    Square,
    Avenue,
    Courtyard,
}

impl SettlementCenterStyle {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Green => "VILLAGE GREEN",
            Self::Square => "MARKET SQUARE",
            Self::Avenue => "CIVIC AVENUE",
            Self::Courtyard => "CIVIC COURTYARD",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SettlementWallStyle {
    #[default]
    Organic,
    Round,
    Square,
    DistrictFitted,
}

impl SettlementWallStyle {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Organic => "ORGANIC",
            Self::Round => "ROUND",
            Self::Square => "SQUARE",
            Self::DistrictFitted => "DISTRICT-FITTED",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SettlementProgressGate {
    #[default]
    FoodSecurity,
    Population,
    Marketplace,
    Tavern,
    Trade,
    Prosperity,
    Church,
    Sustaining,
    CivicHallMaterials,
    CivicHallConstruction,
    Complete,
}

impl SettlementProgressGate {
    pub const fn label(self) -> &'static str {
        match self {
            Self::FoodSecurity => "secure food supply",
            Self::Population => "more permanent residents",
            Self::Marketplace => "build a marketplace",
            Self::Tavern => "build a tavern",
            Self::Trade => "operate the local market",
            Self::Prosperity => "raise prosperity",
            Self::Church => "build a church",
            Self::Sustaining => "sustain all requirements",
            Self::CivicHallMaterials => "buy and stage materials for the Hall upgrade",
            Self::CivicHallConstruction => "construct the Hall upgrade",
            Self::Complete => "highest settlement tier reached",
        }
    }
}

/// Replicated, inspectable settlement plan and promotion ledger.
///
/// The seed fixes a settlement's planning temperament. Demand and permits still
/// decide how many farms, houses and businesses exist, so the seed is a set of
/// preferences rather than a pre-baked city.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SettlementDevelopment {
    pub plan_seed: u64,
    pub layout: SettlementLayoutStyle,
    pub center: SettlementCenterStyle,
    pub inner_wall: SettlementWallStyle,
    pub outer_wall: SettlementWallStyle,
    pub next_gate: SettlementProgressGate,
    pub progress_days: u16,
    pub required_days: u16,
    pub last_progress_day: u32,
    pub last_road_work_day: u32,
    pub dirt_roads: u16,
    pub stone_roads: u16,
    pub stone_committed: u32,
    pub stone_needed: u32,
}

/// Current permit-market signals published by the settlement hall.
///
/// These are invitations, not construction orders. A subsidized opportunity
/// receives the enacted permit discount; residents may still choose a lower
/// signal at full price or decline every offer. Scores are quantized so normal
/// stock movement does not create noisy high-frequency replication.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct PermitMarketOpportunity {
    pub kind: SettlementBuildingKind,
    pub score: u8,
    pub subsidized: bool,
    /// Competition signals are meant to admit a new owner, rather than let
    /// the incumbent use a public discount to deepen the same monopoly.
    #[serde(default)]
    pub requires_independent_owner: bool,
}

#[derive(Component, Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct SettlementOpportunityBoard {
    /// Highest signal first; bounded by the settlement's tier-unlocked uses.
    pub opportunities: Vec<PermitMarketOpportunity>,
}

/// One unspent land-use right purchased by a player-controlled person.
///
/// The paid permit fee is refundable until a plot is chosen. Business working
/// capital is deliberately *not* escrowed here: it remains ordinary company
/// cash and is spent on materials, inputs, wages or later expansion when those
/// costs actually occur.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct PlayerPermit {
    pub id: super::PermitId,
    pub settlement: super::SettlementId,
    pub kind: SettlementBuildingKind,
    pub fee_escrow: u64,
    pub purchased_day: u32,
    /// Productive and private-service permits must name their legal company.
    /// Housing remains personal and therefore carries `None`.
    #[serde(default)]
    pub company: Option<super::CompanyId>,
}

/// Small replicated permit wallet on a player's live hero.
///
/// This is deliberately separate from cargo: a stamped land right has no
/// physical bulk. The server caps it and remains the only authority allowed to
/// append, consume or refund an entry.
#[derive(Component, Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct PlayerPermitLedger {
    pub permits: Vec<PlayerPermit>,
}

impl PlayerPermitLedger {
    /// A replication/UI anti-spam bound on simultaneously *unused* stamps.
    /// It never restricts permit kinds or lifetime ownership: placing or
    /// surrendering any stamp immediately frees its slot.
    pub const MAX_ACTIVE: usize = 8;

    pub fn get(&self, id: super::PermitId) -> Option<&PlayerPermit> {
        self.permits.iter().find(|permit| permit.id == id)
    }
}

/// Whether a property listing is a finished workplace or a permitted site
/// whose materials and construction duty transfer with the purchase.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub enum PropertyListingStage {
    CompletedBusiness,
    UnfinishedWorksite,
}

impl PropertyListingStage {
    pub const fn label(self) -> &'static str {
        match self {
            Self::CompletedBusiness => "Completed business",
            Self::UnfinishedWorksite => "Unfinished worksite",
        }
    }
}

/// One small, globally useful property-market record published by the hall.
///
/// Buildings themselves remain detailed world entities. This summary lets a
/// settlement menu stay complete even when a large town's outer workplace is
/// beyond the client's detailed replication radius.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct PropertyMarketListing {
    pub kind: SettlementBuildingKind,
    pub stage: PropertyListingStage,
    pub asking_price: u64,
    pub listed_day: u32,
    pub reason: crate::economy::BusinessSaleReason,
    pub position: Vec3,
}

/// Current private buildings and unfinished business permits offered for
/// takeover in this settlement. Empty is a real market state, not missing data.
#[derive(Component, Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct SettlementPropertyBoard {
    pub listings: Vec<PropertyMarketListing>,
}

impl SettlementDevelopment {
    pub fn from_foundation(name: &str, position: Vec3, day: u32) -> Self {
        let mut seed = 0xcbf2_9ce4_8422_2325_u64;
        for byte in name.bytes() {
            seed ^= u64::from(byte);
            seed = seed.wrapping_mul(0x0000_0100_0000_01b3);
        }
        seed ^= u64::from(position.x.to_bits()).rotate_left(17);
        seed ^= u64::from(position.z.to_bits()).rotate_left(41);
        let choose = |shift: u32, count: u64| (seed.rotate_right(shift) % count) as u8;
        let layout = match choose(0, 5) {
            0 => SettlementLayoutStyle::Organic,
            1 => SettlementLayoutStyle::Radial,
            2 => SettlementLayoutStyle::Grid,
            3 => SettlementLayoutStyle::Avenue,
            _ => SettlementLayoutStyle::Polycentric,
        };
        let center = match choose(11, 4) {
            0 => SettlementCenterStyle::Green,
            1 => SettlementCenterStyle::Square,
            2 => SettlementCenterStyle::Avenue,
            _ => SettlementCenterStyle::Courtyard,
        };
        let wall = |value| match value {
            0 => SettlementWallStyle::Organic,
            1 => SettlementWallStyle::Round,
            2 => SettlementWallStyle::Square,
            _ => SettlementWallStyle::DistrictFitted,
        };
        let inner_wall = wall(choose(23, 4));
        let mut outer_wall = wall(choose(37, 4));
        if outer_wall == inner_wall {
            outer_wall = wall((choose(37, 4) + 1) % 4);
        }
        Self {
            plan_seed: seed,
            layout,
            center,
            inner_wall,
            outer_wall,
            next_gate: SettlementProgressGate::FoodSecurity,
            progress_days: 0,
            required_days: crate::economy::VILLAGE_REQUIRED_SECURE_DAYS,
            last_progress_day: day,
            last_road_work_day: day,
            dirt_roads: 0,
            stone_roads: 0,
            stone_committed: 0,
            stone_needed: 0,
        }
    }
}

/// Lowest vertical gap between a rotated ground rectangle and the local water
/// surface beneath it.
///
/// Unlike subtracting [`WorldTerrain::water_level`](crate::terrain::WorldTerrain::water_level),
/// this includes sloping inland rivers. Samples stay below one metre apart so
/// a narrow headwater cannot pass between a rectangle's corners unnoticed.
pub fn minimum_rotated_rect_water_clearance(
    terrain: &crate::terrain::WorldTerrain,
    centre: Vec3,
    half_extents: Vec2,
    rotation_y: f32,
) -> f32 {
    const SAMPLE_SPACING: f32 = 0.75;
    let x_intervals = ((half_extents.x * 2.0 / SAMPLE_SPACING).ceil() as usize).max(1);
    let z_intervals = ((half_extents.y * 2.0 / SAMPLE_SPACING).ceil() as usize).max(1);
    let mut minimum = f32::INFINITY;

    for x_step in 0..=x_intervals {
        for z_step in 0..=z_intervals {
            let t_x = x_step as f32 / x_intervals as f32;
            let t_z = z_step as f32 / z_intervals as f32;
            let local = Vec2::new(
                -half_extents.x + half_extents.x * 2.0 * t_x,
                -half_extents.y + half_extents.y * 2.0 * t_z,
            );
            let offset = crate::rotation::local_to_world_xz(local, rotation_y);
            let x = centre.x + offset.x;
            let z = centre.z + offset.y;
            let Some(water) = terrain.water_surface_height(x, z) else {
                continue;
            };
            minimum = minimum.min(terrain.get_height(x, z) - water);
        }
    }
    minimum
}

/// Minimum local-water clearance under a building's full rotated footprint
/// and authored door position.
pub fn minimum_building_water_clearance(
    terrain: &crate::terrain::WorldTerrain,
    centre: Vec3,
    kind: SettlementBuildingKind,
    rotation_y: f32,
) -> f32 {
    let definition = kind.placement_definition();
    let half = definition.footprint * 0.5 + Vec2::splat(0.25);
    let footprint_center = definition.world_footprint_center(centre, rotation_y);
    let footprint = minimum_rotated_rect_water_clearance(
        terrain,
        Vec3::new(footprint_center.x, centre.y, footprint_center.y),
        half,
        rotation_y,
    );
    let door = kind.entrance_position(centre, rotation_y);
    let door_clearance = terrain
        .water_surface_height(door.x, door.z)
        .map_or(f32::INFINITY, |water| {
            terrain.get_height(door.x, door.z) - water
        });
    footprint.min(door_clearance)
}

/// Local-water clearance beneath the complete civic shell reserved on the
/// founding day, not merely beneath the currently visible Moot Hall.
pub fn minimum_civic_hall_reservation_water_clearance(
    terrain: &crate::terrain::WorldTerrain,
    root: Vec3,
    rotation_y: f32,
) -> f32 {
    let definition = CivicHallLevel::largest_supported()
        .building_type()
        .definition();
    let footprint_center = definition.world_footprint_center(root, rotation_y);
    let footprint = minimum_rotated_rect_water_clearance(
        terrain,
        Vec3::new(footprint_center.x, root.y, footprint_center.y),
        definition.footprint * 0.5 + Vec2::splat(0.25),
        rotation_y,
    );
    let door = SettlementBuildingKind::Hall.entrance_position(root, rotation_y);
    let door_clearance = terrain
        .water_surface_height(door.x, door.z)
        .map_or(f32::INFINITY, |water| {
            terrain.get_height(door.x, door.z) - water
        });
    footprint.min(door_clearance)
}

/// How far apart settlements must be founded, in metres.
///
/// Lives in `shared` because BOTH sides need it and they must not disagree: the
/// server enforces it, and the client checks it before sending so the player is
/// told why a click did nothing instead of watching the button reset in silence.
pub const MIN_SETTLEMENT_SPACING: f32 = 300.0;

/// How far above the waterline a settlement must be founded, in metres.
///
/// Same reason as the spacing: a hall founded in a lake looks fine and then
/// never builds anything, because every plot its residents try is refused as
/// underwater. Better to say no at the click.
pub const SETTLEMENT_FREEBOARD: f32 = 0.35;

/// Why a settlement cannot be founded at a point, if it cannot.
///
/// Returned as a sentence rather than a code because its only job is to be
/// shown to a person.
pub fn founding_refusal(
    ground: f32,
    water_level: Option<f32>,
    nearest_settlement: Option<(&str, f32)>,
) -> Option<String> {
    if water_level.is_some_and(|level| ground < level + SETTLEMENT_FREEBOARD) {
        return Some("The Moot Hall would touch the water".to_string());
    }
    if let Some((name, distance)) = nearest_settlement {
        if distance < MIN_SETTLEMENT_SPACING {
            return Some(format!(
                "Too close to {name} ({distance:.0}m of {MIN_SETTLEMENT_SPACING:.0}m)"
            ));
        }
    }
    None
}

/// Authoritative founding check against the complete hall footprint and local
/// river/ocean surface.
pub fn settlement_founding_refusal(
    terrain: &crate::terrain::WorldTerrain,
    centre: Vec3,
    nearest_settlement: Option<(&str, f32)>,
) -> Option<String> {
    if minimum_civic_hall_reservation_water_clearance(terrain, centre, 0.0) < SETTLEMENT_FREEBOARD {
        return Some("The Moot Hall would touch the water".to_string());
    }
    founding_refusal(f32::INFINITY, None, nearest_settlement)
}

/// Where a person lives.
///
/// Human-readable residence label for panels and old-state migration.
/// [`super::ResidentOf`] and [`super::LivesAt`] are the authoritative durable
/// relationships. Absent means unhoused -- a real state, not a missing value.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct Residence(pub String);

/// What a person does for a living, if anything.
///
/// `None` is unemployed, which is a real and common state -- a villager who has
/// just walked into town holds no position until one exists to hold. Present on
/// every villager so the panel never has to guess whether the answer is
/// "nothing" or "not loaded yet".
#[derive(Component, Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct Occupation(pub Option<String>);

/// A person's durable relationship to work, separate from their current
/// animation and from the title printed in [`Occupation`].
///
/// Three states are deliberately enough for thousands of residents: a person
/// either holds a position, wants one, or has chosen not to seek one. Rich
/// owners and future homemakers can therefore relax without being repeatedly
/// offered the first vacancy every simulation tick.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum WorkStatus {
    Employed,
    #[default]
    LookingForWork,
    Chilling,
}

/// Broad use of a resident's discretionary time in one inspectable day plan.
///
/// This is deliberately not a happiness need. It tells the simulation and UI
/// where an otherwise-free person intends to spend part of the day, while the
/// actual visit still depends on a route, an open business, stock and money.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PlannedLeisure {
    /// Household errands, roadside conversation or an unstructured walk.
    #[default]
    LocalFreeTime,
    /// One paid meal at a private Tavern if the quoted price remains sensible.
    TavernMeal,
}

impl PlannedLeisure {
    pub const fn label(self) -> &'static str {
        match self {
            Self::LocalFreeTime => "Free time in town",
            Self::TavernMeal => "Tavern meal",
        }
    }
}

/// Progress of the discretionary entry on [`CharacterDayPlan`].
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum PlannedLeisureStatus {
    #[default]
    Planned,
    InProgress,
    Completed,
    CouldNotAfford,
    TavernUnavailable,
    CouldNotReach,
}

impl PlannedLeisureStatus {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Planned => "planned",
            Self::InProgress => "happening now",
            Self::Completed => "completed",
            Self::CouldNotAfford => "skipped: price too high",
            Self::TavernUnavailable => "skipped: tavern unavailable",
            Self::CouldNotReach => "skipped: no route",
        }
    }
}

/// A compact, server-authored calendar for one resident's current world day.
///
/// Times are display-clock minutes after midnight. The plan is regenerated at
/// most once per person per day (or when their employment state changes), so
/// thousands of NPCs do not need continuously evaluated behaviour trees.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct CharacterDayPlan {
    pub day: u32,
    pub wake_minute: u16,
    /// `None` means job seeking, household tasks or owner leisure replaces a
    /// formal shift today.
    pub work_minutes: Option<(u16, u16)>,
    pub meal_minute: u16,
    pub leisure_minutes: (u16, u16),
    pub sleep_minute: u16,
    pub leisure: PlannedLeisure,
    pub leisure_status: PlannedLeisureStatus,
    /// Employment snapshot which caused this plan. It lets a same-day hire or
    /// resignation refresh the calendar without polling more mutable facts.
    pub planned_work_status: WorkStatus,
}

impl CharacterDayPlan {
    pub fn has_due_leisure(self, display_minute: u16) -> bool {
        self.leisure == PlannedLeisure::TavernMeal
            && self.leisure_status == PlannedLeisureStatus::Planned
            && display_minute >= self.leisure_minutes.0
            && display_minute < self.leisure_minutes.1
    }
}

impl WorkStatus {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Employed => "Employed",
            Self::LookingForWork => "Looking for work",
            Self::Chilling => "Chilling",
        }
    }
}

/// Readable nutrition state derived from consecutive daily meal outcomes.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum NutritionCondition {
    Unassessed,
    Fed,
    Hungry,
    Starving,
    Critical,
}

impl NutritionCondition {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Unassessed => "Not yet assessed",
            Self::Fed => "Fed",
            Self::Hungry => "Hungry",
            Self::Starving => "Starving",
            Self::Critical => "Critical starvation",
        }
    }
}

/// One character's meal history.
///
/// Settlement food security remains the planning aggregate; this is the small
/// per-person fact used by inspection UI and Health, and later by migration.
/// `None` means the villager has not crossed a simulated meal boundary yet --
/// importantly different from claiming they are either fed or hungry.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Nutrition {
    pub last_meal_day: Option<u32>,
    pub consecutive_missed_meals: u16,
    /// Lifetime successful daily meals. This gives Health the same
    /// time-warp-safe, exactly-once accounting used for missed meals.
    #[serde(default)]
    pub total_meals: u32,
    /// Lifetime missed daily meals. Unlike the consecutive counter this does
    /// not reset after eating, which lets the health system apply every missed
    /// day exactly once even when a time warp crosses several day boundaries
    /// in one simulation update.
    #[serde(default)]
    pub total_missed_meals: u32,
}

impl Nutrition {
    pub fn record_meal(&mut self, day: u32) {
        if self.last_meal_day.is_none_or(|last_day| day > last_day) {
            self.last_meal_day = Some(day);
            self.total_meals = self.total_meals.saturating_add(1);
        }
        self.consecutive_missed_meals = 0;
    }

    pub fn record_missed_meal(&mut self) {
        self.consecutive_missed_meals = self.consecutive_missed_meals.saturating_add(1);
        self.total_missed_meals = self.total_missed_meals.saturating_add(1);
    }

    pub const fn is_hungry(self) -> bool {
        self.consecutive_missed_meals > 0
    }

    pub const fn condition(self) -> NutritionCondition {
        match self.consecutive_missed_meals {
            0 if self.last_meal_day.is_none() => NutritionCondition::Unassessed,
            0 => NutritionCondition::Fed,
            1..=2 => NutritionCondition::Hungry,
            3..=10 => NutritionCondition::Starving,
            _ => NutritionCondition::Critical,
        }
    }

    /// Food-conditioned Health ceiling as a percentage of the character's
    /// normal maximum. Ten hungry days can make someone extremely vulnerable,
    /// but ordinary hunger cannot itself reduce the ceiling below ten percent.
    pub const fn health_ceiling_percent(self) -> u8 {
        match self.consecutive_missed_meals {
            0 => 100,
            1 => 80,
            2 => 70,
            3 => 60,
            missed @ 4..=10 => 60 - (((missed - 3) * 50) / 7) as u8,
            _ => 10,
        }
    }
}

#[cfg(test)]
mod nutrition_tests {
    use super::*;

    #[test]
    fn hunger_conditions_lower_health_without_a_lethal_ceiling() {
        let expected = [80, 70, 60, 53, 46, 39, 32, 25, 18, 10];
        let mut nutrition = Nutrition::default();
        for (index, ceiling) in expected.into_iter().enumerate() {
            nutrition.record_missed_meal();
            assert_eq!(
                nutrition.health_ceiling_percent(),
                ceiling,
                "miss {}",
                index + 1
            );
        }
        assert_eq!(nutrition.condition(), NutritionCondition::Starving);
        nutrition.record_missed_meal();
        assert_eq!(nutrition.health_ceiling_percent(), 10);
        assert_eq!(nutrition.condition(), NutritionCondition::Critical);
    }

    #[test]
    fn one_meal_day_resets_hunger_and_is_counted_once() {
        let mut nutrition = Nutrition::default();
        nutrition.record_missed_meal();
        nutrition.record_meal(7);
        nutrition.record_meal(7);
        assert_eq!(nutrition.total_meals, 1);
        assert_eq!(nutrition.consecutive_missed_meals, 0);
        assert_eq!(nutrition.condition(), NutritionCondition::Fed);
        assert_eq!(nutrition.health_ceiling_percent(), 100);
    }
}

/// A permitted building that has not gone up yet.
///
/// Replicated so the settlement panel can honestly distinguish "approved" from
/// "standing" -- a decision and its result are separate events, and a panel that
/// showed only finished buildings would make construction invisible.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct ConstructionSite {
    pub kind: SettlementBuildingKind,
    pub settlement: String,
    /// False while the builder is still walking out; true once the plot is
    /// cleared and the frame is going up.
    ///
    /// A BOOL, not a progress float, and that is deliberate: a float would mark
    /// this component changed every tick and resend nearby detail without a
    /// meaningful state transition. This flips once. The client runs its own
    /// clock from the flip and uses [`SETTLEMENT_RAISE_SECONDS`],
    /// which both sides agree on.
    pub raising: bool,
    /// Where the builder stands to work — beside the plot, not on it.
    pub stand: Vec3,
    /// Which way the finished building will face.
    ///
    /// Replicated because the frame RISES before the building exists, and it
    /// has to rise already turned the right way. Without this the model came up
    /// unrotated and then snapped to its real bearing the instant it finished.
    pub rotation: f32,
}

/// How long a permitted building takes to rise, in seconds.
///
/// Shared because the server times it and the client animates against it; two
/// copies would drift and the building would pop or stall at the end.
pub const SETTLEMENT_RAISE_SECONDS: f32 = 10.0;

/// Where a builder stands to work on a plot.
///
/// Beside the footprint, in front of it, facing in — a villager standing in the
/// middle of their own building looks like a bug even when it is not, and once
/// the frame starts rising out of the ground they would be inside it.
pub fn builder_stand_position(plot: Vec3, rotation_y: f32, footprint_depth: f32) -> Vec3 {
    let offset = crate::rotation::local_to_world_xz(
        Vec2::new(0.0, -(footprint_depth * 0.5 + 1.6)),
        rotation_y,
    );
    Vec3::new(plot.x + offset.x, plot.y, plot.z + offset.y)
}

/// A building standing in a settlement.
///
/// Replicated as its own entity so the client can draw it without knowing any
/// of the rules that put it there.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct SettlementBuilding {
    pub kind: SettlementBuildingKind,
    /// Display label retained for panels and old-state migration. [`super::BuildingOf`]
    /// is the authoritative settlement relationship.
    pub settlement: String,
    /// Readable owner label retained for panels and old-state migration.
    /// [`super::OwnedBy`] is authoritative for rights and money.
    pub owner: Option<String>,
    /// How well the ground it stands on suits its trade, 0..1.
    ///
    /// Sampled ONCE, where it was built, and then carried. Recomputing it per
    /// tick would mark the component changed at tick rate and resend nearby
    /// detail without conveying a real state transition.
    pub quality: f32,
    /// Readable worker roster derived from employees for panels and legacy
    /// migration. [`super::EmployedAt`] is authoritative; fewer entries than
    /// `kind.positions()` means vacancies.
    pub workers: Vec<String>,
}

/// Replicated evidence that useful work is currently happening inside a
/// processing workplace.
///
/// This component is intentionally transient: the server adds it only while
/// one or more embodied workers can consume a real input batch and store its
/// output. Clients can therefore drive chimney smoke, machinery and sound from
/// production truth without replaying the server's inventory rules.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct WorkplaceOperation {
    pub active_workers: u8,
}

impl WorkplaceOperation {
    pub const fn is_active(self) -> bool {
        self.active_workers > 0
    }
}

/// Visual breathing room kept around the authored soil slab.
pub const FARM_FIELD_EDGE_CLEARANCE: f32 = 0.45;
/// Graded verge around a field. Terrain vertices are two metres apart, so the
/// level inner rectangle extends one complete sample beyond the visible soil;
/// otherwise bilinear interpolation can leave a model corner tilted.
pub const FARM_FIELD_TERRACE_MARGIN: f32 = 2.0;

/// Two fields are required for a Farmstead's full production capacity.
pub const FARM_FIELDS_PER_FARMSTEAD: u8 = 2;

/// Sideways distance from the Farmstead centre to either wheat-field centre.
///
/// Each field is four metres wide from its centre. Including the visual edge
/// clearance here leaves a clean gap between the two authored soil slabs.
pub const FARM_FIELD_LATERAL_OFFSET: f32 = 4.45;

/// A planted crop field belonging to one Farmstead.
///
/// Separate from `SettlementBuilding`: the field is a walkable environmental
/// prop, not architecture, and deliberately has no collider.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct FarmField {
    pub settlement: String,
    /// Farmstead plot position retained for layout and old-save migration.
    /// [`AttachedTo`](super::AttachedTo) is the authoritative parent link.
    pub farmstead: Vec3,
    /// Zero-based position in the Farmstead's two-field layout.
    #[serde(default)]
    pub plot_index: u8,
    pub quality: f32,
}

/// One fenced grazing plot belonging to a completed Livestock Farm.
///
/// Animals are deterministic client-side presentation children, not replicated
/// pathfinding people. The server owns this one plot record and all physical
/// production, inventory and worker behaviour.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct LivestockPasture {
    pub settlement: String,
    pub livestock_farm: Vec3,
    pub quality: f32,
}

/// The collider-free pier paired with one completed Fisherman's Hut.
///
/// Its [`PlayerPosition`] is the pier asset's landward origin at water level,
/// not terrain height. The hut position remains for layout and old-save
/// migration; [`AttachedTo`](super::AttachedTo) is authoritative.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct FishingPier {
    pub settlement: String,
    pub fishermans_hut: Vec3,
    pub quality: f32,
}

/// The residents assigned to one house.
///
/// A household is attached only to completed [`SettlementBuildingKind::House`]
/// entities. [`resident_ids`](Self::resident_ids) is authoritative. The
/// readable `residents` list is a derived UI/old-save mirror only: duplicate
/// or changed display names must never move a bed, a pantry contribution, or
/// a shopper assignment between people.
#[derive(Component, Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Household {
    #[serde(default)]
    pub resident_ids: Vec<super::PersonId>,
    /// Display-only roster derived from `resident_ids`.
    #[serde(default)]
    pub residents: Vec<String>,
}

/// Stable civic temperament used by the automatic Reeve. These are priorities,
/// not separate economies: every current strategy still trades through private
/// seller-owned offers.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CivicStrategy {
    #[default]
    Balanced,
    Frugal,
    Mercantile,
    MutualAid,
    Growth,
}

impl CivicStrategy {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Balanced => "Balanced",
            Self::Frugal => "Frugal",
            Self::Mercantile => "Mercantile",
            Self::MutualAid => "Mutual aid",
            Self::Growth => "Growth",
        }
    }
}

/// Whether the treasury may buy food for residents who cannot afford a meal.
/// `SurplusOnly` never creates stock or ignores scarcity: recent production
/// must cover the population and the enacted reserve floor must remain after
/// the purchase.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PoorReliefMode {
    #[default]
    Off,
    SurplusOnly,
}

impl PoorReliefMode {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::SurplusOnly => "Surplus only",
        }
    }

    pub const fn allows_purchase(self) -> bool {
        matches!(self, Self::SurplusOnly)
    }
}

/// How many of the tier's available public positions the settlement attempts
/// to fill. Treasury runway remains a hard constraint under every posture.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CivicStaffingPosture {
    Essential,
    #[default]
    Balanced,
    Full,
}

impl CivicStaffingPosture {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Essential => "Essential",
            Self::Balanced => "Balanced",
            Self::Full => "Full",
        }
    }

    pub const fn level(self) -> u8 {
        match self {
            Self::Essential => 0,
            Self::Balanced => 1,
            Self::Full => 2,
        }
    }

    pub fn targets(self, tier: SettlementTier) -> (u8, u8) {
        let workers = tier.public_worker_positions();
        let guards = tier.public_guard_positions();
        match self {
            Self::Essential => (workers.min(1), 0),
            Self::Balanced => (workers, guards.min(1)),
            Self::Full => (workers, guards),
        }
    }
}

/// Why the automatic Reeve last changed an enacted policy.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CivicPolicyReason {
    #[default]
    None,
    PayrollArrears,
    TreasuryStress,
    SustainableRelief,
    FoodStress,
    HealthySurplus,
}

impl CivicPolicyReason {
    pub const fn label(self) -> &'static str {
        match self {
            Self::None => "No adjustment",
            Self::PayrollArrears => "Civic payroll arrears",
            Self::TreasuryStress => "Low treasury runway",
            Self::SustainableRelief => "Sustainable food surplus",
            Self::FoodStress => "Food reserve stress",
            Self::HealthySurplus => "Healthy civic surplus",
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CivicPolicyAdjustment {
    #[default]
    None,
    RaisedMarketFee,
    LoweredMarketFee,
    RaisedProfitTax,
    LoweredProfitTax,
    EnabledPoorRelief,
    DisabledPoorRelief,
    RaisedGrowthSubsidy,
    LoweredGrowthSubsidy,
    ExpandedStaffing,
    ReducedStaffing,
}

impl CivicPolicyAdjustment {
    pub const fn label(self) -> &'static str {
        match self {
            Self::None => "No policy change",
            Self::RaisedMarketFee => "Raised market fee",
            Self::LoweredMarketFee => "Lowered market fee",
            Self::RaisedProfitTax => "Raised profit levy",
            Self::LoweredProfitTax => "Lowered profit levy",
            Self::EnabledPoorRelief => "Enabled Poor Relief",
            Self::DisabledPoorRelief => "Disabled Poor Relief",
            Self::RaisedGrowthSubsidy => "Raised growth subsidy",
            Self::LoweredGrowthSubsidy => "Lowered growth subsidy",
            Self::ExpandedStaffing => "Expanded civic staffing",
            Self::ReducedStaffing => "Reduced civic staffing",
        }
    }
}

/// Public rules chosen by a settlement rather than hidden simulation switches.
///
/// The first policy is deliberately narrow: it does not make food free. When
/// enabled, the settlement treasury may buy one market ration for a resident
/// whose personal wallet cannot, but only from sustainable surplus above the
/// configured emergency reserve. Public money, production and physical stock
/// can all constrain relief, so it softens unemployment without deleting
/// scarcity.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct SettlementPolicies {
    #[serde(default = "default_poor_relief")]
    pub poor_relief: PoorReliefMode,
    #[serde(default = "default_food_reserve_target_days")]
    pub food_reserve_target_days: u8,
    #[serde(default)]
    pub strategy: CivicStrategy,
    #[serde(default = "default_true")]
    pub autopilot: bool,
    #[serde(default = "default_market_fee_bps")]
    pub market_fee_bps: u16,
    #[serde(default = "default_business_profit_tax_bps")]
    pub business_profit_tax_bps: u16,
    #[serde(default = "default_civic_payroll_reserve_days")]
    pub civic_payroll_reserve_days: u8,
    #[serde(default)]
    pub staffing_posture: CivicStaffingPosture,
    /// Discount on settlement-requested private business permits. This is
    /// foregone permit revenue, not a treasury payment or newly created coin.
    #[serde(default = "default_business_permit_subsidy_bps")]
    pub business_permit_subsidy_bps: u16,
    #[serde(default = "unreviewed_day")]
    pub last_review_day: u32,
    #[serde(default = "unreviewed_day")]
    pub last_change_day: u32,
    #[serde(default)]
    pub last_adjustment: CivicPolicyAdjustment,
    #[serde(default)]
    pub last_reason: CivicPolicyReason,
}

const fn default_poor_relief() -> PoorReliefMode {
    PoorReliefMode::SurplusOnly
}

const fn default_food_reserve_target_days() -> u8 {
    3
}

const fn default_true() -> bool {
    true
}

const fn default_market_fee_bps() -> u16 {
    crate::economy::DEFAULT_MARKET_FEE_BPS
}

const fn default_business_profit_tax_bps() -> u16 {
    crate::economy::DEFAULT_BUSINESS_PROFIT_TAX_BPS
}

const fn default_civic_payroll_reserve_days() -> u8 {
    crate::economy::DEFAULT_CIVIC_PAYROLL_RESERVE_DAYS
}

const fn default_business_permit_subsidy_bps() -> u16 {
    crate::economy::DEFAULT_BUSINESS_PERMIT_SUBSIDY_BPS
}

const fn unreviewed_day() -> u32 {
    u32::MAX
}

impl Default for SettlementPolicies {
    fn default() -> Self {
        Self {
            poor_relief: default_poor_relief(),
            food_reserve_target_days: default_food_reserve_target_days(),
            strategy: CivicStrategy::Balanced,
            autopilot: true,
            market_fee_bps: default_market_fee_bps(),
            business_profit_tax_bps: default_business_profit_tax_bps(),
            civic_payroll_reserve_days: default_civic_payroll_reserve_days(),
            staffing_posture: CivicStaffingPosture::Balanced,
            business_permit_subsidy_bps: default_business_permit_subsidy_bps(),
            last_review_day: u32::MAX,
            last_change_day: u32::MAX,
            last_adjustment: CivicPolicyAdjustment::None,
            last_reason: CivicPolicyReason::None,
        }
    }
}

impl SettlementPolicies {
    /// Visual settlement seeds choose streets and centre form, not politics.
    /// Every foundation begins from the agreed Balanced charter; later Reeve
    /// reviews or player control may enact different values.
    pub fn from_foundation(_name: &str, _position: Vec3) -> Self {
        Self::default()
    }

    pub const fn poor_relief() -> Self {
        Self {
            poor_relief: PoorReliefMode::SurplusOnly,
            food_reserve_target_days: default_food_reserve_target_days(),
            strategy: CivicStrategy::Balanced,
            autopilot: true,
            market_fee_bps: default_market_fee_bps(),
            business_profit_tax_bps: default_business_profit_tax_bps(),
            civic_payroll_reserve_days: default_civic_payroll_reserve_days(),
            staffing_posture: CivicStaffingPosture::Balanced,
            business_permit_subsidy_bps: default_business_permit_subsidy_bps(),
            last_review_day: u32::MAX,
            last_change_day: u32::MAX,
            last_adjustment: CivicPolicyAdjustment::None,
            last_reason: CivicPolicyReason::None,
        }
    }
}

/// A short-lived request to hold one building's animated door open.
///
/// Multiple people can request the same position at once; the server folds
/// them into one [`BuildingDoorDemand`] on the building instead of replicating
/// every threshold interaction. Position is used rather than an entity
/// reference because every building already has a unique world plot in the
/// current village model.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct BuildingDoorUse {
    pub building: Vec3,
}

/// Stable, replicated aggregate door demand on a building.
///
/// [`BuildingDoorUse`] is the per-person server-side request. The server folds
/// those short threshold interactions into this persistent value on the
/// building, so a client cannot miss an open/close transition merely because
/// an actor component was inserted and removed between network snapshots.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BuildingDoorDemand {
    pub open: bool,
}

/// The rungs a settlement climbs. Every step asks for something the step below
/// did not -- see the tier table in WORLD-DESIGN section 1.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Default)]
pub enum SettlementTier {
    /// Reached by DESTRUCTION, deliberate razing, or long physical decay after
    /// abandonment -- never by economic decline alone.
    Ruins,
    /// A city hall and not much else. Where founding lands you.
    #[default]
    Hamlet,
    /// Feeds itself.
    Village,
    /// Trade and defence.
    Town,
    /// Leisure: something past survival.
    City,
}

impl SettlementTier {
    pub fn label(self) -> &'static str {
        match self {
            SettlementTier::Ruins => "RUINS",
            SettlementTier::Hamlet => "HAMLET",
            SettlementTier::Village => "VILLAGE",
            SettlementTier::Town => "TOWN",
            SettlementTier::City => "CITY",
        }
    }

    /// What this rung must acquire to reach the next one. `None` at the top.
    pub fn next_requirement(self) -> Option<&'static str> {
        match self {
            SettlementTier::Ruins => Some("refounding"),
            SettlementTier::Hamlet => Some("food security: fed, grown here or bought in"),
            SettlementTier::Village => Some("external trade and administration: a working market"),
            SettlementTier::Town => Some("regional pull: diverse work and real amenities"),
            SettlementTier::City => None,
        }
    }

    pub const fn public_guard_positions(self) -> u8 {
        match self {
            SettlementTier::Ruins => 0,
            SettlementTier::Hamlet => 0,
            SettlementTier::Village | SettlementTier::Town | SettlementTier::City => 2,
        }
    }

    pub const fn public_worker_positions(self) -> u8 {
        match self {
            SettlementTier::Ruins => 0,
            SettlementTier::Hamlet
            | SettlementTier::Village
            | SettlementTier::Town
            | SettlementTier::City => 2,
        }
    }
}

/// Who COMMANDS this unit: the lowercase account name of the player whose
/// orders it obeys.
///
/// Deliberately NOT the banner. A banner is affiliation -- who you side with --
/// and clans are joinable by several players (WORLD-DESIGN section 4), so
/// banner-as-command would hand your units to anyone who joined your clan. It
/// would also detonate at ROADMAP Phase 8, when the placeholder banner index
/// becomes a real ClanId.
///
/// Keyed by ACCOUNT NAME rather than `PeerId` because peer ids are random per
/// session: a retinue keyed by account survives a disconnect with no repair,
/// whereas `Hero::owner` has to be re-pointed by hand on every reconnect.
///
/// Replicated so the client can show at a glance what it may order -- but the
/// client's copy is display only. The server checks this component itself before
/// moving anything.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct CommandedBy(pub String);

/// Who a character answers to.
///
/// `None` is unaffiliated, and per WORLD-DESIGN section 4 that is a normal and
/// permanent state, not a gap waiting to be filled.
///
/// The index points into [`crate::names::BANNERS`], a placeholder roster that
/// stands in until clans are real (ROADMAP Phase 8), at which point this becomes
/// a `ClanId`. Server-authoritative and replicated: affiliation decides who is
/// hostile to whom, so it can never be a client-side label.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct CharacterAffiliation(pub Option<u8>);

impl CharacterAffiliation {
    pub fn label(self) -> &'static str {
        crate::names::banner_name(self.0).unwrap_or("UNAFFILIATED")
    }

    /// Step through the banner roster, wrapping via unaffiliated.
    ///
    /// Unaffiliated is part of the cycle rather than a separate control, so god
    /// mode can always get back to it without a second button.
    pub fn cycled(self, step: i16) -> Self {
        let len = crate::names::BANNERS.len() as i16;
        // 0 = unaffiliated, 1..=len = banner index + 1.
        let current = self.0.map(|i| i as i16 + 1).unwrap_or(0);
        let next = (current + step).rem_euclid(len + 1);
        Self(if next == 0 {
            None
        } else {
            Some((next - 1) as u8)
        })
    }
}

/// Wardrobe slots a hero can carry. The shipped model uses three
/// (bottom/top/hair); the spare capacity lets the art build add a slot
/// without a protocol change, since this component is replicated.
pub const HERO_SLOT_MAX: usize = 6;

/// Chosen wardrobe for a hero. Replicated with the hero so every client
/// dresses the same character.
///
/// Deliberately holds only INDICES: the node names, slot order and skin
/// palette live in the art build's manifest
/// ([`crate::character::CharacterManifest`]), so adding a hairstyle is an
/// asset change, not a protocol change. Every index is clamped on use —
/// a stale or hostile client can never index out of bounds.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq)]
pub struct HeroOutfit {
    /// Item index per wardrobe slot, in manifest slot order.
    pub slots: [u8; HERO_SLOT_MAX],
    /// Index into the manifest's skin tones.
    pub skin: u8,
}

impl Default for HeroOutfit {
    fn default() -> Self {
        // First item of every slot + first skin tone: valid for any manifest
        // without reading one. The creator seeds from the manifest's declared
        // defaults instead (see `from_manifest`).
        Self {
            slots: [0; HERO_SLOT_MAX],
            skin: 0,
        }
    }
}

impl HeroOutfit {
    /// The look the art build declares as default.
    /// A deterministic outfit for a generated person, so a crowd is not
    /// identical twins. Indices are clamped on use, so a value beyond a slot's
    /// item count simply lands on the last item rather than panicking.
    pub fn varied(seed: u64) -> Self {
        let mut rng = crate::rng::XorShift64::new(seed ^ 0x5DEE_CE66_D3A1_9B0F);
        let mut slots = [0u8; HERO_SLOT_MAX];
        for slot in slots.iter_mut() {
            *slot = (rng.next_u64() % 6) as u8;
        }
        Self {
            slots,
            skin: (rng.next_u64() % 6) as u8,
        }
    }

    pub fn from_manifest(manifest: &crate::character::CharacterManifest) -> Self {
        let mut outfit = Self::default();
        for (index, slot) in manifest.slots.iter().take(HERO_SLOT_MAX).enumerate() {
            outfit.slots[index] = slot.default_index();
        }
        outfit.skin = manifest.default_skin_index();
        outfit
    }

    /// Item index chosen for a slot (0 for slots beyond capacity).
    pub fn slot(&self, slot_index: usize) -> u8 {
        self.slots.get(slot_index).copied().unwrap_or(0)
    }

    /// Cycle a slot's selection by `step`, wrapping over `item_count`.
    pub fn cycle_slot(&mut self, slot_index: usize, step: i16, item_count: usize) {
        if slot_index >= HERO_SLOT_MAX || item_count == 0 {
            return;
        }
        let count = item_count as i16;
        let next = (self.slots[slot_index] as i16 + step).rem_euclid(count);
        self.slots[slot_index] = next as u8;
    }

    /// Cycle the skin tone by `step`, wrapping over `tone_count`.
    pub fn cycle_skin(&mut self, step: i16, tone_count: usize) {
        if tone_count == 0 {
            return;
        }
        let count = tone_count as i16;
        self.skin = (self.skin as i16 + step).rem_euclid(count) as u8;
    }

    /// True when `node` is a wardrobe item this outfit does NOT wear.
    ///
    /// Exactly one item per slot may show: overlapping garments intersect and
    /// read as an untextured patch (CHARACTER_PIPELINE.md §8).
    pub fn hides_node(&self, manifest: &crate::character::CharacterManifest, node: &str) -> bool {
        for (index, slot) in manifest.slots.iter().enumerate() {
            if !slot.items.iter().any(|item| item == node) {
                continue;
            }
            return slot.item(self.slot(index)) != Some(node);
        }
        false
    }
}

/// Player position component - replicated across network.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct PlayerPosition(pub Vec3);

/// Authoritative world-space velocity for an embodied character.
///
/// Position snapshots normally arrive less frequently than the server's 60 Hz
/// movement step. Replicating velocity only when direction or speed changes
/// lets clients render continuous motion between snapshots without predicting
/// decisions, routes, or arrivals themselves.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Default)]
pub struct CharacterMotion {
    pub velocity: Vec3,
}

impl CharacterMotion {
    pub const STATIONARY: Self = Self {
        velocity: Vec3::ZERO,
    };

    pub fn new(velocity: Vec3) -> Self {
        Self {
            velocity: if velocity.is_finite() {
                velocity
            } else {
                Vec3::ZERO
            },
        }
    }

    pub fn is_moving(self) -> bool {
        self.velocity.length_squared() > 0.01
    }
}

/// Player rotation (yaw only for simplicity) - replicated across network.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
pub struct PlayerRotation(pub f32);

/// Marker for the local player (client-side only).
#[derive(Component)]
pub struct LocalPlayer;

/// Marker for ground/terrain.
#[derive(Component)]
pub struct Ground;

#[cfg(test)]
mod affiliation_tests {
    use super::*;

    /// Unaffiliated must be reachable in the cycle, or god mode could set a
    /// banner and never take it off again.
    #[test]
    fn cycling_returns_to_unaffiliated() {
        let mut current = CharacterAffiliation::default();
        assert_eq!(current.0, None);

        let steps = crate::names::BANNERS.len() + 1;
        let mut seen_none = 0;
        for _ in 0..steps {
            current = current.cycled(1);
            if current.0.is_none() {
                seen_none += 1;
            }
        }
        assert_eq!(
            current.0, None,
            "a full cycle did not return to unaffiliated"
        );
        assert_eq!(
            seen_none, 1,
            "unaffiliated appeared {seen_none} times in one cycle"
        );
    }

    /// Every banner must be reachable, and no index may fall outside the roster
    /// -- an out-of-range index renders as UNAFFILIATED and would look like the
    /// setting silently failed.
    #[test]
    fn cycling_visits_every_banner_and_stays_in_range() {
        let mut current = CharacterAffiliation::default();
        let mut visited = std::collections::HashSet::new();
        for _ in 0..crate::names::BANNERS.len() + 1 {
            current = current.cycled(1);
            if let Some(index) = current.0 {
                assert!(
                    (index as usize) < crate::names::BANNERS.len(),
                    "banner index {index} is outside the roster"
                );
                visited.insert(index);
            }
        }
        assert_eq!(
            visited.len(),
            crate::names::BANNERS.len(),
            "missed a banner"
        );
    }

    #[test]
    fn cycling_backwards_is_the_inverse() {
        for start in 0..crate::names::BANNERS.len() + 1 {
            let mut current = CharacterAffiliation::default();
            for _ in 0..start {
                current = current.cycled(1);
            }
            assert_eq!(
                current.cycled(1).cycled(-1),
                current,
                "step +1 then -1 moved"
            );
        }
    }

    #[test]
    fn unaffiliated_reads_as_unaffiliated() {
        assert_eq!(CharacterAffiliation(None).label(), "UNAFFILIATED");
        assert_eq!(
            CharacterAffiliation(Some(0)).label(),
            crate::names::BANNERS[0]
        );
        // Out of range must not panic.
        assert_eq!(CharacterAffiliation(Some(200)).label(), "UNAFFILIATED");
    }

    #[test]
    fn character_attributes_never_exceed_one_hundred() {
        let mut attributes = CharacterAttributes::new(250, 101, 100);
        assert_eq!(attributes.physique(), CharacterAttributes::MAX);
        assert_eq!(attributes.intelligence(), CharacterAttributes::MAX);
        assert_eq!(attributes.charm(), CharacterAttributes::MAX);
        assert_eq!(attributes.train_physique(50), 0);
        assert_eq!(attributes.train_intelligence(50), 0);
        assert_eq!(attributes.train_charm(50), 0);
    }

    #[test]
    fn generated_attributes_are_stable_and_varied() {
        let first = CharacterAttributes::from_seed(42);
        assert_eq!(first, CharacterAttributes::from_seed(42));
        assert_ne!(first, CharacterAttributes::from_seed(43));
        for value in [first.physique(), first.intelligence(), first.charm()] {
            assert!((8..=20).contains(&value));
        }
    }
}

#[cfg(test)]
mod civic_hall_level_tests {
    use super::*;

    #[test]
    fn settlement_tiers_map_to_the_authored_civic_ladder() {
        assert_eq!(
            CivicHallLevel::for_tier(SettlementTier::Hamlet),
            CivicHallLevel::Moot
        );
        assert_eq!(
            CivicHallLevel::for_tier(SettlementTier::Village),
            CivicHallLevel::Village
        );
        assert_eq!(
            CivicHallLevel::for_tier(SettlementTier::Town),
            CivicHallLevel::Town
        );
        assert_eq!(
            CivicHallLevel::for_tier(SettlementTier::City),
            CivicHallLevel::Town,
            "City deliberately retains the Town Hall until a fourth asset exists"
        );
    }

    #[test]
    fn founding_reservation_contains_every_supported_hall_footprint() {
        let reserved = CivicHallLevel::largest_supported()
            .building_type()
            .definition();
        let reserved_min = reserved.footprint_center - reserved.footprint * 0.5;
        let reserved_max = reserved.footprint_center + reserved.footprint * 0.5;

        for level in [
            CivicHallLevel::Moot,
            CivicHallLevel::Village,
            CivicHallLevel::Town,
        ] {
            let definition = level.building_type().definition();
            let minimum = definition.footprint_center - definition.footprint * 0.5;
            let maximum = definition.footprint_center + definition.footprint * 0.5;
            assert!(
                minimum.cmpge(reserved_min).all() && maximum.cmple(reserved_max).all(),
                "{} exceeds the founding civic reservation",
                level.label()
            );
        }

        assert!(
            reserved.root_footprint_radius() <= SettlementBuildingKind::Hall.clearance(),
            "the planning clearance must contain the complete future Hall shell"
        );
    }
}

#[cfg(test)]
mod house_appearance_tests {
    use super::*;
    use crate::building::BuildingType;

    #[test]
    fn every_house_line_and_level_maps_to_its_matching_asset() {
        for (line, level, expected) in [
            (HouseLine::Cabin, HouseLevel::Ground, BuildingType::LogCabin),
            (
                HouseLine::Cabin,
                HouseLevel::UpperStorey,
                BuildingType::CabinL2,
            ),
            (
                HouseLine::LongCabin,
                HouseLevel::Ground,
                BuildingType::LongCabin,
            ),
            (
                HouseLine::LongCabin,
                HouseLevel::UpperStorey,
                BuildingType::LongCabinL2,
            ),
        ] {
            assert_eq!(HouseAppearance { line, level }.building_type(), expected);
        }
    }

    #[test]
    fn plot_pick_is_stable_and_tier_only_changes_the_rung() {
        let plot = Vec3::new(137.25, 0.0, -82.75);
        let hamlet = HouseAppearance::for_new_house(SettlementTier::Hamlet, plot);
        assert_eq!(
            hamlet,
            HouseAppearance::for_new_house(SettlementTier::Hamlet, plot)
        );
        let village = HouseAppearance::for_new_house(SettlementTier::Village, plot);
        assert_eq!(hamlet.line, village.line);
        assert_eq!(hamlet.level, HouseLevel::Ground);
        assert_eq!(village.level, HouseLevel::UpperStorey);
    }

    #[test]
    fn planning_envelope_contains_both_upgrade_lines() {
        let reserved = SettlementBuildingKind::House.placement_definition();
        for art in [BuildingType::CabinL2, BuildingType::LongCabinL2] {
            let actual = art.definition();
            assert!(reserved.footprint.cmpge(actual.footprint).all());
        }
    }
}
