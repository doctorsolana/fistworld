/// Canonical identity for a shipped environmental prop.
///
/// Rust variants describe what the current asset depicts. Serialized ids use
/// `snake_case`, and model files use `UpperCamelCase.glb`. Old map ids are
/// accepted only by [`PropKind::from_id`] and immediately normalize to one of
/// these variants; new content never writes purchased-pack numbers such as
/// `tree_09` or `spring_flower_06`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PropKind {
    // environment/rocks
    SmallRockA,
    SmallRockB,
    SmallRockC,
    BoulderA,
    BoulderB,

    // environment/trees/broadleaf
    BroadleafNarrowA,
    OakA,
    BroadleafLargeA,
    BroadleafSpreadingA,
    BirchA,
    BirchB,
    ChestnutA,
    BroadleafHighCrownA,
    BroadleafTallA,

    // environment/trees/dead
    DeadTreeA,
    DeadTreeB,
    DeadTreeC,
    DeadGnarledA,

    // environment/trees/conifer
    PineA,
    PineB,
    PineTallA,
    PineTallB,
    PineYoungA,
    PineYoungB,

    // environment/bushes
    BushA,
    BushB,
    BushC,

    // environment/flowers
    FlowerA,
    FlowerB,
    FlowerC,
    FlowerD,

    // environment/grass
    GrassShortA,
    GrassTallA,

    // environment/crops and environment/shore. These are deliberately
    // walkable and therefore have no collider-manifest entries.
    WheatField,
    FishingPier,
}

impl PropKind {
    /// Whether this prop is a living tree that a woodcutter can harvest.
    pub const fn is_tree(self) -> bool {
        matches!(
            self,
            Self::BroadleafNarrowA
                | Self::OakA
                | Self::BroadleafLargeA
                | Self::BroadleafSpreadingA
                | Self::BirchA
                | Self::BirchB
                | Self::ChestnutA
                | Self::BroadleafHighCrownA
                | Self::BroadleafTallA
                | Self::PineA
                | Self::PineB
                | Self::PineTallA
                | Self::PineTallB
                | Self::PineYoungA
                | Self::PineYoungB
        )
    }

    pub const fn is_dead_tree(self) -> bool {
        matches!(
            self,
            Self::DeadTreeA | Self::DeadTreeB | Self::DeadTreeC | Self::DeadGnarledA
        )
    }

    /// Scenery a public road crew may remove after an embodied chopping job.
    ///
    /// Rocks remain permanent route constraints. Dead trunks are included:
    /// they obstruct a lane just like a living tree and are still timber a
    /// road crew can reasonably clear.
    pub const fn is_road_clearable(self) -> bool {
        self.is_tree() || self.is_dead_tree()
    }

    /// Large authored scenery a village path surveys around instead of
    /// deleting. Low brush, flowers and grass are wear rather than barriers.
    pub const fn blocks_village_road(self) -> bool {
        self.is_road_clearable()
            || matches!(
                self,
                Self::SmallRockA
                    | Self::SmallRockB
                    | Self::SmallRockC
                    | Self::BoulderA
                    | Self::BoulderB
            )
    }

    /// Resolve a canonical id or an id written by an older map.
    ///
    /// Aliases intentionally live at this single deserialization boundary.
    /// The rest of the game sees only canonical variants and [`Self::id`]
    /// always returns the current name.
    #[inline]
    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "small_rock_a" | "rock_1" | "rock_4" => Some(Self::SmallRockA),
            "small_rock_b" | "rock_2" | "rock_5" => Some(Self::SmallRockB),
            "small_rock_c" | "rock_3" => Some(Self::SmallRockC),
            "boulder_a" => Some(Self::BoulderA),
            "boulder_b" => Some(Self::BoulderB),

            "broadleaf_narrow_a" | "tree_01" => Some(Self::BroadleafNarrowA),
            "oak_a" | "tree_02" => Some(Self::OakA),
            "broadleaf_large_a" | "tree_08" => Some(Self::BroadleafLargeA),
            "broadleaf_spreading_a" | "tree_09" => Some(Self::BroadleafSpreadingA),
            "birch_a" | "tree_10" => Some(Self::BirchA),
            "birch_b" => Some(Self::BirchB),
            "chestnut_a" | "tree_18" => Some(Self::ChestnutA),
            "broadleaf_high_crown_a" | "tree_29" => Some(Self::BroadleafHighCrownA),
            "broadleaf_tall_a" => Some(Self::BroadleafTallA),

            "dead_tree_a" | "dead_tree_1" => Some(Self::DeadTreeA),
            "dead_tree_b" | "dead_tree_2" => Some(Self::DeadTreeB),
            "dead_tree_c" | "dead_tree_3" => Some(Self::DeadTreeC),
            "dead_gnarled_a" => Some(Self::DeadGnarledA),

            "pine_a" | "pine_tree_1" => Some(Self::PineA),
            "pine_b" | "pine_tree_2" => Some(Self::PineB),
            "pine_tall_a" | "pine_tree_3" => Some(Self::PineTallA),
            "pine_tall_b" => Some(Self::PineTallB),
            "pine_young_a" | "pine_tree_4" => Some(Self::PineYoungA),
            "pine_young_b" => Some(Self::PineYoungB),

            "bush_a" | "bush_01" | "bush_04" => Some(Self::BushA),
            "bush_b" | "bush_02" => Some(Self::BushB),
            "bush_c" | "bush_03" => Some(Self::BushC),

            "flower_a" | "flower_01" => Some(Self::FlowerA),
            "flower_b" | "flower_03" => Some(Self::FlowerB),
            "flower_c" | "spring_flower_06" => Some(Self::FlowerC),
            "flower_d" | "spring_flower_08" => Some(Self::FlowerD),

            "grass_short_a" | "grass_patch" => Some(Self::GrassShortA),
            "grass_tall_a" | "grass_tall" => Some(Self::GrassTallA),
            "wheat_field" => Some(Self::WheatField),
            "fishing_pier" => Some(Self::FishingPier),
            _ => None,
        }
    }

    /// Stable canonical string written by generated and authored maps.
    pub const fn id(self) -> &'static str {
        match self {
            Self::SmallRockA => "small_rock_a",
            Self::SmallRockB => "small_rock_b",
            Self::SmallRockC => "small_rock_c",
            Self::BoulderA => "boulder_a",
            Self::BoulderB => "boulder_b",
            Self::BroadleafNarrowA => "broadleaf_narrow_a",
            Self::OakA => "oak_a",
            Self::BroadleafLargeA => "broadleaf_large_a",
            Self::BroadleafSpreadingA => "broadleaf_spreading_a",
            Self::BirchA => "birch_a",
            Self::BirchB => "birch_b",
            Self::ChestnutA => "chestnut_a",
            Self::BroadleafHighCrownA => "broadleaf_high_crown_a",
            Self::BroadleafTallA => "broadleaf_tall_a",
            Self::DeadTreeA => "dead_tree_a",
            Self::DeadTreeB => "dead_tree_b",
            Self::DeadTreeC => "dead_tree_c",
            Self::DeadGnarledA => "dead_gnarled_a",
            Self::PineA => "pine_a",
            Self::PineB => "pine_b",
            Self::PineTallA => "pine_tall_a",
            Self::PineTallB => "pine_tall_b",
            Self::PineYoungA => "pine_young_a",
            Self::PineYoungB => "pine_young_b",
            Self::BushA => "bush_a",
            Self::BushB => "bush_b",
            Self::BushC => "bush_c",
            Self::FlowerA => "flower_a",
            Self::FlowerB => "flower_b",
            Self::FlowerC => "flower_c",
            Self::FlowerD => "flower_d",
            Self::GrassShortA => "grass_short_a",
            Self::GrassTallA => "grass_tall_a",
            Self::WheatField => "wheat_field",
            Self::FishingPier => "fishing_pier",
        }
    }

    /// Human-facing display label. This is intentionally not derived from an
    /// old serialized id or a filename.
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::SmallRockA => "Small Rock A",
            Self::SmallRockB => "Small Rock B",
            Self::SmallRockC => "Small Rock C",
            Self::BoulderA => "Boulder A",
            Self::BoulderB => "Boulder B",
            Self::BroadleafNarrowA => "Narrow Broadleaf A",
            Self::OakA => "Oak A",
            Self::BroadleafLargeA => "Large Broadleaf A",
            Self::BroadleafSpreadingA => "Spreading Broadleaf A",
            Self::BirchA => "Birch A",
            Self::BirchB => "Birch B",
            Self::ChestnutA => "Chestnut A",
            Self::BroadleafHighCrownA => "High-Crown Broadleaf A",
            Self::BroadleafTallA => "Tall Broadleaf A",
            Self::DeadTreeA => "Dead Tree A",
            Self::DeadTreeB => "Dead Tree B",
            Self::DeadTreeC => "Dead Tree C",
            Self::DeadGnarledA => "Gnarled Dead Tree A",
            Self::PineA => "Pine A",
            Self::PineB => "Pine B",
            Self::PineTallA => "Tall Pine A",
            Self::PineTallB => "Tall Pine B",
            Self::PineYoungA => "Young Pine A",
            Self::PineYoungB => "Young Pine B",
            Self::BushA => "Bush A",
            Self::BushB => "Bush B",
            Self::BushC => "Bush C",
            Self::FlowerA => "Wildflower A",
            Self::FlowerB => "Wildflower B",
            Self::FlowerC => "Wildflower C",
            Self::FlowerD => "Wildflower D",
            Self::GrassShortA => "Short Grass A",
            Self::GrassTallA => "Tall Grass A",
            Self::WheatField => "Wheat Field",
            Self::FishingPier => "Fishing Pier",
        }
    }

    /// Asset scene path used by the client and collider baker.
    pub const fn scene_path(self) -> &'static str {
        match self {
            Self::SmallRockA => "game_assets/environment/rocks/SmallRockA.glb#Scene0",
            Self::SmallRockB => "game_assets/environment/rocks/SmallRockB.glb#Scene0",
            Self::SmallRockC => "game_assets/environment/rocks/SmallRockC.glb#Scene0",
            Self::BoulderA => "game_assets/environment/rocks/BoulderA.glb#Scene0",
            Self::BoulderB => "game_assets/environment/rocks/BoulderB.glb#Scene0",
            Self::BroadleafNarrowA => {
                "game_assets/environment/trees/broadleaf/BroadleafNarrowA.glb#Scene0"
            }
            Self::OakA => "game_assets/environment/trees/broadleaf/OakA.glb#Scene0",
            Self::BroadleafLargeA => {
                "game_assets/environment/trees/broadleaf/BroadleafLargeA.glb#Scene0"
            }
            Self::BroadleafSpreadingA => {
                "game_assets/environment/trees/broadleaf/BroadleafSpreadingA.glb#Scene0"
            }
            Self::BirchA => "game_assets/environment/trees/broadleaf/BirchA.glb#Scene0",
            Self::BirchB => "game_assets/environment/trees/broadleaf/BirchB.glb#Scene0",
            Self::ChestnutA => "game_assets/environment/trees/broadleaf/ChestnutA.glb#Scene0",
            Self::BroadleafHighCrownA => {
                "game_assets/environment/trees/broadleaf/BroadleafHighCrownA.glb#Scene0"
            }
            Self::BroadleafTallA => {
                "game_assets/environment/trees/broadleaf/BroadleafTallA.glb#Scene0"
            }
            Self::DeadTreeA => "game_assets/environment/trees/dead/DeadTreeA.glb#Scene0",
            Self::DeadTreeB => "game_assets/environment/trees/dead/DeadTreeB.glb#Scene0",
            Self::DeadTreeC => "game_assets/environment/trees/dead/DeadTreeC.glb#Scene0",
            Self::DeadGnarledA => "game_assets/environment/trees/dead/DeadGnarledA.glb#Scene0",
            Self::PineA => "game_assets/environment/trees/conifer/PineA.glb#Scene0",
            Self::PineB => "game_assets/environment/trees/conifer/PineB.glb#Scene0",
            Self::PineTallA => "game_assets/environment/trees/conifer/PineTallA.glb#Scene0",
            Self::PineTallB => "game_assets/environment/trees/conifer/PineTallB.glb#Scene0",
            Self::PineYoungA => "game_assets/environment/trees/conifer/PineYoungA.glb#Scene0",
            Self::PineYoungB => "game_assets/environment/trees/conifer/PineYoungB.glb#Scene0",
            Self::BushA => "game_assets/environment/bushes/BushA.glb#Scene0",
            Self::BushB => "game_assets/environment/bushes/BushB.glb#Scene0",
            Self::BushC => "game_assets/environment/bushes/BushC.glb#Scene0",
            Self::FlowerA => "game_assets/environment/flowers/FlowerA.glb#Scene0",
            Self::FlowerB => "game_assets/environment/flowers/FlowerB.glb#Scene0",
            Self::FlowerC => "game_assets/environment/flowers/FlowerC.glb#Scene0",
            Self::FlowerD => "game_assets/environment/flowers/FlowerD.glb#Scene0",
            Self::GrassShortA => "game_assets/environment/grass/GrassShortA.glb#Scene0",
            Self::GrassTallA => "game_assets/environment/grass/GrassTallA.glb#Scene0",
            Self::WheatField => "game_assets/environment/crops/WheatField.glb#Scene0",
            Self::FishingPier => "game_assets/environment/shore/FishingPier.glb#Scene0",
        }
    }
}

/// Every canonical prop asset, exactly once. Order is the authored catalog order.
pub const ALL_PROP_KINDS: &[PropKind] = &[
    PropKind::SmallRockA,
    PropKind::SmallRockB,
    PropKind::SmallRockC,
    PropKind::BoulderA,
    PropKind::BoulderB,
    PropKind::BroadleafNarrowA,
    PropKind::OakA,
    PropKind::BroadleafLargeA,
    PropKind::BroadleafSpreadingA,
    PropKind::BirchA,
    PropKind::BirchB,
    PropKind::ChestnutA,
    PropKind::BroadleafHighCrownA,
    PropKind::BroadleafTallA,
    PropKind::DeadTreeA,
    PropKind::DeadTreeB,
    PropKind::DeadTreeC,
    PropKind::DeadGnarledA,
    PropKind::PineA,
    PropKind::PineB,
    PropKind::PineTallA,
    PropKind::PineTallB,
    PropKind::PineYoungA,
    PropKind::PineYoungB,
    PropKind::BushA,
    PropKind::BushB,
    PropKind::BushC,
    PropKind::FlowerA,
    PropKind::FlowerB,
    PropKind::FlowerC,
    PropKind::FlowerD,
    PropKind::GrassShortA,
    PropKind::GrassTallA,
    PropKind::WheatField,
    PropKind::FishingPier,
];

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::fs;
    use std::path::Path;

    use super::*;

    fn glb_scene_name(path: &Path) -> String {
        let bytes = fs::read(path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        assert_eq!(&bytes[0..4], b"glTF", "{} is not a GLB", path.display());
        let json_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
        let document: serde_json::Value = serde_json::from_slice(&bytes[20..20 + json_len])
            .unwrap_or_else(|error| panic!("{}: invalid GLB JSON: {error}", path.display()));
        document["scenes"][0]["name"]
            .as_str()
            .unwrap_or_else(|| panic!("{}: default scene has no name", path.display()))
            .to_string()
    }

    #[test]
    fn canonical_prop_registry_is_complete_unique_and_loadable() {
        let mut kinds = HashSet::new();
        let mut ids = HashSet::new();
        let mut paths = HashSet::new();
        for kind in ALL_PROP_KINDS.iter().copied() {
            assert!(kinds.insert(kind), "duplicate kind {kind:?}");
            assert!(ids.insert(kind.id()), "duplicate id {}", kind.id());
            assert!(
                paths.insert(kind.scene_path()),
                "duplicate scene path {}",
                kind.scene_path()
            );
            assert_eq!(PropKind::from_id(kind.id()), Some(kind));

            let relative = kind.scene_path().split('#').next().unwrap();
            let file = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../client/assets")
                .join(relative);
            assert!(file.is_file(), "missing asset {}", file.display());
            assert_eq!(
                glb_scene_name(&file),
                file.file_stem().unwrap().to_string_lossy(),
                "{} has stale internal scene metadata",
                file.display()
            );
        }
    }

    #[test]
    fn legacy_map_ids_resolve_to_canonical_assets() {
        let aliases = [
            ("rock_4", PropKind::SmallRockA),
            ("tree_01", PropKind::BroadleafNarrowA),
            ("tree_08", PropKind::BroadleafLargeA),
            ("tree_09", PropKind::BroadleafSpreadingA),
            ("tree_29", PropKind::BroadleafHighCrownA),
            ("pine_tree_4", PropKind::PineYoungA),
            ("bush_04", PropKind::BushA),
            ("spring_flower_06", PropKind::FlowerC),
            ("grass_patch", PropKind::GrassShortA),
        ];
        for (legacy, canonical) in aliases {
            assert_eq!(PropKind::from_id(legacy), Some(canonical));
            assert_ne!(legacy, canonical.id());
        }
        assert_eq!(PropKind::from_id("unknown_kind"), None);
    }
}
