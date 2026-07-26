use bevy::prelude::*;
use serde::{Deserialize, Serialize};


/// Types of buildings that can be constructed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum BuildingType {
    #[default]
    Windmill,
    Church,
    House05,
    House06,
    House07,
    Multistory01,
    Multistory02,
    Multistory03,
    Multistory04,
    Multistory05,
    Multistory06,
    Multistory07,
    Multistory08,
    Multistory09,
}

/// All building types with GLTF models (for collider baking).
pub const ALL_BUILDING_TYPES: &[BuildingType] = &[
    BuildingType::Windmill,
    BuildingType::Church,
    BuildingType::House05,
    BuildingType::House06,
    BuildingType::House07,
    BuildingType::Multistory01,
    BuildingType::Multistory02,
    BuildingType::Multistory03,
    BuildingType::Multistory04,
    BuildingType::Multistory05,
    BuildingType::Multistory06,
    BuildingType::Multistory07,
    BuildingType::Multistory08,
    BuildingType::Multistory09,
];

impl BuildingType {
    pub fn all() -> &'static [BuildingType] {
        ALL_BUILDING_TYPES
    }

    /// Stable string id used by the collider bake manifest / database.
    pub const fn id(&self) -> &'static str {
        match self {
            BuildingType::Windmill => "building_windmill",
            BuildingType::Church => "building_church",
            BuildingType::House05 => "building_house_05",
            BuildingType::House06 => "building_house_06",
            BuildingType::House07 => "building_house_07",
            BuildingType::Multistory01 => "building_multistory_01",
            BuildingType::Multistory02 => "building_multistory_02",
            BuildingType::Multistory03 => "building_multistory_03",
            BuildingType::Multistory04 => "building_multistory_04",
            BuildingType::Multistory05 => "building_multistory_05",
            BuildingType::Multistory06 => "building_multistory_06",
            BuildingType::Multistory07 => "building_multistory_07",
            BuildingType::Multistory08 => "building_multistory_08",
            BuildingType::Multistory09 => "building_multistory_09",
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
            BuildingType::Multistory01 => {
                Some("game_assets/buildings/multistory/Multistory_01.glb#Scene0")
            }
            BuildingType::Multistory02 => {
                Some("game_assets/buildings/multistory/Multistory_02.glb#Scene0")
            }
            BuildingType::Multistory03 => {
                Some("game_assets/buildings/multistory/Multistory_03.glb#Scene0")
            }
            BuildingType::Multistory04 => {
                Some("game_assets/buildings/multistory/Multistory_04.glb#Scene0")
            }
            BuildingType::Multistory05 => {
                Some("game_assets/buildings/multistory/Multistory_05.glb#Scene0")
            }
            BuildingType::Multistory06 => {
                Some("game_assets/buildings/multistory/Multistory_06.glb#Scene0")
            }
            BuildingType::Multistory07 => {
                Some("game_assets/buildings/multistory/Multistory_07.glb#Scene0")
            }
            BuildingType::Multistory08 => {
                Some("game_assets/buildings/multistory/Multistory_08.glb#Scene0")
            }
            BuildingType::Multistory09 => {
                Some("game_assets/buildings/multistory/Multistory_09.glb#Scene0")
            }
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
                footprint: Vec2::new(8.0, 8.0),
                height: 10.0,
                flatten_radius: 2.0,
                color: Color::srgb(0.7, 0.65, 0.55),
                model_path: Some("game_assets/buildings/village/Bld_Windmill_01.glb#Scene0"),
            },
            BuildingType::Church => BuildingDef {
                building_type: *self,
                display_name: "Church",
                footprint: Vec2::new(12.0, 8.0),
                height: 12.0,
                flatten_radius: 3.0,
                color: Color::srgb(0.75, 0.72, 0.68),
                model_path: Some("game_assets/buildings/village/Church.glb#Scene0"),
            },
            BuildingType::House05 => BuildingDef {
                building_type: *self,
                display_name: "House 05",
                footprint: Vec2::new(7.0, 6.0),
                height: 5.0,
                flatten_radius: 1.5,
                color: Color::srgb(0.65, 0.6, 0.55),
                model_path: Some("game_assets/buildings/village/House_05.glb#Scene0"),
            },
            BuildingType::House06 => BuildingDef {
                building_type: *self,
                display_name: "House 06",
                footprint: Vec2::new(7.0, 6.0),
                height: 5.0,
                flatten_radius: 1.5,
                color: Color::srgb(0.65, 0.6, 0.55),
                model_path: Some("game_assets/buildings/village/House_06.glb#Scene0"),
            },
            BuildingType::House07 => BuildingDef {
                building_type: *self,
                display_name: "House 07",
                footprint: Vec2::new(8.0, 6.5),
                height: 6.0,
                flatten_radius: 1.75,
                color: Color::srgb(0.68, 0.62, 0.58),
                model_path: Some("game_assets/buildings/village/House_07.glb#Scene0"),
            },
            BuildingType::Multistory01 => {
                multistory_def(*self, "Multistory 01", Vec2::new(7.1446, 6.2987), 12.3559)
            }
            BuildingType::Multistory02 => {
                multistory_def(*self, "Multistory 02", Vec2::new(7.1446, 6.2987), 14.7378)
            }
            BuildingType::Multistory03 => {
                multistory_def(*self, "Multistory 03", Vec2::new(14.0670, 6.2987), 14.0470)
            }
            BuildingType::Multistory04 => {
                multistory_def(*self, "Multistory 04", Vec2::new(14.0670, 6.2987), 14.3836)
            }
            BuildingType::Multistory05 => {
                multistory_def(*self, "Multistory 05", Vec2::new(7.1446, 6.2987), 12.3559)
            }
            BuildingType::Multistory06 => {
                multistory_def(*self, "Multistory 06", Vec2::new(7.1446, 6.2987), 14.7378)
            }
            BuildingType::Multistory07 => {
                multistory_def(*self, "Multistory 07", Vec2::new(14.0670, 6.2987), 14.0470)
            }
            BuildingType::Multistory08 => {
                multistory_def(*self, "Multistory 08", Vec2::new(14.0670, 6.2987), 14.3836)
            }
            BuildingType::Multistory09 => {
                multistory_def(*self, "Multistory 09", Vec2::new(7.1446, 6.3741), 10.4102)
            }
        }
    }

    pub fn display_name(&self) -> &'static str {
        self.definition().display_name
    }
}

fn multistory_def(
    building_type: BuildingType,
    display_name: &'static str,
    footprint: Vec2,
    height: f32,
) -> BuildingDef {
    BuildingDef {
        building_type,
        display_name,
        footprint,
        height,
        flatten_radius: 1.5,
        color: Color::srgb(0.58, 0.56, 0.53),
        model_path: building_type.scene_path(),
    }
}

/// Definition of a building's properties.
#[derive(Debug, Clone)]
pub struct BuildingDef {
    pub building_type: BuildingType,
    pub display_name: &'static str,
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
