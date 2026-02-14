//! material systems.

use super::*;

const TOON_WATER_SHADER: &str = "toon_water.wgsl";

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct ToonWaterMaterial {
    #[uniform(0)]
    pub uniform: ToonWaterUniform,
    pub alpha_mode: AlphaMode,
}

#[derive(ShaderType, Debug, Clone, Copy)]
pub struct ToonWaterUniform {
    pub shallow_color: LinearRgba,
    pub deep_color: LinearRgba,
    pub foam_color: LinearRgba,
    pub foam_params: Vec4,
    pub ring_params: Vec4,
    pub wave_params: Vec4,
}

impl Material for ToonWaterMaterial {
    fn vertex_shader() -> ShaderRef {
        TOON_WATER_SHADER.into()
    }

    fn fragment_shader() -> ShaderRef {
        TOON_WATER_SHADER.into()
    }

    fn specialize(
        _pipeline: &bevy::pbr::MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: bevy::pbr::MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = None;
        Ok(())
    }

    fn alpha_mode(&self) -> AlphaMode {
        self.alpha_mode
    }
}

pub(super) fn setup_water_assets(
    mut commands: Commands,
    mut materials: ResMut<Assets<ToonWaterMaterial>>,
) {
    let material = materials.add(ToonWaterMaterial {
        uniform: ToonWaterUniform {
            shallow_color: LinearRgba::new(0.12, 0.62, 0.92, 0.70),
            deep_color: LinearRgba::new(0.02, 0.16, 0.40, 0.84),
            foam_color: LinearRgba::new(0.96, 0.98, 1.00, 1.0),
            // x: foam edge width, y: foam smoothness, z: fleck density, w: flow speed
            foam_params: Vec4::new(0.16, 0.055, 1.0, 0.16),
            // x: wave scale, y: shore min depth, z: shore max depth, w: shore noise amount
            ring_params: Vec4::new(1.35, 0.03, 0.14, 0.012),
            // x: wave amplitude, y: wave frequency, z: wave speed, w: depth start
            wave_params: Vec4::new(0.28, 0.095, 1.0, 0.12),
        },
        alpha_mode: AlphaMode::Blend,
    });

    commands.insert_resource(WaterRenderAssets { material });
}
