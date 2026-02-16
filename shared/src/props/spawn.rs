use bevy::prelude::*;

use crate::terrain::{ChunkCoord, TerrainGenerator};

use crate::props::{PropKind, PropRenderTuning};

use super::tuning::{default_render_tuning, default_unmapped_render_tuning};

/// A single prop spawn (deterministic from world seed + chunk coord).
#[derive(Debug, Clone)]
pub struct PropSpawn {
    pub kind: Option<PropKind>,
    pub scene_path: String,
    pub chunk: ChunkCoord,
    pub position: Vec3,
    pub rotation: Quat,
    pub scale: f32,
    pub render_tuning: PropRenderTuning,
}

/// Deterministically generate all prop spawns for a given chunk.
///
/// In fixed-map mode, this uses authored object spawns from `map.ron`.
pub fn generate_chunk_prop_spawns(terrain: &TerrainGenerator, chunk: ChunkCoord) -> Vec<PropSpawn> {
    let mut out = Vec::new();
    if !chunk.in_world_bounds() {
        return out;
    }

    let Some(indexed) = terrain
        .loaded_map()
        .objects_by_chunk
        .get(&(chunk.x, chunk.z))
    else {
        return out;
    };
    out.reserve(indexed.len());

    for object in indexed {
        let x = object.position[0];
        let z = object.position[2];
        let ground_y = terrain.get_height(x, z);
        let y = ground_y + object.position[1];
        let render_tuning = object
            .kind
            .map(default_render_tuning)
            .unwrap_or_else(default_unmapped_render_tuning);

        out.push(PropSpawn {
            kind: object.kind,
            scene_path: object.scene_path.clone(),
            chunk,
            position: Vec3::new(x, y, z),
            rotation: object.rotation,
            scale: object.scale,
            render_tuning,
        });
    }

    out
}
