//! Spread fully validated communities across inhabitable land. Each connected
//! group has a certified coastal gateway; disconnected groups cannot promise
//! overland freight to one another.

use super::*;
use crate::world::village_roads::overland_trade_corridor_exists;
use std::collections::{HashMap, HashSet};

const NEIGHBOUR_REACH: f32 = 2400.0;
const COASTAL_REACH: f32 = 600.0;

type CorridorCache = HashMap<(u64, u64), bool>;

fn corridor(
    terrain: &WorldTerrain,
    a: &sites::Site,
    b: &sites::Site,
    cache: &mut CorridorCache,
) -> bool {
    let key = (a.salt.min(b.salt), a.salt.max(b.salt));
    *cache
        .entry(key)
        .or_insert_with(|| overland_trade_corridor_exists(terrain, a.hall.xz(), b.hall.xz()))
}

/// Physical coverage remains valuable beyond the first local neighbourhood.
/// The old penalty after 1.8 km made the opposite half of the world lose to
/// another nearby site, even when it had its own valid coastal arrival.
fn coverage_score(site: &sites::Site, nearest: f32, timber: f32, stone: f32) -> f32 {
    let opportunity =
        (site.resources.wood - timber).max(0.0) + (site.resources.stone - stone).max(0.0);
    nearest / 1000.0 + site.rank * 0.60 + opportunity * 0.25
}

pub(super) fn plan_world(
    terrain: &WorldTerrain,
    library: &DerivedColliderLibrary,
    seed: u64,
    config: &crate::world::start_config::WorldStartConfig,
) -> Result<Vec<Community>, String> {
    let mut candidates = sites::survey(terrain, seed);
    let surveyed = candidates.len();
    let voyages = sites::approaches(terrain, seed);
    let regions = connectivity::LandRegions::survey(terrain);
    let labels: HashMap<_, _> = candidates
        .iter()
        .map(|site| (site.salt, regions.at(terrain, site.hall.xz())))
        .collect();
    let shore_distance: HashMap<_, _> = candidates
        .iter()
        .map(|site| {
            let distance = voyages
                .iter()
                .map(|v| v.landing.xz().distance(site.hall.xz()))
                .fold(f32::INFINITY, f32::min);
            (site.salt, distance)
        })
        .collect();
    let coastal_regions: HashSet<_> = candidates
        .iter()
        .filter(|site| shore_distance[&site.salt] <= COASTAL_REACH)
        .map(|site| labels[&site.salt])
        .filter(|label| *label != 0)
        .collect();
    if coastal_regions.is_empty() {
        return Err("No inhabitable land region has a coastal approach".into());
    }
    // A coarse label only shortlists possible land connections. Keep EVERY
    // eligible region; selecting the largest alone stranded most of the map.
    candidates.retain(|site| coastal_regions.contains(&labels[&site.salt]));
    let mut communities: Vec<Community> = Vec::new();
    let mut routes = CorridorCache::new();
    let mut arrivals: HashMap<u64, Option<CoastalVoyage>> = HashMap::new();
    let mut invalid_layouts = HashSet::new();
    info!(
        "World founding survey: {} viable sites across {} coastal land regions ({} surveyed), {} ocean approaches",
        candidates.len(), coastal_regions.len(), surveyed, voyages.len()
    );
    while communities.len() < config.settlement_count {
        candidates.retain(|site| {
            communities
                .iter()
                .all(|c| c.site.hall.xz().distance(site.hall.xz()) >= MIN_SETTLEMENT_DISTANCE)
        });
        if communities.is_empty() {
            candidates.sort_by(|a, b| {
                let score = |site: &sites::Site| site.rank - shore_distance[&site.salt] / 600.0;
                score(b).total_cmp(&score(a)).then(a.salt.cmp(&b.salt))
            });
        } else {
            let timber = communities
                .iter()
                .map(|c| c.site.resources.wood)
                .fold(0.0_f32, f32::max);
            let stone = communities
                .iter()
                .map(|c| c.site.resources.stone)
                .fold(0.0_f32, f32::max);
            candidates.sort_by(|a, b| {
                let score = |site: &sites::Site| {
                    let nearest = communities
                        .iter()
                        .map(|c| c.site.hall.xz().distance(site.hall.xz()))
                        .fold(f32::INFINITY, f32::min);
                    coverage_score(site, nearest, timber, stone)
                };
                score(b).total_cmp(&score(a)).then(a.salt.cmp(&b.salt))
            });
        }
        let mut accepted = None;
        let mut rejected = [0usize; 4];
        for (index, site) in candidates.iter().enumerate() {
            if invalid_layouts.contains(&site.salt) {
                continue;
            }
            let label = labels[&site.salt];
            let mut neighbours: Vec<_> = communities
                .iter()
                .enumerate()
                .filter(|(_, c)| {
                    labels[&c.site.salt] == label
                        && site.hall.xz().distance(c.site.hall.xz()) <= NEIGHBOUR_REACH
                })
                .map(|(index, _)| index)
                .collect();
            neighbours.sort_by(|a, b| {
                communities[*a]
                    .site
                    .hall
                    .xz()
                    .distance_squared(site.hall.xz())
                    .total_cmp(
                        &communities[*b]
                            .site
                            .hall
                            .xz()
                            .distance_squared(site.hall.xz()),
                    )
                    .then(a.cmp(b))
            });
            let arrival = if shore_distance[&site.salt] <= COASTAL_REACH {
                *arrivals
                    .entry(site.salt)
                    .or_insert_with(|| sites::approach_for(terrain, site, &voyages))
            } else {
                None
            };
            // The first place in any disconnected group needs an actual safe
            // arrival. A coarse label or proximity to water cannot approve it.
            if neighbours.is_empty() && arrival.is_none() {
                rejected[0] += 1;
                continue;
            }
            let linked = neighbours.iter().copied().find(|&neighbour| {
                corridor(terrain, site, &communities[neighbour].site, &mut routes)
            });
            if linked.is_none() && arrival.is_none() {
                rejected[1] += 1;
                continue;
            }
            let colliders =
                crate::collision::streaming::survey_settlement_props(terrain, library, site.hall);
            if !crate::world::village_roads::doorway_road_apron_is_clear_of_props(
                shared::components::SettlementBuildingKind::Hall,
                site.hall,
                0.0,
                &colliders,
                library,
            ) {
                invalid_layouts.insert(site.salt);
                rejected[2] += 1;
                continue;
            }
            debug!(
                "Founding layout attempt at {:?}, population={}",
                site.hall, site.potential_population
            );
            let layout = match config.opening {
                crate::world::start_config::OpeningProfile::Mature => {
                    layout::plan(terrain, site, &colliders, library)
                }
                crate::world::start_config::OpeningProfile::Frontier => frontier::plan(
                    terrain,
                    site,
                    &colliders,
                    library,
                    config.founders_per_settlement,
                ),
            };
            let Some(layout) = layout else {
                invalid_layouts.insert(site.salt);
                rejected[3] += 1;
                continue;
            };
            let radius = layout.radius(site.hall);
            if communities
                .iter()
                .any(|c| site.hall.xz().distance(c.site.hall.xz()) < radius + c.radius + 32.0)
            {
                invalid_layouts.insert(site.salt);
                rejected[3] += 1;
                continue;
            }
            accepted = Some((index, layout, arrival, linked, neighbours));
            break;
        }
        let Some((index, layout, arrival, linked, neighbours)) = accepted else {
            if communities.len() >= config.minimum_settlements() {
                info!("World geography supports {} communities with coastal access; keeping valid plans instead of forcing the {}-place target", communities.len(), config.settlement_count);
                break;
            }
            return Err(format!("Seed {seed} could support only {} of {} required {:?} settlements with certified coastal access (rejected arrival/connection/door/layout={rejected:?}); no incomplete world was published", communities.len(), config.minimum_settlements(), config.opening));
        };
        let site = candidates.remove(index);
        let network = linked.map_or(communities.len() as u64 + 1, |i| {
            communities[i].land_network
        });
        // A new town can connect two previously separate gateway clusters.
        // Merge only links that pass the real terrain corridor proof; do not
        // infer a freight connection merely from a shared coarse region label.
        let mut merged = HashSet::from([network]);
        for neighbour in neighbours {
            let other = &communities[neighbour];
            if !merged.contains(&other.land_network)
                && corridor(terrain, &site, &other.site, &mut routes)
            {
                merged.insert(other.land_network);
            }
        }
        let land_network = *merged.iter().min().unwrap();
        for community in &mut communities {
            if merged.contains(&community.land_network) {
                community.land_network = land_network;
            }
        }
        let name = sites::name(seed, communities.len());
        info!("Founding {name}: residents={} buildings={} at=({:.1},{:.1}) farmland={:.2} timber={:.2} stone={:.2} land_network={land_network}",
            layout.population, layout.plots.len(), site.hall.x, site.hall.z,
            site.resources.farmland, site.resources.wood, site.resources.stone);
        communities.push(Community {
            radius: layout.radius(site.hall),
            site,
            layout,
            name,
            arrival,
            land_network,
        });
    }
    Ok(communities)
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::worldgen::ResourceProfile;

    fn candidate(rank: f32) -> sites::Site {
        sites::Site {
            hall: Vec3::ZERO,
            resources: ResourceProfile {
                farmland: 0.6,
                wood: 0.3,
                stone: 0.2,
                iron: 0.0,
            },
            potential_population: 24,
            rank,
            salt: 1,
        }
    }

    #[test]
    fn another_fertile_neighbour_does_not_outrank_a_viable_unserved_region() {
        let rich = candidate(0.9);
        let modest = candidate(0.4);
        assert!(coverage_score(&modest, 4200.0, 0.8, 0.5) > coverage_score(&rich, 900.0, 0.8, 0.5));
        assert!(
            coverage_score(&modest, 4200.0, 0.8, 0.5) > coverage_score(&modest, 2400.0, 0.8, 0.5)
        );
        // Equal coverage still rewards better land rather than a rigid grid.
        assert!(
            coverage_score(&rich, 1600.0, 0.8, 0.5) > coverage_score(&modest, 1600.0, 0.8, 0.5)
        );
    }
}
