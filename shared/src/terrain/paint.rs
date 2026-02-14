use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Terrain surface layers used for splatmap blending.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TerrainLayer {
    Grass,
    Dirt,
    Sand,
    Cobblestone,
}

impl TerrainLayer {
    pub fn index(self) -> usize {
        match self {
            TerrainLayer::Grass => 0,
            TerrainLayer::Dirt => 1,
            TerrainLayer::Sand => 2,
            TerrainLayer::Cobblestone => 3,
        }
    }
}

/// Shape used for terrain paint operations.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum TerrainPaintShape {
    Rect {
        center: Vec2,
        half_extents: Vec2,
        rotation: f32,
    },
    Line {
        start: Vec2,
        end: Vec2,
        width: f32,
    },
    Circle {
        center: Vec2,
        radius: f32,
    },
}

/// Server-authoritative terrain paint operation (replicated).
#[derive(Component, Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TerrainPaintOp {
    pub id: u64,
    pub layer: TerrainLayer,
    /// Strength 0..1 applied to the target layer.
    pub strength: f32,
    /// Blend falloff width in meters (soft edge).
    pub falloff: f32,
    pub shape: TerrainPaintShape,
}
