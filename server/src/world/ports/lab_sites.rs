//! Initial-fixture site selection only. This scan is never ordinary port policy.

use crate::player::boat::{
    WaterPlanResult, WaterRouteCache, berth,
    clearance::{WaterNavigationGeometry, WatercraftClearance},
};
use crate::world::village_roads::road_corridor_is_dry;
use bevy::prelude::*;
use shared::{components::*, terrain::WorldTerrain};

fn footprints(site: &(Vec3, f32, PortGeometry)) -> [PortFootprint; 4] {
    let [shore, walk, head] = site.2.footprints();
    [
        PortFootprint {
            center: CivicHallLevel::reserved_world_center(site.0, site.1),
            half_extents: CivicHallLevel::reserved_half_extents(),
            yaw: site.1,
        },
        shore,
        walk,
        head,
    ]
}

fn sites_disjoint(a: &(Vec3, f32, PortGeometry), b: &(Vec3, f32, PortGeometry)) -> bool {
    footprints(a).into_iter().all(|a| {
        footprints(b).into_iter().all(|b| {
            !oriented_rects_overlap(
                a.center,
                a.half_extents + Vec2::splat(1.),
                a.yaw,
                b.center,
                b.half_extents + Vec2::splat(1.),
                b.yaw,
            )
        })
    })
}

/// Fixture setup alone may wait on a bounded ordinary search. A curved coast
/// must not fail merely because the two harbours cannot see one another in a
/// straight line. Include both future piers in the exact same water geometry.
fn certified_connection(
    terrain: &WorldTerrain,
    geometry: &WaterNavigationGeometry,
    first: PortGeometry,
    second: PortGeometry,
) -> Result<Vec<Vec2>, &'static str> {
    let mut cache = WaterRouteCache::default();
    cache.geometry = geometry.with_proposed_ports([first, second]);
    let mut search = cache.begin_for(
        terrain,
        first.departure.xz(),
        second.departure.xz(),
        WatercraftClearance::for_ship(ShipKind::Coaster),
    );
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    for _ in 0..10_000 {
        match cache.advance(&mut search, terrain) {
            WaterPlanResult::Complete(Some(route)) => return Ok(route),
            WaterPlanResult::Complete(None) => return Err("pair water route unreachable"),
            WaterPlanResult::Pending => {}
        }
        if std::time::Instant::now() >= deadline {
            break;
        }
    }
    Err("pair water planning budget")
}

/// Two completed-port fixture sites on actual generated ocean frontage, joined
/// by a class-certified water corridor. No geometry or runtime routes are changed.
/// The caller still checks permanent live obstacles before initially spawning.
pub(super) fn select(
    terrain: &WorldTerrain,
    geometry: &WaterNavigationGeometry,
) -> Option<[(Vec3, f32, PortGeometry); 2]> {
    let map = terrain.generator.loaded_map();
    let ocean = map.definition.terrain.water_level?;
    let bounds = terrain.generator.active_map_bounds();
    let mut candidates: Vec<(Vec3, f32, PortGeometry)> = Vec::new();
    let mut refusals = std::collections::BTreeMap::<&'static str, usize>::new();
    let trace = cfg!(test) || std::env::var_os("FISTWORLD_PORT_SITE_DIAGNOSTICS").is_some();
    let mut surveyed = 0;
    let mut x = bounds.min[0] + 64.;
    while x <= bounds.max[0] - 64. && candidates.len() < 96 {
        let mut z = bounds.min[1] + 64.;
        while z <= bounds.max[1] - 64. && candidates.len() < 96 {
            let point = Vec2::new(x, z);
            let height = terrain.get_height(x, z);
            if height > ocean + 0.4 && height < ocean + 4. {
                for seaward in [Vec2::X, Vec2::NEG_X, Vec2::Y, Vec2::NEG_Y] {
                    // The authored pier can span 20–72 m. A legacy "wet at
                    // 12 m" shortcut excluded dry inland landing foundations.
                    let Some(port) = berth::survey_port_berth_observed(
                        terrain,
                        geometry,
                        point,
                        seaward,
                        ShipKind::Coaster,
                        |reason| *refusals.entry(reason).or_default() += 1,
                    ) else {
                        continue;
                    };
                    surveyed += 1;
                    if trace && surveyed <= 8 {
                        eprintln!("PORT_SITE surveyed={port:?}");
                    }
                    if candidates
                        .iter()
                        .any(|(_, _, other)| other.shore.distance(port.shore) < 36.)
                    {
                        *refusals.entry("duplicate shore").or_default() += 1;
                        continue;
                    }
                    let hall_xz = point - seaward * 30.;
                    let hall = Vec3::new(
                        hall_xz.x,
                        terrain.get_height(hall_xz.x, hall_xz.y),
                        hall_xz.y,
                    );
                    let yaw = (-seaward.x).atan2(-seaward.y);
                    let pickup = SettlementBuildingKind::Hall.entrance_position(hall, yaw);
                    if settlement_founding_refusal(terrain, hall, None).is_some() {
                        *refusals.entry("Hall founding").or_default() += 1;
                        continue;
                    }
                    if !road_corridor_is_dry(terrain, &[pickup.xz(), point], 2.6) {
                        *refusals.entry("Hall access").or_default() += 1;
                        continue;
                    }
                    // Existing authored trees stay in the shared recipe; select
                    // a clear initial Hall and access strip instead of deleting them.
                    if map.definition.objects.iter().any(|object| {
                        let p = Vec2::new(object.position[0], object.position[2]);
                        let line = point - pickup.xz();
                        let t = ((p - pickup.xz()).dot(line) / line.length_squared().max(0.01))
                            .clamp(0., 1.);
                        p.distance(hall.xz()) < 22.
                            || p.distance(pickup.xz() + line * t) < 4.
                            || port
                                .footprints()
                                .into_iter()
                                .any(|rect| rect.distance_squared(p) < 4.)
                    }) {
                        *refusals.entry("authored tree").or_default() += 1;
                        continue;
                    }
                    let candidate = (hall, yaw, port);
                    if trace {
                        eprintln!("PORT_SITE accepted Hall={hall:?} hall_yaw={yaw} port={port:?}");
                    }
                    // Return only a short, genuinely navigable fixture voyage;
                    // subsequent sailing still uses the ordinary retained planner.
                    for previous in &candidates {
                        let distance = previous.0.distance(hall);
                        if !(80. ..=480.).contains(&distance) {
                            *refusals.entry("pair separation").or_default() += 1;
                            continue;
                        }
                        if !sites_disjoint(previous, &candidate) {
                            *refusals
                                .entry("Hall or port footprints overlap")
                                .or_default() += 1;
                            continue;
                        }
                        match certified_connection(terrain, geometry, previous.2, port) {
                            Ok(route) => {
                                if trace {
                                    eprintln!(
                                        "PORT_SITE success surveyed={surveyed} candidates={} route={route:?} rejected={refusals:?}",
                                        candidates.len()
                                    );
                                }
                                return Some([*previous, candidate]);
                            }
                            Err(reason) => *refusals.entry(reason).or_default() += 1,
                        }
                    }
                    candidates.push(candidate);
                    break;
                }
            }
            z += 8.;
        }
        x += 8.;
    }
    if trace {
        eprintln!(
            "PORT_SITE exhausted surveyed={surveyed} candidates={} rejected={refusals:?}",
            candidates.len()
        );
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "actual village_lab harbour asset fitting diagnostic"]
    fn authored_port_lab_has_two_full_footprint_certified_sites() {
        let mut terrain = WorldTerrain::default();
        terrain.generator = shared::terrain::TerrainGenerator::from_loaded_map(
            shared::map::load_map("village_lab").unwrap(),
        );
        let geometry = WaterNavigationGeometry::default();
        let short_capture_port = berth::survey_port_berth(
            &terrain,
            &geometry,
            Vec2::new(248., 152.),
            Vec2::X,
            ShipKind::Coaster,
        )
        .expect("the maintained short-pier capture has a fitted broad landing");
        println!("AUTHORED_PORT_SHORT_CAPTURE geometry={short_capture_port:?}");
        let sites = select(&terrain, &geometry).expect("two real broad-landing T-head ports");
        assert!(sites_disjoint(&sites[0], &sites[1]));
        for (hall, yaw, port) in sites {
            assert!(berth::port_geometry_valid(&terrain, &geometry, &port));
            println!("AUTHORED_PORT Hall={hall:?} hall_yaw={yaw} geometry={port:?}");
        }
        let route = certified_connection(&terrain, &geometry, sites[0].2, sites[1].2)
            .expect("ordinary retained full-hull route between actual harbours");
        assert_eq!(route.last(), Some(&sites[1].2.departure.xz()));
        let complete_geometry = geometry.with_proposed_ports([sites[0].2, sites[1].2]);
        let mut previous = sites[0].2.departure.xz();
        for point in route {
            assert!(complete_geometry.segment_clear(
                &terrain,
                previous,
                point,
                WatercraftClearance::for_ship(ShipKind::Coaster),
            ));
            previous = point;
        }
    }
}
