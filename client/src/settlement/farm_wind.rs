//! Shared crop wind; all animation runs on the GPU with authored vertex weights.

use bevy::pbr::{ExtendedMaterial, MaterialExtension};
use bevy::prelude::*;
use bevy::render::render_resource::AsBindGroup;
use bevy::shader::ShaderRef;

pub(super) type CropWindMaterial = ExtendedMaterial<StandardMaterial, CropWind>;

// No extra uniform binding: Bevy's opaque depth-only/shadow path intentionally
// omits the material bind group. The shared direction literal is covered by
// crate::wind's shader parity test, just like the existing foliage material.
#[derive(Asset, TypePath, AsBindGroup, Clone, Debug, Default)]
pub(super) struct CropWind {}

impl MaterialExtension for CropWind {
    fn vertex_shader() -> ShaderRef {
        "shaders/crop_wind.wgsl".into()
    }

    fn prepass_vertex_shader() -> ShaderRef {
        "shaders/crop_wind.wgsl".into()
    }

    fn deferred_vertex_shader() -> ShaderRef {
        "shaders/crop_wind.wgsl".into()
    }
}
