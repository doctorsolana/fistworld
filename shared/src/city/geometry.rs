use std::collections::HashSet;

use bevy::prelude::*;

use crate::terrain::{ChunkCoord, CHUNK_SIZE};

use super::{MapPlot, MapRoad, RoadClass, RoadSide};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OrientedRect {
    pub center: Vec2,
    pub half_extents: Vec2,
    pub rotation_y: f32,
}

impl OrientedRect {
    #[inline]
    pub fn corners(&self) -> [Vec2; 4] {
        let cos_r = self.rotation_y.cos();
        let sin_r = self.rotation_y.sin();
        let axis_x = Vec2::new(cos_r, sin_r);
        let axis_z = Vec2::new(-sin_r, cos_r);
        [
            self.center + axis_x * self.half_extents.x + axis_z * self.half_extents.y,
            self.center - axis_x * self.half_extents.x + axis_z * self.half_extents.y,
            self.center - axis_x * self.half_extents.x - axis_z * self.half_extents.y,
            self.center + axis_x * self.half_extents.x - axis_z * self.half_extents.y,
        ]
    }

    pub fn chunk_bounds(&self) -> (i32, i32, i32, i32) {
        let corners = self.corners();
        let mut min_x = f32::MAX;
        let mut max_x = f32::MIN;
        let mut min_z = f32::MAX;
        let mut max_z = f32::MIN;

        for corner in corners {
            min_x = min_x.min(corner.x);
            max_x = max_x.max(corner.x);
            min_z = min_z.min(corner.y);
            max_z = max_z.max(corner.y);
        }

        (
            (min_x / CHUNK_SIZE).floor() as i32,
            (max_x / CHUNK_SIZE).floor() as i32,
            (min_z / CHUNK_SIZE).floor() as i32,
            (max_z / CHUNK_SIZE).floor() as i32,
        )
    }

    pub fn contains_point(&self, point: Vec2) -> bool {
        let rel = point - self.center;
        let cos_r = self.rotation_y.cos();
        let sin_r = self.rotation_y.sin();
        let local_x = rel.x * cos_r + rel.y * sin_r;
        let local_z = -rel.x * sin_r + rel.y * cos_r;
        local_x.abs() <= self.half_extents.x && local_z.abs() <= self.half_extents.y
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoadSegment {
    pub road_id: u64,
    pub segment_index: usize,
    pub road_class: RoadClass,
    pub start: Vec2,
    pub end: Vec2,
    pub tangent: Vec2,
    pub normal: Vec2,
    pub length: f32,
    pub road_width: f32,
    pub sidewalk_left: bool,
    pub sidewalk_right: bool,
    pub sidewalk_width: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct RoadEndpointVisual {
    pub road_extension: f32,
    pub sidewalk_trim: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RoadRenderSegment {
    pub segment: RoadSegment,
    pub start_visual: RoadEndpointVisual,
    pub end_visual: RoadEndpointVisual,
}

impl RoadSegment {
    #[inline]
    pub fn rotation_y(&self) -> f32 {
        self.tangent.y.atan2(self.tangent.x)
    }

    #[inline]
    pub fn road_rect(&self) -> OrientedRect {
        OrientedRect {
            center: (self.start + self.end) * 0.5,
            half_extents: Vec2::new(self.length * 0.5, self.road_width * 0.5),
            rotation_y: self.rotation_y(),
        }
    }

    pub fn sidewalk_rect(&self, side: RoadSide) -> Option<OrientedRect> {
        self.sidewalk_rect_with_trims(side, 0.0, 0.0)
    }

    pub fn sidewalk_rect_with_trims(
        &self,
        side: RoadSide,
        start_trim: f32,
        end_trim: f32,
    ) -> Option<OrientedRect> {
        let enabled = match side {
            RoadSide::Left => self.sidewalk_left,
            RoadSide::Right => self.sidewalk_right,
        };
        if !enabled || self.sidewalk_width <= 0.0 {
            return None;
        }

        let start_trim = start_trim.max(0.0);
        let end_trim = end_trim.max(0.0);
        let usable_length = self.length - start_trim - end_trim;
        if usable_length <= 0.05 {
            return None;
        }

        let sign = match side {
            RoadSide::Left => 1.0,
            RoadSide::Right => -1.0,
        };
        let offset = sign * (self.road_width * 0.5 + self.sidewalk_width * 0.5);
        let start = self.start + self.tangent * start_trim;
        let end = self.end - self.tangent * end_trim;
        Some(OrientedRect {
            center: (start + end) * 0.5 + self.normal * offset,
            half_extents: Vec2::new(usable_length * 0.5, self.sidewalk_width * 0.5),
            rotation_y: self.rotation_y(),
        })
    }

    pub fn envelope_rect(&self) -> OrientedRect {
        OrientedRect {
            center: (self.start + self.end) * 0.5,
            half_extents: Vec2::new(
                self.length * 0.5,
                self.road_width * 0.5
                    + self.sidewalk_width.max(0.0)
                        * if self.sidewalk_left || self.sidewalk_right {
                            1.0
                        } else {
                            0.0
                        },
            ),
            rotation_y: self.rotation_y(),
        }
    }
}

impl RoadRenderSegment {
    #[inline]
    pub fn sidewalk_rect(&self, side: RoadSide) -> Option<OrientedRect> {
        self.segment.sidewalk_rect_with_trims(
            side,
            self.start_visual.sidewalk_trim,
            self.end_visual.sidewalk_trim,
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct StripSample {
    pub left: Vec2,
    pub right: Vec2,
    pub distance: f32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SegmentProjection {
    pub closest: Vec2,
    pub distance: f32,
    pub t: f32,
}

pub fn build_road_segments(road: &MapRoad) -> Vec<RoadSegment> {
    let mut out = Vec::new();
    for (segment_index, pair) in road.points.windows(2).enumerate() {
        let start = Vec2::new(pair[0][0], pair[0][1]);
        let end = Vec2::new(pair[1][0], pair[1][1]);
        let delta = end - start;
        let length = delta.length();
        if length <= 1e-3 {
            continue;
        }

        let tangent = delta / length;
        let normal = Vec2::new(-tangent.y, tangent.x);
        out.push(RoadSegment {
            road_id: road.id,
            segment_index,
            road_class: road.road_class,
            start,
            end,
            tangent,
            normal,
            length,
            road_width: road.width,
            sidewalk_left: road.sidewalk_left,
            sidewalk_right: road.sidewalk_right,
            sidewalk_width: road.sidewalk_width.max(0.0),
        });
    }
    out
}

pub fn build_road_render_segments(roads: &[MapRoad]) -> Vec<RoadRenderSegment> {
    let mut render_segments = Vec::new();

    for road in roads {
        for segment in build_road_segments(road) {
            render_segments.push(RoadRenderSegment {
                segment,
                start_visual: RoadEndpointVisual::default(),
                end_visual: RoadEndpointVisual::default(),
            });
        }
    }

    let base_segments = render_segments.clone();
    for (index, render_segment) in render_segments.iter_mut().enumerate() {
        render_segment.start_visual = endpoint_visual(
            &base_segments,
            index,
            render_segment.segment.start,
            render_segment.segment.tangent,
        );
        render_segment.end_visual = endpoint_visual(
            &base_segments,
            index,
            render_segment.segment.end,
            -render_segment.segment.tangent,
        );
    }

    render_segments
}

fn endpoint_visual(
    segments: &[RoadRenderSegment],
    segment_index: usize,
    point: Vec2,
    tangent_out: Vec2,
) -> RoadEndpointVisual {
    const ROAD_JUNCTION_TOLERANCE: f32 = 0.1;
    const CONTINUATION_DOT_THRESHOLD: f32 = 0.95;

    let this_segment = segments[segment_index].segment;
    let tangent_out = tangent_out.normalize_or_zero();
    let mut max_other_road_half_width: f32 = 0.0;
    let mut max_other_total_half_width: f32 = 0.0;
    let mut distinct_other_roads = HashSet::new();
    let mut saw_non_continuation = false;

    for (other_index, other_render_segment) in segments.iter().enumerate() {
        if other_index == segment_index {
            continue;
        }

        let other = other_render_segment.segment;
        let projection = project_point_onto_segment(point, other.start, other.end);
        if projection.distance > ROAD_JUNCTION_TOLERANCE {
            continue;
        }

        let hits_other_endpoint = projection.t <= 0.05 || projection.t >= 0.95;
        if other.road_id == this_segment.road_id && hits_other_endpoint {
            continue;
        }

        distinct_other_roads.insert(other.road_id);
        max_other_road_half_width = max_other_road_half_width.max(other.road_width * 0.5);
        max_other_total_half_width = max_other_total_half_width.max(
            other.road_width * 0.5
                + if other.sidewalk_left || other.sidewalk_right {
                    other.sidewalk_width.max(0.0)
                } else {
                    0.0
                },
        );

        let continuation = hits_other_endpoint
            && tangent_out.dot(other.tangent.normalize_or_zero()).abs()
                >= CONTINUATION_DOT_THRESHOLD;
        if !continuation {
            saw_non_continuation = true;
        }
    }

    if max_other_road_half_width <= 0.0 || !saw_non_continuation || distinct_other_roads.is_empty()
    {
        RoadEndpointVisual::default()
    } else {
        RoadEndpointVisual {
            road_extension: max_other_road_half_width,
            sidewalk_trim: max_other_total_half_width,
        }
    }
}

pub fn plot_rect(plot: &MapPlot) -> OrientedRect {
    OrientedRect {
        center: plot.center_vec2(),
        half_extents: plot.half_extents_vec2(),
        rotation_y: plot.rotation_degrees.to_radians(),
    }
}

pub fn plot_chunk_bounds(plot: &MapPlot) -> (i32, i32, i32, i32) {
    plot_rect(plot).chunk_bounds()
}

pub fn chunk_coords_in_bounds(bounds: (i32, i32, i32, i32)) -> Vec<ChunkCoord> {
    let (min_x, max_x, min_z, max_z) = bounds;
    let mut coords = Vec::new();
    for chunk_x in min_x..=max_x {
        for chunk_z in min_z..=max_z {
            coords.push(ChunkCoord::new(chunk_x, chunk_z));
        }
    }
    coords
}

pub fn sample_strip(segment: &RoadSegment, half_width: f32, max_step: f32) -> Vec<StripSample> {
    sample_strip_extended(segment, half_width, max_step, 0.0, 0.0)
}

pub fn sample_strip_extended(
    segment: &RoadSegment,
    half_width: f32,
    max_step: f32,
    start_extension: f32,
    end_extension: f32,
) -> Vec<StripSample> {
    let start = segment.start - segment.tangent * start_extension.max(0.0);
    let end = segment.end + segment.tangent * end_extension.max(0.0);
    let length = start.distance(end);
    let steps = ((length / max_step.max(0.5)).ceil() as usize).max(1);
    let mut out = Vec::with_capacity(steps + 1);
    for step in 0..=steps {
        let t = step as f32 / steps as f32;
        let center = start.lerp(end, t);
        out.push(StripSample {
            left: center + segment.normal * half_width,
            right: center - segment.normal * half_width,
            distance: length * t,
        });
    }
    out
}

pub fn road_polyline_points(road: &MapRoad) -> (Vec<Vec2>, bool) {
    let mut points = road
        .points
        .iter()
        .map(|point| Vec2::new(point[0], point[1]))
        .collect::<Vec<_>>();
    if points.len() < 2 {
        return (points, false);
    }

    let closed = points.len() >= 3 && points.first() == points.last();
    if closed {
        points.pop();
    }
    (points, closed)
}

pub fn sample_polyline_strip(
    points: &[Vec2],
    closed: bool,
    left_offset: f32,
    right_offset: f32,
    max_step: f32,
    start_shift: f32,
    end_shift: f32,
) -> Vec<StripSample> {
    if points.len() < 2 {
        return Vec::new();
    }

    let count = points.len();
    let segment_count = if closed { count } else { count - 1 };
    let mut tangents = Vec::with_capacity(segment_count);
    let mut normals = Vec::with_capacity(segment_count);
    let mut lengths = Vec::with_capacity(segment_count);

    for index in 0..segment_count {
        let start = points[index];
        let end = points[(index + 1) % count];
        let delta = end - start;
        let length = delta.length();
        if length <= 1.0e-4 {
            tangents.push(Vec2::X);
            normals.push(Vec2::Y);
            lengths.push(0.0);
            continue;
        }
        let tangent = delta / length;
        tangents.push(tangent);
        normals.push(Vec2::new(-tangent.y, tangent.x));
        lengths.push(length);
    }

    let mut joints = Vec::with_capacity(if closed { count + 1 } else { count });
    let mut distance = 0.0;

    for index in 0..count {
        let point = points[index];
        let (center, left, right) = if closed {
            let prev_segment = (index + segment_count - 1) % segment_count;
            let next_segment = index % segment_count;
            let prev_tangent = tangents[prev_segment];
            let next_tangent = tangents[next_segment];
            let prev_normal = normals[prev_segment];
            let next_normal = normals[next_segment];
            (
                point,
                offset_polyline_vertex(
                    point,
                    prev_tangent,
                    next_tangent,
                    prev_normal,
                    next_normal,
                    left_offset,
                ),
                offset_polyline_vertex(
                    point,
                    prev_tangent,
                    next_tangent,
                    prev_normal,
                    next_normal,
                    right_offset,
                ),
            )
        } else if index == 0 {
            let tangent = tangents[0];
            let normal = normals[0];
            let center = point - tangent * start_shift;
            (
                center,
                center + normal * left_offset,
                center + normal * right_offset,
            )
        } else if index == count - 1 {
            let tangent = tangents[segment_count - 1];
            let normal = normals[segment_count - 1];
            let center = point + tangent * end_shift;
            (
                center,
                center + normal * left_offset,
                center + normal * right_offset,
            )
        } else {
            let prev_segment = index - 1;
            let next_segment = index;
            let prev_tangent = tangents[prev_segment];
            let next_tangent = tangents[next_segment];
            let prev_normal = normals[prev_segment];
            let next_normal = normals[next_segment];
            (
                point,
                offset_polyline_vertex(
                    point,
                    prev_tangent,
                    next_tangent,
                    prev_normal,
                    next_normal,
                    left_offset,
                ),
                offset_polyline_vertex(
                    point,
                    prev_tangent,
                    next_tangent,
                    prev_normal,
                    next_normal,
                    right_offset,
                ),
            )
        };

        if let Some(previous) = joints.last() {
            distance += previous_center(previous).distance(center);
        }
        joints.push((
            center,
            StripSample {
                left,
                right,
                distance,
            },
        ));
    }

    if closed {
        let first = joints[0];
        distance += previous_center(&joints[count - 1]).distance(first.0);
        joints.push((
            first.0,
            StripSample {
                left: first.1.left,
                right: first.1.right,
                distance,
            },
        ));
    }

    let max_step = max_step.max(0.5);
    let mut out = Vec::new();
    for pair in joints.windows(2) {
        let (start_center, start_sample) = pair[0];
        let (end_center, end_sample) = pair[1];
        let segment_length = start_center.distance(end_center);
        let steps = (segment_length / max_step).ceil() as usize;
        let steps = steps.max(1);
        for step in 0..steps {
            if !out.is_empty() && step == 0 {
                continue;
            }
            let t = step as f32 / steps as f32;
            out.push(StripSample {
                left: start_sample.left.lerp(end_sample.left, t),
                right: start_sample.right.lerp(end_sample.right, t),
                distance: start_sample.distance + (end_sample.distance - start_sample.distance) * t,
            });
        }
    }

    if let Some((_, last)) = joints.last() {
        out.push(*last);
    }

    out
}

fn previous_center(sample: &(Vec2, StripSample)) -> Vec2 {
    sample.0
}

fn offset_polyline_vertex(
    point: Vec2,
    prev_tangent: Vec2,
    next_tangent: Vec2,
    prev_normal: Vec2,
    next_normal: Vec2,
    offset: f32,
) -> Vec2 {
    let prev_tangent = prev_tangent.normalize_or_zero();
    let next_tangent = next_tangent.normalize_or_zero();
    let prev_normal = prev_normal.normalize_or_zero();
    let next_normal = next_normal.normalize_or_zero();

    if prev_tangent.length_squared() <= 1.0e-6 {
        return point + next_normal * offset;
    }
    if next_tangent.length_squared() <= 1.0e-6 {
        return point + prev_normal * offset;
    }

    let joined = prev_normal + next_normal;
    if joined.length_squared() <= 1.0e-6 {
        return point + next_normal * offset;
    }

    let miter = joined.normalize();
    let denom = miter.dot(next_normal).abs().max(0.25);
    let miter_length = (offset / denom).clamp(-offset.abs() * 4.0, offset.abs() * 4.0);
    point + miter * miter_length
}

pub fn project_point_onto_segment(point: Vec2, start: Vec2, end: Vec2) -> SegmentProjection {
    let delta = end - start;
    let len_sq = delta.length_squared();
    if len_sq <= 1e-6 {
        return SegmentProjection {
            closest: start,
            distance: point.distance(start),
            t: 0.0,
        };
    }

    let t = ((point - start).dot(delta) / len_sq).clamp(0.0, 1.0);
    let closest = start + delta * t;
    SegmentProjection {
        closest,
        distance: point.distance(closest),
        t,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::city::{MapPlot, MapRoad, PlotZone, RoadClass, RoadSide};

    #[test]
    fn road_segments_skip_zero_length_pairs() {
        let road = MapRoad {
            id: 1,
            points: vec![[0.0, 0.0], [0.0, 0.0], [5.0, 0.0]],
            width: 8.0,
            road_class: RoadClass::Local,
            lane_count: 2,
            sidewalk_left: true,
            sidewalk_right: true,
            sidewalk_width: 2.0,
            parking_left: false,
            parking_right: false,
            district: None,
        };

        let segments = build_road_segments(&road);
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].length, 5.0);
    }

    #[test]
    fn plot_rect_contains_center() {
        let plot = MapPlot {
            id: 7,
            center: [12.0, -4.0],
            half_extents: [6.0, 8.0],
            rotation_degrees: 15.0,
            zone: PlotZone::Residential,
            frontage_road_id: None,
            setback: 3.0,
            driveway_side: None,
            archetypes: Vec::new(),
            building_kind: None,
            tags: Vec::new(),
        };

        assert!(plot_rect(&plot).contains_point(Vec2::new(12.0, -4.0)));
    }

    #[test]
    fn t_intersection_extends_road_and_trims_sidewalks() {
        let roads = vec![
            MapRoad {
                id: 1,
                points: vec![[0.0, 0.0], [20.0, 0.0]],
                width: 8.0,
                road_class: RoadClass::Local,
                lane_count: 2,
                sidewalk_left: true,
                sidewalk_right: true,
                sidewalk_width: 2.0,
                parking_left: false,
                parking_right: false,
                district: None,
            },
            MapRoad {
                id: 2,
                points: vec![[10.0, 12.0], [10.0, 0.0]],
                width: 8.0,
                road_class: RoadClass::Local,
                lane_count: 2,
                sidewalk_left: true,
                sidewalk_right: true,
                sidewalk_width: 2.0,
                parking_left: false,
                parking_right: false,
                district: None,
            },
        ];

        let render_segments = build_road_render_segments(&roads);
        let side_road = render_segments
            .iter()
            .find(|segment| segment.segment.road_id == 2)
            .copied()
            .expect("side road render segment");

        assert!((side_road.end_visual.road_extension - 4.0).abs() < 1.0e-4);
        assert!((side_road.end_visual.sidewalk_trim - 6.0).abs() < 1.0e-4);
        let trimmed = side_road
            .sidewalk_rect(RoadSide::Left)
            .expect("trimmed sidewalk");
        assert!(trimmed.half_extents.x < side_road.segment.length * 0.5);
    }

    #[test]
    fn same_road_polyline_vertex_does_not_trim_sidewalks() {
        let roads = vec![MapRoad {
            id: 1,
            points: vec![[0.0, 0.0], [10.0, 0.0], [10.0, 10.0]],
            width: 8.0,
            road_class: RoadClass::Local,
            lane_count: 2,
            sidewalk_left: true,
            sidewalk_right: true,
            sidewalk_width: 2.0,
            parking_left: false,
            parking_right: false,
            district: None,
        }];

        let render_segments = build_road_render_segments(&roads);
        assert_eq!(render_segments.len(), 2);
        for render_segment in &render_segments {
            assert_eq!(render_segment.start_visual.sidewalk_trim, 0.0);
            assert_eq!(render_segment.end_visual.sidewalk_trim, 0.0);
        }
    }
}
