use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::terrain::{ChunkCoord, TerrainDeltaData, CHUNK_RESOLUTION};

pub const MAP_EDITS_VERSION: u32 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapEditsDefinition {
    #[serde(default = "default_edits_version")]
    pub version: u32,
    #[serde(default)]
    pub terrain_deltas: Vec<MapTerrainDeltaChunk>,
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

        for (index, road) in self.roads.iter().enumerate() {
            if road.points.len() < 2 {
                return Err(format!("roads[{index}] requires at least 2 points"));
            }
            if road.width <= 0.0 {
                return Err(format!("roads[{index}] width must be > 0"));
            }
        }

        for (index, plot) in self.plots.iter().enumerate() {
            if plot.half_extents[0] <= 0.0 || plot.half_extents[1] <= 0.0 {
                return Err(format!("plots[{index}] half_extents must be > 0"));
            }
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapTerrainDeltaChunk {
    pub coord: ChunkCoord,
    pub deltas_cm: Vec<i16>,
    #[serde(default)]
    pub version: u32,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapRoad {
    pub id: u64,
    pub points: Vec<[f32; 2]>,
    #[serde(default = "default_road_width")]
    pub width: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MapPlot {
    pub id: u64,
    pub center: [f32; 2],
    pub half_extents: [f32; 2],
    #[serde(default)]
    pub rotation_degrees: f32,
    #[serde(default)]
    pub tags: Vec<String>,
}

#[inline]
fn default_edits_version() -> u32 {
    MAP_EDITS_VERSION
}

#[inline]
fn default_spawn_radius() -> f32 {
    2.0
}

#[inline]
fn default_road_width() -> f32 {
    4.0
}
