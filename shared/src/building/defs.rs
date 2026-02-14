use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::items::ItemType;

/// Types of buildings that can be constructed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum BuildingType {
    #[default]
    Windmill,
    Church,
    House05,
    House06,
    House07,
}

/// All building types with GLTF models (for collider baking).
pub const ALL_BUILDING_TYPES: &[BuildingType] = &[
    BuildingType::Windmill,
    BuildingType::Church,
    BuildingType::House05,
    BuildingType::House06,
    BuildingType::House07,
];

impl BuildingType {
    pub fn all() -> &'static [BuildingType] {
        &[
            BuildingType::Windmill,
            BuildingType::Church,
            BuildingType::House05,
            BuildingType::House06,
            BuildingType::House07,
        ]
    }

    /// Stable string id used by the collider bake manifest / database.
    pub const fn id(&self) -> &'static str {
        match self {
            BuildingType::Windmill => "building_windmill",
            BuildingType::Church => "building_church",
            BuildingType::House05 => "building_house_05",
            BuildingType::House06 => "building_house_06",
            BuildingType::House07 => "building_house_07",
        }
    }

    /// GLTF scene path for this building type.
    pub const fn scene_path(&self) -> Option<&'static str> {
        match self {
            BuildingType::Windmill => {
                Some("game_assets/buildings/village/Bld_Windmill_01.glb#Scene0")
            }
            BuildingType::Church => Some("game_assets/buildings/village/Church.glb#Scene0"),
            BuildingType::House05 => Some("game_assets/buildings/village/House_05.glb#Scene0"),
            BuildingType::House06 => Some("game_assets/buildings/village/House_06.glb#Scene0"),
            BuildingType::House07 => Some("game_assets/buildings/village/House_07.glb#Scene0"),
        }
    }

    pub const fn has_baked_collider(&self) -> bool {
        self.scene_path().is_some()
    }

    pub fn definition(&self) -> BuildingDef {
        match self {
            BuildingType::Windmill => BuildingDef {
                building_type: *self,
                display_name: "Windmill",
                cost: &[(ItemType::Wood, 20), (ItemType::Stone, 10)],
                footprint: Vec2::new(8.0, 8.0),
                height: 10.0,
                flatten_radius: 2.0,
                color: Color::srgb(0.7, 0.65, 0.55),
                model_path: Some("game_assets/buildings/village/Bld_Windmill_01.glb#Scene0"),
            },
            BuildingType::Church => BuildingDef {
                building_type: *self,
                display_name: "Church",
                cost: &[(ItemType::Wood, 30), (ItemType::Stone, 25)],
                footprint: Vec2::new(12.0, 8.0),
                height: 12.0,
                flatten_radius: 3.0,
                color: Color::srgb(0.75, 0.72, 0.68),
                model_path: Some("game_assets/buildings/village/Church.glb#Scene0"),
            },
            BuildingType::House05 => BuildingDef {
                building_type: *self,
                display_name: "House 05",
                cost: &[(ItemType::Wood, 8), (ItemType::Stone, 6)],
                footprint: Vec2::new(7.0, 6.0),
                height: 5.0,
                flatten_radius: 1.5,
                color: Color::srgb(0.65, 0.6, 0.55),
                model_path: Some("game_assets/buildings/village/House_05.glb#Scene0"),
            },
            BuildingType::House06 => BuildingDef {
                building_type: *self,
                display_name: "House 06",
                cost: &[(ItemType::Wood, 8), (ItemType::Stone, 6)],
                footprint: Vec2::new(7.0, 6.0),
                height: 5.0,
                flatten_radius: 1.5,
                color: Color::srgb(0.65, 0.6, 0.55),
                model_path: Some("game_assets/buildings/village/House_06.glb#Scene0"),
            },
            BuildingType::House07 => BuildingDef {
                building_type: *self,
                display_name: "House 07",
                cost: &[(ItemType::Wood, 9), (ItemType::Stone, 7)],
                footprint: Vec2::new(8.0, 6.5),
                height: 6.0,
                flatten_radius: 1.75,
                color: Color::srgb(0.68, 0.62, 0.58),
                model_path: Some("game_assets/buildings/village/House_07.glb#Scene0"),
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
    /// Resource cost as (ItemType, quantity) pairs.
    pub cost: &'static [(ItemType, u32)],
    /// Building footprint in meters (width x depth).
    pub footprint: Vec2,
    /// Building height in meters.
    pub height: f32,
    /// Extra radius around footprint for terrain flattening (smooth transition).
    pub flatten_radius: f32,
    /// Building color (for dummy mesh fallback).
    pub color: Color,
    /// Optional GLTF model path (if None, uses generated box mesh).
    pub model_path: Option<&'static str>,
}

impl BuildingDef {
    /// Get the total area that needs terrain flattening.
    pub fn flatten_footprint(&self) -> Vec2 {
        Vec2::new(
            self.footprint.x + self.flatten_radius * 2.0,
            self.footprint.y + self.flatten_radius * 2.0,
        )
    }
}
