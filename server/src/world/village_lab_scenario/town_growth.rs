//! Shared terrain and immigration controls for real town-growth experiments.

use super::*;

pub(crate) const TOWN_GROWTH_FOUNDERS: usize = 8;
const GROWTH_RADIUS: f32 = 120.0;
// Also the town-growth camera focus in run.sh. Surveyed against village_lab's
// seed-3 recipe; a terrain edit must revalidate this fixture, never move it
// silently and leave the connected camera looking at the former site.
const TOWN_GROWTH_ANCHOR: Vec2 = Vec2::new(-100.0, 120.0);
const MIN_GROWTH_TREES: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GrowthProfile {
    Low,
    Steady,
    Burst,
    CityGradual(usize),
    CitySurge(usize),
}

impl GrowthProfile {
    pub(crate) fn parse(value: &str) -> Self {
        match value {
            "low" => Self::Low,
            "steady" => Self::Steady,
            "burst" => Self::Burst,
            value => {
                let parts: Vec<_> = value.split('-').collect();
                if let ["city", population, pace] = parts.as_slice() {
                    if let Ok(target @ (100 | 250 | 500)) = population.parse::<usize>() {
                        match *pace {
                            "gradual" => return Self::CityGradual(target),
                            "surge" => return Self::CitySurge(target),
                            _ => {}
                        }
                    }
                }
                panic!("FISTWORLD_TOWN_PROFILE must be low, steady, burst, or city-{{100|250|500}}-{{gradual|surge}}")
            }
        }
    }

    pub(crate) fn from_environment() -> Self {
        Self::parse(&std::env::var("FISTWORLD_TOWN_PROFILE").unwrap_or_else(|_| "steady".into()))
    }

    #[cfg(test)]
    pub(crate) fn default_minutes(self) -> f32 {
        match self {
            Self::CityGradual(_) | Self::CitySurge(_) => 1_440.0,
            _ => 240.0,
        }
    }

    /// Offered population includes founders; admission and retention remain outcomes.
    #[cfg(test)]
    pub(crate) fn target_population(self) -> usize {
        TOWN_GROWTH_FOUNDERS + self.waves().iter().map(|wave| wave.count).sum::<usize>()
    }

    pub(crate) fn waves(self) -> Vec<LabArrivalWave> {
        let waves: Vec<(u32, usize)> = match self {
            Self::Low => vec![(2, 2), (4, 2), (6, 2)],
            Self::Steady => (2..=8).map(|day| (day, 3)).collect(),
            Self::Burst => vec![(4, 24)],
            Self::CityGradual(target) | Self::CitySurge(target) => {
                // Establish the same initial economy before changing migration
                // pressure. No capacity, goods, permits or tiers are fabricated.
                let mut waves: Vec<_> = (2..=8).map(|day| (day, 3)).collect();
                let mut remaining = target - (TOWN_GROWTH_FOUNDERS + 21);
                if matches!(self, Self::CitySurge(_)) {
                    waves.push((12, remaining));
                } else {
                    let batch = match target {
                        100 => 7,
                        250 => 12,
                        _ => 20,
                    };
                    let mut day = 9;
                    while remaining > 0 {
                        let count = remaining.min(batch);
                        waves.push((day, count));
                        remaining -= count;
                        day += 1;
                    }
                }
                waves
            }
        };
        waves
            .into_iter()
            .map(|(day, count)| LabArrivalWave {
                day,
                count,
                target: LabArrivalTarget::Meadow,
            })
            .collect()
    }
}

pub(crate) fn town_growth_seed() -> u64 {
    std::env::var("FISTWORLD_TOWN_SEED")
        .map_or(23, |value| value.parse::<u64>().expect("integer town seed"))
}

fn river_clearance(terrain: &WorldTerrain, point: Vec2) -> f32 {
    terrain
        .rivers()
        .iter()
        .flat_map(|river| river.windows(2))
        .map(|segment| {
            let a = segment[0].xz();
            let delta = segment[1].xz() - a;
            let along =
                ((point - a).dot(delta) / delta.length_squared().max(0.0001)).clamp(0.0, 1.0);
            point.distance(a + delta * along) - shared::worldgen::RIVER_WATER_REACH
        })
        .fold(f32::INFINITY, f32::min)
}

fn inspect_growth_site(terrain: &WorldTerrain, point: Vec2) -> Option<(Vec3, f32)> {
    let map = terrain.generator.loaded_map();
    let field = map.biome_field.as_deref()?;
    let height = terrain.get_height(point.x, point.y);
    let slope = slope_at(terrain, point.x, point.y);
    let hall = Vec3::new(point.x, height, point.y);
    let farmland = field.resources(point.x, point.y, height, slope).farmland;
    if field.biome(point.x, point.y, height, slope) != WorldBiome::Meadows
        || farmland < 0.45
        || slope >= 0.10
        || river_clearance(terrain, point) < GROWTH_RADIUS
        || shared::components::minimum_building_water_clearance(
            terrain,
            hall,
            SettlementBuildingKind::Hall,
            0.0,
        ) < shared::components::SETTLEMENT_FREEBOARD
    {
        return None;
    }
    // Check the growth area, not just the founding footprint. A compact
    // 8-metre survey rejects coasts, channels and steep hills around the town.
    // The extra 8 metres protect the required 120 m envelope between samples.
    let mut gentle = 0;
    let mut samples = 0;
    for x in -16..=16 {
        for z in -16..=16 {
            let offset = Vec2::new(x as f32 * 8.0, z as f32 * 8.0);
            if offset.length_squared() > (GROWTH_RADIUS + 8.0).powi(2) {
                continue;
            }
            let at = point + offset;
            let ground = terrain.get_height(at.x, at.y);
            if terrain
                .water_surface_height(at.x, at.y)
                .is_some_and(|water| ground - water < shared::components::SETTLEMENT_FREEBOARD)
            {
                return None;
            }
            samples += 1;
            gentle += usize::from(slope_at(terrain, at.x, at.y) < 0.10);
        }
    }
    (gentle as f32 / samples as f32 >= 0.90).then_some((hall, farmland))
}

/// Select a central inland growth area with real reachable timber. This lab
/// controls only where people found their town; all accepted plots are live.
pub(crate) fn choose_town_growth_site(terrain: &WorldTerrain) -> (Vec3, usize, f32) {
    assert_eq!(
        terrain.generator.loaded_map().definition.map_id,
        "village_lab"
    );
    let (hall, farmland) = inspect_growth_site(terrain, TOWN_GROWTH_ANCHOR)
        .expect("town-growth anchor lost its fertile, dry 120 m growth area; re-survey the fixture and camera");
    let trees = nearby_tree_count(terrain, TOWN_GROWTH_ANCHOR, 120.0);
    assert!(
        trees >= MIN_GROWTH_TREES && village::lumber_plot_has_reachable_tree(terrain, hall),
        "town-growth anchor lost its reachable timber stand; re-survey the fixture and camera"
    );
    (hall, trees, farmland)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "loads the dedicated generated lab map; run in its own process"]
    fn town_growth_site_has_room_and_resources() {
        std::env::set_var("CITYSIM_MAP_ID", "village_lab");
        let terrain = WorldTerrain::default();
        assert_eq!(
            terrain.generator.loaded_map().definition.map_id,
            "village_lab"
        );
        let (hall, trees, farmland) = choose_town_growth_site(&terrain);
        assert!(inspect_growth_site(&terrain, hall.xz()).is_some());
        assert!(trees >= MIN_GROWTH_TREES);
        assert!(river_clearance(&terrain, hall.xz()) >= GROWTH_RADIUS);
        assert!(village::lumber_plot_has_reachable_tree(&terrain, hall));
        println!(
            "TOWN site x={} z={} elevation={} farmland={} trees={} dry_radius={} river_clearance={}",
            hall.x, hall.z, hall.y, farmland, trees, GROWTH_RADIUS, river_clearance(&terrain, hall.xz())
        );
    }
}
