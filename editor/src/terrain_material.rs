//! Editor-side terrain texture assets.
//!
//! There is deliberately **no material definition here**. The editor renders terrain with the
//! same `TerrainSplatExtension` the client uses, from `shared::terrain`, because it loads the
//! same `terrain_splat.wgsl`.
//!
//! It used to declare its own. That struct listed bindings 100-105 and 120-123; when the shader
//! gained `palette` at binding 124 the editor's copy was not updated, its pipeline layout stopped
//! matching the shader, both the forward and deferred pipelines failed validation, and Bevy 0.19
//! escalated that to a process exit ~20-40 s after launch. The editor was dead from `8b189ca`
//! until this file stopped duplicating the definition. Two bind groups against one shader is a
//! mismatch nothing can catch -- not the compiler, not a test -- so the only real fix is to have
//! one.

use bevy::asset::RenderAssetUsages;
use bevy::image::{ImageLoaderSettings, ImageSampler};
use bevy::prelude::*;
use bevy::render::render_resource::{
    Extent3d, TextureDimension, TextureFormat, TextureViewDescriptor, TextureViewDimension,
};

use shared::terrain::{
    layer_tiling, repeat_sampler, weightmap_sampler, TERRAIN_ALBEDO_ARRAY, TERRAIN_NORMAL_ARRAY,
};

/// The one terrain material. Aliased rather than redefined -- see the module docs.
pub type EditorTerrainSplatMaterial = shared::terrain::TerrainSplatMaterial;
pub type EditorTerrainSplatExtension = shared::terrain::TerrainSplatExtension;

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
        let albedo_array = asset_server
            .load_builder()
            .with_settings(|settings: &mut ImageLoaderSettings| {
                settings.is_srgb = true;
                settings.sampler = repeat_sampler();
            })
            .load(TERRAIN_ALBEDO_ARRAY);
        let normal_array = asset_server
            .load_builder()
            .with_settings(|settings: &mut ImageLoaderSettings| {
                settings.is_srgb = false;
                settings.sampler = repeat_sampler();
            })
            .load(TERRAIN_NORMAL_ARRAY);

        Self {
            albedo_array,
            normal_array,
            layer_tiling: layer_tiling(),
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
        &mut images
            .get_mut(&assets.albedo_array)
            .expect("albedo array was checked above"),
        "editor_terrain_albedo_array",
    );
    configure_array_image(
        &mut images
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
    image.sampler = repeat_sampler();
    image.texture_view_descriptor = Some(TextureViewDescriptor {
        label: Some(label),
        dimension: Some(TextureViewDimension::D2Array),
        ..default()
    });
}

/// The weightmap sampler, by its editor-local name. Descriptor lives in `shared`.
fn clamped_sampler() -> ImageSampler {
    weightmap_sampler()
}
