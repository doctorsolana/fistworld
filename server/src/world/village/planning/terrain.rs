//! Resource suitability, earthworks, water clearance and plot navigation bounds.

use crate::world::village::*;

/// How well this ground suits what is being built, 0..1.
///
/// Reads the SAME `BiomeField::resources` the grass density and the economy
/// read, so a farmstead standing in thick grass really is standing on good
/// soil — the thing you can see is the thing the number says.
///
/// Slope is passed as zero deliberately. Site quality describes the soil that
/// remains after construction; the separate earthwork score decides how much
/// labour is needed to terrace a moderately uneven Farmstead.
pub fn site_quality(terrain: &WorldTerrain, kind: SettlementBuildingKind, at: Vec3) -> f32 {
    let Some(field) = terrain.generator.loaded_map().biome_field.as_deref() else {
        // Hand-authored maps carry no biome field. Neutral rather than zero: a
        // building that works nowhere is worse than one that works averagely.
        return 0.5;
    };
    let profile = field.resources(at.x, at.z, at.y, 0.0);
    kind.yield_quality(&profile)
}

/// Plot-ranking preference, distinct from the completed building's yield.
/// This lets Windmills seek open ground without giving them a fictitious soil
/// percentage or changing Flour throughput.
pub(super) fn site_placement_suitability(
    terrain: &WorldTerrain,
    kind: SettlementBuildingKind,
    at: Vec3,
) -> f32 {
    let Some(field) = terrain.generator.loaded_map().biome_field.as_deref() else {
        return 0.5;
    };
    let profile = field.resources(at.x, at.z, at.y, 0.0);
    kind.placement_suitability(&profile)
}

/// Steepness at a point, as a rise over the sampling distance.
pub(in crate::world::village) fn slope_at(terrain: &WorldTerrain, x: f32, z: f32) -> f32 {
    const STEP: f32 = 3.0;
    let here = terrain.get_height(x, z);
    let dx = (terrain.get_height(x + STEP, z) - here).abs();
    let dz = (terrain.get_height(x, z + STEP) - here).abs();
    dx.max(dz) / STEP
}

/// Steepest ground a building will accept.
pub(super) const MAX_BUILD_SLOPE: f32 = 0.30;

/// Maximum vertical cut/fill from a plot's own centre height. These are modest
/// farm terraces, not mountain levelling: each field is graded independently
/// and retains a blended edge into the authored terrain.
pub(super) const FARMYARD_MAX_CUT_FILL: f32 = 1.75;

pub(super) const FARM_FIELD_MAX_CUT_FILL: f32 = 2.25;

pub(super) fn rect_max_cut_fill(
    terrain: &WorldTerrain,
    center: Vec2,
    half: Vec2,
    rotation: f32,
) -> f32 {
    let target = terrain.get_height(center.x, center.y);
    rect_max_cut_fill_to(terrain, center, half, rotation, target)
}

pub(super) fn rect_max_cut_fill_to(
    terrain: &WorldTerrain,
    center: Vec2,
    half: Vec2,
    rotation: f32,
    target: f32,
) -> f32 {
    [-1.0_f32, 0.0, 1.0]
        .into_iter()
        .flat_map(|x| {
            [-1.0_f32, 0.0, 1.0].into_iter().map(move |z| {
                shared::rotation::local_to_world_xz(Vec2::new(half.x * x, half.y * z), rotation)
            })
        })
        .map(|offset| (terrain.get_height(center.x + offset.x, center.y + offset.y) - target).abs())
        .fold(0.0, f32::max)
}

pub(in crate::world::village) fn farmstead_earthwork_effort(
    terrain: &WorldTerrain,
    candidate: Vec3,
    rotation: f32,
) -> Option<f32> {
    let kind = SettlementBuildingKind::Farmstead;
    let definition = kind.placement_definition();
    let yard_center = definition.world_footprint_center(candidate, rotation);
    let yard_cut_fill =
        rect_max_cut_fill(terrain, yard_center, definition.footprint * 0.5, rotation);
    if yard_cut_fill > FARMYARD_MAX_CUT_FILL {
        return None;
    }
    let fields = kind.field_positions(candidate, rotation)?;
    let field_half = kind.field_half_extents()?;
    let field_target = fields
        .iter()
        .map(|field| terrain.get_height(field.x, field.z))
        .sum::<f32>()
        / fields.len() as f32;
    let mut total = yard_cut_fill / FARMYARD_MAX_CUT_FILL;
    for field in fields {
        let cut_fill = rect_max_cut_fill_to(
            terrain,
            Vec2::new(field.x, field.z),
            field_half,
            rotation,
            field_target,
        );
        if cut_fill > FARM_FIELD_MAX_CUT_FILL {
            return None;
        }
        total += cut_fill / FARM_FIELD_MAX_CUT_FILL;
    }
    Some(total / 3.0)
}

pub(super) fn livestock_earthwork_effort(
    terrain: &WorldTerrain,
    candidate: Vec3,
    rotation: f32,
) -> Option<f32> {
    let kind = SettlementBuildingKind::LivestockFarm;
    let definition = kind.placement_definition();
    let yard_center = definition.world_footprint_center(candidate, rotation);
    let yard = rect_max_cut_fill(terrain, yard_center, definition.footprint * 0.5, rotation);
    let pasture = kind.pasture_position(candidate, rotation)?;
    let pasture_half = kind.pasture_half_extents()?;
    let grazing = rect_max_cut_fill(
        terrain,
        Vec2::new(pasture.x, pasture.z),
        pasture_half,
        rotation,
    );
    (yard <= FARMYARD_MAX_CUT_FILL && grazing <= FARM_FIELD_MAX_CUT_FILL)
        .then_some((yard / FARMYARD_MAX_CUT_FILL + grazing / FARM_FIELD_MAX_CUT_FILL) * 0.5)
}

/// How far above the waterline anything a settlement builds must stand, in metres.
///
/// Not zero: ground exactly at the waterline is shoreline, and a farmstead with
/// its doorstep in the lake reads as a bug even though the maths permitted it.
pub const FREEBOARD: f32 = 1.5;

/// Keep every authored interaction point on navigable terrain.
///
/// The settlement radius is intentionally allowed to grow, but the active map
/// is still a hard boundary for embodied simulation. Merely checking a plot's
/// centre is not enough near an edge: a valid-looking farmhouse can put its
/// fields outside the map, and a hut can leave its door or pier unreachable.
pub(super) fn plot_fits_navigation_bounds(
    kind: SettlementBuildingKind,
    candidate: Vec3,
    rotation: f32,
) -> bool {
    let point_is_inside =
        |point: Vec3| point.is_finite() && shared::terrain::world_pos_in_bounds(point.x, point.z);
    let rect_is_inside = |center: Vec3, half: Vec2| {
        [
            Vec2::new(-half.x, -half.y),
            Vec2::new(-half.x, half.y),
            Vec2::new(half.x, -half.y),
            Vec2::new(half.x, half.y),
        ]
        .into_iter()
        .map(|corner| shared::rotation::local_to_world_xz(corner, rotation))
        .all(|offset| {
            shared::terrain::world_pos_in_bounds(center.x + offset.x, center.z + offset.y)
        })
    };

    let definition = kind.placement_definition();
    let footprint = definition.footprint * 0.5 + Vec2::splat(0.45);
    let footprint_center = definition.world_footprint_center(candidate, rotation);
    if !point_is_inside(candidate)
        || !rect_is_inside(
            Vec3::new(footprint_center.x, candidate.y, footprint_center.y),
            footprint,
        )
        || !point_is_inside(kind.entrance_position(candidate, rotation))
        || !point_is_inside(shared::components::builder_stand_position(
            candidate,
            rotation,
            definition.footprint.y,
        ))
    {
        return false;
    }

    if let (Some(fields), Some(field_half)) = (
        kind.field_positions(candidate, rotation),
        kind.field_half_extents(),
    ) {
        let reserved_half = field_half + Vec2::splat(shared::components::FARM_FIELD_TERRACE_MARGIN);
        if fields
            .into_iter()
            .any(|field| !rect_is_inside(field, reserved_half))
        {
            return false;
        }
    }
    if let (Some(pasture), Some(half)) = (
        kind.pasture_position(candidate, rotation),
        kind.pasture_half_extents(),
    ) {
        if !rect_is_inside(pasture, half + Vec2::splat(2.0)) {
            return false;
        }
    }

    [
        kind.nets_position(candidate, rotation),
        kind.pier_position(candidate, rotation),
        kind.fishing_position(candidate, rotation),
    ]
    .into_iter()
    .flatten()
    .all(point_is_inside)
}

#[allow(clippy::too_many_arguments)]
pub(super) fn resource_plot_is_viable(
    terrain: &WorldTerrain,
    hall: Vec3,
    kind: SettlementBuildingKind,
    candidate: Vec3,
    rotation: f32,
    colliders: Option<&StaticColliders>,
    derived: Option<&DerivedColliderLibrary>,
) -> bool {
    let stand = shared::components::builder_stand_position(
        candidate,
        rotation,
        kind.placement_definition().footprint.y,
    );
    let hall_entrance = SettlementBuildingKind::Hall.entrance_position(hall, 0.0);
    // The reserved road proves that a future connector can reach the door;
    // material delivery starts before that road exists. Certify the builder's
    // current land route as well so construction cannot create a circular
    // dependency in which the unreachable shell must finish before its access
    // road may be built.
    if !crate::world::village_roads::permit_land_route_exists(terrain, hall_entrance, stand)
        || colliders.zip(derived).is_some_and(|(colliders, derived)| {
            !crate::world::village_roads::navigation_point_is_clear_of_props(
                Vec2::new(stand.x, stand.z),
                colliders,
                derived,
            )
        })
    {
        return false;
    }
    if kind == SettlementBuildingKind::Farmstead
        && kind
            .field_positions(candidate, rotation)
            .is_some_and(|fields| {
                fields.into_iter().enumerate().any(|(index, field)| {
                    crate::world::village_roads::reachable_farm_work_stand(
                        terrain,
                        candidate,
                        rotation,
                        field,
                        index as u32,
                        None,
                        // The Farmstead land claim clears trees from both crop
                        // plots before the fields are planted. Permanent props
                        // were rejected by the rectangle check above, so prove
                        // the future post-earthwork route rather than rejecting
                        // a field because of a tree that the builders remove.
                        None,
                        None,
                    )
                    .is_none()
                })
            })
    {
        return false;
    }
    kind != SettlementBuildingKind::LumberjackHut
        || lumber_plot_has_reachable_tree(terrain, kind.entrance_position(candidate, rotation))
}
