use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::{ChunkCoord, CHUNK_RESOLUTION};

/// Per-chunk height delta storage
/// Stores height adjustments at each vertex of the chunk grid
/// Final height = procedural_height + delta
#[derive(Debug, Clone)]
pub struct TerrainDeltaData {
    /// Height deltas at each vertex (CHUNK_RESOLUTION x CHUNK_RESOLUTION grid)
    /// Stored row-major: index = zi * CHUNK_RESOLUTION + xi
    pub deltas: Vec<f32>,
    /// Version for change detection
    pub version: u32,
}

impl Default for TerrainDeltaData {
    fn default() -> Self {
        Self {
            deltas: vec![0.0; CHUNK_RESOLUTION * CHUNK_RESOLUTION],
            version: 0,
        }
    }
}

impl TerrainDeltaData {
    /// Get delta at vertex indices (with bounds check)
    #[inline]
    pub fn get_vertex(&self, xi: usize, zi: usize) -> f32 {
        if xi < CHUNK_RESOLUTION && zi < CHUNK_RESOLUTION {
            self.deltas[zi * CHUNK_RESOLUTION + xi]
        } else {
            0.0
        }
    }

    /// Add to existing delta at vertex (composable edits)
    #[inline]
    pub fn add_vertex(&mut self, xi: usize, zi: usize, additional: f32) {
        if xi < CHUNK_RESOLUTION && zi < CHUNK_RESOLUTION {
            self.deltas[zi * CHUNK_RESOLUTION + xi] += additional;
        }
    }

    /// Convert to network-friendly quantized format (cm precision)
    pub fn to_quantized(&self) -> Vec<i16> {
        self.deltas
            .iter()
            .map(|&d| (d * 100.0).round().clamp(-32768.0, 32767.0) as i16)
            .collect()
    }

    /// Create from network-friendly quantized format
    pub fn from_quantized(deltas_cm: &[i16]) -> Self {
        let deltas: Vec<f32> = deltas_cm.iter().map(|&d| d as f32 / 100.0).collect();
        Self { deltas, version: 0 }
    }
}

/// Network-replicated terrain delta chunk component
/// Quantized to centimeters for bandwidth efficiency
#[derive(Component, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TerrainDeltaChunk {
    /// Chunk coordinate this delta applies to
    pub coord: ChunkCoord,
    /// Height deltas in centimeters (i16 for bandwidth, ±327m range)
    pub deltas_cm: Vec<i16>,
    /// Version for change detection
    pub version: u32,
}

impl TerrainDeltaChunk {
    /// Create from TerrainDeltaData
    pub fn from_delta_data(coord: ChunkCoord, data: &TerrainDeltaData) -> Self {
        Self {
            coord,
            deltas_cm: data.to_quantized(),
            version: data.version,
        }
    }

    /// Convert to TerrainDeltaData
    pub fn to_delta_data(&self) -> TerrainDeltaData {
        let mut data = TerrainDeltaData::from_quantized(&self.deltas_cm);
        data.version = self.version;
        data
    }
}
