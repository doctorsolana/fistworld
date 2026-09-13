//! Opt-in geography audit against surveyed inhabitable land, not ocean quadrants.

use super::*;
use bevy::ecs::system::RunSystemOnce;
use std::collections::{BTreeMap, BTreeSet};
use std::io::Write;

const SEEDS: [u64; 4] = [4794248476676134349, 7, 91, 12345];
const COVERAGE_RADIUS: f32 = 1500.0;

#[derive(Debug)]
struct Coverage {
    mean: f32,
    p90: f32,
    maximum: f32,
    covered_fraction: f32,
}

/// Survey points represent equal-area grid samples. Rivers/ocean/mountain
/// cells rejected by the production survey contribute no demand for a town.
fn coverage(sites: &[Vec2], towns: &[Vec2]) -> Option<Coverage> {
    if sites.is_empty() || towns.is_empty() {
        return None;
    }
    let mut nearest: Vec<_> = sites
        .iter()
        .map(|site| {
            towns
                .iter()
                .map(|town| town.distance(*site))
                .fold(f32::INFINITY, f32::min)
        })
        .collect();
    nearest.sort_by(f32::total_cmp);
    Some(Coverage {
        mean: nearest.iter().sum::<f32>() / nearest.len() as f32,
        p90: nearest[((nearest.len() as f32 * 0.9).ceil() as usize).saturating_sub(1)],
        maximum: *nearest.last().unwrap(),
        covered_fraction: nearest.iter().filter(|d| **d <= COVERAGE_RADIUS).count() as f32
            / nearest.len() as f32,
    })
}

#[test]
fn coverage_weights_usable_sites_and_exposes_an_unserved_landmass() {
    let sites = [
        Vec2::ZERO,
        Vec2::X * 100.0,
        Vec2::X * 4000.0,
        Vec2::X * 4100.0,
    ];
    let clustered = coverage(&sites, &[Vec2::ZERO, Vec2::X * 100.0]).unwrap();
    let spread = coverage(&sites, &[Vec2::ZERO, Vec2::X * 4000.0]).unwrap();
    assert_eq!(clustered.covered_fraction, 0.5);
    assert_eq!(spread.covered_fraction, 1.0);
    assert_eq!(clustered.p90, 4000.0);
    assert_eq!(spread.p90, 100.0);
    assert_eq!(spread.mean, 50.0);
    assert!(spread.maximum < clustered.maximum);
    assert!(coverage(&[], &[Vec2::ZERO]).is_none());
    assert!(coverage(&sites, &[]).is_none());
}

#[test]
#[ignore = "full-size production plans across four seeds; writes optional CSV audit evidence"]
fn ordinary_openings_report_land_aware_distribution() {
    let mut logging = App::new();
    logging.add_plugins(bevy::log::LogPlugin::default());
    let seeds = std::env::var("FISTWORLD_OPENING_AUDIT_SEEDS").map_or_else(
        |_| SEEDS.to_vec(),
        |raw| {
            raw.split(',')
                .map(|s| s.trim().parse::<u64>().expect("audit seed"))
                .collect()
        },
    );
    assert!(
        !seeds.is_empty() && seeds.len() <= 12,
        "audit is bounded to1–12 seeds"
    );
    let out = std::env::var_os("FISTWORLD_OPENING_AUDIT_DIR").map(std::path::PathBuf::from);
    if let Some(out) = &out {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..");
        let logs = root
            .join("logs")
            .canonicalize()
            .expect("repository logs directory");
        std::fs::create_dir_all(out).unwrap();
        assert!(
            out.canonicalize().unwrap().starts_with(logs),
            "audit artifacts belong under logs/"
        );
    }
    let mut world = World::new();
    world
        .run_system_once(crate::collision::library::setup_baked_colliders)
        .unwrap();
    let library = world.resource::<DerivedColliderLibrary>();
    for seed in seeds {
        let started = std::time::Instant::now();
        let terrain = WorldTerrain::from_loaded_map(
            shared::map::load_session_map(&shared::map::new_world_recipe(seed)).unwrap(),
        );
        let sites = sites::survey(&terrain, seed);
        let land = connectivity::LandRegions::survey(&terrain);
        let voyages = sites::approaches(&terrain, seed);
        let labels: Vec<_> = sites
            .iter()
            .map(|s| land.at(&terrain, s.hall.xz()))
            .collect();
        // This is a geography shortlist, not an assertion that every coastal
        // candidate has a certified gateway or room for a complete layout.
        let coastal: BTreeSet<_> = sites
            .iter()
            .zip(&labels)
            .filter(|(site, label)| {
                **label != 0
                    && voyages
                        .iter()
                        .any(|v| v.landing.xz().distance(site.hall.xz()) <= 600.0)
            })
            .map(|(_, label)| *label)
            .collect();
        let mut eligible = BTreeMap::<usize, Vec<Vec2>>::new();
        for (site, region) in sites.iter().zip(&labels) {
            if coastal.contains(region) {
                eligible.entry(*region).or_default().push(site.hall.xz());
            }
        }
        if let Some(out) = &out {
            let mut csv = std::fs::File::create(out.join(format!("{seed}-sites.csv"))).unwrap();
            writeln!(csv, "x,z,region,eligible,farmland,timber,stone").unwrap();
            for (site, region) in sites.iter().zip(&labels) {
                writeln!(
                    csv,
                    "{},{},{},{},{},{},{}",
                    site.hall.x,
                    site.hall.z,
                    region,
                    coastal.contains(region),
                    site.resources.farmland,
                    site.resources.wood,
                    site.resources.stone
                )
                .unwrap();
            }
        }
        let towns = planning::plan_world(&terrain, library, seed).unwrap_or_else(|error| {
            if let Some(out) = &out {
                std::fs::write(out.join(format!("{seed}-error.txt")), &error).unwrap();
            }
            panic!("{error}")
        });
        let points: Vec<_> = towns.iter().map(|c| c.site.hall.xz()).collect();
        let town_regions: Vec<_> = towns
            .iter()
            .map(|c| land.at(&terrain, c.site.hall.xz()))
            .collect();
        let all_sites: Vec<_> = eligible.values().flatten().copied().collect();
        let coverage = coverage(&all_sites, &points).expect("inhabitable coastal sites and towns");
        let major_minimum = ((all_sites.len() as f32 * 0.1).ceil() as usize).max(20);
        let major: Vec<_> = eligible
            .iter()
            .filter(|(_, sites)| sites.len() >= major_minimum)
            .map(|(region, _)| *region)
            .collect();
        let occupied_major = major
            .iter()
            .filter(|region| town_regions.contains(region))
            .count();
        let min_spacing = points
            .iter()
            .enumerate()
            .flat_map(|(i, a)| points[i + 1..].iter().map(move |b| a.distance(*b)))
            .fold(f32::INFINITY, f32::min);
        let bounds = terrain.generator.active_map_bounds();
        eprintln!("WORLD_DISTRIBUTION seed={seed} towns={} eligible_sites={} major_regions={}/{} nearest_mean={:.0}m nearest_p90={:.0}m nearest_max={:.0}m covered1500={:.3} min_spacing={:.0}m elapsed={:.2}s",
            towns.len(), all_sites.len(), occupied_major, major.len(), coverage.mean, coverage.p90,
            coverage.maximum, coverage.covered_fraction, min_spacing, started.elapsed().as_secs_f32());
        if let Some(out) = &out {
            let mut csv = std::fs::File::create(out.join(format!("{seed}-coverage.csv"))).unwrap();
            writeln!(csv, "seed,towns,eligible_sites,major_regions,occupied_major_regions,nearest_mean_m,nearest_p90_m,nearest_max_m,covered_1500_fraction,min_spacing_m,min_x,min_z,max_x,max_z").unwrap();
            writeln!(
                csv,
                "{seed},{},{},{},{},{},{},{},{},{},{},{},{},{}",
                towns.len(),
                all_sites.len(),
                major.len(),
                occupied_major,
                coverage.mean,
                coverage.p90,
                coverage.maximum,
                coverage.covered_fraction,
                min_spacing,
                bounds.min[0],
                bounds.min[1],
                bounds.max[0],
                bounds.max[1]
            )
            .unwrap();
            let mut csv = std::fs::File::create(out.join(format!("{seed}-regions.csv"))).unwrap();
            writeln!(
                csv,
                "region,eligible_sites,towns,major,has_accepted_gateway"
            )
            .unwrap();
            for (region, sites) in &eligible {
                let accepted = towns
                    .iter()
                    .zip(&town_regions)
                    .filter(|(_, r)| **r == *region);
                writeln!(
                    csv,
                    "{region},{},{},{},{}",
                    sites.len(),
                    accepted.clone().count(),
                    major.contains(region),
                    accepted.into_iter().any(|(town, _)| town.arrival.is_some())
                )
                .unwrap();
            }
            let mut csv = std::fs::File::create(out.join(format!("{seed}-towns.csv"))).unwrap();
            writeln!(
                csv,
                "name,x,z,region,population,buildings,farmland,timber,stone,arrival,land_network,rated_rations,funded_positions"
            )
            .unwrap();
            let mut plots = std::fs::File::create(out.join(format!("{seed}-plots.csv"))).unwrap();
            writeln!(plots, "town,kind,x,z,quality").unwrap();
            for (town, region) in towns.iter().zip(&town_regions) {
                let economy = layout::audit_economy(
                    &town
                        .layout
                        .plots
                        .iter()
                        .map(|p| (p.kind, p.quality))
                        .collect::<Vec<_>>(),
                );
                writeln!(
                    csv,
                    "{},{},{},{},{},{},{},{},{},{},{},{},{}",
                    town.name,
                    town.site.hall.x,
                    town.site.hall.z,
                    region,
                    town.layout.population,
                    town.layout.plots.len(),
                    town.site.resources.farmland,
                    town.site.resources.wood,
                    town.site.resources.stone,
                    town.arrival.is_some(),
                    town.land_network,
                    economy.rated_rations,
                    economy.funded_positions
                )
                .unwrap();
                for plot in &town.layout.plots {
                    writeln!(
                        plots,
                        "{},{:?},{},{},{}",
                        town.name, plot.kind, plot.position.x, plot.position.z, plot.quality
                    )
                    .unwrap();
                }
            }
        }
        assert!((MIN_SETTLEMENT_COUNT..=SETTLEMENT_COUNT).contains(&towns.len()));
        assert!(min_spacing >= MIN_SETTLEMENT_DISTANCE);
        // These four maintained recipes have measured, feasible space for a
        // well-spread opening: 99.4–100% within 1.5 km and p90 <= 1.27 km.
        // Keep headroom for valid layout changes while rejecting the old
        // one-neighbourhood result (39.2% coverage, p90 4.57 km on the reported
        // seed). Arbitrary custom seeds remain geography diagnostics; a small
        // island or inaccessible survey hint must not force an unsafe town.
        if SEEDS.contains(&seed) {
            assert!(
                coverage.covered_fraction >= 0.95,
                "maintained seed {seed}: less than 95% of eligible land is within 1.5 km of a town: {coverage:?}"
            );
            assert!(
                coverage.p90 <= COVERAGE_RADIUS,
                "maintained seed {seed}: nearest-town p90 exceeds 1.5 km: {coverage:?}"
            );
            assert_eq!(
                occupied_major,
                major.len(),
                "maintained seed {seed}: an eligible major land region was left empty"
            );
        }
        for (i, town) in towns.iter().enumerate() {
            let economy = layout::audit_economy(
                &town
                    .layout
                    .plots
                    .iter()
                    .map(|p| (p.kind, p.quality))
                    .collect::<Vec<_>>(),
            );
            assert!(
                economy.supports(town.layout.population),
                "{} cannot sustain its opening: {economy:?}",
                town.name
            );
            assert!(
                towns
                    .iter()
                    .any(|c| c.land_network == town.land_network && c.arrival.is_some()),
                "freight group {} has no certified arrival",
                town.land_network
            );
            assert!(
                coastal.contains(&town_regions[i]),
                "town outside coastal-capable surveyed land"
            );
            assert!(sites
                .iter()
                .any(|s| s.salt == town.site.salt && s.hall == town.site.hall));
            assert!(
                towns
                    .iter()
                    .zip(&town_regions)
                    .any(|(c, r)| *r == town_regions[i] && c.arrival.is_some()),
                "inhabited region{} has no accepted coastal gateway",
                town_regions[i]
            );
            for other in &towns[i + 1..] {
                assert!(
                    town.site.hall.xz().distance(other.site.hall.xz())
                        >= town.radius + other.radius + 32.0
                );
            }
        }
    }
}
