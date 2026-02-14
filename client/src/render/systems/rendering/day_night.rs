//! day night systems.

use super::atmosphere::AtmospherePreset;
use super::clouds::CLOUD_SUN_YAW_OFFSET;
use super::*;

/// Update sun, ambient light, and sky color based on replicated WorldTime.
/// Creates beautiful sunrise/sunset transitions.
pub fn update_day_night_cycle(
    world_time_query: Query<&shared::components::WorldTime>,
    mut sun_query: Query<
        (&mut DirectionalLight, &mut Transform),
        (With<SunLight>, Without<FillLight>, Without<Camera3d>),
    >,
    mut fill_query: Query<
        (&mut DirectionalLight, &mut Transform),
        (With<FillLight>, Without<SunLight>, Without<Camera3d>),
    >,
    mut ambient: ResMut<GlobalAmbientLight>,
    settings: Res<GraphicsSettings>,
    time: Res<Time>,
    mut debug_timer: Local<f32>,
) {
    *debug_timer += time.delta_secs();
    let should_log = *debug_timer > 3.0;
    if should_log {
        *debug_timer = 0.0;
    }

    // Get replicated world time (spawned by server)
    let world_time = match world_time_query.iter().next() {
        Some(wt) => wt,
        None => {
            if should_log {
                info!("Day/Night: Waiting for WorldTime from server...");
            }
            return;
        }
    };

    // Normalized time: 0.0 = midnight, 0.25 = sunrise, 0.5 = noon, 0.75 = sunset
    let t = world_time.normalized_time();

    // =========================================================================
    // SUN POSITION - Rotates around the world (rises in east, sets in west)
    // =========================================================================
    // IMPORTANT: we want elevation = -1 at midnight, 0 at sunrise/sunset, +1 at noon.
    let phase = t * std::f32::consts::TAU;
    let elevation = -phase.cos(); // [-1, 1]

    // Sunrise at t=0.25 should come from +X (east) -> rays point toward -X.
    let azimuth = phase - std::f32::consts::PI;

    // Convert elevation factor to an angle. Clamp so the sun doesn't go *perfectly* overhead.
    let elev_angle = elevation.clamp(-1.0, 1.0) * std::f32::consts::FRAC_PI_2 * 0.9;
    let cos_e = elev_angle.cos();
    let sin_e = elev_angle.sin();

    // Direction the light rays travel (from sun -> world). y is negative when sun is above horizon.
    let sun_dir =
        Vec3::new(azimuth.sin() * cos_e, -sin_e, azimuth.cos() * cos_e).normalize_or_zero();

    // =========================================================================
    // SUN INTENSITY
    // =========================================================================
    // Atmosphere handles the sky tinting; keep the directional light physically plausible.
    // "Sun height" factor: 0 at horizon, 1 at noon
    let sun_height = elevation.max(0.0);

    // Day factor: 1 during day, 0 at night
    let day_factor = smoothstep(-0.05, 0.15, elevation);

    // Sun intensity: RAW_SUNLIGHT at noon, fades out smoothly toward night.
    let sun_illuminance = lux::RAW_SUNLIGHT * sun_height.powf(0.6) * day_factor;
    // Warm tint only near sunrise/sunset (keeps midday neutral).
    let dust_factor = 1.0 - smoothstep(0.15, 0.65, sun_height);
    let warm_sun_color = Color::srgb(1.0, 0.92, 0.82);
    let neutral_sun_color = Color::WHITE;
    let sun_color = lerp_color(neutral_sun_color, warm_sun_color, dust_factor);

    // Bevy directional light points along -Z (forward). Rotate -Z to match sun_dir.
    for (mut sun_light, mut sun_transform) in sun_query.iter_mut() {
        sun_transform.rotation = Quat::from_rotation_arc(Vec3::NEG_Z, sun_dir);
        sun_light.color = sun_color;
        sun_light.illuminance = sun_illuminance;
    }

    // =========================================================================
    // FILL LIGHT - Lift shadows for readability
    // =========================================================================
    let lighting_boost = settings.lighting_boost.clamp(0.5, 4.0);
    let fill_illuminance = lerp_f32(200.0, 3200.0, day_factor) * lighting_boost;
    let fill_dir = Vec3::new(-sun_dir.x * 0.25, -0.7, -sun_dir.z * 0.25).normalize_or_zero();
    for (mut fill_light, mut fill_transform) in fill_query.iter_mut() {
        fill_transform.rotation = Quat::from_rotation_arc(Vec3::NEG_Z, fill_dir);
        fill_light.color = Color::WHITE;
        fill_light.illuminance = fill_illuminance;
    }

    // =========================================================================
    // AMBIENT LIGHT - Desert environment: warm during day, cool blue at night
    // =========================================================================
    // With `AtmosphereEnvironmentMapLight`, most ambient should come from the atmosphere-driven IBL.
    // Boost brightness significantly for harsh desert daylight.
    let day_ambient_neutral = Color::srgb(0.75, 0.82, 0.92); // Neutral daylight (bluer sky tint)
    let day_ambient_warm = Color::srgb(0.9, 0.85, 0.75); // Warm sandy tones near horizon
    let day_ambient_color = lerp_color(day_ambient_neutral, day_ambient_warm, dust_factor);
    let night_ambient_color = Color::srgb(0.12, 0.16, 0.25); // Cooler blue night
    ambient.color = lerp_color(night_ambient_color, day_ambient_color, day_factor);
    // Night ambient is now 16 lux (up from 1) so you can actually see
    // Day ambient peaks at 80 lux to keep terrain curves readable
    let lighting_boost = settings.lighting_boost.clamp(0.5, 4.0);
    ambient.brightness = (16.0 + 64.0 * day_factor) * lighting_boost;
}

/// Helper to linearly interpolate between two colors
pub(super) fn lerp_color(a: Color, b: Color, t: f32) -> Color {
    let a_rgba = a.to_srgba();
    let b_rgba = b.to_srgba();
    Color::srgba(
        a_rgba.red + (b_rgba.red - a_rgba.red) * t,
        a_rgba.green + (b_rgba.green - a_rgba.green) * t,
        a_rgba.blue + (b_rgba.blue - a_rgba.blue) * t,
        a_rgba.alpha + (b_rgba.alpha - a_rgba.alpha) * t,
    )
}

pub(super) fn lerp_f32(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

pub(super) fn lerp_vec3(a: Vec3, b: Vec3, t: f32) -> Vec3 {
    a + (b - a) * t
}

pub(super) fn blend_atmosphere(
    clear: AtmospherePreset,
    dusty: AtmospherePreset,
    t: f32,
) -> AtmospherePreset {
    AtmospherePreset {
        bottom_radius: lerp_f32(clear.bottom_radius, dusty.bottom_radius, t),
        top_radius: lerp_f32(clear.top_radius, dusty.top_radius, t),
        ground_albedo: lerp_vec3(clear.ground_albedo, dusty.ground_albedo, t),
        rayleigh_density_exp_scale: lerp_f32(
            clear.rayleigh_density_exp_scale,
            dusty.rayleigh_density_exp_scale,
            t,
        ),
        rayleigh_scattering: lerp_vec3(clear.rayleigh_scattering, dusty.rayleigh_scattering, t),
        mie_density_exp_scale: lerp_f32(
            clear.mie_density_exp_scale,
            dusty.mie_density_exp_scale,
            t,
        ),
        mie_scattering: lerp_f32(clear.mie_scattering, dusty.mie_scattering, t),
        mie_absorption: lerp_f32(clear.mie_absorption, dusty.mie_absorption, t),
        mie_asymmetry: lerp_f32(clear.mie_asymmetry, dusty.mie_asymmetry, t),
        ozone_layer_altitude: lerp_f32(clear.ozone_layer_altitude, dusty.ozone_layer_altitude, t),
        ozone_layer_width: lerp_f32(clear.ozone_layer_width, dusty.ozone_layer_width, t),
        ozone_absorption: lerp_vec3(clear.ozone_absorption, dusty.ozone_absorption, t),
    }
}

pub(super) fn smoothstep(edge0: f32, edge1: f32, x: f32) -> f32 {
    let t = ((x - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

pub(super) fn sun_yaw_from_phase(phase: f32) -> f32 {
    // Same azimuth logic as update_day_night_cycle (sunrise at +X).
    let azimuth = phase - std::f32::consts::PI;
    // Sunlight direction (sun -> world); rotate sky toward actual sun position.
    let sun_dir = Vec3::new(azimuth.sin(), 0.0, azimuth.cos()).normalize_or_zero();
    sun_dir.x.atan2(sun_dir.z) + CLOUD_SUN_YAW_OFFSET
}
