use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use crate::city::{MapPlot, MapRoad};
use crate::terrain::{
    build_terrain_weightmap_weights, terrain_paint_op_chunk_coords, ChunkCoord, TerrainDeltaData,
    TerrainGenerator, TerrainPaintOp, CHUNK_RESOLUTION, TERRAIN_WEIGHTMAP_RESOLUTION,
};

pub const MAP_EDITS_VERSION: u32 = 3;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapEditsDefinition {
    #[serde(default = "default_edits_version")]
    pub version: u32,
    #[serde(default)]
    pub terrain_deltas: Vec<MapTerrainDeltaChunk>,
    /// Legacy stroke history. Baked into `terrain_weightmaps` by the editor
    /// on load; kept so pre-bake map files still paint correctly.
    #[serde(default)]
    pub terrain_paint_ops: Vec<TerrainPaintOp>,
    /// Authored surface paint, baked per chunk (RLE, final — legacy ops are
    /// NOT applied on top of a chunk that has a baked weightmap).
    #[serde(default)]
    pub terrain_weightmaps: Vec<MapTerrainWeightmapChunk>,
    #[serde(default)]
    pub spawn_markers: Vec<MapSpawnMarker>,
    #[serde(default)]
    pub roads: Vec<MapRoad>,
    #[serde(default)]
    pub plots: Vec<MapPlot>,
}

impl Default for MapEditsDefinition {
    fn default() -> Self {
        Self {
            version: MAP_EDITS_VERSION,
            terrain_deltas: Vec::new(),
            terrain_paint_ops: Vec::new(),
            terrain_weightmaps: Vec::new(),
            spawn_markers: Vec::new(),
            roads: Vec::new(),
            plots: Vec::new(),
        }
    }
}

impl MapEditsDefinition {
    pub fn validate(&self) -> Result<(), String> {
        if self.version == 0 {
            return Err("version must be >= 1".to_string());
        }

        for (index, chunk) in self.terrain_deltas.iter().enumerate() {
            if chunk.deltas_cm.len() != CHUNK_RESOLUTION * CHUNK_RESOLUTION {
                return Err(format!(
                    "terrain_deltas[{index}] has invalid deltas length {} (expected {})",
                    chunk.deltas_cm.len(),
                    CHUNK_RESOLUTION * CHUNK_RESOLUTION
                ));
            }
        }

        let mut paint_ids = HashSet::with_capacity(self.terrain_paint_ops.len());
        for (index, op) in self.terrain_paint_ops.iter().enumerate() {
            op.validate()
                .map_err(|err| format!("terrain_paint_ops[{index}] {err}"))?;
            if !paint_ids.insert(op.id) {
                return Err(format!(
                    "terrain_paint_ops[{index}] duplicates id {}",
                    op.id
                ));
            }
        }

        let expected_texels = (TERRAIN_WEIGHTMAP_RESOLUTION * TERRAIN_WEIGHTMAP_RESOLUTION) as u64;
        let mut weightmap_coords = HashSet::with_capacity(self.terrain_weightmaps.len());
        for (index, chunk) in self.terrain_weightmaps.iter().enumerate() {
            let texels: u64 = chunk.runs.iter().map(|(count, _)| *count as u64).sum();
            if texels != expected_texels {
                return Err(format!(
                    "terrain_weightmaps[{index}] covers {texels} texels (expected {expected_texels})"
                ));
            }
            if !weightmap_coords.insert(chunk.coord) {
                return Err(format!(
                    "terrain_weightmaps[{index}] duplicates chunk ({}, {})",
                    chunk.coord.x, chunk.coord.z
                ));
            }
        }

        for (index, road) in self.roads.iter().enumerate() {
            road.validate()
                .map_err(|err| format!("roads[{index}] {err}"))?;
        }

        for (index, plot) in self.plots.iter().enumerate() {
            plot.validate()
                .map_err(|err| format!("plots[{index}] {err}"))?;
        }

        Ok(())
    }

    pub fn terrain_deltas_by_chunk(&self) -> Result<HashMap<ChunkCoord, TerrainDeltaData>, String> {
        let mut out = HashMap::with_capacity(self.terrain_deltas.len());
        for chunk in &self.terrain_deltas {
            if chunk.deltas_cm.len() != CHUNK_RESOLUTION * CHUNK_RESOLUTION {
                return Err(format!(
                    "invalid deltas length for chunk ({}, {}): {}",
                    chunk.coord.x,
                    chunk.coord.z,
                    chunk.deltas_cm.len()
                ));
            }

            let mut data = TerrainDeltaData::from_quantized(&chunk.deltas_cm);
            data.version = chunk.version;
            out.insert(chunk.coord, data);
        }
        Ok(out)
    }

    pub fn set_terrain_deltas_from_world(
        &mut self,
        chunks: &HashMap<ChunkCoord, TerrainDeltaData>,
    ) {
        let mut out = Vec::with_capacity(chunks.len());
        for (coord, data) in chunks {
            out.push(MapTerrainDeltaChunk {
                coord: *coord,
                deltas_cm: data.to_quantized(),
                version: data.version,
            });
        }
        out.sort_by_key(|chunk| (chunk.coord.x, chunk.coord.z));
        self.terrain_deltas = out;
    }

    /// Decoded baked weightmap for a chunk, if one is stored.
    pub fn weightmap_for_chunk(&self, coord: ChunkCoord) -> Option<Vec<[u8; 4]>> {
        self.terrain_weightmaps
            .iter()
            .find(|chunk| chunk.coord == coord)
            .map(|chunk| decode_weightmap_rle(&chunk.runs))
    }

    /// Store (or replace) a chunk's baked weightmap.
    pub fn set_weightmap_for_chunk(&mut self, coord: ChunkCoord, weights: &[[u8; 4]]) {
        let runs = encode_weightmap_rle(weights);
        if let Some(chunk) = self
            .terrain_weightmaps
            .iter_mut()
            .find(|chunk| chunk.coord == coord)
        {
            chunk.runs = runs;
        } else {
            self.terrain_weightmaps
                .push(MapTerrainWeightmapChunk { coord, runs });
            self.terrain_weightmaps
                .sort_by_key(|chunk| (chunk.coord.x, chunk.coord.z));
        }
    }

    /// Final surface weights for a chunk: the baked weightmap when stored,
    /// otherwise the procedural base with any legacy paint ops replayed.
    pub fn resolve_chunk_weights(
        &self,
        generator: &TerrainGenerator,
        coord: ChunkCoord,
        resolution: u32,
    ) -> Vec<[u8; 4]> {
        if resolution == TERRAIN_WEIGHTMAP_RESOLUTION {
            if let Some(weights) = self.weightmap_for_chunk(coord) {
                return weights;
            }
        }
        build_terrain_weightmap_weights(generator, coord, &self.terrain_paint_ops, resolution)
    }

    /// Migrate legacy stroke history into baked weightmaps: every chunk a
    /// legacy op touches gets its final weights stored, then the op list is
    /// cleared. Returns true if anything changed.
    pub fn bake_legacy_paint_ops(&mut self, generator: &TerrainGenerator) -> bool {
        if self.terrain_paint_ops.is_empty() {
            return false;
        }

        let mut coords = HashSet::new();
        for op in &self.terrain_paint_ops {
            for coord in terrain_paint_op_chunk_coords(op) {
                if coord.in_world_bounds() {
                    coords.insert(coord);
                }
            }
        }
        for coord in coords {
            let weights =
                self.resolve_chunk_weights(generator, coord, TERRAIN_WEIGHTMAP_RESOLUTION);
            self.set_weightmap_for_chunk(coord, &weights);
        }
        self.terrain_paint_ops.clear();
        true
    }
}

pub fn encode_weightmap_rle(weights: &[[u8; 4]]) -> Vec<(u16, [u8; 4])> {
    let mut runs: Vec<(u16, [u8; 4])> = Vec::new();
    for weight in weights {
        match runs.last_mut() {
            Some((count, value)) if value == weight && *count < u16::MAX => *count += 1,
            _ => runs.push((1, *weight)),
        }
    }
    runs
}

pub fn decode_weightmap_rle(runs: &[(u16, [u8; 4])]) -> Vec<[u8; 4]> {
    let total: usize = runs.iter().map(|(count, _)| *count as usize).sum();
    let mut weights = Vec::with_capacity(total);
    for (count, value) in runs {
        weights.extend(std::iter::repeat(*value).take(*count as usize));
    }
    weights
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapTerrainDeltaChunk {
    pub coord: ChunkCoord,
    pub deltas_cm: Vec<i16>,
    #[serde(default)]
    pub version: u32,
}

/// A chunk's baked surface weightmap, RLE-encoded as (run length, rgba
/// layer weights) covering TERRAIN_WEIGHTMAP_RESOLUTION² texels row-major.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapTerrainWeightmapChunk {
    pub coord: ChunkCoord,
    pub runs: Vec<(u16, [u8; 4])>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum SpawnMarkerKind {
    Player,
    NpcGroup,
    Poi,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapSpawnMarker {
    pub id: u64,
    pub kind: SpawnMarkerKind,
    pub position: [f32; 3],
    #[serde(default)]
    pub rotation_degrees: f32,
    #[serde(default = "default_spawn_radius")]
    pub radius: f32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::{TerrainLayer, TerrainPaintShape};
    use bevy::prelude::Vec2;

    #[test]
    fn legacy_edits_default_to_no_surface_paint() {
        let edits: MapEditsDefinition = ron::from_str("(version: 1)").unwrap();

        assert!(edits.terrain_paint_ops.is_empty());
        assert!(edits.validate().is_ok());
    }

    #[test]
    fn weightmap_rle_roundtrips() {
        let mut weights = vec![[255, 0, 0, 0]; 4096];
        for w in weights.iter_mut().skip(1000).take(500) {
            *w = [0, 0, 0, 255];
        }
        let runs = encode_weightmap_rle(&weights);
        assert!(runs.len() < 10);
        assert_eq!(decode_weightmap_rle(&runs), weights);
    }

    #[test]
    fn stored_weightmaps_validate_texel_coverage() {
        let edits = MapEditsDefinition {
            terrain_weightmaps: vec![MapTerrainWeightmapChunk {
                coord: ChunkCoord::new(0, 0),
                runs: vec![(100, [255, 0, 0, 0])],
            }],
            ..Default::default()
        };
        assert!(edits.validate().is_err());

        let edits = MapEditsDefinition {
            terrain_weightmaps: vec![MapTerrainWeightmapChunk {
                coord: ChunkCoord::new(0, 0),
                runs: vec![(4096, [255, 0, 0, 0])],
            }],
            ..Default::default()
        };
        assert!(edits.validate().is_ok());
    }

    #[test]
    fn duplicate_surface_paint_ids_are_rejected() {
        let operation = TerrainPaintOp {
            id: 7,
            layer: TerrainLayer::Grass,
            strength: 0.5,
            falloff: 1.0,
            shape: TerrainPaintShape::Circle {
                center: Vec2::ZERO,
                radius: 4.0,
            },
        };
        let edits = MapEditsDefinition {
            terrain_paint_ops: vec![operation.clone(), operation],
            ..Default::default()
        };

        assert!(edits.validate().is_err());
    }
}

#[inline]
fn default_edits_version() -> u32 {
    MAP_EDITS_VERSION
}

#[inline]
fn default_spawn_radius() -> f32 {
    2.0
}
