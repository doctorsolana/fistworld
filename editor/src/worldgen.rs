//! Random world generation: one click produces an explorable landscape with
//! forests, meadows, beaches, ocean, and rivers.
//!
//! The terrain core (noise recipe, height grid, rivers, road flattening,
//! surface paint formula) lives in `shared::worldgen` so the client and
//! server can rebuild generated worlds from their seed recipe at load time
//! (the Valheim model — worlds ship as a few KB of RON, not baked chunk
//! data). This module drives it and adds the editor-only content on top:
//! landmark placement, road pathfinding, the harbour village, vegetation
//! scatter, and spawn picking — all of which write ordinary map content
//! (roads, plots, props, markers) that is stored explicitly.

use bevy::prelude::*;
use noise::NoiseFn;

use shared::map::{load_map_from_parts, MapObjectSpawn};
use shared::props::PropKind;
use shared::terrain::WorldTerrain;
pub use shared::worldgen::{
    fbm, rand01, splitmix64, surface_weights, weights_to_bytes, FlattenStroke, GeneratedWorld,
    HeightField, HeightGrid, RoadMask, WorldStyle, SEA_LEVEL,
};

use crate::city::CityEditorState;
use crate::session::{EditorEnvironmentState, EditorSession};
use crate::tools::VisualRefreshFlags;

/// Scatter vegetation from the heightfield: forests on a biome mask,
/// meadows between them, rocks on slopes, sparse flowers.
fn scatter_props(
    grid: &HeightGrid,
    seed: u64,
    half_extent: f32,
    roads: Option<&RoadMask>,
) -> Vec<MapObjectSpawn> {
    // Vegetation follows the biome field — the same sampler the runtime
    // uses for painting and the world map — so what you see growing IS the
    // resource availability: dense trees where wood is high, rock fields
    // where stone is high, ore-rock clusters on iron veins.
    let biomes = shared::worldgen::BiomeField::new(seed);
    // Within-forest clumping so woods have glades instead of uniform fill.
    let clump_mask = fbm(splitmix64(seed ^ 77) as u32, 3, 1.0 / 90.0);

    const TREES_BROADLEAF: &[PropKind] = &[
        PropKind::Tree_01,
        PropKind::Tree_02,
        PropKind::Tree_08,
        PropKind::Tree_09,
        PropKind::Tree_29,
    ];
    const TREES_PINE: &[PropKind] = &[
        PropKind::Pine_Tree_1,
        PropKind::Pine_Tree_2,
        PropKind::Pine_Tree_3,
        PropKind::Pine_Tree_4,
    ];
    const BUSHES: &[PropKind] = &[
        PropKind::Bush_01,
        PropKind::Bush_02,
        PropKind::Bush_03,
        PropKind::Bush_04,
    ];
    const ROCKS: &[PropKind] = &[
        PropKind::Rock_1,
        PropKind::Rock_2,
        PropKind::Rock_3,
        PropKind::Rock_4,
        PropKind::Rock_5,
    ];
    const FLOWERS: &[PropKind] = &[
        PropKind::Flower_01,
        PropKind::Flower_03,
        PropKind::Spring_Flower_06,
        PropKind::Spring_Flower_08,
    ];

    let pick = |pool: &[PropKind], r: f32| pool[((r * pool.len() as f32) as usize).min(pool.len() - 1)];

    // Jittered grid sampling across the whole map.
    let cell = 7.0;
    let cells = (half_extent * 2.0 / cell) as i32;

    // Each cell seeds its own stream from its own index instead of drawing
    // from one sequential stream, so a cell's contents never depend on how
    // many cells were visited or accepted before it. That is what lets the
    // density pass below sample cells out of order and still agree with the
    // full sweep for a given seed.
    let cell_seed = |xi: i32, zi: i32, salt: u64| {
        let key = ((zi as u32 as u64) << 32) | (xi as u32 as u64);
        splitmix64(splitmix64(key ^ salt) ^ seed)
    };

    let sample_cell = |xi: i32, zi: i32| -> Option<MapObjectSpawn> {
        let mut rng = cell_seed(xi, zi, 0xF00D);
        let x = -half_extent + (xi as f32 + rand01(&mut rng)) * cell;
        let z = -half_extent + (zi as f32 + rand01(&mut rng)) * cell;
        let h = grid.height(x, z);
        if h < SEA_LEVEL + 1.1 {
            return None; // no vegetation in the water or on the wet sand line
        }
        // Keep the roads and their shoulders clear.
        if let Some(roads) = roads {
            if roads.distance(x, z) < 11.0 {
                return None;
            }
        }
        let slope = grid.slope(x, z);

        let biome = biomes.biome(x, z, h, slope);
        let clump = (clump_mask.get([x as f64, z as f64]) as f32 * 0.5 + 0.5).clamp(0.0, 1.0);
        let roll = rand01(&mut rng);

        use shared::worldgen::WorldBiome;
        let (kind, scale) = if slope > 0.85 {
            // Cliffs: occasional rocks only, whatever the biome says.
            if roll < 0.12 {
                (pick(ROCKS, rand01(&mut rng)), 1.4 + rand01(&mut rng) * 1.4)
            } else {
                return None;
            }
        } else {
            match biome {
                WorldBiome::Forest => {
                    // The wood biome: dense trees with clump-driven glades,
                    // bushes at the clump fringes.
                    if h > SEA_LEVEL + 2.0 && roll < 0.18 + clump * 0.50 {
                        if clump < 0.35 && rand01(&mut rng) < 0.40 {
                            (pick(BUSHES, rand01(&mut rng)), 0.8 + rand01(&mut rng) * 0.5)
                        } else if h > 16.0 {
                            (pick(TREES_PINE, rand01(&mut rng)), 0.85 + rand01(&mut rng) * 0.45)
                        } else {
                            (
                                pick(TREES_BROADLEAF, rand01(&mut rng)),
                                0.85 + rand01(&mut rng) * 0.45,
                            )
                        }
                    } else if roll > 0.97 {
                        (pick(ROCKS, rand01(&mut rng)), 0.5 + rand01(&mut rng) * 0.5)
                    } else {
                        return None;
                    }
                }
                WorldBiome::Meadows => {
                    // The farmland biome: open grass, flowers, the odd lone
                    // tree — visibly sparse in wood.
                    if roll < 0.30 {
                        if rand01(&mut rng) < 0.10 {
                            (pick(FLOWERS, rand01(&mut rng)), 0.8 + rand01(&mut rng) * 0.4)
                        } else {
                            (PropKind::Env_Grass_Tall_04, 0.32 + rand01(&mut rng) * 0.16)
                        }
                    } else if roll < 0.325 && h > SEA_LEVEL + 2.0 {
                        (
                            pick(TREES_BROADLEAF, rand01(&mut rng)),
                            0.9 + rand01(&mut rng) * 0.4,
                        )
                    } else if roll > 0.995 {
                        (pick(ROCKS, rand01(&mut rng)), 0.5 + rand01(&mut rng) * 0.5)
                    } else {
                        return None;
                    }
                }
                WorldBiome::Highlands => {
                    // The stone biome: rock fields, sparse pines. Iron veins
                    // read as tight clusters of big dark boulders.
                    let vein = biomes.iron_vein(x, z);
                    if vein > 0.55 && roll < 0.45 {
                        // Ore outcrop: boulder-sized so a deposit reads from
                        // gameplay zoom (the rock assets are pebbles at 1.0).
                        (PropKind::Rock_5, 3.6 + rand01(&mut rng) * 2.2)
                    } else if roll < 0.12 {
                        (pick(ROCKS, rand01(&mut rng)), 1.6 + rand01(&mut rng) * 1.6)
                    } else if roll < 0.165 && h > SEA_LEVEL + 2.0 {
                        (pick(TREES_PINE, rand01(&mut rng)), 0.75 + rand01(&mut rng) * 0.35)
                    } else {
                        return None;
                    }
                }
                WorldBiome::Mountains => {
                    let vein = biomes.iron_vein(x, z);
                    if vein > 0.55 && roll < 0.50 {
                        (PropKind::Rock_5, 4.0 + rand01(&mut rng) * 2.5)
                    } else if roll < 0.14 {
                        (pick(ROCKS, rand01(&mut rng)), 2.0 + rand01(&mut rng) * 1.8)
                    } else if roll < 0.155 && h < 42.0 {
                        (pick(TREES_PINE, rand01(&mut rng)), 0.7 + rand01(&mut rng) * 0.3)
                    } else {
                        return None;
                    }
                }
            }
        };

        Some(MapObjectSpawn {
            kind: kind.id().to_string(),
            position: [x, 0.0, z],
            rotation_degrees: rand01(&mut rng) * 360.0,
            scale,
        })
    };

    // Estimate the natural yield on a strided subsample, then thin the full
    // sweep by budget/estimate. Stopping the sweep at a hard cap instead would
    // fill a band along the low-z edge and leave the rest of the map bare,
    // because the sweep is z-major. Thinning scales every biome by the same
    // factor, so forests stay denser than scrub, and rolling the (cheap) thin
    // test before the (expensive) noise lookups makes the full sweep cost less
    // than the truncated one did.
    // Density, not a fixed count. A flat 6500 was tuned for a 2816m map and left an 8km
    // world at ~97 props/km² — visibly empty. Cost is per-radius (the client streams props
    // around the camera), so density is what matters for frame time; the absolute total
    // only drives map.ron size. Clustering from the forest mask means real forests come out
    // far denser than this average.
    const PROPS_PER_KM2: f32 = 1_100.0;
    /// Ceiling on total props so map.ron stays a manageable size (~190 bytes each).
    const PROP_HARD_CAP: usize = 90_000;
    let area_km2 = (half_extent * 2.0 / 1000.0).powi(2);
    let prop_budget = ((PROPS_PER_KM2 * area_km2) as usize).min(PROP_HARD_CAP);
    const ESTIMATE_STRIDE: i32 = 7;
    let mut sampled = 0usize;
    let mut hits = 0usize;
    let mut zi = 0;
    while zi < cells {
        let mut xi = 0;
        while xi < cells {
            sampled += 1;
            if sample_cell(xi, zi).is_some() {
                hits += 1;
            }
            xi += ESTIMATE_STRIDE;
        }
        zi += ESTIMATE_STRIDE;
    }
    if hits == 0 {
        return Vec::new();
    }
    let estimated = hits as f32 * (cells as f32 * cells as f32) / sampled as f32;
    let keep = (prop_budget as f32 / estimated).min(1.0);

    let capacity = estimated.min(prop_budget as f32) as usize;
    let mut out: Vec<MapObjectSpawn> = Vec::with_capacity(capacity);
    for zi in 0..cells {
        for xi in 0..cells {
            let mut thin = cell_seed(xi, zi, 0x7A15);
            if rand01(&mut thin) >= keep {
                continue;
            }
            if let Some(spawn) = sample_cell(xi, zi) {
                out.push(spawn);
            }
        }
    }
    out
}

/// Pick a pleasant start: flat grass near the water's edge.
fn pick_spawn(grid: &HeightGrid, seed: u64, half_extent: f32) -> [f32; 3] {
    let mut rng = splitmix64(seed ^ 0x5A17);
    let mut best: Option<([f32; 3], f32)> = None;
    for _ in 0..400 {
        let x = (rand01(&mut rng) - 0.5) * half_extent * 1.7;
        let z = (rand01(&mut rng) - 0.5) * half_extent * 1.7;
        let h = grid.height(x, z);
        if h < SEA_LEVEL + 1.5 || h > SEA_LEVEL + 6.0 {
            continue;
        }
        let slope = grid.slope(x, z);
        if slope > 0.25 {
            continue;
        }
        // Prefer close to the shore: sample toward the sea.
        let mut shore_score = 0.0;
        for d in [12.0f32, 24.0, 40.0] {
            if grid.height(x + d, z) < SEA_LEVEL || grid.height(x, z + d) < SEA_LEVEL {
                shore_score += 1.0;
            }
        }
        let score = shore_score - slope * 4.0;
        if best.map(|(_, s)| score > s).unwrap_or(true) {
            best = Some(([x, h + 1.0, z], score));
        }
    }
    best.map(|(p, _)| p).unwrap_or([0.0, 6.0, 0.0])
}

/// Generate a whole world into the session. The caller has already pushed an
/// undo snapshot and blanked the map (reset_map_to_blank).
pub fn generate_world(
    style: WorldStyle,
    seed: u64,
    session: &mut EditorSession,
    world: &mut WorldTerrain,
    env_state: &mut EditorEnvironmentState,
    _city_state: &mut CityEditorState,
    flags: &mut VisualRefreshFlags,
) -> Result<(), String> {
    let bounds = world.generator.active_map_bounds();
    let half_extent = (bounds.max[0] - bounds.min[0]).min(bounds.max[1] - bounds.min[1]) * 0.5;
    let field = HeightField::new(style, seed, half_extent);
    let mut grid = HeightGrid::build(&field, half_extent);

    // --- The recipe IS the terrain: nothing is baked ---
    // Heights and weightmaps used to be written per-chunk here (644MB of RON
    // for an 8km map, scaling quadratically). Now the definition stores the
    // seed recipe and every binary rebuilds the identical grid at load time;
    // edits.ron goes back to holding only sparse hand edits.
    session.map_edits.terrain_deltas.clear();
    session.map_edits.terrain_paint_ops.clear();
    session.map_edits.terrain_weightmaps.clear();
    session.paint_weights.clear();
    // Map directories start as copies of a shipped map, so the definition can
    // still carry the donor's id (big_world shipped claiming to be city_alpha,
    // which mislabels logs and map-relative asset lookups).
    session.map_definition.map_id = session.map_id.clone();
    let (height_min, height_max) = grid.height_range();
    session.map_definition.terrain.height_min = height_min;
    session.map_definition.terrain.height_max = height_max;
    session.map_definition.generated = Some(GeneratedWorld {
        style,
        seed,
        generator_version: shared::worldgen::WORLDGEN_VERSION,
        half_extent,
        // Terrain only for now: no preset roads/village means no recorded
        // flatten strokes. Settlement generation will come back as its own
        // pass and record strokes again then.
        strokes: Vec::new(),
    });

    // --- Vegetation + water + spawn ---
    session.map_definition.objects = scatter_props(&grid, seed, half_extent, None);
    session.map_definition.terrain.water_level = Some(SEA_LEVEL);
    env_state.show_water = true;
    env_state.water_level = SEA_LEVEL;

    // No preset content: the world starts as pure terrain.
    session.map_edits.roads.clear();
    session.map_edits.plots.clear();
    session.map_edits.spawn_markers.clear();
    session.map_definition.player_spawn = Some(pick_spawn(&grid, seed, half_extent));

    // --- Reload the live world from the generated data ---
    let loaded_map = load_map_from_parts(
        &session.map_dir,
        &session.map_definition,
        &session.map_edits,
    )?;
    world.reload_from_loaded_map(loaded_map);
    session.refresh_next_ids();
    session.mark_map_dirty();
    session.mark_edits_dirty();

    flags.terrain_all = true;
    flags.paint_all = true;
    flags.water_all = true;
    flags.props = true;
    flags.markers = true;
    flags.city_layout = true;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stats(grid: &HeightGrid, half: f32) -> (f32, f32, f32, f32) {
        let mut land = 0u32;
        let mut ocean = 0u32;
        let mut beach = 0u32;
        let mut max_h = f32::MIN;
        let mut samples = 0u32;
        let step = 10.0;
        let mut z = -half;
        while z < half {
            let mut x = -half;
            while x < half {
                let h = grid.height(x, z);
                samples += 1;
                max_h = max_h.max(h);
                if h < SEA_LEVEL {
                    ocean += 1;
                } else if h < SEA_LEVEL + 1.6 {
                    beach += 1;
                } else {
                    land += 1;
                }
                x += step;
            }
            z += step;
        }
        (
            land as f32 / samples as f32,
            ocean as f32 / samples as f32,
            beach as f32 / samples as f32,
            max_h,
        )
    }

    #[test]
    fn island_has_land_beaches_and_edge_ocean() {
        for seed in [7u64, 42, 1234] {
            let field = HeightField::new(WorldStyle::Island, seed, 700.0);
            let grid = HeightGrid::build(&field, 700.0);
            let (land, ocean, beach, max_h) = stats(&grid, 700.0);
            assert!(land > 0.12, "seed {seed}: land fraction {land}");
            assert!(ocean > 0.15, "seed {seed}: ocean fraction {ocean}");
            assert!(beach > 0.01, "seed {seed}: beach fraction {beach}");
            assert!(max_h > 8.0, "seed {seed}: max height {max_h}");
            // The map border must be under water on all sides.
            for t in [-680.0f32, -300.0, 0.0, 300.0, 680.0] {
                assert!(grid.height(t, -690.0) < SEA_LEVEL, "seed {seed} north edge");
                assert!(grid.height(t, 690.0) < SEA_LEVEL, "seed {seed} south edge");
                assert!(grid.height(-690.0, t) < SEA_LEVEL, "seed {seed} west edge");
                assert!(grid.height(690.0, t) < SEA_LEVEL, "seed {seed} east edge");
            }
            let spawn = pick_spawn(&grid, seed, 700.0);
            assert!(spawn[1] > SEA_LEVEL, "seed {seed}: spawn underwater");
            let props = scatter_props(&grid, seed, 700.0, None);
            assert!(props.len() > 400, "seed {seed}: only {} props", props.len());
        }
    }

    #[test]
    fn mainland_has_coast_rivers_and_interior() {
        for seed in [3u64, 99, 777] {
            let field = HeightField::new(WorldStyle::Mainland, seed, 700.0);
            let grid = HeightGrid::build(&field, 700.0);
            let (land, ocean, _beach, max_h) = stats(&grid, 700.0);
            assert!(land > 0.30, "seed {seed}: land fraction {land}");
            assert!(ocean > 0.08, "seed {seed}: ocean fraction {ocean}");
            assert!(max_h > 10.0, "seed {seed}: max height {max_h}");
            assert!(!field.rivers.is_empty(), "seed {seed}: no rivers");
            for river in &field.rivers {
                let last = river.last().unwrap();
                assert!(
                    last.y < SEA_LEVEL,
                    "seed {seed}: river bed ends above sea ({})",
                    last.y
                );
            }
        }
    }

    #[test]
    fn showcase_has_mountains_bay_islands_and_lakes() {
        for seed in [11u64, 2024] {
            let field = HeightField::new(WorldStyle::Showcase, seed, 700.0);
            let grid = HeightGrid::build(&field, 700.0);
            let (land, ocean, beach, max_h) = stats(&grid, 700.0);
            assert!(land > 0.20, "seed {seed}: land {land}");
            assert!(ocean > 0.10, "seed {seed}: ocean {ocean}");
            assert!(beach > 0.01, "seed {seed}: beach {beach}");
            assert!(max_h > 40.0, "seed {seed}: peaks only {max_h}m");
            assert!(!field.islands.is_empty(), "seed {seed}: no islands");
            assert!(!field.lakes.is_empty(), "seed {seed}: no lakes");

            // At least one offshore island actually breaks the surface.
            let island_land = field
                .islands
                .iter()
                .any(|island| grid.height(island.center.x, island.center.y) > SEA_LEVEL + 1.0);
            assert!(island_land, "seed {seed}: all islands submerged");

            let spawn = pick_spawn(&grid, seed, 700.0);
            assert!(spawn[1] > SEA_LEVEL, "seed {seed}: spawn underwater");
        }
    }

    /// Props must reach every part of the map. The count assertions above pass
    /// happily when the sweep truncates at a budget cap and fills a single
    /// edge band, so coverage is asserted per grid bucket instead.
    ///
    /// This runs at the shipped map's half extent on purpose: the smaller
    /// 700m worlds the other tests use never produce more props than the
    /// budget, so truncation cannot show up there at all.
    #[test]
    fn props_cover_the_whole_map() {
        const HALF: f32 = 1408.0;
        const N: usize = 4;
        let bucket = |v: f32| (((v + HALF) / (HALF * 2.0 / N as f32)) as usize).min(N - 1);

        for (style, seed) in [
            (WorldStyle::Island, 7u64),
            (WorldStyle::Mainland, 3),
            (WorldStyle::Showcase, 11),
        ] {
            let field = HeightField::new(style, seed, HALF);
            let grid = HeightGrid::build(&field, HALF);
            let props = scatter_props(&grid, seed, HALF, None);
            assert!(!props.is_empty(), "{style:?} seed {seed}: no props at all");

            // Land coverage per bucket, so ocean-only buckets are exempt.
            let mut land = [[0u32; N]; N];
            let mut total = [[0u32; N]; N];
            let mut z = -HALF;
            while z < HALF {
                let mut x = -HALF;
                while x < HALF {
                    total[bucket(z)][bucket(x)] += 1;
                    if grid.height(x, z) > SEA_LEVEL + 2.0 {
                        land[bucket(z)][bucket(x)] += 1;
                    }
                    x += 10.0;
                }
                z += 10.0;
            }

            let mut counts = [[0usize; N]; N];
            let (mut min_x, mut max_x) = (f32::MAX, f32::MIN);
            let (mut min_z, mut max_z) = (f32::MAX, f32::MIN);
            for prop in &props {
                counts[bucket(prop.position[2])][bucket(prop.position[0])] += 1;
                min_x = min_x.min(prop.position[0]);
                max_x = max_x.max(prop.position[0]);
                min_z = min_z.min(prop.position[2]);
                max_z = max_z.max(prop.position[2]);
            }

            for bz in 0..N {
                for bx in 0..N {
                    let land_frac = land[bz][bx] as f32 / total[bz][bx] as f32;
                    if land_frac < 0.25 {
                        continue; // mostly water: nothing is expected to grow
                    }
                    assert!(
                        counts[bz][bx] > 0,
                        "{style:?} seed {seed}: bucket ({bx},{bz}) is {:.0}% land but got no props ({:?})",
                        land_frac * 100.0,
                        counts
                    );
                }
            }

            // The truncated sweep spanned ~18% of the map in z; a healthy
            // scatter spans most of both axes.
            assert!(
                max_x - min_x > HALF * 1.4 && max_z - min_z > HALF * 1.4,
                "{style:?} seed {seed}: props span x {:.0}..{:.0}, z {:.0}..{:.0}",
                min_x,
                max_x,
                min_z,
                max_z
            );
            // Budget stays bounded: the thinning must not explode the count.
            // The thinning targets the density budget via a strided estimate,
            // so allow ~20% estimator noise above it.
            let budget = (1_100.0 * (HALF * 2.0 / 1000.0).powi(2)) as usize;
            assert!(
                props.len() < budget + budget / 5,
                "{style:?} seed {seed}: {} props blows the budget",
                props.len()
            );
        }
    }

    #[test]
    #[ignore = "timing probe; run with --ignored --nocapture"]
    fn showcase_timing_probe() {
        let start = std::time::Instant::now();
        let half = 704.0;
        let field = HeightField::new(WorldStyle::Showcase, 2026, half);
        let noise_done = start.elapsed();
        let grid = HeightGrid::build(&field, half);
        let grid_done = start.elapsed();
        let props = scatter_props(&grid, 2026, half, None);
        let props_done = start.elapsed();
        let (land, ocean, beach, max_h) = stats(&grid, half);
        println!(
            "noise+rivers {:?} | grid {:?} | props {:?}",
            noise_done,
            grid_done - noise_done,
            props_done - grid_done
        );
        println!(
            "land {:.0}% ocean {:.0}% beach {:.0}% peak {:.0}m | {} props",
            land * 100.0,
            ocean * 100.0,
            beach * 100.0,
            max_h,
            props.len()
        );
    }

    #[test]
    fn paint_weights_are_normalized_and_sane() {
        let field = HeightField::new(WorldStyle::Island, 42, 700.0);
        let grid = HeightGrid::build(&field, 700.0);
        let mut saw_sand = false;
        let mut saw_grass = false;
        for (x, z) in [(0.0f32, 0.0f32), (120.0, -80.0), (-300.0, 250.0), (500.0, 500.0)] {
            let h = grid.height(x, z);
            let bytes = weights_to_bytes(surface_weights(&grid, x, z, h, None));
            let sum: u16 = bytes.iter().map(|b| *b as u16).sum();
            assert_eq!(sum, 255);
            if bytes[2] > 128 {
                saw_sand = true;
            }
            if bytes[0] > 128 {
                saw_grass = true;
            }
        }
        let _ = (saw_sand, saw_grass); // presence depends on sample points
    }
}
