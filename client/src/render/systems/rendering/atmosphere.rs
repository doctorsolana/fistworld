//! atmosphere systems.

use super::day_night::{blend_atmosphere, smoothstep};
use super::*;

#[derive(Clone, Copy)]
pub(super) struct AtmospherePreset {
    pub(super) bottom_radius: f32,
    pub(super) top_radius: f32,
    pub(super) ground_albedo: Vec3,
    pub(super) rayleigh_density_exp_scale: f32,
    pub(super) rayleigh_scattering: Vec3,
    pub(super) mie_density_exp_scale: f32,
    pub(super) mie_scattering: f32,
    pub(super) mie_absorption: f32,
    pub(super) mie_asymmetry: f32,
    pub(super) ozone_layer_altitude: f32,
    pub(super) ozone_layer_width: f32,
    pub(super) ozone_absorption: Vec3,
}

#[derive(Resource)]
pub(crate) struct AtmosphereMedia {
    pub(super) clear: AtmospherePreset,
    pub(super) dusty: AtmospherePreset,
    pub(super) active_medium: Handle<ScatteringMedium>,
    pub(super) last_blend: f32,
}

/// Desert atmosphere preset - more dust, redder sunsets, brighter sand
/// Based on Earth but with increased Mie scattering for that UAE/Arabian desert look
pub(super) fn desert_atmosphere_preset() -> AtmospherePreset {
    AtmospherePreset {
        // Same planet dimensions as Earth
        bottom_radius: 6_360_000.0,
        top_radius: 6_460_000.0,
        // Sandy desert surface is more reflective (warm tones)
        ground_albedo: Vec3::new(0.45, 0.38, 0.28),
        // Standard Rayleigh (blue sky) - slightly reduced for drier desert air
        rayleigh_density_exp_scale: 1.0 / 8_500.0,
        rayleigh_scattering: Vec3::new(5.0e-6, 12.0e-6, 28.0e-6),
        // INCREASED Mie scattering - desert dust! This is key for red sunsets
        mie_density_exp_scale: 1.0 / 800.0, // Dust extends higher
        mie_scattering: 12.0e-6,            // 3x Earth's default - more dust particles
        mie_absorption: 1.5e-6,             // Slightly more absorption too
        mie_asymmetry: 0.75,                // Slightly more isotropic scattering
        // Less ozone for desert environment
        ozone_layer_altitude: 25_000.0,
        ozone_layer_width: 30_000.0,
        ozone_absorption: Vec3::new(0.5e-6, 1.5e-6, 0.07e-6),
    }
}

/// Clear daytime atmosphere - less dust for bluer midday skies.
pub(super) fn clear_atmosphere_preset() -> AtmospherePreset {
    AtmospherePreset {
        bottom_radius: 6_360_000.0,
        top_radius: 6_460_000.0,
        ground_albedo: Vec3::new(0.35, 0.4, 0.45),
        rayleigh_density_exp_scale: 1.0 / 8_000.0,
        rayleigh_scattering: Vec3::new(5.8e-6, 13.6e-6, 33.1e-6),
        mie_density_exp_scale: 1.0 / 1_200.0,
        mie_scattering: 3.2e-6,
        mie_absorption: 0.3e-6,
        mie_asymmetry: 0.8,
        ozone_layer_altitude: 25_000.0,
        ozone_layer_width: 30_000.0,
        ozone_absorption: Vec3::new(0.65e-6, 1.88e-6, 0.085e-6),
    }
}

fn atmosphere_height(preset: &AtmospherePreset) -> f32 {
    (preset.top_radius - preset.bottom_radius).max(1.0)
}

fn exp_falloff_scale(density_exp_scale: f32, height: f32) -> f32 {
    if density_exp_scale <= 0.0 || !density_exp_scale.is_finite() {
        return 1.0;
    }
    (1.0 / density_exp_scale) / height
}

pub(super) fn scattering_medium_from_preset(
    preset: AtmospherePreset,
    label: &'static str,
) -> ScatteringMedium {
    let height = atmosphere_height(&preset);
    let rayleigh_scale = exp_falloff_scale(preset.rayleigh_density_exp_scale, height);
    let mie_scale = exp_falloff_scale(preset.mie_density_exp_scale, height);
    let ozone_center = (preset.ozone_layer_altitude / height).clamp(0.0, 1.0);
    let ozone_width = (preset.ozone_layer_width / height).clamp(0.0, 1.0);

    ScatteringMedium::new(
        256,
        256,
        [
            ScatteringTerm {
                absorption: Vec3::ZERO,
                scattering: preset.rayleigh_scattering,
                falloff: Falloff::Exponential {
                    scale: rayleigh_scale,
                },
                phase: PhaseFunction::Rayleigh,
            },
            ScatteringTerm {
                absorption: Vec3::splat(preset.mie_absorption),
                scattering: Vec3::splat(preset.mie_scattering),
                falloff: Falloff::Exponential { scale: mie_scale },
                phase: PhaseFunction::Mie {
                    asymmetry: preset.mie_asymmetry,
                },
            },
            ScatteringTerm {
                absorption: preset.ozone_absorption,
                scattering: Vec3::ZERO,
                falloff: Falloff::Tent {
                    center: ozone_center,
                    width: ozone_width,
                },
                phase: PhaseFunction::Isotropic,
            },
        ],
    )
    .with_label(label)
}

/// Atmosphere LUT settings tuned for *realtime gameplay*.
///
/// Bevy's defaults look great but can be expensive:
/// - sky_view_lut_size default: 400x200
/// - aerial_view_lut_size default: 32^3
///
/// Lowering these tends to recover a lot of FPS, especially on integrated GPUs.
pub(super) fn desert_atmosphere_settings_perf() -> AtmosphereSettings {
    AtmosphereSettings {
        // Global LUTs (aggressively reduced for performance)
        transmittance_lut_size: UVec2::new(128, 64),
        transmittance_lut_samples: 20,
        multiscattering_lut_size: UVec2::new(16, 16),
        multiscattering_lut_dirs: 32,
        multiscattering_lut_samples: 10,

        // View-dependent LUTs (big wins - reduced further)
        sky_view_lut_size: UVec2::new(192, 96),
        sky_view_lut_samples: 8,
        aerial_view_lut_size: UVec3::new(16, 16, 16),
        aerial_view_lut_samples: 4,
        // Smaller max distance - don't need aerial perspective past 10km
        aerial_view_lut_max_distance: 1.0e4,

        // 1 unit = 1 meter in our world
        scene_units_to_m: 1.0,

        // Fallback cap used in some paths
        sky_max_samples: 8,

        rendering_method: AtmosphereMode::LookupTexture,
    }
}

pub(super) fn default_bloom_settings() -> Bloom {
    Bloom {
        intensity: 0.12,
        low_frequency_boost: 0.42,
        low_frequency_boost_curvature: 0.55,
        high_pass_frequency: 0.92,
        composite_mode: BloomCompositeMode::Additive,
        ..default()
    }
}

/// Blend atmosphere for clear midday skies and dusty sunsets.
pub fn update_atmosphere(
    world_time_query: Query<&shared::components::WorldTime>,
    mut atmosphere_query: Query<&mut Atmosphere, With<Camera3d>>,
    mut media_assets: ResMut<Assets<ScatteringMedium>>,
    mut media: ResMut<AtmosphereMedia>,
    time: Res<Time>,
    mut rebuild_timer: Local<f32>,
) {
    let Some(world_time) = world_time_query.iter().next() else {
        return;
    };
    let mut atmosphere = match atmosphere_query.single_mut() {
        Ok(atm) => atm,
        Err(_) => return,
    };

    let t = world_time.normalized_time();
    let phase = t * std::f32::consts::TAU;
    let elevation = -phase.cos(); // [-1, 1]
    let sun_height = elevation.max(0.0);

    // Dusty near horizon, clear at noon.
    let dust_factor = 1.0 - smoothstep(0.15, 0.65, sun_height);

    let blended = blend_atmosphere(media.clear, media.dusty, dust_factor);
    atmosphere.bottom_radius = blended.bottom_radius;
    atmosphere.top_radius = blended.top_radius;
    atmosphere.ground_albedo = blended.ground_albedo;
    atmosphere.medium = media.active_medium.clone();

    const ATMOSPHERE_REBUILD_INTERVAL: f32 = 0.35;
    const ATMOSPHERE_BLEND_EPS: f32 = 0.03;

    *rebuild_timer += time.delta_secs().max(0.0);
    let needs_rebuild = (media.last_blend - dust_factor).abs() > ATMOSPHERE_BLEND_EPS;
    let allow_rebuild = *rebuild_timer >= ATMOSPHERE_REBUILD_INTERVAL || media.last_blend < 0.0;

    if needs_rebuild && allow_rebuild {
        if let Some(medium) = media_assets.get_mut(&media.active_medium) {
            *medium = scattering_medium_from_preset(blended, "dynamic_atmosphere");
        }
        media.last_blend = dust_factor;
        *rebuild_timer = 0.0;
    }
}
