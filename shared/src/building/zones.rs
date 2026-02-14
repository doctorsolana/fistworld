use bevy::prelude::*;
use std::collections::HashMap;

use crate::building::BuildingType;
use crate::terrain::{ChunkCoord, CHUNK_SIZE};

/// Precomputed build-zone footprint derived from a placed building.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BuildZoneEntry {
    pub center: Vec2,
    pub half_extents: Vec2,
    inv_basis_x: Vec2,
    inv_basis_z: Vec2,
}

impl BuildZoneEntry {
    #[inline]
    pub fn from_building(position: Vec3, building_type: BuildingType, rotation_y: f32) -> Self {
        let def = building_type.definition();
        let half_extents = Vec2::new(
            def.footprint.x / 2.0 + def.flatten_radius,
            def.footprint.y / 2.0 + def.flatten_radius,
        );
        let cos_r = rotation_y.cos();
        let sin_r = rotation_y.sin();

        Self {
            center: Vec2::new(position.x, position.z),
            half_extents,
            // Inverse rotation basis for world->local projection.
            inv_basis_x: Vec2::new(cos_r, sin_r),
            inv_basis_z: Vec2::new(-sin_r, cos_r),
        }
    }

    #[inline]
    pub fn contains_point(&self, point_xz: Vec2) -> bool {
        let rel = point_xz - self.center;
        let local_x = rel.dot(self.inv_basis_x);
        let local_z = rel.dot(self.inv_basis_z);
        local_x.abs() <= self.half_extents.x && local_z.abs() <= self.half_extents.y
    }

    #[inline]
    pub fn chunk_bounds(&self) -> (i32, i32, i32, i32) {
        let cos_r = self.inv_basis_x.x.abs();
        let sin_r = self.inv_basis_x.y.abs();
        let aabb_half_x = self.half_extents.x * cos_r + self.half_extents.y * sin_r;
        let aabb_half_z = self.half_extents.x * sin_r + self.half_extents.y * cos_r;

        let min_chunk_x = ((self.center.x - aabb_half_x) / CHUNK_SIZE).floor() as i32;
        let max_chunk_x = ((self.center.x + aabb_half_x) / CHUNK_SIZE).floor() as i32;
        let min_chunk_z = ((self.center.y - aabb_half_z) / CHUNK_SIZE).floor() as i32;
        let max_chunk_z = ((self.center.y + aabb_half_z) / CHUNK_SIZE).floor() as i32;

        (min_chunk_x, max_chunk_x, min_chunk_z, max_chunk_z)
    }
}

/// Build precomputed zone entries from placed building tuples.
pub fn build_zone_entries(buildings: &[(Vec3, BuildingType, f32)]) -> Vec<BuildZoneEntry> {
    buildings
        .iter()
        .map(|(position, building_type, rotation)| {
            BuildZoneEntry::from_building(*position, *building_type, *rotation)
        })
        .collect()
}

/// Build a chunk-local zone lookup map.
pub fn build_zones_by_chunk(
    buildings: &[(Vec3, BuildingType, f32)],
) -> HashMap<ChunkCoord, Vec<BuildZoneEntry>> {
    let mut by_chunk: HashMap<ChunkCoord, Vec<BuildZoneEntry>> = HashMap::new();
    for zone in build_zone_entries(buildings) {
        let (min_chunk_x, max_chunk_x, min_chunk_z, max_chunk_z) = zone.chunk_bounds();
        for chunk_x in min_chunk_x..=max_chunk_x {
            for chunk_z in min_chunk_z..=max_chunk_z {
                by_chunk
                    .entry(ChunkCoord::new(chunk_x, chunk_z))
                    .or_default()
                    .push(zone);
            }
        }
    }
    by_chunk
}

/// Check a point against precomputed build zones.
#[inline]
pub fn point_in_any_build_zone_entries(point_xz: Vec2, zones: &[BuildZoneEntry]) -> bool {
    zones.iter().any(|zone| zone.contains_point(point_xz))
}

/// Check if a point is inside any build zone (building footprint + flatten radius).
/// Returns true if the point should be excluded (prop/collider should not spawn here).
pub fn point_in_any_build_zone(point_xz: Vec2, buildings: &[(Vec3, BuildingType, f32)]) -> bool {
    buildings.iter().any(|(position, building_type, rotation)| {
        BuildZoneEntry::from_building(*position, *building_type, *rotation).contains_point(point_xz)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::building::BuildingType;

    #[test]
    fn chunk_index_lookup_matches_linear_scan() {
        let buildings = vec![
            (
                Vec3::new(4.0, 0.0, 6.0),
                BuildingType::Windmill,
                30.0f32.to_radians(),
            ),
            (
                Vec3::new(92.0, 0.0, -20.0),
                BuildingType::Church,
                -15.0f32.to_radians(),
            ),
        ];

        let by_chunk = build_zones_by_chunk(&buildings);
        let points = [
            Vec2::new(4.0, 6.0),
            Vec2::new(7.5, 4.0),
            Vec2::new(92.0, -20.0),
            Vec2::new(120.0, -20.0),
            Vec2::new(-30.0, -30.0),
        ];

        for point in points {
            let chunk = ChunkCoord::new(
                (point.x / CHUNK_SIZE).floor() as i32,
                (point.y / CHUNK_SIZE).floor() as i32,
            );
            let indexed = by_chunk
                .get(&chunk)
                .map(|zones| point_in_any_build_zone_entries(point, zones))
                .unwrap_or(false);
            let linear = point_in_any_build_zone(point, &buildings);
            assert_eq!(indexed, linear);
        }
    }
}
