use bevy::prelude::*;

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

    if let Some(indexed) = terrain
        .loaded_map()
        .objects_by_chunk
        .get(&(chunk.x, chunk.z))
    {
        out.reserve(indexed.len());

        for object in indexed {
            let x = object.position[0];
            let z = object.position[2];
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
/// Named for the kinds they hold rather than for their height. Both ARE
/// patches; the difference is how tall the blades stand, and the kind names
/// are the ones that appear in `map.ron` ids and asset filenames.
const GRASS_PATCH: PropKind = PropKind::Grass_Patch;
const GRASS_TALL: PropKind = PropKind::Grass_Tall;

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
    let mut out = Vec::new();
    let map = terrain.loaded_map();
    // Hand-authored maps carry no recipe, so they get no procedural cover:
    // scattering into a map somebody placed by hand would be vandalism.
    let (Some(field), Some(generated)) = (map.biome_field.as_ref(), map.definition.generated.as_ref())
    else {
        return out;
    };
    let seed = generated.seed;
    let water = map.heightmap.water_level.unwrap_or(f32::NEG_INFINITY);

    let base_x = chunk.x as f32 * CHUNK_SIZE;
    let base_z = chunk.z as f32 * CHUNK_SIZE;
    let steps = (CHUNK_SIZE / GRASS_CELL).ceil() as i32;

    let short_tuning = default_render_tuning(GRASS_PATCH);
    let tall_tuning = default_render_tuning(GRASS_TALL);
    let short_path = GRASS_PATCH.scene_path().to_string();
    let tall_path = GRASS_TALL.scene_path().to_string();

    let mut grown = 0usize;
    for iz in 0..steps {
        for ix in 0..steps {
            if grown >= GRASS_PER_CHUNK_CAP {
                return out;
            }
            // Seeded from the CELL's world identity, not from a running
            // counter, so rejecting a cell never shifts what the next one
            // rolls. Same discipline as the editor's scatter.
            let key = ((chunk.z as u32 as u64) << 40)
                ^ ((chunk.x as u32 as u64) << 16)
                ^ ((iz as u64) << 8)
                ^ ix as u64;
            let mut rng = crate::worldgen::splitmix64(crate::worldgen::splitmix64(key) ^ seed);

            let x = base_x + (ix as f32 + crate::worldgen::rand01(&mut rng)) * GRASS_CELL;
            let z = base_z + (iz as f32 + crate::worldgen::rand01(&mut rng)) * GRASS_CELL;
            let height = terrain.get_height(x, z);
            if height < water + 0.4 {
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
            let density =
                (profile.farmland * 1.25 + profile.wood * 0.45).min(1.0) * GRASS_DENSITY_SCALE;
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
            let c = crate::worldgen::climate_at(
                generated.seed,
                x,
                z,
                height,
                generated.half_extent,
            );
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
                            ChunkCoord { x: centre.x + dx, z: centre.z + dz },
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
}
