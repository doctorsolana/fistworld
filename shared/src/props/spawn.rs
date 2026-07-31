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

/// Ceiling on grass patches per chunk. A backstop, not a design value: at 2.6 m
/// spacing a 64 m chunk offers ~600 sample points, and this caps the richest
/// meadow so one freak chunk cannot stall the spawn queue.
const GRASS_PER_CHUNK_CAP: usize = 480;

/// Steepest ground grass will grow on.
const GRASS_MAX_SLOPE: f32 = 0.72;

/// Two variants, mixed. The short patch is the common one and the tall one
/// breaks up the repeat; roughly 2:1, which is what the asset pair was built
/// for.
const GRASS_SHORT: PropKind = PropKind::Env_Grass_06;
const GRASS_TALL: PropKind = PropKind::Env_Grass_Tall_04;

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

    let short_tuning = default_render_tuning(GRASS_SHORT);
    let tall_tuning = default_render_tuning(GRASS_TALL);
    let short_path = GRASS_SHORT.scene_path().to_string();
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
            let density = (profile.farmland * 1.25 + profile.wood * 0.45).min(1.0);
            if crate::worldgen::rand01(&mut rng) > density {
                continue;
            }

            let tall = crate::worldgen::rand01(&mut rng) < 0.34;
            let (kind, path, tuning) = if tall {
                (GRASS_TALL, &tall_path, tall_tuning)
            } else {
                (GRASS_SHORT, &short_path, short_tuning)
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
            println!(
                "{label:22} {:>5} patches over {chunks} chunks = {:>4.0}/chunk",
                total,
                total as f32 / chunks as f32
            );
        }
    }
}
