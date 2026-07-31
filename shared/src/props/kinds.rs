/// Stable identifier for a prop scene / asset type.
///
/// Names match the filenames in `game_assets/` exactly (without extension).
#[allow(non_camel_case_types)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PropKind {
    // environment/rocks
    Rock_1,
    Rock_2,
    Rock_3,
    Rock_4,
    Rock_5,

    // environment/trees
    Tree_01,
    Tree_02,
    Tree_08,
    Tree_09,
    Tree_10,
    Tree_18,
    Tree_29,

    // environment/trees_dead
    Dead_tree_1,
    Dead_tree_2,
    Dead_tree_3,

    // environment/trees_pine
    Pine_Tree_1,
    Pine_Tree_2,
    Pine_Tree_3,
    Pine_Tree_4,

    // environment/bushes
    Bush_01,
    Bush_02,
    Bush_03,
    Bush_04,

    // environment/flowers
    Flower_01,
    Flower_02,
    Flower_03,
    Flower_04,
    Flower_05,
    Spring_Flower_06,
    Spring_Flower_07,
    Spring_Flower_08,
    Spring_Flower_09,

    // environment/grass
    GrassBlade_9v,
    Env_Grass_Tall_04,
    Env_Grass_06,
    Env_Grass_07,

    // environment/crops — built in-repo by asset_creation/houses/build_wheat_field.py.
    // Deliberately has NO colliders_manifest.ron entry: a crop field must be walkable so farmers
    // can stand in it to harvest.
    Wheat_Field,
}

impl PropKind {
    /// Resolve a stable string id to a prop kind.
    #[inline]
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "rock_1" => Some(PropKind::Rock_1),
            "rock_2" => Some(PropKind::Rock_2),
            "rock_3" => Some(PropKind::Rock_3),
            "rock_4" => Some(PropKind::Rock_4),
            "rock_5" => Some(PropKind::Rock_5),
            "tree_01" => Some(PropKind::Tree_01),
            "tree_02" => Some(PropKind::Tree_02),
            "tree_08" => Some(PropKind::Tree_08),
            "tree_09" => Some(PropKind::Tree_09),
            "tree_10" => Some(PropKind::Tree_10),
            "tree_18" => Some(PropKind::Tree_18),
            "tree_29" => Some(PropKind::Tree_29),
            "dead_tree_1" => Some(PropKind::Dead_tree_1),
            "dead_tree_2" => Some(PropKind::Dead_tree_2),
            "dead_tree_3" => Some(PropKind::Dead_tree_3),
            "pine_tree_1" => Some(PropKind::Pine_Tree_1),
            "pine_tree_2" => Some(PropKind::Pine_Tree_2),
            "pine_tree_3" => Some(PropKind::Pine_Tree_3),
            "pine_tree_4" => Some(PropKind::Pine_Tree_4),
            "bush_01" => Some(PropKind::Bush_01),
            "bush_02" => Some(PropKind::Bush_02),
            "bush_03" => Some(PropKind::Bush_03),
            "bush_04" => Some(PropKind::Bush_04),
            "flower_01" => Some(PropKind::Flower_01),
            "flower_02" => Some(PropKind::Flower_02),
            "flower_03" => Some(PropKind::Flower_03),
            "flower_04" => Some(PropKind::Flower_04),
            "flower_05" => Some(PropKind::Flower_05),
            "spring_flower_06" => Some(PropKind::Spring_Flower_06),
            "spring_flower_07" => Some(PropKind::Spring_Flower_07),
            "spring_flower_08" => Some(PropKind::Spring_Flower_08),
            "spring_flower_09" => Some(PropKind::Spring_Flower_09),
            "grass_blade_9v" => Some(PropKind::GrassBlade_9v),
            "env_grass_tall_04" => Some(PropKind::Env_Grass_Tall_04),
            "env_grass_06" => Some(PropKind::Env_Grass_06),
            "env_grass_07" => Some(PropKind::Env_Grass_07),
            "wheat_field" => Some(PropKind::Wheat_Field),
            _ => None,
        }
    }

    /// Stable string id used by the collider bake manifest / database.
    pub const fn id(&self) -> &'static str {
        match self {
            PropKind::Rock_1 => "rock_1",
            PropKind::Rock_2 => "rock_2",
            PropKind::Rock_3 => "rock_3",
            PropKind::Rock_4 => "rock_4",
            PropKind::Rock_5 => "rock_5",
            PropKind::Tree_01 => "tree_01",
            PropKind::Tree_02 => "tree_02",
            PropKind::Tree_08 => "tree_08",
            PropKind::Tree_09 => "tree_09",
            PropKind::Tree_10 => "tree_10",
            PropKind::Tree_18 => "tree_18",
            PropKind::Tree_29 => "tree_29",
            PropKind::Dead_tree_1 => "dead_tree_1",
            PropKind::Dead_tree_2 => "dead_tree_2",
            PropKind::Dead_tree_3 => "dead_tree_3",
            PropKind::Pine_Tree_1 => "pine_tree_1",
            PropKind::Pine_Tree_2 => "pine_tree_2",
            PropKind::Pine_Tree_3 => "pine_tree_3",
            PropKind::Pine_Tree_4 => "pine_tree_4",
            PropKind::Bush_01 => "bush_01",
            PropKind::Bush_02 => "bush_02",
            PropKind::Bush_03 => "bush_03",
            PropKind::Bush_04 => "bush_04",
            PropKind::Flower_01 => "flower_01",
            PropKind::Flower_02 => "flower_02",
            PropKind::Flower_03 => "flower_03",
            PropKind::Flower_04 => "flower_04",
            PropKind::Flower_05 => "flower_05",
            PropKind::Spring_Flower_06 => "spring_flower_06",
            PropKind::Spring_Flower_07 => "spring_flower_07",
            PropKind::Spring_Flower_08 => "spring_flower_08",
            PropKind::Spring_Flower_09 => "spring_flower_09",
            PropKind::GrassBlade_9v => "grass_blade_9v",
            PropKind::Env_Grass_Tall_04 => "env_grass_tall_04",
            PropKind::Env_Grass_06 => "env_grass_06",
            PropKind::Env_Grass_07 => "env_grass_07",
            PropKind::Wheat_Field => "wheat_field",
        }
    }

    /// Asset scene path (used by the client and by the collider bake tool).
    pub const fn scene_path(&self) -> &'static str {
        match self {
            PropKind::Rock_1 => "game_assets/environment/rocks/Rock_A.glb#Scene0",
            PropKind::Rock_2 => "game_assets/environment/rocks/Rock_B.glb#Scene0",
            PropKind::Rock_3 => "game_assets/environment/rocks/Rock_C.glb#Scene0",
            PropKind::Rock_4 => "game_assets/environment/rocks/Rock_A.glb#Scene0",
            PropKind::Rock_5 => "game_assets/environment/rocks/Rock_B.glb#Scene0",
            PropKind::Tree_01 => "game_assets/environment/trees/Tree01_Graft.glb#Scene0",
            PropKind::Tree_02 => "game_assets/environment/trees/Oak_A.glb#Scene0",
            PropKind::Tree_08 => "game_assets/environment/trees/Broadleaf_Big_A.glb#Scene0",
            PropKind::Tree_09 => "game_assets/environment/trees/Tree09_Graft.glb#Scene0",
            PropKind::Tree_10 => "game_assets/environment/trees/Birch_A.glb#Scene0",
            PropKind::Tree_18 => "game_assets/environment/trees/Chestnut_A.glb#Scene0",
            PropKind::Tree_29 => "game_assets/environment/trees/Tree29_Graft.glb#Scene0",
            PropKind::Dead_tree_1 => "game_assets/environment/trees_dead/Dead_A.glb#Scene0",
            PropKind::Dead_tree_2 => "game_assets/environment/trees_dead/Dead_B.glb#Scene0",
            PropKind::Dead_tree_3 => "game_assets/environment/trees_dead/Dead_C.glb#Scene0",
            PropKind::Pine_Tree_1 => "game_assets/environment/trees_pine/Pine_A.glb#Scene0",
            PropKind::Pine_Tree_2 => "game_assets/environment/trees_pine/Pine_B.glb#Scene0",
            PropKind::Pine_Tree_3 => "game_assets/environment/trees_pine/Pine_Tall_A.glb#Scene0",
            PropKind::Pine_Tree_4 => "game_assets/environment/trees_pine/Pine_Small_A.glb#Scene0",
            PropKind::Bush_01 => "game_assets/environment/bushes/Bush_A.glb#Scene0",
            PropKind::Bush_02 => "game_assets/environment/bushes/Bush_B.glb#Scene0",
            PropKind::Bush_03 => "game_assets/environment/bushes/Bush_C.glb#Scene0",
            PropKind::Bush_04 => "game_assets/environment/bushes/Bush_A.glb#Scene0",
            PropKind::Flower_01 => "game_assets/environment/flowers/Flower_A.glb#Scene0",
            PropKind::Flower_02 => "game_assets/environment/flowers/Flower_B.glb#Scene0",
            PropKind::Flower_03 => "game_assets/environment/flowers/Flower_B.glb#Scene0",
            PropKind::Flower_04 => "game_assets/environment/flowers/Flower_C.glb#Scene0",
            PropKind::Flower_05 => "game_assets/environment/flowers/Flower_D.glb#Scene0",
            PropKind::Spring_Flower_06 => {
                "game_assets/environment/flowers/Flower_C.glb#Scene0"
            }
            PropKind::Spring_Flower_07 => {
                "game_assets/environment/flowers/Flower_A.glb#Scene0"
            }
            PropKind::Spring_Flower_08 => {
                "game_assets/environment/flowers/Flower_D.glb#Scene0"
            }
            PropKind::Spring_Flower_09 => {
                "game_assets/environment/flowers/Flower_B.glb#Scene0"
            }
            PropKind::GrassBlade_9v => "game_assets/environment/grass/Grass_Patch_A.glb#Scene0",
            PropKind::Env_Grass_Tall_04 => {
                "game_assets/environment/grass/Grass_Tall_A.glb#Scene0"
            }
            PropKind::Env_Grass_06 => "game_assets/environment/grass/Grass_Patch_A.glb#Scene0",
            PropKind::Env_Grass_07 => "game_assets/environment/grass/Grass_Patch_A.glb#Scene0",
            PropKind::Wheat_Field => "game_assets/environment/crops/WheatField.glb#Scene0",
        }
    }
}

/// All prop kinds currently used in the world (client loads these at startup).
pub const ALL_PROP_KINDS: &[PropKind] = &[
    PropKind::Rock_1,
    PropKind::Rock_2,
    PropKind::Rock_3,
    PropKind::Rock_4,
    PropKind::Rock_5,
    PropKind::Tree_01,
    PropKind::Tree_02,
    PropKind::Tree_08,
    PropKind::Tree_09,
    PropKind::Tree_10,
    PropKind::Tree_18,
    PropKind::Tree_29,
    PropKind::Dead_tree_1,
    PropKind::Dead_tree_2,
    PropKind::Dead_tree_3,
    PropKind::Pine_Tree_1,
    PropKind::Pine_Tree_2,
    PropKind::Pine_Tree_3,
    PropKind::Pine_Tree_4,
    PropKind::Bush_01,
    PropKind::Bush_02,
    PropKind::Bush_03,
    PropKind::Bush_04,
    PropKind::Flower_01,
    PropKind::Flower_02,
    PropKind::Flower_03,
    PropKind::Flower_04,
    PropKind::Flower_05,
    PropKind::Spring_Flower_06,
    PropKind::Spring_Flower_07,
    PropKind::Spring_Flower_08,
    PropKind::Spring_Flower_09,
    PropKind::GrassBlade_9v,
    PropKind::Env_Grass_Tall_04,
    PropKind::Env_Grass_06,
    PropKind::Env_Grass_07,
    PropKind::Wheat_Field,
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_id_matches_all_known_kinds() {
        for kind in ALL_PROP_KINDS {
            assert_eq!(PropKind::from_id(kind.id()), Some(*kind));
        }
        assert_eq!(PropKind::from_id("unknown_kind"), None);
    }
}
