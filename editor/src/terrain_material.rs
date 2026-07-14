use bevy::asset::{Asset, RenderAssetUsages};
use bevy::image::{
    ImageAddressMode, ImageFilterMode, ImageLoaderSettings, ImageSampler, ImageSamplerDescriptor,
};
use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::reflect::TypePath;
use bevy::render::render_resource::{
    AsBindGroup, Extent3d, TextureDimension, TextureFormat, TextureViewDescriptor,
    TextureViewDimension,
};
use bevy::shader::ShaderRef;

pub type EditorTerrainSplatMaterial =
    ExtendedMaterial<StandardMaterial, EditorTerrainSplatExtension>;

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct EditorTerrainSplatExtension {
    #[texture(100)]
    #[sampler(101)]
    pub weight_map: Handle<Image>,
    #[texture(102, dimension = "2d_array")]
    #[sampler(103)]
    pub albedo_array: Handle<Image>,
    #[texture(104, dimension = "2d_array")]
    #[sampler(105)]
    pub normal_array: Handle<Image>,
    #[uniform(120)]
    pub layer_tiling: Vec4,
    #[uniform(121)]
    pub debug_mode: u32,
    #[uniform(122)]
    pub normal_strength: f32,
    /// x: water level (world Y), y: 1.0 when water is shown, zw: unused.
    #[uniform(123)]
    pub water_params: Vec4,
}

impl MaterialExtension for EditorTerrainSplatExtension {
    fn fragment_shader() -> ShaderRef {
        "shaders/terrain_splat.wgsl".into()
    }

    fn deferred_fragment_shader() -> ShaderRef {
        "shaders/terrain_splat.wgsl".into()
    }
}

#[derive(Resource)]
pub struct EditorTerrainTextureAssets {
    pub albedo_array: Handle<Image>,
    pub normal_array: Handle<Image>,
    pub layer_tiling: Vec4,
    configured: bool,
}

impl FromWorld for EditorTerrainTextureAssets {
    fn from_world(world: &mut World) -> Self {
        let asset_server = world.resource::<AssetServer>().clone();
        let albedo_array = asset_server.load_with_settings(
            "textures/terrain/optimized_1k/terrain_albedo_array.ktx2",
            |settings: &mut ImageLoaderSettings| {
                settings.is_srgb = true;
                settings.sampler = repeating_sampler();
            },
        );
        let normal_array = asset_server.load_with_settings(
            "textures/terrain/optimized_1k/terrain_normal_array.ktx2",
            |settings: &mut ImageLoaderSettings| {
                settings.is_srgb = false;
                settings.sampler = repeating_sampler();
            },
        );

        Self {
            albedo_array,
            normal_array,
            layer_tiling: Vec4::new(8.0, 7.0, 6.0, 5.0),
            configured: false,
        }
    }
}

pub fn configure_editor_terrain_arrays(
    mut assets: ResMut<EditorTerrainTextureAssets>,
    mut images: ResMut<Assets<Image>>,
) {
    if assets.configured
        || images.get(&assets.albedo_array).is_none()
        || images.get(&assets.normal_array).is_none()
    {
        return;
    }

    configure_array_image(
        images
            .get_mut(&assets.albedo_array)
            .expect("albedo array was checked above"),
        "editor_terrain_albedo_array",
    );
    configure_array_image(
        images
            .get_mut(&assets.normal_array)
            .expect("normal array was checked above"),
        "editor_terrain_normal_array",
    );
    assets.configured = true;
    info!("Editor terrain texture arrays loaded");
}

pub fn create_weightmap_image(weights: &[[u8; 4]], resolution: u32) -> Image {
    let mut bytes = Vec::with_capacity((resolution * resolution * 4) as usize);
    for weight in weights {
        bytes.extend_from_slice(weight);
    }

    let mut image = Image::new(
        Extent3d {
            width: resolution,
            height: resolution,
            depth_or_array_layers: 1,
        },
        TextureDimension::D2,
        bytes,
        TextureFormat::Rgba8Unorm,
        RenderAssetUsages::RENDER_WORLD | RenderAssetUsages::MAIN_WORLD,
    );
    image.sampler = clamped_sampler();
    image
}

pub fn update_weightmap_image(image: &mut Image, weights: &[[u8; 4]]) {
    let bytes = image.data.get_or_insert_with(Vec::new);
    bytes.clear();
    bytes.reserve(weights.len() * 4);
    for weight in weights {
        bytes.extend_from_slice(weight);
    }
}

fn configure_array_image(image: &mut Image, label: &'static str) {
    image.sampler = repeating_sampler();
    image.texture_view_descriptor = Some(TextureViewDescriptor {
        label: Some(label),
        dimension: Some(TextureViewDimension::D2Array),
        ..default()
    });
}

fn repeating_sampler() -> ImageSampler {
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

fn clamped_sampler() -> ImageSampler {
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
