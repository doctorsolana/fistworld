use std::collections::HashMap;

use bevy::prelude::*;

use crate::{
    map::MapEditsDefinition,
    terrain::{ChunkCoord, WorldTerrain},
};

use super::{
    build_road_segments, chunk_coords_in_bounds, project_point_onto_segment, MapPlot, MapRoad,
    RoadSegment, RoadSide,
};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct NearestRoadSegment {
    pub road_id: u64,
    pub segment_index: usize,
    pub closest_point: Vec2,
    pub distance: f32,
    pub t: f32,
    pub side: Option<RoadSide>,
    pub tangent: Vec2,
    pub normal: Vec2,
    pub width: f32,
    pub sidewalk_width: f32,
}

#[derive(Debug, Clone)]
pub struct CityLayout {
    roads: Vec<MapRoad>,
    plots: Vec<MapPlot>,
    road_segments: Vec<RoadSegment>,
    road_segments_by_chunk: HashMap<ChunkCoord, Vec<usize>>,
    roads_by_id: HashMap<u64, usize>,
    plots_by_id: HashMap<u64, usize>,
}

impl CityLayout {
    pub fn new(roads: Vec<MapRoad>, plots: Vec<MapPlot>) -> Self {
        let mut road_segments = Vec::new();
        let mut road_segments_by_chunk: HashMap<ChunkCoord, Vec<usize>> = HashMap::new();
        let roads_by_id = roads
            .iter()
            .enumerate()
            .map(|(index, road)| (road.id, index))
            .collect::<HashMap<_, _>>();
        let plots_by_id = plots
            .iter()
            .enumerate()
            .map(|(index, plot)| (plot.id, index))
            .collect::<HashMap<_, _>>();

        for road in &roads {
            for segment in build_road_segments(road) {
                let segment_index = road_segments.len();
                let chunk_bounds = segment.envelope_rect().chunk_bounds();
                for chunk in chunk_coords_in_bounds(chunk_bounds) {
                    road_segments_by_chunk
                        .entry(chunk)
                        .or_default()
                        .push(segment_index);
                }
                road_segments.push(segment);
            }
        }

        Self {
            roads,
            plots,
            road_segments,
            road_segments_by_chunk,
            roads_by_id,
            plots_by_id,
        }
    }

    pub fn from_map_edits(edits: &MapEditsDefinition) -> Self {
        Self::new(edits.roads.clone(), edits.plots.clone())
    }

    #[inline]
    pub fn roads(&self) -> &[MapRoad] {
        &self.roads
    }

    #[inline]
    pub fn plots(&self) -> &[MapPlot] {
        &self.plots
    }

    #[inline]
    pub fn road_segments(&self) -> &[RoadSegment] {
        &self.road_segments
    }

    pub fn road(&self, id: u64) -> Option<&MapRoad> {
        self.roads_by_id
            .get(&id)
            .and_then(|index| self.roads.get(*index))
    }

    pub fn plot(&self, id: u64) -> Option<&MapPlot> {
        self.plots_by_id
            .get(&id)
            .and_then(|index| self.plots.get(*index))
    }

    pub fn nearest_road_segment(
        &self,
        point: Vec2,
        max_distance: f32,
    ) -> Option<NearestRoadSegment> {
        let radius = max_distance.max(0.0);
        let min_chunk_x = ((point.x - radius) / crate::terrain::CHUNK_SIZE).floor() as i32;
        let max_chunk_x = ((point.x + radius) / crate::terrain::CHUNK_SIZE).floor() as i32;
        let min_chunk_z = ((point.y - radius) / crate::terrain::CHUNK_SIZE).floor() as i32;
        let max_chunk_z = ((point.y + radius) / crate::terrain::CHUNK_SIZE).floor() as i32;

        let mut candidate_indices = Vec::new();
        for chunk_x in min_chunk_x..=max_chunk_x {
            for chunk_z in min_chunk_z..=max_chunk_z {
                let coord = ChunkCoord::new(chunk_x, chunk_z);
                if let Some(indices) = self.road_segments_by_chunk.get(&coord) {
                    candidate_indices.extend(indices.iter().copied());
                }
            }
        }

        if candidate_indices.is_empty() {
            candidate_indices.extend(0..self.road_segments.len());
        }

        let mut best = None;
        let mut best_distance = max_distance;
        for index in candidate_indices {
            let segment = self.road_segments[index];
            let projection = project_point_onto_segment(point, segment.start, segment.end);
            if projection.distance > best_distance {
                continue;
            }
            best_distance = projection.distance;
            let side = {
                let side_dot = (point - projection.closest).dot(segment.normal);
                if side_dot.abs() <= 1e-3 {
                    None
                } else if side_dot > 0.0 {
                    Some(RoadSide::Left)
                } else {
                    Some(RoadSide::Right)
                }
            };
            best = Some(NearestRoadSegment {
                road_id: segment.road_id,
                segment_index: segment.segment_index,
                closest_point: projection.closest,
                distance: projection.distance,
                t: projection.t,
                side,
                tangent: segment.tangent,
                normal: segment.normal,
                width: segment.road_width,
                sidewalk_width: segment.sidewalk_width,
            });
        }

        best
    }
}

#[derive(Resource, Debug, Clone)]
pub struct AuthoredCityLayout {
    pub map_id: String,
    pub content_hash: u64,
    pub layout: CityLayout,
}

impl FromWorld for AuthoredCityLayout {
    fn from_world(world: &mut World) -> Self {
        let terrain = world.resource::<WorldTerrain>();
        let loaded_map = terrain.generator.loaded_map();
        Self {
            map_id: loaded_map.definition.map_id.clone(),
            content_hash: loaded_map.content_hash,
            layout: CityLayout::from_map_edits(&loaded_map.edits),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::city::{MapPlot, MapRoad, PlotZone, RoadClass};

    #[test]
    fn nearest_road_segment_finds_closest_segment() {
        let layout = CityLayout::new(
            vec![MapRoad {
                id: 5,
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
            }],
            vec![MapPlot {
                id: 9,
                center: [8.0, 12.0],
                half_extents: [4.0, 6.0],
                rotation_degrees: 0.0,
                zone: PlotZone::Residential,
                frontage_road_id: Some(5),
                setback: 3.0,
                driveway_side: Some(RoadSide::Left),
                archetypes: Vec::new(),
                building_kind: None,
                tags: Vec::new(),
            }],
        );

        let nearest = layout
            .nearest_road_segment(Vec2::new(10.0, 5.0), 10.0)
            .expect("nearest segment should exist");
        assert_eq!(nearest.road_id, 5);
        assert_eq!(nearest.side, Some(RoadSide::Left));
        assert!(nearest.distance < 6.0);
    }
}
