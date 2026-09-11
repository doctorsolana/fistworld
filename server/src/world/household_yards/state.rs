//! Changed-only authoring inputs and the small local redesign queue.

use super::*;

pub(super) const SITE_RADIUS: f32 = 24.0;
// Frontage is measured from the door as well as the house; include the door's
// displacement when finding affected house roots from a changed road segment.
const FRONTAGE_EVENT_RADIUS: f32 = SITE_RADIUS;
const HOUSE_CELL: f32 = 16.0;

#[derive(Clone, PartialEq)]
pub(super) struct HouseSource {
    pub origin: Vec3,
    pub yaw: f32,
    pub appearance: HouseAppearance,
}

#[derive(Clone)]
pub(super) struct RoadLand {
    pub points: Vec<Vec2>,
    pub reservation: f32,
    pub width: f32,
    pub built: usize,
    pub surface: RoadSurface,
    pub class: RoadClass,
}

impl RoadLand {
    pub fn from_road(road: &VillageRoad) -> Self {
        Self {
            points: road.points.clone(),
            reservation: road.reservation_width(),
            width: road.width,
            built: road.built_points().len(),
            surface: road.surface,
            class: road.class,
        }
    }

    pub fn same_claim(&self, road: &VillageRoad) -> bool {
        self.reservation == road.reservation_width() && self.points == road.points
    }

    pub fn same_frontage(&self, road: &VillageRoad) -> bool {
        self.built == road.built_points().len()
            && self.width == road.width
            && self.surface == road.surface
            && self.class == road.class
    }
}

#[derive(Clone, PartialEq)]
pub(super) struct SiteContext {
    pub source: HouseSource,
    pub land: u64,
    pub terrain: u64,
    pub frontage: u64,
}

impl SiteContext {
    pub fn new(
        entity: Entity,
        source: &HouseSource,
        land: &HouseholdYardLand,
        terrain: &WorldTerrain,
    ) -> Self {
        let lo = ChunkCoord::from_world_pos(source.origin - Vec3::splat(SITE_RADIUS + 0.5));
        let hi = ChunkCoord::from_world_pos(source.origin + Vec3::splat(SITE_RADIUS + 0.5));
        let mut stamp = u64::from(terrain.full_rebuild_version());
        for x in lo.x..=hi.x {
            for z in lo.z..=hi.z {
                stamp = shared::worldgen::splitmix64(
                    stamp ^ u64::from(terrain.chunk_modification_version(ChunkCoord::new(x, z))),
                );
            }
        }
        Self {
            source: source.clone(),
            land: land.site_signature(entity.to_bits(), source.origin, SITE_RADIUS),
            terrain: stamp,
            frontage: land.frontage_signature(source.origin, source.yaw),
        }
    }
}

#[derive(Default)]
pub(super) struct HouseLocations {
    cells: HashMap<(i32, i32), Vec<Entity>>,
    pub sources: HashMap<Entity, HouseSource>,
}

fn cell(point: Vec2) -> (i32, i32) {
    (
        (point.x / HOUSE_CELL).floor() as i32,
        (point.y / HOUSE_CELL).floor() as i32,
    )
}

impl HouseLocations {
    pub fn remove(&mut self, entity: Entity) -> bool {
        let Some(old) = self.sources.remove(&entity) else {
            return false;
        };
        let key = cell(old.origin.xz());
        if let Some(entries) = self.cells.get_mut(&key) {
            entries.retain(|e| *e != entity);
            if entries.is_empty() {
                self.cells.remove(&key);
            }
        }
        true
    }

    pub fn update(&mut self, entity: Entity, source: HouseSource) -> bool {
        if self.sources.get(&entity) == Some(&source) {
            return false;
        }
        self.remove(entity);
        self.cells
            .entry(cell(source.origin.xz()))
            .or_default()
            .push(entity);
        self.sources.insert(entity, source);
        true
    }

    pub fn around_bounds(&self, min: Vec2, max: Vec2, affected: &mut HashSet<Entity>) {
        let min = cell(min - Vec2::splat(FRONTAGE_EVENT_RADIUS));
        let max = cell(max + Vec2::splat(FRONTAGE_EVENT_RADIUS));
        for x in min.0..=max.0 {
            for z in min.1..=max.1 {
                if let Some(entries) = self.cells.get(&(x, z)) {
                    affected.extend(entries.iter().copied());
                }
            }
        }
    }

    pub fn around_segments(&self, points: &[Vec2], affected: &mut HashSet<Entity>) {
        for pair in points.windows(2) {
            self.around_bounds(pair[0].min(pair[1]), pair[0].max(pair[1]), affected);
        }
    }

    pub fn changed_frontage(
        &self,
        old: &RoadLand,
        road: &VillageRoad,
        affected: &mut HashSet<Entity>,
    ) {
        let next = road.built_points();
        if old.width == road.width && old.surface == road.surface && old.class == road.class {
            // Progress changes just the newly built/removed segments. The
            // unchanged beginning of a long road must not wake every home.
            let start = old.built.min(next.len()).saturating_sub(1);
            let end = old.built.max(next.len()).min(road.points.len());
            self.around_segments(&road.points[start..end], affected);
        } else {
            self.around_segments(&old.points[..old.built], affected);
            self.around_segments(next, affected);
        }
    }

    pub fn ordered(&self, entities: impl IntoIterator<Item = Entity>) -> Vec<Entity> {
        let mut result: Vec<_> = entities
            .into_iter()
            .filter(|entity| self.sources.contains_key(entity))
            .collect();
        result.sort_by(|a, b| {
            let a = &self.sources[a];
            let b = &self.sources[b];
            a.origin
                .x
                .total_cmp(&b.origin.x)
                .then(a.origin.z.total_cmp(&b.origin.z))
                .then(a.yaw.total_cmp(&b.yaw))
        });
        result
    }
}
