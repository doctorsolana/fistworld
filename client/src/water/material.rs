//! material systems.

use super::*;

const TOON_WATER_SHADER: &str = "toon_water.wgsl";

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
#[bind_group_data(ToonWaterKey)]
pub struct ToonWaterMaterial {
    #[uniform(0)]
    pub uniform: ToonWaterUniform,
    pub alpha_mode: AlphaMode,
    /// Render both faces of the water surface. Only needed while the camera is
    /// underwater; above water, back-face culling halves the water's
    /// blended-fragment cost.
    pub double_sided: bool,
}

/// Pipeline key so cull mode can respecialize when `double_sided` flips.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct ToonWaterKey {
    pub double_sided: bool,
}

impl From<&ToonWaterMaterial> for ToonWaterKey {
    fn from(material: &ToonWaterMaterial) -> Self {
        Self {
            double_sided: material.double_sided,
        }
    }
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
        key: bevy::pbr::MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        descriptor.primitive.cull_mode = if key.bind_group_data.double_sided {
            None
        } else {
            Some(bevy::render::render_resource::Face::Back)
        };
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
        double_sided: false,
    });

    commands.insert_resource(WaterRenderAssets { material });
}

/// Flip water between single-sided (camera above water — the cheap, common
/// case) and double-sided (camera underwater, so the surface stays visible
/// from below). Mutates the shared material only on the transition.
pub(super) fn update_water_cull_mode(
    terrain: Res<WorldTerrain>,
    render_assets: Option<Res<WaterRenderAssets>>,
    cameras: Query<&GlobalTransform, With<Camera3d>>,
    mut materials: ResMut<Assets<ToonWaterMaterial>>,
) {
    let Some(render_assets) = render_assets else {
        return;
    };
    let Some(water_level) = terrain.water_level() else {
        return;
    };
    let Some(camera_tf) = cameras.iter().next() else {
        return;
    };

    // Small margin so wave displacement near the surface never shows a culled
    // backside for a frame.
    let wants_double_sided = camera_tf.translation().y <= water_level + 0.6;

    // Read first: `get_mut` marks the asset changed and would re-prepare the
    // material every frame.
    let Some(material) = materials.get(&render_assets.material) else {
        return;
    };
    if material.double_sided == wants_double_sided {
        return;
    }
    if let Some(material) = materials.get_mut(&render_assets.material) {
        material.double_sided = wants_double_sided;
    }
}
