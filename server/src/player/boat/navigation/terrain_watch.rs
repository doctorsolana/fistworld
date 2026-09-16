//! Terrain cells actually read by a retained geometric search. Sparse edits
//! elsewhere must not discard its frontier; touched edits and full rebuilds do.
use bevy::{platform::collections::HashMap, prelude::*};
use shared::terrain::{ChunkCoord, WorldTerrain, CHUNK_SIZE};

#[derive(Default, Debug, Clone)]
pub(crate) struct TerrainDependencies {
    revision: Option<(u64, u32, u32)>,
    chunks: HashMap<ChunkCoord, u32>,
    last: Option<ChunkCoord>,
}

impl TerrainDependencies {
    pub(crate) fn changed(&mut self, terrain: &WorldTerrain) -> bool {
        let next = (
            terrain.generator.active_map_content_hash(),
            terrain.full_rebuild_version(),
            terrain.modification_version(),
        );
        if self.revision == Some(next) {
            return false;
        }
        let changed = self.revision.is_some_and(|old| {
            old.0 != next.0
                || old.1 != next.1
                || self
                    .chunks
                    .iter()
                    .any(|(chunk, version)| terrain.chunk_modification_version(*chunk) != *version)
        });
        if changed {
            self.chunks.clear();
            self.last = None;
        }
        self.revision = Some(next);
        changed
    }

    pub(crate) fn observe(&mut self, terrain: &WorldTerrain, point: Vec2) {
        let chunk = ChunkCoord::from_world_pos(Vec3::new(point.x, 0., point.y));
        // Most neighbouring hull samples lie in the same chunk.
        if self.last == Some(chunk) {
            return;
        }
        self.chunks
            .entry(chunk)
            .or_insert_with(|| terrain.chunk_modification_version(chunk));
        self.last = Some(chunk);
    }

    /// Cover long diagonal legs in small envelopes rather than indexing the
    /// enormous bounding rectangle between two distant endpoints.
    pub(crate) fn observe_segment(
        &mut self,
        terrain: &WorldTerrain,
        start: Vec2,
        end: Vec2,
        radius: f32,
    ) {
        let steps = (start.distance(end) / CHUNK_SIZE).ceil().max(1.) as usize;
        let padding = Vec2::splat(radius);
        let mut previous = start;
        for step in 1..=steps {
            let point = start.lerp(end, step as f32 / steps as f32);
            self.observe_bounds(
                terrain,
                previous.min(point) - padding,
                previous.max(point) + padding,
            );
            previous = point;
        }
    }

    pub(crate) fn observe_bounds(&mut self, terrain: &WorldTerrain, min: Vec2, max: Vec2) {
        let bounds = terrain.generator.active_map_bounds();
        let min = min.max(Vec2::from_array(bounds.min));
        let max = max.min(Vec2::from_array(bounds.max));
        let min = ChunkCoord::from_world_pos(Vec3::new(min.x, 0., min.y));
        let max = ChunkCoord::from_world_pos(Vec3::new(max.x, 0., max.y));
        for x in min.x..=max.x {
            for z in min.z..=max.z {
                let chunk = ChunkCoord::new(x, z);
                self.chunks
                    .entry(chunk)
                    .or_insert_with(|| terrain.chunk_modification_version(chunk));
            }
        }
    }
}
