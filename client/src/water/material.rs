//! material systems.

use super::*;
use shared::components::WorldTime;
use shared::water::{
    OCEAN_LOOP_SECONDS, WATER_DEEP_SWELL_AMPLITUDE, WATER_SWELL_DEPTH_FULL, WATER_SWELL_DEPTH_START,
};

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
    /// x: max swell amplitude, y/z: depth fade, w: server clock offset.
    pub wave_params: Vec4,
    /// xyz: direction to the sun (world), w: glint strength (0 at night).
    pub sun_params: Vec4,
    /// Interaction ripples: xy = world xz, z = spawn time, w = strength
    /// (0 = slot empty). The shader animates each ring from its spawn time.
    pub ripples: [Vec4; 8],
    /// Cloud shadow field: x coverage, y inv world scale, zw wind offset.
    pub clouds_a: Vec4,
    /// Cloud shadow field: xy sun projection (sun_dir.xz / sun_dir.y),
    /// z shadow strength, w seed phase.
    pub clouds_b: Vec4,
    /// x: anchor time (client seconds), z: drift speed (client-time units).
    pub clouds_c: Vec4,
    /// Reserved (water skips snow); mirrors the terrain palette lane.
    pub climate: Vec4,
    /// THE storm system: xy = center at the wind anchor, z = storminess
    /// (0 while clouds are disabled), w: reserved.
    pub storm: Vec4,
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
            // x: foam scale, y/z: shore-distance swell fade, w: near-shore swell multiplier
            ring_params: Vec4::new(1.35, 0.03, 0.72, 0.12),
            wave_params: Vec4::new(
                WATER_DEEP_SWELL_AMPLITUDE,
                WATER_SWELL_DEPTH_START,
                WATER_SWELL_DEPTH_FULL,
                0.0,
            ),
            // Overwritten every frame the sun moves appreciably.
            sun_params: Vec4::new(0.35, 0.75, 0.30, 1.1),
            ripples: [Vec4::new(0.0, 0.0, -100.0, 0.0); 8],
            // Cloud shadows start off (strength 0 = shade 1.0 exactly);
            // sync_cloud_shadow_params owns these fields at runtime.
            clouds_a: Vec4::ZERO,
            clouds_b: Vec4::ZERO,
            clouds_c: Vec4::ZERO,
            climate: Vec4::ZERO,
            storm: Vec4::new(1.0e8, 1.0e8, 0.0, 0.0),
        },
        alpha_mode: AlphaMode::Blend,
        double_sided: false,
    });

    commands.insert_resource(WaterRenderAssets { material });
}

#[derive(Default)]
pub(super) struct WaterWaveClockSync {
    world_time_entity: Option<Entity>,
}

/// Align Bevy's client-local shader clock once for each replicated world-clock
/// entity. Both clocks then advance locally at the same rate, avoiding any
/// per-frame material updates or packet-timing jitter in the waves.
pub(super) fn sync_water_wave_clock(
    time: Res<Time>,
    render_assets: Option<Res<WaterRenderAssets>>,
    world_time: Query<(Entity, &WorldTime)>,
    mut materials: ResMut<Assets<ToonWaterMaterial>>,
    mut sync: Local<WaterWaveClockSync>,
) {
    let Some(render_assets) = render_assets else {
        return;
    };
    let Ok((world_time_entity, world_time)) = world_time.single() else {
        return;
    };
    if sync.world_time_entity == Some(world_time_entity) {
        return;
    }

    let local = time.elapsed_secs_wrapped().rem_euclid(OCEAN_LOOP_SECONDS);
    let half_loop = OCEAN_LOOP_SECONDS * 0.5;
    let offset =
        (world_time.ocean_seconds - local + half_loop).rem_euclid(OCEAN_LOOP_SECONDS) - half_loop;

    if let Some(mut material) = materials.get_mut(&render_assets.material) {
        material.uniform.wave_params.w = offset;
        sync.world_time_entity = Some(world_time_entity);
    }
}

/// Keep the water's glint direction in sync with the day/night sun. Only
/// mutates the material when the sun has moved appreciably, so the material
/// isn't re-prepared every frame.
pub(super) fn update_water_sun_dir(
    render_assets: Option<Res<WaterRenderAssets>>,
    sun: Query<&GlobalTransform, With<SunLight>>,
    mut materials: ResMut<Assets<ToonWaterMaterial>>,
) {
    let Some(render_assets) = render_assets else {
        return;
    };
    let Ok(sun_tf) = sun.single() else {
        return;
    };

    let to_sun = Vec3::from(sun_tf.back());
    // Glints fade out as the sun approaches the horizon.
    let strength = 1.1 * to_sun.y.clamp(0.0, 1.0).sqrt();
    let target = Vec4::new(to_sun.x, to_sun.y, to_sun.z, strength);

    let Some(material) = materials.get(&render_assets.material) else {
        return;
    };
    if material.uniform.sun_params.distance_squared(target) < 1e-4 {
        return;
    }
    if let Some(mut material) = materials.get_mut(&render_assets.material) {
        material.uniform.sun_params = target;
    }
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
    if let Some(mut material) = materials.get_mut(&render_assets.material) {
        material.double_sided = wants_double_sided;
    }
}
