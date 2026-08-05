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
    /// First and repeat permits are all free in the pre-wallet prototype, so
    /// this stays at zero and says so honestly.
    pub treasury: u64,
}

/// The small public office operated from a settlement's Moot Hall.
///
/// This is separate from [`Settlement`] because administration is optional
/// state that can grow without making every old settlement constructor and
/// save record know about future civic jobs. The first office has one bounded
/// position: a road steward who audits paths and repairs connections to the
/// hall. It is replicated so clicking the hall exposes who holds the job and
/// what the public purse owes them.
#[derive(Component, Serialize, Deserialize, Clone, Debug, PartialEq, Eq)]
pub struct MootAdministration {
    /// Senior civic clerk. The Reeve remains accountable for permits and the
    /// public purse while the other two founding positions have physical jobs.
    #[serde(default)]
    pub reeve: Option<String>,
    /// Early-market porter who collects saleable output from businesses. This
    /// keeps farmers, fishers and woodcutters at their actual trades.
    #[serde(default)]
    pub market_porter: Option<String>,
    pub road_steward: Option<String>,
    /// Public safety positions. Guards are real named jobs even before guard
    /// patrol/combat behaviour is implemented.
    #[serde(default)]
    pub guards: Vec<String>,
    /// Public works positions. The first worker is also the road steward so
    /// existing road audits keep one clear accountable owner.
    #[serde(default)]
    pub city_workers: Vec<String>,
    pub road_steward_daily_salary: u64,
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
            market_porter: None,
            road_steward: None,
            guards: Vec::new(),
            city_workers: Vec::new(),
            road_steward_daily_salary: crate::economy::ROAD_STEWARD_DAILY_SALARY,
            wage_arrears: 0,
            roadless_buildings: 0,
            disconnected_buildings: 0,
            pending_road_buildings: 0,
            last_road_audit_day: 0,
        }
    }
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
    /// Tier-two civic/commercial blockouts. These are semantic buildings now;
    /// only their final art is temporary.
    Market,
    Tavern,
    Church,
}

impl SettlementBuildingKind {
    pub const fn is_civic(self) -> bool {
        matches!(
            self,
            SettlementBuildingKind::Hall
                | SettlementBuildingKind::Market
                | SettlementBuildingKind::Tavern
                | SettlementBuildingKind::Church
        )
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
        }
    }

    /// The art that stands in for this role today.
    ///
    /// A windmill is not a farmstead and a log cabin is not a moot hall; both
    /// read closely enough to test the systems, and swapping them later is one
    /// line here rather than a change to any rule.
    pub fn art(self) -> crate::building::BuildingType {
        use crate::building::BuildingType as Art;
        match self {
            SettlementBuildingKind::Hall => Art::MootHall,
            SettlementBuildingKind::Farmstead => Art::Farmstead,
            SettlementBuildingKind::LumberjackHut => Art::LumberjackHut,
            SettlementBuildingKind::FishermansHut => Art::FishermansHut,
            SettlementBuildingKind::House => Art::LogCabin,
            SettlementBuildingKind::Market => Art::PlaceholderMarket,
            SettlementBuildingKind::Tavern => Art::PlaceholderTavern,
            SettlementBuildingKind::Church => Art::PlaceholderChurch,
        }
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
            SettlementBuildingKind::LumberjackHut => profile.wood,
            // Fishing quality is geometry rather than a land resource: the
            // server measures navigable open water around the authored pier.
            SettlementBuildingKind::FishermansHut => 0.5,
            // A hall and a house harvest nothing. Neutral rather than zero, so
            // "quality" never reads as "this house is broken".
            SettlementBuildingKind::Hall
            | SettlementBuildingKind::House
            | SettlementBuildingKind::Market
            | SettlementBuildingKind::Tavern
            | SettlementBuildingKind::Church => 0.5,
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
            SettlementBuildingKind::Market => Some("Market Trader"),
            SettlementBuildingKind::Tavern => Some("Innkeeper"),
            SettlementBuildingKind::Church => Some("Cleric"),
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
            SettlementBuildingKind::Hall => 3,
            SettlementBuildingKind::House => 0,
            SettlementBuildingKind::Market => 2,
            SettlementBuildingKind::Tavern => 2,
            SettlementBuildingKind::Church => 1,
        }
    }

    /// Bounded bulk storage physically available at this place.
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
            | SettlementBuildingKind::Church => 0,
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
            SettlementBuildingKind::Market => Vec2::new(0.0, -4.0),
            SettlementBuildingKind::Tavern => Vec2::new(0.0, -4.0),
            SettlementBuildingKind::Church => Vec2::new(0.0, -6.5),
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
        }
    }

    /// Ground a building of this kind needs to itself, in metres.
    pub fn clearance(self) -> f32 {
        match self {
            SettlementBuildingKind::Hall => 10.0,
            SettlementBuildingKind::Farmstead => 12.0,
            SettlementBuildingKind::LumberjackHut => 9.0,
            SettlementBuildingKind::FishermansHut => 11.0,
            SettlementBuildingKind::House => 8.0,
            SettlementBuildingKind::Market => 11.0,
            SettlementBuildingKind::Tavern => 10.0,
            SettlementBuildingKind::Church => 12.0,
        }
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

/// Lowest terrain sample beneath a building footprint, including a small
/// apron. Founding uses this instead of the centre height: a dry centre with a
/// wet corner is still a drowned building.
pub fn minimum_building_ground(
    terrain: &crate::terrain::WorldTerrain,
    centre: Vec3,
    kind: SettlementBuildingKind,
    rotation_y: f32,
) -> f32 {
    const GRID: usize = 5;
    let half = kind.art().definition().footprint * 0.5 + Vec2::splat(0.25);
    let mut lowest = f32::INFINITY;
    for x_step in 0..GRID {
        for z_step in 0..GRID {
            let t_x = x_step as f32 / (GRID - 1) as f32;
            let t_z = z_step as f32 / (GRID - 1) as f32;
            let local = Vec2::new(-half.x + half.x * 2.0 * t_x, -half.y + half.y * 2.0 * t_z);
            let offset = crate::rotation::local_to_world_xz(local, rotation_y);
            lowest = lowest.min(terrain.get_height(centre.x + offset.x, centre.z + offset.y));
        }
    }
    lowest
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
    let half = kind.art().definition().footprint * 0.5 + Vec2::splat(0.25);
    let footprint = minimum_rotated_rect_water_clearance(terrain, centre, half, rotation_y);
    let door = kind.entrance_position(centre, rotation_y);
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
    if minimum_building_water_clearance(terrain, centre, SettlementBuildingKind::Hall, 0.0)
        < SETTLEMENT_FREEBOARD
    {
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

impl WorkStatus {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Employed => "Employed",
            Self::LookingForWork => "Looking for work",
            Self::Chilling => "Chilling",
        }
    }
}

/// One villager's meal history.
///
/// Settlement food security remains the planning aggregate; this is the small
/// per-person fact needed by inspection UI and, later, health and migration.
/// `None` means the villager has not crossed a simulated meal boundary yet --
/// importantly different from claiming they are either fed or hungry.
#[derive(Component, Serialize, Deserialize, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Nutrition {
    pub last_meal_day: Option<u32>,
    pub consecutive_missed_meals: u16,
}

impl Nutrition {
    pub fn record_meal(&mut self, day: u32) {
        self.last_meal_day = Some(day);
        self.consecutive_missed_meals = 0;
    }

    pub fn record_missed_meal(&mut self) {
        self.consecutive_missed_meals = self.consecutive_missed_meals.saturating_add(1);
    }

    pub const fn is_hungry(self) -> bool {
        self.consecutive_missed_meals > 0
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

/// Visual breathing room kept around the authored soil slab.
pub const FARM_FIELD_EDGE_CLEARANCE: f32 = 0.45;

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

/// The named residents assigned to one house.
///
/// A household is attached only to completed [`SettlementBuildingKind::House`]
/// entities. Keeping the roster on the cabin makes housing inspectable and
/// capacity-bounded without pretending that settlement residency alone tells
/// us where somebody sleeps.
#[derive(Component, Serialize, Deserialize, Clone, Debug, Default, PartialEq, Eq)]
pub struct Household {
    pub residents: Vec<String>,
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
    pub poor_relief: bool,
    #[serde(default = "default_poor_relief_reserve_days")]
    pub poor_relief_reserve_days: u8,
}

const fn default_poor_relief_reserve_days() -> u8 {
    3
}

impl Default for SettlementPolicies {
    fn default() -> Self {
        Self {
            poor_relief: false,
            poor_relief_reserve_days: default_poor_relief_reserve_days(),
        }
    }
}

impl SettlementPolicies {
    pub const fn poor_relief() -> Self {
        Self {
            poor_relief: true,
            poor_relief_reserve_days: default_poor_relief_reserve_days(),
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
            SettlementTier::Hamlet => 1,
            SettlementTier::Village | SettlementTier::Town | SettlementTier::City => 2,
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
