use bevy::prelude::*;
use serde::{Deserialize, Serialize};


/// Types of buildings that can be constructed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum BuildingType {
    /// Every one of these is built in-repo from asset_creation/houses/. The
    /// bought sets -- desert, train, multistory, and the village buildings we
    /// did not make -- were deleted: this game uses four buildings.
    #[default]
    LogCabin,
    LumberjackHut,
    Farmstead,
    TownHall,
}

/// All building types with GLTF models (for collider baking).
pub const ALL_BUILDING_TYPES: &[BuildingType] = &[
    BuildingType::LogCabin,
    BuildingType::LumberjackHut,
    BuildingType::Farmstead,
    BuildingType::TownHall,
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
            BuildingType::TownHall => "building_town_hall",
        }
    }

    /// GLTF scene path for this building type.
    pub const fn scene_path(&self) -> Option<&'static str> {
        match self {
            BuildingType::LogCabin => {
                Some("game_assets/buildings/village/LogCabin.glb#Scene0")
            }
            BuildingType::LumberjackHut => {
                Some("game_assets/buildings/village/LumberjackHut.glb#Scene0")
            }
            BuildingType::Farmstead => {
                Some("game_assets/buildings/village/Farmstead.glb#Scene0")
            }
            BuildingType::TownHall => {
                Some("game_assets/buildings/village/TownHall.glb#Scene0")
            }
        }
    }

    pub const fn has_baked_collider(&self) -> bool {
        self.scene_path().is_some()
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
                height: 3.76,
                flatten_radius: 1.4,
                color: Color::srgb(0.40, 0.27, 0.17),
                model_path: Some("game_assets/buildings/village/LumberjackHut.glb#Scene0"),
            },
            // The FIELD is a separate asset (PropKind::Wheat_Field) because it must be walkable —
            // see the note on its manifest absence. This footprint is the house and farmyard only;
            // Farmstead.glb ships an `Anchor_Field` empty marking where the field belongs.
            BuildingType::Farmstead => BuildingDef {
                building_type: *self,
                display_name: "Farmstead",
                footprint: Vec2::new(5.41, 6.62),
                height: 4.07,
                flatten_radius: 1.6,
                color: Color::srgb(0.44, 0.30, 0.19),
                model_path: Some("game_assets/buildings/village/Farmstead.glb#Scene0"),
            },
            // Two storeys, but a village hall rather than a courthouse: 6.45 x 8.74 on the ground,
            // 8.18 m to the tip of the bell cupola's finial. Measured off the glb.
            BuildingType::TownHall => BuildingDef {
                building_type: *self,
                display_name: "Town Hall",
                footprint: Vec2::new(6.45, 8.74),
                height: 8.18,
                flatten_radius: 2.2,
                color: Color::srgb(0.43, 0.29, 0.18),
                model_path: Some("game_assets/buildings/village/TownHall.glb#Scene0"),
            },
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
