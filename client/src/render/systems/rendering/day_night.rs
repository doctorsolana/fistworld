//! day night systems.

use super::atmosphere::AtmospherePreset;
use super::*;
use crate::terrain::map_view::MapViewBlend;

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
    mut fog_query: Query<&mut DistanceFog, With<Camera3d>>,
    mut atmosphere_settings_query: Query<&mut AtmosphereSettings, With<Camera3d>>,
    mut atmosphere_light_query: Query<&mut AtmosphereEnvironmentMapLight, With<Camera3d>>,
    mut ambient: ResMut<GlobalAmbientLight>,
    settings: Res<GraphicsSettings>,
    map_blend: Res<MapViewBlend>,
    time: Res<Time>,
    mut debug_timer: Local<f32>,
    mut last_fog_key: Local<Option<(f32, f32, f32, f32)>>,
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

    // =========================================================================
    // SUN POSITION - East-west great-circle arc (rises in east, sets in west)
    // =========================================================================
    // IMPORTANT: we want elevation = -1 at midnight, 0 at sunrise/sunset, +1 at noon.
    // Cycle-fraction phase, NOT display time: the display clock runs summer
    // hours (sunset 20:00) and must never steer the physical sun.
    let phase = world_time.sun_phase();
    let elevation = -phase.cos(); // [-1, 1]

    // Arc angle: 0 = sunrise, PI/2 = noon, PI = sunset; below horizon at night.
    let a = phase - std::f32::consts::FRAC_PI_2;

    // Cap the max elevation at ~61°: a near-overhead noon sun (the old 81°)
    // lights the world like an operating theater — rooftops blasted, faces and
    // walls in the dark. A capped sun keeps direction and modeling on vertical
    // surfaces all day (this is what most outdoor games do; real midday sun
    // rarely exceeds ~60-70° at temperate latitudes anyway).
    let max_elev = std::f32::consts::FRAC_PI_2 * 0.68;
    let noon_dir = Vec3::new(0.0, max_elev.sin(), -max_elev.cos());

    // Unit great circle from +X (east horizon) through noon_dir (high noon,
    // bowed toward -Z) to -X (west horizon): the sun's bearing swings E->N->W
    // across daylight instead of spinning the full compass. sin(a) ==
    // elevation, so every intensity curve keyed on `elevation` behaves exactly
    // as before the arc.
    let sun_pos = a.cos() * Vec3::X + a.sin() * noon_dir;

    // Direction the light rays travel (from sun -> world). y is negative when sun is above horizon.
    let sun_dir = -sun_pos;

    // =========================================================================
    // SUN INTENSITY
    // =========================================================================
    // Atmosphere handles the sky tinting; keep the directional light physically plausible.
    // "Sun height" factor: 0 at horizon, 1 at noon
    let sun_height = elevation.max(0.0);

    // Day factor: 1 during day, 0 at night
    let day_factor = smoothstep(-0.05, 0.15, elevation);

    // Twilight needs its own lift: the sun is low enough to contribute little direct light,
    // but the sky should still bounce a readable amount of warm/cool ambient light.
    let twilight_factor =
        (1.0 - smoothstep(0.02, 0.38, sun_height)) * smoothstep(-0.22, 0.04, elevation);

    // Sun intensity: DIRECT_SUNLIGHT at noon (RAW_SUNLIGHT is unfiltered
    // top-of-atmosphere intensity — clinically harsh under AgX), fading
    // smoothly toward night.
    let sun_illuminance = lux::DIRECT_SUNLIGHT * sun_height.powf(0.6) * day_factor;
    // Keep daylight gently warm, then push low sun into a stronger golden tint.
    let dust_factor = 1.0 - smoothstep(0.15, 0.65, sun_height);
    let warm_sun_color = Color::srgb(1.0, 0.84, 0.62);
    let neutral_sun_color = Color::srgb(1.0, 0.97, 0.90);
    let sun_color = lerp_color(neutral_sun_color, warm_sun_color, dust_factor);

    // Bevy directional light points along -Z (forward). Rotate -Z to match sun_dir.
    let sun_rotation = Quat::from_rotation_arc(Vec3::NEG_Z, sun_dir);
    for (mut sun_light, mut sun_transform) in sun_query.iter_mut() {
        if sun_transform.rotation != sun_rotation {
            sun_transform.rotation = sun_rotation;
        }
        if sun_light.color != sun_color || sun_light.illuminance != sun_illuminance {
            sun_light.color = sun_color;
            sun_light.illuminance = sun_illuminance;
        }
    }

    // =========================================================================
    // FILL LIGHT - Sky/ground bounce approximation
    // =========================================================================
    // The camera uses physical sunlight exposure (EV ~15), where the sun is
    // ~100k lux. A fill in the hundreds of lux is invisible at that exposure;
    // real sky+bounce contributes 10-20% of the sun. Keep the fill low-angle
    // and opposite the sun so it lights the faces/walls the sun misses.
    let lighting_boost = settings.lighting_boost.clamp(0.5, 4.0);
    // At night the fill BECOMES the moon: the RTS night convention is a dim
    // cool key light (~2-3% of the sun) so shapes still model instead of
    // crushing to silhouette. It rises as the sun drops well below horizon.
    let moon_factor = smoothstep(0.05, 0.35, -elevation);
    let fill_illuminance =
        (lerp_f32(60.0, 11_000.0, day_factor) + 900.0 * twilight_factor + 9_000.0 * moon_factor)
            * lighting_boost;
    // The moon rides the antipode of the sun's arc: while the sun is below
    // the horizon, -sun_dir points down from an elevated moon.
    let day_fill_dir = Vec3::new(-sun_dir.x, -0.35, -sun_dir.z).normalize_or_zero();
    let fill_dir = day_fill_dir.lerp(-sun_dir, moon_factor).normalize_or_zero();
    let fill_rotation = Quat::from_rotation_arc(Vec3::NEG_Z, fill_dir);
    let fill_color = Color::srgb(0.60, 0.72, 0.95);
    for (mut fill_light, mut fill_transform) in fill_query.iter_mut() {
        if fill_transform.rotation != fill_rotation {
            fill_transform.rotation = fill_rotation;
        }
        if fill_light.color != fill_color || fill_light.illuminance != fill_illuminance {
            fill_light.color = fill_color;
            fill_light.illuminance = fill_illuminance;
        }
    }

    // =========================================================================
    // AMBIENT LIGHT - Desert environment: warm during day, cool blue at night
    // =========================================================================
    // With `AtmosphereEnvironmentMapLight`, most ambient should come from the atmosphere-driven IBL.
    // Keep ambient lower than the key sun so forms keep readable shadow shape.
    let day_ambient_neutral = Color::srgb(0.46, 0.55, 0.68);
    let day_ambient_warm = Color::srgb(0.68, 0.64, 0.56);
    let day_ambient_color = lerp_color(day_ambient_neutral, day_ambient_warm, dust_factor);
    let twilight_ambient_color = Color::srgb(0.50, 0.49, 0.45);
    // A real blue, not near-black: night reads as COLOR SHIFT (moonlit blue,
    // desaturated), never as actual darkness — the whole map must stay
    // playable at midnight.
    let night_ambient_color = Color::srgb(0.38, 0.47, 0.72);
    let base_ambient_color = lerp_color(night_ambient_color, day_ambient_color, day_factor);
    let ambient_color = lerp_color(
        base_ambient_color,
        twilight_ambient_color,
        twilight_factor * 0.55,
    );
    // At physical sunlight exposure a mid-gray needs thousands of lux to
    // register; the old ~39 peak was ~1% of that (pitch-black shadows, the
    // "hospital light" contrast). ~2600 at noon puts shadowed sides at a
    // readable ~1:4 ratio against sunlit surfaces, like a clear real sky.
    // Night ambient runs HIGHER than day's (the moon+ambient must carry the
    // whole scene at the fixed daylight exposure — physical moonlight would
    // render black). The blue color keeps it reading as night. Day term is
    // unchanged: lerp hits the old 2610 at day_factor 1.
    let ambient_brightness =
        (lerp_f32(3_600.0, 2_610.0, day_factor) + 600.0 * twilight_factor) * lighting_boost;
    if ambient.color != ambient_color || ambient.brightness != ambient_brightness {
        ambient.color = ambient_color;
        ambient.brightness = ambient_brightness;
    }

    let atmosphere_intensity = if settings.atmosphere_enabled {
        (1.4 + 0.45 * twilight_factor) * lighting_boost
    } else {
        0.0
    };
    for mut atmosphere_light in atmosphere_light_query.iter_mut() {
        if atmosphere_light.intensity != atmosphere_intensity {
            atmosphere_light.intensity = atmosphere_intensity;
        }
    }

    // Single writer for DistanceFog: the map-view fade is folded in here rather
    // than living in a second unordered system racing over the same component.
    // Aerial haze sells depth at ground level, but at map scale it is
    // integrated over kilometres and turns the whole map into a grey-brown
    // wash; dial it out (alpha down, visibility up) exactly as the detail
    // chunks fade so the map stays legible.
    let map_clear = 1.0 - map_blend.0;
    // Fog is a pure function of these four factors; skip the write (and its
    // change-detection ripple) while they sit on their plateaus.
    let fog_key = (day_factor, dust_factor, twilight_factor, map_clear);
    if *last_fog_key != Some(fog_key) {
        *last_fog_key = Some(fog_key);

        // Physical aerial perspective integrates kilometres of air at map zoom and
        // shrouds the whole map (the LUT saturates at its max distance, so lowering
        // that distance caps the veil). Ground level keeps the full 10 km depth cue;
        // map view clamps it right down, mirroring the DistanceFog fade below.
        let aerial_max = lerp_f32(10_000.0, 1_500.0, map_blend.0);
        for mut atmo_settings in atmosphere_settings_query.iter_mut() {
            if atmo_settings.aerial_view_lut_max_distance != aerial_max {
                atmo_settings.aerial_view_lut_max_distance = aerial_max;
            }
        }

        let (clear_fog_color, dusty_fog_color, night_fog_color) = (
            Color::srgba(0.50, 0.60, 0.70, 0.10),
            Color::srgba(0.72, 0.67, 0.56, 0.20),
            Color::srgba(0.05, 0.08, 0.14, 0.26),
        );
        let day_fog_color = lerp_color(clear_fog_color, dusty_fog_color, dust_factor);
        let mut fog_color = lerp_color(night_fog_color, day_fog_color, day_factor);
        fog_color.set_alpha(fog_color.alpha() * map_clear);
        let visibility = (lerp_f32(320.0, 1050.0, day_factor) * lerp_f32(1.0, 0.68, dust_factor))
            .max(520.0 * twilight_factor);
        let fog_light_factor = (day_factor * dust_factor + 0.35 * twilight_factor).clamp(0.0, 1.0);
        let fog_sky_factor = (day_factor + 0.35 * twilight_factor).clamp(0.0, 1.0);
        let extinction = lerp_color(
            Color::srgb(0.26, 0.34, 0.48),
            Color::srgb(0.56, 0.54, 0.48),
            fog_light_factor,
        );
        let inscattering = lerp_color(
            Color::srgb(0.12, 0.16, 0.27),
            Color::srgb(0.82, 0.78, 0.68),
            fog_sky_factor,
        );
        let mut directional_light_color = lerp_color(
            Color::srgba(0.08, 0.12, 0.2, 0.04),
            Color::srgba(1.0, 0.86, 0.62, 0.24),
            fog_light_factor,
        );
        directional_light_color.set_alpha(directional_light_color.alpha() * map_clear);

        for mut fog in fog_query.iter_mut() {
            fog.color = fog_color;
            fog.directional_light_color = directional_light_color;
            fog.directional_light_exponent = lerp_f32(18.0, 34.0, day_factor);
            fog.falloff = FogFalloff::from_visibility_colors(
                visibility.max(80.0) / map_clear.max(0.02),
                extinction,
                inscattering,
            );
        }
    }
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
