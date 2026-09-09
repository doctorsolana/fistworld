//! Deterministic, bounded meadow-herd placement. Never tied to camera movement.
use super::*;
use crate::collision::library::DerivedColliderLibrary;
use bevy::prelude::*;
use shared::{components::*, terrain::WorldTerrain};
use shared::{
    props::generate_chunk_blocking_props,
    terrain::ChunkCoord,
    worldgen::{splitmix64, WorldBiome},
};

const CELL_SIZE: f32 = 256.0;
const MAX_HERDS: usize = 24;
const HERD_SIZE: usize = 4;
#[derive(Resource)]
struct Populated;

pub(super) fn habitat(terrain: &WorldTerrain, point: Vec3) -> bool {
    let p = point.xz();
    let height = terrain.get_height(p.x, p.y);
    let normal = terrain.generator.get_normal(p.x, p.y);
    terrain.generator.active_map_bounds().contains_xz(p.x, p.y)
        && terrain
            .get_water_height(p.x, p.y)
            .is_none_or(|water| height > water + 0.4)
        && normal.y > 0.96
        && terrain.generator.get_surface_weights(p.x, p.y)[0] > 0.55
        && terrain
            .generator
            .loaded_map()
            .biome_field
            .as_ref()
            .is_none_or(|field| {
                field.biome(p.x, p.y, height, 1.0 - normal.y) == WorldBiome::Meadows
            })
}

// Spawn validation also checks unstreamed vegetation. This is bootstrap work,
// using the same deterministic blockers and baked radii as navigation surveys.
fn vegetation_clear(world: &World, point: Vec3) -> bool {
    let terrain = world.resource::<WorldTerrain>();
    let library = world.get_resource::<DerivedColliderLibrary>();
    let centre = ChunkCoord::from_world_pos(point);
    for z in -1..=1 {
        for x in -1..=1 {
            let chunk = ChunkCoord::new(centre.x + x, centre.z + z);
            for prop in generate_chunk_blocking_props(&terrain.generator, chunk) {
                let radius = library
                    .and_then(|l| l.by_kind.get(&prop.kind))
                    .map_or(1.5, |c| c.horizontal_radius)
                    * prop.scale;
                if prop.position.distance_squared(point.xz())
                    < (radius + HORSE_CLEARANCE + 0.3).powi(2)
                {
                    return false;
                }
            }
        }
    }
    true
}

pub(super) fn candidates(bounds: shared::map::MapBounds, seed: u64) -> Vec<Vec3> {
    let mut cells = Vec::new();
    let columns = (bounds.width() / CELL_SIZE).ceil().max(1.0) as u32;
    let rows = (bounds.depth() / CELL_SIZE).ceil().max(1.0) as u32;
    // Map bounds already carry the world's finite extent. Hash order prevents
    // the population cap from filling only the south-west corner.
    for z in 0..rows {
        for x in 0..columns {
            let key = splitmix64(seed ^ (u64::from(x) << 32) ^ u64::from(z) ^ 0x484F525345);
            cells.push((key, x, z));
        }
    }
    cells.sort_unstable();
    let mut out = Vec::with_capacity(cells.len() * 8);
    for (key, x, z) in cells {
        for trial in 0..8 {
            let h = splitmix64(key ^ trial);
            let fx = (h as u32) as f32 / u32::MAX as f32;
            let fz = (h >> 32) as f32 / u32::MAX as f32;
            out.push(Vec3::new(
                bounds.min[0] + (x as f32 + 0.1 + 0.8 * fx) * CELL_SIZE,
                0.,
                bounds.min[1] + (z as f32 + 0.1 + 0.8 * fz) * CELL_SIZE,
            ));
        }
    }
    out
}

pub fn populate(world: &mut World) {
    if world.contains_resource::<Populated>() {
        return;
    }
    let Some(terrain) = world.get_resource::<WorldTerrain>() else {
        return;
    };
    let bounds = terrain.generator.active_map_bounds();
    let seed = terrain
        .generator
        .loaded_map()
        .definition
        .generated
        .as_ref()
        .map_or(terrain.generator.active_map_content_hash(), |g| g.seed);
    let points = candidates(bounds, seed);
    let mut herds = 0;
    let mut horses = 0;
    let mut homes = Vec::<Vec3>::new();
    for trials in points.chunks(8) {
        if herds >= MAX_HERDS {
            break;
        }
        for &point in trials {
            if homes
                .iter()
                .any(|p| p.xz().distance_squared(point.xz()) < 90.0_f32.powi(2))
            {
                continue;
            }
            if !habitat(world.resource::<WorldTerrain>(), point) {
                continue;
            }
            let mut herd = 0;
            for member in 0..HERD_SIZE {
                let angle = member as f32 * std::f32::consts::TAU / HERD_SIZE as f32;
                let p = point + Vec3::new(angle.cos(), 0., angle.sin()) * 5.0;
                if habitat(world.resource::<WorldTerrain>(), p) && vegetation_clear(world, p) {
                    if let Ok(entity) = spawn_checked(world, p) {
                        // One herd shares a compact home patch but keeps individual
                        // identity, orientation and staggered behavior clocks.
                        world.get_mut::<PlayerRotation>(entity).unwrap().0 = angle;
                        herd += 1;
                    }
                }
            }
            if herd > 0 {
                homes.push(point);
                herds += 1;
                horses += herd;
                break;
            }
        }
    }
    world.insert_resource(Populated);
    info!("Wildlife: {horses} persistent horses in {herds} meadow herds; at most {MAX_ACTIVE_HORSES} wander near observers");
}
