//! Settlement layout grammar, candidate ranking and resumable land-site searches.

use super::districts::SettlementUrbanPlan;
use super::neighborhood::{
    affinity_score, house_frontage_candidates, house_frontage_pitch, PlotNeighbor,
};
use super::road_access::{
    direct_road_access_is_coarsely_clear, nearest_completed_road_frontage,
    planned_road_access_path, RoadAccessBlocker,
};
use super::terrain::{
    farmstead_earthwork_effort, livestock_earthwork_effort, plot_fits_navigation_bounds,
    resource_plot_is_viable, site_placement_suitability, slope_at, FREEBOARD, MAX_BUILD_SLOPE,
};
use crate::world::village::*;

pub(super) const MAX_SETTLEMENT_SEARCH_RADIUS: f32 = 320.0;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub(super) struct SiteSearchRejections {
    pub(super) sampled: u32,
    pub(super) earthworks: u32,
    pub(super) bounds: u32,
    pub(super) water: u32,
    pub(super) props: u32,
    pub(super) occupied: u32,
    pub(super) roads: u32,
    pub(super) access_or_work: u32,
}

impl SiteSearchRejections {
    pub(super) fn summary(self) -> String {
        format!(
            "sampled={} rejected earthworks={} bounds={} water={} permanent_props={} occupied={} roads={} access/work={}",
            self.sampled,
            self.earthworks,
            self.bounds,
            self.water,
            self.props,
            self.occupied,
            self.roads,
            self.access_or_work,
        )
    }
}

pub(super) fn include_resumable_search_cursor(
    min_radius: f32,
    max_radius: f32,
    cursor: Option<f32>,
) -> (f32, f32) {
    let Some(cursor) = cursor else {
        return (min_radius, max_radius);
    };
    let cursor = cursor.clamp(min_radius, MAX_SETTLEMENT_SEARCH_RADIUS);
    // The cursor is authoritative progress through the physical envelope. It
    // must expand the current preferred band as it advances; clamping it back
    // to `max_radius` repeatedly sampled the same ring and eventually recorded
    // a false "320 m exhausted" result without ever visiting outer ground.
    (min_radius.max(cursor), max_radius.max(cursor))
}

/// Deterministic fallback search for a legal building plot.
///
/// Walks outward in rings from the hall, sampling a fixed number of bearings
/// per ring, and takes the first spot that is flat enough and clear of what is
/// already there. Deterministic on purpose: the same village in the same state
/// makes the same choice, so a bug is reproducible rather than a story about
/// what happened once.
///
/// Production permits call `find_site_with_plan` with the settlement charter,
/// obstacle indexes and adjacency context. This public no-context wrapper is a
/// conservative baseline used by geometry and road regression tests; it still
/// treats completed roads as occupied infrastructure.
#[cfg(test)]
pub fn find_site(
    terrain: &WorldTerrain,
    hall: Vec3,
    kind: SettlementBuildingKind,
    occupied: &[(Vec3, f32)],
    roads: &[&VillageRoad],
) -> Option<(Vec3, f32)> {
    find_site_with_plan(
        terrain,
        hall,
        kind,
        occupied,
        roads,
        &[],
        &[],
        None,
        None,
        None,
        None,
        None,
    )
}

#[derive(Clone, Copy, Debug)]
pub(super) struct PlannedPlotCandidate {
    /// Building centre relative to the Moot Hall.
    pub(super) local: Vec2,
    /// Point on the intended lane, street, avenue, or neighbourhood green that
    /// the building's door should face.
    pub(super) frontage: Vec2,
}

pub(super) fn plan_axis(plan: &shared::components::SettlementDevelopment) -> (Vec2, Vec2) {
    // Small explicit review seeds need the same full-width entropy as a
    // foundation hash. Keep the public seed; decode geometry from mixed bits.
    let seed = shared::worldgen::splitmix64(plan.plan_seed);
    let fraction = (seed.rotate_right(29) & 0xffff) as f32 / 65_535.0;
    let angle = fraction * std::f32::consts::TAU;
    let axis = Vec2::new(angle.cos(), angle.sin());
    (axis, Vec2::new(-axis.y, axis.x))
}

pub(super) fn plan_center_offset(
    plan: &shared::components::SettlementDevelopment,
    axis: Vec2,
    side: Vec2,
) -> Vec2 {
    use shared::components::SettlementCenterStyle as Center;
    let handedness = if shared::worldgen::splitmix64(plan.plan_seed).rotate_right(17) & 1 == 0 {
        1.0
    } else {
        -1.0
    };
    match plan.center {
        Center::Green => side * handedness * 8.0,
        Center::Square => Vec2::ZERO,
        Center::Avenue => axis * 6.0,
        Center::Courtyard => (axis + side * handedness) * 5.0,
    }
}

/// Convert one deterministic search sample into a recognisable piece of the
/// settlement's street grammar. Buildings are placed BESIDE an implied street
/// and face that street. The road builder later surveys from the authored door
/// to the existing network, turning this inexpensive plan into real terrain-
/// aware paths without pre-baking a whole city.
pub(super) fn planned_plot_candidate(
    plan: &shared::components::SettlementDevelopment,
    kind: SettlementBuildingKind,
    radius: f32,
    bearing: usize,
    base: Vec2,
) -> PlannedPlotCandidate {
    use shared::components::SettlementLayoutStyle as Style;

    let (axis, side) = plan_axis(plan);
    let centre = plan_center_offset(plan, axis, side);
    let seed = shared::worldgen::splitmix64(plan.plan_seed);
    let handedness = if seed & 1 == 0 { 1.0 } else { -1.0 };
    let side_sign = if bearing & 1 == 0 { 1.0 } else { -1.0 };

    match plan.layout {
        Style::Organic => {
            // Three gently wandering lanes. Houses occupy alternating verges
            // instead of forming a ring around the hall.
            let branch = bearing % 3;
            let branch_angle = (seed.rotate_right(7) & 0xffff) as f32 / 65_535.0
                * std::f32::consts::TAU
                + branch as f32 * std::f32::consts::TAU / 3.0
                + (radius * 0.085 + branch as f32).sin() * 0.18;
            let direction = Vec2::new(branch_angle.cos(), branch_angle.sin());
            let normal = Vec2::new(-direction.y, direction.x);
            let bend =
                normal * (radius * 0.11 + bearing as f32 + (seed & 31) as f32 * 0.03).sin() * 5.5;
            let frontage = centre + direction * radius + bend;
            let setback = 8.5 + (bearing / 6) as f32 * 3.0;
            PlannedPlotCandidate {
                local: frontage + normal * side_sign * setback,
                frontage,
            }
        }
        Style::Radial => {
            // Buildings front the sides of several spokes, not the Moot Hall.
            let branches = 5 + (seed.rotate_right(13) & 1) as usize;
            let branch = (bearing / 2) % branches;
            let angle =
                axis.y.atan2(axis.x) + branch as f32 * std::f32::consts::TAU / branches as f32;
            let direction = Vec2::new(angle.cos(), angle.sin());
            let normal = Vec2::new(-direction.y, direction.x);
            let frontage = centre + direction * radius;
            PlannedPlotCandidate {
                local: frontage + normal * side_sign * 9.0,
                frontage,
            }
        }
        Style::Grid => {
            // Seed-rotated orthogonal streets. Each sample is a building set
            // back from the nearest grid line with its facade parallel to it.
            let frontage_step = house_frontage_pitch();
            const SETBACK: f32 = 9.0;
            // Opposing rear yards must fit too: a 26 m block with two 9 m
            // setbacks left only 8 m between rows and rejected one whole side.
            let block =
                SETBACK * 2.0 + frontage_step + (seed.rotate_right(43) & 255) as f32 / 255.0 * 3.0;
            let along = base.dot(axis);
            let across = base.dot(side);
            if bearing & 1 == 0 {
                let street_across = (across / block).round() * block;
                let frontage = centre
                    + axis * ((along / frontage_step).round() * frontage_step)
                    + side * street_across;
                let verge = if (across - street_across).abs() > 0.5 {
                    (across - street_across).signum()
                } else {
                    side_sign
                };
                PlannedPlotCandidate {
                    local: frontage + side * verge * SETBACK,
                    frontage,
                }
            } else {
                let street_along = (along / block).round() * block;
                let frontage = centre
                    + axis * street_along
                    + side * ((across / frontage_step).round() * frontage_step);
                let verge = if (along - street_along).abs() > 0.5 {
                    (along - street_along).signum()
                } else {
                    side_sign
                };
                PlannedPlotCandidate {
                    local: frontage + axis * verge * SETBACK,
                    frontage,
                }
            }
        }
        Style::Avenue => {
            // A long civic spine with buildings on both sides. Farther rings
            // extend the avenue rather than inflating another circle.
            let mut along = base.dot(axis) * 1.35;
            if along.abs() < 10.0 {
                along = side_sign * radius * 0.8;
            }
            let avenue = centre + axis * along;
            let verge = if base.dot(side).abs() > 0.5 {
                base.dot(side).signum()
            } else {
                side_sign * handedness
            };
            PlannedPlotCandidate {
                local: avenue + side * verge * (12.0 + (bearing / 8) as f32 * 5.0),
                frontage: avenue,
            }
        }
        Style::Polycentric => {
            // Persistent neighbourhood centres with expanding local streets.
            // Previously the outward search only rotated 9/14 m pockets, so
            // mature clusters inevitably exhausted them and lost their grammar
            // in the generic radial fallback despite nearby open ground.
            let districts = 3 + ((seed.rotate_right(21) & 1) as usize);
            let district = bearing % districts;
            let district_angle = axis.y.atan2(axis.x)
                + handedness * district as f32 * std::f32::consts::TAU / districts as f32;
            let district_direction = Vec2::new(district_angle.cos(), district_angle.sin());
            let district_distance = match kind {
                SettlementBuildingKind::Farmstead
                | SettlementBuildingKind::LivestockFarm
                | SettlementBuildingKind::LumberjackHut
                | SettlementBuildingKind::StoneQuarry
                | SettlementBuildingKind::FishermansHut => 44.0,
                _ => 30.0,
            };
            let district_centre = centre + district_direction * district_distance;
            let orbit_angle = district_angle
                + std::f32::consts::FRAC_PI_2
                + (bearing / districts) as f32 * 0.75
                + (radius * 0.035).sin() * 0.22;
            let growth = (radius - kind.preferred_ring().0).max(0.0);
            let openness = 0.62 + (seed.rotate_right(35) & 255) as f32 / 255.0 * 0.18;
            let orbit = Vec2::new(orbit_angle.cos(), orbit_angle.sin()) * (9.0 + growth * openness);
            PlannedPlotCandidate {
                local: district_centre + orbit,
                frontage: district_centre,
            }
        }
    }
}

pub(super) fn rotation_facing_frontage(building: Vec2, frontage: Vec2) -> f32 {
    // Building doors are authored on local -Z. Rotating local -Z toward the
    // target requires the vector FROM the target back to the building.
    let outward = building - frontage;
    outward.x.atan2(outward.y)
}

#[cfg(test)]
pub(in crate::world::village) fn find_site_with_plan(
    terrain: &WorldTerrain,
    hall: Vec3,
    kind: SettlementBuildingKind,
    occupied: &[(Vec3, f32)],
    roads: &[&VillageRoad],
    planned_accesses: &[PlannedRoadAccess],
    access_blockers: &[RoadAccessBlocker],
    development: Option<&shared::components::SettlementDevelopment>,
    colliders: Option<&StaticColliders>,
    derived: Option<&DerivedColliderLibrary>,
    minimum_radius_hint: Option<f32>,
    maximum_search_rings: Option<usize>,
) -> Option<(Vec3, f32)> {
    find_site_with_plan_diagnostics(
        terrain,
        hall,
        kind,
        occupied,
        &[],
        roads,
        planned_accesses,
        access_blockers,
        development,
        colliders,
        derived,
        minimum_radius_hint,
        maximum_search_rings,
        None,
        None,
        None,
        &[],
    )
}

#[allow(clippy::too_many_arguments)]
pub(super) fn find_site_with_plan_diagnostics(
    terrain: &WorldTerrain,
    hall: Vec3,
    kind: SettlementBuildingKind,
    occupied: &[(Vec3, f32)],
    neighbors: &[PlotNeighbor],
    roads: &[&VillageRoad],
    planned_accesses: &[PlannedRoadAccess],
    access_blockers: &[RoadAccessBlocker],
    development: Option<&shared::components::SettlementDevelopment>,
    colliders: Option<&StaticColliders>,
    derived: Option<&DerivedColliderLibrary>,
    minimum_radius_hint: Option<f32>,
    maximum_search_rings: Option<usize>,
    mut rejections: Option<&mut SiteSearchRejections>,
    urban: Option<&SettlementUrbanPlan>,
    defenses: Option<&shared::components::SettlementDefenses>,
    squares: &[&shared::components::SettlementCivicSquare],
) -> Option<(Vec3, f32)> {
    macro_rules! reject {
        ($field:ident) => {
            if let Some(rejections) = rejections.as_deref_mut() {
                rejections.$field = rejections.$field.saturating_add(1);
            }
        };
    }
    const SEEDED_BEARINGS: usize = 12;
    const FALLBACK_BEARINGS: usize = 48;
    const RING_STEP: f32 = 6.0;
    const RESOURCE_PLOT_SHORTLIST: usize = 3;
    // Farms and timber plots still compare several directions and three
    // successive distance bands. Searching every ring out to an expanding
    // 320 m city boundary merely to replace a six-entry shortlist made one
    // permit decision monopolise a server tick in mature settlements.
    const RESOURCE_SEARCH_BANDS: usize = 3;

    let hall_door3 = SettlementBuildingKind::Hall.entrance_position(hall, 0.0);
    let connected_road_keys = crate::world::village_roads::hall_connected_road_keys(
        Vec2::new(hall_door3.x, hall_door3.z),
        roads,
    );

    let (mut min_radius, mut max_radius) = kind.preferred_ring();
    // The authored rings are the attractive founding core, not a hard city
    // boundary. Six buildings and their fields can fill that core quickly;
    // widen future search bands deterministically as the occupied envelope
    // grows so a migration wave cannot strand everyone after the sixth cabin.
    let expansion_bands = occupied.len().saturating_sub(6).div_ceil(6) as f32;
    let expansion_per_band = match kind {
        SettlementBuildingKind::House
        | SettlementBuildingKind::Market
        | SettlementBuildingKind::Tavern
        | SettlementBuildingKind::Church
        | SettlementBuildingKind::Bakery
        | SettlementBuildingKind::StorageHall => 18.0,
        SettlementBuildingKind::Farmstead
        | SettlementBuildingKind::LivestockFarm
        | SettlementBuildingKind::LumberjackHut
        | SettlementBuildingKind::FishermansHut
        | SettlementBuildingKind::Windmill
        | SettlementBuildingKind::StoneQuarry => 12.0,
        SettlementBuildingKind::Hall => 0.0,
    };
    max_radius =
        (max_radius + expansion_bands * expansion_per_band).min(MAX_SETTLEMENT_SEARCH_RADIUS);
    if let Some(plan) = development {
        use shared::components::SettlementCenterStyle as Center;
        if kind == SettlementBuildingKind::House {
            // Preserve the selected civic centre from the first cabin onward.
            min_radius = min_radius.max(match plan.center {
                Center::Green => 24.0,
                Center::Square => 27.0,
                Center::Avenue => 19.0,
                Center::Courtyard => 26.0,
            });
        }
    }
    // Monotonic construction means a ring exhausted before the previous
    // success cannot become less occupied. Keep testing the successful ring
    // (there may be unused bearings), then continue outward. The cursor also
    // expands the preferred band: it represents physical search progress, not
    // merely a ranking hint inside the founding layout radius.
    (min_radius, max_radius) =
        include_resumable_search_cursor(min_radius, max_radius, minimum_radius_hint);
    let clearance = kind.clearance();
    let resource_scored = matches!(
        kind,
        SettlementBuildingKind::Farmstead
            | SettlementBuildingKind::LivestockFarm
            | SettlementBuildingKind::LumberjackHut
            | SettlementBuildingKind::Windmill
            | SettlementBuildingKind::StoneQuarry
    );
    // The authored grammar needs only its stable lane/spoke samples. If that
    // preferred pass is exhausted, the open-land fallback deliberately probes
    // between those streets as well; twelve radial bearings left large wedges
    // completely invisible once several roads and crop plots existed.
    let bearings = if development.is_none() {
        FALLBACK_BEARINGS
    } else {
        SEEDED_BEARINGS
    };
    let mut best_resource_plots: Vec<(f32, Vec3, f32)> = Vec::new();
    let mut resource_rings_since_proof = 0_usize;
    let mut frontage_candidates = development
        .filter(|_| kind == SettlementBuildingKind::House)
        .map(|plan| {
            let connected_roads: Vec<_> = roads
                .iter()
                .copied()
                .filter(|road| {
                    road.is_complete()
                        && road.built_points().iter().any(|point| {
                            connected_road_keys
                                .contains(&crate::world::village_roads::road_point_key(*point))
                        })
                })
                .collect();
            house_frontage_candidates(
                plan,
                hall,
                neighbors,
                &connected_roads,
                min_radius,
                max_radius,
            )
        })
        .unwrap_or_default();
    if kind == SettlementBuildingKind::House {
        if let Some(urban) = urban {
            // Grow complete quarters around the established village. The
            // older 2–5-house rule remains a founding/infill fallback.
            let mut district_candidates = urban.candidates(hall, occupied, roads);
            district_candidates.retain(|candidate| {
                candidate.local.length() >= min_radius
                    && candidate.local.length() <= MAX_SETTLEMENT_SEARCH_RADIUS
            });
            district_candidates.append(&mut frontage_candidates);
            frontage_candidates = district_candidates;
        }
    }

    let own_square = squares
        .iter()
        .copied()
        .filter(|square| square.center.xz().distance(hall.xz()) < 80.0)
        .min_by(|a, b| {
            a.center
                .xz()
                .distance_squared(hall.xz())
                .total_cmp(&b.center.xz().distance_squared(hall.xz()))
        });
    if let Some(square) = own_square {
        let mut civic_candidates =
            super::civic_square::civic_frontage_candidates(square, hall, kind);
        civic_candidates.append(&mut frontage_candidates);
        frontage_candidates = civic_candidates;
    }

    let mut radius = min_radius;
    let mut rings_scanned = 0_usize;
    let mut candidates = Vec::with_capacity(FALLBACK_BEARINGS);
    while radius <= max_radius {
        let frontage_pass = !frontage_candidates.is_empty();
        candidates.clear();
        if frontage_pass {
            candidates.append(&mut frontage_candidates);
        } else {
            candidates.extend((0..bearings).map(|i| {
                // Offset each ring's bearings so successive rings do not line every
                // building up on the same spokes.
                let seeded_turn = development.map_or(0.0, |plan| {
                    ((shared::worldgen::splitmix64(plan.plan_seed).rotate_right(9) & 0xffff) as f32
                        / 65_535.0)
                        * 0.92
                });
                let ring_turn = (radius / RING_STEP) * 0.5;
                let turn = if development.is_none() {
                    // The open-land fallback refines each bounded outward ring to
                    // 48 bearings, closing the broad gaps left between the twelve
                    // preferred grammar directions without creating one large
                    // synchronous search.
                    i as f32 / bearings as f32 + ring_turn / SEEDED_BEARINGS as f32
                } else {
                    (i as f32 + ring_turn + seeded_turn) / SEEDED_BEARINGS as f32
                };
                let angle = turn * std::f32::consts::TAU;
                let base = Vec2::new(angle.cos(), angle.sin()) * radius;
                development.map_or(
                    PlannedPlotCandidate {
                        local: base,
                        frontage: Vec2::ZERO,
                    },
                    |plan| planned_plot_candidate(plan, kind, radius, i, base),
                )
            }));
        }
        if !frontage_pass
            && matches!(
                kind,
                SettlementBuildingKind::Bakery | SettlementBuildingKind::StorageHall
            )
        {
            candidates.sort_by(|a, b| {
                let hall2 = Vec2::new(hall.x, hall.z);
                let score = |point| {
                    affinity_score(kind, point, neighbors)
                        + urban.map_or(0.0, |urban| urban.land_use_score(kind, point))
                };
                score(hall2 + b.local).total_cmp(&score(hall2 + a.local))
            });
        }
        'candidate: for planned in candidates.iter().copied() {
            reject!(sampled);
            let local = planned.local;
            let x = hall.x + local.x;
            let z = hall.z + local.y;

            let ground = terrain.get_height(x, z);
            let candidate = Vec3::new(x, ground, z);
            // Prefer real completed frontage once a street exists. Before
            // that, face the implied street from the seed grammar. This is the
            // rotation used by water, field, collision, door, and road checks.
            let candidate2 = Vec2::new(x, z);
            let planned_frontage =
                Vec2::new(hall.x + planned.frontage.x, hall.z + planned.frontage.y);
            let completed_road_frontage = nearest_completed_road_frontage(candidate2, roads);
            // Once streets exist, cabins extend those streets. Letting a late
            // house fall back to a grammar-only frontage beyond the 48 m road
            // catchment can approve a perfectly buildable but permanently
            // isolated pocket. Resource workplaces remain allowed farther out
            // because soil, forest and shore quality legitimately outrank a
            // short connector for those plots.
            if kind == SettlementBuildingKind::House
                && roads.iter().any(|road| road.is_complete())
                && completed_road_frontage.is_none()
            {
                continue;
            }
            // An infill plot continues a short existing frontage, including
            // one legal street extension past its end. Snapping its direction
            // back to the nearest endpoint would turn each new facade into a
            // fan. The same access proof below still reserves its real road.
            let frontage = if frontage_pass {
                planned_frontage
            } else {
                completed_road_frontage.unwrap_or(planned_frontage)
            };
            let rotation = rotation_facing_frontage(candidate2, frontage);
            if defenses.is_some_and(|defenses| {
                super::reservations::plot_intersects_defenses(defenses, kind, candidate, rotation)
            }) {
                reject!(occupied);
                continue;
            }
            if squares
                .iter()
                .any(|square| square.blocks_plot(kind, candidate, rotation))
            {
                reject!(occupied);
                continue;
            }
            let earthwork_effort = if kind == SettlementBuildingKind::Farmstead {
                let Some(effort) = farmstead_earthwork_effort(terrain, candidate, rotation) else {
                    reject!(earthworks);
                    continue;
                };
                effort
            } else if kind == SettlementBuildingKind::LivestockFarm {
                let Some(effort) = livestock_earthwork_effort(terrain, candidate, rotation) else {
                    reject!(earthworks);
                    continue;
                };
                effort
            } else {
                if slope_at(terrain, x, z) > MAX_BUILD_SLOPE {
                    reject!(earthworks);
                    continue;
                }
                0.0
            };
            if !plot_fits_navigation_bounds(kind, candidate, rotation) {
                reject!(bounds);
                continue;
            }
            // Test every part of the rotated footprint and the authored door
            // against the LOCAL water surface. Comparing the centre to the
            // global ocean plane misses inland rivers entirely.
            if shared::components::minimum_building_water_clearance(
                terrain, candidate, kind, rotation,
            ) < FREEBOARD
            {
                reject!(water);
                continue;
            }
            if !crate::world::village_roads::doorway_road_apron_is_dry(
                terrain, kind, candidate, rotation,
            ) {
                reject!(water);
                continue;
            }
            if colliders.zip(derived).is_some_and(|(colliders, derived)| {
                !crate::world::village_roads::doorway_road_apron_is_clear_of_props(
                    kind, candidate, rotation, colliders, derived,
                )
            }) {
                reject!(props);
                continue;
            }
            let clashes = occupied.iter().any(|(other, other_clearance)| {
                let flat = Vec2::new(candidate.x - other.x, candidate.z - other.z).length();
                flat < clearance + other_clearance
            });
            if clashes {
                reject!(occupied);
                continue;
            }
            if let (Some(field_positions), Some(field_half)) = (
                kind.field_positions(candidate, rotation),
                kind.field_half_extents(),
            ) {
                for field in field_positions {
                    if shared::components::minimum_rotated_rect_water_clearance(
                        terrain,
                        field,
                        field_half + Vec2::splat(shared::components::FARM_FIELD_TERRACE_MARGIN),
                        rotation,
                    ) < FREEBOARD
                    {
                        reject!(water);
                        continue 'candidate;
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
                        reject!(props);
                        continue 'candidate;
                    }
                    let field_clearance =
                        field_half.length() + shared::components::FARM_FIELD_TERRACE_MARGIN;
                    if occupied.iter().any(|(other, other_clearance)| {
                        Vec2::new(field.x - other.x, field.z - other.z).length()
                            < field_clearance + other_clearance
                    }) {
                        reject!(occupied);
                        continue 'candidate;
                    }
                }
            }
            if let (Some(pasture), Some(half)) = (
                kind.pasture_position(candidate, rotation),
                kind.pasture_half_extents(),
            ) {
                if shared::components::minimum_rotated_rect_water_clearance(
                    terrain,
                    pasture,
                    half + Vec2::splat(2.0),
                    rotation,
                ) < FREEBOARD
                {
                    reject!(water);
                    continue 'candidate;
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
                    reject!(props);
                    continue 'candidate;
                }
                let pasture_clearance = half.length() + 2.0;
                if occupied.iter().any(|(other, other_clearance)| {
                    Vec2::new(pasture.x - other.x, pasture.z - other.z).length()
                        < pasture_clearance + other_clearance
                }) {
                    reject!(occupied);
                    continue 'candidate;
                }
            }
            let footprint_radius = kind.placement_definition().root_footprint_radius() + 0.45;
            if roads.iter().any(|road| {
                road.contains_reserved_point(Vec2::new(candidate.x, candidate.z), footprint_radius)
            }) {
                reject!(roads);
                continue;
            }
            if planned_accesses
                .iter()
                .any(|access| access.intersects_circle(candidate2, footprint_radius))
            {
                reject!(roads);
                continue;
            }
            if let (Some(field_positions), Some(field_half)) = (
                kind.field_positions(candidate, rotation),
                kind.field_half_extents(),
            ) {
                for field in field_positions {
                    let field_center = Vec2::new(field.x, field.z);
                    if roads.iter().any(|road| {
                        road.intersects_rotated_rect(
                            field_center,
                            field_half,
                            rotation,
                            shared::components::FARM_FIELD_TERRACE_MARGIN,
                        )
                    }) {
                        reject!(roads);
                        continue 'candidate;
                    }
                    if planned_accesses.iter().any(|access| {
                        access.intersects_circle(
                            field_center,
                            field_half.length() + shared::components::FARM_FIELD_TERRACE_MARGIN,
                        )
                    }) {
                        reject!(roads);
                        continue 'candidate;
                    }
                }
            }
            if let (Some(pasture), Some(half)) = (
                kind.pasture_position(candidate, rotation),
                kind.pasture_half_extents(),
            ) {
                let center = Vec2::new(pasture.x, pasture.z);
                if roads
                    .iter()
                    .any(|road| road.intersects_rotated_rect(center, half, rotation, 1.0))
                    || planned_accesses
                        .iter()
                        .any(|access| access.intersects_circle(center, half.length() + 1.0))
                {
                    reject!(roads);
                    continue 'candidate;
                }
            }
            if resource_scored {
                if !direct_road_access_is_coarsely_clear(
                    terrain,
                    hall,
                    kind,
                    candidate,
                    rotation,
                    roads,
                    access_blockers,
                    &connected_road_keys,
                ) {
                    reject!(roads);
                    continue;
                }
                // A layout grammar says which street this plot belongs to;
                // geography still decides whether a farm or timber workplace
                // is worth building there. A small travel penalty prevents a
                // negligible quality gain from sending the very first worker
                // to the edge of the full 120 m search band.
                let quality = site_placement_suitability(terrain, kind, candidate);
                let travel = candidate2.distance(Vec2::new(hall.x, hall.z));
                let builder_stand = shared::components::builder_stand_position(
                    candidate,
                    rotation,
                    kind.placement_definition().footprint.y,
                );
                // A completed street is already a certified land route. Prove
                // only the new frontage-to-door leg when one is nearby rather
                // than repeatedly searching all the way back to the hall. In
                // a mature riverside settlement the latter could run a large
                // A* for every permit candidate even though the existing road
                // already winds safely around the water.
                let route_origin =
                    completed_road_frontage.unwrap_or_else(|| Vec2::new(hall.x, hall.z));
                // Rank obviously same-landmass plots before remote soil. The
                // old six-entry shortlist compared soil first, so a fertile
                // meadow across a river could occupy every slot; all six then
                // failed the bounded route proof while hundreds of reachable
                // candidates were never examined. This coarse water probe is
                // only a ranking hint: the selected candidate receives the
                // exact 20 cm proof below, and bounded A* still admits an
                // indirect route around an inlet.
                let direct_land = crate::world::village_roads::road_segment_is_coarsely_dry(
                    terrain,
                    route_origin,
                    Vec2::new(builder_stand.x, builder_stand.z),
                );
                // A resource workplace occupies far more ground than a
                // cabin. Prefer plots whose authored door has a direct lane
                // to the existing network; admitting six boxed-in farms to a
                // bounded A* shortlist made one mature-settlement permit
                // spend over a second proving the same negative geometry.
                // Cabins may still reserve a bent lane, while farms and timber
                // huts continue outward until a clean frontage exists.
                let score =
                    quality * 100.0 - travel / max_radius.max(1.0) * 9.0 - earthwork_effort * 12.0
                        + if direct_land { 1_000.0 } else { 0.0 }
                        + affinity_score(kind, candidate2, neighbors);
                let score =
                    score + urban.map_or(0.0, |urban| urban.land_use_score(kind, candidate2));
                best_resource_plots.push((score, candidate, rotation));
                best_resource_plots.sort_by(|a, b| {
                    b.0.total_cmp(&a.0)
                        .then_with(|| a.1.x.total_cmp(&b.1.x))
                        .then_with(|| a.1.z.total_cmp(&b.1.z))
                });
                best_resource_plots.truncate(RESOURCE_PLOT_SHORTLIST);
            } else {
                if planned_road_access_path(
                    terrain,
                    hall,
                    kind,
                    candidate,
                    rotation,
                    roads,
                    access_blockers,
                    &connected_road_keys,
                )
                .is_none()
                {
                    reject!(access_or_work);
                    continue;
                }
                // This candidate already passed the authoritative full-width
                // access proof above; actor routing later certifies the
                // builder's changing obstacle world. A second independent
                // terrain route here is neither more authority nor a safe
                // mature-city tick cost.
                return Some((candidate, rotation));
            }
        }
        if frontage_pass {
            // The durable civic anchor is the Market site. A temporary blocked
            // approach must not silently build the Market elsewhere and leave
            // the public square permanently unused.
            if kind == SettlementBuildingKind::Market && own_square.is_some() {
                return None;
            }
            continue;
        }
        if resource_scored {
            resource_rings_since_proof += 1;
            if resource_rings_since_proof >= RESOURCE_SEARCH_BANDS {
                let proof_candidates = best_resource_plots.len() as u32;
                if let Some((_, candidate, rotation)) =
                    best_resource_plots
                        .iter()
                        .copied()
                        .find(|(_, candidate, rotation)| {
                            resource_plot_is_viable(
                                terrain, hall, kind, *candidate, *rotation, colliders, derived,
                            )
                        })
                {
                    return Some((candidate, rotation));
                }
                if let Some(rejections) = rejections.as_deref_mut() {
                    rejections.access_or_work =
                        rejections.access_or_work.saturating_add(proof_candidates);
                }
                // The best local choices can still fail the exact field-door,
                // tree or 20 cm road proof. Discard only this bounded batch
                // and continue with the next three rings; never mistake a bad
                // shortlist for proof that the whole settlement is full.
                best_resource_plots.clear();
                resource_rings_since_proof = 0;
            }
        }
        rings_scanned += 1;
        if maximum_search_rings.is_some_and(|maximum| rings_scanned >= maximum) {
            break;
        }
        radius += RING_STEP;
    }
    let proof_candidates = best_resource_plots.len() as u32;
    let selected = best_resource_plots
        .into_iter()
        .find(|(_, candidate, rotation)| {
            resource_plot_is_viable(
                terrain, hall, kind, *candidate, *rotation, colliders, derived,
            )
        })
        .map(|(_, candidate, rotation)| (candidate, rotation));
    if selected.is_none() {
        if let Some(rejections) = rejections.as_deref_mut() {
            rejections.access_or_work = rejections.access_or_work.saturating_add(proof_candidates);
        }
    }

    if selected.is_none() && development.is_some() {
        // A charter is a preference, never a hard buildable boundary. Dense
        // seeded spokes or clusters can eventually consume every candidate in
        // their small grammar even though suitable, reachable land remains
        // between them. Retry the same deterministic physical checks on an
        // unrestricted radial sweep before telling the settlement it has no
        // viable plot. Completed roads still determine frontage and every
        // water, prop, field, collision and route rule remains authoritative.
        return find_site_with_plan_diagnostics(
            terrain,
            hall,
            kind,
            occupied,
            neighbors,
            roads,
            planned_accesses,
            access_blockers,
            None,
            colliders,
            derived,
            minimum_radius_hint,
            maximum_search_rings,
            rejections,
            None,
            defenses,
            squares,
        );
    }

    selected
}

#[cfg(test)]
mod neighborhood_integration_tests {
    use super::*;
    use shared::components::{SettlementDevelopment, SettlementLayoutStyle};

    #[test]
    fn grid_can_fill_adjacent_frontages_and_opposing_rear_yards() {
        let mut plan = SettlementDevelopment::from_foundation("Grid", Vec3::ZERO, 0);
        plan.plan_seed = 0;
        plan.layout = SettlementLayoutStyle::Grid;
        plan.center = shared::components::SettlementCenterStyle::Square;
        let (axis, side) = plan_axis(&plan);
        let locations = [
            Vec2::new(0.0, 1.0),
            Vec2::new(13.0, 1.0),
            Vec2::new(0.0, 28.0),
        ]
        .map(|base| {
            planned_plot_candidate(
                &plan,
                SettlementBuildingKind::House,
                30.0,
                0,
                axis * base.x + side * base.y,
            )
            .local
        });
        for (i, location) in locations.iter().enumerate() {
            assert!(locations[..i].iter().all(|other| other.distance(*location) >= SettlementBuildingKind::House.clearance() * 2.0),
                "nominally adjacent grid slots must not fail their own house reservations: {locations:?}");
        }
    }

    #[test]
    fn house_infill_uses_real_access_and_cannot_overwrite_reserved_ground() {
        let terrain = WorldTerrain::default();
        let hall = Vec3::new(1720.0, terrain.get_height(1720.0, 0.0), 0.0);
        let house = PlotNeighbor {
            kind: SettlementBuildingKind::House,
            position: hall + Vec3::X * 40.0,
            rotation: 0.0,
        };
        let other_house = PlotNeighbor {
            position: hall + Vec3::X * 68.0,
            ..house
        };
        let neighbors = [house, other_house];
        let mut plan = SettlementDevelopment::from_foundation("Infill", hall, 0);
        plan.plan_seed = 2;
        let door = SettlementBuildingKind::Hall.entrance_position(hall, 0.0);
        let mut street_points = vec![Vec2::new(door.x, door.z)];
        street_points.extend((0..=20).map(|i| Vec2::new(hall.x + i as f32 * 4.0, -12.0)));
        let road = VillageRoad {
            settlement: "Infill".into(),
            builder: "Mara".into(),
            built_through: street_points.len() as u16,
            points: street_points,
            width: crate::world::village_roads::VILLAGE_ROAD_WIDTH,
            reserved_width: shared::components::RoadClass::Lane.initial_reserved_width(),
            surface: default(),
            class: default(),
            stone_committed: 0,
        };
        let mut occupied = vec![
            (hall, SettlementBuildingKind::Hall.clearance()),
            (house.position, house.kind.clearance()),
            (other_house.position, other_house.kind.clearance()),
        ];
        let blockers: Vec<_> = neighbors
            .iter()
            .flat_map(|neighbor| {
                super::super::road_access::road_access_blockers_for_plot(
                    neighbor.kind,
                    neighbor.position,
                    neighbor.rotation,
                )
            })
            .collect();
        let find = |occupied: &[(Vec3, f32)], context: &[PlotNeighbor]| {
            find_site_with_plan_diagnostics(
                &terrain,
                hall,
                house.kind,
                occupied,
                context,
                &[&road],
                &[],
                &blockers,
                Some(&plan),
                None,
                None,
                None,
                None,
                None,
                None,
                None,
                &[],
            )
            .expect("the open existing street has legal adjacent frontage")
        };
        let (first, rotation) = find(&occupied, &neighbors);
        let (without_context, _) = find(&occupied, &[]);
        assert!(first.distance(house.position) >= house.kind.clearance() * 2.0);
        assert!(
            first.distance(house.position) < 14.0,
            "prefer adjacent frontage over unrelated open ring samples"
        );
        assert!((first.z - house.position.z).abs() < 0.01);
        assert!(
            rotation.abs() < 0.01,
            "new homes retain the same street-facing facade"
        );
        assert!(
            without_context.z.abs() > first.z.abs() + 0.5
                || without_context.distance(house.position) > first.distance(house.position) + 0.5,
            "typed infill should improve actual accepted adjacency over grammar alone: infill={first:?}, grammar={without_context:?}"
        );
        let reversed = [other_house, house];
        assert_eq!((first, rotation), find(&occupied, &reversed));

        // Fields and other reservations need not themselves be a house to
        // invalidate a promising infill candidate. The shared proof remains
        // authoritative over the new neighbourhood preference.
        occupied.push((first, 8.0));
        let (next, _) = find(&occupied, &neighbors);
        assert!(next.distance(first) >= 8.0 + house.kind.clearance());
        assert!(next.distance(house.position) >= house.kind.clearance() * 2.0);
    }

    #[test]
    fn polycentric_grammar_keeps_room_for_outer_neighborhoods_before_fallback() {
        for seed in [3, 7, 19, 41] {
            let mut plan = SettlementDevelopment::from_foundation("Growth", Vec3::ZERO, 0);
            plan.plan_seed = seed;
            plan.layout = SettlementLayoutStyle::Polycentric;
            let mut accepted: Vec<Vec2> = Vec::new();
            for radius in (72..=180).step_by(6) {
                for bearing in 0..12 {
                    let candidate = planned_plot_candidate(
                        &plan,
                        SettlementBuildingKind::House,
                        radius as f32,
                        bearing,
                        Vec2::ZERO,
                    );
                    if candidate.local.length() > 75.0
                        && accepted
                            .iter()
                            .all(|other| other.distance(candidate.local) >= house_frontage_pitch())
                    {
                        accepted.push(candidate.local);
                    }
                }
            }
            assert!(
                accepted.len() >= 18,
                "seed {seed} exhausts its founding pockets instead of offering outer streets: {}",
                accepted.len()
            );
            assert!(accepted.iter().any(|point| point.length() > 110.0));
        }
    }
}
