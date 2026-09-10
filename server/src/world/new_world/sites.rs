//! Seeded settlement geography. A shortlist is cheap; complete plot and
//! access validation happens only for sites we actually try to inhabit.

use crate::player::boat::{coastal_voyages, CoastalVoyage};
use crate::world::village_roads::overland_trade_corridor_exists;
use bevy::prelude::*;
use shared::{
    components::SettlementBuildingKind, terrain::WorldTerrain, worldgen::ResourceProfile,
};

#[derive(Clone, Debug)]
pub(super) struct Site {
    pub hall: Vec3,
    pub resources: ResourceProfile,
    pub potential_population: usize,
    pub rank: f32,
    pub salt: u64,
}

pub(super) fn survey(terrain: &WorldTerrain, seed: u64) -> Vec<Site> {
    let bounds = terrain.generator.active_map_bounds();
    let Some(field) = terrain.generator.loaded_map().biome_field.as_deref() else {
        return Vec::new();
    };
    let mut rng = shared::rng::XorShift64::new(seed ^ 0x504C_4143_4553);
    let mut sites = Vec::new();
    let step = 112.0;
    let margin = 360.0;
    let mut x = bounds.min[0] + margin;
    while x < bounds.max[0] - margin {
        let mut z = bounds.min[1] + margin;
        while z < bounds.max[1] - margin {
            let salt = rng.next_u64();
            let at = Vec2::new(
                x + ((salt & 255) as f32 / 255.0 - 0.5) * 64.0,
                z + (((salt >> 8) & 255) as f32 / 255.0 - 0.5) * 64.0,
            );
            let hall = Vec3::new(at.x, terrain.get_height(at.x, at.y), at.y);
            let gradient = slope(terrain, at);
            let resources = field.resources(at.x, at.y, hall.y, gradient);
            // Even a specialised inland hamlet needs a plausible local food
            // base until shipping can reliably sustain a wholly import-fed town.
            if gradient < 0.10
                && resources.farmland >= 0.22
                && shared::components::minimum_building_water_clearance(
                    terrain,
                    hall,
                    SettlementBuildingKind::Hall,
                    0.0,
                ) >= shared::components::SETTLEMENT_FREEBOARD
            {
                let usable = [-144.0, -72.0, 0.0, 72.0, 144.0]
                    .into_iter()
                    .flat_map(|dx| {
                        [-144.0, -72.0, 0.0, 72.0, 144.0]
                            .into_iter()
                            .map(move |dz| at + Vec2::new(dx, dz))
                    })
                    .filter(|point| {
                        let height = terrain.get_height(point.x, point.y);
                        slope(terrain, *point) < 0.16
                            && terrain
                                .water_surface_height(point.x, point.y)
                                .is_none_or(|water| {
                                    height - water >= shared::components::SETTLEMENT_FREEBOARD
                                })
                    })
                    .count() as f32
                    / 25.0;
                if usable >= 0.76 {
                    let maturity = 0.15 + ((salt >> 24) & 1023) as f32 / 1023.0 * 0.85;
                    let capacity = (resources.farmland * usable).clamp(0.0, 1.0);
                    let potential_population = (12.0 + 64.0 * capacity * maturity.powi(2)) as usize;
                    sites.push(Site {
                        hall,
                        resources,
                        potential_population,
                        salt,
                        // Age affects population, not whether a place deserves
                        // to exist. Ranking by maturity selected almost only
                        // old, large towns and erased the younger communities.
                        rank: capacity * 0.55 + resources.wood * 0.30 + resources.stone * 0.15,
                    });
                }
            }
            z += step;
        }
        x += step;
    }
    sites
}

pub(super) fn slope(terrain: &WorldTerrain, point: Vec2) -> f32 {
    let h = terrain.get_height(point.x, point.y);
    ((terrain.get_height(point.x + 3.0, point.y) - h)
        .abs()
        .max((terrain.get_height(point.x, point.y + 3.0) - h).abs()))
        / 3.0
}

pub(super) fn approaches(terrain: &WorldTerrain, seed: u64) -> Vec<CoastalVoyage> {
    coastal_voyages(terrain, seed)
}

/// Arrival points are chosen from genuine ocean approaches, with a dry route
/// into an inhabited place. This is spawn suitability, not a scripted quest.
pub(super) fn approach_for(
    terrain: &WorldTerrain,
    site: &Site,
    voyages: &[CoastalVoyage],
) -> Option<CoastalVoyage> {
    let entrance = SettlementBuildingKind::Hall
        .entrance_position(site.hall, 0.0)
        .xz();
    let mut candidates: Vec<_> = voyages
        .iter()
        .filter(|v| v.landing.xz().distance(entrance) <= 600.0)
        .copied()
        .collect();
    candidates.sort_by(|a, b| {
        a.landing
            .xz()
            .distance_squared(entrance)
            .total_cmp(&b.landing.xz().distance_squared(entrance))
    });
    candidates
        .into_iter()
        .take(4)
        .find(|v| overland_trade_corridor_exists(terrain, v.landing.xz(), entrance))
}

pub(super) fn name(seed: u64, ordinal: usize) -> String {
    const PREFIX: [&str; 20] = [
        "Alder", "Ash", "Bracken", "Briar", "Broad", "Elder", "Elm", "Fair", "Fern", "Green",
        "Hazel", "High", "Low", "Oak", "Raven", "Red", "Stone", "West", "Willow", "White",
    ];
    const SUFFIX: [&str; 12] = [
        "brook", "combe", "dale", "field", "ford", "ham", "haven", "holt", "mead", "stead", "wick",
        "wood",
    ];
    // Coprime stride visits unique combinations for all ten communities.
    let index = (seed as usize % 240 + ordinal * 37) % 240;
    format!(
        "{}{}",
        PREFIX[index % PREFIX.len()],
        SUFFIX[index / PREFIX.len()]
    )
}
