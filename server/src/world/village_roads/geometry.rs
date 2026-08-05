//! Pure road-width and dry-ground geometry rules.

use bevy::prelude::*;
use shared::components::RoadClass;
use shared::terrain::WorldTerrain;

use crate::world::navgrid::NAVIGATION_SAMPLE_STEP;

use super::VILLAGE_ROAD_WIDTH;

const ROAD_WATER_FREEBOARD: f32 = 0.25;

pub(crate) fn surface_width_for_tier(
    tier: shared::components::SettlementTier,
    class: RoadClass,
) -> f32 {
    match (tier, class) {
        (shared::components::SettlementTier::Hamlet, _) => VILLAGE_ROAD_WIDTH,
        (shared::components::SettlementTier::Village, RoadClass::Main) => 3.4,
        (shared::components::SettlementTier::Village, RoadClass::Lane) => 2.8,
        (_, RoadClass::Main) => 4.0,
        (_, RoadClass::Lane) => 3.0,
    }
}

pub(crate) fn road_sample_is_dry_at_width(terrain: &WorldTerrain, point: Vec2, width: f32) -> bool {
    let shoulder = width * 0.5 + 0.2;
    let diagonal = shoulder * std::f32::consts::FRAC_1_SQRT_2;
    [
        Vec2::ZERO,
        Vec2::X * shoulder,
        Vec2::NEG_X * shoulder,
        Vec2::Y * shoulder,
        Vec2::NEG_Y * shoulder,
        Vec2::new(diagonal, diagonal),
        Vec2::new(-diagonal, diagonal),
        Vec2::new(diagonal, -diagonal),
        Vec2::new(-diagonal, -diagonal),
    ]
    .into_iter()
    .all(|offset| {
        let sample = point + offset;
        terrain
            .water_surface_height(sample.x, sample.y)
            .is_none_or(|water| {
                terrain.get_height(sample.x, sample.y) >= water + ROAD_WATER_FREEBOARD
            })
    })
}

pub(crate) fn road_sample_is_dry(terrain: &WorldTerrain, point: Vec2) -> bool {
    road_sample_is_dry_at_width(terrain, point, VILLAGE_ROAD_WIDTH)
}

pub(crate) fn road_segment_is_dry(terrain: &WorldTerrain, start: Vec2, end: Vec2) -> bool {
    road_segment_is_dry_at_width(terrain, start, end, VILLAGE_ROAD_WIDTH)
}

pub(crate) fn road_segment_is_dry_at_width(
    terrain: &WorldTerrain,
    start: Vec2,
    end: Vec2,
    width: f32,
) -> bool {
    let steps = (start.distance(end) / NAVIGATION_SAMPLE_STEP)
        .ceil()
        .max(1.0) as usize;
    (0..=steps).all(|step| {
        road_sample_is_dry_at_width(terrain, start.lerp(end, step as f32 / steps as f32), width)
    })
}

pub(crate) fn road_corridor_is_dry(terrain: &WorldTerrain, points: &[Vec2], width: f32) -> bool {
    points
        .windows(2)
        .all(|pair| road_segment_is_dry_at_width(terrain, pair[0], pair[1], width))
}
