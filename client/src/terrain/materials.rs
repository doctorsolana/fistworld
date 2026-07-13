use bevy::asset::Asset;
use bevy::image::{
    ImageAddressMode, ImageFilterMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor,
};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::reflect::TypePath;
use bevy::render::render_resource::{AsBindGroup, TextureViewDescriptor, TextureViewDimension};
use bevy::shader::ShaderRef;

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
