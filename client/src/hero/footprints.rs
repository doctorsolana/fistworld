//! Footprints in snow and desert sand.
//!
//! Every walking character — the player's hero and every villager share one
//! rig and one movement pipeline — leaves a trail of pooled decal stamps on
//! soft ground. Client-only cosmetics: positions come from the replicated,
//! smoothed visual transforms, and "is this snow" is the same pure
//! `climate_at` the terrain shader paints with, so prints appear exactly
//! where the ground looks white (or dune-sand dry).
//!
//! The design follows the two pooled-decal precedents in this codebase:
//! the selection ring (material recipe: unlit + fog off + blend, terrain
//! lift) and the chimney smoke (staged fade MATERIALS swapped by handle, so
//! aging never mutates a live GPU material). A fixed ring buffer caps the
//! worst case: hundreds of walkers can never spawn more than [`POOL_CAP`]
//! quads, and stamping is gated to walkers near the camera focus at close
//! zooms, matching how footstep audio culls to the nearest few.

use bevy::prelude::*;
use std::collections::HashMap;

use crate::camera_rts::CommanderCamera;
use crate::hero::HeroVisual;
use shared::components::{AboardBoat, CharacterActivity};
use shared::terrain::WorldTerrain;

/// Metres of travel between stamps: half a stride at the 3.52 m/s run, so
/// alternating feet land naturally.
const STRIDE: f32 = 0.85;
/// Sideways offset of each print from the path centreline.
const FOOT_OFFSET: f32 = 0.09;
/// Seconds a print survives; the last stages fade it out.
const LIFETIME: f32 = 45.0;
/// Discrete fade stages (material handle swaps, never material mutation).
const STAGES: usize = 6;
/// Hard cap on live prints; the ring buffer recycles the oldest.
const POOL_CAP: usize = 128;
/// Only walkers this close to the camera focus stamp (the audio system's
/// nearest-N idea, by radius).
const TRACK_RADIUS: f32 = 64.0;
/// Beyond this zoom a 30cm print is sub-pixel; stop stamping entirely.
const MAX_STAMP_ZOOM: f32 = 320.0;
/// Speed hysteresis matching the walk animation's "is stepping" decision.
const MIN_STEP_SPEED: f32 = 0.24;

#[derive(Resource)]
pub struct FootprintAssets {
    mesh: Handle<Mesh>,
    /// Fade stages for snow (cool compressed-snow shadow)…
    snow: [Handle<StandardMaterial>; STAGES],
    /// …and for dry sand (sun-shadowed warm brown).
    sand: [Handle<StandardMaterial>; STAGES],
}

#[derive(Component)]
pub struct Footprint {
    born: f32,
    sand: bool,
    stage: usize,
}

#[derive(Resource, Default)]
pub struct FootprintPool {
    entities: Vec<Entity>,
    next: usize,
}

struct StrideTracker {
    last: Vec3,
    accum: f32,
    left: bool,
}

#[derive(Resource, Default)]
pub struct StrideTrackers(HashMap<Entity, StrideTracker>);

/// One shared quad + two small stacks of stage materials, built once.
pub fn setup_footprint_assets(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    // Rounded oval print. The mesh is authored in XY; stamps rotate it flat.
    let mesh = meshes.add(Ellipse::new(FOOT_OFFSET, 0.17));
    // A print is a DEPRESSION: it darkens and cools the lit ground beneath
    // it (trampled snow holds shadow — the same verdict every snow-trail
    // reference lands on: darken the albedo, never brighten). Multiply
    // blending inherits the scene lighting for free, so prints dim with the
    // ground at night instead of glowing like unlit paint.
    let mut stage_material = |multiplier: Color| {
        materials.add(StandardMaterial {
            base_color: multiplier,
            unlit: true,
            // fog off, same contract as the selection ring: the multiplier
            // must not desaturate toward sky blue at distance.
            fog_enabled: false,
            alpha_mode: AlphaMode::Multiply,
            double_sided: true,
            cull_mode: None,
            depth_bias: 4.0,
            ..default()
        })
    };
    // The fade eases the multiplier back to identity (white = no effect).
    // Strength holds for most of the life, then releases — a linear
    // fade-from-birth reads as ghosting (footprints research verdict).
    const FADE: [f32; STAGES] = [0.0, 0.08, 0.18, 0.38, 0.65, 0.88];
    let lerp_white = |base: Srgba, t: f32| {
        Color::srgb(
            base.red + (1.0 - base.red) * t,
            base.green + (1.0 - base.green) * t,
            base.blue + (1.0 - base.blue) * t,
        )
    };
    // Snow depressions read cool blue-grey; sand depressions warm brown.
    let snow_tone = Srgba::new(0.68, 0.74, 0.90, 1.0);
    let sand_tone = Srgba::new(0.70, 0.60, 0.48, 1.0);
    let snow = std::array::from_fn(|i| stage_material(lerp_white(snow_tone, FADE[i])));
    let sand = std::array::from_fn(|i| stage_material(lerp_white(sand_tone, FADE[i])));
    commands.insert_resource(FootprintAssets { mesh, snow, sand });
}

/// Track every visible walker's travelled distance and stamp prints on soft
/// ground each stride.
#[allow(clippy::too_many_arguments)]
pub fn stamp_footprints(
    time: Res<Time>,
    terrain: Option<Res<WorldTerrain>>,
    assets: Option<Res<FootprintAssets>>,
    cameras: Query<&CommanderCamera>,
    walkers: Query<(
        Entity,
        &GlobalTransform,
        &HeroVisual,
        Option<&CharacterActivity>,
        Has<AboardBoat>,
        Has<shared::components::Mounted>,
    )>,
    mut trackers: ResMut<StrideTrackers>,
    mut pool: ResMut<FootprintPool>,
    mut prints: Query<(
        &mut Transform,
        &mut Visibility,
        &mut Footprint,
        &mut MeshMaterial3d<StandardMaterial>,
    )>,
    mut commands: Commands,
) {
    let (Some(terrain), Some(assets)) = (terrain.as_deref(), assets.as_deref()) else {
        return;
    };
    let Ok(camera) = cameras.single() else {
        return;
    };
    if camera.zoom > MAX_STAMP_ZOOM {
        trackers.0.clear();
        return;
    }
    // Hoist the climate parameters once (the per-sample fn is a handful of
    // sin/smoothstep ops, mirrored from the terrain shader).
    let map = terrain.generator.loaded_map();
    let Some(generated) = map.definition.generated.as_ref() else {
        return; // hand-authored maps have no climate bands
    };
    let phase = shared::worldgen::climate_phase(generated.seed);
    let half_extent = generated.half_extent;
    let water = map.heightmap.water_level.unwrap_or(f32::NEG_INFINITY);
    let now = time.elapsed_secs();
    let focus = camera.focus;

    let mut seen: Vec<Entity> = Vec::new();
    for (entity, transform, visual, activity, aboard, mounted) in walkers.iter() {
        let pos = transform.translation();
        if shared::character::locomotion::swimming_at(
            terrain.get_height(pos.x, pos.z),
            terrain.get_water_height(pos.x, pos.z),
            pos.y,
            aboard,
        ) || aboard
            || mounted
            || matches!(
                activity,
                Some(
                    CharacterActivity::Sitting
                        | CharacterActivity::LyingDown
                        | CharacterActivity::Indoors
                )
            )
            || Vec2::new(pos.x - focus.x, pos.z - focus.z).length_squared()
                > TRACK_RADIUS * TRACK_RADIUS
        {
            trackers.0.remove(&entity);
            continue;
        }
        seen.push(entity);
        let tracker = trackers.0.entry(entity).or_insert(StrideTracker {
            last: pos,
            accum: 0.0,
            left: false,
        });
        let step = Vec2::new(pos.x - tracker.last.x, pos.z - tracker.last.z);
        let moved = step.length();
        tracker.last = pos;
        // Match the walk animation's hysteresis so idle shuffles don't stamp,
        // and ignore teleports (respawn/interest snaps).
        if visual.speed() < MIN_STEP_SPEED || moved > 5.0 {
            tracker.accum = 0.0;
            continue;
        }
        tracker.accum += moved;
        if tracker.accum < STRIDE {
            continue;
        }
        tracker.accum -= STRIDE;
        tracker.left = !tracker.left;

        // Soft ground only: the same snow the terrain paints (slope-shed
        // included), or dry dune sand. Skip anything at or under water.
        let h = terrain.get_height(pos.x, pos.z);
        if h < water + 0.3 {
            continue;
        }
        const SLOPE_STEP: f32 = 2.0;
        let dx = terrain.get_height(pos.x + SLOPE_STEP, pos.z) - h;
        let dz = terrain.get_height(pos.x, pos.z + SLOPE_STEP) - h;
        let slope = dx.abs().max(dz.abs()) / SLOPE_STEP;
        let climate = shared::worldgen::climate_at_with_phase(phase, pos.x, pos.z, h, half_extent);
        let snow_keep = 1.0 - ((slope - 0.35) / 0.30).clamp(0.0, 1.0);
        let snow_vis = climate.snow * snow_keep;
        let sand = climate.dry > 0.55;
        if snow_vis < 0.45 && !sand {
            continue;
        }

        let dir = step / moved;
        let side = Vec2::new(-dir.y, dir.x)
            * if tracker.left {
                FOOT_OFFSET
            } else {
                -FOOT_OFFSET
            };
        let x = pos.x + side.x;
        let z = pos.z + side.y;
        let ground = terrain.get_height(x, z);
        let translation = Vec3::new(x, ground + 0.035, z);
        let rotation = Quat::from_rotation_y(dir.x.atan2(dir.y))
            * Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2);
        let material = if sand {
            assets.sand[0].clone()
        } else {
            assets.snow[0].clone()
        };

        if pool.entities.len() < POOL_CAP {
            let id = commands
                .spawn((
                    Footprint {
                        born: now,
                        sand,
                        stage: 0,
                    },
                    Mesh3d(assets.mesh.clone()),
                    MeshMaterial3d(material),
                    Transform {
                        translation,
                        rotation,
                        ..default()
                    },
                    bevy::light::NotShadowCaster,
                    bevy::light::NotShadowReceiver,
                ))
                .id();
            pool.entities.push(id);
        } else {
            // Recycle the oldest slot in ring order.
            let slot = pool.entities[pool.next];
            pool.next = (pool.next + 1) % POOL_CAP;
            if let Ok((mut t, mut vis, mut print, mut mat)) = prints.get_mut(slot) {
                t.translation = translation;
                t.rotation = rotation;
                *vis = Visibility::Inherited;
                print.born = now;
                print.sand = sand;
                print.stage = 0;
                mat.0 = material;
            }
        }
    }
    trackers.0.retain(|entity, _| seen.contains(entity));
}

/// Age prints through the staged fade materials, then hide them.
pub fn fade_footprints(
    time: Res<Time>,
    assets: Option<Res<FootprintAssets>>,
    mut prints: Query<(
        &mut Footprint,
        &mut Visibility,
        &mut MeshMaterial3d<StandardMaterial>,
    )>,
) {
    let Some(assets) = assets.as_deref() else {
        return;
    };
    let now = time.elapsed_secs();
    for (mut print, mut visibility, mut material) in prints.iter_mut() {
        if *visibility == Visibility::Hidden {
            continue;
        }
        let age = now - print.born;
        if age >= LIFETIME {
            *visibility = Visibility::Hidden;
            continue;
        }
        let stage = ((age / LIFETIME) * STAGES as f32) as usize;
        let stage = stage.min(STAGES - 1);
        if stage != print.stage {
            print.stage = stage;
            material.0 = if print.sand {
                assets.sand[stage].clone()
            } else {
                assets.snow[stage].clone()
            };
        }
    }
}
