//! clouds systems.

use super::day_night::{lerp_color, lerp_f32, smoothstep};
use super::*;

const CLOUD_TEXTURE_PATH: &str = "sky_10_2k/sky_10_2k.png";
const CLOUD_LAYER_RADII: [f32; 1] = [1900.0];
const CLOUD_LAYER_SPEEDS: [Vec2; 1] = [Vec2::new(0.0006, 0.0)];
// Sky dome texture includes a prominent baked-in sun + sunset glow. The dome
// must therefore NOT track the real sun's azimuth (a yaw-coupled dome reads as
// a second sun orbiting the camera); the atmosphere + SunDisk own the sun, and
// the dome only drifts slowly so its glow reads as distant cloud movement.
const CLOUD_LAYER_ALPHAS: [f32; 1] = [0.3];
const CLOUD_LAYER_UV_SCALES: [Vec2; 1] = [Vec2::splat(1.0)];
const CLOUD_LAYER_UV_ROTATIONS: [f32; 1] = [0.0];
const CLOUD_LAYER_YAW_OFFSETS: [f32; 1] = [0.0];
// Authored resting orientation of the dome texture (kept from the old
// sun-aligned calibration so the pretty band starts in the same place).
const CLOUD_BASE_YAW: f32 =
    std::f32::consts::PI + std::f32::consts::FRAC_PI_2 + (170.0_f32.to_radians());
// 0.5 deg/min of wall-clock time: slow enough to read as weather, not a body
// orbiting the camera. Client-local on purpose — the dome is cosmetic, like
// the cloud-card drift velocities.
const CLOUD_YAW_DRIFT_RATE: f32 = 0.5_f32.to_radians() / 60.0;
const CLOUD_BASE_PITCH: f32 = -std::f32::consts::FRAC_PI_2;

/// Yaw of the cloud dome and card ring: fixed base orientation plus a slow
/// constant drift, deliberately independent of the sun.
fn cloud_drift_yaw(elapsed_secs: f32) -> f32 {
    CLOUD_BASE_YAW + CLOUD_YAW_DRIFT_RATE * elapsed_secs
}

const CLOUD_DAY_TINT: Color = Color::srgb(0.86, 0.92, 1.0);
const CLOUD_SUNSET_TINT: Color = Color::srgb(1.0, 0.74, 0.55);
const CLOUD_NIGHT_TINT: Color = Color::srgb(0.25, 0.3, 0.4);

const CLOUD_COVER_SEGMENT_SECS: f32 = 180.0;
const CLOUD_COVER_LERP_SPEED: f32 = 0.08;
const CLOUD_COVER_CLEAR_RANGE: (f32, f32) = (0.0, 0.25);
const CLOUD_COVER_CLOUDY_RANGE: (f32, f32) = (0.45, 0.68);

#[derive(Component, Clone, Copy)]
pub struct CloudLayer {
    pub uv_offset: Vec2,
    pub uv_speed: Vec2,
    pub uv_scale: Vec2,
    pub uv_rotation: f32,
    pub yaw_offset: f32,
    pub alpha: f32,
    /// UV offset at the last material write. Scroll accumulates every frame,
    /// but the material (and its GPU re-prepare) only updates once the drift
    /// exceeds a visible threshold.
    pub uv_offset_written: Vec2,
}

#[derive(Resource)]
pub struct CloudAssets {
    pub texture: Handle<Image>,
    pub sampler_configured: bool,
}

#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum CloudCoverMode {
    #[default]
    Auto,
    Clear,
    Cloudy,
}

#[derive(Resource, Clone, Copy, Debug)]
pub struct CloudCoverOverride {
    pub mode: CloudCoverMode,
}

impl Default for CloudCoverOverride {
    fn default() -> Self {
        Self {
            mode: CloudCoverMode::Auto,
        }
    }
}

#[derive(Resource, Clone, Copy)]
pub struct CloudCover {
    pub current: f32,
    pub target: f32,
    pub segment: i64,
}

impl Default for CloudCover {
    fn default() -> Self {
        Self {
            current: 1.0,
            target: 1.0,
            segment: -1,
        }
    }
}

/// Cached cloud tint/visibility to avoid per-frame material mutations.
/// Only mutate GPU materials when the visual actually changes.
#[derive(Resource, Default)]
pub struct CloudMaterialCache {
    pub last_tint: Option<[f32; 3]>,
    pub last_visibility: f32,
}

pub(super) fn setup_cloud_layers(
    commands: &mut Commands,
    asset_server: &Res<AssetServer>,
    meshes: &mut ResMut<Assets<Mesh>>,
    materials: &mut ResMut<Assets<StandardMaterial>>,
) {
    let cloud_texture = asset_server.load(CLOUD_TEXTURE_PATH);
    let cloud_mesh = meshes.add(Sphere::new(1.0).mesh().uv(64, 32));

    commands.insert_resource(CloudAssets {
        texture: cloud_texture.clone(),
        sampler_configured: false,
    });

    for i in 0..CLOUD_LAYER_RADII.len() {
        let alpha = CLOUD_LAYER_ALPHAS[i];
        let material = materials.add(StandardMaterial {
            base_color: Color::srgb(1.0, 1.0, 1.0),
            base_color_texture: Some(cloud_texture.clone()),
            unlit: true,
            alpha_mode: AlphaMode::Blend,
            cull_mode: None,
            perceptual_roughness: 1.0,
            metallic: 0.0,
            ..default()
        });

        commands.spawn((
            CloudLayer {
                uv_offset: Vec2::ZERO,
                uv_speed: CLOUD_LAYER_SPEEDS[i],
                uv_scale: CLOUD_LAYER_UV_SCALES[i],
                uv_rotation: CLOUD_LAYER_UV_ROTATIONS[i],
                yaw_offset: CLOUD_LAYER_YAW_OFFSETS[i],
                alpha,
                uv_offset_written: Vec2::ZERO,
            },
            NotShadowCaster,
            Mesh3d(cloud_mesh.clone()),
            MeshMaterial3d(material),
            Transform::from_scale(Vec3::splat(CLOUD_LAYER_RADII[i])),
            GlobalTransform::default(),
            Visibility::default(),
            InheritedVisibility::default(),
        ));
    }
}

pub(super) fn hash_to_unit(seed: u64, salt: u64) -> f32 {
    let mut x = seed ^ salt;
    x ^= x >> 30;
    x = x.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    x ^= x >> 27;
    x = x.wrapping_mul(0x94D0_49BB_1331_11EB);
    x ^= x >> 31;
    (x as f64 / u64::MAX as f64) as f32
}

/// Ensure cloud texture sampler repeats for UV scrolling.
pub fn apply_cloud_texture_sampler(
    mut cloud_assets: ResMut<CloudAssets>,
    mut images: ResMut<Assets<Image>>,
) {
    if cloud_assets.sampler_configured {
        return;
    }

    let Some(mut image) = images.get_mut(&cloud_assets.texture) else {
        return;
    };

    image.sampler = ImageSampler::Descriptor(ImageSamplerDescriptor {
        address_mode_u: ImageAddressMode::Repeat,
        address_mode_v: ImageAddressMode::Repeat,
        address_mode_w: ImageAddressMode::Repeat,
        mag_filter: ImageFilterMode::Linear,
        min_filter: ImageFilterMode::Linear,
        mipmap_filter: ImageFilterMode::Linear,
        ..default()
    });

    cloud_assets.sampler_configured = true;
}

/// Scroll cloud layers and tint them by sun height.
pub fn update_cloud_cover(
    time: Res<Time>,
    world_time_query: Query<&shared::components::WorldTime>,
    seed_query: Query<&shared::components::CloudSeed>,
    override_mode: Res<CloudCoverOverride>,
    settings: Res<GraphicsSettings>,
    mut cover: ResMut<CloudCover>,
) {
    if !settings.clouds_enabled {
        return;
    }
    let Some(world_time) = world_time_query.iter().next() else {
        return;
    };
    let seed = seed_query.iter().next().map(|s| s.seed).unwrap_or(0);

    let seconds = world_time.seconds_in_cycle.max(0.0);
    let segment = (seconds / CLOUD_COVER_SEGMENT_SECS).floor() as i64;
    match override_mode.mode {
        CloudCoverMode::Clear => {
            cover.segment = segment;
            cover.target = 0.0;
        }
        CloudCoverMode::Cloudy => {
            cover.segment = segment;
            // Deliberately never full overcast: an RTS must keep the world
            // readable under the weather, so "cloudy" tops out below blanket.
            cover.target = CLOUD_COVER_CLOUDY_RANGE.1;
        }
        CloudCoverMode::Auto => {
            if cover.segment != segment {
                cover.segment = segment;

                let t = world_time.normalized_time();
                let phase = t * std::f32::consts::TAU;
                let elevation = -phase.cos();
                let dawn_dusk = 1.0 - smoothstep(0.25, 0.6, elevation.abs());
                let cloudy_chance = lerp_f32(0.4, 0.65, dawn_dusk);

                let pick = hash_to_unit(seed, segment as u64);
                let roll = hash_to_unit(seed ^ 0x9E37_79B9_7F4A_7C15, segment as u64);

                cover.target = if pick < cloudy_chance {
                    lerp_f32(CLOUD_COVER_CLOUDY_RANGE.0, CLOUD_COVER_CLOUDY_RANGE.1, roll)
                } else {
                    lerp_f32(CLOUD_COVER_CLEAR_RANGE.0, CLOUD_COVER_CLEAR_RANGE.1, roll)
                };
            }
        }
    }

    let dt = time.delta_secs().max(0.0);
    let blend = 1.0 - (-CLOUD_COVER_LERP_SPEED * dt).exp();
    cover.current = lerp_f32(cover.current, cover.target, blend).clamp(0.0, 1.0);
}

/// Scroll cloud layers and tint them by sun height.
pub fn update_cloud_layers(
    time: Res<Time>,
    world_time_query: Query<&shared::components::WorldTime>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
    cover: Res<CloudCover>,
    settings: Res<GraphicsSettings>,
    map_blend: Res<crate::terrain::map_view::MapViewBlend>,
    mut layers: Query<(
        &mut CloudLayer,
        &MeshMaterial3d<StandardMaterial>,
        &mut Transform,
    )>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut cache: ResMut<CloudMaterialCache>,
) {
    if !settings.clouds_enabled {
        return;
    }
    let Ok(camera_tf) = camera.single() else {
        return;
    };

    let t = world_time_query
        .iter()
        .next()
        .map(|wt| wt.normalized_time())
        .unwrap_or(0.5);

    let phase = t * std::f32::consts::TAU;
    let elevation = -phase.cos();
    let drift_yaw = cloud_drift_yaw(time.elapsed_secs());
    let day_factor = smoothstep(-0.05, 0.15, elevation);
    let twilight = 1.0 - smoothstep(0.12, 0.35, elevation.max(0.0));
    let base_tint = lerp_color(CLOUD_NIGHT_TINT, CLOUD_DAY_TINT, day_factor);
    let tint = lerp_color(base_tint, CLOUD_SUNSET_TINT, twilight);
    // The dome is a camera-centered translucent sphere: at map zoom the whole
    // world is seen through its lower hemisphere, which reads as a grey veil
    // over the map. Fade the layer out with the map-view blend — the camera is
    // conceptually above the weather up there.
    let visibility = (0.2 + 0.8 * day_factor) * cover.current * (1.0 - map_blend.0);

    // Check if tint/visibility actually changed (avoid GPU re-upload when steady)
    let tint_rgba = tint.to_srgba();
    let tint_arr = [tint_rgba.red, tint_rgba.green, tint_rgba.blue];
    let tint_changed = cache.last_tint.is_none_or(|prev| {
        (prev[0] - tint_arr[0]).abs() > 0.005
            || (prev[1] - tint_arr[1]).abs() > 0.005
            || (prev[2] - tint_arr[2]).abs() > 0.005
            || (cache.last_visibility - visibility).abs() > 0.005
    });

    for (mut layer, material_handle, mut transform) in layers.iter_mut() {
        // Keep clouds centered on the camera (scale stays the authored radius,
        // so the dome never deforms however far the camera zooms out).
        let translation = camera_tf.translation();
        let rotation = Quat::from_rotation_y(drift_yaw + layer.yaw_offset)
            * Quat::from_rotation_x(CLOUD_BASE_PITCH);
        if transform.translation != translation || transform.rotation != rotation {
            transform.translation = translation;
            transform.rotation = rotation;
        }

        // Scroll UVs slowly for parallax.
        let delta = layer.uv_speed * time.delta_secs();
        layer.uv_offset = Vec2::new(
            (layer.uv_offset.x + delta.x).rem_euclid(1.0),
            (layer.uv_offset.y + delta.y).rem_euclid(1.0),
        );

        // Every material touch re-prepares it on the GPU, so the slow UV drift
        // batches into steps below visible size (~1 texel at dome scale) instead
        // of writing every frame.
        let scroll_due = (layer.uv_offset - layer.uv_offset_written).length() > 0.0015;
        if tint_changed || scroll_due {
            if let Some(mut material) = materials.get_mut(&material_handle.0) {
                material.uv_transform = bevy::math::Affine2::from_scale_angle_translation(
                    layer.uv_scale,
                    layer.uv_rotation,
                    layer.uv_offset,
                );
                layer.uv_offset_written = layer.uv_offset;
                if tint_changed {
                    material.base_color =
                        color_with_alpha(tint, (layer.alpha * visibility).clamp(0.0, 1.0));
                }
            }
        }
    }

    if tint_changed {
        cache.last_tint = Some(tint_arr);
        cache.last_visibility = visibility;
    }
}

fn color_with_alpha(color: Color, alpha: f32) -> Color {
    let rgba = color.to_srgba();
    Color::srgba(rgba.red, rgba.green, rgba.blue, alpha)
}
