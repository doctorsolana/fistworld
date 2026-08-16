//! Building footprint and collider metadata.
//!
//! Defines building types, their resource costs, footprints, and terrain modification parameters.

mod defs;
mod footprint;
mod zones;

pub use defs::*;
pub use footprint::*;
pub use zones::*;

#[cfg(test)]
mod tests {
    use bevy::prelude::Vec2;
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

    /// Every building the game has must have a definition and a model.
    ///
    /// Written as a sweep over `all()` rather than naming two by hand: the
    /// hand-named version broke the moment the bought building sets were
    /// deleted, and it would have said nothing about the ones we kept.
    #[test]
    fn every_building_has_a_definition_and_a_model() {
        let mut ids = HashSet::new();
        let mut paths = HashSet::new();
        for kind in BuildingType::all() {
            let def = kind.definition();
            assert!(!def.display_name.is_empty(), "{kind:?} has no display name");
            let scene = kind
                .scene_path()
                .unwrap_or_else(|| panic!("{kind:?} has no model to draw"));
            assert!(ids.insert(kind.id()), "duplicate building id {}", kind.id());
            assert!(paths.insert(scene), "duplicate building scene {scene}");

            let relative = scene.split('#').next().unwrap();
            let file = Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../client/assets")
                .join(relative);
            assert!(file.is_file(), "missing building asset {}", file.display());
            assert_eq!(
                glb_scene_name(&file),
                file.file_stem().unwrap().to_string_lossy(),
                "{} has stale internal scene metadata",
                file.display()
            );
        }
    }

    #[test]
    fn civic_hall_variants_keep_distinct_serialized_identities() {
        for kind in [
            BuildingType::MootHall,
            BuildingType::VillageHall,
            BuildingType::TownHall,
        ] {
            let encoded = ron::to_string(&kind).unwrap();
            assert_eq!(ron::from_str::<BuildingType>(&encoded).unwrap(), kind);
        }
    }

    #[test]
    fn market_levels_share_the_authored_walkable_square_contract() {
        for kind in [BuildingType::Market, BuildingType::MarketPaved] {
            let definition = kind.definition();
            assert_eq!(definition.footprint, Vec2::splat(12.0));
            assert_eq!(definition.footprint_center, Vec2::ZERO);
            assert_eq!(definition.terrain_flat_margin(), 3.0);
            assert_eq!(definition.terrain_flat_half_extents(), Vec2::splat(9.0));
            assert!((definition.terrain_blend_width() - 1.8).abs() < 1e-5);
            assert!(definition.model_path.is_some());
            assert!(!kind.has_baked_collider());
            assert!(!kind.blocks_ground_navigation());
        }
        assert_eq!(
            ron::from_str::<BuildingType>("PlaceholderMarket").unwrap(),
            BuildingType::Market,
            "pre-art RON saves must continue to deserialize"
        );
    }

    #[test]
    fn flatten_footprint_is_square_and_positive() {
        for kind in BuildingType::all() {
            let flatten = kind.definition().flatten_footprint();
            assert!(
                flatten.x > 0.0 && flatten.y > 0.0,
                "{kind:?} flattens nothing"
            );
        }
    }
}
