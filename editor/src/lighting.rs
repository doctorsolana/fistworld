use bevy::prelude::*;

use crate::session::EditorEnvironmentState;

#[derive(Component)]
pub struct EditorSunLight;

#[derive(Component)]
pub struct EditorFillLight;

const DAY_MIN_ELEV_DEGREES: f32 = 18.0;
const DAY_MAX_ELEV_DEGREES: f32 = 38.0;
const NIGHT_ELEV_DEGREES: f32 = 12.0;

pub fn apply_editor_environment(
    mut env_state: ResMut<EditorEnvironmentState>,
    mut sun_query: Query<
        (&mut DirectionalLight, &mut Transform),
        (With<EditorSunLight>, Without<EditorFillLight>),
    >,
    mut fill_query: Query<
        (&mut DirectionalLight, &mut Transform),
        (With<EditorFillLight>, Without<EditorSunLight>),
    >,
    mut ambient: ResMut<GlobalAmbientLight>,
) {
    let Ok((mut sun_light, mut sun_transform)) = sun_query.single_mut() else {
        return;
    };
    let Ok((mut fill_light, mut fill_transform)) = fill_query.single_mut() else {
        return;
    };

    let clamped_day = env_state.day_time_hours.clamp(0.0, 24.0);
    if (clamped_day - env_state.day_time_hours).abs() > f32::EPSILON {
        env_state.day_time_hours = clamped_day;
    }

    let phase = (clamped_day / 24.0) * std::f32::consts::TAU;
    let elevation = -phase.cos();
    let azimuth = phase - std::f32::consts::PI;
    let sun_height = elevation.max(0.0);
    let day_factor = smoothstep(-0.08, 0.10, elevation);
    let dust_factor = 1.0 - smoothstep(0.15, 0.65, sun_height);

    let daylight_elevation = lerp_f32(
        DAY_MIN_ELEV_DEGREES,
        DAY_MAX_ELEV_DEGREES,
        sun_height.powf(0.7),
    )
    .to_radians();
    let signed_elevation = if elevation >= 0.0 {
        daylight_elevation
    } else {
        -NIGHT_ELEV_DEGREES.to_radians()
    };
    let sun_dir = directional_light_vector(azimuth, signed_elevation);

    let fill_yaw = azimuth + 0.9;
    let fill_elevation =
        lerp_f32(26.0, 42.0, (1.0 - sun_height).clamp(0.0, 1.0) * 0.6 + 0.4).to_radians();
    let fill_dir = directional_light_vector(fill_yaw, fill_elevation);

    sun_transform.rotation = Quat::from_rotation_arc(Vec3::NEG_Z, sun_dir);
    sun_light.color = lerp_color(Color::WHITE, Color::srgb(1.0, 0.93, 0.84), dust_factor);
    sun_light.illuminance = lerp_f32(1_500.0, 24_000.0, sun_height.powf(0.65)) * day_factor;
    sun_light.shadows_enabled = day_factor > 0.02;

    fill_transform.rotation = Quat::from_rotation_arc(Vec3::NEG_Z, fill_dir);
    fill_light.color = lerp_color(
        Color::srgb(0.72, 0.80, 0.92),
        Color::srgb(0.95, 0.92, 0.84),
        dust_factor * 0.35,
    );
    fill_light.illuminance = lerp_f32(180.0, 1_600.0, day_factor);

    ambient.color = lerp_color(
        Color::srgb(0.10, 0.13, 0.19),
        lerp_color(
            Color::srgb(0.62, 0.72, 0.82),
            Color::srgb(0.88, 0.84, 0.76),
            dust_factor,
        ),
        day_factor,
    );
    ambient.brightness = lerp_f32(4.0, 16.0, day_factor);
}

fn directional_light_vector(azimuth: f32, elevation: f32) -> Vec3 {
    let cos_e = elevation.cos();
    let sin_e = elevation.sin();
    Vec3::new(azimuth.sin() * cos_e, -sin_e, azimuth.cos() * cos_e).normalize_or_zero()
}

fn smoothstep(edge0: f32, edge1: f32, value: f32) -> f32 {
    let t = ((value - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn lerp_f32(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn lerp_color(a: Color, b: Color, t: f32) -> Color {
    let a_rgba = a.to_srgba();
    let b_rgba = b.to_srgba();
    Color::srgba(
        a_rgba.red + (b_rgba.red - a_rgba.red) * t,
        a_rgba.green + (b_rgba.green - a_rgba.green) * t,
        a_rgba.blue + (b_rgba.blue - a_rgba.blue) * t,
        a_rgba.alpha + (b_rgba.alpha - a_rgba.alpha) * t,
    )
}
