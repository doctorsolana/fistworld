//! Worn presentation geometry inside a road's surveyed right-of-way.

use bevy::prelude::*;
use shared::components::VillageRoad;

/// Use the entire planned route to derive tangents, then emit only its built
/// prefix. Finishing the next segment never rerolls the already worn ground.
/// Every surveyed anchor remains on the ribbon, including graph junctions.
pub(super) fn worn_road_points(road: &VillageRoad) -> (Vec<Vec2>, Vec<usize>) {
    let points = &road.points;
    let count = road.built_points().len();
    let mut result = Vec::with_capacity(count * 6);
    let mut sources = Vec::with_capacity(count * 6);
    if count == 0 {
        return (result, sources);
    }
    result.push(points[0]);
    // Leave some shoulder room before the raster's final survey-capsule fade.
    // Legacy roads have no spare claimed land and keep their original centreline.
    let margin = (((road.reservation_width() - road.width) * 0.5).min(0.90) - 0.15).max(0.0);
    for index in 0..count.saturating_sub(1) {
        let start = points[index];
        let end = points[index + 1];
        let delta = end - start;
        let length = delta.length();
        if length < 0.001 {
            if start != end {
                result.push(end);
                sources.push(index);
            }
            continue;
        }
        let tangent = |at: usize| {
            let before = at.checked_sub(1).map(|i| points[at] - points[i]);
            let after = points.get(at + 1).map(|p| *p - points[at]);
            (before.unwrap_or(delta).normalize_or_zero()
                + after.unwrap_or(delta).normalize_or_zero())
            .normalize_or_zero()
        };
        let begin = tangent(index) * length;
        let finish = tangent(index + 1) * length;
        let steps = length.ceil().max(1.0) as usize;
        for step in 1..steps {
            let t = step as f32 / steps as f32;
            result.push(worn_segment_point(start, delta, begin, finish, margin, t));
            sources.push(index);
        }
        result.push(end);
        sources.push(index);
    }
    (result, sources)
}

fn worn_segment_point(
    start: Vec2,
    delta: Vec2,
    begin: Vec2,
    finish: Vec2,
    margin: f32,
    t: f32,
) -> Vec2 {
    if margin <= 0.0 {
        return start + delta * t;
    }
    let length = delta.length();
    let direction = delta / length;
    let normal = Vec2::new(-direction.y, direction.x);
    let t2 = t * t;
    let t3 = t2 * t;
    // Relative coordinates avoid cancellation at distant world coordinates.
    // Keep the Hermite's longitudinal component: replacing it with t*length
    // changes the tangent and leaves a visible kink at surveyed corners.
    let curve = delta * (-2.0 * t3 + 3.0 * t2) + begin * (t3 - 2.0 * t2 + t) + finish * (t3 - t2);
    let along = curve.dot(direction).clamp(0.0, length);
    let straight = start + direction * along;
    // Zero drift slope at either anchor preserves the shared curve tangent.
    let envelope = ((t * length).min((1.0 - t) * length) * 0.5).clamp(0.0, 1.0);
    let envelope = envelope * envelope * (3.0 - 2.0 * envelope);
    let drift = (road_wear(straight * 0.085) - 0.5) * 0.40 * envelope;
    // Smooth saturation avoids the flat offset plateaux and sudden curvature
    // changes produced by a hard clamp. Final paint still clips to the survey.
    let bend = margin * ((curve.dot(normal) + drift) / margin).tanh();
    straight + normal * bend
}

pub(super) fn width_variation(point: Vec2) -> f32 {
    // Long constrictions and broader passing places, with smaller local wear.
    // Slow spatial variation avoids a scalloped row of circular brush stamps.
    0.76 + road_wear(point * 0.075 + Vec2::new(17.2, -9.4)) * 0.42
        + (road_wear(point * 0.37) - 0.5) * 0.08
}

/// Smooth value noise, shared with low-cost verge dressing. No entity ID,
/// streaming order, camera or frame time enters this field.
pub(in crate::settlement) fn road_wear(point: Vec2) -> f32 {
    let cell = point.floor();
    let t = point - cell;
    let t = t * t * (Vec2::splat(3.0) - 2.0 * t);
    let hash = |x: f32, y: f32| {
        let mut h =
            (x as i32 as u32).wrapping_mul(0x9e3779b9) ^ (y as i32 as u32).wrapping_mul(0x85ebca6b);
        h ^= h >> 16;
        h = h.wrapping_mul(0x7feb352d);
        h ^= h >> 15;
        (h & 0xffff) as f32 / 65535.0
    };
    hash(cell.x, cell.y)
        .lerp(hash(cell.x + 1.0, cell.y), t.x)
        .lerp(
            hash(cell.x, cell.y + 1.0).lerp(hash(cell.x + 1.0, cell.y + 1.0), t.x),
            t.y,
        )
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::components::{RoadClass, RoadSurface};

    fn lane(points: Vec<Vec2>, reserved_width: f32) -> VillageRoad {
        VillageRoad {
            built_through: points.len() as u16,
            points,
            width: 2.6,
            reserved_width,
            settlement: "A".into(),
            builder: "B".into(),
            surface: RoadSurface::Dirt,
            class: RoadClass::Lane,
            stone_committed: 0,
        }
    }

    #[test]
    fn road_progress_preserves_worn_prefix_and_every_surveyed_junction() {
        let points = vec![
            Vec2::new(-20.0, 2.0),
            Vec2::new(10.0, 2.0),
            Vec2::new(10.0, 24.0),
        ];
        let mut road = lane(points.clone(), 4.0);
        road.built_through = 2;
        let (prefix, prefix_sources) = worn_road_points(&road);
        road.built_through = 3;
        let (full, full_sources) = worn_road_points(&road);
        assert_eq!(&full[..prefix.len()], prefix.as_slice());
        assert_eq!(
            &full_sources[..prefix_sources.len()],
            prefix_sources.as_slice()
        );
        assert_eq!(full_sources.len() + 1, full.len());
        assert!(points.iter().all(|point| full.contains(point)));
        assert!(prefix.iter().any(|p| (p.y - 2.0).abs() > 0.4));
        assert!(prefix.iter().all(|p| (p.y - 2.0).abs() <= 0.551));
    }

    #[test]
    fn curved_sharp_turns_stay_inside_their_own_survey_and_move_forward() {
        for angle in [0.0_f32, 0.4, 1.57, 2.7, std::f32::consts::PI] {
            for reserved in [0.0, 2.6, 2.8, 4.0, 6.0] {
                let pivot = Vec2::new(-70.0, 63.8);
                let road = lane(
                    vec![
                        pivot - Vec2::X * 18.0,
                        pivot,
                        pivot + Vec2::new(angle.cos(), angle.sin()) * 23.0,
                    ],
                    reserved,
                );
                let (points, sources) = worn_road_points(&road);
                assert_eq!(sources.len() + 1, points.len());
                let margin =
                    (((road.reservation_width() - road.width) * 0.5).min(0.9) - 0.15).max(0.0);
                for (pair, source) in points.windows(2).zip(sources) {
                    let start = road.points[source];
                    let end = road.points[source + 1];
                    let delta = end - start;
                    for point in pair {
                        assert!(point.is_finite());
                        assert!(
                            shared::components::distance_squared_to_segment(*point, start, end)
                                <= (margin + 0.0001).powi(2),
                            "escaped source corridor: {point:?}, angle={angle}, reserve={reserved}"
                        );
                        let along = (*point - start).dot(delta) / delta.length_squared();
                        assert!((-0.00001..=1.00001).contains(&along));
                    }
                    assert!((pair[1] - pair[0]).dot(delta) >= -0.0001);
                }
            }
        }
    }

    #[test]
    fn legacy_short_and_duplicate_segments_remain_finite_and_keep_their_anchors() {
        for points in [
            vec![],
            vec![Vec2::ZERO],
            vec![Vec2::ZERO, Vec2::ZERO],
            vec![Vec2::ZERO, Vec2::X * 0.0005, Vec2::new(0.0005, 0.0008)],
            vec![Vec2::ZERO, Vec2::X * 0.05, Vec2::new(0.05, 0.08)],
            vec![
                Vec2::ZERO,
                Vec2::ZERO,
                Vec2::X * 8.0,
                Vec2::X * 8.0,
                Vec2::splat(8.0),
            ],
        ] {
            for reservation in [0.0, 4.0] {
                let road = lane(points.clone(), reservation);
                let (worn, sources) = worn_road_points(&road);
                assert!(worn.iter().all(|p| p.is_finite()));
                assert_eq!(sources.len(), worn.len().saturating_sub(1));
                assert!(points.iter().all(|p| worn.contains(p)));
                for (pair, source) in worn.windows(2).zip(sources) {
                    if reservation == 0.0 {
                        for point in pair {
                            assert!(
                                shared::components::distance_squared_to_segment(
                                    *point,
                                    road.points[source],
                                    road.points[source + 1],
                                ) < 0.000001
                            );
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn endpoint_drift_preserves_the_shared_hermite_tangent() {
        let joint = Vec2::ZERO;
        let tangent = Vec2::ONE.normalize();
        let step = 0.0001;
        let before = worn_segment_point(
            -Vec2::X * 20.0,
            Vec2::X * 20.0,
            Vec2::X * 20.0,
            tangent * 20.0,
            0.55,
            1.0 - step,
        );
        let after = worn_segment_point(
            joint,
            Vec2::Y * 30.0,
            tangent * 30.0,
            Vec2::Y * 30.0,
            0.55,
            step,
        );
        assert!((joint - before).normalize().dot(tangent) > 0.9999);
        assert!((after - joint).normalize().dot(tangent) > 0.9999);
    }
}
