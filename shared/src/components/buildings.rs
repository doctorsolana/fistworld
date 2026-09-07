//! Built structures, visual upgrades, construction sites and workplace adjuncts.

use super::{SettlementBuildingKind, SettlementTier};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

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
        let minimum = reserved.footprint_center - reserved.footprint * 0.5;
        let maximum = reserved.footprint_center + reserved.footprint * 0.5;
        for art in [
            BuildingType::LogCabin,
            BuildingType::LongCabin,
            BuildingType::CabinL2,
            BuildingType::LongCabinL2,
        ] {
            let actual = art.definition();
            assert!(minimum
                .cmple(actual.footprint_center - actual.footprint * 0.5)
                .all());
            assert!(maximum
                .cmpge(actual.footprint_center + actual.footprint * 0.5)
                .all());
        }
    }
}
