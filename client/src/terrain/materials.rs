use bevy::image::ImageLoaderSettings;
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::{AsBindGroup, TextureViewDescriptor, TextureViewDimension};
use bevy::shader::ShaderRef;
use shared::components::WorldTime;
use shared::water::{OCEAN_LOOP_SECONDS, WATER_SURFACE_OFFSET};

use super::chunks::TerrainChunk;
use crate::render::systems::SunLight;

// The material, palette and samplers live in `shared` so every renderer uses one
// binding contract for the same shader. See shared/src/terrain/material.rs.
// `TerrainPalette` is deliberately absent: the client never names the type, it only calls
// `stylized_palette()`. Re-exporting it anyway would be an unused import, and silencing that
// with an allow would hide the next one that means something.
pub use shared::terrain::{
    layer_tiling, repeat_sampler, stylized_palette, weightmap_sampler, TerrainSplatExtension,
    TerrainSplatMaterial, TERRAIN_ALBEDO_ARRAY, TERRAIN_NORMAL_ARRAY,
};

/// The low-resolution world mesh still uses StandardMaterial lighting for
/// land, but its ocean vertices are blended to an unlit water color in the
/// extension fragment. This avoids trying to match an unlit close-water
/// shader by tuning the RGB of normally-lit seabed.
pub type FarTerrainMaterial = ExtendedMaterial<StandardMaterial, FarTerrainExtension>;

#[derive(Asset, TypePath, AsBindGroup, Clone, Debug)]
pub struct FarTerrainExtension {
    /// x: height of the sun direction. The far water mirrors the detailed
    /// water's compact night tint without inheriting terrain lighting.
    /// y/z: detail-hole centre, w: detail-hole half extent.
    #[uniform(100)]
    pub water_params: Vec4,
    /// xyz: direction to the sun (world), w: glint strength (0 at night).
    /// Mirrors the detailed water's `sun_params` so the map-scale glitter
    /// keeps the same day/night response through the LOD crossfade.
    #[uniform(101)]
    pub sun_glint: Vec4,
}

impl MaterialExtension for FarTerrainExtension {
    // The far mesh underlays the detail terrain by only 5cm. With TAA's
    // jittered depth prepass, that near-tie flipped the depth winner
    // per-pixel in world-anchored patches and the far mesh's coarse baked
    // colors (pale snow/coast tints) bled through the detail ground. Kept
    // out of the prepass, the detail terrain's prepass depth always wins.
    fn enable_prepass() -> bool {
        false
    }

    fn vertex_shader() -> ShaderRef {
        "shaders/far_terrain.wgsl".into()
    }

    fn fragment_shader() -> ShaderRef {
        "shaders/far_terrain.wgsl".into()
    }
}

/// Terrain water uniform from a generator's loaded map.
pub fn water_params_for_generator(generator: &shared::terrain::TerrainGenerator) -> Vec4 {
    match generator.loaded_map().heightmap.water_level {
        Some(level) => Vec4::new(level, 1.0, 0.0, WATER_SURFACE_OFFSET),
        None => Vec4::ZERO,
    }
}

/// Shared terrain render assets (textures + far material).
#[derive(Resource)]
pub struct TerrainRenderAssets {
    pub albedo_array: Handle<Image>,
    pub normal_array: Handle<Image>,
    pub layer_tiling: Vec4,
    pub far_mesh_material: Handle<FarTerrainMaterial>,
}

#[derive(Resource)]
pub struct TerrainTextureSources {
    pub albedo_array: Handle<Image>,
    pub normal_array: Handle<Image>,
    pub layer_tiling: Vec4,
    pub far_mesh_material: Handle<FarTerrainMaterial>,
}

/// Create shared terrain material once.
pub(super) fn setup_terrain_render_assets(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut materials: ResMut<Assets<FarTerrainMaterial>>,
) {
    let albedo_array: Handle<Image> = asset_server
        .load_builder()
        .with_settings(|settings: &mut ImageLoaderSettings| {
            settings.is_srgb = true;
            settings.sampler = repeat_sampler();
        })
        .load(TERRAIN_ALBEDO_ARRAY);
    let normal_array: Handle<Image> = asset_server
        .load_builder()
        .with_settings(|settings: &mut ImageLoaderSettings| {
            settings.is_srgb = false;
            settings.sampler = repeat_sampler();
        })
        .load(TERRAIN_NORMAL_ARRAY);

    let far_mesh_material = materials.add(FarTerrainMaterial {
        base: StandardMaterial {
            // Far mesh uses vertex colors for biome tinting; keep base color white.
            base_color: Color::WHITE,
            perceptual_roughness: 0.98,
            metallic: 0.0,
            reflectance: 0.08,
            // Land overlap is resolved selectively in far_terrain.wgsl.
            // Material-wide bias would also push water behind its seabed.
            depth_bias: 0.0,
            ..default()
        },
        extension: FarTerrainExtension {
            water_params: Vec4::new(0.75, 0.0, 0.0, 0.0),
            // Matches the toon-water default until the first sun sync runs.
            sun_glint: Vec4::new(0.35, 0.75, 0.30, 1.1),
        },
    });

    let layer_tiling = layer_tiling();

    commands.insert_resource(TerrainTextureSources {
        albedo_array,
        normal_array,
        layer_tiling,
        far_mesh_material,
    });

    info!("Queued terrain KTX2 arrays (albedo + normal)");
}

/// Keep the far ocean's dawn/night response aligned with the detailed water.
/// Land stays on StandardMaterial lighting inside the same shader.
pub(super) fn sync_far_terrain_water_sun(
    render_assets: Option<Res<TerrainRenderAssets>>,
    sun: Query<&GlobalTransform, With<SunLight>>,
    mut materials: ResMut<Assets<FarTerrainMaterial>>,
) {
    let Some(render_assets) = render_assets else {
        return;
    };
    let Ok(sun_tf) = sun.single() else {
        return;
    };
    let to_sun = Vec3::from(sun_tf.back());
    // Identical strength curve to `update_water_sun_dir`, so the far glitter
    // and the detailed glint brighten and die together across the day.
    let strength = 1.1 * to_sun.y.clamp(0.0, 1.0).sqrt();
    let glint = Vec4::new(to_sun.x, to_sun.y, to_sun.z, strength);
    let Some(material) = materials.get(&render_assets.far_mesh_material) else {
        return;
    };
    if (material.extension.water_params.x - to_sun.y).abs() < 0.002
        && material.extension.sun_glint.distance_squared(glint) < 1e-4
    {
        return;
    }
    if let Some(mut material) = materials.get_mut(&render_assets.far_mesh_material) {
        material.extension.water_params.x = to_sun.y;
        material.extension.sun_glint = glint;
    }
}

pub(super) fn build_terrain_texture_arrays(
    mut commands: Commands,
    mut images: ResMut<Assets<Image>>,
    sources: Option<Res<TerrainTextureSources>>,
    render_assets: Option<Res<TerrainRenderAssets>>,
) {
    if render_assets.is_some() {
        return;
    }
    let Some(sources) = sources else { return };

    {
        let Some(mut albedo) = images.get_mut(&sources.albedo_array) else {
            return;
        };
        albedo.sampler = repeat_sampler();
        albedo.texture_view_descriptor = Some(TextureViewDescriptor {
            label: Some("terrain_albedo_array"),
            dimension: Some(TextureViewDimension::D2Array),
            ..default()
        });
    }
    {
        let Some(mut normal) = images.get_mut(&sources.normal_array) else {
            return;
        };
        normal.sampler = repeat_sampler();
        normal.texture_view_descriptor = Some(TextureViewDescriptor {
            label: Some("terrain_normal_array"),
            dimension: Some(TextureViewDimension::D2Array),
            ..default()
        });
    }

    commands.insert_resource(TerrainRenderAssets {
        albedo_array: sources.albedo_array.clone(),
        normal_array: sources.normal_array.clone(),
        layer_tiling: sources.layer_tiling,
        far_mesh_material: sources.far_mesh_material.clone(),
    });
    commands.remove_resource::<TerrainTextureSources>();

    info!("Loaded terrain KTX2 arrays (albedo + normal).");
}

#[derive(Default)]
pub(super) struct TerrainWaterClockSync {
    world_time_entity: Option<Entity>,
    offset: f32,
}

/// Keep the animated wet shoreline on the same clock as the water shader.
/// Existing materials are updated once when the replicated clock arrives;
/// newly streamed chunks inherit the cached offset as they are added.
pub(super) fn sync_terrain_water_clock(
    time: Res<Time>,
    world_time: Query<(Entity, &WorldTime)>,
    chunks: Query<(&TerrainChunk, Ref<TerrainChunk>)>,
    mut materials: ResMut<Assets<TerrainSplatMaterial>>,
    mut sync: Local<TerrainWaterClockSync>,
) {
    let Ok((world_time_entity, world_time)) = world_time.single() else {
        return;
    };

    let clock_changed = sync.world_time_entity != Some(world_time_entity);
    if clock_changed {
        let local = time.elapsed_secs_wrapped().rem_euclid(OCEAN_LOOP_SECONDS);
        let half_loop = OCEAN_LOOP_SECONDS * 0.5;
        sync.offset = (world_time.ocean_seconds - local + half_loop).rem_euclid(OCEAN_LOOP_SECONDS)
            - half_loop;
        sync.world_time_entity = Some(world_time_entity);
    }

    for (chunk, change) in &chunks {
        if !clock_changed && !change.is_added() {
            continue;
        }
        let Some(mut material) = materials.get_mut(&chunk.material) else {
            continue;
        };
        material.extension.water_params.z = sync.offset;
    }
}
