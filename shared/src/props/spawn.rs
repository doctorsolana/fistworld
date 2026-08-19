use bevy::prelude::*;
use noise::NoiseFn;

use crate::terrain::{ChunkCoord, TerrainGenerator, CHUNK_SIZE};

use crate::props::{PropKind, PropRenderTuning};

use super::tuning::{default_render_tuning, default_unmapped_render_tuning};

/// A single prop spawn (deterministic from world seed + chunk coord).
#[derive(Debug, Clone)]
pub struct PropSpawn {
    pub kind: Option<PropKind>,
    pub scene_path: String,
    pub chunk: ChunkCoord,
    pub position: Vec3,
    pub rotation: Quat,
    pub scale: f32,
    pub render_tuning: PropRenderTuning,
}

/// Collision-relevant authored prop data without scene strings or terrain Y.
/// Road and navigation survey code should use this view instead of paying for
/// the full render spawn when it only needs a ground-plane blocker.
#[derive(Debug, Clone, Copy)]
pub struct BlockingPropSpawn {
    pub kind: PropKind,
    pub position: Vec2,
    pub scale: f32,
}

/// How far from a river centreline the ground is kept clear of anything with a
/// silhouette. The water is [`RIVER_HALF_WIDTH`] + 1.5 m wide, so this leaves a
/// margin of bank beyond the waterline rather than letting trunks stand in the
/// shallows.
const RIVER_CLEARANCE: f32 = crate::worldgen::RIVER_WATER_REACH;

/// Grass stops short of the prop clearance, so a bank keeps its grass right
/// down to the waterline instead of showing a bald strip either side of the
/// river — which is the road look this all exists to undo.
const GRASS_RIVER_CLEARANCE: f32 = crate::worldgen::RIVER_HALF_WIDTH + 1.5;

/// The river segments near one chunk, and a point test against them.
///
/// Built per chunk so the test is against a handful of segments rather than
/// every segment of every river in the world: a 4 km river is ~450 segments and
/// a chunk holds ~60 props, and the naive loop is 27,000 distance tests for a
/// chunk that usually has no river in it at all.
struct RiverReach {
    segments: Vec<(Vec2, Vec2)>,
    radius: f32,
}

impl RiverReach {
    fn for_chunk(terrain: &TerrainGenerator, chunk: ChunkCoord, radius: f32) -> Self {
        let segments = terrain
            .loaded_map()
            .river_segments_by_chunk
            .get(&(chunk.x, chunk.z))
            .cloned()
            .unwrap_or_default();
        Self { segments, radius }
    }

    fn contains(&self, point: Vec2) -> bool {
        self.segments.iter().any(|(a, b)| {
            let seg = *b - *a;
            let t = ((point - *a).dot(seg) / seg.length_squared().max(1e-6)).clamp(0.0, 1.0);
            point.distance_squared(*a + seg * t) < self.radius * self.radius
        })
    }
}

/// Deterministically generate all prop spawns for a given chunk.
///
/// Two sources, and the split is deliberate.
///
/// **Authored**, from `map.ron`: trees, rocks, bushes, flowers. Things with a
/// silhouette, that a designer might want to move, and that the server bakes
/// colliders for.
///
/// **Generated**, separately: ground cover, via [`generate_chunk_grass`]. It is
/// NOT emitted here on purpose — grass draws to 80 m but props stream out to
/// 512 m at full zoom, so folding it into this list would spawn tens of
/// thousands of patches that are never drawn. The client streams it on its own
/// tight radius; the server, which only wants colliders, never asks for it.
pub fn generate_chunk_prop_spawns(terrain: &TerrainGenerator, chunk: ChunkCoord) -> Vec<PropSpawn> {
    let mut out = Vec::new();
    if !chunk.in_world_bounds() {
        return out;
    }

    // Rivers are cut from the seed at load, but the props in `map.ron` were
    // baked before the channel existed, so nothing in that list knows the
    // ground moved. Without this, trees stand in the water.
    let river_reach = RiverReach::for_chunk(terrain, chunk, RIVER_CLEARANCE);

    if let Some(indexed) = terrain
        .loaded_map()
        .objects_by_chunk
        .get(&(chunk.x, chunk.z))
    {
        out.reserve(indexed.len());

        for object in indexed {
            let x = object.position[0];
            let z = object.position[2];
            if river_reach.contains(Vec2::new(x, z)) {
                continue;
            }
            let ground_y = terrain.get_height(x, z);
            let y = ground_y + object.position[1];
            let render_tuning = object
                .kind
                .map(default_render_tuning)
                .unwrap_or_else(default_unmapped_render_tuning);

            out.push(PropSpawn {
                kind: object.kind,
                scene_path: object.scene_path.clone(),
                chunk,
                position: Vec3::new(x, y, z),
                rotation: object.rotation,
                scale: object.scale,
                render_tuning,
            });
        }
    }

    // Generated maps grow their vegetation from the seed at runtime; the
    // authored list above carries only hand-placed edits for such maps.
    for hit in chunk_scatter_hits(terrain, chunk) {
        if river_reach.contains(Vec2::new(hit.x, hit.z)) {
            continue;
        }
        let y = terrain.get_height(hit.x, hit.z);
        out.push(PropSpawn {
            kind: Some(hit.kind),
            scene_path: hit.kind.scene_path().to_string(),
            chunk,
            position: Vec3::new(hit.x, y, hit.z),
            rotation: Quat::from_rotation_y(hit.rotation),
            scale: hit.scale,
            render_tuning: default_render_tuning(hit.kind),
        });
    }

    out
}

/// Deterministically generate only props that block village roads.
///
/// This intentionally shares the authored chunk index and river-clearance
/// rule with [`generate_chunk_prop_spawns`], but omits height sampling, render
/// tuning and scene-path cloning. A server road survey may warm many chunks
/// during a settlement burst and none of that visual data is used there.
pub fn generate_chunk_blocking_props(
    terrain: &TerrainGenerator,
    chunk: ChunkCoord,
) -> Vec<BlockingPropSpawn> {
    if !chunk.in_world_bounds() {
        return Vec::new();
    }
    let river_reach = RiverReach::for_chunk(terrain, chunk, RIVER_CLEARANCE);
    let mut out = Vec::new();
    if let Some(indexed) = terrain
        .loaded_map()
        .objects_by_chunk
        .get(&(chunk.x, chunk.z))
    {
        out.reserve(indexed.len());
        for object in indexed {
            let Some(kind) = object.kind.filter(|kind| kind.blocks_village_road()) else {
                continue;
            };
            let position = Vec2::new(object.position[0], object.position[2]);
            if river_reach.contains(position) {
                continue;
            }
            out.push(BlockingPropSpawn {
                kind,
                position,
                scale: object.scale,
            });
        }
    }
    // Scattered vegetation blocks roads exactly like authored props: the
    // scatter is deterministic, so server surveys and client visuals agree.
    for hit in chunk_scatter_hits(terrain, chunk) {
        if !hit.kind.blocks_village_road() {
            continue;
        }
        let position = Vec2::new(hit.x, hit.z);
        if river_reach.contains(position) {
            continue;
        }
        out.push(BlockingPropSpawn {
            kind: hit.kind,
            position,
            scale: hit.scale,
        });
    }
    out
}

// ---------------------------------------------------------------------------
// Vegetation scatter (generated maps)
// ---------------------------------------------------------------------------
//
// Trees, bushes, rocks and flowers are scattered at RUNTIME from the seed,
// exactly like ground cover below — the retired map editor used to bake this
// same logic into an 11MB objects list in map.ron (scatter_props, removed
// with the editor). Deterministic per cell: client render spawns and server
// road/collider surveys derive the identical world from the recipe.

/// Metres between scatter sample points, on a GLOBAL grid anchored at
/// -half_extent (not per chunk) so densities and clump shapes are seamless
/// across chunk borders. A cell's jittered point never leaves the cell, so
/// chunks partition cells exactly: each point is generated by precisely one
/// chunk — whichever contains it.
const SCATTER_CELL: f32 = 7.0;
/// Global thinning applied after the biome decides. The retired editor
/// estimated natural yield and thinned to an 880/km² budget; on the shipped
/// world that came out at ~0.8, kept here as a constant so densities match.
const SCATTER_KEEP: f32 = 0.80;

const TREES_BROADLEAF: &[PropKind] = &[
    PropKind::BroadleafNarrowA,
    PropKind::OakA,
    PropKind::BroadleafLargeA,
    PropKind::BroadleafSpreadingA,
    PropKind::BirchA,
    PropKind::BirchB,
    PropKind::ChestnutA,
    PropKind::BroadleafHighCrownA,
    PropKind::BroadleafTallA,
];
const TREES_PINE: &[PropKind] = &[
    PropKind::PineA,
    PropKind::PineB,
    PropKind::PineTallA,
    PropKind::PineTallB,
    PropKind::PineYoungA,
    PropKind::PineYoungB,
];
const TREES_DEAD: &[PropKind] = &[
    PropKind::DeadTreeA,
    PropKind::DeadTreeB,
    PropKind::DeadTreeC,
    PropKind::DeadGnarledA,
];
const SCATTER_BUSHES: &[PropKind] = &[PropKind::BushA, PropKind::BushB, PropKind::BushC];
const SCATTER_ROCKS: &[PropKind] = &[
    PropKind::SmallRockA,
    PropKind::SmallRockB,
    PropKind::SmallRockC,
];
const SCATTER_FLOWERS: &[PropKind] = &[
    PropKind::FlowerA,
    PropKind::FlowerB,
    PropKind::FlowerC,
    PropKind::FlowerD,
];

/// One accepted scatter point, before terrain-height sampling.
struct ScatterHit {
    kind: PropKind,
    x: f32,
    z: f32,
    rotation: f32,
    scale: f32,
}

/// Deterministic vegetation for the cells of one chunk. Empty for
/// hand-authored maps (no recipe): scattering into a map somebody placed by
/// hand would be vandalism, same rule as ground cover.
fn chunk_scatter_hits(terrain: &TerrainGenerator, chunk: ChunkCoord) -> Vec<ScatterHit> {
    use crate::worldgen::{climate_at_with_phase, fbm, rand01, splitmix64, WorldBiome, SEA_LEVEL};

    let map = terrain.loaded_map();
    let (Some(field), Some(generated)) =
        (map.biome_field.as_ref(), map.definition.generated.as_ref())
    else {
        return Vec::new();
    };
    if !generated.scatter_vegetation {
        return Vec::new();
    }
    let seed = generated.seed;
    let half_extent = generated.half_extent;
    let phase = crate::worldgen::climate_phase(seed);

    // Density-variation masks, same three questions the editor asked:
    // within-forest thick/thin stands, rare walk-in forest CLEARINGS, and
    // meadow COPSES (open grass with occasional stands).
    let clump_mask = fbm(splitmix64(seed ^ 77) as u32, 3, 1.0 / 90.0);
    let glade_mask = fbm(splitmix64(seed ^ 0x61A_DE) as u32, 2, 1.0 / 150.0);
    let copse_mask = fbm(splitmix64(seed ^ 0xC0F_5E) as u32, 2, 1.0 / 230.0);

    let pick =
        |pool: &[PropKind], r: f32| pool[((r * pool.len() as f32) as usize).min(pool.len() - 1)];
    let feature = |value: f32, lo: f32, hi: f32| {
        let t = ((value - lo) / (hi - lo)).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    // Species mix: the BIOME sets the base (highlands are conifer country at
    // any latitude), CLIMATE shifts it north/south. Conifers take over just
    // before the ground whitens (frost leads the snowline), dead wood claims
    // the dry fringe — the treeline and the snowline are one fact.
    let tree_pool = |conifer_bias: f32,
                     climate: &crate::worldgen::ClimateSample,
                     rng: &mut u64|
     -> &'static [PropKind] {
        let conifer = (conifer_bias + climate.frost * 0.90).clamp(0.0, 0.98);
        let dead = (climate.dry * 0.70).min(0.72);
        let r = rand01(rng);
        if r < dead {
            TREES_DEAD
        } else if r < dead + conifer * (1.0 - dead) {
            TREES_PINE
        } else {
            TREES_BROADLEAF
        }
    };

    let cell_seed = |xi: i64, zi: i64, salt: u64| {
        let key = ((zi as u32 as u64) << 32) | (xi as u32 as u64);
        splitmix64(splitmix64(key ^ salt) ^ seed)
    };

    let base_x = chunk.x as f32 * CHUNK_SIZE;
    let base_z = chunk.z as f32 * CHUNK_SIZE;
    let min_xi = ((base_x + half_extent) / SCATTER_CELL).floor() as i64;
    let max_xi = ((base_x + CHUNK_SIZE + half_extent) / SCATTER_CELL).ceil() as i64;
    let min_zi = ((base_z + half_extent) / SCATTER_CELL).floor() as i64;
    let max_zi = ((base_z + CHUNK_SIZE + half_extent) / SCATTER_CELL).ceil() as i64;

    let mut out = Vec::new();
    for zi in min_zi..=max_zi {
        for xi in min_xi..=max_xi {
            // The budget thin rolls before anything expensive, off its own
            // salt so it never shifts what an accepted cell contains.
            let mut thin = cell_seed(xi, zi, 0x7A15);
            if rand01(&mut thin) >= SCATTER_KEEP {
                continue;
            }
            let mut rng = cell_seed(xi, zi, 0xF00D);
            let x = -half_extent + (xi as f32 + rand01(&mut rng)) * SCATTER_CELL;
            let z = -half_extent + (zi as f32 + rand01(&mut rng)) * SCATTER_CELL;
            // Cells straddling the chunk border are generated by whichever
            // chunk actually contains the jittered point.
            if x < base_x || x >= base_x + CHUNK_SIZE || z < base_z || z >= base_z + CHUNK_SIZE {
                continue;
            }
            let h = terrain.get_height(x, z);
            if h < SEA_LEVEL + 1.1 {
                continue; // no vegetation in the water or on the wet sand line
            }
            const STEP: f32 = 2.0;
            let dx = terrain.get_height(x + STEP, z) - h;
            let dz = terrain.get_height(x, z + STEP) - h;
            let slope = dx.abs().max(dz.abs()) / STEP;

            let biome = field.biome(x, z, h, slope);
            let climate = climate_at_with_phase(phase, x, z, h, half_extent);
            let clump =
                (clump_mask.get([x as f64, z as f64]) as f32 * 0.5 + 0.5).clamp(0.0, 1.0);
            let glade_raw =
                (glade_mask.get([x as f64, z as f64]) as f32 * 0.5 + 0.5).clamp(0.0, 1.0);
            let copse_raw =
                (copse_mask.get([x as f64, z as f64]) as f32 * 0.5 + 0.5).clamp(0.0, 1.0);
            let glade = feature(glade_raw, 0.60, 0.86);
            let copse = feature(copse_raw, 0.52, 0.88);
            let roll = rand01(&mut rng);

            let (kind, scale) = if slope > 0.85 {
                // Cliffs: occasional rocks only, whatever the biome says.
                if roll < 0.12 {
                    (
                        pick(SCATTER_ROCKS, rand01(&mut rng)),
                        1.4 + rand01(&mut rng) * 1.4,
                    )
                } else {
                    continue;
                }
            } else {
                match biome {
                    WorldBiome::Ocean => continue,
                    WorldBiome::Forest => {
                        // Dense almost everywhere, with real clearings: the
                        // 0.52 floor keeps any stand denser than open
                        // grassland; glades cut hard but are a separate,
                        // rare mask rather than a side effect of low density.
                        let density = (0.52 + clump * 0.36) * (1.0 - glade * 0.88);
                        if h > SEA_LEVEL + 2.0 && roll < density {
                            let alt_bias = ((h - 16.0) / 45.0).clamp(0.0, 0.55);
                            let pool = tree_pool(0.08 + alt_bias, &climate, &mut rng);
                            (pick(pool, rand01(&mut rng)), 0.85 + rand01(&mut rng) * 0.45)
                        } else if roll < density + 0.10 && glade > 0.25 {
                            (
                                pick(SCATTER_BUSHES, rand01(&mut rng)),
                                0.8 + rand01(&mut rng) * 0.5,
                            )
                        } else if roll > 0.97 {
                            (
                                pick(SCATTER_ROCKS, rand01(&mut rng)),
                                0.5 + rand01(&mut rng) * 0.5,
                            )
                        } else {
                            continue;
                        }
                    }
                    WorldBiome::Meadows => {
                        // Open grass, flowers, trees gathered into copses.
                        let density = 0.02 + copse * 0.62;
                        if roll < 0.085 {
                            (
                                pick(SCATTER_FLOWERS, rand01(&mut rng)),
                                0.8 + rand01(&mut rng) * 0.4,
                            )
                        } else if roll < 0.085 + density && h > SEA_LEVEL + 2.0 {
                            let pool = tree_pool(0.03, &climate, &mut rng);
                            (pick(pool, rand01(&mut rng)), 0.9 + rand01(&mut rng) * 0.4)
                        } else if roll > 0.995 {
                            (
                                pick(SCATTER_ROCKS, rand01(&mut rng)),
                                0.5 + rand01(&mut rng) * 0.5,
                            )
                        } else {
                            continue;
                        }
                    }
                    WorldBiome::Highlands => {
                        // Rock fields, sparse pines; iron veins read as tight
                        // clusters of big dark boulders.
                        let vein = field.iron_vein(x, z);
                        if vein > 0.55 && roll < 0.45 {
                            (PropKind::BoulderB, 0.9 + rand01(&mut rng) * 0.6)
                        } else if roll < 0.12 {
                            (
                                pick(SCATTER_ROCKS, rand01(&mut rng)),
                                1.6 + rand01(&mut rng) * 1.6,
                            )
                        } else if roll < 0.165 && h > SEA_LEVEL + 2.0 {
                            let pool = tree_pool(0.80, &climate, &mut rng);
                            (pick(pool, rand01(&mut rng)), 0.75 + rand01(&mut rng) * 0.35)
                        } else {
                            continue;
                        }
                    }
                    WorldBiome::Mountains => {
                        let vein = field.iron_vein(x, z);
                        if vein > 0.55 && roll < 0.50 {
                            (PropKind::BoulderB, 1.0 + rand01(&mut rng) * 0.7)
                        } else if roll < 0.14 {
                            (
                                pick(SCATTER_ROCKS, rand01(&mut rng)),
                                2.0 + rand01(&mut rng) * 1.8,
                            )
                        } else if roll < 0.155 && h < 42.0 {
                            let pool = tree_pool(0.90, &climate, &mut rng);
                            (pick(pool, rand01(&mut rng)), 0.7 + rand01(&mut rng) * 0.3)
                        } else {
                            continue;
                        }
                    }
                    WorldBiome::Snowlands => {
                        // Taiga: thin pine stands over the snow, boulders and
                        // rocks breaking the white. Almost every tree is a
                        // conifer; the rare dead trunk sells the cold.
                        let density = (0.06 + clump * 0.16) * (1.0 - glade * 0.70);
                        if h > SEA_LEVEL + 2.0 && roll < density {
                            let pool = if rand01(&mut rng) < 0.92 {
                                TREES_PINE
                            } else {
                                TREES_DEAD
                            };
                            (pick(pool, rand01(&mut rng)), 0.8 + rand01(&mut rng) * 0.4)
                        } else if roll < density + 0.045 {
                            (
                                pick(SCATTER_ROCKS, rand01(&mut rng)),
                                1.1 + rand01(&mut rng) * 1.2,
                            )
                        } else if roll > 0.988 {
                            (PropKind::BoulderA, 0.9 + rand01(&mut rng) * 0.6)
                        } else {
                            continue;
                        }
                    }
                    WorldBiome::Desert => {
                        // Dunes stay mostly bare; life clings to the flats —
                        // skeleton trees, dry scrub, sun-split rocks.
                        if roll < 0.020 && h > SEA_LEVEL + 2.0 {
                            (
                                pick(TREES_DEAD, rand01(&mut rng)),
                                0.85 + rand01(&mut rng) * 0.4,
                            )
                        } else if roll < 0.048 {
                            (
                                pick(SCATTER_BUSHES, rand01(&mut rng)),
                                0.55 + rand01(&mut rng) * 0.35,
                            )
                        } else if roll < 0.075 {
                            (
                                pick(SCATTER_ROCKS, rand01(&mut rng)),
                                0.9 + rand01(&mut rng) * 1.1,
                            )
                        } else {
                            continue;
                        }
                    }
                }
            };

            out.push(ScatterHit {
                kind,
                x,
                z,
                rotation: rand01(&mut rng) * std::f32::consts::TAU,
                scale,
            });
        }
    }
    out
}

// ---------------------------------------------------------------------------
// Ground cover
// ---------------------------------------------------------------------------

/// Metres between grass sample points. A patch mesh covers ~2 m, so at 2.6 m
/// spacing neighbouring patches overlap and the ground reads as carpet rather
/// than as dots. This is the dial for "more grass".
const GRASS_CELL: f32 = 2.6;

/// Ceiling on grass patches per chunk. A backstop, and it must STAY one.
///
/// At 480 it was binding — 6 chunks of a 25-chunk temperate sample sat pinned
/// exactly on it, so the cap rather than the biome was setting density in the
/// best meadow, and it flat-topped the richest ground at a uniform value.
/// 640 is above the 625 the densest measured chunk wants, so the biome decides
/// again and [`GRASS_DENSITY_SCALE`] is the honest dial.
const GRASS_PER_CHUNK_CAP: usize = 640;

/// Steepest ground grass will grow on.
const GRASS_MAX_SLOPE: f32 = 0.72;

/// Global thinning applied after the biome decides.
///
/// Multiplying the acceptance probability rather than widening [`GRASS_CELL`]
/// keeps the farmland gradient's SHAPE intact — wider spacing would thin the
/// already-sparse north proportionally harder and strip it bare.
///
/// 0.685 rather than a round 0.8 because the per-chunk cap used to bind: the
/// richest meadow generated 324 patches a chunk and was being clipped to 480
/// across a 25-chunk sample, so the number anyone actually SAW was 278. This
/// lands that on ~222, a true 20% off the observed density, and the cap no
/// longer touches it.
const GRASS_DENSITY_SCALE: f32 = 0.685;

/// Two variants, mixed roughly 2:1 -- the low patch is the common ground and
/// the tall tufts break up the repeat, which is what the asset pair was built
/// for.
///
const GRASS_PATCH: PropKind = PropKind::GrassShortA;
const GRASS_TALL: PropKind = PropKind::GrassTallA;

/// Grow the ground cover for a chunk.
///
/// **Grass is generated, not authored, and that is the whole point.** Baked
/// into `map.ron` it was 38,578 entries — more than half the file — to describe
/// something a hash of the position can say for free. Worse, the density was
/// then frozen: wanting more grass meant regenerating and re-committing a 14 MB
/// file. Here it is a constant, and it costs nothing on disk.
///
/// Density comes from [`ResourceProfile::farmland`], which is the same number
/// the economy reads. That is not a shortcut, it is the point: farmland already
/// falls to zero in the frozen north and the desert south and peaks in
/// temperate meadow, so the grass thins exactly where food does. What you see
/// growing IS what the simulation says grows.
///
/// Deterministic from (seed, world position) alone — never from iteration
/// order — so a chunk looks the same however the player approached it.
pub fn generate_chunk_grass(terrain: &TerrainGenerator, chunk: ChunkCoord) -> Vec<PropSpawn> {
    generate_chunk_grass_at_density(terrain, chunk, 1.0)
}

/// Generate ground cover at a diagnostic density multiplier.
///
/// Normal gameplay always calls [`generate_chunk_grass`] and therefore keeps
/// the authored 1x appearance. The client capture/performance harness can use
/// this variant to stress the renderer without committing an artificially
/// dense world or changing any simulation data.
pub fn generate_chunk_grass_at_density(
    terrain: &TerrainGenerator,
    chunk: ChunkCoord,
    multiplier: f32,
) -> Vec<PropSpawn> {
    let mut out = Vec::new();
    let map = terrain.loaded_map();
    // Hand-authored maps carry no recipe, so they get no procedural cover:
    // scattering into a map somebody placed by hand would be vandalism.
    let (Some(field), Some(generated)) =
        (map.biome_field.as_ref(), map.definition.generated.as_ref())
    else {
        return out;
    };
    let seed = generated.seed;
    let water = map.heightmap.water_level.unwrap_or(f32::NEG_INFINITY);

    let multiplier = if multiplier.is_finite() {
        multiplier.clamp(1.0, 32.0)
    } else {
        1.0
    };
    // Halving cell width yields four times as many samples over the same
    // ground area. Density acceptance remains biome-driven, so a 16x stress
    // meadow is still recognisably the same meadow instead of a uniform grid.
    let cell = GRASS_CELL / multiplier.sqrt();
    let per_chunk_cap = (GRASS_PER_CHUNK_CAP as f32 * multiplier).ceil() as usize;
    let base_x = chunk.x as f32 * CHUNK_SIZE;
    let base_z = chunk.z as f32 * CHUNK_SIZE;
    let steps = (CHUNK_SIZE / cell).ceil() as i32;

    let short_tuning = default_render_tuning(GRASS_PATCH);
    let tall_tuning = default_render_tuning(GRASS_TALL);
    let short_path = GRASS_PATCH.scene_path().to_string();
    let tall_path = GRASS_TALL.scene_path().to_string();

    // Grass is cleared only to the waterline, not to the prop clearance: a
    // riverbank with grass running down to the water is the point, and a bald
    // strip either side would look like the road this used to be mistaken for.
    let river_reach = RiverReach::for_chunk(terrain, chunk, GRASS_RIVER_CLEARANCE);

    let mut grown = 0usize;
    for iz in 0..steps {
        for ix in 0..steps {
            if grown >= per_chunk_cap {
                return out;
            }
            // Seeded from the CELL's world identity, not from a running
            // counter, so rejecting a cell never shifts what the next one
            // rolls. Keep procedural scatter deterministic across processes.
            let key = ((chunk.z as u32 as u64) << 40)
                ^ ((chunk.x as u32 as u64) << 16)
                ^ ((iz as u64) << 8)
                ^ ix as u64;
            let mut rng = crate::worldgen::splitmix64(crate::worldgen::splitmix64(key) ^ seed);

            let x = base_x + (ix as f32 + crate::worldgen::rand01(&mut rng)) * cell;
            let z = base_z + (iz as f32 + crate::worldgen::rand01(&mut rng)) * cell;
            let height = terrain.get_height(x, z);
            if height < water + 0.4 {
                continue;
            }
            if river_reach.contains(Vec2::new(x, z)) {
                continue;
            }

            // Slope from the real terrain, deltas included, so grass does not
            // grow up a cliff the generator never knew about.
            const STEP: f32 = 2.0;
            let dx = terrain.get_height(x + STEP, z) - height;
            let dz = terrain.get_height(x, z + STEP) - height;
            let slope = dx.abs().max(dz.abs()) / STEP;
            if slope > GRASS_MAX_SLOPE {
                continue;
            }

            let profile = field.resources(x, z, height, slope);
            // Bare rock and deep forest floor still show some cover, so the
            // ground never reads as a texture with nothing on it.
            // Weighted toward FARMLAND, so grassland is grassy and a forest
            // floor is not. At 1.25/0.45 a meadow and a wood came out at 0.69
            // and 0.55 -- barely separable. At 1.45/0.20 the meadow still
            // saturates and the wood drops to 0.43, which reads as canopy
            // shading the ground out.
            let density =
                (profile.farmland * 1.45 + profile.wood * 0.20).min(1.0) * GRASS_DENSITY_SCALE;
            if crate::worldgen::rand01(&mut rng) > density {
                continue;
            }

            let tall = crate::worldgen::rand01(&mut rng) < 0.34;
            let (kind, path, tuning) = if tall {
                (GRASS_TALL, &tall_path, tall_tuning)
            } else {
                (GRASS_PATCH, &short_path, short_tuning)
            };

            out.push(PropSpawn {
                kind: Some(kind),
                scene_path: path.clone(),
                chunk,
                position: Vec3::new(x, height, z),
                rotation: Quat::from_rotation_y(
                    crate::worldgen::rand01(&mut rng) * std::f32::consts::TAU,
                ),
                // Modest spread only. Grass reads as a carpet, and a patch at
                // 1.6x is a bush.
                scale: 0.85 + crate::worldgen::rand01(&mut rng) * 0.45,
                render_tuning: tuning,
            });
            grown += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::WorldTerrain;

    /// The lightweight blocking view (server road/collider surveys) must
    /// agree exactly with the full render spawn list on which props block —
    /// vegetation is scattered from the seed now, so both derive it live.
    #[test]
    fn indexed_lightweight_blockers_match_full_spawns() {
        let terrain = WorldTerrain::default();
        let mut compared = 0;
        for cz in -4..4 {
            for cx in -4..4 {
                let chunk = ChunkCoord::new(cx * 8, cz * 8);
                let expected: Vec<_> = generate_chunk_prop_spawns(&terrain.generator, chunk)
                    .into_iter()
                    .filter_map(|spawn| {
                        let kind = spawn.kind.filter(|kind| kind.blocks_village_road())?;
                        Some((
                            kind,
                            Vec2::new(spawn.position.x, spawn.position.z),
                            spawn.scale,
                        ))
                    })
                    .collect();
                let actual = generate_chunk_blocking_props(&terrain.generator, chunk);
                assert_eq!(actual.len(), expected.len());
                for (actual, expected) in actual.iter().zip(expected) {
                    assert_eq!(actual.kind, expected.0);
                    assert_eq!(actual.position, expected.1);
                    assert_eq!(actual.scale, expected.2);
                    compared += 1;
                }
            }
        }
        assert!(compared > 0, "test area contained no blocking props");
    }

    /// Somewhere to point the camera for each biome.
    #[test]
    #[ignore = "diagnostic: cargo test -p shared -- --ignored --nocapture find_biome_spots"]
    fn find_biome_spots() {
        let terrain = WorldTerrain::default();
        let map = terrain.generator.loaded_map();
        let field = map.biome_field.as_deref().expect("generated map");
        let mut want = vec!["Forest", "Meadows", "Highlands"];
        let mut x = -3000.0f32;
        while x < 3000.0 && !want.is_empty() {
            let mut z = -1500.0f32;
            while z < 1500.0 && !want.is_empty() {
                let h = terrain.get_height(x, z);
                if h > 3.0 {
                    let n = terrain.get_normal(x, z);
                    let slope = (n.x * n.x + n.z * n.z).sqrt() / n.y.max(0.01);
                    let b = format!("{:?}", field.biome(x, z, h, slope));
                    if let Some(i) = want.iter().position(|w| *w == b) {
                        // Require the neighbourhood to agree, so the camera
                        // lands INSIDE the biome rather than on its edge.
                        let solid = [(-70.0, 0.0), (70.0, 0.0), (0.0, -70.0), (0.0, 70.0)]
                            .iter()
                            .all(|(dx, dz)| {
                                let (sx, sz) = (x + dx, z + dz);
                                let sh = terrain.get_height(sx, sz);
                                let sn = terrain.get_normal(sx, sz);
                                let ss = (sn.x * sn.x + sn.z * sn.z).sqrt() / sn.y.max(0.01);
                                sh > 3.0 && format!("{:?}", field.biome(sx, sz, sh, ss)) == b
                            });
                        if solid {
                            println!("{b:10} --at {x:.0},{z:.0}");
                            want.remove(i);
                        }
                    }
                }
                z += 40.0;
            }
            x += 40.0;
        }
        if !want.is_empty() {
            println!("not found: {want:?}");
        }
    }

    /// Actual prop density per biome, measured from the shipped map.
    ///
    /// "Forest feels emptier than meadows" is either a real defect or an
    /// illusion, and the map file can settle it. Classifies every authored
    /// object by the biome it stands in and reports trees per square kilometre.
    #[test]
    #[ignore = "diagnostic: cargo test -p shared -- --ignored --nocapture density_by_biome"]
    fn density_by_biome() {
        use std::collections::HashMap;
        let terrain = WorldTerrain::default();
        let map = terrain.generator.loaded_map();
        let field = map.biome_field.as_deref().expect("generated map");

        // Area per biome, from a coarse sweep, so counts become densities.
        let mut area: HashMap<String, f64> = HashMap::new();
        const STEP: f32 = 64.0;
        let half = map.definition.generated.as_ref().unwrap().half_extent;
        let mut x = -half;
        while x < half {
            let mut z = -half;
            while z < half {
                let h = terrain.get_height(x, z);
                if h > 1.0 {
                    let n = terrain.get_normal(x, z);
                    let slope = (n.x * n.x + n.z * n.z).sqrt() / n.y.max(0.01);
                    let b = format!("{:?}", field.biome(x, z, h, slope));
                    *area.entry(b).or_default() += (STEP * STEP) as f64;
                }
                z += STEP;
            }
            x += STEP;
        }

        let mut counts: HashMap<(String, String), u32> = HashMap::new();
        for objects in map.objects_by_chunk.values() {
            for object in objects {
                let (x, z) = (object.position[0], object.position[2]);
                let h = terrain.get_height(x, z);
                let n = terrain.get_normal(x, z);
                let slope = (n.x * n.x + n.z * n.z).sqrt() / n.y.max(0.01);
                let b = format!("{:?}", field.biome(x, z, h, slope));
                let id = object.kind.map(|k| k.id()).unwrap_or("?");
                let family = if id.starts_with("pine") {
                    "conifer"
                } else if id.starts_with("dead") {
                    "dead"
                } else if id.starts_with("tree") {
                    "broadleaf"
                } else if id.starts_with("bush") {
                    "bush"
                } else if id.starts_with("rock") {
                    "rock"
                } else {
                    "flower"
                };
                *counts.entry((b, family.to_string())).or_default() += 1;
            }
        }

        let mut biomes: Vec<&String> = area.keys().collect();
        biomes.sort();
        println!("\n{:<11}{:>9}   {:>26}", "BIOME", "km2", "per km2");
        for b in biomes {
            let km2 = area[b] / 1_000_000.0;
            let get = |f: &str| *counts.get(&(b.clone(), f.to_string())).unwrap_or(&0) as f64 / km2;
            let trees = get("broadleaf") + get("conifer");
            println!(
                "{b:<11}{km2:>9.1}   trees {trees:>6.0}  (broadleaf {:>5.0} conifer {:>5.0})  bush {:>4.0}  rock {:>4.0}  flower {:>4.0}",
                get("broadleaf"), get("conifer"), get("bush"), get("rock"), get("flower")
            );
        }
    }

    /// What the F3 overlay will report, at three latitudes.
    ///
    /// The overlay is only worth having if the numbers are right, and "it
    /// printed something" is not evidence of that. This samples the exact same
    /// calls the overlay makes and shows they differ sensibly across the world.
    #[test]
    #[ignore = "diagnostic: cargo test -p shared -- --ignored --nocapture biome_readout"]
    fn biome_readout() {
        let terrain = WorldTerrain::default();
        let map = terrain.generator.loaded_map();
        let field = map.biome_field.as_deref().expect("generated map");
        let generated = map.definition.generated.as_ref().expect("recipe");
        for (label, x, z) in [
            ("far north", -754.0f32, -3000.0f32),
            ("temperate", 1720.0, 0.0),
            ("far south", 427.0, 2900.0),
        ] {
            let height = terrain.get_height(x, z);
            let n = terrain.get_normal(x, z);
            let slope = (n.x * n.x + n.z * n.z).sqrt() / n.y.max(0.01);
            let biome = field.biome(x, z, height, slope);
            let p = field.resources(x, z, height, slope);
            let c =
                crate::worldgen::climate_at(generated.seed, x, z, height, generated.half_extent);
            println!(
                "{label:10} Biome: {biome:?} ground {height:.0}m slope {slope:.2} | \
                 snow {:.2} frost {:.2} dry {:.2} | farm {:.2} wood {:.2} stone {:.2} iron {:.2}",
                c.snow, c.frost, c.dry, p.farmland, p.wood, p.stone, p.iron
            );
        }
    }

    /// Measure real ground-cover density, because the radius is a budget
    /// decision and estimating "about 400 a chunk" is how budgets get blown.
    #[test]
    #[ignore = "diagnostic: cargo test -p shared -- --ignored --nocapture ground_cover_density"]
    fn ground_cover_density() {
        let terrain = WorldTerrain::default();
        for (label, wx, wz) in [
            ("temperate (1720,0)", 1720.0f32, 0.0f32),
            ("north (-754,-3000)", -754.0, -3000.0),
            ("south (427,2900)", 427.0, 2900.0),
        ] {
            let centre = ChunkCoord::from_world_pos(Vec3::new(wx, 0.0, wz));
            let mut total = 0usize;
            let mut chunks = 0usize;
            for dz in -2..=2 {
                for dx in -2..=2 {
                    let c = ChunkCoord {
                        x: centre.x + dx,
                        z: centre.z + dz,
                    };
                    total += generate_chunk_grass(&terrain.generator, c).len();
                    chunks += 1;
                }
            }
            let mut per: Vec<usize> = Vec::new();
            for dz in -2..=2 {
                for dx in -2..=2 {
                    per.push(
                        generate_chunk_grass(
                            &terrain.generator,
                            ChunkCoord {
                                x: centre.x + dx,
                                z: centre.z + dz,
                            },
                        )
                        .len(),
                    );
                }
            }
            per.sort_unstable();
            let capped = per.iter().filter(|n| **n >= GRASS_PER_CHUNK_CAP).count();
            println!(
                "{label:22} {:>5} over {chunks} chunks = {:>4.0}/chunk  min {} med {} max {}  at-cap {}",
                total,
                total as f32 / chunks as f32,
                per[0],
                per[per.len() / 2],
                per[per.len() - 1],
                capped
            );
        }
    }

    #[test]
    fn diagnostic_ground_cover_density_scales_without_changing_the_normal_recipe() {
        let terrain = WorldTerrain::default();
        let chunk = ChunkCoord::from_world_pos(Vec3::new(1720.0, 0.0, 0.0));
        let normal = generate_chunk_grass(&terrain.generator, chunk);
        let explicit_normal = generate_chunk_grass_at_density(&terrain.generator, chunk, 1.0);
        let stressed = generate_chunk_grass_at_density(&terrain.generator, chunk, 4.0);

        assert_eq!(normal.len(), explicit_normal.len());
        assert_eq!(
            normal
                .iter()
                .map(|spawn| spawn.position)
                .collect::<Vec<_>>(),
            explicit_normal
                .iter()
                .map(|spawn| spawn.position)
                .collect::<Vec<_>>()
        );
        assert!(stressed.len() > normal.len() * 2);
    }
}
