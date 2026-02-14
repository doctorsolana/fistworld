use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use super::{world_pos_in_bounds, CHUNK_SIZE};

/// Biome types available in the world.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Biome {
    Desert,
    Grasslands,
    Natureland,
    Mountain,
    Ocean,
}

impl Biome {
    pub fn color(&self) -> Color {
        match self {
            Biome::Desert => Color::srgb(0.85, 0.75, 0.55),
            Biome::Grasslands => Color::srgb(0.35, 0.55, 0.25),
            Biome::Natureland => Color::srgb(0.28, 0.42, 0.22),
            Biome::Mountain => Color::srgb(0.45, 0.47, 0.50),
            Biome::Ocean => Color::srgb(0.12, 0.24, 0.28),
        }
    }

    pub fn accent_color(&self) -> Color {
        match self {
            Biome::Desert => Color::srgb(0.90, 0.80, 0.60),
            Biome::Grasslands => Color::srgb(0.40, 0.60, 0.30),
            Biome::Natureland => Color::srgb(0.32, 0.48, 0.26),
            Biome::Mountain => Color::srgb(0.55, 0.57, 0.60),
            Biome::Ocean => Color::srgb(0.16, 0.30, 0.34),
        }
    }
}

/// Chunk coordinate (integer grid position).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Component, Serialize, Deserialize)]
pub struct ChunkCoord {
    pub x: i32,
    pub z: i32,
}

impl ChunkCoord {
    pub fn new(x: i32, z: i32) -> Self {
        Self { x, z }
    }

    pub fn from_world_pos(pos: Vec3) -> Self {
        Self {
            x: (pos.x / CHUNK_SIZE).floor() as i32,
            z: (pos.z / CHUNK_SIZE).floor() as i32,
        }
    }

    pub fn world_pos(&self) -> Vec3 {
        Vec3::new(self.x as f32 * CHUNK_SIZE, 0.0, self.z as f32 * CHUNK_SIZE)
    }

    pub fn chunks_in_radius(&self, radius: i32) -> Vec<ChunkCoord> {
        if radius < 0 {
            return Vec::new();
        }
        let side = radius as usize * 2 + 1;
        let mut chunks = Vec::with_capacity(side * side);
        for dx in -radius..=radius {
            for dz in -radius..=radius {
                chunks.push(ChunkCoord::new(self.x + dx, self.z + dz));
            }
        }
        chunks
    }

    pub fn in_world_bounds(&self) -> bool {
        let center_x = (self.x as f32 + 0.5) * CHUNK_SIZE;
        let center_z = (self.z as f32 + 0.5) * CHUNK_SIZE;
        world_pos_in_bounds(center_x, center_z)
    }
}

/// Generated mesh data for a terrain chunk.
pub struct ChunkMeshData {
    pub positions: Vec<[f32; 3]>,
    pub normals: Vec<[f32; 3]>,
    pub uvs: Vec<[f32; 2]>,
    pub colors: Vec<[f32; 4]>,
    pub material_ids: Vec<u8>,
    pub indices: Vec<u32>,
    pub grass_indices: Vec<u32>,
    pub desert_indices: Vec<u32>,
    pub mountain_indices: Vec<u32>,
    pub nature_indices: Vec<u32>,
    pub base_indices: Vec<u32>,
    pub biome: Biome,
}
