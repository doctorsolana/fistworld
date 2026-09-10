//! Settlement building taxonomy and shared gameplay/asset definitions.

use super::{HouseAppearance, SettlementTier, FARM_FIELD_LATERAL_OFFSET};
use bevy::prelude::*;
use serde::{Deserialize, Serialize};

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
            SettlementBuildingKind::Tavern => Art::Tavern,
            SettlementBuildingKind::Church => Art::Church,
            SettlementBuildingKind::Windmill => Art::Windmill,
            SettlementBuildingKind::Bakery => Art::Bakery,
            // Dedicated art can replace this semantic mapping without a save
            // migration. Keep its temporary solid box distinct from the
            // walkable open-air marketplace.
            SettlementBuildingKind::StorageHall => Art::StorageHall,
            SettlementBuildingKind::StoneQuarry => Art::StoneQuarry,
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
        if self == SettlementBuildingKind::Tavern {
            let mut definition = self.art().definition();
            definition.footprint = Vec2::new(9.4, 13.4);
            definition.footprint_center = Vec2::new(0.0, -2.3);
            return definition;
        }
        if self != SettlementBuildingKind::House {
            return self.art().definition();
        }
        let mut definition = crate::building::BuildingType::LongCabinL2.definition();
        let cabin = crate::building::BuildingType::CabinL2.definition();
        let minimum = (definition.footprint_center - definition.footprint * 0.5)
            .min(cabin.footprint_center - cabin.footprint * 0.5);
        let maximum = (definition.footprint_center + definition.footprint * 0.5)
            .max(cabin.footprint_center + cabin.footprint * 0.5);
        definition.footprint = maximum - minimum;
        definition.footprint_center = (minimum + maximum) * 0.5;
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
            SettlementBuildingKind::House => Vec2::new(0.0, -4.30),
            SettlementBuildingKind::Market => Vec2::new(0.0, -6.5),
            SettlementBuildingKind::Tavern => Vec2::new(0.0, -4.85),
            SettlementBuildingKind::Church => Vec2::new(0.0, -6.5),
            SettlementBuildingKind::Windmill | SettlementBuildingKind::Bakery => {
                Vec2::new(0.0, -4.0)
            }
            SettlementBuildingKind::StorageHall => Vec2::new(0.0, -4.0),
            SettlementBuildingKind::StoneQuarry => Vec2::new(0.0, -4.8),
            SettlementBuildingKind::LivestockFarm => Vec2::new(0.0, -3.8),
        }
    }

    /// World-space staging point where a character enters or leaves this building.
    ///
    /// The authored anchor names the visible doorway, but the route goal must
    /// sit outside the character-clearance footprint. The livestock barn and
    /// upper-storey cabins have trim close enough to their anchor to put an
    /// unadjusted goal inside the navigation blocker. Houses use the reserved
    /// envelope so their entrance remains reachable after an appearance upgrade.
    pub fn entrance_position(self, plot: Vec3, rotation_y: f32) -> Vec3 {
        let definition = if self == Self::Tavern {
            // The patio is reserved land, not a solid wall in front of the door.
            self.art().definition()
        } else {
            self.placement_definition()
        };
        let mut anchor = self.door_offset();
        // Current building doors face local -Z. Keep a small gap beyond the
        // exact boundary so rotation and interpolation cannot turn it into a
        // tangent contact. door_offset() remains the unchanged art contract.
        let front = definition.footprint_center.y
            - definition.footprint.y * 0.5
            - crate::physics::CHARACTER_NAV_RADIUS
            - 0.05;
        anchor.y = anchor.y.min(front);
        let offset = crate::rotation::local_to_world_xz(anchor, rotation_y);
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
    use crate::components::MarketLevel;

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
