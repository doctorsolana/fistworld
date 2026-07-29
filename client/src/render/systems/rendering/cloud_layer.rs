//! World-anchored cloud plane.
//!
//! One huge horizontal quad at [`CLOUD_LAYER_HEIGHT`] whose fragment shader
//! evaluates a deterministic world-space cloud field (the same field the
//! terrain/water shaders project as cloud shadows — the wind/seed/coverage
//! params here must stay bit-identical with theirs). The mesh follows the
//! camera in XZ; the noise samples `world_position`, so mesh motion is
//! invisible.

use super::clouds::{hash_to_unit, CloudCover};
use super::day_night::{lerp_color, smoothstep};
use super::*;
use bevy::light::NotShadowReceiver;
use bevy::mesh::MeshVertexBufferLayoutRef;
use bevy::reflect::TypePath;
use bevy::render::render_resource::{
    AsBindGroup, RenderPipelineDescriptor, ShaderType, SpecializedMeshPipelineError,
};
use bevy::shader::ShaderRef;
use shared::components::{CloudSeed, WorldTime};

const CLOUD_LAYER_SHADER: &str = "shaders/cloud_layer.wgsl";

/// Cloud deck altitude in world metres (terrain tops out around 60). Must
/// match the `CLOUD_LAYER_HEIGHT` const in cloud_layer.wgsl and the shadow
/// projection in the terrain/water shaders.
pub const CLOUD_LAYER_HEIGHT: f32 = 350.0;
/// Covers the 12km map-zoom view with margin.
const CLOUD_PLANE_SIZE: f32 = 40_000.0;
/// Dominant blob scale of the cloud field (shared design constant).
const CLOUD_FIELD_INV_SCALE: f32 = 1.0 / 190.0;
/// Fixed wind bearing + speed (world units/sec). Shared with the terrain and
/// water cloud-shadow params — any change must be mirrored there.
const CLOUD_ALPHA_SCALE: f32 = 0.85;

// Diff-gates: every materials.get_mut re-prepares the material on the GPU, so
// writes only happen past thresholds that are sub-pixel at any RTS zoom.
const PLANE_FOLLOW_STEP: f32 = 1.0;
// Wind is anchor-extrapolated in the shader (globals.time); the anchor
// refreshes ~1/sec or when gust/warp changes the drift speed.
const ANCHOR_REFRESH_SECS: f32 = 1.0;
const SPEED_WRITE_STEP: f32 = 0.01;
const PARAM_EPSILON: f32 = 0.005;

// Lit/shadow tint stops. Shadow tone stays a sky-blue-grey at >= ~60% of the
// lit tone's brightness so clouds never go muddy (the field's half-Lambert
// only mixes between these two).
const CLOUD_PLANE_DAY_LIT: Color = Color::srgb(1.0, 1.0, 0.94);
const CLOUD_PLANE_DAY_SHADOW: Color = Color::srgb(0.63, 0.71, 0.85);
const CLOUD_PLANE_SUNSET_LIT: Color = Color::srgb(1.0, 0.78, 0.58);
const CLOUD_PLANE_SUNSET_SHADOW: Color = Color::srgb(0.56, 0.51, 0.64);
const CLOUD_PLANE_NIGHT_LIT: Color = Color::srgb(0.22, 0.27, 0.38);
const CLOUD_PLANE_NIGHT_SHADOW: Color = Color::srgb(0.14, 0.18, 0.28);

#[derive(ShaderType, Debug, Clone, Copy)]
pub struct CloudLayerUniform {
    /// x: coverage 0..1, y: inv world scale, zw: wind offset (world units).
    pub params_a: Vec4,
    /// x: seed phase, y: day factor, z: drift anchor time (client seconds),
    /// w: alpha scale.
    pub params_b: Vec4,
    /// xyz: sun light travel direction (sun toward world), w: drift speed in
    /// client-time units (world wind speed x time warp).
    pub sun_dir: Vec4,
    pub tint_lit: Vec4,
    pub tint_shadow: Vec4,
}

#[derive(Asset, TypePath, AsBindGroup, Debug, Clone)]
pub struct CloudLayerMaterial {
    #[uniform(0)]
    pub uniform: CloudLayerUniform,
}

impl Material for CloudLayerMaterial {
    fn fragment_shader() -> ShaderRef {
        CLOUD_LAYER_SHADER.into()
    }

    fn specialize(
        _pipeline: &bevy::pbr::MaterialPipeline,
        descriptor: &mut RenderPipelineDescriptor,
        _layout: &MeshVertexBufferLayoutRef,
        _key: bevy::pbr::MaterialPipelineKey<Self>,
    ) -> Result<(), SpecializedMeshPipelineError> {
        // The camera can cross the deck, so both faces must draw.
        descriptor.primitive.cull_mode = None;
        Ok(())
    }

    fn alpha_mode(&self) -> AlphaMode {
        AlphaMode::Blend
    }
}

/// Marker for the single world-anchored cloud plane entity.
#[derive(Component)]
pub struct CloudLayerPlane;

#[derive(Resource)]
pub struct CloudPlaneSpawned;

/// Spawn the plane once the replicated cloud seed arrives (the seed phase is
/// baked into the material, so spawning can't happen earlier).
pub fn spawn_cloud_plane(
    mut commands: Commands,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<CloudLayerMaterial>>,
    seed_query: Query<&CloudSeed>,
    spawned: Option<Res<CloudPlaneSpawned>>,
    settings: Res<GraphicsSettings>,
) {
    if spawned.is_some() {
        return;
    }
    if !settings.clouds_enabled {
        return;
    }
    let Some(seed) = seed_query.iter().next().map(|s| s.seed) else {
        return;
    };

    let seed_phase = hash_to_unit(seed, 0) * 37.0;
    let material = materials.add(CloudLayerMaterial {
        uniform: CloudLayerUniform {
            params_a: Vec4::new(1.0, CLOUD_FIELD_INV_SCALE, 0.0, 0.0),
            params_b: Vec4::new(seed_phase, 1.0, 0.0, CLOUD_ALPHA_SCALE),
            sun_dir: Vec4::new(0.0, -1.0, 0.0, 0.0),
            tint_lit: color_vec4(CLOUD_PLANE_DAY_LIT),
            tint_shadow: color_vec4(CLOUD_PLANE_DAY_SHADOW),
        },
    });

    commands.spawn((
        CloudLayerPlane,
        NotShadowCaster,
        NotShadowReceiver,
        Mesh3d(meshes.add(Plane3d::default().mesh().size(CLOUD_PLANE_SIZE, CLOUD_PLANE_SIZE))),
        MeshMaterial3d(material),
        Transform::from_xyz(0.0, CLOUD_LAYER_HEIGHT, 0.0),
        GlobalTransform::default(),
        Visibility::default(),
        InheritedVisibility::default(),
    ));
    commands.insert_resource(CloudPlaneSpawned);
}

/// Last-written material params so steady frames touch nothing.
#[derive(Default)]
pub struct CloudPlaneCache {
    written: bool,
    wind_offset: Vec2,
    anchor_time: f32,
    speed_client: f32,
    coverage: f32,
    day_factor: f32,
    sun_dir: Vec4,
    tint_lit: Vec4,
    tint_shadow: Vec4,
}

/// Follow the camera in XZ and push coverage/wind/tint params.
pub fn update_cloud_plane(
    time: Res<Time>,
    world_time_query: Query<&WorldTime>,
    warp_query: Query<&shared::components::TimeWarp>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
    sun: Query<&GlobalTransform, With<SunLight>>,
    cover: Res<CloudCover>,
    settings: Res<GraphicsSettings>,
    mut plane: Query<(&mut Transform, &MeshMaterial3d<CloudLayerMaterial>), With<CloudLayerPlane>>,
    mut materials: ResMut<Assets<CloudLayerMaterial>>,
    mut cache: Local<CloudPlaneCache>,
) {
    if !settings.clouds_enabled {
        return;
    }
    let Ok((mut transform, material_handle)) = plane.single_mut() else {
        return;
    };
    let Some(world_time) = world_time_query.iter().next() else {
        return;
    };
    let Ok(camera_tf) = camera.single() else {
        return;
    };

    // Keep the quad centered under/over the camera. Sampling is world-space,
    // so the hop is invisible; 1m steps keep the transform quiet when idle.
    let cam = camera_tf.translation();
    let dx = cam.x - transform.translation.x;
    let dz = cam.z - transform.translation.z;
    if dx * dx + dz * dz > PLANE_FOLLOW_STEP * PLANE_FOLLOW_STEP {
        transform.translation = Vec3::new(cam.x, CLOUD_LAYER_HEIGHT, cam.z);
    }

    // ABSOLUTE world seconds: monotonic across the day wrap and warp-aware.
    // Must match the terrain/water cloud-shadow clock exactly, or the shadows
    // desync from the visible deck.
    let world_seconds =
        world_time.day as f32 * world_time.cycle_duration() + world_time.seconds_in_cycle;
    // Seed phase was baked into the material at spawn; it also seeds the wind
    // swell phases, keeping plane and shadow drift byte-identical.
    let seed_phase = materials
        .get(&material_handle.0)
        .map(|m| m.uniform.params_b.x)
        .unwrap_or(0.0);
    let (wind_offset, wind_speed) = super::clouds::cloud_wind_state(world_seconds, seed_phase);
    // The shader extrapolates drift with globals.time (client seconds), so the
    // anchored speed carries the warp factor. Must mirror cloud_shadows.rs.
    let warp = warp_query.iter().next().map(|w| w.0).unwrap_or(1.0);
    let anchor_time = time.elapsed_secs();
    let speed_client = wind_speed * warp;

    // Same day/night cadence as the dome tints in update_cloud_layers.
    let elevation = -world_time.sun_phase().cos();
    let day_factor = smoothstep(-0.05, 0.15, elevation);
    let twilight = 1.0 - smoothstep(0.12, 0.35, elevation.max(0.0));
    let tint_lit = color_vec4(lerp_color(
        lerp_color(CLOUD_PLANE_NIGHT_LIT, CLOUD_PLANE_DAY_LIT, day_factor),
        CLOUD_PLANE_SUNSET_LIT,
        twilight,
    ));
    let tint_shadow = color_vec4(lerp_color(
        lerp_color(CLOUD_PLANE_NIGHT_SHADOW, CLOUD_PLANE_DAY_SHADOW, day_factor),
        CLOUD_PLANE_SUNSET_SHADOW,
        twilight,
    ));

    let sun_dir = sun
        .single()
        .map(|tf| {
            let d = Vec3::from(tf.forward());
            Vec4::new(d.x, d.y, d.z, 0.0)
        })
        .unwrap_or(cache.sun_dir);

    let dirty = !cache.written
        || anchor_time - cache.anchor_time > ANCHOR_REFRESH_SECS
        || (cache.speed_client - speed_client).abs() > SPEED_WRITE_STEP
        || (cache.coverage - cover.current).abs() > PARAM_EPSILON
        || (cache.day_factor - day_factor).abs() > PARAM_EPSILON
        || cache.sun_dir.distance_squared(sun_dir) > 1e-4
        || (cache.tint_lit - tint_lit).abs().max_element() > PARAM_EPSILON
        || (cache.tint_shadow - tint_shadow).abs().max_element() > PARAM_EPSILON;
    if !dirty {
        return;
    }

    let Some(mut material) = materials.get_mut(&material_handle.0) else {
        return;
    };
    material.uniform.params_a = Vec4::new(
        cover.current,
        CLOUD_FIELD_INV_SCALE,
        wind_offset.x,
        wind_offset.y,
    );
    // Preserve the baked seed phase in .x; .z carries the drift anchor time.
    material.uniform.params_b.y = day_factor;
    material.uniform.params_b.z = anchor_time;
    material.uniform.params_b.w = CLOUD_ALPHA_SCALE;
    // sun_dir.w carries the drift speed in client-time units.
    material.uniform.sun_dir = Vec4::new(sun_dir.x, sun_dir.y, sun_dir.z, speed_client);
    material.uniform.tint_lit = tint_lit;
    material.uniform.tint_shadow = tint_shadow;

    cache.written = true;
    cache.wind_offset = wind_offset;
    cache.anchor_time = anchor_time;
    cache.speed_client = speed_client;
    cache.coverage = cover.current;
    cache.day_factor = day_factor;
    cache.sun_dir = sun_dir;
    cache.tint_lit = tint_lit;
    cache.tint_shadow = tint_shadow;
}

fn color_vec4(color: Color) -> Vec4 {
    let l = color.to_linear();
    Vec4::new(l.red, l.green, l.blue, 1.0)
}
