use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Types of buildings that can be constructed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum BuildingType {
    /// Authored buildings are built in-repo from `asset_creation/houses/`.
    /// Keep existing variants in this order: binary replication and baked data
    /// use their enum discriminants, so new variants are appended below.
    #[default]
    LogCabin,
    LumberjackHut,
    Farmstead,
    MootHall,
    FishermansHut,
    /// The permanent earthen market square. This keeps the old market
    /// discriminant so replicated state and saves remain stable; the alias
    /// reads pre-art RON data that still calls it `PlaceholderMarket`.
    #[serde(alias = "PlaceholderMarket")]
    Market,
    /// Generated blockouts used by the settlement simulation until authored
    /// civic art arrives. They deliberately have no scene path: the client
    /// draws their definitions as simple coloured boxes.
    PlaceholderTavern,
    PlaceholderChurch,
    VillageHall,
    TownHall,
    /// First processing industries. Appended here originally as blockouts;
    /// keep their position stable because replicated discriminants and baked
    /// collider records depend on it. The aliases read pre-art RON data while
    /// all newly serialized state uses the permanent semantic names.
    #[serde(alias = "PlaceholderWindmill")]
    Windmill,
    #[serde(alias = "PlaceholderBakery")]
    Bakery,
    /// Paved upgrade of the market square. It deliberately shares the exact
    /// same anchors and footprint as `Market`, so promotion never moves it.
    MarketPaved,
    /// Authored private depot. Retains the original blockout's position so
    /// binary replication discriminants remain unchanged.
    StorageHall,
    /// Temporary solid blockout for the first stone-extraction trade. The
    /// semantic building remains `StoneQuarry`, so authored quarry art can
    /// replace this without touching saves or economy code.
    PlaceholderStoneQuarry,
    /// The sheep barn. Its pasture stays a separately replicated and
    /// rendered entity. Keeps the old blockout's discriminant; the alias
    /// reads pre-art RON data that still calls it `PlaceholderLivestockFarm`.
    #[serde(alias = "PlaceholderLivestockFarm")]
    LivestockFarm,
    /// Additional authored house art. Appended to preserve every existing
    /// replicated discriminant and baked-collider identity.
    LongCabin,
    CabinL2,
    LongCabinL2,
}

/// All authored building types. Collider baking filters this list through
/// [`BuildingType::has_baked_collider`] because open markets stay walkable.
pub const ALL_BUILDING_TYPES: &[BuildingType] = &[
    BuildingType::LogCabin,
    BuildingType::LumberjackHut,
    BuildingType::Farmstead,
    BuildingType::MootHall,
    BuildingType::VillageHall,
    BuildingType::TownHall,
    BuildingType::FishermansHut,
    BuildingType::Windmill,
    BuildingType::Bakery,
    BuildingType::Market,
    BuildingType::MarketPaved,
    BuildingType::LongCabin,
    BuildingType::CabinL2,
    BuildingType::LongCabinL2,
    BuildingType::LivestockFarm,
    BuildingType::StorageHall,
];

impl BuildingType {
    pub fn all() -> &'static [BuildingType] {
        ALL_BUILDING_TYPES
    }

    /// Stable string id used by the collider bake manifest / database.
    pub const fn id(&self) -> &'static str {
        match self {
            BuildingType::LogCabin => "building_log_cabin",
            BuildingType::LumberjackHut => "building_lumberjack_hut",
            BuildingType::Farmstead => "building_farmstead",
            BuildingType::MootHall => "building_moot_hall",
            BuildingType::VillageHall => "building_village_hall",
            BuildingType::TownHall => "building_town_hall",
            BuildingType::FishermansHut => "building_fishermans_hut",
            BuildingType::Market => "building_market",
            BuildingType::PlaceholderTavern => "placeholder_tavern",
            BuildingType::PlaceholderChurch => "placeholder_church",
            BuildingType::Windmill => "building_windmill",
            BuildingType::Bakery => "building_bakery",
            BuildingType::MarketPaved => "building_market_paved",
            BuildingType::StorageHall => "building_storage_hall",
            BuildingType::PlaceholderStoneQuarry => "placeholder_stone_quarry",
            BuildingType::LivestockFarm => "building_livestock_farm",
            BuildingType::LongCabin => "building_long_cabin",
            BuildingType::CabinL2 => "building_cabin_l2",
            BuildingType::LongCabinL2 => "building_long_cabin_l2",
        }
    }

    /// GLTF scene path for this building type.
    pub const fn scene_path(&self) -> Option<&'static str> {
        match self {
            BuildingType::LogCabin => Some("game_assets/buildings/village/LogCabin.glb#Scene0"),
            BuildingType::LumberjackHut => {
                Some("game_assets/buildings/village/LumberjackHut.glb#Scene0")
            }
            BuildingType::Farmstead => Some("game_assets/buildings/village/Farmstead.glb#Scene0"),
            BuildingType::MootHall => Some("game_assets/buildings/village/MootHall.glb#Scene0"),
            BuildingType::VillageHall => {
                Some("game_assets/buildings/village/VillageHall.glb#Scene0")
            }
            BuildingType::TownHall => Some("game_assets/buildings/village/TownHall.glb#Scene0"),
            BuildingType::FishermansHut => {
                Some("game_assets/buildings/village/FishermansHut.glb#Scene0")
            }
            BuildingType::Windmill => Some("game_assets/buildings/village/WindMill.glb#Scene0"),
            BuildingType::Bakery => Some("game_assets/buildings/village/Bakery.glb#Scene0"),
            BuildingType::Market => Some("game_assets/buildings/village/Market.glb#Scene0"),
            BuildingType::MarketPaved => {
                Some("game_assets/buildings/village/MarketPaved.glb#Scene0")
            }
            BuildingType::PlaceholderTavern | BuildingType::PlaceholderChurch => None,
            BuildingType::StorageHall => {
                Some("game_assets/buildings/village/StorageHall.glb#Scene0")
            }
            BuildingType::PlaceholderStoneQuarry => None,
            BuildingType::LivestockFarm => {
                Some("game_assets/buildings/village/LivestockFarm.glb#Scene0")
            }
            BuildingType::LongCabin => Some("game_assets/buildings/village/LongCabin.glb#Scene0"),
            BuildingType::CabinL2 => Some("game_assets/buildings/village/CabinL2.glb#Scene0"),
            BuildingType::LongCabinL2 => {
                Some("game_assets/buildings/village/LongCabinL2.glb#Scene0")
            }
        }
    }

    pub const fn has_baked_collider(&self) -> bool {
        self.scene_path().is_some() && !matches!(self, Self::Market | Self::MarketPaved)
    }

    /// Whether the coarse server navigation cache should treat the whole
    /// authored footprint as solid. Markets are open-air walkable squares:
    /// blocking their 12 x 12 m plot would prevent customers and traders from
    /// reaching the stalls even though their render scenes have no collider.
    pub const fn blocks_ground_navigation(self) -> bool {
        !matches!(self, Self::Market | Self::MarketPaved)
    }

    pub const fn is_civic_hall(self) -> bool {
        matches!(self, Self::MootHall | Self::VillageHall | Self::TownHall)
    }

    pub fn definition(&self) -> BuildingDef {
        match self {
            // Measured off the glb, not guessed: X 6.00 (gable to gable) by Z 6.94 (the roof
            // overhang, which is wider than the 5.00 walls), 4.33 tall from the sunk foundation
            // at -0.16 to the ridge at +4.17. See asset_creation/inspect_prop_glb.py.
            BuildingType::LogCabin => BuildingDef {
                building_type: *self,
                display_name: "Log Cabin",
                footprint: Vec2::new(6.0, 6.94),
                footprint_center: Vec2::ZERO,
                height: 4.33,
                flatten_radius: 1.5,
                color: Color::srgb(0.42, 0.28, 0.18),
                model_path: Some("game_assets/buildings/village/LogCabin.glb#Scene0"),
            },
            // Footprint spans the YARD as well as the house: the hut itself is only 3.60 x 4.20,
            // but the woodpile off the +X eave and the chopping block by the door are part of the
            // plot and part of the collider. Measured off the glb via inspect_prop_glb.py.
            BuildingType::LumberjackHut => BuildingDef {
                building_type: *self,
                display_name: "Lumberjack Hut",
                footprint: Vec2::new(5.16, 5.40),
                footprint_center: Vec2::new(0.3296, -0.1800),
                height: 3.76,
                flatten_radius: 1.4,
                color: Color::srgb(0.40, 0.27, 0.17),
                model_path: Some("game_assets/buildings/village/LumberjackHut.glb#Scene0"),
            },
            // The FIELD is a separate asset (PropKind::WheatField) because it must be walkable —
            // see the note on its manifest absence. This footprint is the house and farmyard only;
            // Farmstead.glb ships an `Anchor_Field` empty marking where the field belongs.
            BuildingType::Farmstead => BuildingDef {
                building_type: *self,
                display_name: "Farmstead",
                footprint: Vec2::new(5.41, 6.62),
                footprint_center: Vec2::new(0.1354, -0.1704),
                height: 4.07,
                flatten_radius: 1.6,
                color: Color::srgb(0.44, 0.30, 0.19),
                model_path: Some("game_assets/buildings/village/Farmstead.glb#Scene0"),
            },
            // Two storeys, but a village hall rather than a courthouse: 6.45 x 8.74 on the ground,
            // 8.18 m to the tip of the bell cupola's finial. Measured off the glb.
            BuildingType::MootHall => BuildingDef {
                building_type: *self,
                display_name: "Moot Hall",
                footprint: Vec2::new(6.45, 8.74),
                footprint_center: Vec2::new(0.0, -0.2300),
                height: 8.18,
                flatten_radius: 2.2,
                color: Color::srgb(0.43, 0.29, 0.18),
                model_path: Some("game_assets/buildings/village/MootHall.glb#Scene0"),
            },
            BuildingType::VillageHall => BuildingDef {
                building_type: *self,
                display_name: "Village Hall",
                footprint: Vec2::new(7.7779, 10.4400),
                footprint_center: Vec2::new(0.0, 0.6200),
                height: 9.44,
                flatten_radius: 2.4,
                color: Color::srgb(0.45, 0.36, 0.27),
                model_path: Some("game_assets/buildings/village/VillageHall.glb#Scene0"),
            },
            BuildingType::TownHall => BuildingDef {
                building_type: *self,
                display_name: "Town Hall",
                footprint: Vec2::new(10.2400, 14.4900),
                footprint_center: Vec2::new(0.0, 2.6450),
                height: 21.62,
                flatten_radius: 2.8,
                color: Color::srgb(0.47, 0.44, 0.39),
                model_path: Some("game_assets/buildings/village/TownHall.glb#Scene0"),
            },
            // The PIER is a separate asset (PropKind::FishingPier) because it must be walkable —
            // a convex hull over hut + jetty would enclose the open water between them. This
            // footprint is the hut and its shore yard; FishermansHut.glb ships `Anchor_Pier`
            // marking where the pier's landward end butts on.
            BuildingType::FishermansHut => BuildingDef {
                building_type: *self,
                display_name: "Fisherman's Hut",
                footprint: Vec2::new(6.44, 6.51),
                footprint_center: Vec2::new(-0.4000, -0.5125),
                height: 3.86,
                flatten_radius: 1.4,
                color: Color::srgb(0.41, 0.28, 0.18),
                model_path: Some("game_assets/buildings/village/FishermansHut.glb#Scene0"),
            },
            // Open-air square: stalls, counters and paving are decoration, not
            // a solid hall. Both levels intentionally share geometry, anchors
            // and footprint so a settlement upgrade can swap scenes in place.
            BuildingType::Market => BuildingDef {
                building_type: *self,
                display_name: "Marketplace",
                footprint: Vec2::new(12.0, 12.0),
                footprint_center: Vec2::ZERO,
                height: 3.2,
                // Three metres of fully level apron prevents the 2 m terrain
                // grid's triangles from interpolating through the authored
                // floor edge; the remaining 1.8 m blends back to the meadow.
                flatten_radius: 4.8,
                color: Color::srgb(0.67, 0.48, 0.23),
                model_path: Some("game_assets/buildings/village/Market.glb#Scene0"),
            },
            BuildingType::MarketPaved => BuildingDef {
                building_type: *self,
                display_name: "Paved Marketplace",
                footprint: Vec2::new(12.0, 12.0),
                footprint_center: Vec2::ZERO,
                height: 3.2,
                flatten_radius: 4.8,
                color: Color::srgb(0.67, 0.48, 0.23),
                model_path: Some("game_assets/buildings/village/MarketPaved.glb#Scene0"),
            },
            BuildingType::StorageHall => BuildingDef {
                building_type: *self,
                display_name: "Storage Hall",
                footprint: Vec2::new(9.0, 7.0),
                footprint_center: Vec2::ZERO,
                height: 6.235,
                flatten_radius: 1.8,
                color: Color::srgb(0.47, 0.34, 0.21),
                model_path: Some("game_assets/buildings/village/StorageHall.glb#Scene0"),
            },
            BuildingType::PlaceholderStoneQuarry => BuildingDef {
                building_type: *self,
                display_name: "Stone Quarry (blockout)",
                footprint: Vec2::new(9.0, 8.0),
                footprint_center: Vec2::ZERO,
                height: 3.0,
                flatten_radius: 2.0,
                color: Color::srgb(0.43, 0.44, 0.42),
                model_path: None,
            },
            // Measured off LivestockFarm.glb (asset_creation/houses/inspect_prop_glb.py): the barn
            // with its hay porch, haystack and holding pen, X -4.26..+5.02 by Z -3.60..+4.14, base
            // sunk to -0.16 and the main ridge at +5.16. Anchor_Door is pinned to door_offset
            // (0, -3.8) at export, which is why the footprint centre sits off-origin. The PASTURE
            // is not part of this footprint: it is the replicated `LivestockPasture` 12 m behind.
            BuildingType::LivestockFarm => BuildingDef {
                building_type: *self,
                display_name: "Livestock Farm",
                footprint: Vec2::new(9.2790, 7.7440),
                footprint_center: Vec2::new(0.3795, 0.2720),
                height: 5.3200,
                flatten_radius: 2.2,
                color: Color::srgb(0.46, 0.30, 0.18),
                model_path: Some("game_assets/buildings/village/LivestockFarm.glb#Scene0"),
            },
            BuildingType::PlaceholderTavern => BuildingDef {
                building_type: *self,
                display_name: "Tavern (blockout)",
                footprint: Vec2::new(8.0, 7.0),
                footprint_center: Vec2::ZERO,
                height: 4.4,
                flatten_radius: 1.7,
                color: Color::srgb(0.52, 0.25, 0.16),
                model_path: None,
            },
            BuildingType::PlaceholderChurch => BuildingDef {
                building_type: *self,
                display_name: "Church (blockout)",
                footprint: Vec2::new(8.0, 12.0),
                footprint_center: Vec2::ZERO,
                height: 7.0,
                flatten_radius: 2.0,
                color: Color::srgb(0.58, 0.58, 0.54),
                model_path: None,
            },
            // Solid tower footprint only: the animated sails sweep 8.80 m
            // overhead but clear the cabin at every cap yaw. Reserving their
            // whole disc on the ground would make villagers avoid empty air.
            BuildingType::Windmill => BuildingDef {
                building_type: *self,
                display_name: "Windmill",
                footprint: Vec2::new(5.8180, 5.8180),
                footprint_center: Vec2::new(0.0, -0.4910),
                height: 12.58,
                flatten_radius: 1.8,
                color: Color::srgb(0.68, 0.60, 0.43),
                model_path: Some("game_assets/buildings/village/WindMill.glb#Scene0"),
            },
            BuildingType::Bakery => BuildingDef {
                building_type: *self,
                display_name: "Bakery",
                footprint: Vec2::new(7.0420, 8.2400),
                footprint_center: Vec2::new(0.0990, 0.7200),
                height: 5.33,
                flatten_radius: 1.6,
                color: Color::srgb(0.64, 0.37, 0.20),
                model_path: Some("game_assets/buildings/village/Bakery.glb#Scene0"),
            },
            // Long plan, eave entry and hipped roof. The footprint centre is
            // off-origin in Z because the porch projects past the front eave.
            BuildingType::LongCabin => BuildingDef {
                building_type: *self,
                display_name: "Long Cabin",
                footprint: Vec2::new(8.1180, 5.6000),
                footprint_center: Vec2::new(0.0000, -0.1900),
                height: 5.3800,
                flatten_radius: 1.9,
                color: Color::srgb(0.42, 0.28, 0.18),
                model_path: Some("game_assets/buildings/village/LongCabin.glb#Scene0"),
            },
            // Jettied two-storey upgrade of the original square cabin line.
            BuildingType::CabinL2 => BuildingDef {
                building_type: *self,
                display_name: "Cabin, Upper Storey",
                footprint: Vec2::new(6.4721, 7.3600),
                footprint_center: Vec2::ZERO,
                height: 6.9800,
                flatten_radius: 1.8,
                color: Color::srgb(0.45, 0.36, 0.27),
                model_path: Some("game_assets/buildings/village/CabinL2.glb#Scene0"),
            },
            // Matching upper-storey rung for the long cabin line, including
            // its balcony and stone chimney.
            BuildingType::LongCabinL2 => BuildingDef {
                building_type: *self,
                display_name: "Long Cabin, Upper Storey",
                footprint: Vec2::new(8.6866, 5.6721),
                footprint_center: Vec2::ZERO,
                height: 6.8920,
                flatten_radius: 2.1,
                color: Color::srgb(0.45, 0.36, 0.27),
                model_path: Some("game_assets/buildings/village/LongCabinL2.glb#Scene0"),
            },
        }
    }

    pub fn display_name(&self) -> &'static str {
        self.definition().display_name
    }
}

/// Definition of a building's properties.
#[derive(Debug, Clone)]
pub struct BuildingDef {
    pub building_type: BuildingType,
    pub display_name: &'static str,
    /// Building footprint in meters (width x depth).
    pub footprint: Vec2,
    /// Centre of that footprint relative to the model/root origin.
    ///
    /// This is deliberately separate from `BuildingPosition`: civic halls pin
    /// their root to one permanent doorway while larger levels grow behind it.
    pub footprint_center: Vec2,
    /// Building height in meters.
    pub height: f32,
    /// Total landscaped radius outside the footprint. Most buildings use all
    /// of it as a smooth transition; broad ground slabs can reserve an inner
    /// fully-level apron through [`BuildingDef::terrain_flat_margin`].
    pub flatten_radius: f32,
    /// Building color (for dummy mesh fallback).
    pub color: Color,
    /// Optional GLTF model path (if None, uses generated box mesh).
    pub model_path: Option<&'static str>,
}

impl BuildingDef {
    /// Fully level ground outside the visible footprint before blending starts.
    ///
    /// A market's slab is as wide as its declared footprint. Because terrain
    /// vertices are two metres apart, flattening only to the exact edge still
    /// lets an adjacent triangle interpolate through the floor. A three-metre
    /// apron keeps every contributing triangle below the authored ground.
    pub const fn terrain_flat_margin(&self) -> f32 {
        match self.building_type {
            BuildingType::Market | BuildingType::MarketPaved => 3.0,
            _ => 0.0,
        }
    }

    pub fn terrain_flat_half_extents(&self) -> Vec2 {
        self.footprint * 0.5 + Vec2::splat(self.terrain_flat_margin())
    }

    pub fn terrain_blend_width(&self) -> f32 {
        (self.flatten_radius - self.terrain_flat_margin()).max(0.0)
    }

    /// World-space X/Z centre of this model's rotated ground footprint.
    #[inline]
    pub fn world_footprint_center(&self, root: Vec3, rotation_y: f32) -> Vec2 {
        Vec2::new(root.x, root.z)
            + crate::rotation::local_to_world_xz(self.footprint_center, rotation_y)
    }

    /// Furthest footprint corner from the model/root origin.
    #[inline]
    pub fn root_footprint_radius(&self) -> f32 {
        let half = self.footprint * 0.5;
        [
            Vec2::new(-half.x, -half.y),
            Vec2::new(-half.x, half.y),
            Vec2::new(half.x, -half.y),
            Vec2::new(half.x, half.y),
        ]
        .into_iter()
        .map(|corner| (self.footprint_center + corner).length())
        .fold(0.0, f32::max)
    }

    /// Get the total area that needs terrain flattening.
    pub fn flatten_footprint(&self) -> Vec2 {
        Vec2::new(
            self.footprint.x + self.flatten_radius * 2.0,
            self.footprint.y + self.flatten_radius * 2.0,
        )
    }
}
