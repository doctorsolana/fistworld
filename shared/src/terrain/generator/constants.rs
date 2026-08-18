/// World seed remains available for deterministic systems that still key off it.
pub const WORLD_SEED: u32 = 42;

/// Chunk size in world units (meters).
pub const CHUNK_SIZE: f32 = 64.0;
/// Number of vertices per chunk side (resolution).
pub const CHUNK_RESOLUTION: usize = 33;
/// Spacing between vertices.
pub const VERTEX_SPACING: f32 = CHUNK_SIZE / (CHUNK_RESOLUTION - 1) as f32;

/// Height normalization helper for mesh color variation.
pub const MAX_HEIGHT: f32 = 25.0;

/// Legacy world radius constants retained for systems that still read them.
pub const WORLD_RADIUS_CHUNKS: i32 = 90;
pub const WORLD_RADIUS_METERS: f32 = WORLD_RADIUS_CHUNKS as f32 * CHUNK_SIZE;

/// Default sea level used by existing water visuals.
pub const SEA_LEVEL: f32 = 0.0;
