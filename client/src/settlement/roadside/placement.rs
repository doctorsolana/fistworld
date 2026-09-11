//! Setup-time placement for low, cosmetic road-shoulder details.

use bevy::prelude::*;
use shared::building::BuildZoneEntry;
use shared::components::{distance_squared_to_segment, FarmField, FarmFieldShape, HouseholdYard};
use shared::terrain::{ChunkCoord, CHUNK_SIZE};

pub(super) const MAX_CANDIDATES_PER_CHUNK: usize = 512;
pub(super) const MAX_CLUSTERS_PER_CHUNK: usize = 64;
// The close shoulder plus occasional planted patches in the open land between
// lanes. The index must cover that entire band, including chunk-border centres.
pub(super) const ROADSIDE_BAND: f32 = 22.0;

#[derive(Clone, Copy, Debug)]
pub(super) struct RoadSegment {
    pub owner: Entity,
    pub start: Vec2,
    pub end: Vec2,
    pub distance_before: f32,
    pub width: f32,
    pub reserved_width: f32,
    pub built: bool,
}

impl RoadSegment {
    pub fn chunks(&self) -> impl Iterator<Item = ChunkCoord> {
        let padding = self.reserved_width * 0.5 + ROADSIDE_BAND;
        let minimum = ((self.start.min(self.end) - Vec2::splat(padding)) / CHUNK_SIZE)
            .floor()
            .as_ivec2();
        let maximum = ((self.start.max(self.end) + Vec2::splat(padding)) / CHUNK_SIZE)
            .floor()
            .as_ivec2();
        let start = self.start;
        let delta = self.end - start;
        // Clip each column against the expanded segment, instead of indexing
        // the whole AABB of a long diagonal road. The conservative square band
        // includes all decoration/clearance samples, with work linear in length.
        (minimum.x..=maximum.x)
            .filter_map(move |x| {
                let (from, to) = if delta.x.abs() < 0.001 {
                    (0.0, 1.0)
                } else {
                    let a = (x as f32 * CHUNK_SIZE - padding - start.x) / delta.x;
                    let b = ((x + 1) as f32 * CHUNK_SIZE + padding - start.x) / delta.x;
                    (a.min(b).max(0.0), a.max(b).min(1.0))
                };
                if from > to {
                    return None;
                }
                let a = start.y + delta.y * from;
                let b = start.y + delta.y * to;
                let first = ((a.min(b) - padding) / CHUNK_SIZE).floor() as i32;
                let last = ((a.max(b) + padding) / CHUNK_SIZE).floor() as i32;
                Some((x, first, last))
            })
            .flat_map(|(x, first, last)| (first..=last).map(move |z| ChunkCoord::new(x, z)))
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) enum PlotShape {
    Field(FarmFieldShape),
    Yard(HouseholdYard),
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct PlotFootprint {
    pub origin: Vec3,
    pub yaw: f32,
    pub shape: PlotShape,
}

impl PlotFootprint {
    pub fn from_field(field: &FarmField, origin: Vec3, yaw: f32) -> Self {
        Self {
            origin,
            yaw,
            // An explicit empty accepted shape owns no crop land. Only a
            // missing shape uses the original saved rectangle.
            shape: PlotShape::Field(
                field
                    .shape
                    .clone()
                    .unwrap_or_else(FarmFieldShape::legacy_rectangle),
            ),
        }
    }

    pub fn contains(&self, point: Vec2, radius: f32) -> bool {
        match &self.shape {
            PlotShape::Field(shape) => {
                if !shape.is_valid() {
                    return false;
                }
                let local = shared::rotation::world_to_local_xz(point - self.origin.xz(), self.yaw);
                if shape.contains_local_point(local, 0.) {
                    return true;
                }
                // Expanding X bounds at one row underestimates the clearance
                // perpendicular to a slanted edge. Measure the actual polygon
                // segments, including the end caps, without per-candidate allocation.
                let clearance_squared = (radius + 0.8).powi(2);
                let close_to = |a: Vec2, b: Vec2| {
                    distance_squared_to_segment(local, a, b) <= clearance_squared
                };
                shape.sections.windows(2).any(|pair| {
                    close_to(
                        Vec2::new(pair[0].left, pair[0].z),
                        Vec2::new(pair[1].left, pair[1].z),
                    ) || close_to(
                        Vec2::new(pair[0].right, pair[0].z),
                        Vec2::new(pair[1].right, pair[1].z),
                    )
                }) || [
                    shape.sections.first().unwrap(),
                    shape.sections.last().unwrap(),
                ]
                .into_iter()
                .any(|section| {
                    close_to(
                        Vec2::new(section.left, section.z),
                        Vec2::new(section.right, section.z),
                    )
                })
            }
            PlotShape::Yard(yard) => crate::settlement::yards::ground_cover_contains(
                yard,
                point,
                self.origin,
                self.yaw,
                radius + 0.45,
            ),
        }
    }

    pub fn zone(&self) -> Option<BuildZoneEntry> {
        let (minimum, maximum) = match &self.shape {
            PlotShape::Yard(yard) => {
                let (lo, hi) =
                    crate::settlement::yards::ground_cover_bounds(yard, self.origin, self.yaw, 3.0);
                return Some(BuildZoneEntry::from_rotated_rect(
                    (lo + hi) * 0.5,
                    (hi - lo) * 0.5,
                    0.,
                ));
            }
            PlotShape::Field(shape) => {
                if !shape.is_valid() {
                    return None;
                }
                let mut minimum = Vec2::splat(f32::INFINITY);
                let mut maximum = Vec2::splat(f32::NEG_INFINITY);
                for section in &shape.sections {
                    minimum = minimum.min(Vec2::new(section.left, section.z));
                    maximum = maximum.max(Vec2::new(section.right, section.z));
                }
                (minimum, maximum)
            }
        };
        let center = self.origin.xz()
            + shared::rotation::local_to_world_xz((minimum + maximum) * 0.5, self.yaw);
        Some(BuildZoneEntry::from_rotated_rect(
            center,
            (maximum - minimum) * 0.5 + Vec2::splat(3.0),
            self.yaw,
        ))
    }

    pub fn chunks(&self) -> Vec<ChunkCoord> {
        let Some(zone) = self.zone() else {
            return Vec::new();
        };
        let (x0, x1, z0, z1) = zone.chunk_bounds();
        (x0..=x1)
            .flat_map(|x| (z0..=z1).map(move |z| ChunkCoord::new(x, z)))
            .collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum DetailKind {
    Stone,
    Flowers,
    Bush,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Candidate {
    pub point: Vec2,
    pub radius: f32,
    pub seed: u64,
    pub kind: DetailKind,
}

impl Candidate {
    pub fn clear_of_road(&self, segment: &RoadSegment) -> bool {
        let half = if self.kind == DetailKind::Bush || !segment.built {
            segment.reserved_width * 0.5
        } else {
            segment.width * 0.5
        };
        let gap = match self.kind {
            DetailKind::Stone => 0.08,
            DetailKind::Flowers => 0.35,
            DetailKind::Bush => 0.45,
        };
        distance_squared_to_segment(self.point, segment.start, segment.end)
            > (half + self.radius + gap).powi(2)
    }
}

pub(super) fn roll(seed: u64, salt: u64) -> f32 {
    let mut value = seed.wrapping_add(salt.wrapping_mul(0x9e3779b97f4a7c15));
    value = (value ^ (value >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
    value = (value ^ (value >> 27)).wrapping_mul(0x94d049bb133111eb);
    ((value ^ (value >> 31)) >> 40) as f32 / (1_u32 << 24) as f32
}

fn seed_at(point: Vec2, side: i32) -> u64 {
    let x = (point.x * 16.0).round() as i32 as u32 as u64;
    let z = (point.y * 16.0).round() as i32 as u32 as u64;
    (x << 32)
        ^ z
        ^ if side < 0 {
            0x728cf93d89c173a5
        } else {
            0xb982c78d102ac987
        }
}

/// Only the part of a long segment near this chunk is sampled. Candidates are
/// world-seeded and sorted independently of entity/query/streaming order.
pub(super) fn candidates(coord: ChunkCoord, segments: &[RoadSegment]) -> Vec<Candidate> {
    let minimum = coord.world_pos().xz() - Vec2::splat(ROADSIDE_BAND);
    let maximum = minimum + Vec2::splat(CHUNK_SIZE + ROADSIDE_BAND * 2.0);
    let corners = [
        minimum,
        Vec2::new(minimum.x, maximum.y),
        maximum,
        Vec2::new(maximum.x, minimum.y),
    ];
    let mut ordered = segments
        .iter()
        .filter(|segment| segment.built)
        .collect::<Vec<_>>();
    ordered.sort_unstable_by(|a, b| {
        a.start
            .x
            .total_cmp(&b.start.x)
            .then(a.start.y.total_cmp(&b.start.y))
            .then(a.end.x.total_cmp(&b.end.x))
            .then(a.end.y.total_cmp(&b.end.y))
            .then(a.width.total_cmp(&b.width))
            .then(a.reserved_width.total_cmp(&b.reserved_width))
            .then(a.distance_before.total_cmp(&b.distance_before))
    });
    let mut output = Vec::new();
    'segments: for segment in ordered {
        let delta = segment.end - segment.start;
        let length = delta.length();
        if length < 0.001 {
            continue;
        }
        let direction = delta / length;
        let normal = Vec2::new(-direction.y, direction.x);
        let projected = corners.map(|p| (p - segment.start).dot(direction));
        let from = projected.into_iter().fold(f32::INFINITY, f32::min).max(0.0);
        let to = projected
            .into_iter()
            .fold(f32::NEG_INFINITY, f32::max)
            .min(length);
        if from > to {
            continue;
        }
        let spacing = 7.5;
        let first = ((segment.distance_before + from) / spacing).ceil() as u32;
        let last = ((segment.distance_before + to) / spacing).floor() as u32;
        for step in first..=last {
            let distance = step as f32 * spacing - segment.distance_before;
            // The next segment owns the shared endpoint, avoiding double clusters.
            if distance >= length - 0.001 {
                continue;
            }
            let center = segment.start + direction * distance;
            for side in [-1, 1] {
                let seed = seed_at(center, side);
                let choice = roll(seed, 1);
                let kind = if choice < 0.23 {
                    DetailKind::Bush
                } else if choice < 0.69 {
                    DetailKind::Flowers
                } else if choice < 0.84 {
                    DetailKind::Stone
                } else {
                    continue;
                };
                let radius = match kind {
                    DetailKind::Bush => 1.15 + roll(seed, 2) * 0.55,
                    DetailKind::Flowers => 1.10 + roll(seed, 2) * 0.55,
                    DetailKind::Stone => 0.32 + roll(seed, 2) * 0.25,
                };
                let offset = match kind {
                    DetailKind::Bush => {
                        segment.reserved_width * 0.5 + radius + 0.72 + roll(seed, 3) * 1.2
                    }
                    DetailKind::Flowers => {
                        segment.width * 0.5 + radius + 0.67 + roll(seed, 3) * 1.1
                    }
                    DetailKind::Stone => segment.width * 0.5 + radius + 0.14 + roll(seed, 3) * 0.45,
                };
                let point = center + normal * offset * side as f32;
                if ChunkCoord::from_world_pos(Vec3::new(point.x, 0.0, point.y)) != coord {
                    continue;
                }
                output.push(Candidate {
                    point,
                    radius,
                    seed,
                    kind,
                });
                if kind != DetailKind::Stone {
                    // Offer a few nearby alternatives for the SAME patch. Dense
                    // frontage often blocks its first shoulder position; wider
                    // random scattering either misses the village or fills every
                    // gap. The chunk builder accepts this seed at most once.
                    for attempt in 1..=2 {
                        let along =
                            direction * (if side < 0 { -1.0 } else { 1.0 }) * attempt as f32 * 1.35;
                        let alternative =
                            point + along + normal * side as f32 * attempt as f32 * 1.15;
                        if ChunkCoord::from_world_pos(Vec3::new(alternative.x, 0.0, alternative.y))
                            == coord
                        {
                            output.push(Candidate {
                                point: alternative,
                                radius,
                                seed,
                                kind,
                            });
                        }
                    }
                }
                if output.len() >= MAX_CANDIDATES_PER_CHUNK {
                    output.truncate(MAX_CANDIDATES_PER_CHUNK);
                    break 'segments;
                }
            }
        }
    }
    // Stable sorting retains primary-before-alternate preference for one seed.
    output.sort_by_key(|candidate| candidate.seed);
    let mut seen = std::collections::HashSet::new();
    output.retain(|candidate| {
        seen.insert((
            candidate.seed,
            candidate.point.x.to_bits(),
            candidate.point.y.to_bits(),
        ))
    });
    output
}

/// Sparse world-cell planting between lanes complements the shoulder clusters.
/// These are low cosmetic flowers/shrubs, never another simulated vegetation
/// population. The existing exact plot/road/ground checks and mesh budgets still
/// decide acceptance. Centres are owned by one chunk, independent of road order.
pub(super) fn meadow_candidates(
    coord: ChunkCoord,
    segments: &[RoadSegment],
    world_seed: u64,
) -> Vec<Candidate> {
    const CELL: f32 = 10.0;
    let minimum = coord.world_pos().xz();
    let maximum = minimum + Vec2::splat(CHUNK_SIZE);
    let lo = (minimum / CELL).floor().as_ivec2();
    let hi = (maximum / CELL).floor().as_ivec2();
    let mut result = Vec::new();
    for x in lo.x..=hi.x {
        for z in lo.y..=hi.y {
            let seed = shared::worldgen::splitmix64(
                ((x as u32 as u64) << 32) ^ z as u32 as u64 ^ world_seed ^ 0x41BD_2F03_89AC_6735,
            );
            if roll(seed, 1) > 0.46 {
                continue;
            }
            let point = Vec2::new(x as f32 + 0.5, z as f32 + 0.5) * CELL
                + Vec2::new(roll(seed, 2) - 0.5, roll(seed, 3) - 0.5) * 6.0;
            if point.x < minimum.x
                || point.y < minimum.y
                || point.x >= maximum.x
                || point.y >= maximum.y
            {
                continue;
            }
            let edge_distance = segments
                .iter()
                .filter(|s| s.built)
                .map(|s| {
                    distance_squared_to_segment(point, s.start, s.end).sqrt()
                        - s.reserved_width * 0.5
                })
                .fold(f32::INFINITY, f32::min);
            if !(8.0..=20.0).contains(&edge_distance) {
                continue;
            }
            let kind = if roll(seed, 4) < 0.28 {
                DetailKind::Bush
            } else {
                DetailKind::Flowers
            };
            result.push(Candidate {
                point,
                radius: 1.15 + roll(seed, 5) * 0.45,
                seed,
                kind,
            });
        }
    }
    result.sort_by_key(|c| c.seed);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segment(start: Vec2, end: Vec2) -> RoadSegment {
        RoadSegment {
            owner: Entity::from_bits(1),
            start,
            end,
            distance_before: 0.0,
            width: 2.6,
            reserved_width: 4.0,
            built: true,
        }
    }

    fn slanted_field() -> FarmFieldShape {
        use shared::components::FarmFieldSection;
        FarmFieldShape {
            sections: vec![
                FarmFieldSection {
                    z: 0.,
                    left: 0.,
                    right: 10.,
                },
                FarmFieldSection {
                    z: 4.,
                    left: 12.,
                    right: 22.,
                },
            ],
        }
    }

    #[test]
    fn slanted_crop_edges_use_perpendicular_plant_clearance() {
        let shape = slanted_field();
        let local = Vec2::new(2.8, 2.);
        let radius = 1.5;
        assert!(
            !shape.contains_local_point(local, radius + 0.8),
            "the old row-width inflation misses this overlapping plant"
        );
        let plot = PlotFootprint {
            origin: Vec3::ZERO,
            yaw: 0.,
            shape: PlotShape::Field(shape),
        };
        assert!(plot.contains(local, radius));
        assert!(!plot.contains(Vec2::new(-4., 2.), radius));
        // Rounded end-cap clearance also keeps plants out of field corners.
        assert!(plot.contains(Vec2::new(-1., -1.), radius));
        assert!(!plot.contains(Vec2::new(-2., -2.), radius));
    }

    #[test]
    fn rotated_field_exclusions_stay_inside_the_chunk_broad_phase() {
        let shape = slanted_field();
        let radius = 1.7; // largest supported roadside plant footprint
        for at in [Vec3::new(63.8, 3., 63.8), Vec3::new(-64.2, 3., -0.2)] {
            for turn in 0..16 {
                let yaw = turn as f32 * std::f32::consts::TAU / 16.;
                let plot = PlotFootprint {
                    origin: at,
                    yaw,
                    shape: PlotShape::Field(shape.clone()),
                };
                let chunks = plot.chunks();
                let boundary = shape.boundary_points();
                for (a, b) in boundary
                    .iter()
                    .zip(boundary.iter().cycle().skip(1))
                    .take(boundary.len())
                {
                    for step in 0..=4 {
                        for angle in 0..16 {
                            let angle = angle as f32 * std::f32::consts::TAU / 16.;
                            let local = a.lerp(*b, step as f32 / 4.)
                                + Vec2::new(angle.cos(), angle.sin()) * (radius + 0.79);
                            let point = at.xz() + shared::rotation::local_to_world_xz(local, yaw);
                            assert!(plot.contains(point, radius));
                            assert!(
                                chunks.contains(&ChunkCoord::from_world_pos(Vec3::new(
                                    point.x, 0., point.y
                                ))),
                                "exact exclusion extends outside its broad phase at {point}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn explicit_empty_fields_do_not_fall_back_to_legacy_crop_land() {
        let mut field = FarmField {
            settlement: "Test".into(),
            farmstead: Vec3::ZERO,
            plot_index: 0,
            quality: 1.,
            shape: None,
            layout_version: 0,
        };
        let legacy = PlotFootprint::from_field(&field, Vec3::ZERO, 0.);
        assert!(legacy.contains(Vec2::ZERO, 1.5));
        assert!(!legacy.chunks().is_empty());
        field.shape = Some(FarmFieldShape::default());
        let empty = PlotFootprint::from_field(&field, Vec3::ZERO, 0.);
        assert!(!empty.contains(Vec2::ZERO, 1.5));
        assert!(empty.chunks().is_empty());
    }

    #[test]
    fn sampling_is_order_independent_bounded_and_chunk_owned() {
        let a = segment(Vec2::new(-4000.0, 18.0), Vec2::new(4000.0, 18.0));
        let b = segment(Vec2::new(11.0, -100.0), Vec2::new(11.0, 200.0));
        let coord = ChunkCoord::new(0, 0);
        let found = candidates(coord, &[a, b]);
        assert_eq!(found, candidates(coord, &[b, a]));
        assert!(!found.is_empty() && found.len() < MAX_CANDIDATES_PER_CHUNK);
        assert!(found.iter().all(|c| c.point.x >= 0.0
            && c.point.x < CHUNK_SIZE
            && c.point.y >= 0.0
            && c.point.y < CHUNK_SIZE));
    }

    #[test]
    fn meadow_planting_is_seeded_chunk_owned_and_follows_only_built_lanes() {
        let road = segment(Vec2::new(-80., 31.), Vec2::new(150., 31.));
        let coord = ChunkCoord::new(0, 0);
        let first = meadow_candidates(coord, &[road], 91);
        assert!(first.len() >= 4 && first.len() <= 64);
        assert_eq!(first, meadow_candidates(coord, &[road, road], 91));
        assert_ne!(first, meadow_candidates(coord, &[road], 92));
        assert!(meadow_candidates(
            coord,
            &[RoadSegment {
                built: false,
                ..road
            }],
            91
        )
        .is_empty());
        let neighbour = meadow_candidates(ChunkCoord::new(1, 0), &[road], 91);
        for c in &first {
            assert_eq!(
                ChunkCoord::from_world_pos(Vec3::new(c.point.x, 0., c.point.y)),
                coord
            );
            assert!(road.chunks().any(|chunk| chunk == coord));
            assert!(!neighbour.iter().any(|n| n.seed == c.seed));
            assert!(c.clear_of_road(&road));
            assert!((8.0..=20.0).contains(&((c.point.y - 31.).abs() - 2.0)));
        }
    }

    #[test]
    fn diagonal_road_index_is_linear_and_covers_the_full_shoulder() {
        let road = segment(
            Vec2::splat(-CHUNK_SIZE * 50.0),
            Vec2::splat(CHUNK_SIZE * 50.0),
        );
        let chunks = road.chunks().collect::<std::collections::HashSet<_>>();
        // An AABB would contain >10,000 chunks for this 100-column road.
        assert!(chunks.len() < 500);
        let reverse = RoadSegment {
            start: road.end,
            end: road.start,
            ..road
        };
        assert_eq!(chunks, reverse.chunks().collect());
        let padding = road.reserved_width * 0.5 + ROADSIDE_BAND;
        for sample in 0..=800 {
            let center = road.start.lerp(road.end, sample as f32 / 800.0);
            for offset in [Vec2::ZERO, Vec2::ONE, -Vec2::ONE, Vec2::new(1.0, -1.0)] {
                let point = center + offset * padding;
                assert!(chunks.contains(&ChunkCoord::from_world_pos(Vec3::new(
                    point.x, 0.0, point.y
                ))));
            }
        }
    }

    #[test]
    fn crossing_and_future_road_reservations_stay_clear() {
        let road = segment(Vec2::new(-20.0, 0.0), Vec2::new(20.0, 0.0));
        let flower = Candidate {
            point: Vec2::new(0.0, 2.4),
            radius: 0.5,
            seed: 1,
            kind: DetailKind::Flowers,
        };
        assert!(flower.clear_of_road(&road));
        assert!(!flower.clear_of_road(&RoadSegment {
            reserved_width: 6.0,
            built: false,
            ..road
        }));
        assert!(!flower.clear_of_road(&segment(Vec2::new(0.0, -10.0), Vec2::new(0.0, 10.0))));
        assert!(!Candidate {
            kind: DetailKind::Bush,
            ..flower
        }
        .clear_of_road(&road));
    }

    #[test]
    fn blocked_patch_alternatives_share_identity_and_stay_in_the_primary_chunk() {
        let road = segment(Vec2::new(1.0, 18.0), Vec2::new(63.0, 18.0));
        let coord = ChunkCoord::new(0, 0);
        let found = candidates(coord, &[road, road]);
        let mut counts = std::collections::HashMap::new();
        for candidate in &found {
            *counts.entry(candidate.seed).or_insert(0) += 1;
            assert_eq!(
                ChunkCoord::from_world_pos(Vec3::new(candidate.point.x, 0.0, candidate.point.y)),
                coord
            );
        }
        assert!(counts.values().any(|count| *count > 1));
        assert!(counts.values().all(|count| *count <= 3));
        assert_eq!(found, candidates(coord, &[road]));
    }

    #[test]
    fn rotated_crop_footprint_reserves_worker_margin() {
        let plot = PlotFootprint {
            origin: Vec3::new(30.0, 0.0, 20.0),
            yaw: 0.7,
            shape: PlotShape::Field(FarmFieldShape::legacy_rectangle()),
        };
        let point =
            plot.origin.xz() + shared::rotation::local_to_world_xz(Vec2::new(4.7, 0.0), plot.yaw);
        assert!(plot.contains(point, 0.2));
        assert!(!plot.contains(Vec2::new(50.0, 50.0), 0.2));
    }
}
