//! A disposable coarse land-region hint for founding. It prevents repeatedly
//! running full route searches across ocean/river barriers. It never approves
//! a town connection: the ordinary corridor validator still does that.

use crate::world::village_roads::{road_sample_is_dry, road_segment_is_coarsely_dry};
use bevy::prelude::*;
use shared::terrain::WorldTerrain;

const STEP: f32 = 32.0;

pub(super) struct LandRegions {
    min: Vec2,
    width: usize,
    height: usize,
    labels: Vec<usize>,
}

impl LandRegions {
    pub fn survey(terrain: &WorldTerrain) -> Self {
        let bounds = terrain.generator.active_map_bounds();
        let min = bounds.min_vec2();
        let size = bounds.max_vec2() - min;
        let width = (size.x / STEP) as usize + 1;
        let height = (size.y / STEP) as usize + 1;
        let mut result = Self {
            min,
            width,
            height,
            labels: vec![0; width * height],
        };
        let dry: Vec<_> = (0..width * height)
            .map(|i| road_sample_is_dry(terrain, result.point(i)))
            .collect();
        let mut label = 0;
        let mut stack = Vec::new();
        for start in 0..dry.len() {
            if !dry[start] || result.labels[start] != 0 {
                continue;
            }
            label += 1;
            result.labels[start] = label;
            stack.push(start);
            while let Some(i) = stack.pop() {
                let x = i % width;
                let z = i / width;
                let neighbors = [
                    (x > 0).then(|| i - 1),
                    (x + 1 < width).then_some(i + 1),
                    (z > 0).then(|| i - width),
                    (z + 1 < height).then_some(i + width),
                ];
                for next in neighbors.into_iter().flatten() {
                    if dry[next]
                        && result.labels[next] == 0
                        && road_segment_is_coarsely_dry(
                            terrain,
                            result.point(i),
                            result.point(next),
                        )
                    {
                        result.labels[next] = label;
                        stack.push(next);
                    }
                }
            }
        }
        result
    }

    fn point(&self, index: usize) -> Vec2 {
        self.min + Vec2::new((index % self.width) as f32, (index / self.width) as f32) * STEP
    }

    pub fn at(&self, terrain: &WorldTerrain, point: Vec2) -> usize {
        let local = ((point - self.min) / STEP).round().as_ivec2();
        for offset in [IVec2::ZERO, IVec2::X, IVec2::NEG_X, IVec2::Y, IVec2::NEG_Y] {
            let cell = local + offset;
            if cell.x < 0
                || cell.y < 0
                || cell.x as usize >= self.width
                || cell.y as usize >= self.height
            {
                continue;
            }
            let index = cell.y as usize * self.width + cell.x as usize;
            if self.labels[index] != 0
                && road_segment_is_coarsely_dry(terrain, point, self.point(index))
            {
                return self.labels[index];
            }
        }
        0
    }
}
