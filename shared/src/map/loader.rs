use bevy::prelude::Quat;
use image::ImageReader;
use std::collections::{hash_map::DefaultHasher, HashMap};
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use crate::props::PropKind;
use crate::terrain::CHUNK_SIZE;

use super::{HeightmapData, MapBounds, MapDefinition, MapObjectSpawn, DEFAULT_MAP_ID};

const ASSET_ROOT_CANDIDATES: [&str; 2] = ["assets", "client/assets"];

#[derive(Debug, Clone)]
pub struct LoadedMap {
    pub definition: MapDefinition,
    pub heightmap: HeightmapData,
    pub objects_by_chunk: HashMap<(i32, i32), Vec<ResolvedMapObject>>,
    pub content_hash: u64,
    pub map_dir: PathBuf,
}

#[derive(Debug, Clone, Copy)]
pub struct ResolvedMapObject {
    pub kind: PropKind,
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

    definition.validate()?;

    let map_dir = map_path
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| format!("Map path has no parent: {}", map_path.display()))?;

    let heightmap_path = resolve_map_relative_file(&map_dir, map_id, &definition.terrain.heightmap)
        .ok_or_else(|| {
            format!(
                "Could not locate heightmap '{}' for map '{}'",
                definition.terrain.heightmap, map_id
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

    // Optional minimap is resolved early so missing files fail fast with a clear error.
    if let Some(minimap_rel) = definition.terrain.minimap.as_deref() {
        let _ = resolve_map_relative_file(&map_dir, map_id, minimap_rel).ok_or_else(|| {
            format!(
                "Could not locate minimap '{}' for map '{}'",
                minimap_rel, map_id
            )
        })?;
    }

    let objects_by_chunk = build_objects_by_chunk(&definition.objects);

    let mut hasher = DefaultHasher::new();
    definition.map_id.hash(&mut hasher);
    map_bytes.hash(&mut hasher);
    heightmap_bytes.hash(&mut hasher);
    let content_hash = hasher.finish();

    Ok(LoadedMap {
        definition,
        heightmap,
        objects_by_chunk,
        content_hash,
        map_dir,
    })
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

fn asset_roots() -> Vec<PathBuf> {
    let mut out = Vec::with_capacity(ASSET_ROOT_CANDIDATES.len() + 1);

    if let Ok(exe_path) = std::env::current_exe() {
        if let Some(exe_dir) = exe_path.parent() {
            out.push(exe_dir.join("assets"));
        }
    }

    for root in ASSET_ROOT_CANDIDATES {
        out.push(PathBuf::from(root));
    }

    #[cfg(test)]
    {
        // During `cargo test -p shared`, cwd is typically `shared/`.
        let shared_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        if let Some(workspace_dir) = shared_dir.parent() {
            out.push(workspace_dir.join("assets"));
            out.push(workspace_dir.join("client/assets"));
        }
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
        let Some(kind) = object.prop_kind() else {
            continue;
        };

        by_chunk
            .entry(object_chunk_key(object.position))
            .or_default()
            .push(ResolvedMapObject {
                kind,
                position: object.position,
                rotation: Quat::from_rotation_y(object.rotation_degrees.to_radians()),
                scale: object.scale,
            });
    }

    by_chunk
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
            .copied()
            .unwrap();
        assert_eq!(item.kind.id(), "rock_1");
        let rotated_forward = item.rotation * bevy::prelude::Vec3::Z;
        assert!((rotated_forward.x - 1.0).abs() < 1e-6);
        assert!(rotated_forward.z.abs() < 1e-6);
        assert!((item.scale - 1.2).abs() < 1e-6);
    }
}
