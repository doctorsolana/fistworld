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
const CLOUD_YAW_DRIFT_RATE: f32 = 2.0_f32.to_radians() / 60.0;
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
const CLOUD_COVER_CLEAR_RANGE: (f32, f32) = (0.0, 0.15);
const CLOUD_COVER_CLOUDY_RANGE: (f32, f32) = (0.30, 0.50);
/// Base (between-cells) cover while a storm system is on the map: the drama
/// lives in the drifting cells, so the sky between them stays broken, not
/// blanketed — the world must remain readable even mid-storm.
const CLOUD_COVER_STORM_BASE: (f32, f32) = (0.18, 0.30);

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
    /// Dev/capture override: a full storm system overhead.
    Storm,
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
    /// Storm-system strength 0..1: gates the drifting storm cells that
    /// locally thicken cloud and rain-darken the ground.
    pub storminess: f32,
    pub storm_target: f32,
    pub segment: i64,
}

impl Default for CloudCover {
    fn default() -> Self {
        // Start near-clear: the lerp toward the rolled weather is slow
        // (~90s), and joining the world under a dissolving cloud blanket
        // read as a rendering glitch rather than weather.
        Self {
            current: 0.12,
            target: 0.12,
            storminess: 0.0,
            storm_target: 0.0,
            segment: -1,
        }
    }
}

impl CloudCover {
    /// Fully-settled state for a forced weather mode: captures and the god-mode
    /// FORCE buttons want the look NOW, not after the ~90s natural transition.
    pub fn snapped(mode: CloudCoverMode) -> Self {
        let (cover, storm) = match mode {
            CloudCoverMode::Auto => return Self::default(),
            CloudCoverMode::Clear => (0.0, 0.0),
            CloudCoverMode::Cloudy => (CLOUD_COVER_CLOUDY_RANGE.1, 0.0),
            CloudCoverMode::Storm => (CLOUD_COVER_STORM_BASE.1, 1.0),
        };
        Self {
            current: cover,
            target: cover,
            storminess: storm,
            storm_target: storm,
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

/// Fixed wind bearing shared by the cloud plane and the cloud shadows.
///
/// The world's ONE wind now, rather than a bearing of its own. It was
/// `(0.86, 0.5)`: 4.7 degrees off the foliage and 0.99479 long, so cloud drift
/// ran half a percent slower than `CLOUD_WIND_SPEED` claimed and the storm's
/// 400 m meander below was really 397.9 m. Both are exact now.
const CLOUD_WIND_BEARING: Vec2 = crate::wind::WIND_DIRECTION;
/// Wind speed wanders between these bounds (world units/sec) on slow swells,
/// keeping the 1:3 calm-to-gust ratio. Deliberately far above realistic
/// (~0.3): clouds should visibly ROLL over the world at 1x game speed.
const CLOUD_WIND_SPEED_MIN: f32 = 1.5;
const CLOUD_WIND_SPEED_MAX: f32 = 4.5;

/// World-space cloud drift offset at an absolute world time.
///
/// THE single source of wind for the cloud plane and the terrain/water cloud
/// shadows — both must call this or they desync. The varying speed is applied
/// as its exact closed-form integral, so gusts accelerate the drift without
/// ever teleporting the field, and the result stays deterministic, identical
/// across clients, and time-warp aware (absolute world seconds in, offset out).
/// Wind offset AND instantaneous speed (world units/sec along the bearing).
///
/// The speed is what shaders extrapolate with between anchor writes: material
/// uniforms carry (offset at anchor time, speed), and `globals.time` advances
/// the drift per-frame, so cloud/shadow motion is frame-smooth while material
/// re-uploads stay at ~1/sec.
pub(super) fn cloud_wind_state(abs_seconds: f32, seed_phase: f32) -> (Vec2, f32) {
    use std::f32::consts::TAU;
    let amp = 0.5 * (CLOUD_WIND_SPEED_MAX - CLOUD_WIND_SPEED_MIN);
    let mid = CLOUD_WIND_SPEED_MIN + amp;
    // Two incommensurate swell periods so gusts never settle into a loop.
    let w1 = TAU / 540.0;
    let w2 = TAU / 197.0;
    let p1 = seed_phase;
    let p2 = seed_phase * 2.7;
    // ∫ mid + amp·(0.7·sin(w1·t+p1) + 0.3·sin(w2·t+p2)) dt, anchored so the
    // integral is 0 at t = 0.
    let integral = mid * abs_seconds
        + amp
            * (0.7 * (p1.cos() - (w1 * abs_seconds + p1).cos()) / w1
                + 0.3 * (p2.cos() - (w2 * abs_seconds + p2).cos()) / w2);
    let speed = mid
        + amp
            * (0.7 * (w1 * abs_seconds + p1).sin() + 0.3 * (w2 * abs_seconds + p2).sin());
    (CLOUD_WIND_BEARING * integral, speed)
}

/// How much slower THE storm system drifts than the clouds streaming through
/// it. Must match the extrapolation factor in the shaders' storm callers.
pub(super) const STORM_DRIFT_FACTOR: f32 = 0.55;
/// Outer radius of the storm disc (m). Must match `storm_cell` in the shaders.
pub(super) const STORM_EDGE_RADIUS: f32 = 1300.0;
/// The storm center is reflected inside +/- this fraction of the half extent,
/// so a storm is ALWAYS somewhere on the map — the drift is slow (~1.7 m/s),
/// and a wrapped-off-map storm would leave FORCE STORM showing nothing for
/// tens of minutes.
const STORM_TRACK_LIMIT_FRAC: f32 = 0.85;

/// Center of THE storm system — one per map, by design: a concentrated squall
/// that drifts over the land, not a field of cells. Deterministic in
/// (seed phase, absolute world seconds), so every client sees the same storm
/// in the same place. Drifts at [`STORM_DRIFT_FACTOR`]x the cloud wind plus a
/// slow crosswind meander (amplitude/rate kept small enough that the ~1 Hz
/// anchor writes never step visibly). The track REFLECTS off the map bounds
/// (triangle fold) instead of wrapping: continuous position, no teleport, and
/// the storm never leaves the playfield.
pub(super) fn storm_center(seed_phase: f32, abs_seconds: f32, half_extent: f32) -> Vec2 {
    let (wind_offset, _) = cloud_wind_state(abs_seconds, seed_phase);
    // Float-only spawn hash: derived from the same seed phase the shaders
    // already carry, so no extra plumbing.
    let h = |k: f32| ((seed_phase * k).sin() * 43758.5453).fract();
    let spawn = Vec2::new(
        (h(12.9898) - 0.5) * 1.6 * half_extent,
        (h(78.233) - 0.5) * 1.6 * half_extent,
    );
    let perp = Vec2::new(-CLOUD_WIND_BEARING.y, CLOUD_WIND_BEARING.x);
    let meander = perp * (400.0 * (abs_seconds * 0.002 + seed_phase).sin());
    let pos = spawn + wind_offset * STORM_DRIFT_FACTOR + meander;
    let limit = half_extent * STORM_TRACK_LIMIT_FRAC;
    let reflect = |v: f32| {
        let u = (v + limit).rem_euclid(4.0 * limit);
        let folded = if u <= 2.0 * limit { u } else { 4.0 * limit - u };
        folded - limit
    };
    Vec2::new(reflect(pos.x), reflect(pos.y))
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
    mut cover: ResMut<CloudCover>,
) {
    // Deliberately NOT gated on clouds_enabled: the weather state must keep
    // evolving while clouds are switched off (the GPU pushes gate the
    // visuals), or storminess freezes at its last value and the rain
    // darkening would come back stale when clouds are re-enabled.
    let Some(world_time) = world_time_query.iter().next() else {
        return;
    };
    let seed = seed_query.iter().next().map(|s| s.seed).unwrap_or(0);

    // Absolute calendar seconds, not time-of-day: weather segments must not
    // replay the same pattern every day.
    let seconds = (world_time.day as f32 * world_time.cycle_duration()
        + world_time.seconds_in_cycle.max(0.0))
    .max(0.0);
    let segment = (seconds / CLOUD_COVER_SEGMENT_SECS).floor() as i64;
    match override_mode.mode {
        CloudCoverMode::Clear => {
            cover.segment = segment;
            cover.target = 0.0;
            cover.storm_target = 0.0;
        }
        CloudCoverMode::Cloudy => {
            cover.segment = segment;
            // Deliberately never full overcast: an RTS must keep the world
            // readable under the weather, so "cloudy" tops out below blanket.
            cover.target = CLOUD_COVER_CLOUDY_RANGE.1;
            cover.storm_target = 0.0;
        }
        CloudCoverMode::Storm => {
            cover.segment = segment;
            cover.target = CLOUD_COVER_STORM_BASE.1;
            cover.storm_target = 1.0;
        }
        CloudCoverMode::Auto => {
            if cover.segment != segment {
                cover.segment = segment;

                let elevation = -world_time.sun_phase().cos();
                let dawn_dusk = 1.0 - smoothstep(0.25, 0.6, elevation.abs());
                // Cloudy spells are a fairly rare event — the default sky is
                // the sparse "forced clear" look; weather is the exception.
                let cloudy_chance = lerp_f32(0.12, 0.22, dawn_dusk);

                let pick = hash_to_unit(seed, segment as u64);
                let roll = hash_to_unit(seed ^ 0x9E37_79B9_7F4A_7C15, segment as u64);

                if pick < cloudy_chance {
                    cover.target =
                        lerp_f32(CLOUD_COVER_CLOUDY_RANGE.0, CLOUD_COVER_CLOUDY_RANGE.1, roll);
                    // A cloudy spell sometimes carries a storm system.
                    let storm_roll = hash_to_unit(seed ^ 0x5708_11ED, segment as u64);
                    if storm_roll < 0.35 {
                        cover.storm_target =
                            0.6 + 0.4 * hash_to_unit(seed ^ 0x5708_22EE, segment as u64);
                        // Storm sky: broken cover between the cells, not a blanket.
                        cover.target =
                            lerp_f32(CLOUD_COVER_STORM_BASE.0, CLOUD_COVER_STORM_BASE.1, roll);
                    } else {
                        cover.storm_target = 0.0;
                    }
                } else {
                    cover.target =
                        lerp_f32(CLOUD_COVER_CLEAR_RANGE.0, CLOUD_COVER_CLEAR_RANGE.1, roll);
                    cover.storm_target = 0.0;
                };
            }
        }
    }

    let dt = time.delta_secs().max(0.0);
    let blend = 1.0 - (-CLOUD_COVER_LERP_SPEED * dt).exp();
    cover.current = lerp_f32(cover.current, cover.target, blend).clamp(0.0, 1.0);
    cover.storminess = lerp_f32(cover.storminess, cover.storm_target, blend).clamp(0.0, 1.0);
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
        &mut Visibility,
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

    // Physical sun phase, not display time (the display clock runs summer
    // hours and no longer tracks the sun's true position).
    let elevation = world_time_query
        .iter()
        .next()
        .map(|wt| -wt.sun_phase().cos())
        .unwrap_or(1.0);
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

    for (mut layer, material_handle, mut transform, mut layer_visibility) in layers.iter_mut() {
        // Alpha 0 still rasterizes: the camera sits INSIDE this sphere, so at
        // map zoom (visibility exactly 0) it was a fullscreen transparent
        // draw at Retina resolution doing nothing. Hide it outright.
        let target_visibility = if visibility < 0.004 {
            Visibility::Hidden
        } else {
            Visibility::Inherited
        };
        if *layer_visibility != target_visibility {
            *layer_visibility = target_visibility;
        }
        if target_visibility == Visibility::Hidden {
            continue;
        }

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

#[cfg(test)]
mod storm_tests {
    use super::*;

    /// The storm must always be ON the map (FORCE STORM shows a storm now,
    /// not in twenty minutes), and the track must be continuous — the
    /// reflect fold must never jump the center between nearby timestamps.
    #[test]
    fn storm_center_stays_on_map_and_moves_continuously() {
        let seed_phase = hash_to_unit(7, 0) * 37.0;
        let half = 4096.0;
        let mut prev = storm_center(seed_phase, 0.0, half);
        println!("capture-seed storm center @225s: {:?}", storm_center(seed_phase, 225.0, half));
        for i in 1..20_000 {
            let t = i as f32 * 1.0;
            let c = storm_center(seed_phase, t, half);
            assert!(c.x.abs() <= half && c.y.abs() <= half, "off map at {t}: {c:?}");
            assert!(c.distance(prev) < 8.0, "teleport at {t}: {prev:?} -> {c:?}");
            prev = c;
        }
    }
}
