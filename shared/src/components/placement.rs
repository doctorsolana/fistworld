//! Deterministic water clearance and settlement-founding validation.

use super::{CivicHallLevel, SettlementBuildingKind};
use bevy::prelude::*;

/// Lowest vertical gap between a rotated ground rectangle and the local water
/// surface beneath it.
///
/// Unlike subtracting [`WorldTerrain::water_level`](crate::terrain::WorldTerrain::water_level),
/// this includes sloping inland rivers. Samples stay below one metre apart so
/// a narrow headwater cannot pass between a rectangle's corners unnoticed.
pub fn minimum_rotated_rect_water_clearance(
    terrain: &crate::terrain::WorldTerrain,
    centre: Vec3,
    half_extents: Vec2,
    rotation_y: f32,
) -> f32 {
    const SAMPLE_SPACING: f32 = 0.75;
    let x_intervals = ((half_extents.x * 2.0 / SAMPLE_SPACING).ceil() as usize).max(1);
    let z_intervals = ((half_extents.y * 2.0 / SAMPLE_SPACING).ceil() as usize).max(1);
    let mut minimum = f32::INFINITY;

    for x_step in 0..=x_intervals {
        for z_step in 0..=z_intervals {
            let t_x = x_step as f32 / x_intervals as f32;
            let t_z = z_step as f32 / z_intervals as f32;
            let local = Vec2::new(
                -half_extents.x + half_extents.x * 2.0 * t_x,
                -half_extents.y + half_extents.y * 2.0 * t_z,
            );
            let offset = crate::rotation::local_to_world_xz(local, rotation_y);
            let x = centre.x + offset.x;
            let z = centre.z + offset.y;
            let Some(water) = terrain.water_surface_height(x, z) else {
                continue;
            };
            minimum = minimum.min(terrain.get_height(x, z) - water);
        }
    }
    minimum
}

/// Minimum local-water clearance under a building's full rotated footprint
/// and authored door position.
pub fn minimum_building_water_clearance(
    terrain: &crate::terrain::WorldTerrain,
    centre: Vec3,
    kind: SettlementBuildingKind,
    rotation_y: f32,
) -> f32 {
    let definition = kind.placement_definition();
    let half = definition.footprint * 0.5 + Vec2::splat(0.25);
    let footprint_center = definition.world_footprint_center(centre, rotation_y);
    let footprint = minimum_rotated_rect_water_clearance(
        terrain,
        Vec3::new(footprint_center.x, centre.y, footprint_center.y),
        half,
        rotation_y,
    );
    let door = kind.entrance_position(centre, rotation_y);
    let door_clearance = terrain
        .water_surface_height(door.x, door.z)
        .map_or(f32::INFINITY, |water| {
            terrain.get_height(door.x, door.z) - water
        });
    footprint.min(door_clearance)
}

/// Local-water clearance beneath the complete civic shell reserved on the
/// founding day, not merely beneath the currently visible Moot Hall.
pub fn minimum_civic_hall_reservation_water_clearance(
    terrain: &crate::terrain::WorldTerrain,
    root: Vec3,
    rotation_y: f32,
) -> f32 {
    let definition = CivicHallLevel::largest_supported()
        .building_type()
        .definition();
    let footprint_center = definition.world_footprint_center(root, rotation_y);
    let footprint = minimum_rotated_rect_water_clearance(
        terrain,
        Vec3::new(footprint_center.x, root.y, footprint_center.y),
        definition.footprint * 0.5 + Vec2::splat(0.25),
        rotation_y,
    );
    let door = SettlementBuildingKind::Hall.entrance_position(root, rotation_y);
    let door_clearance = terrain
        .water_surface_height(door.x, door.z)
        .map_or(f32::INFINITY, |water| {
            terrain.get_height(door.x, door.z) - water
        });
    footprint.min(door_clearance)
}

/// How far apart settlements must be founded, in metres.
///
/// Lives in `shared` because BOTH sides need it and they must not disagree: the
/// server enforces it, and the client checks it before sending so the player is
/// told why a click did nothing instead of watching the button reset in silence.
pub const MIN_SETTLEMENT_SPACING: f32 = 300.0;

/// How far above the waterline a settlement must be founded, in metres.
///
/// Same reason as the spacing: a hall founded in a lake looks fine and then
/// never builds anything, because every plot its residents try is refused as
/// underwater. Better to say no at the click.
pub const SETTLEMENT_FREEBOARD: f32 = 0.35;

/// Why a settlement cannot be founded at a point, if it cannot.
///
/// Returned as a sentence rather than a code because its only job is to be
/// shown to a person.
pub fn founding_refusal(
    ground: f32,
    water_level: Option<f32>,
    nearest_settlement: Option<(&str, f32)>,
) -> Option<String> {
    if water_level.is_some_and(|level| ground < level + SETTLEMENT_FREEBOARD) {
        return Some("The Moot Hall would touch the water".to_string());
    }
    if let Some((name, distance)) = nearest_settlement {
        if distance < MIN_SETTLEMENT_SPACING {
            return Some(format!(
                "Too close to {name} ({distance:.0}m of {MIN_SETTLEMENT_SPACING:.0}m)"
            ));
        }
    }
    None
}

/// Authoritative founding check against the complete hall footprint and local
/// river/ocean surface.
pub fn settlement_founding_refusal(
    terrain: &crate::terrain::WorldTerrain,
    centre: Vec3,
    nearest_settlement: Option<(&str, f32)>,
) -> Option<String> {
    if minimum_civic_hall_reservation_water_clearance(terrain, centre, 0.0) < SETTLEMENT_FREEBOARD {
        return Some("The Moot Hall would touch the water".to_string());
    }
    founding_refusal(f32::INFINITY, None, nearest_settlement)
}
