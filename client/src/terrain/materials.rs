use bevy::asset::Asset;
use bevy::image::{
    ImageAddressMode, ImageFilterMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor,
};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::ShaderType;
use bevy::reflect::TypePath;
use bevy::render::render_resource::{AsBindGroup, TextureViewDescriptor, TextureViewDimension};
use bevy::shader::ShaderRef;
use shared::components::WorldTime;
use shared::water::{OCEAN_LOOP_SECONDS, WATER_SURFACE_OFFSET};

use super::chunks::TerrainChunk;

/// Splatmap material definition (StandardMaterial + extension).
pub type TerrainSplatMaterial = ExtendedMaterial<StandardMaterial, TerrainSplatExtension>;

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct TerrainSplatExtension {
    // Weight map (RGBA): grass, dirt, sand, cobblestone.
    #[texture(100)]
    #[sampler(101)]
    pub weight_map: Handle<Image>,

    // Array textures: layer 0 = grass, 1 = dirt, 2 = sand, 3 = cobblestone.
    #[texture(102, dimension = "2d_array")]
    #[sampler(103)]
    pub albedo_array: Handle<Image>,
    #[texture(104, dimension = "2d_array")]
    #[sampler(105)]
    pub normal_array: Handle<Image>,

    // UV tiling per layer.
    #[uniform(120)]
    pub layer_tiling: Vec4,

    // Debug mode selector.
    #[uniform(121)]
    pub debug_mode: u32,

    // 1.0 = full normal mapping, 0.0 = skip normal-map contribution.
    #[uniform(122)]
    pub normal_strength: f32,

    // x: water level, y: enabled, z: server clock offset, w: surface offset.
    #[uniform(123)]
    pub water_params: Vec4,

    // --- Stylised palette ---
    //
    // The photographic splat textures read as "realistic dirt" no matter how the frame is
    // graded, which fights the low-poly look. These flat per-layer colours replace them.
    // `stylize.x` blends between the two (0 = photo textures, 1 = flat colour) so the
    // change stays A/B-able instead of being a one-way rewrite.
    //
    // Packed into ONE binding on purpose: seven separate `#[uniform]` attributes each
    // allocate their own buffer, which overran the Metal vertex-stage buffer limit
    // ("pipeline needs too many buffers in the vertex stage: 1 vertex and 17 layout").
    #[uniform(124)]
    pub palette: TerrainPalette,
}

/// Flat palette for the stylised terrain.
///
/// Slightly desaturated, slightly blue-shifted in shadow-facing values so the world reads
/// storybook rather than photographic. Keep these as the single source of truth — the
/// far-terrain material below should match, or distant hills change colour at the LOD seam.
pub fn stylized_palette() -> TerrainPalette {
    TerrainPalette {
        // Deep enough to survive the sun's exposure and the aerial haze. A first pass used
        // mid-value colours and the whole world came out pale and bland — the light and fog
        // both wash these out, so the authored values need to sit darker/richer than the
        // intended on-screen result.
        grass: Vec4::new(0.26, 0.45, 0.20, 1.0),
        dirt: Vec4::new(0.42, 0.32, 0.22, 1.0),
        sand: Vec4::new(0.78, 0.70, 0.50, 1.0),
        cobble: Vec4::new(0.40, 0.39, 0.38, 1.0),
        rock: Vec4::new(0.36, 0.35, 0.38, 1.0),
        // 1.0 stylised, 5 bands, gentle banding, strong slope rock.
        stylize: Vec4::new(1.0, 5.0, 0.10, 0.85),
        // Bands span 0..90m of height, with a little texture break-up so large flat areas
        // are not perfectly uniform (which reads as untextured rather than stylised).
        bands: Vec4::new(0.0, 90.0, 0.18, 0.0),
    }
}

#[derive(Clone, Copy, Debug, ShaderType)]
pub struct TerrainPalette {
    pub grass: Vec4,
    pub dirt: Vec4,
    pub sand: Vec4,
    pub cobble: Vec4,
    pub rock: Vec4,
    pub stylize: Vec4,
    pub bands: Vec4,
}

/// Terrain water uniform from a generator's loaded map.
pub fn water_params_for_generator(generator: &shared::terrain::TerrainGenerator) -> Vec4 {
    match generator.loaded_map().heightmap.water_level {
        Some(level) => Vec4::new(level, 1.0, 0.0, WATER_SURFACE_OFFSET),
        None => Vec4::ZERO,
    }
}

impl MaterialExtension for TerrainSplatExtension {
    fn fragment_shader() -> ShaderRef {
        "shaders/terrain_splat.wgsl".into()
    }

    fn deferred_fragment_shader() -> ShaderRef {
        "shaders/terrain_splat.wgsl".into()
    }
}

/// Shared terrain render assets (textures + far material).
#[derive(Resource)]
pub struct TerrainRenderAssets {
    pub albedo_array: Handle<Image>,
    pub normal_array: Handle<Image>,
    pub layer_tiling: Vec4,
    pub far_mesh_material: Handle<StandardMaterial>,
}

#[derive(Resource)]
pub struct TerrainTextureSources {
    pub albedo_array: Handle<Image>,
    pub normal_array: Handle<Image>,
    pub layer_tiling: Vec4,
    pub far_mesh_material: Handle<StandardMaterial>,
}

/// Create shared terrain material once.
pub(super) fn setup_terrain_render_assets(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let albedo_array: Handle<Image> = asset_server.load_with_settings(
        "textures/terrain/optimized_1k/terrain_albedo_array.ktx2",
        |settings: &mut ImageLoaderSettings| {
            settings.is_srgb = true;
            settings.sampler = repeat_sampler();
        },
    );
    let normal_array: Handle<Image> = asset_server.load_with_settings(
        "textures/terrain/optimized_1k/terrain_normal_array.ktx2",
        |settings: &mut ImageLoaderSettings| {
            settings.is_srgb = false;
            settings.sampler = repeat_sampler();
        },
    );

    let far_mesh_material = materials.add(StandardMaterial {
        // Far mesh uses vertex colors for biome tinting; keep base color white.
        base_color: Color::WHITE,
        perceptual_roughness: 0.98,
        metallic: 0.0,
        reflectance: 0.08,
        ..default()
    });

    let layer_tiling = Vec4::new(8.0, 7.0, 6.0, 5.0);

    commands.insert_resource(TerrainTextureSources {
        albedo_array,
        normal_array,
        layer_tiling,
        far_mesh_material,
    });

    info!("Queued terrain KTX2 arrays (albedo + normal)");
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
        let Some(albedo) = images.get_mut(&sources.albedo_array) else {
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
        let Some(normal) = images.get_mut(&sources.normal_array) else {
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

fn repeat_sampler() -> ImageSampler {
    ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        address_mode_w: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        ..default()
    })
}

pub(crate) fn weightmap_sampler() -> ImageSampler {
    ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::ClampToEdge,
        address_mode_v: ImageAddressMode::ClampToEdge,
        address_mode_w: ImageAddressMode::ClampToEdge,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        ..default()
    })
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
        let Some(material) = materials.get_mut(&chunk.material) else {
            continue;
        };
        material.extension.water_params.z = sync.offset;
    }
}
