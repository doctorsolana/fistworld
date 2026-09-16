//! Bounded local fitting of the authored T-shaped harbour. Civic planning
//! separately certifies its dry work-point connection and permanent obstacles.
use super::{
    clearance::{WaterNavigationGeometry, WatercraftClearance},
    dry_at, water_at,
};
use bevy::prelude::*;
use shared::{components::*, terrain::WorldTerrain};

/// Six lengths and two departure sides on one supplied dry shore. The office,
/// shore landing and T-head retain their authored size; only the pier stretches.
pub(crate) fn survey_port_berth(
    terrain: &WorldTerrain,
    geometry: &WaterNavigationGeometry,
    shore_hint: Vec2,
    seaward: Vec2,
    kind: ShipKind,
) -> Option<PortGeometry> {
    survey_port_berth_observed(terrain, geometry, shore_hint, seaward, kind, |_| {})
}

pub(crate) fn survey_port_berth_observed(
    terrain: &WorldTerrain,
    geometry: &WaterNavigationGeometry,
    shore_hint: Vec2,
    seaward: Vec2,
    kind: ShipKind,
    mut rejected: impl FnMut(&'static str),
) -> Option<PortGeometry> {
    if !shore_hint.is_finite() || !seaward.is_finite() || seaward.length_squared() < 0.01 {
        return None;
    }
    let seaward = seaward.normalize();
    let side = Vec2::new(-seaward.y, seaward.x);
    // The broad landing is tested once before trying longer over-water piers.
    let Some((deck, _)) = landing_height_range(terrain, shore_hint, seaward) else {
        rejected("shore landing");
        return None;
    };
    let shore = Vec3::new(shore_hint.x, deck, shore_hint.y);
    let hull = WatercraftClearance::for_ship(kind);
    for distance in [20.0, 26.0, 34.0, 44.0, 56.0, 72.0] {
        let end = shore_hint + seaward * distance;
        let Some(water) = water_at(terrain, end) else {
            rejected("head not over water");
            continue;
        };
        for sign in [-1.0, 1.0] {
            // Boat length lies along the 16 m head. Circular navigation still
            // reserves the full half-diagonal, not just half the vessel beam.
            let berth = end + seaward * (hull.radius + PORT_BERTH_GAP);
            let outward = side * sign;
            let departure =
                berth + outward * (PORT_HEAD_WIDTH * 0.5 + hull.radius + PORT_BERTH_GAP);
            let (Some(berth_y), Some(departure_y)) =
                (water_at(terrain, berth), water_at(terrain, departure))
            else {
                rejected("berth or departure not water");
                continue;
            };
            let proposed = PortGeometry {
                shore,
                pier_end: Vec3::new(end.x, water + 1.0, end.y),
                berth: Vec3::new(berth.x, berth_y, berth.y),
                departure: Vec3::new(departure.x, departure_y, departure.y),
                yaw: (-outward.x).atan2(-outward.y),
                maximum_ship: kind,
            };
            if let Some(reason) = port_geometry_refusal(terrain, geometry, &proposed) {
                rejected(reason);
            } else {
                return Some(proposed);
            }
        }
    }
    None
}

/// Minimum clear platform plane and the highest physically supported plane.
/// The deck sits above all sampled ground, rather than burying its flags and
/// office floor into a slope. A low entry step and finite stone foundation
/// bound adaptation; steep/wet land still needs a different site.
fn landing_height_range(terrain: &WorldTerrain, shore: Vec2, sea: Vec2) -> Option<(f32, f32)> {
    if !shore.is_finite() || !sea.is_finite() || !dry_at(terrain, shore) {
        return None;
    }
    let center = terrain.get_height(shore.x, shore.y);
    let mut minimum = center;
    let mut maximum = center;
    let right = Vec2::new(-sea.y, sea.x);
    // <=1 m grid includes all four exact edges; centre is included above.
    for x in 0..=15 {
        for z in 0..=8 {
            let p = shore
                + right * (-8.4 + PORT_SHORE_WIDTH * x as f32 / 15.)
                + sea * (-5.4 + PORT_SHORE_DEPTH * z as f32 / 8.);
            if !dry_at(terrain, p) {
                return None;
            }
            let height = terrain.get_height(p.x, p.y);
            minimum = minimum.min(height);
            maximum = maximum.max(height);
            if maximum + 0.03 > (center + 0.30).min(minimum + 0.65) {
                return None;
            }
        }
    }
    let plane = maximum + 0.03;
    let ceiling = (center + 0.30).min(minimum + 0.65);
    (plane <= ceiling).then_some((plane, ceiling))
}

fn dry_landing(terrain: &WorldTerrain, shore: Vec3, sea: Vec2) -> bool {
    shore.is_finite()
        && landing_height_range(terrain, shore.xz(), sea)
            .is_some_and(|(minimum, maximum)| shore.y >= minimum - 0.005 && shore.y <= maximum)
}

/// Approval and launch share the complete authored footprint, full-hull sea
/// corridor and the actual walking-height profile. The own port is excluded
/// only from static structure overlap; ships still cannot enter its footprint.
pub(crate) fn port_geometry_valid(
    terrain: &WorldTerrain,
    geometry: &WaterNavigationGeometry,
    port: &PortGeometry,
) -> bool {
    port_geometry_refusal(terrain, geometry, port).is_none()
}

fn port_geometry_refusal(
    terrain: &WorldTerrain,
    geometry: &WaterNavigationGeometry,
    port: &PortGeometry,
) -> Option<&'static str> {
    if !port.valid() {
        return Some("invalid geometry");
    }
    if !dry_landing(terrain, port.shore, port.seaward()) {
        return Some("shore landing");
    }
    if !geometry.port_footprint_clear(port) {
        return Some("existing water structure");
    }
    let Some(water) = water_at(terrain, port.pier_end.xz()) else {
        return Some("head not over water");
    };
    let length = port.length();
    if (port.pier_end.y - (water + 1.)).abs() > 0.1
        || (port.pier_end.y - port.shore.y).abs() / (length - PORT_HEAD_DEPTH - PORT_SHORE_FRONT)
            > 0.20
    {
        return Some("deck slope or water height");
    }
    // All the approach and broad loading head must clear the ground, be above
    // flood water, and have a reachable seabed for their fitted support piles.
    for rect in port.footprints().into_iter().skip(1) {
        let steps = (rect.half_extents * 2.).ceil().as_uvec2().max(UVec2::ONE);
        let rotation = Quat::from_rotation_y(rect.yaw);
        for x in 0..=steps.x {
            for z in 0..=steps.y {
                let local = (Vec2::new(x as f32 / steps.x as f32, z as f32 / steps.y as f32) * 2.
                    - Vec2::ONE)
                    * rect.half_extents;
                let p = rect.center + (rotation * local.extend(0.).xzy()).xz();
                if !terrain.generator.active_map_bounds().contains_xz(p.x, p.y) {
                    return Some("footprint outside map");
                }
                let along = (p - port.shore.xz()).dot(port.seaward());
                let deck = port.deck_height(along);
                let ground = terrain.get_height(p.x, p.y);
                if ground > deck - 0.015 {
                    return Some("ground above deck");
                }
                if deck - ground > 12. {
                    return Some("piles too deep");
                }
                if water_at(terrain, p).is_some_and(|water| water > deck - 0.5) {
                    return Some("deck too near water");
                }
            }
        }
    }
    let hull = WatercraftClearance::for_ship(port.maximum_ship);
    let along = (port.berth.xz() - port.pier_end.xz()).dot(port.seaward());
    if along < hull.radius + PORT_BERTH_GAP - 0.01 {
        return Some("berth too near head");
    }
    let direction = (port.departure - port.berth).xz().normalize_or_zero();
    if direction.dot(port.seaward()).abs() > 0.01 {
        return Some("departure not alongside");
    }
    let heading = Vec2::new(-port.yaw.sin(), -port.yaw.cos());
    if heading.dot(direction) < 0.99 {
        return Some("ship heading not alongside");
    }
    let steps = port.berth.xz().distance(port.departure.xz()).ceil() as usize;
    for i in 0..=steps {
        let p = port
            .berth
            .xz()
            .lerp(port.departure.xz(), i as f32 / steps as f32);
        if port.water_obstructs(p, hull.radius) {
            return Some("hull intersects own port");
        }
    }
    for p in [port.berth, port.departure] {
        if water_at(terrain, p.xz()).is_none_or(|y| (y - p.y).abs() > 0.1) {
            return Some("ship not at water height");
        }
    }
    if !geometry.segment_clear(terrain, port.berth.xz(), port.departure.xz(), hull) {
        return Some("berth full hull corridor");
    }
    None
}
