//! Revisit exhausted inner plots when their physical access becomes different.
//! Snapshots are compared only at an admitted permit review, never per tick.

use super::plots::MAX_SETTLEMENT_SEARCH_RADIUS;
use crate::world::village::*;
use shared::terrain::{ChunkCoord, WorldTerrain, CHUNK_SIZE};

#[derive(PartialEq, Eq, PartialOrd, Ord)]
struct BuiltRoadAccess {
    complete: bool,
    width: u32,
    reserved_width: u32,
    points: Vec<[u32; 2]>,
}

#[derive(Default)]
pub(in crate::world::village) struct LandSearchAccess {
    center: Option<Vec2>,
    roads: Vec<BuiltRoadAccess>,
    terrain_version: u32,
    full_rebuild_version: u32,
    terrain_chunks: Vec<u32>,
    revision: u64,
}

impl LandSearchAccess {
    fn refresh(&mut self, hall: Vec3, terrain: &WorldTerrain, roads: &[&VillageRoad]) -> bool {
        // Labels, payment and unfinished suffixes cannot make land accessible.
        // Include completion because frontage selection distinguishes a fully
        // built road from an otherwise identical partial connector.
        let mut built: Vec<_> = roads
            .iter()
            .filter(|road| road.built_points().len() >= 2)
            .map(|road| BuiltRoadAccess {
                complete: road.is_complete(),
                width: road.width.to_bits(),
                reserved_width: road.reservation_width().to_bits(),
                points: road
                    .built_points()
                    .iter()
                    .map(|p| [p.x.to_bits(), p.y.to_bits()])
                    .collect(),
            })
            .collect();
        built.sort_unstable();
        let moved = self.center != Some(hall.xz());
        let terrain_chunks = (moved || self.terrain_version != terrain.modification_version())
            .then(|| {
                // One extra chunk covers plot extents and the short access
                // survey outside the 320 m centre-search envelope.
                let reach = MAX_SETTLEMENT_SEARCH_RADIUS + CHUNK_SIZE;
                let min = ChunkCoord::from_world_pos(hall - Vec3::new(reach, 0.0, reach));
                let max = ChunkCoord::from_world_pos(hall + Vec3::new(reach, 0.0, reach));
                (min.x..=max.x)
                    .flat_map(|x| (min.z..=max.z).map(move |z| ChunkCoord::new(x, z)))
                    .map(|coord| terrain.chunk_modification_version(coord))
                    .collect::<Vec<_>>()
            });
        let changed = self.center.is_some()
            && (moved
                || self.roads != built
                || self.full_rebuild_version != terrain.full_rebuild_version()
                || terrain_chunks
                    .as_ref()
                    .is_some_and(|next| *next != self.terrain_chunks));
        self.center = Some(hall.xz());
        self.roads = built;
        self.terrain_version = terrain.modification_version();
        self.full_rebuild_version = terrain.full_rebuild_version();
        if let Some(chunks) = terrain_chunks {
            self.terrain_chunks = chunks;
        }
        if changed {
            self.revision = self.revision.wrapping_add(1);
        }
        changed
    }
}

pub(super) fn refresh_land_search_access(
    clock: &mut VillageClock,
    settlement: Entity,
    hall: Vec3,
    terrain: &WorldTerrain,
    roads: &[&VillageRoad],
) -> u64 {
    let access = clock.land_search_access.entry(settlement).or_default();
    let changed = access.refresh(hall, terrain, roads);
    let revision = access.revision;
    if changed {
        clock.site_search_radii.retain(|(owner, kind), _| {
            *owner != settlement || *kind == SettlementBuildingKind::FishermansHut
        });
        if clock
            .failed_site_searches
            .get(&settlement)
            .is_some_and(|failed| failed.kind != SettlementBuildingKind::FishermansHut)
        {
            clock.failed_site_searches.remove(&settlement);
        }
    }
    revision
}

#[cfg(test)]
#[path = "search_access_tests.rs"]
mod tests;
