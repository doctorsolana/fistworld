//! Bounded shoreline searches and resumed fishing-site certification.

use super::terrain::{plot_fits_navigation_bounds, slope_at, MAX_BUILD_SLOPE};
use crate::world::village::*;

/// Find a dry Fisherman's Hut plot whose authored rear pier reaches genuine
/// open water.
///
/// This is intentionally geometry-led rather than biome-led. A northern rock
/// coast and a southern dry coast are both viable if there is a safe hut pad,
/// a dry route around the hut, and water beneath the working end of the pier.
/// The returned rotation points the hut's local +Z (its `Anchor_Pier` side)
/// seaward.
pub fn find_fishing_site(
    terrain: &WorldTerrain,
    hall: Vec3,
    occupied: &[(Vec3, f32)],
    roads: &[&VillageRoad],
) -> Option<(Vec3, f32, f32)> {
    find_fishing_site_with_limits(terrain, hall, occupied, roads, None, None)
}

pub(super) fn find_fishing_site_with_limits(
    terrain: &WorldTerrain,
    hall: Vec3,
    occupied: &[(Vec3, f32)],
    roads: &[&VillageRoad],
    minimum_radius_hint: Option<f32>,
    maximum_search_rings: Option<usize>,
) -> Option<(Vec3, f32, f32)> {
    const BEARINGS: usize = 24;
    const FACINGS: usize = 24;
    const RING_STEP: f32 = 4.0;

    let water = terrain.water_level()?;
    let kind = SettlementBuildingKind::FishermansHut;
    let (min_radius, max_radius) = kind.preferred_ring();
    let clearance = kind.clearance();
    let mut radius = minimum_radius_hint
        .map(|hint| hint.clamp(min_radius, max_radius))
        .unwrap_or(min_radius);
    let mut rings_scanned = 0_usize;

    while radius <= max_radius {
        let mut best_at_radius: Option<(Vec3, f32, f32)> = None;
        for i in 0..BEARINGS {
            let turn = (i as f32 + (radius / RING_STEP) * 0.5) / BEARINGS as f32;
            let angle = turn * std::f32::consts::TAU;
            let x = hall.x + angle.cos() * radius;
            let z = hall.z + angle.sin() * radius;
            if slope_at(terrain, x, z) > MAX_BUILD_SLOPE {
                continue;
            }
            let ground = terrain.get_height(x, z);
            let candidate = Vec3::new(x, ground, z);
            if occupied.iter().any(|(other, other_clearance)| {
                Vec2::new(candidate.x - other.x, candidate.z - other.z).length()
                    < clearance + other_clearance
            }) {
                continue;
            }
            let footprint_radius = kind.placement_definition().root_footprint_radius() + 0.45;
            if roads.iter().any(|road| {
                road.contains_reserved_point(Vec2::new(candidate.x, candidate.z), footprint_radius)
            }) {
                continue;
            }

            for facing in 0..FACINGS {
                let rotation = facing as f32 / FACINGS as f32 * std::f32::consts::TAU;
                if !plot_fits_navigation_bounds(kind, candidate, rotation) {
                    continue;
                }
                if shared::components::minimum_building_water_clearance(
                    terrain, candidate, kind, rotation,
                ) < shared::components::SETTLEMENT_FREEBOARD
                {
                    continue;
                }
                if !crate::world::village_roads::doorway_road_apron_is_dry(
                    terrain, kind, candidate, rotation,
                ) {
                    continue;
                }

                // The side-route anchor is where a fisher rounds the solid
                // building on the way from its front door to its rear pier.
                // It must remain dry and reasonably level with the hut pad.
                let Some(nets) = kind.nets_position(candidate, rotation) else {
                    continue;
                };
                let nets_ground = terrain.get_height(nets.x, nets.z);
                if nets_ground < water + 0.15 || (nets_ground - ground).abs() > 1.6 {
                    continue;
                }

                let Some(fish_spot) = kind.fishing_position(candidate, rotation) else {
                    continue;
                };
                let quality = fishing_water_quality(terrain, fish_spot, rotation, water);
                if quality <= 0.0 {
                    continue;
                }
                let stand = shared::components::builder_stand_position(
                    candidate,
                    rotation,
                    kind.placement_definition().footprint.y,
                );
                if !crate::world::village_roads::embodied_land_route_exists(terrain, hall, stand) {
                    continue;
                }
                let replace = best_at_radius
                    .as_ref()
                    .is_none_or(|(_, _, best_quality)| quality > *best_quality);
                if replace {
                    best_at_radius = Some((candidate, rotation, quality));
                }
            }
        }
        if best_at_radius.is_some() {
            return best_at_radius;
        }
        rings_scanned += 1;
        if maximum_search_rings.is_some_and(|maximum| rings_scanned >= maximum) {
            break;
        }
        radius += RING_STEP;
    }
    None
}

/// Spend at most one shoreline ring per live permit decision and resume on the
/// next permit tick. Once every ring is exhausted, only a terrain edit can
/// create new coast; monotonically added houses and roads cannot make an
/// occupied shoreline freer than it was before.
pub(super) fn find_incremental_fishing_site(
    terrain: &WorldTerrain,
    hall: Vec3,
    occupied: &[(Vec3, f32)],
    roads: &[&VillageRoad],
    settlement: Entity,
    clock: &mut VillageClock,
) -> Option<(Vec3, f32, f32)> {
    const RING_STEP: f32 = 4.0;
    let kind = SettlementBuildingKind::FishermansHut;
    let (min_radius, max_radius) = kind.preferred_ring();
    let terrain_version = terrain.modification_version();
    if clock
        .failed_fishing_terrain_versions
        .get(&settlement)
        .is_some_and(|failed_version| *failed_version == terrain_version)
    {
        return None;
    }
    if clock
        .failed_fishing_terrain_versions
        .remove(&settlement)
        .is_some()
    {
        clock
            .site_search_radii
            .insert((settlement, kind), min_radius);
    }
    if terrain.water_level().is_none() {
        clock
            .failed_fishing_terrain_versions
            .insert(settlement, terrain_version);
        return None;
    }
    let radius = clock
        .site_search_radii
        .get(&(settlement, kind))
        .copied()
        .unwrap_or(min_radius)
        .clamp(min_radius, max_radius);
    let site = find_fishing_site_with_limits(terrain, hall, occupied, roads, Some(radius), Some(1));
    if site.is_some() {
        clock.site_search_radii.insert((settlement, kind), radius);
        clock.failed_fishing_terrain_versions.remove(&settlement);
    } else if radius < max_radius {
        clock
            .site_search_radii
            .insert((settlement, kind), (radius + RING_STEP).min(max_radius));
    } else {
        clock
            .failed_fishing_terrain_versions
            .insert(settlement, terrain_version);
    }
    site
}

/// Rejecting one otherwise valid shoreline plot must not poison the whole
/// coast. Advance to the next bounded ring after its final road/access proof
/// fails; `false` means the rejected plot was already on the last ring.
pub(super) fn advance_incremental_fishing_search(
    clock: &mut VillageClock,
    settlement: Entity,
) -> bool {
    const RING_STEP: f32 = 4.0;
    let kind = SettlementBuildingKind::FishermansHut;
    let (minimum, maximum) = kind.preferred_ring();
    let cursor = clock
        .site_search_radii
        .entry((settlement, kind))
        .or_insert(minimum);
    if *cursor >= maximum {
        return false;
    }
    *cursor = (*cursor + RING_STEP).min(maximum);
    clock.failed_fishing_terrain_versions.remove(&settlement);
    true
}

/// Score water around the working end of the authored pier. Zero means the
/// three seaward samples are not all submerged, so the layout would visibly
/// terminate on land. Non-zero values reward deeper, broader water without
/// making oceans categorically better than rivers or lakes.
pub(super) fn fishing_water_quality(
    terrain: &WorldTerrain,
    fish_spot: Vec3,
    rotation: f32,
    water: f32,
) -> f32 {
    let mut depth_score = 0.0;
    let mut samples = 0.0;
    for forward in [-1.0_f32, 0.75, 2.5] {
        for side in [-1.4_f32, 0.0, 1.4] {
            let offset = shared::rotation::local_to_world_xz(Vec2::new(side, forward), rotation);
            let ground = terrain.get_height(fish_spot.x + offset.x, fish_spot.z + offset.y);
            let depth = water - ground;
            // The outer row is load-bearing: a pier whose tip merely touches a
            // shallow puddle is not a fishing site.
            if forward >= 2.5 && depth < 0.18 {
                return 0.0;
            }
            depth_score += (depth / 2.5).clamp(0.0, 1.0);
            samples += 1.0;
        }
    }
    // Require the actual authored standing point to be above water, too.
    let tip_depth = water - terrain.get_height(fish_spot.x, fish_spot.z);
    if tip_depth < 0.12 {
        return 0.0;
    }
    (0.35 + 0.65 * depth_score / samples).clamp(0.35, 1.0)
}
