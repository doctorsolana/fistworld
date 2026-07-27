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
    let forest_mask = fbm(splitmix64(seed ^ 77) as u32, 3, 1.0 / 260.0);
    let meadow_mask = fbm(splitmix64(seed ^ 78) as u32, 3, 1.0 / 150.0);

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

        let forest = (forest_mask.get([x as f64, z as f64]) as f32 * 0.5 + 0.5).clamp(0.0, 1.0);
        let meadow = (meadow_mask.get([x as f64, z as f64]) as f32 * 0.5 + 0.5).clamp(0.0, 1.0);
        let roll = rand01(&mut rng);

        let (kind, scale) = if slope > 0.85 {
            // Steep ground: occasional rocks only.
            if roll < 0.12 {
                (pick(ROCKS, rand01(&mut rng)), 0.8 + rand01(&mut rng) * 0.7)
            } else {
                return None;
            }
        } else if forest > 0.62 && h > SEA_LEVEL + 2.0 && roll < (forest - 0.45) * 1.6 {
            // Forest: pines up high, broadleaf low, bushes at the fringe.
            let fringe = forest < 0.70;
            if fringe && rand01(&mut rng) < 0.35 {
                (pick(BUSHES, rand01(&mut rng)), 0.8 + rand01(&mut rng) * 0.5)
            } else if h > 16.0 {
                (pick(TREES_PINE, rand01(&mut rng)), 0.85 + rand01(&mut rng) * 0.45)
            } else {
                (
                    pick(TREES_BROADLEAF, rand01(&mut rng)),
                    0.85 + rand01(&mut rng) * 0.45,
                )
            }
        } else if meadow > 0.55 && forest < 0.6 && roll < 0.5 {
            // Meadows: dense grass with sparse flowers.
            if rand01(&mut rng) < 0.06 {
                (pick(FLOWERS, rand01(&mut rng)), 0.8 + rand01(&mut rng) * 0.4)
            } else {
                (PropKind::Env_Grass_Tall_04, 0.32 + rand01(&mut rng) * 0.16)
            }
        } else if roll < 0.02 {
            (pick(ROCKS, rand01(&mut rng)), 0.5 + rand01(&mut rng) * 0.6)
        } else {
            return None;
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

    // Showcase: landmarks, pathfound roads, and the harbour village. This
    // runs BEFORE the recipe is recorded because it beds roads into the
    // terrain grid (and those strokes are part of the recipe).
    let showcase = (style == WorldStyle::Showcase)
        .then(|| build_showcase_content(&field, &mut grid, seed, half_extent));
    let road_mask = showcase.as_ref().map(|content| &content.road_mask);

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
        half_extent,
        strokes: showcase
            .as_ref()
            .map(|content| content.strokes.clone())
            .unwrap_or_default(),
    });

    // --- Vegetation + water + spawn ---
    session.map_definition.objects = scatter_props(&grid, seed, half_extent, road_mask);
    session.map_definition.terrain.water_level = Some(SEA_LEVEL);
    env_state.show_water = true;
    env_state.water_level = SEA_LEVEL;

    if let Some(content) = showcase {
        session.map_edits.roads = content.roads;
        session.map_edits.plots = content.plots;
        session.map_edits.spawn_markers = content.markers;
        session.map_definition.player_spawn = Some(content.spawn);
    } else {
        session.map_definition.player_spawn = Some(pick_spawn(&grid, seed, half_extent));
    }

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
    fn showcase_has_mountains_bay_islands_and_roads() {
        for seed in [11u64, 2024] {
            let field = HeightField::new(WorldStyle::Showcase, seed, 700.0);
            let mut grid = HeightGrid::build(&field, 700.0);
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

            let content = build_showcase_content(&field, &mut grid, seed, 700.0);
            assert!(
                content.roads.len() >= 2,
                "seed {seed}: only {} roads",
                content.roads.len()
            );
            assert_eq!(content.plots.len(), 8, "seed {seed}: village plots");
            assert!(content.markers.len() >= 4, "seed {seed}: landmarks");
            assert!(
                content.spawn[1] > SEA_LEVEL,
                "seed {seed}: village spawn underwater"
            );
            // Roads must be drivable: gentle slope along the bedded corridor.
            for road in &content.roads {
                for window in road.points.windows(2) {
                    let a = Vec2::new(window[0][0], window[0][1]);
                    let b = Vec2::new(window[1][0], window[1][1]);
                    let ha = grid.height(a.x, a.y);
                    let hb = grid.height(b.x, b.y);
                    let run = a.distance(b).max(1.0);
                    let grade = (hb - ha).abs() / run;
                    assert!(
                        grade < 0.65,
                        "seed {seed}: road grade {grade} over {run}m"
                    );
                    assert!(ha > SEA_LEVEL - 0.5, "seed {seed}: road underwater");
                }
            }
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
            let mut grid = HeightGrid::build(&field, HALF);
            // Showcase carries a road mask through to the scatter, so run the
            // same pipeline the editor does.
            let showcase = (style == WorldStyle::Showcase)
                .then(|| build_showcase_content(&field, &mut grid, seed, HALF));
            let props = scatter_props(
                &grid,
                seed,
                HALF,
                showcase.as_ref().map(|content| &content.road_mask),
            );
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
            assert!(
                props.len() < 9000,
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
        let mut grid = HeightGrid::build(&field, half);
        let grid_done = start.elapsed();
        let content = build_showcase_content(&field, &mut grid, 2026, half);
        let roads_done = start.elapsed();
        let props = scatter_props(&grid, 2026, half, Some(&content.road_mask));
        let props_done = start.elapsed();
        let (land, ocean, beach, max_h) = stats(&grid, half);
        println!(
            "noise+rivers {:?} | grid {:?} | roads {:?} | props {:?}",
            noise_done,
            grid_done - noise_done,
            roads_done - grid_done,
            props_done - roads_done
        );
        println!(
            "land {:.0}% ocean {:.0}% beach {:.0}% peak {:.0}m | {} roads, {} plots, {} props",
            land * 100.0,
            ocean * 100.0,
            beach * 100.0,
            max_h,
            content.roads.len(),
            content.plots.len(),
            props.len()
        );
        for road in &content.roads {
            let len: f32 = road
                .points
                .windows(2)
                .map(|w| Vec2::new(w[0][0], w[0][1]).distance(Vec2::new(w[1][0], w[1][1])))
                .sum();
            println!("  road {:?}: {:.0}m, {} points", road.road_class, len, road.points.len());
        }
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

// ============================================================================
// Showcase extras: landmarks, roads, village
// ============================================================================

/// A landmark the road network connects.
struct Landmark {
    name: &'static str,
    pos: Vec2,
    kind: shared::map::SpawnMarkerKind,
}

/// Find the flattest land near a target point (spiral search).
fn find_flat_near(grid: &HeightGrid, target: Vec2, search: f32, min_h: f32, max_h: f32) -> Vec2 {
    let mut best = target;
    let mut best_score = f32::MAX;
    let steps = 26;
    for ring in 0..steps {
        let r = search * (ring as f32 / steps as f32);
        let samples = 8 + ring * 2;
        for s in 0..samples {
            let a = (s as f32 / samples as f32) * std::f32::consts::TAU;
            let p = target + Vec2::new(a.cos(), a.sin()) * r;
            let h = grid.height(p.x, p.y);
            if h < min_h || h > max_h {
                continue;
            }
            let score = grid.slope(p.x, p.y) * 10.0 + r / search;
            if score < best_score {
                best_score = score;
                best = p;
            }
        }
    }
    best
}

/// A* over a coarse grid: roads prefer gentle ground and avoid water, so
/// they naturally follow valleys and contour around mountains.
fn find_road_path(grid: &HeightGrid, half_extent: f32, from: Vec2, to: Vec2) -> Option<Vec<Vec2>> {
    use std::collections::BinaryHeap;

    const CELL: f32 = 16.0;
    let size = ((half_extent * 2.0) / CELL) as usize + 1;
    let to_cell = |p: Vec2| -> (usize, usize) {
        (
            (((p.x + half_extent) / CELL).round().clamp(0.0, (size - 1) as f32)) as usize,
            (((p.y + half_extent) / CELL).round().clamp(0.0, (size - 1) as f32)) as usize,
        )
    };
    let to_world =
        |cx: usize, cz: usize| Vec2::new(cx as f32 * CELL - half_extent, cz as f32 * CELL - half_extent);

    let start = to_cell(from);
    let goal = to_cell(to);

    // Per-cell traversal cost (impassable = None).
    let cell_cost = |cx: usize, cz: usize| -> Option<f32> {
        let p = to_world(cx, cz);
        let h = grid.height(p.x, p.y);
        if h < SEA_LEVEL + 1.2 {
            return None; // water / tidal flats
        }
        let slope = grid.slope(p.x, p.y);
        if slope > 1.15 {
            return None; // cliffs
        }
        Some(1.0 + slope * 14.0)
    };

    #[derive(PartialEq)]
    struct Node(f32, usize, usize);
    impl Eq for Node {}
    impl Ord for Node {
        fn cmp(&self, other: &Self) -> std::cmp::Ordering {
            other.0.total_cmp(&self.0) // min-heap
        }
    }
    impl PartialOrd for Node {
        fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
            Some(self.cmp(other))
        }
    }

    let idx = |cx: usize, cz: usize| cz * size + cx;
    let mut g = vec![f32::MAX; size * size];
    let mut came: Vec<Option<(usize, usize)>> = vec![None; size * size];
    let mut open = BinaryHeap::new();

    g[idx(start.0, start.1)] = 0.0;
    open.push(Node(0.0, start.0, start.1));

    let heuristic = |cx: usize, cz: usize| {
        let dx = cx as f32 - goal.0 as f32;
        let dz = cz as f32 - goal.1 as f32;
        (dx * dx + dz * dz).sqrt()
    };

    let mut found = false;
    while let Some(Node(_, cx, cz)) = open.pop() {
        if (cx, cz) == goal {
            found = true;
            break;
        }
        let current_g = g[idx(cx, cz)];
        for (dx, dz) in [
            (1i32, 0i32),
            (-1, 0),
            (0, 1),
            (0, -1),
            (1, 1),
            (1, -1),
            (-1, 1),
            (-1, -1),
        ] {
            let nx = cx as i32 + dx;
            let nz = cz as i32 + dz;
            if nx < 0 || nz < 0 || nx >= size as i32 || nz >= size as i32 {
                continue;
            }
            let (nx, nz) = (nx as usize, nz as usize);
            let Some(cost) = cell_cost(nx, nz) else {
                continue;
            };
            let diagonal = dx != 0 && dz != 0;
            let step = if diagonal { 1.414 } else { 1.0 };
            let tentative = current_g + cost * step;
            if tentative < g[idx(nx, nz)] {
                g[idx(nx, nz)] = tentative;
                came[idx(nx, nz)] = Some((cx, cz));
                open.push(Node(tentative + heuristic(nx, nz) * 1.05, nx, nz));
            }
        }
    }
    if !found {
        return None;
    }

    // Walk the path back, then smooth it (Chaikin) so roads curve.
    let mut cells = vec![goal];
    let mut cur = goal;
    while let Some(prev) = came[idx(cur.0, cur.1)] {
        cells.push(prev);
        cur = prev;
        if cells.len() > size * size {
            break;
        }
    }
    cells.reverse();
    let mut path: Vec<Vec2> = cells.iter().map(|(cx, cz)| to_world(*cx, *cz)).collect();
    for _ in 0..2 {
        path = chaikin(&path);
    }
    // Thin out dense points.
    let mut thinned = Vec::with_capacity(path.len() / 2 + 2);
    for (i, p) in path.iter().enumerate() {
        if i == 0 || i == path.len() - 1 || thinned.last().map(|l: &Vec2| l.distance(*p) > 9.0).unwrap_or(true) {
            thinned.push(*p);
        }
    }
    (thinned.len() >= 2).then_some(thinned)
}

/// Corner-cutting smoothing.
fn chaikin(points: &[Vec2]) -> Vec<Vec2> {
    if points.len() < 3 {
        return points.to_vec();
    }
    let mut out = Vec::with_capacity(points.len() * 2);
    out.push(points[0]);
    for window in points.windows(2) {
        let (a, b) = (window[0], window[1]);
        out.push(a * 0.75 + b * 0.25);
        out.push(a * 0.25 + b * 0.75);
    }
    out.push(*points.last().expect("non-empty"));
    out
}

/// Everything the showcase adds on top of the terrain.
pub struct ShowcaseContent {
    pub roads: Vec<shared::city::MapRoad>,
    pub plots: Vec<shared::city::MapPlot>,
    pub markers: Vec<shared::map::MapSpawnMarker>,
    pub road_mask: RoadMask,
    /// Road-flattening strokes in application order, recorded for the
    /// world recipe so load-time terrain rebuilds replay them exactly.
    pub strokes: Vec<FlattenStroke>,
    pub spawn: [f32; 3],
}

/// Place landmarks, connect them with pathfound roads, bed the roads into
/// the terrain, and lay out a harbour village.
fn build_showcase_content(
    field: &HeightField,
    grid: &mut HeightGrid,
    seed: u64,
    half_extent: f32,
) -> ShowcaseContent {
    use shared::city::{MapPlot, MapRoad, PlotZone, RoadClass};
    use shared::map::{MapSpawnMarker, SpawnMarkerKind};

    let mut rng = splitmix64(seed ^ 0xB00C);
    let lateral = Vec2::new(-field.coast_dir.y, field.coast_dir.x);

    // --- Landmarks -------------------------------------------------------
    let village = find_flat_near(grid, field.plains_center, half_extent * 0.25, 2.0, 9.0);
    // Harbour: flat ground close to the water, out toward the bay.
    let harbour_target = village + field.coast_dir * half_extent * 0.30;
    let harbour = find_flat_near(grid, harbour_target, half_extent * 0.22, 1.6, 5.0);
    // Mountain waypoint: high ground on the spine.
    let mut peak = field.range_dir * (rand01(&mut rng) - 0.5) * half_extent
        + Vec2::new(-field.range_dir.y, field.range_dir.x) * field.range_offset;
    peak = find_flat_near(grid, peak, half_extent * 0.20, 18.0, 70.0);
    // Lake shore camp.
    let lake_camp = field
        .lakes
        .first()
        .map(|lake| find_flat_near(grid, lake.center + Vec2::new(lake.radius * 1.3, 0.0), half_extent * 0.12, 1.8, 12.0))
        .unwrap_or(village + lateral * half_extent * 0.3);
    // Far outpost inland.
    let outpost = find_flat_near(
        grid,
        village - field.coast_dir * half_extent * 0.45 + lateral * half_extent * 0.25,
        half_extent * 0.22,
        2.0,
        22.0,
    );

    let landmarks = vec![
        Landmark {
            name: "Harbour",
            pos: harbour,
            kind: SpawnMarkerKind::Poi,
        },
        Landmark {
            name: "Village",
            pos: village,
            kind: SpawnMarkerKind::Player,
        },
        Landmark {
            name: "Mountain Pass",
            pos: peak,
            kind: SpawnMarkerKind::Poi,
        },
        Landmark {
            name: "Lake Camp",
            pos: lake_camp,
            kind: SpawnMarkerKind::Poi,
        },
        Landmark {
            name: "Outpost",
            pos: outpost,
            kind: SpawnMarkerKind::NpcGroup,
        },
    ];

    // --- Road network: village is the hub --------------------------------
    let mut roads: Vec<MapRoad> = Vec::new();
    let mut road_mask = RoadMask::new(half_extent, 4.0);
    let mut next_id = 1u64;

    let connections: [(Vec2, Vec2, RoadClass); 4] = [
        (village, harbour, RoadClass::Collector),
        (village, peak, RoadClass::Local),
        (village, lake_camp, RoadClass::Local),
        (village, outpost, RoadClass::Collector),
    ];

    let mut road_paths: Vec<Vec<Vec2>> = Vec::new();
    for (from, to, class) in connections {
        let Some(path) = find_road_path(grid, half_extent, from, to) else {
            continue;
        };
        road_mask.stamp_path(&path, 26.0);
        roads.push(MapRoad {
            id: next_id,
            points: path.iter().map(|p| [p.x, p.y]).collect(),
            width: class.default_width(),
            road_class: class,
            lane_count: class.default_lane_count(),
            sidewalk_left: false,
            sidewalk_right: false,
            sidewalk_width: 0.0,
            parking_left: false,
            parking_right: false,
            district: None,
        });
        next_id += 1;
        road_paths.push(path);
    }

    // Bed the roads into the terrain: flatten a corridor along each path so
    // vehicles can actually drive them. Each stroke is recorded (with the
    // beds it computed from the grid at this moment) for load-time replay.
    let mut strokes: Vec<FlattenStroke> = Vec::new();
    for path in &road_paths {
        if let Some(mut stroke) = grid.flatten_along_path(path, 7.0, 16.0) {
            stroke.mask_influence = 26.0;
            strokes.push(stroke);
        }
    }

    // --- Village: a short main street with plots either side -------------
    let street_dir = (harbour - village).normalize_or(field.coast_dir);
    let street_a = village - street_dir * 55.0;
    let street_b = village + street_dir * 55.0;
    if let Some(mut stroke) = grid.flatten_along_path(&[street_a, street_b], 10.0, 26.0) {
        stroke.mask_influence = 24.0;
        strokes.push(stroke);
    }
    road_mask.stamp_path(&[street_a, street_b], 24.0);
    roads.push(MapRoad {
        id: next_id,
        points: vec![[street_a.x, street_a.y], [street_b.x, street_b.y]],
        width: RoadClass::Local.default_width(),
        road_class: RoadClass::Local,
        lane_count: RoadClass::Local.default_lane_count(),
        sidewalk_left: true,
        sidewalk_right: true,
        sidewalk_width: RoadClass::Local.default_sidewalk_width(),
        parking_left: false,
        parking_right: false,
        district: Some("village".to_string()),
    });
    let street_id = next_id;
    next_id += 1;

    let mut plots: Vec<MapPlot> = Vec::new();
    let side = Vec2::new(-street_dir.y, street_dir.x);
    let building_kinds = [
        shared::city::CityBuildingKind::Multistory01,
        shared::city::CityBuildingKind::Multistory03,
        shared::city::CityBuildingKind::Multistory05,
        shared::city::CityBuildingKind::Multistory07,
    ];
    for i in 0..8 {
        let t = (i / 2) as f32 - 1.5;
        let sign = if i % 2 == 0 { 1.0 } else { -1.0 };
        let center = village + street_dir * (t * 26.0) + side * (sign * 19.0);
        plots.push(MapPlot {
            id: next_id,
            center: [center.x, center.y],
            half_extents: [8.0, 8.0],
            rotation_degrees: street_dir.y.atan2(street_dir.x).to_degrees(),
            zone: PlotZone::Residential,
            frontage_road_id: Some(street_id),
            setback: 2.0,
            driveway_side: None,
            archetypes: Vec::new(),
            building_kind: Some(building_kinds[(i as usize) % building_kinds.len()]),
            tags: vec!["village".to_string()],
        });
        next_id += 1;
    }

    // --- Markers ---------------------------------------------------------
    let markers = landmarks
        .iter()
        .enumerate()
        .map(|(i, landmark)| MapSpawnMarker {
            id: i as u64 + 1,
            kind: landmark.kind,
            position: [
                landmark.pos.x,
                grid.height(landmark.pos.x, landmark.pos.y) + 0.5,
                landmark.pos.y,
            ],
            rotation_degrees: 0.0,
            radius: if landmark.name == "Outpost" { 22.0 } else { 8.0 },
        })
        .collect();

    let spawn = [village.x, grid.height(village.x, village.y) + 1.0, village.y];

    ShowcaseContent {
        roads,
        plots,
        markers,
        road_mask,
        strokes,
        spawn,
    }
}
