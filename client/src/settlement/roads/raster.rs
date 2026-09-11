//! Coverage-union rasterization: intersections widen naturally without piling
//! opaque brush stamps on top of one another and hardening the shoulders.

use super::*;

/// A built square remains a planned common, but its paving ends in a chipped
/// perimeter and slightly rounded corners instead of a perfect stamped slab.
pub(super) fn paint_square(op: &TerrainPaintOp, chunk_min: Vec2, map: &mut WeightMapData) {
    let TerrainPaintShape::Rect {
        center,
        half_extents,
        rotation,
    } = op.shape
    else {
        return;
    };
    let (sin, cos) = rotation.sin_cos();
    let smaller_half = half_extents.min_element();
    let scale = (smaller_half / 8.0).min(1.0);
    let radius =
        (2.0 + road_wear(center * 0.067 + Vec2::new(7.3, -2.8)) * 0.4).min(smaller_half * 0.35);
    let paving = op.layer == TerrainLayer::Cobblestone;
    let feather = if paving {
        op.falloff.clamp(0.18, 0.34)
    } else {
        op.falloff.clamp(0.24, 0.42)
    } * scale;
    for z in 0..map.resolution {
        for x in 0..map.resolution {
            let point = map.sample_position(chunk_min, x, z);
            let rel = point - center;
            let local = Vec2::new(rel.x * cos + rel.y * sin, -rel.x * sin + rel.y * cos);
            if local.abs().cmpge(half_extents).any() {
                continue;
            }
            let q = local.abs() - half_extents + Vec2::splat(radius);
            let distance = q.max(Vec2::ZERO).length() + q.max_element().min(0.0) - radius;
            // Broad missing runs make the old paving edge visible at town
            // distance; smaller chips keep it from looking like a wavy slab.
            // All variation is world-space, so adjacent chunks agree exactly.
            let broad = road_wear(point * 0.16 + Vec2::new(9.1, 4.7)) * 0.8
                + road_wear(point * 0.055 + Vec2::new(-6.2, 17.3)) * 0.2;
            let chips = road_wear(point * 1.15 + Vec2::new(3.4, -8.6));
            let recession = if paving {
                0.25 + broad * 0.80 + chips * 0.16
            } else {
                // The dirt bed ends closer to the boundary than the cobbles,
                // exposing a narrow dusty/gravelly transition inside the lot.
                0.04 + broad * 0.16 + chips * 0.05
            } * scale;
            // Both soft bands recede inward. The square's accepted rectangle,
            // interior market pad and navigation shape remain authoritative.
            let amount = (1.0 - smooth(-feather, 0.0, distance + recession)) * op.strength;
            blend(
                &mut map.weights[(z * map.resolution + x) as usize],
                op.layer.index(),
                amount,
            );
        }
    }
}

pub(super) fn paint_road_and_yard_segments(
    chunk_min: Vec2,
    map: &mut WeightMapData,
    segments: &[RoadSegmentRef],
    yard_paths: &[&YardPathPaintSnapshot],
) {
    let mut dirt = vec![0.0_f32; map.weights.len()];
    let mut stone = vec![0.0_f32; map.weights.len()];
    let step = map.sample_step();
    for segment in segments {
        let road = segment.road;
        let dense_index = segment.dense_index;
        let pair = &road.points[dense_index..=dense_index + 1];
        let source_index = road.source_segments[dense_index];
        let source_start = road.surveyed_points[source_index];
        let source_end = road.surveyed_points[source_index + 1];
        let (start, end) = (pair[0], pair[1]);
        let delta = end - start;
        let length_sq = delta.length_squared();
        if length_sq < 0.0001 {
            continue;
        }
        let padding = road.reserved_width * 0.5 + 1.6;
        let minimum = start.min(end) - Vec2::splat(padding);
        let maximum = start.max(end) + Vec2::splat(padding);
        if maximum.x < chunk_min.x
            || maximum.y < chunk_min.y
            || minimum.x > chunk_min.x + CHUNK_SIZE
            || minimum.y > chunk_min.y + CHUNK_SIZE
        {
            continue;
        }
        let min = ((minimum - chunk_min) / step).floor().max(Vec2::ZERO);
        let max = ((maximum - chunk_min) / step)
            .ceil()
            .min(Vec2::splat(map.resolution as f32 - 1.0));
        for z in min.y as u32..=max.y as u32 {
            for x in min.x as u32..=max.x as u32 {
                let point = map.sample_position(chunk_min, x, z);
                let t = ((point - start).dot(delta) / length_sq).clamp(0.0, 1.0);
                let nearest = start + delta * t;
                let distance = point.distance(nearest);
                let survey_distance = shared::components::distance_squared_to_segment(
                    point,
                    source_start,
                    source_end,
                )
                .sqrt();
                let corridor = 1.0
                    - smooth(
                        road.reserved_width * 0.5 - 0.28,
                        road.reserved_width * 0.5,
                        survey_distance,
                    );
                if corridor <= 0.0 {
                    continue;
                }
                let approach = if road.class == RoadClass::Lane && road.surface == RoadSurface::Dirt
                {
                    // Household door approaches read as smaller paths before
                    // opening onto the communal lane; never cut the anchor.
                    0.72 + 0.28 * smooth(0.8, 5.5, nearest.distance(road.points[0]))
                } else {
                    1.0
                };
                let radius = (road.width * width_variation(nearest) * approach * 0.5)
                    .min(road.reserved_width * 0.5 - 0.15);
                let edge = (road_wear(point * 0.72 + Vec2::new(3.2, 12.6)) - 0.5) * 0.44;
                let signed = distance - radius - edge;
                let index = (z * map.resolution + x) as usize;
                match road.surface {
                    RoadSurface::Dirt => {
                        // Worn centre, broken turf edge and a faint outer
                        // shoulder. The transition straddles the lane edge.
                        let cover = (1.0 - smooth(-0.34, 0.58, signed)) * DIRT_STRENGTH;
                        dirt[index] = dirt[index].max(cover * corridor);
                    }
                    RoadSurface::Stone => {
                        let bed = (1.0 - smooth(0.60, 1.60, signed)) * STONE_DIRT_SHOULDER_STRENGTH;
                        let paving =
                            (1.0 - smooth(0.0, STONE_FALLOFF_METERS, signed)) * STONE_STRENGTH;
                        dirt[index] = dirt[index].max(bed * corridor);
                        stone[index] = stone[index].max(paving * corridor);
                    }
                }
            }
        }
    }
    for path in yard_paths {
        path.rasterize(chunk_min, map, &mut dirt);
    }
    for ((weights, dirt), stone) in map.weights.iter_mut().zip(dirt).zip(stone) {
        blend(weights, 1, dirt);
        blend(weights, 3, stone);
    }
}

#[cfg(test)]
fn paint_road_segments(chunk_min: Vec2, map: &mut WeightMapData, segments: &[RoadSegmentRef]) {
    paint_road_and_yard_segments(chunk_min, map, segments, &[]);
}

#[cfg(test)]
fn paint_road_network(
    chunk_min: Vec2,
    map: &mut WeightMapData,
    roads: &[(Entity, &RoadPaintSnapshot)],
) {
    let segments: Vec<_> = roads
        .iter()
        .flat_map(|(_, road)| {
            (0..road.points.len().saturating_sub(1))
                .map(move |dense_index| RoadSegmentRef { road, dense_index })
        })
        .collect();
    paint_road_segments(chunk_min, map, &segments);
}

pub(super) fn smooth(low: f32, high: f32, value: f32) -> f32 {
    let t = ((value - low) / (high - low)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn blend(weights: &mut [u8; 4], channel: usize, amount: f32) {
    if amount <= 0.0 {
        return;
    }
    let mut values = weights.map(|value| value as f32 * (1.0 - amount));
    values[channel] += 255.0 * amount;
    let mut bytes = values.map(|value| value.round() as u8);
    let total: i32 = bytes.iter().map(|value| *value as i32).sum();
    let largest = values
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1))
        .unwrap()
        .0;
    bytes[largest] = (i32::from(bytes[largest]) + 255 - total).clamp(0, 255) as u8;
    *weights = bytes;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn road(surface: RoadSurface, points: Vec<Vec2>) -> VillageRoad {
        VillageRoad {
            settlement: "Border test".into(),
            builder: "Builder".into(),
            built_through: points.len() as u16,
            points,
            width: 2.6,
            reserved_width: 4.0,
            class: RoadClass::Lane,
            surface,
            stone_committed: 0,
        }
    }

    fn endpoint_map() -> WeightMapData {
        let mut images = Assets::<Image>::default();
        let mut map = crate::terrain::paint::build_weightmap_from_weights(
            vec![[255, 0, 0, 0]; 128 * 128],
            128,
            &mut images,
        );
        map.endpoint_samples = true;
        map
    }

    fn paved_square(center: Vec2, half_extents: Vec2, rotation: f32) -> [TerrainPaintOp; 2] {
        [
            TerrainPaintOp {
                id: 1,
                layer: TerrainLayer::Dirt,
                strength: STONE_DIRT_SHOULDER_STRENGTH,
                falloff: STONE_DIRT_SHOULDER_FALLOFF_METERS,
                shape: TerrainPaintShape::Rect {
                    center,
                    half_extents,
                    rotation,
                },
            },
            TerrainPaintOp {
                id: 2,
                layer: TerrainLayer::Cobblestone,
                strength: STONE_STRENGTH,
                falloff: STONE_FALLOFF_METERS,
                shape: TerrainPaintShape::Rect {
                    center,
                    half_extents,
                    rotation,
                },
            },
        ]
    }

    #[test]
    fn eroded_squares_keep_their_filled_centres_and_never_paint_outside_accepted_land() {
        let center = Vec2::splat(32.0);
        for half_extents in [Vec2::new(2.0, 3.5), Vec2::new(20.0, 16.0)] {
            for rotation in [0.0, 0.37, -0.8] {
                let mut map = endpoint_map();
                for op in paved_square(center, half_extents, rotation) {
                    paint_square(&op, Vec2::ZERO, &mut map);
                }
                let mut filled = 0;
                let mut dusty_edge = 0;
                for z in 0..map.resolution {
                    for x in 0..map.resolution {
                        let point = map.sample_position(Vec2::ZERO, x, z);
                        let local = shared::rotation::world_to_local_xz(point - center, -rotation);
                        let weights = map.weights[(z * map.resolution + x) as usize];
                        if local.abs().cmpge(half_extents).any() {
                            assert_eq!(weights, [255, 0, 0, 0], "escaped square at {point:?}");
                        } else if local.abs().cmple(half_extents * 0.5).all() {
                            assert!(weights[3] > 240, "market pad lost its paving at {point:?}");
                            filled += 1;
                        } else if weights[1] > 30 && weights[1] > weights[3] {
                            dusty_edge += 1;
                        }
                        assert_eq!(weights.iter().map(|w| u32::from(*w)).sum::<u32>(), 255);
                    }
                }
                assert!(filled > 0);
                assert!(
                    dusty_edge > 0,
                    "the eroded paving must expose its inner dirt bed"
                );
            }
        }
    }

    #[test]
    fn worn_square_edges_match_exactly_across_both_chunk_axes() {
        let ops = paved_square(Vec2::ZERO, Vec2::new(28.0, 18.0), 0.37);
        let paint = |minimum| {
            let mut map = endpoint_map();
            for op in &ops {
                paint_square(op, minimum, &mut map);
            }
            map
        };
        let southwest = paint(Vec2::new(-64.0, -64.0));
        let southeast = paint(Vec2::new(0.0, -64.0));
        let northwest = paint(Vec2::new(-64.0, 0.0));
        let northeast = paint(Vec2::ZERO);
        for i in 0..128 {
            assert_eq!(southwest.weights[i * 128 + 127], southeast.weights[i * 128]);
            assert_eq!(northwest.weights[i * 128 + 127], northeast.weights[i * 128]);
            assert_eq!(southwest.weights[127 * 128 + i], northwest.weights[i]);
            assert_eq!(southeast.weights[127 * 128 + i], northeast.weights[i]);
        }
        assert!(
            (0..128).any(|i| {
                let w = northeast.weights[i * 128];
                w[1] > 30 && w[3] < 230
            }),
            "the shared border must cross the eroded transition, not just uniform ground"
        );
    }

    #[test]
    fn inclusive_border_coverage_matches_for_parallel_and_crossing_roads() {
        let horizontal = RoadPaintSnapshot::from_road(&road(
            RoadSurface::Dirt,
            vec![Vec2::new(-8., 62.75), Vec2::new(136., 62.75)],
        ));
        let crossing = RoadPaintSnapshot::from_road(&road(
            RoadSurface::Stone,
            vec![Vec2::new(14., 42.), Vec2::new(53., 91.)],
        ));
        let roads = [
            (Entity::from_bits(1), &horizontal),
            (Entity::from_bits(2), &crossing),
        ];
        let mut south = endpoint_map();
        let mut north = endpoint_map();
        paint_road_network(Vec2::ZERO, &mut south, &roads);
        paint_road_network(Vec2::new(0., 64.), &mut north, &roads);
        for x in 0..128 {
            assert_eq!(
                south.sample_position(Vec2::ZERO, x, 127),
                north.sample_position(Vec2::new(0., 64.), x, 0)
            );
            assert_eq!(
                south.weights[127 * 128 + x as usize],
                north.weights[x as usize],
                "road coverage jumps across the shared z=64 border at x={x}"
            );
        }
        assert!(
            north.weights[..128].iter().any(|w| w[1] > 30 && w[1] < 230),
            "the border must cut a soft shoulder, not just uniform meadow"
        );
        // The same contract holds for a vertical border and negative chunks.
        let shifted = RoadPaintSnapshot::from_road(&road(
            RoadSurface::Dirt,
            vec![Vec2::new(-1.25, -72.), Vec2::new(-1.25, 72.)],
        ));
        let roads = [(Entity::from_bits(3), &shifted)];
        let mut west = endpoint_map();
        let mut east = endpoint_map();
        paint_road_network(Vec2::new(-64., -64.), &mut west, &roads);
        paint_road_network(Vec2::new(0., -64.), &mut east, &roads);
        for z in 0..128 {
            assert_eq!(west.weights[z * 128 + 127], east.weights[z * 128]);
        }
    }

    #[test]
    fn mixed_surfaces_are_order_independent_and_never_paint_outside_their_reservations() {
        let dirt = road(
            RoadSurface::Dirt,
            vec![Vec2::new(5., 24.), Vec2::new(30., 27.), Vec2::new(54., 12.)],
        );
        let stone = road(
            RoadSurface::Stone,
            vec![Vec2::new(25., 6.), Vec2::new(32., 42.), Vec2::new(56., 42.)],
        );
        let a = RoadPaintSnapshot::from_road(&dirt);
        let b = RoadPaintSnapshot::from_road(&stone);
        let mut first = endpoint_map();
        let mut second = endpoint_map();
        paint_road_network(
            Vec2::ZERO,
            &mut first,
            &[(Entity::from_bits(3), &a), (Entity::from_bits(9), &b)],
        );
        paint_road_network(
            Vec2::ZERO,
            &mut second,
            &[(Entity::from_bits(9), &b), (Entity::from_bits(3), &a)],
        );
        assert_eq!(first.weights, second.weights);
        for z in 0..128 {
            for x in 0..128 {
                let point = first.sample_position(Vec2::ZERO, x, z);
                let weights = first.weights[(z * 128 + x) as usize];
                if !dirt.contains_reserved_point(point, 0.)
                    && !stone.contains_reserved_point(point, 0.)
                {
                    assert_eq!(
                        weights,
                        [255, 0, 0, 0],
                        "surface/shoulder escaped accepted road land at {point:?}"
                    );
                }
                assert_eq!(weights.iter().map(|w| u32::from(*w)).sum::<u32>(), 255);
            }
        }
    }

    #[test]
    fn sharp_narrow_lane_keeps_a_continuous_painted_core_through_the_curve() {
        for surface in [RoadSurface::Dirt, RoadSurface::Stone] {
            for angle in [0.7_f32, std::f32::consts::FRAC_PI_2, 2.5] {
                let joint = Vec2::splat(32.0);
                let input = road(
                    surface,
                    vec![
                        joint - Vec2::X * 23.0,
                        joint,
                        joint + Vec2::new(angle.cos(), angle.sin()) * 23.0,
                    ],
                );
                let snapshot = RoadPaintSnapshot::from_road(&input);
                let mut map = endpoint_map();
                paint_road_network(Vec2::ZERO, &mut map, &[(Entity::from_bits(1), &snapshot)]);
                // The core remains a visible walking strip through both the
                // exact junction and the most displaced part of either curve.
                // Sample more finely than the half-metre weightmap spacing.
                for pair in snapshot.points.windows(2) {
                    let direction = (pair[1] - pair[0]).normalize_or_zero();
                    let normal = Vec2::new(-direction.y, direction.x);
                    for along in 0..=4 {
                        for across in [-0.25, 0.0, 0.25] {
                            let point = pair[0].lerp(pair[1], along as f32 / 4.0) + normal * across;
                            let pixel = (point / map.sample_step()).round().as_uvec2();
                            let weights =
                                map.weights[(pixel.y * map.resolution + pixel.x) as usize];
                            let coverage = u16::from(weights[1]) + u16::from(weights[3]);
                            assert!(
                                coverage > 128,
                                "lane core vanished at {point:?}: {weights:?}, {surface:?}, turn={angle}"
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn overlapping_roads_do_not_harden_a_shoulder_or_depend_on_entity_order() {
        let mut images = Assets::<Image>::default();
        let mut map = crate::terrain::paint::build_weightmap_from_weights(
            vec![[255, 0, 0, 0]; 128 * 128],
            128,
            &mut images,
        );
        let road = RoadPaintSnapshot {
            points: vec![Vec2::new(8.0, 32.0), Vec2::new(56.0, 32.0)],
            surveyed_points: vec![Vec2::new(8.0, 32.0), Vec2::new(56.0, 32.0)],
            source_segments: vec![0],
            width: 2.6,
            reserved_width: 4.0,
            class: RoadClass::Lane,
            surface: RoadSurface::Dirt,
        };
        paint_road_network(Vec2::ZERO, &mut map, &[(Entity::from_bits(1), &road)]);
        let once = map.weights.clone();
        map.weights.fill([255, 0, 0, 0]);
        paint_road_network(
            Vec2::ZERO,
            &mut map,
            &[(Entity::from_bits(9), &road), (Entity::from_bits(1), &road)],
        );
        assert_eq!(map.weights, once);
        assert!(map.weights.iter().any(|p| p[1] > 30 && p[1] < 200));
        assert!(map
            .weights
            .iter()
            .all(|p| p.iter().map(|v| *v as u32).sum::<u32>() == 255));
    }
}
