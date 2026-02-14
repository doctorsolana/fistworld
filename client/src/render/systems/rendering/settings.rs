//! settings systems.

use super::atmosphere::default_bloom_settings;
use super::clouds::{CloudCard, CloudLayer};
use super::*;

/// Runtime-toggleable graphics settings for troubleshooting and optimization.
/// Players can adjust these in the pause menu to fix flickering or improve FPS.
#[derive(Resource, Clone)]
pub struct GraphicsSettings {
    /// Enable Retina rendering (scale factor from OS). When false, forces 1.0 scale (macOS only).
    pub retina_render_enabled: bool,
    /// Use alpha cutout for foliage instead of alpha blending.
    pub foliage_cutout_enabled: bool,
    pub bloom_enabled: bool,
    pub shadows_enabled: bool,
    pub atmosphere_enabled: bool,
    pub clouds_enabled: bool,
    pub far_terrain_enabled: bool,
    pub props_enabled: bool,
    pub vsync_enabled: bool,
    pub fullscreen_enabled: bool,
    pub tonemapping: Tonemapping,
    /// Color grading exposure offset (EV). Range: -2.0..2.0. Default: 0.20.
    pub grade_exposure: f32,
    /// View distance in chunks (1 chunk = 64m). Range: 2-16. Default: 8 (512m).
    pub view_distance: i32,
    /// Multiplier for prop render distance. Range: 0.25-4.0. Default: 1.0.
    pub prop_render_multiplier: f32,
    /// Multiplier for ambient + IBL lighting. Range: 0.5-4.0. Default: 1.0.
    pub lighting_boost: f32,
}

/// Launcher window resolution (physical pixels).
pub const LAUNCHER_RESOLUTION: (u32, u32) = (1600, 900);

impl Default for GraphicsSettings {
    fn default() -> Self {
        Self {
            retina_render_enabled: false,
            foliage_cutout_enabled: true,
            bloom_enabled: true,
            shadows_enabled: true,
            atmosphere_enabled: true,
            clouds_enabled: true,
            far_terrain_enabled: true,
            props_enabled: true,
            vsync_enabled: true,
            fullscreen_enabled: false,
            tonemapping: Tonemapping::AgX,
            grade_exposure: 0.2,
            view_distance: 8,
            prop_render_multiplier: 1.0,
            lighting_boost: 1.0,
        }
    }
}

/// Runtime-adjustable input settings for player controls.
/// Players can adjust these in the pause menu under Controls.
#[derive(Resource, Clone)]
pub struct InputSettings {
    /// Mouse sensitivity multiplier. Range: 0.1-3.0. Default: 1.0.
    /// Applied as a multiplier to the base MOUSE_SENSITIVITY constant.
    pub mouse_sensitivity: f32,
}

impl Default for InputSettings {
    fn default() -> Self {
        Self {
            mouse_sensitivity: 1.0,
        }
    }
}

/// Apply graphics settings changes to rendering components.
/// This system runs when GraphicsSettings is changed and toggles:
/// - Bloom on the camera
/// - Shadows on directional lights
/// - Atmosphere environment map intensity
pub fn apply_graphics_settings(
    mut commands: Commands,
    settings: Res<GraphicsSettings>,
    mut camera_query: Query<
        (
            Entity,
            Option<&mut Bloom>,
            &mut ColorGrading,
            &mut Tonemapping,
            &mut AtmosphereEnvironmentMapLight,
        ),
        With<Camera3d>,
    >,
    mut sun_query: Query<&mut DirectionalLight, With<SunLight>>,
    mut clouds: Query<&mut Visibility, Or<(With<CloudLayer>, With<CloudCard>)>>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    mut ui_scale: ResMut<UiScale>,
) {
    // Only run when settings actually changed
    if !settings.is_changed() {
        return;
    }

    info!(
        "Applying graphics settings: retina={} foliage_cutout={} bloom={}, shadows={}, atmosphere={}, clouds={}, far_terrain={}, vsync={}, fullscreen={}, tonemapping={:?}, exposure={:.2}",
        settings.retina_render_enabled,
        settings.foliage_cutout_enabled,
        settings.bloom_enabled,
        settings.shadows_enabled,
        settings.atmosphere_enabled,
        settings.clouds_enabled,
        settings.far_terrain_enabled,
        settings.vsync_enabled,
        settings.fullscreen_enabled,
        settings.tonemapping,
        settings.grade_exposure
    );

    // Toggle bloom component (avoid running the bloom pass when disabled).
    for (entity, bloom_opt, mut color_grading, mut tonemapping, mut atmosphere_light) in
        camera_query.iter_mut()
    {
        let has_bloom = bloom_opt.is_some();
        if settings.bloom_enabled {
            if let Some(mut bloom) = bloom_opt {
                bloom.intensity = 0.08;
            } else {
                commands.entity(entity).insert(default_bloom_settings());
            }
        } else if has_bloom {
            commands.entity(entity).remove::<Bloom>();
        }

        *tonemapping = settings.tonemapping;
        color_grading.global.exposure = settings.grade_exposure.clamp(-2.0, 2.0);

        // Toggle atmosphere environment map intensity
        let lighting_boost = settings.lighting_boost.clamp(0.5, 4.0);
        atmosphere_light.intensity = if settings.atmosphere_enabled {
            1.0 * lighting_boost
        } else {
            0.0
        };
    }

    // Toggle shadows on sun light
    for mut sun_light in sun_query.iter_mut() {
        sun_light.shadows_enabled = settings.shadows_enabled;
    }

    let cloud_visibility = if settings.clouds_enabled {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    for mut vis in clouds.iter_mut() {
        *vis = cloud_visibility;
    }

    // Apply vsync (present mode) for the primary window.
    for mut window in windows.iter_mut() {
        #[cfg(target_os = "macos")]
        {
            let base_scale = window.resolution.base_scale_factor();
            window.resolution.set_scale_factor_override(Some(1.0));
            ui_scale.0 = base_scale;
        }
        #[cfg(not(target_os = "macos"))]
        {
            ui_scale.0 = 1.0;
        }
        window.present_mode = if settings.vsync_enabled {
            PresentMode::AutoVsync
        } else {
            PresentMode::AutoNoVsync
        };

        info!(
            "Window state: mode={:?} physical={}x{} logical={:.0}x{:.0} scale(base={:.2} override={:?}) ui_scale={:.2}",
            window.mode,
            window.resolution.physical_width(),
            window.resolution.physical_height(),
            window.resolution.width(),
            window.resolution.height(),
            window.resolution.base_scale_factor(),
            window.resolution.scale_factor_override(),
            ui_scale.0
        );
    }
}
