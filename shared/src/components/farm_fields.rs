//! Bounded, replicated crop parcels. Rows describe actual agricultural ground;
//! the two records remain the Farmstead's two independently worked subareas.

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

mod boundaries;
pub use boundaries::{FARM_FENCE_HEIGHT, FARM_FENCE_OBSTACLE_TYPE, FARM_FENCE_THICKNESS};

/// Immutable permanent-prop blockers for one bounded farm survey. Authority and
/// explicit offline fitting use the same recipe and horizontal baked radii.
pub fn farm_field_permanent_obstacles(
    terrain: &crate::terrain::WorldTerrain,
    farm: Vec3,
    mut radius_for: impl FnMut(crate::props::PropKind) -> Option<f32>,
) -> Vec<(Vec2, f32)> {
    use crate::terrain::ChunkCoord;
    let mut result = Vec::new();
    let lo = ChunkCoord::from_world_pos(farm - Vec3::new(50., 0., 50.));
    let hi = ChunkCoord::from_world_pos(farm + Vec3::new(50., 0., 50.));
    for x in lo.x..=hi.x {
        for z in lo.z..=hi.z {
            for prop in crate::props::generate_chunk_blocking_props(
                &terrain.generator,
                ChunkCoord::new(x, z),
            ) {
                if prop.kind.is_road_clearable() || !prop.kind.blocks_village_road() {
                    continue;
                }
                if let Some(radius) = radius_for(prop.kind) {
                    let radius = radius * prop.scale + 0.15;
                    if prop.position.distance_squared(farm.xz()) <= (45. + radius + 0.8).powi(2) {
                        result.push((prop.position, radius));
                    }
                }
            }
        }
    }
    result
}

/// One horizontal section of a field boundary, in the field entity's local X/Z.
/// Consecutive sections form trapezoids, so both soil and crops share one outline.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq)]
pub struct FarmFieldSection {
    pub z: f32,
    pub left: f32,
    pub right: f32,
}

#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
pub struct FarmFieldShape {
    pub sections: Vec<FarmFieldSection>,
}

impl FarmFieldShape {
    pub const MAX_SECTIONS: usize = 20;
    pub const REFERENCE_AREA: f32 = 88.0;

    pub fn legacy_rectangle() -> Self {
        Self {
            sections: vec![
                FarmFieldSection {
                    z: -5.5,
                    left: -4.0,
                    right: 4.0,
                },
                FarmFieldSection {
                    z: 5.5,
                    left: -4.0,
                    right: 4.0,
                },
            ],
        }
    }

    pub fn is_valid(&self) -> bool {
        (2..=Self::MAX_SECTIONS).contains(&self.sections.len())
            && self.sections.iter().all(|s| {
                s.z.is_finite()
                    && s.left.is_finite()
                    && s.right.is_finite()
                    && s.left < s.right
                    && s.left.abs().max(s.right.abs()).max(s.z.abs()) <= 48.0
            })
            && self.sections.windows(2).all(|s| s[1].z > s[0].z)
    }

    pub fn area(&self) -> f32 {
        if !self.is_valid() {
            return 0.0;
        }
        self.sections
            .windows(2)
            .map(|s| {
                (s[1].z - s[0].z) * ((s[0].right - s[0].left) + (s[1].right - s[1].left)) * 0.5
            })
            .sum()
    }

    /// Wider parcels do not create free production: throughput remains limited
    /// by the existing workers, while genuinely constrained ground yields less.
    pub fn productive_fraction(&self) -> f32 {
        (self.area() / Self::REFERENCE_AREA).clamp(0.0, 1.0)
    }

    pub fn span_at(&self, z: f32) -> Option<(f32, f32)> {
        self.sections.windows(2).find_map(|s| {
            if z < s[0].z || z > s[1].z {
                return None;
            }
            let t = (z - s[0].z) / (s[1].z - s[0].z);
            Some((
                s[0].left + (s[1].left - s[0].left) * t,
                s[0].right + (s[1].right - s[0].right) * t,
            ))
        })
    }

    pub fn contains_local_point(&self, point: Vec2, margin: f32) -> bool {
        let Some(first) = self.sections.first() else {
            return false;
        };
        let Some(last) = self.sections.last() else {
            return false;
        };
        if point.y < first.z - margin || point.y > last.z + margin {
            return false;
        }
        self.span_at(point.y.clamp(first.z, last.z))
            .is_some_and(|(left, right)| point.x >= left - margin && point.x <= right + margin)
    }

    /// Contain a whole parcel in the same local frame, including notches between
    /// the other parcel's vertices. Between either outline's section heights,
    /// both edges are linear, so their extrema occur at those shared breakpoints.
    /// Margin has the same row-axis meaning as `contains_local_point`.
    pub fn contains_shape(&self, other: &Self, margin: f32) -> bool {
        if !self.is_valid() || !other.is_valid() || !margin.is_finite() || margin < 0.0 {
            return false;
        }
        let first = other.sections[0].z;
        let last = other.sections.last().unwrap().z;
        other
            .sections
            .iter()
            .map(|section| section.z)
            .chain(
                self.sections
                    .iter()
                    .map(|section| section.z)
                    .filter(|z| *z >= first && *z <= last),
            )
            .all(|z| {
                let (left, right) = other.span_at(z).unwrap();
                self.contains_local_point(Vec2::new(left, z), margin)
                    && self.contains_local_point(Vec2::new(right, z), margin)
            })
    }

    pub fn contains_world_point(
        &self,
        point: Vec2,
        position: Vec3,
        rotation: f32,
        margin: f32,
    ) -> bool {
        self.contains_local_point(
            crate::rotation::world_to_local_xz(point - position.xz(), rotation),
            margin,
        )
    }

    /// Small deterministic candidate set for the ordinary route certifier.
    /// Every candidate is inside the accepted crop ground, including clipped fields.
    pub fn work_candidates(&self, salt: u32) -> Vec<Vec2> {
        if !self.is_valid() {
            return Vec::new();
        }
        let mut candidates = Vec::with_capacity(8);
        for i in 0..8 {
            let index = (i * 3 + salt as usize % self.sections.len()) % self.sections.len();
            let s = self.sections[index];
            let first = self.sections[0].z;
            let last = self.sections.last().unwrap().z;
            let inset = 0.6_f32.min((last - first) * 0.5);
            let z = s.z.clamp(first + inset, last - inset);
            if let Some((left, right)) = self.span_at(z) {
                let x = left + (right - left) * if i & 1 == 0 { 0.4 } else { 0.6 };
                let p = Vec2::new(x, z);
                if self.contains_local_point(p, -0.35) {
                    candidates.push(p);
                }
            }
        }
        candidates
    }
}

/// Survey one planned agricultural parcel behind a Farmstead. New plots reserve
/// the intended envelope at approval; legacy farms may only expand into land
/// the caller certifies as unclaimed. Expanded ground follows its existing grade
/// rather than creating a larger flat terrace. Site checks include dry ground,
/// roads, neighbouring claims and permanent props.
///
/// Straight tapered boundaries give each farm a coherent parcel rather than
/// noisy edges on two repeated rectangles. Obstructions shorten actual rows;
/// disconnected slivers are discarded. At most 19 × 81 candidates are sampled.
pub fn fit_farm_field_shapes(
    farm: Vec3,
    rotation: f32,
    seed: u64,
    mut usable: impl FnMut(Vec2) -> bool,
) -> [FarmFieldShape; 2] {
    let mut rng = crate::rng::XorShift64::new(
        seed ^ (farm.x.to_bits() as u64).rotate_left(17) ^ farm.z.to_bits() as u64,
    );
    let mut roll = || (rng.next_u64() >> 40) as f32 / (1_u32 << 24) as f32;
    let front = 3.25 + roll() * 0.20;
    let back = 30.0 + roll() * 4.0;
    let left_width = 16.6 + roll() * 2.7;
    let right_width = 16.6 + roll() * 2.7;
    let lean = (roll() - 0.5) * 2.0;
    let end_taper = 1.0 + roll() * 1.5;
    let count = 19;
    let dz = (back - front) / (count - 1) as f32;
    let mut bands: Vec<Option<FarmFieldSection>> = Vec::with_capacity(count);
    for index in 0..count {
        let t = index as f32 / (count - 1) as f32;
        let z = front + dz * index as f32;
        let taper = if t < 0.2 { (0.2 - t) * 8.0 } else { 0.0 }
            + if t > 0.7 {
                (t - 0.7) / 0.3 * end_taper
            } else {
                0.0
            };
        let left = (-left_width + taper + lean * t).max(-19.6);
        let right = (right_width - taper + lean * t).min(19.6);
        let mut runs = Vec::new();
        let mut start = None;
        let steps = ((right - left) / 0.5).floor() as usize;
        for step in 0..=steps + 1 {
            let x = left + step as f32 * 0.5;
            let valid = step <= steps && {
                // Include the half-band height, so interpolation between the
                // boundary sections cannot bridge a thin wet/blocked strip.
                [-dz * 0.5, 0.0, dz * 0.5].into_iter().all(|offset| {
                    let local = Vec2::new(x, (z + offset).clamp(front, back));
                    usable(farm.xz() + crate::rotation::local_to_world_xz(local, rotation))
                })
            };
            if valid {
                if start.is_none() {
                    start = Some(x);
                }
            } else if let Some(begin) = start.take() {
                let end = x - 0.5;
                if end - begin >= 2.0 {
                    runs.push((begin, end));
                }
            }
        }
        let best = runs.into_iter().max_by(|a, b| {
            // Prefer the broad workable patch, with a modest preference for
            // the shared centre so paired workers do not claim separate islands.
            let score = |r: &(f32, f32)| r.1 - r.0 - (r.0 + r.1).abs() * 0.15;
            score(a).total_cmp(&score(b))
        });
        bands.push(best.map(|(left, right)| FarmFieldSection { z, left, right }));
    }
    // Retain the largest connected run. A river/road through the parcel cannot
    // be hidden by filling a polygon across separate workable pieces.
    let mut runs: Vec<Vec<FarmFieldSection>> = Vec::new();
    let mut run: Vec<FarmFieldSection> = Vec::new();
    for band in bands {
        if let Some(section) = band {
            if run.last().is_some_and(|last| {
                section.left.max(last.left) + 1.0 >= section.right.min(last.right)
            }) {
                runs.push(std::mem::take(&mut run));
            }
            run.push(section);
        } else if !run.is_empty() {
            runs.push(std::mem::take(&mut run));
        }
    }
    if !run.is_empty() {
        runs.push(run);
    }
    let parcel = runs
        .into_iter()
        .max_by(|a, b| {
            let area = |s: &Vec<FarmFieldSection>| {
                s.windows(2)
                    .map(|w| {
                        (w[1].z - w[0].z) * (w[0].right - w[0].left + w[1].right - w[1].left) * 0.5
                    })
                    .sum::<f32>()
            };
            area(a).total_cmp(&area(b))
        })
        .unwrap_or_default();
    std::array::from_fn(|index| {
        let side = if index == 0 {
            -super::FARM_FIELD_LATERAL_OFFSET
        } else {
            super::FARM_FIELD_LATERAL_OFFSET
        };
        let mut runs: Vec<Vec<FarmFieldSection>> = Vec::new();
        let mut current = Vec::new();
        for s in &parcel {
            let left = if index == 0 { s.left } else { s.left.max(0.0) };
            let right = if index == 0 {
                s.right.min(0.0)
            } else {
                s.right
            };
            if right - left >= 1.0 {
                current.push(FarmFieldSection {
                    z: s.z - 9.0,
                    left: left - side,
                    right: right - side,
                });
            } else if !current.is_empty() {
                runs.push(std::mem::take(&mut current));
            }
        }
        if !current.is_empty() {
            runs.push(current);
        }
        let shape = runs
            .into_iter()
            .map(|sections| FarmFieldShape { sections })
            .max_by(|a, b| a.area().total_cmp(&b.area()))
            .unwrap_or_default();
        if shape.is_valid() && shape.area() >= 10.0 && !shape.work_candidates(0).is_empty() {
            shape
        } else {
            FarmFieldShape::default()
        }
    })
}

/// Shared site contract for authority and explicitly opted-in offline captures.
/// `clear_of_props` lets the server supply its derived-collider index without
/// introducing server geometry or renderer resources into the shared crate.
pub fn fit_farm_field_shapes_on_terrain(
    terrain: &crate::terrain::WorldTerrain,
    farm: Vec3,
    rotation: f32,
    buildings: &[(super::SettlementBuildingKind, Vec3, f32)],
    roads: &[&super::VillageRoad],
    mut clear_of_props: impl FnMut(Vec2) -> bool,
) -> [FarmFieldShape; 2] {
    let seed = terrain
        .generator
        .loaded_map()
        .definition
        .generated
        .as_ref()
        .map_or(0, |map| map.seed);
    let blockers: Vec<_> = buildings
        .iter()
        .flat_map(|(kind, position, facing)| {
            crate::building::clearance_zones_for_building(
                *position,
                kind.placement_definition().building_type,
                *facing,
            )
        })
        .collect();
    fit_farm_field_shapes(farm, rotation, seed, |point| {
        if !crate::terrain::world_pos_in_bounds(point.x, point.y)
            || blockers.iter().any(|zone| zone.contains_point(point))
            || roads
                .iter()
                .any(|road| road.contains_reserved_point(point, 0.8))
        {
            return false;
        }
        let height = terrain.get_height(point.x, point.y);
        if !height.is_finite()
            || terrain
                .water_surface_height(point.x, point.y)
                .is_some_and(|water| height - water < 0.7)
        {
            return false;
        }
        if [Vec2::X, Vec2::NEG_X, Vec2::Y, Vec2::NEG_Y]
            .into_iter()
            .any(|axis| {
                let p = point + axis * 0.65;
                (terrain.get_height(p.x, p.y) - height).abs() > 0.65 * 0.38
                    || terrain
                        .water_surface_height(p.x, p.y)
                        .is_some_and(|water| terrain.get_height(p.x, p.y) - water < 0.7)
            })
        {
            return false;
        }
        clear_of_props(point)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn larger_parcel_cannot_cut_old_land_between_its_section_heights() {
        let old = FarmFieldShape::legacy_rectangle();
        let notched = FarmFieldShape {
            sections: [(-5.5, -5.0), (-1.0, -1.0), (1.0, -1.0), (5.5, -5.0)]
                .into_iter()
                .map(|(z, left)| FarmFieldSection {
                    z,
                    left,
                    right: 10.0,
                })
                .collect(),
        };
        assert!(notched.area() > old.area());
        assert!(old
            .boundary_points()
            .iter()
            .all(|p| notched.contains_local_point(*p, 0.05)));
        assert!(!notched.contains_local_point(Vec2::new(-3.0, 0.0), 0.05));
        assert!(!notched.contains_shape(&old, 0.05));

        let mut expanded = notched;
        for section in &mut expanded.sections {
            section.left = section.left.min(-4.2);
        }
        assert!(expanded.contains_shape(&old, 0.05));
        assert!(!old.contains_shape(&expanded, 0.05));
        assert!(old.contains_shape(&old, 0.0));
    }

    #[test]
    fn shape_containment_checks_inner_breakpoints_and_front_back_extent() {
        let outer = FarmFieldShape::legacy_rectangle();
        let mut inner = outer.clone();
        inner.sections.insert(
            1,
            FarmFieldSection {
                z: 0.0,
                left: -4.2,
                right: 3.0,
            },
        );
        assert!(!outer.contains_shape(&inner, 0.05));
        inner.sections[1].left = -3.0;
        assert!(outer.contains_shape(&inner, 0.05));

        inner.sections[0].z -= 0.04;
        inner.sections.last_mut().unwrap().z += 0.04;
        assert!(outer.contains_shape(&inner, 0.05));
        assert!(!outer.contains_shape(&inner, 0.0));
        inner.sections[0].z -= 0.1;
        assert!(!outer.contains_shape(&inner, 0.05));
        assert!(!outer.contains_shape(&FarmFieldShape::default(), 0.05));
        assert!(!FarmFieldShape::default().contains_shape(&outer, 0.05));
    }

    #[test]
    fn parcels_are_deterministic_contiguous_and_within_new_intended_envelope() {
        let farm = Vec3::new(18.0, 0.0, -40.0);
        let a = fit_farm_field_shapes(farm, 0.8, 12, |_| true);
        assert_eq!(a, fit_farm_field_shapes(farm, 0.8, 12, |_| true));
        assert_ne!(a, fit_farm_field_shapes(farm, 0.8, 13, |_| true));
        for (index, shape) in a.iter().enumerate() {
            assert!(shape.is_valid());
            assert!(shape.area() > 330.0);
            assert!(shape.productive_fraction() <= 1.0);
            let offset = if index == 0 {
                -super::super::FARM_FIELD_LATERAL_OFFSET
            } else {
                super::super::FARM_FIELD_LATERAL_OFFSET
            };
            for s in &shape.sections {
                assert!(s.left + offset >= -20.0 && s.right + offset <= 20.0);
                assert!((3.0..=35.0).contains(&(s.z + 9.0)));
            }
        }
        assert_eq!(
            a[0].sections[5].right - super::super::FARM_FIELD_LATERAL_OFFSET,
            0.0
        );
        assert_eq!(
            a[1].sections[5].left + super::super::FARM_FIELD_LATERAL_OFFSET,
            0.0
        );
    }

    #[test]
    fn diagonal_shore_shortens_real_rows_and_reduces_work_capacity() {
        let full = fit_farm_field_shapes(Vec3::ZERO, 0.0, 12, |_| true);
        let shore = |point: Vec2| point.x < 4.0 - (point.y - 4.0) * 0.7;
        let clipped = fit_farm_field_shapes(Vec3::ZERO, 0.0, 12, shore);
        assert!(clipped[1].area() < full[1].area() * 0.65);
        assert!(clipped[1].productive_fraction() < 1.0);
        for (index, shape) in clipped.iter().enumerate() {
            let x = if index == 0 {
                -super::super::FARM_FIELD_LATERAL_OFFSET
            } else {
                super::super::FARM_FIELD_LATERAL_OFFSET
            };
            for s in &shape.sections {
                assert!(shore(Vec2::new(s.right + x, s.z + 9.0)));
            }
        }
    }

    #[test]
    fn blocking_strip_does_not_create_a_bridge_between_disconnected_pieces() {
        let shapes = fit_farm_field_shapes(Vec3::ZERO, 0.0, 7, |p| !(8.0..10.0).contains(&p.y));
        for shape in &shapes {
            for s in &shape.sections {
                assert!(!(8.0..10.0).contains(&(s.z + 9.0)));
            }
            if shape.is_valid() {
                assert!(
                    !(shape.sections[0].z + 9.0 < 8.0
                        && shape.sections.last().unwrap().z + 9.0 > 10.0)
                );
            }
        }
    }

    #[test]
    fn a_shallow_workable_patch_has_inset_candidates_without_panicking() {
        let shape = FarmFieldShape {
            sections: vec![
                FarmFieldSection {
                    z: 0.0,
                    left: -7.0,
                    right: 7.0,
                },
                FarmFieldSection {
                    z: 0.8,
                    left: -7.0,
                    right: 7.0,
                },
            ],
        };
        assert!(shape.is_valid());
        assert!(!shape.work_candidates(0).is_empty());
        for p in shape.work_candidates(0) {
            assert!(shape.contains_local_point(p, -0.35));
        }
    }

    #[test]
    fn each_worker_half_discards_disconnected_slivers() {
        let shapes = fit_farm_field_shapes(Vec3::ZERO, 0.0, 7, |p| {
            p.x < -1.0 || !(8.0..10.0).contains(&p.y)
        });
        let right = &shapes[1];
        if right.is_valid() {
            assert!(
                !(right.sections[0].z + 9.0 < 8.0 && right.sections.last().unwrap().z + 9.0 > 10.0)
            );
        }
    }

    #[test]
    fn work_candidates_stay_inside_clipped_ground_and_legacy_area_is_preserved() {
        assert_eq!(FarmFieldShape::legacy_rectangle().area(), 88.0);
        let shapes = fit_farm_field_shapes(Vec3::ZERO, 0.0, 19, |p| p.x < 7.0 - p.y * 0.3);
        for shape in shapes {
            for point in shape.work_candidates(5) {
                assert!(shape.contains_local_point(point, -0.35));
            }
        }
    }
}
