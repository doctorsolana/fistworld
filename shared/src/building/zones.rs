use bevy::prelude::*;
use std::collections::HashMap;

use crate::building::BuildingType;
use crate::terrain::{ChunkCoord, CHUNK_SIZE};

/// Precomputed build-zone footprint derived from a placed building.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BuildZoneEntry {
    pub center: Vec2,
    pub half_extents: Vec2,
    rotation_y: f32,
    inv_basis_x: Vec2,
    inv_basis_z: Vec2,
}

impl BuildZoneEntry {
    #[inline]
    pub fn from_rotated_rect(center: Vec2, half_extents: Vec2, rotation_y: f32) -> Self {
        let cos_r = rotation_y.cos();
        let sin_r = rotation_y.sin();

        Self {
            center,
            half_extents,
            rotation_y,
            inv_basis_x: Vec2::new(cos_r, -sin_r),
            inv_basis_z: Vec2::new(sin_r, cos_r),
        }
    }

    #[inline]
    pub fn from_building(position: Vec3, building_type: BuildingType, rotation_y: f32) -> Self {
        let def = building_type.definition();
        let half_extents = Vec2::new(
            def.footprint.x / 2.0 + def.flatten_radius,
            def.footprint.y / 2.0 + def.flatten_radius,
        );
        Self::from_rotated_rect(
            def.world_footprint_center(position, rotation_y),
            half_extents,
            rotation_y,
        )
    }

    #[inline]
    pub fn contains_point(&self, point_xz: Vec2) -> bool {
        // The ONE convention (crate::rotation), so this can never drift from
        // where the model actually stands again.
        let local = crate::rotation::world_to_local_xz(point_xz - self.center, self.rotation_y);
        local.x.abs() <= self.half_extents.x && local.y.abs() <= self.half_extents.y
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

/// Every piece of ground claimed when a building is cleared.
///
/// Most buildings own only their landscaped footprint. A Farmstead also owns
/// its two authored crop plots: this is the shared authority used by client
/// foliage culling and server collider streaming, so a tree accepted as
/// clearable by the planner cannot survive inside the finished wheat rows.
pub fn clearance_zones_for_building(
    position: Vec3,
    building_type: BuildingType,
    rotation_y: f32,
) -> Vec<BuildZoneEntry> {
    let mut zones = vec![BuildZoneEntry::from_building(
        position,
        building_type,
        rotation_y,
    )];
    if building_type == BuildingType::Farmstead {
        let kind = crate::components::SettlementBuildingKind::Farmstead;
        if let (Some(fields), Some(half)) = (
            kind.field_positions(position, rotation_y),
            kind.field_half_extents(),
        ) {
            let half = half + Vec2::splat(crate::components::FARM_FIELD_TERRACE_MARGIN);
            zones.extend(fields.into_iter().map(|field| {
                BuildZoneEntry::from_rotated_rect(Vec2::new(field.x, field.z), half, rotation_y)
            }));
        }
    }
    zones
}

/// Build precomputed zone entries from placed building tuples.
pub fn build_zone_entries(buildings: &[(Vec3, BuildingType, f32)]) -> Vec<BuildZoneEntry> {
    buildings
        .iter()
        .flat_map(|(position, building_type, rotation)| {
            clearance_zones_for_building(*position, *building_type, *rotation)
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
        clearance_zones_for_building(*position, *building_type, *rotation)
            .iter()
            .any(|zone| zone.contains_point(point_xz))
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
                BuildingType::MootHall,
                30.0f32.to_radians(),
            ),
            (
                Vec3::new(92.0, 0.0, -20.0),
                BuildingType::Farmstead,
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

    #[test]
    fn farmstead_clearance_claims_both_authored_fields() {
        let position = Vec3::new(92.0, 0.0, -20.0);
        let rotation = 0.37;
        let zones = clearance_zones_for_building(position, BuildingType::Farmstead, rotation);
        assert_eq!(zones.len(), 3, "farmyard plus two crop plots");

        let fields = crate::components::SettlementBuildingKind::Farmstead
            .field_positions(position, rotation)
            .unwrap();
        for field in fields {
            let point = Vec2::new(field.x, field.z);
            assert!(zones.iter().any(|zone| zone.contains_point(point)));
        }

        assert_eq!(
            clearance_zones_for_building(position, BuildingType::LogCabin, rotation).len(),
            1,
            "ordinary buildings still claim only their landscaped footprint"
        );
    }
}

#[cfg(test)]
mod rotation_tests {
    use super::*;

    /// The zone must agree with the MODEL about which way the building faces.
    ///
    /// The model is placed with `Transform::with_rotation(Quat::from_rotation_y(r))`,
    /// so that quaternion is the ground truth for where the walls actually are.
    /// This takes points that are inside the footprint in the building's own
    /// local space, moves them into the world exactly the way the renderer
    /// does, and asks the zone whether they are inside.
    ///
    /// A sign error here is invisible on a square building and leaves an
    /// uncovered wedge on an oblong one — which is precisely how a tree ends up
    /// standing inside a finished house.
    #[test]
    fn the_zone_covers_the_ground_the_model_actually_stands_on() {
        let kind = BuildingType::MootHall;
        let def = kind.definition();
        let centre = Vec3::new(137.0, 0.0, -64.0);

        // Corners of the real footprint, in the building's local frame.
        let half = def.footprint * 0.5;
        let local_corners = [
            def.footprint_center + Vec2::new(half.x, half.y),
            def.footprint_center + Vec2::new(-half.x, half.y),
            def.footprint_center + Vec2::new(half.x, -half.y),
            def.footprint_center + Vec2::new(-half.x, -half.y),
        ];

        for step in 0..16 {
            let rotation = std::f32::consts::TAU * step as f32 / 16.0;
            let zone = BuildZoneEntry::from_building(centre, kind, rotation);
            let quat = Quat::from_rotation_y(rotation);
            for corner in local_corners {
                // Exactly how the renderer places a vertex of the model.
                let world = centre + quat * Vec3::new(corner.x, 0.0, corner.y);
                assert!(
                    zone.contains_point(Vec2::new(world.x, world.z)),
                    "rotation {rotation:.3}: the model's corner {corner:?} lands at \
                     ({:.2},{:.2}), which the build zone says is OUTSIDE the building",
                    world.x,
                    world.z
                );
            }
        }
    }

    /// And the chunk bounds must contain the zone, or the clearing never even
    /// looks at the chunk the tree is in.
    #[test]
    fn chunk_bounds_cover_every_rotated_corner() {
        let kind = BuildingType::MootHall;
        let centre = Vec3::new(137.0, 0.0, -64.0);
        for step in 0..16 {
            let rotation = std::f32::consts::TAU * step as f32 / 16.0;
            let zone = BuildZoneEntry::from_building(centre, kind, rotation);
            let (min_x, max_x, min_z, max_z) = zone.chunk_bounds();
            let quat = Quat::from_rotation_y(rotation);
            let def = kind.definition();
            let half = def.footprint * 0.5 + Vec2::splat(def.flatten_radius);
            for corner in [
                def.footprint_center + Vec2::new(half.x, half.y),
                def.footprint_center + Vec2::new(-half.x, half.y),
                def.footprint_center + Vec2::new(half.x, -half.y),
                def.footprint_center + Vec2::new(-half.x, -half.y),
            ] {
                let world = centre + quat * Vec3::new(corner.x, 0.0, corner.y);
                let cx = (world.x / CHUNK_SIZE).floor() as i32;
                let cz = (world.z / CHUNK_SIZE).floor() as i32;
                assert!(
                    (min_x..=max_x).contains(&cx) && (min_z..=max_z).contains(&cz),
                    "rotation {rotation:.3}: corner chunk ({cx},{cz}) outside bounds \
                     ({min_x}..={max_x}, {min_z}..={max_z})"
                );
            }
        }
    }

    /// The exact check the two above cannot make.
    ///
    /// `flatten_radius` pads the zone by metres, so a rotation that is wrong by
    /// a sign can still contain a footprint corner and look fine. This asks the
    /// only question that has no slack in it: does the zone's world->local
    /// transform actually INVERT the quaternion the renderer used?
    #[test]
    fn world_to_local_is_the_exact_inverse_of_the_model_rotation() {
        let kind = BuildingType::MootHall;
        let centre = Vec3::new(137.0, 0.0, -64.0);
        // Deliberately asymmetric, and not on an axis, so any sign or transpose
        // error shows up instead of cancelling.
        let local = Vec2::new(2.75, -1.25);

        for step in 1..12 {
            let rotation = std::f32::consts::TAU * step as f32 / 12.0;
            let zone = BuildZoneEntry::from_building(centre, kind, rotation);
            let world = centre + Quat::from_rotation_y(rotation) * Vec3::new(local.x, 0.0, local.y);
            let rel = Vec2::new(world.x, world.z) - zone.center;
            let round_tripped = Vec2::new(rel.dot(zone.inv_basis_x), rel.dot(zone.inv_basis_z));
            let expected = local - kind.definition().footprint_center;
            assert!(
                (round_tripped - expected).length() < 1e-3,
                "rotation {rotation:.3}: root-local {local:?} -> zone-local came back as \
                 {round_tripped:?}; the zone is not the inverse of the model rotation"
            );
        }
    }
}
