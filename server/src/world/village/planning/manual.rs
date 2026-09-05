//! Validation of a player-selected plot against authoritative placement rules.

use super::fishing::fishing_water_quality;
use super::plots::MAX_SETTLEMENT_SEARCH_RADIUS;
use super::road_access::{planned_road_access_path, RoadAccessBlocker};
use super::terrain::{
    farmstead_earthwork_effort, livestock_earthwork_effort, plot_fits_navigation_bounds,
    resource_plot_is_viable, site_quality, slope_at, FREEBOARD, MAX_BUILD_SLOPE,
};
use crate::world::village::*;

/// Authoritative result of a player-selected plot.
///
/// The client predicts these facts for a responsive ghost, but only this
/// result is allowed to consume a permit or reserve land.
#[derive(Debug, Clone)]
pub(crate) struct ManualPlotApproval {
    pub position: Vec3,
    pub rotation: f32,
    pub quality: f32,
    pub road_access: Vec<Vec2>,
    pub road_snapped: bool,
}

/// Validate one exact player-selected plot with the same physical rules used
/// by automatic settlement planning.
///
/// This intentionally receives compact snapshots rather than ECS queries so
/// the player/network domain cannot grow a second planner. Any future terrain,
/// field, collision or road rule belongs here and therefore governs both NPC
/// and player construction.
#[allow(clippy::too_many_arguments)]
pub(crate) fn validate_manual_plot(
    terrain: &WorldTerrain,
    hall: Vec3,
    kind: SettlementBuildingKind,
    requested_position: Vec3,
    requested_rotation: f32,
    occupied: &[(Vec3, f32)],
    roads: &[&VillageRoad],
    planned_accesses: &[PlannedRoadAccess],
    access_blockers: &[RoadAccessBlocker],
    colliders: Option<&StaticColliders>,
    derived: Option<&DerivedColliderLibrary>,
) -> Result<ManualPlotApproval, String> {
    if !requested_position.is_finite() || !requested_rotation.is_finite() {
        return Err("That plot position is not valid.".into());
    }
    let ground = terrain.get_height(requested_position.x, requested_position.z);
    let position = Vec3::new(requested_position.x, ground, requested_position.z);
    let rotation = requested_rotation.rem_euclid(std::f32::consts::TAU);
    let distance = Vec2::new(position.x - hall.x, position.z - hall.z).length();
    if distance > MAX_SETTLEMENT_SEARCH_RADIUS {
        return Err(format!(
            "That plot is outside the settlement's {:.0}m charter.",
            MAX_SETTLEMENT_SEARCH_RADIUS
        ));
    }

    if kind == SettlementBuildingKind::Farmstead {
        if farmstead_earthwork_effort(terrain, position, rotation).is_none() {
            return Err("The farmyard or one of its fields needs excessive earthworks.".into());
        }
    } else if kind == SettlementBuildingKind::LivestockFarm {
        if livestock_earthwork_effort(terrain, position, rotation).is_none() {
            return Err("The livestock yard or pasture needs excessive earthworks.".into());
        }
    } else if slope_at(terrain, position.x, position.z) > MAX_BUILD_SLOPE {
        return Err("The ground is too steep for this building.".into());
    }
    if !plot_fits_navigation_bounds(kind, position, rotation) {
        return Err("Part of this plot would lie outside the playable world.".into());
    }
    if shared::components::minimum_building_water_clearance(terrain, position, kind, rotation)
        < FREEBOARD
    {
        return Err("The building and its doorway must remain safely above the waterline.".into());
    }
    if !crate::world::village_roads::doorway_road_apron_is_dry(terrain, kind, position, rotation) {
        return Err("The doorway has no dry approach.".into());
    }
    if colliders.zip(derived).is_some_and(|(colliders, derived)| {
        !crate::world::village_roads::doorway_road_apron_is_clear_of_props(
            kind, position, rotation, colliders, derived,
        )
    }) {
        return Err("A permanent object blocks the doorway.".into());
    }

    let clearance = kind.clearance();
    if occupied.iter().any(|(other, other_clearance)| {
        Vec2::new(position.x - other.x, position.z - other.z).length() < clearance + other_clearance
    }) {
        return Err("This plot overlaps an existing or reserved building.".into());
    }

    if let (Some(fields), Some(field_half)) = (
        kind.field_positions(position, rotation),
        kind.field_half_extents(),
    ) {
        for field in fields {
            if shared::components::minimum_rotated_rect_water_clearance(
                terrain,
                field,
                field_half + Vec2::splat(shared::components::FARM_FIELD_TERRACE_MARGIN),
                rotation,
            ) < FREEBOARD
            {
                return Err("One of the two wheat fields reaches wet ground.".into());
            }
            if colliders.zip(derived).is_some_and(|(colliders, derived)| {
                !crate::world::village_roads::rotated_rect_is_clear_of_permanent_props(
                    Vec2::new(field.x, field.z),
                    field_half,
                    rotation,
                    shared::components::FARM_FIELD_TERRACE_MARGIN,
                    colliders,
                    derived,
                )
            }) {
                return Err("A permanent object blocks one of the wheat fields.".into());
            }
            let field_clearance =
                field_half.length() + shared::components::FARM_FIELD_TERRACE_MARGIN;
            if occupied.iter().any(|(other, other_clearance)| {
                Vec2::new(field.x - other.x, field.z - other.z).length()
                    < field_clearance + other_clearance
            }) {
                return Err("One of the two wheat fields overlaps reserved land.".into());
            }
            if roads.iter().any(|road| {
                road.intersects_rotated_rect(
                    Vec2::new(field.x, field.z),
                    field_half,
                    rotation,
                    shared::components::FARM_FIELD_TERRACE_MARGIN,
                )
            }) {
                return Err("A road reservation crosses one of the wheat fields.".into());
            }
            if planned_accesses.iter().any(|access| {
                access.intersects_circle(
                    Vec2::new(field.x, field.z),
                    field_half.length() + shared::components::FARM_FIELD_TERRACE_MARGIN,
                )
            }) {
                return Err("Another building's access lane crosses a wheat field.".into());
            }
        }
    }
    if let (Some(pasture), Some(half)) = (
        kind.pasture_position(position, rotation),
        kind.pasture_half_extents(),
    ) {
        if shared::components::minimum_rotated_rect_water_clearance(
            terrain,
            pasture,
            half + Vec2::splat(2.0),
            rotation,
        ) < FREEBOARD
        {
            return Err("The livestock pasture reaches wet ground.".into());
        }
        let pasture_clearance = half.length() + 2.0;
        if occupied.iter().any(|(other, other_clearance)| {
            Vec2::new(pasture.x - other.x, pasture.z - other.z).length()
                < pasture_clearance + other_clearance
        }) {
            return Err("The livestock pasture overlaps reserved land.".into());
        }
        if colliders.zip(derived).is_some_and(|(colliders, derived)| {
            !crate::world::village_roads::rotated_rect_is_clear_of_permanent_props(
                Vec2::new(pasture.x, pasture.z),
                half,
                rotation,
                1.0,
                colliders,
                derived,
            )
        }) {
            return Err("A permanent object blocks the livestock pasture.".into());
        }
        if roads.iter().any(|road| {
            road.intersects_rotated_rect(Vec2::new(pasture.x, pasture.z), half, rotation, 1.0)
        }) {
            return Err("A road reservation crosses the livestock pasture.".into());
        }
    }

    let point = Vec2::new(position.x, position.z);
    let footprint_radius = kind.placement_definition().root_footprint_radius() + 0.45;
    if roads
        .iter()
        .any(|road| road.contains_reserved_point(point, footprint_radius))
    {
        return Err("The building footprint overlaps a road reservation.".into());
    }
    if planned_accesses
        .iter()
        .any(|access| access.intersects_circle(point, footprint_radius))
    {
        return Err("The building footprint overlaps another reserved access lane.".into());
    }

    if kind == SettlementBuildingKind::FishermansHut {
        let water = terrain
            .water_level()
            .ok_or_else(|| "This world has no fishing water.".to_string())?;
        let nets = kind
            .nets_position(position, rotation)
            .ok_or_else(|| "The fishing hut has no usable shore-side work point.".to_string())?;
        let nets_ground = terrain.get_height(nets.x, nets.z);
        if nets_ground < water + 0.15 || (nets_ground - ground).abs() > 1.6 {
            return Err("The fishing hut's side route is not safe dry ground.".into());
        }
        let fishing = kind
            .fishing_position(position, rotation)
            .ok_or_else(|| "The pier has no fishing position.".to_string())?;
        if fishing_water_quality(terrain, fishing, rotation, water) <= 0.0 {
            return Err("Rotate or move the hut so its pier reaches broad open water.".into());
        }
    }

    if !resource_plot_is_viable(terrain, hall, kind, position, rotation, colliders, derived) {
        return Err(match kind {
            SettlementBuildingKind::Farmstead => {
                "Workers cannot reach both fields safely from this farmstead."
            }
            SettlementBuildingKind::LumberjackHut => {
                "This hut has no reachable working forest nearby."
            }
            _ => "Builders cannot reach this plot safely.",
        }
        .into());
    }

    let hall_door = SettlementBuildingKind::Hall.entrance_position(hall, 0.0);
    let connected = crate::world::village_roads::hall_connected_road_keys(
        Vec2::new(hall_door.x, hall_door.z),
        roads,
    );
    let road_access = planned_road_access_path(
        terrain,
        hall,
        kind,
        position,
        rotation,
        roads,
        access_blockers,
        &connected,
    )
    .ok_or_else(|| {
        "No dry access lane can connect this doorway to the Hall network.".to_string()
    })?;
    let road_snapped = road_access.last().is_some_and(|point| {
        connected.contains(&crate::world::village_roads::road_point_key(*point))
    });

    let quality = if kind == SettlementBuildingKind::FishermansHut {
        let water = terrain.water_level().unwrap_or(0.0);
        kind.fishing_position(position, rotation)
            .map_or(0.5, |fishing| {
                fishing_water_quality(terrain, fishing, rotation, water)
            })
    } else {
        site_quality(terrain, kind, position)
    };
    Ok(ManualPlotApproval {
        position,
        rotation,
        quality,
        road_access,
        road_snapped,
    })
}
