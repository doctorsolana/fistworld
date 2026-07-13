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

    // environment/leaves
    Env_Leaves_02,
    Env_Leaves_03,

    // environment/grass
    GrassBlade_9v,
    Env_Grass_Tall_04,
    Env_Grass_06,
    Env_Grass_07,

    // environment/ivy
    Env_Ivy_08,
    Env_Ivy_13,
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
            "env_leaves_02" => Some(PropKind::Env_Leaves_02),
            "env_leaves_03" => Some(PropKind::Env_Leaves_03),
            "grass_blade_9v" => Some(PropKind::GrassBlade_9v),
            "env_grass_tall_04" => Some(PropKind::Env_Grass_Tall_04),
            "env_grass_06" => Some(PropKind::Env_Grass_06),
            "env_grass_07" => Some(PropKind::Env_Grass_07),
            "env_ivy_08" => Some(PropKind::Env_Ivy_08),
            "env_ivy_13" => Some(PropKind::Env_Ivy_13),
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
            PropKind::Env_Leaves_02 => "env_leaves_02",
            PropKind::Env_Leaves_03 => "env_leaves_03",
            PropKind::GrassBlade_9v => "grass_blade_9v",
            PropKind::Env_Grass_Tall_04 => "env_grass_tall_04",
            PropKind::Env_Grass_06 => "env_grass_06",
            PropKind::Env_Grass_07 => "env_grass_07",
            PropKind::Env_Ivy_08 => "env_ivy_08",
            PropKind::Env_Ivy_13 => "env_ivy_13",
        }
    }

    /// Asset scene path (used by the client and by the collider bake tool).
    pub const fn scene_path(&self) -> &'static str {
        match self {
            PropKind::Rock_1 => "game_assets/environment/rocks/Rock_1.glb#Scene0",
            PropKind::Rock_2 => "game_assets/environment/rocks/Rock_2.glb#Scene0",
            PropKind::Rock_3 => "game_assets/environment/rocks/Rock_3.glb#Scene0",
            PropKind::Rock_4 => "game_assets/environment/rocks/Rock_4.glb#Scene0",
            PropKind::Rock_5 => "game_assets/environment/rocks/Rock_5.glb#Scene0",
            PropKind::Tree_01 => "game_assets/environment/trees/Tree_01.glb#Scene0",
            PropKind::Tree_02 => "game_assets/environment/trees/Tree_02.glb#Scene0",
            PropKind::Tree_08 => "game_assets/environment/trees/Tree_08.glb#Scene0",
            PropKind::Tree_09 => "game_assets/environment/trees/Tree_09.glb#Scene0",
            PropKind::Tree_10 => "game_assets/environment/trees/Tree_10.glb#Scene0",
            PropKind::Tree_18 => "game_assets/environment/trees/Tree_18.glb#Scene0",
            PropKind::Tree_29 => "game_assets/environment/trees/Tree_29.glb#Scene0",
            PropKind::Dead_tree_1 => "game_assets/environment/trees_dead/Dead_tree_1.glb#Scene0",
            PropKind::Dead_tree_2 => "game_assets/environment/trees_dead/Dead_tree_2.glb#Scene0",
            PropKind::Dead_tree_3 => "game_assets/environment/trees_dead/Dead_tree_3.glb#Scene0",
            PropKind::Pine_Tree_1 => "game_assets/environment/trees_pine/Pine_Tree_1.glb#Scene0",
            PropKind::Pine_Tree_2 => "game_assets/environment/trees_pine/Pine_Tree_2.glb#Scene0",
            PropKind::Pine_Tree_3 => "game_assets/environment/trees_pine/Pine_Tree_3.glb#Scene0",
            PropKind::Pine_Tree_4 => "game_assets/environment/trees_pine/Pine_Tree_4.glb#Scene0",
            PropKind::Bush_01 => "game_assets/environment/bushes/Bush_01.glb#Scene0",
            PropKind::Bush_02 => "game_assets/environment/bushes/Bush_02.glb#Scene0",
            PropKind::Bush_03 => "game_assets/environment/bushes/Bush_03.glb#Scene0",
            PropKind::Bush_04 => "game_assets/environment/bushes/Bush_04.glb#Scene0",
            PropKind::Flower_01 => "game_assets/environment/flowers/Flower_01.glb#Scene0",
            PropKind::Flower_02 => "game_assets/environment/flowers/Flower_02.glb#Scene0",
            PropKind::Flower_03 => "game_assets/environment/flowers/Flower_03.glb#Scene0",
            PropKind::Flower_04 => "game_assets/environment/flowers/Flower_04.glb#Scene0",
            PropKind::Flower_05 => "game_assets/environment/flowers/Flower_05.glb#Scene0",
            PropKind::Spring_Flower_06 => {
                "game_assets/environment/flowers/Spring_Flower_06.glb#Scene0"
            }
            PropKind::Spring_Flower_07 => {
                "game_assets/environment/flowers/Spring_Flower_07.glb#Scene0"
            }
            PropKind::Spring_Flower_08 => {
                "game_assets/environment/flowers/Spring_Flower_08.glb#Scene0"
            }
            PropKind::Spring_Flower_09 => {
                "game_assets/environment/flowers/Spring_Flower_09.glb#Scene0"
            }
            PropKind::Env_Leaves_02 => "game_assets/environment/leaves/Env_Leaves_02.glb#Scene0",
            PropKind::Env_Leaves_03 => "game_assets/environment/leaves/Env_Leaves_03.glb#Scene0",
            PropKind::GrassBlade_9v => "game_assets/environment/grass/GrassBlade_9v.glb#Scene0",
            PropKind::Env_Grass_Tall_04 => {
                "game_assets/environment/grass/Env_Grass_Tall_04.glb#Scene0"
            }
            PropKind::Env_Grass_06 => "game_assets/environment/grass/Env_Grass_06.glb#Scene0",
            PropKind::Env_Grass_07 => "game_assets/environment/grass/Env_Grass_07.glb#Scene0",
            PropKind::Env_Ivy_08 => "game_assets/environment/ivy/Env_Ivy_08.glb#Scene0",
            PropKind::Env_Ivy_13 => "game_assets/environment/ivy/Env_Ivy_13.glb#Scene0",
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
    PropKind::Env_Leaves_02,
    PropKind::Env_Leaves_03,
    PropKind::GrassBlade_9v,
    PropKind::Env_Grass_Tall_04,
    PropKind::Env_Grass_06,
    PropKind::Env_Grass_07,
    PropKind::Env_Ivy_08,
    PropKind::Env_Ivy_13,
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
