//! Self-contained recipes for new multiplayer worlds. No authored edits or
//! fixed spawn coordinates from a development map belong to a fresh world.

use super::{load_map_from_parts, LoadedMap, MapBounds, MapDefinition, MapTerrain};
use crate::worldgen::{GeneratedWorld, WorldStyle, WORLDGEN_VERSION};

pub const SESSION_MAP_ID: &str = "world";
pub const SESSION_HALF_EXTENT: f32 = 4096.0;

pub fn new_world_recipe(seed: u64) -> GeneratedWorld {
    GeneratedWorld {
        style: WorldStyle::Showcase,
        seed,
        generator_version: WORLDGEN_VERSION,
        half_extent: SESSION_HALF_EXTENT,
        scatter_vegetation: true,
    }
}

/// Validate before allocating a heightfield, including recipes received from
/// a server. The wire protocol must never request an unbounded terrain grid.
pub fn load_session_map(recipe: &GeneratedWorld) -> Result<LoadedMap, String> {
    if recipe.generator_version != WORLDGEN_VERSION {
        return Err("This server uses a different world generator; update the game.".into());
    }
    if !recipe.half_extent.is_finite()
        || !(64.0..=SESSION_HALF_EXTENT).contains(&recipe.half_extent)
    {
        return Err("The server's world dimensions are unsupported.".into());
    }
    let extent = recipe.half_extent;
    let definition = MapDefinition {
        map_id: SESSION_MAP_ID.into(),
        bounds: MapBounds {
            min: [-extent; 2],
            max: [extent; 2],
        },
        terrain: MapTerrain {
            heightmap: String::new(),
            minimap: None,
            water_level: Some(crate::worldgen::SEA_LEVEL),
            height_min: -64.0,
            height_max: 192.0,
        },
        generated: Some(recipe.clone()),
        player_spawn: None,
        objects: Vec::new(),
        blockers: Vec::new(),
    };
    load_map_from_parts(std::path::Path::new(""), &definition, &Default::default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_recipe_rebuilds_identical_terrain_without_authored_state() {
        let mut recipe = new_world_recipe(147);
        recipe.half_extent = 64.0;
        let a = load_session_map(&recipe).unwrap();
        let b = load_session_map(&recipe).unwrap();
        assert_eq!(a.content_hash, b.content_hash);
        for x in [-31.0, 0.0, 37.0] {
            assert_eq!(
                a.heightmap.sample_height(x, 5.0),
                b.heightmap.sample_height(x, 5.0)
            );
        }
        assert!(a.definition.player_spawn.is_none());
        assert!(a.definition.objects.is_empty());
        assert!(a.terrain_deltas_by_chunk.is_empty());
        recipe.seed += 1;
        assert_ne!(
            a.content_hash,
            load_session_map(&recipe).unwrap().content_hash
        );
    }

    #[test]
    fn rejects_invalid_or_incompatible_recipes_before_generation() {
        let mut recipe = new_world_recipe(1);
        for extent in [f32::NAN, f32::INFINITY, -1.0, 0.0, 8192.0] {
            recipe.half_extent = extent;
            assert!(load_session_map(&recipe).is_err());
        }
        recipe.half_extent = 64.0;
        recipe.generator_version += 1;
        assert!(load_session_map(&recipe).is_err());
    }
}
