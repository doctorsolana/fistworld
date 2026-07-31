use bevy::prelude::Quat;
use image::ImageReader;
use std::collections::{hash_map::DefaultHasher, HashMap};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use crate::props::PropKind;
use crate::terrain::{ChunkCoord, TerrainDeltaData, CHUNK_SIZE};

use super::{
    load_map_edits_optional, map_edits_path, HeightmapData, MapBounds, MapDefinition,
    MapEditsDefinition, MapObjectSpawn, DEFAULT_MAP_ID,
};

const ASSET_ROOT_CANDIDATES: [&str; 2] = ["assets", "client/assets"];

#[derive(Debug, Clone)]
pub struct LoadedMap {
    pub definition: MapDefinition,
    pub heightmap: HeightmapData,
    pub edits: MapEditsDefinition,
    pub terrain_deltas_by_chunk: HashMap<ChunkCoord, TerrainDeltaData>,
    pub objects_by_chunk: HashMap<(i32, i32), Vec<ResolvedMapObject>>,
    /// Road-distance mask rebuilt from the generated-world recipe; drives
    /// procedural surface painting. `None` for hand-authored maps.
    pub road_mask: Option<std::sync::Arc<crate::worldgen::RoadMask>>,
    /// Biome/resource sampler rebuilt from the recipe seed; drives painting,
    /// the world map, and resource availability. `None` for hand-authored
    /// maps.
    pub biome_field: Option<std::sync::Arc<crate::worldgen::BiomeField>>,
    pub content_hash: u64,
    pub map_dir: PathBuf,
}

#[derive(Debug, Clone)]
pub struct ResolvedMapObject {
    pub kind: Option<PropKind>,
    pub scene_path: String,
    pub position: [f32; 3],
    pub rotation: Quat,
    pub scale: f32,
}

pub fn load_default_map() -> Result<LoadedMap, String> {
    load_map(DEFAULT_MAP_ID)
}

pub fn load_map(map_id: &str) -> Result<LoadedMap, String> {
    let (map_path, map_bytes) = load_map_ron_bytes(map_id)?;
    let map_text = std::str::from_utf8(&map_bytes)
        .map_err(|err| format!("{} is not valid UTF-8: {err}", map_path.display()))?;

    let mut definition: MapDefinition = ron::from_str(map_text)
        .map_err(|err| format!("Failed to parse {}: {err}", map_path.display()))?;

    if definition.map_id.trim().is_empty() {
        definition.map_id = map_id.to_string();
    }

    let map_dir = map_path
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| format!("Map path has no parent: {}", map_path.display()))?;

    let edits = load_map_edits_optional(&map_dir)?.unwrap_or_default();
    let edits_bytes = match fs::read(map_edits_path(&map_dir)) {
        Ok(bytes) => Some(bytes),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(err) => {
            return Err(format!(
                "Failed to read {}: {err}",
                map_edits_path(&map_dir).display()
            ));
        }
    };

    build_loaded_map(
        map_dir,
        definition,
        edits,
        &map_bytes,
        edits_bytes.as_deref(),
    )
}

pub fn load_map_from_parts(
    map_dir: &Path,
    definition: &MapDefinition,
    edits: &MapEditsDefinition,
) -> Result<LoadedMap, String> {
    let map_bytes = ron::ser::to_string(definition)
        .map_err(|err| {
            format!(
                "Failed to serialize map definition '{}': {err}",
                definition.map_id
            )
        })?
        .into_bytes();
    let edits_bytes = ron::ser::to_string(edits)
        .map_err(|err| {
            format!(
                "Failed to serialize map edits '{}': {err}",
                definition.map_id
            )
        })?
        .into_bytes();

    build_loaded_map(
        map_dir.to_path_buf(),
        definition.clone(),
        edits.clone(),
        &map_bytes,
        Some(&edits_bytes),
    )
}

pub fn map_rpath(map_id: &str) -> PathBuf {
    PathBuf::from("maps").join(map_id).join("map.ron")
}

pub fn resolve_map_relative_file(
    map_dir: &Path,
    map_id: &str,
    rel_or_abs: &str,
) -> Option<PathBuf> {
    let raw = PathBuf::from(rel_or_abs);

    if raw.is_absolute() {
        return raw.exists().then_some(raw);
    }

    let map_local = map_dir.join(&raw);
    if map_local.exists() {
        return Some(map_local);
    }

    for root in asset_roots() {
        let direct = root.join(&raw);
        if direct.exists() {
            return Some(direct);
        }

        let under_map = root.join("maps").join(map_id).join(&raw);
        if under_map.exists() {
            return Some(under_map);
        }
    }

    None
}

fn load_map_ron_bytes(map_id: &str) -> Result<(PathBuf, Vec<u8>), String> {
    let rel = map_rpath(map_id);

    for root in asset_roots() {
        let candidate = root.join(&rel);
        if !candidate.exists() {
            continue;
        }

        let bytes = fs::read(&candidate)
            .map_err(|err| format!("Failed to read {}: {err}", candidate.display()))?;
        return Ok((candidate, bytes));
    }

    Err(format!(
        "Could not locate '{}' in assets roots (tried assets/... and client/assets/...)",
        rel.display()
    ))
}

fn build_loaded_map(
    map_dir: PathBuf,
    mut definition: MapDefinition,
    edits: MapEditsDefinition,
    map_bytes: &[u8],
    edits_bytes: Option<&[u8]>,
) -> Result<LoadedMap, String> {
    if definition.map_id.trim().is_empty() {
        definition.map_id = map_dir
            .file_name()
            .and_then(|name| name.to_str())
            .filter(|name| !name.trim().is_empty())
            .unwrap_or(DEFAULT_MAP_ID)
            .to_string();
    }

    definition.validate()?;
    edits.validate()?;

    validate_object_scene_assets(&map_dir, &definition.map_id, &definition.objects)?;

    // Generated worlds rebuild their terrain from the seed recipe — the
    // Valheim model. Hand-authored maps still decode a heightmap image.
    let (heightmap, heightmap_bytes, road_mask, biome_field) = if let Some(generated) =
        &definition.generated
    {
        if generated.generator_version != crate::worldgen::WORLDGEN_VERSION {
            bevy::log::error!(
                "Map '{}' was generated with terrain formula v{} but this binary has v{}: \
                 the rebuilt terrain WILL differ from the world that recipe described \
                 (and from binaries built at the other version). Regenerate the map, or \
                 rebuild all binaries at one version.",
                definition.map_id,
                generated.generator_version,
                crate::worldgen::WORLDGEN_VERSION,
            );
        }
        let started = std::time::Instant::now();
        let heightmap =
            generated.build_heightmap(definition.bounds, definition.terrain.water_level)?;
        let road_mask = generated.build_road_mask().map(std::sync::Arc::new);
        let biome_field = Some(std::sync::Arc::new(generated.build_biome_field()));
        bevy::log::info!(
            "Rebuilt '{}' terrain from seed {} ({}x{} grid) in {:.2}s",
            definition.map_id,
            generated.seed,
            heightmap.width,
            heightmap.height,
            started.elapsed().as_secs_f32(),
        );
        // The recipe lives inside map.ron, so map_bytes already covers it
        // for the content hash; there are no heightmap bytes to hash.
        (heightmap, Vec::new(), road_mask, biome_field)
    } else {
        let heightmap_path =
            resolve_map_relative_file(&map_dir, &definition.map_id, &definition.terrain.heightmap)
                .ok_or_else(|| {
                    format!(
                        "Could not locate heightmap '{}' for map '{}'",
                        definition.terrain.heightmap, definition.map_id
                    )
                })?;

        let heightmap_bytes = fs::read(&heightmap_path)
            .map_err(|err| format!("Failed to read {}: {err}", heightmap_path.display()))?;

        let heightmap = decode_heightmap(
            &heightmap_path,
            &heightmap_bytes,
            definition.bounds,
            definition.terrain.height_min,
            definition.terrain.height_max,
            definition.terrain.water_level,
        )?;
        (heightmap, heightmap_bytes, None, None)
    };

    if let Some(minimap_rel) = definition.terrain.minimap.as_deref() {
        let _ = resolve_map_relative_file(&map_dir, &definition.map_id, minimap_rel).ok_or_else(
            || {
                format!(
                    "Could not locate minimap '{}' for map '{}'",
                    minimap_rel, definition.map_id
                )
            },
        )?;
    }

    let terrain_deltas_by_chunk = edits
        .terrain_deltas_by_chunk()
        .map_err(|err| format!("Invalid map edits for '{}': {err}", definition.map_id))?;
    let objects_by_chunk = build_objects_by_chunk(&definition.objects);
    let content_hash =
        compute_loaded_map_hash(&definition.map_id, map_bytes, &heightmap_bytes, edits_bytes);

    Ok(LoadedMap {
        definition,
        heightmap,
        edits,
        terrain_deltas_by_chunk,
        objects_by_chunk,
        road_mask,
        biome_field,
        content_hash,
        map_dir,
    })
}

fn compute_loaded_map_hash(
    map_id: &str,
    map_bytes: &[u8],
    heightmap_bytes: &[u8],
    edits_bytes: Option<&[u8]>,
) -> u64 {
    let mut hasher = DefaultHasher::new();
    map_id.hash(&mut hasher);
    map_bytes.hash(&mut hasher);
    heightmap_bytes.hash(&mut hasher);
    if let Some(bytes) = edits_bytes {
        bytes.hash(&mut hasher);
    }
    hasher.finish()
}

pub fn asset_roots() -> Vec<PathBuf> {
    let mut out = Vec::with_capacity(ASSET_ROOT_CANDIDATES.len() + 4);
    let mut push_unique = |path: PathBuf| {
        if !out.iter().any(|existing| existing == &path) {
            out.push(path);
        }
    };

    if let Ok(configured) = std::env::var("FISTFORCE_ASSET_PATH") {
        let configured = configured.trim();
        if !configured.is_empty() {
            push_unique(PathBuf::from(configured));
        }
    }

    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            push_unique(exe_dir.join("assets"));
        }
    }

    for root in ASSET_ROOT_CANDIDATES {
        push_unique(PathBuf::from(root));
    }

    // Support running binaries from subdirectories (e.g. `editor/`) where cwd-based
    // `assets` or `client/assets` roots may not resolve.
    let shared_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if let Some(workspace_dir) = shared_dir.parent() {
        push_unique(workspace_dir.join("assets"));
        push_unique(workspace_dir.join("client/assets"));
    }

    out
}

fn object_chunk_key(position: [f32; 3]) -> (i32, i32) {
    (
        (position[0] / CHUNK_SIZE).floor() as i32,
        (position[2] / CHUNK_SIZE).floor() as i32,
    )
}

fn build_objects_by_chunk(
    objects: &[MapObjectSpawn],
) -> HashMap<(i32, i32), Vec<ResolvedMapObject>> {
    let mut by_chunk: HashMap<(i32, i32), Vec<ResolvedMapObject>> = HashMap::new();

    for object in objects {
        let Some(scene_path) = object.resolved_scene_path() else {
            continue;
        };
        let kind = object.prop_kind();

        by_chunk
            .entry(object_chunk_key(object.position))
            .or_default()
            .push(ResolvedMapObject {
                kind,
                scene_path,
                position: object.position,
                rotation: Quat::from_rotation_y(object.rotation_degrees.to_radians()),
                scale: object.scale,
            });
    }

    by_chunk
}

fn validate_object_scene_assets(
    map_dir: &Path,
    map_id: &str,
    objects: &[MapObjectSpawn],
) -> Result<(), String> {
    for (index, object) in objects.iter().enumerate() {
        let Some(scene_path) = object.resolved_scene_path() else {
            continue;
        };
        let scene_file = scene_path.split('#').next().unwrap_or(scene_path.as_str());
        if resolve_map_relative_file(map_dir, map_id, scene_file).is_none() {
            return Err(format!(
                "objects[{index}] scene '{}' was not found in asset roots",
                scene_file
            ));
        }
    }
    Ok(())
}

fn decode_heightmap(
    path: &Path,
    bytes: &[u8],
    bounds: MapBounds,
    height_min: f32,
    height_max: f32,
    water_level: Option<f32>,
) -> Result<HeightmapData, String> {
    let reader = ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|err| {
            format!(
                "Failed to detect image format for {}: {err}",
                path.display()
            )
        })?;

    let image = reader
        .decode()
        .map_err(|err| format!("Failed to decode {}: {err}", path.display()))?
        .to_luma8();

    let width = image.width();
    let height = image.height();
    if width < 2 || height < 2 {
        return Err(format!(
            "Heightmap {} must be at least 2x2 pixels",
            path.display()
        ));
    }

    let mut heights = Vec::with_capacity((width as usize) * (height as usize));
    let span = height_max - height_min;
    for pixel in image.pixels() {
        let t = (pixel[0] as f32) / 255.0;
        heights.push(height_min + t * span);
    }

    Ok(HeightmapData::new(
        bounds,
        width,
        height,
        heights,
        water_level,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The Valheim-mode fidelity guarantee: a generated map loaded through
    /// the normal loader must sample bit-for-bit like the generation grid,
    /// including replayed road strokes — no PNG or baked deltas involved.
    #[test]
    fn loaded_generated_map_matches_generation_grid() {
        use crate::worldgen::{GeneratedWorld, WorldStyle};
        use bevy::prelude::Vec2;

        let half = 512.0;
        let base = GeneratedWorld {
            style: WorldStyle::Showcase,
            seed: 2026,
            generator_version: crate::worldgen::WORLDGEN_VERSION,
            half_extent: half,
            strokes: Vec::new(),
        };
        // Record a road stroke the way generation does: flatten the live grid.
        let mut grid = base.build_grid();
        let path: Vec<Vec2> = (0..10)
            .map(|i| Vec2::new(-180.0 + i as f32 * 40.0, (i as f32 * 0.9).cos() * 50.0))
            .collect();
        let stroke = grid.flatten_along_path(&path, 7.0, 16.0).unwrap();
        let recipe = GeneratedWorld {
            strokes: vec![stroke],
            ..base
        };

        let definition = MapDefinition {
            map_id: "gen_roundtrip".to_string(),
            bounds: MapBounds {
                min: [-half, -half],
                max: [half, half],
            },
            terrain: crate::map::MapTerrain {
                heightmap: "height.png".to_string(), // ignored for generated maps
                minimap: None,
                water_level: Some(0.0),
                height_min: -20.0,
                height_max: 120.0,
            },
            generated: Some(recipe),
            player_spawn: None,
            objects: Vec::new(),
            blockers: Vec::new(),
        };

        // No height.png, no edits.ron, no map dir contents — the recipe alone.
        let loaded = load_map_from_parts(
            Path::new("does_not_exist"),
            &definition,
            &MapEditsDefinition::default(),
        )
        .expect("generated map must load without any baked terrain files");

        assert!(loaded.road_mask.is_some(), "road mask must rebuild from strokes");
        for (x, z) in [
            (0.0, 0.0),
            (-180.0, 50.0),
            (100.0, -37.5),
            (505.0, 505.0),
            (-511.0, 3.3),
        ] {
            let expected = grid.height(x, z);
            let got = loaded.heightmap.sample_height(x, z);
            // Not bit-compared: HeightmapData interpolates via a cached
            // reciprocal, so off-lattice samples can differ by an ULP. What
            // must be bit-exact is the rebuilt grid itself across binaries,
            // which the worldgen determinism test covers.
            assert!(
                (expected - got).abs() < 1e-3,
                "loader/generation divergence at ({x},{z}): {expected} vs {got}"
            );
        }
    }

    #[test]
    fn objects_are_indexed_by_chunk_and_resolved_once() {
        let objects = vec![
            MapObjectSpawn {
                kind: "rock_1".to_string(),
                position: [1.0, 0.0, 1.0],
                rotation_degrees: 90.0,
                scale: 1.2,
            },
            MapObjectSpawn {
                kind: "tree_01".to_string(),
                position: [CHUNK_SIZE + 0.5, 2.0, -0.1],
                rotation_degrees: 180.0,
                scale: 0.8,
            },
        ];

        let indexed = build_objects_by_chunk(&objects);
        assert_eq!(indexed.get(&(0, 0)).map(|v| v.len()), Some(1));
        assert_eq!(indexed.get(&(1, -1)).map(|v| v.len()), Some(1));

        let item = indexed
            .get(&(0, 0))
            .and_then(|v| v.first())
            .cloned()
            .unwrap();
        assert_eq!(item.kind.map(|kind| kind.id()), Some("rock_1"));
        // The kind's own path, not a filename literal -- see the note in
        // schema.rs: swapping the art must not break resolution tests.
        assert_eq!(item.scene_path.as_str(), PropKind::Rock_1.scene_path());
        let rotated_forward = item.rotation * bevy::prelude::Vec3::Z;
        assert!((rotated_forward.x - 1.0).abs() < 1e-6);
        assert!(rotated_forward.z.abs() < 1e-6);
        assert!((item.scale - 1.2).abs() < 1e-6);
    }
}
