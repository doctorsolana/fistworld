//! Select a connected set of fully validated founding plans.

use super::*;
use crate::world::village_roads::overland_trade_corridor_exists;

pub(super) fn plan_world(
    terrain: &WorldTerrain,
    library: &DerivedColliderLibrary,
    seed: u64,
) -> Result<Vec<Community>, String> {
    let mut candidates = sites::survey(terrain, seed);
    let voyages = sites::approaches(terrain, seed);
    let regions = connectivity::LandRegions::survey(terrain);
    let labels: std::collections::HashMap<_, _> = candidates
        .iter()
        .map(|site| (site.salt, regions.at(terrain, site.hall.xz())))
        .collect();
    let mut region_sizes = std::collections::BTreeMap::<usize, usize>::new();
    for site in &candidates {
        let label = labels[&site.salt];
        if label != 0 {
            *region_sizes.entry(label).or_default() += 1;
        }
    }
    let coastal: std::collections::HashSet<_> = candidates
        .iter()
        .filter(|site| {
            voyages
                .iter()
                .any(|v| v.landing.xz().distance(site.hall.xz()) <= 600.0)
        })
        .map(|site| labels[&site.salt])
        .collect();
    let region = region_sizes
        .into_iter()
        .filter(|(label, _)| coastal.contains(label))
        .max_by_key(|(label, count)| (*count, std::cmp::Reverse(*label)))
        .map(|(label, _)| label)
        .ok_or("No inhabitable land region has a coastal approach")?;
    candidates.retain(|site| labels[&site.salt] == region);
    let mut communities: Vec<Community> = Vec::new();
    let mut routes = std::collections::HashMap::new();
    let mut invalid_layouts = std::collections::HashSet::new();
    info!(
        "World founding survey: {} viable sites, {} ocean approaches",
        candidates.len(),
        voyages.len()
    );
    while communities.len() < SETTLEMENT_COUNT {
        candidates.retain(|site| {
            communities
                .iter()
                .all(|c| c.site.hall.xz().distance(site.hall.xz()) >= MIN_SETTLEMENT_DISTANCE)
        });
        let first = communities.is_empty();
        if first {
            candidates.sort_by(|a, b| {
                let score = |site: &sites::Site| {
                    let distance = voyages
                        .iter()
                        .map(|v| v.landing.xz().distance(site.hall.xz()))
                        .fold(f32::INFINITY, f32::min);
                    site.rank - distance / 300.0
                };
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
                    let distance = communities
                        .iter()
                        .map(|c| c.site.hall.xz().distance(site.hall.xz()))
                        .fold(f32::INFINITY, f32::min);
                    // Grow a regional network in useful overland hops. A
                    // farthest-point sampler kept retrying the opposite coast
                    // before considering the next reachable valley.
                    let opportunity = (site.resources.wood - timber).max(0.0)
                        + (site.resources.stone - stone).max(0.0);
                    site.rank + opportunity + distance.min(1500.0) / 900.0
                        - (distance - 1800.0).max(0.0) / 300.0
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
            let arrival = sites::approach_for(terrain, site, &voyages);
            if first && arrival.is_none() {
                rejected[0] += 1;
                continue;
            }
            if !first {
                let mut neighbors: Vec<_> = communities
                    .iter()
                    .filter(|c| site.hall.xz().distance(c.site.hall.xz()) <= 2400.0)
                    .collect();
                neighbors.sort_by(|a, b| {
                    a.site
                        .hall
                        .xz()
                        .distance_squared(site.hall.xz())
                        .total_cmp(&b.site.hall.xz().distance_squared(site.hall.xz()))
                });
                // The geometrically nearest town can be across a river. A
                // connection to any existing neighbour keeps the network whole.
                if !neighbors.into_iter().any(|neighbor| {
                    *routes
                        .entry((site.salt, neighbor.site.salt))
                        .or_insert_with(|| {
                            overland_trade_corridor_exists(
                                terrain,
                                site.hall.xz(),
                                neighbor.site.hall.xz(),
                            )
                        })
                }) {
                    rejected[1] += 1;
                    continue;
                }
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
                rejected[2] += 1;
                continue;
            }
            debug!(
                "Founding layout attempt at {:?}, population={}",
                site.hall, site.potential_population
            );
            let Some(layout) = layout::plan(terrain, site, &colliders, library) else {
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
            accepted = Some((index, layout, arrival));
            break;
        }
        let Some((index, layout, arrival)) = accepted else {
            if communities.len() >= MIN_SETTLEMENT_COUNT {
                info!("World geography supports {} connected communities; keeping the valid network instead of forcing the ten-place target", communities.len());
                break;
            }
            return Err(format!("Seed {seed} could support only {} connected settlements (rejected arrival/connection/door/layout={rejected:?}); no incomplete world was published", communities.len()));
        };
        let site = candidates.remove(index);
        let name = sites::name(seed, communities.len());
        info!("Founding {name}: residents={} buildings={} at=({:.1},{:.1}) farmland={:.2} timber={:.2} stone={:.2}",
            layout.population, layout.plots.len(), site.hall.x, site.hall.z,
            site.resources.farmland, site.resources.wood, site.resources.stone);
        communities.push(Community {
            radius: layout.radius(site.hall),
            site,
            layout,
            name,
            arrival,
        });
    }
    Ok(communities)
}
