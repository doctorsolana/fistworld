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
        // Earth-like 3.2e-6 read as a milky gray-tan wash over the whole sky
        // under AgX; thinner haze lets the Rayleigh blue through outside the
        // dusty golden hours.
        mie_scattering: 2.0e-6,
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

/// Make the atmosphere toggle honest: OFF despawns the Atmosphere entity and
/// strips the camera's AtmosphereSettings, removing the sky/LUT cost entirely
/// (the old toggle only zeroed the IBL, darkening the scene while still paying
/// full price). ON restores both from the stored presets.
pub(crate) fn sync_atmosphere_enabled(
    mut commands: Commands,
    settings: Res<GraphicsSettings>,
    media: Res<AtmosphereMedia>,
    atmospheres: Query<Entity, With<Atmosphere>>,
    cameras: Query<(Entity, Has<AtmosphereSettings>), With<Camera3d>>,
) {
    if !settings.is_changed() {
        return;
    }
    if settings.atmosphere_enabled {
        if atmospheres.is_empty() {
            let clear = media.clear;
            commands.spawn((
                Atmosphere {
                    inner_radius: clear.bottom_radius,
                    outer_radius: clear.top_radius,
                    ground_albedo: clear.ground_albedo,
                    medium: media.active_medium.clone(),
                },
                // Planet center one bottom_radius down — see setup.rs.
                Transform::from_xyz(0.0, -clear.bottom_radius, 0.0),
            ));
        }
        for (camera, has_settings) in cameras.iter() {
            if !has_settings {
                commands
                    .entity(camera)
                    .insert(desert_atmosphere_settings_perf());
            }
        }
    } else {
        for entity in atmospheres.iter() {
            commands.entity(entity).despawn();
        }
        for (camera, has_settings) in cameras.iter() {
            if has_settings {
                commands.entity(camera).remove::<AtmosphereSettings>();
            }
        }
    }
}

/// Blend atmosphere for clear midday skies and dusty sunsets.
pub(crate) fn update_atmosphere(
    world_time_query: Query<&shared::components::WorldTime>,
    // The atmosphere is its own entity since bevy 0.19, no longer on the camera.
    mut atmosphere_query: Query<&mut Atmosphere>,
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

    // Physical sun phase, not the display clock (which runs summer hours).
    let elevation = -world_time.sun_phase().cos(); // [-1, 1]
    let sun_height = elevation.max(0.0);

    // Dusty near the horizon, clear at noon — and clear again at NIGHT.
    // sun_height clamps to 0 below the horizon, so without the night gate the
    // dusty preset sat at FULL blend all night and its strong Mie forward
    // scatter painted a fake amber sunset band on the horizon at midnight.
    // Fade the dust out as the sun sinks below the horizon; deep night
    // reverts to the clear preset's dark blue twilight.
    let night_dust_fade = 1.0 - smoothstep(0.06, 0.22, (-elevation).max(0.0));
    // 0.12..0.42: golden hour is roughly sunrise+2h / sunset-2h. The old
    // 0.15..0.65 band kept the sky half-dusty until the sun neared its noon
    // peak, which washed the whole day gray-brown (must match day_night.rs).
    let dust_factor = (1.0 - smoothstep(0.12, 0.42, sun_height)) * night_dust_fade;

    let mut blended = blend_atmosphere(media.clear, media.dusty, dust_factor);
    // THIN the whole medium at night. The atmosphere scatters every
    // directional light (the cool night key included), and along the
    // horizon's enormous optical depth ANY bright light leaves a
    // Rayleigh-red residue — a fake amber sunset ring that sat on the
    // horizon all night at any moon elevation. Reducing the optical depth
    // itself is the only fix inside this model, and it simultaneously
    // darkens the dome toward the deep blue-black a night sky should be
    // (and that stars will eventually need).
    let night_factor = smoothstep(0.05, 0.35, -elevation);
    let thin = 1.0 - 0.88 * night_factor;
    blended.rayleigh_scattering *= thin;
    blended.mie_scattering *= thin;
    blended.mie_absorption *= thin;
    blended.ozone_absorption *= thin;
    // One key drives the throttled medium rebuilds: dust falls 1->0 across
    // sunset while night rises 0->1, so the difference moves monotonically
    // through the whole transition.
    let medium_key = dust_factor - night_factor;
    // Only deref-mut on real change so change detection stays quiet on
    // plateaus. The radii are identical in both presets, so they never move —
    // which is what keeps the spawn-time planet-center anchor (setup.rs:
    // -bottom_radius on Y) valid without a per-frame transform sync.
    if atmosphere.inner_radius != blended.bottom_radius
        || atmosphere.outer_radius != blended.top_radius
        || atmosphere.ground_albedo != blended.ground_albedo
        || atmosphere.medium != media.active_medium
    {
        atmosphere.inner_radius = blended.bottom_radius;
        atmosphere.outer_radius = blended.top_radius;
        atmosphere.ground_albedo = blended.ground_albedo;
        atmosphere.medium = media.active_medium.clone();
    }

    const ATMOSPHERE_REBUILD_INTERVAL: f32 = 0.35;
    const ATMOSPHERE_BLEND_EPS: f32 = 0.03;

    *rebuild_timer += time.delta_secs().max(0.0);
    // last_blend sentinel is far outside the key's [-1, 1] range so the
    // first frame always builds, even when the client joins mid-night.
    let needs_rebuild = (media.last_blend - medium_key).abs() > ATMOSPHERE_BLEND_EPS;
    let allow_rebuild = *rebuild_timer >= ATMOSPHERE_REBUILD_INTERVAL || media.last_blend < -5.0;

    if needs_rebuild && allow_rebuild {
        if let Some(mut medium) = media_assets.get_mut(&media.active_medium) {
            *medium = scattering_medium_from_preset(blended, "dynamic_atmosphere");
        }
        media.last_blend = medium_key;
        *rebuild_timer = 0.0;
    }
}
