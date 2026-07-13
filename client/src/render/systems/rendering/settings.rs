//! settings systems.

use super::atmosphere::default_bloom_settings;
use super::clouds::{CloudCard, CloudLayer};
use super::*;

/// Directional-shadow quality tier. Drives cascade count, cascade range, and
/// shadow map resolution together so they stay coherent.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ShadowQuality {
    Low,
    Medium,
    High,
}

impl ShadowQuality {
    pub fn label(self) -> &'static str {
        match self {
            ShadowQuality::Low => "Low",
            ShadowQuality::Medium => "Medium",
            ShadowQuality::High => "High",
        }
    }

    pub fn next(self) -> Self {
        match self {
            ShadowQuality::Low => ShadowQuality::Medium,
            ShadowQuality::Medium => ShadowQuality::High,
            ShadowQuality::High => ShadowQuality::High,
        }
    }

    pub fn prev(self) -> Self {
        match self {
            ShadowQuality::Low => ShadowQuality::Low,
            ShadowQuality::Medium => ShadowQuality::Low,
            ShadowQuality::High => ShadowQuality::Medium,
        }
    }

    /// Shadow map resolution per cascade.
    pub fn shadow_map_size(self) -> usize {
        match self {
            ShadowQuality::Low => 1024,
            ShadowQuality::Medium => 2048,
            ShadowQuality::High => 2048,
        }
    }

    /// Cascade layout. Every cascade re-renders the scene into a shadow map each
    /// frame, so fewer/shorter cascades are the main shadow cost lever.
    pub fn build_cascades(self) -> CascadeShadowConfig {
        let builder = match self {
            ShadowQuality::Low => CascadeShadowConfigBuilder {
                num_cascades: 2,
                maximum_distance: 48.0,
                first_cascade_far_bound: 12.0,
                overlap_proportion: 0.2,
                ..Default::default()
            },
            ShadowQuality::Medium => CascadeShadowConfigBuilder {
                num_cascades: 2,
                maximum_distance: 80.0,
                first_cascade_far_bound: 14.0,
                overlap_proportion: 0.2,
                ..Default::default()
            },
            ShadowQuality::High => CascadeShadowConfigBuilder {
                num_cascades: 3,
                maximum_distance: 120.0,
                first_cascade_far_bound: 10.0,
                overlap_proportion: 0.22,
                ..Default::default()
            },
        };
        builder.build()
    }
}

/// Runtime-toggleable graphics settings for troubleshooting and optimization.
/// Players can adjust these in the pause menu to fix flickering or improve FPS.
#[derive(Resource, Clone)]
pub struct GraphicsSettings {
    /// 3D resolution scale. The scene renders into an offscreen target of
    /// `window_physical_size * render_scale` and is upscaled to the window, so
    /// GPU fragment cost scales with the square of this value. UI stays native.
    /// Range: 0.5-1.0. Default: 0.75.
    pub render_scale: f32,
    /// Screen-space ambient occlusion (fullscreen pass + depth/normal prepass).
    /// Expensive on integrated GPUs. Default: off.
    pub ssao_enabled: bool,
    /// Directional shadow quality tier (cascade count/range/resolution).
    pub shadow_quality: ShadowQuality,
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

/// The default color grade.
pub fn default_color_grading(exposure: f32) -> ColorGrading {
    ColorGrading {
        global: ColorGradingGlobal {
            exposure,
            temperature: 0.016,
            tint: -0.004,
            post_saturation: 1.07,
            ..Default::default()
        },
        shadows: ColorGradingSection {
            saturation: 1.04,
            contrast: 1.03,
            lift: 0.004,
            ..Default::default()
        },
        midtones: ColorGradingSection {
            saturation: 1.04,
            contrast: 1.05,
            ..Default::default()
        },
        highlights: ColorGradingSection {
            saturation: 1.00,
            contrast: 1.02,
            gain: 0.97,
            ..Default::default()
        },
    }
}

/// SSAO settings used when the toggle is enabled.
pub fn default_ssao_settings() -> ScreenSpaceAmbientOcclusion {
    ScreenSpaceAmbientOcclusion {
        quality_level: ScreenSpaceAmbientOcclusionQualityLevel::Medium,
        constant_object_thickness: 0.18,
    }
}

impl Default for GraphicsSettings {
    fn default() -> Self {
        Self {
            render_scale: 0.75,
            ssao_enabled: false,
            shadow_quality: ShadowQuality::Medium,
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
/// - Bloom / SSAO on the camera
/// - Shadows + cascade quality on directional lights
/// - Atmosphere environment map intensity
pub fn apply_graphics_settings(
    mut commands: Commands,
    settings: Res<GraphicsSettings>,
    mut shadow_map: ResMut<DirectionalLightShadowMap>,
    mut camera_query: Query<
        (
            Entity,
            Option<&mut Bloom>,
            Option<&ScreenSpaceAmbientOcclusion>,
            &mut ColorGrading,
            &mut Tonemapping,
            &mut AtmosphereEnvironmentMapLight,
        ),
        With<Camera3d>,
    >,
    mut sun_query: Query<(Entity, &mut DirectionalLight), With<SunLight>>,
    mut clouds: Query<&mut Visibility, Or<(With<CloudLayer>, With<CloudCard>)>>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    mut ui_scale: ResMut<UiScale>,
) {
    // Only run when settings actually changed
    if !settings.is_changed() {
        return;
    }

    info!(
        "Applying graphics settings: render_scale={:.2} ssao={} shadow_quality={:?} foliage_cutout={} bloom={}, shadows={}, atmosphere={}, clouds={}, far_terrain={}, vsync={}, fullscreen={}, tonemapping={:?}, exposure={:.2}",
        settings.render_scale,
        settings.ssao_enabled,
        settings.shadow_quality,
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
    for (entity, bloom_opt, ssao_opt, mut color_grading, mut tonemapping, mut atmosphere_light) in
        camera_query.iter_mut()
    {
        let has_bloom = bloom_opt.is_some();
        if settings.bloom_enabled {
            if let Some(mut bloom) = bloom_opt {
                *bloom = default_bloom_settings();
            } else {
                commands.entity(entity).insert(default_bloom_settings());
            }
        } else if has_bloom {
            commands.entity(entity).remove::<Bloom>();
        }

        // Toggle SSAO. Removing the component alone is not enough: SSAO's
        // required DepthPrepass/NormalPrepass components stay behind and keep
        // costing a full extra geometry pass, so strip those too.
        if settings.ssao_enabled {
            if ssao_opt.is_none() {
                commands.entity(entity).insert(default_ssao_settings());
            }
        } else if ssao_opt.is_some() {
            commands
                .entity(entity)
                .remove::<(ScreenSpaceAmbientOcclusion, DepthPrepass, NormalPrepass)>();
        }

        *tonemapping = settings.tonemapping;
        *color_grading = default_color_grading(settings.grade_exposure.clamp(-2.0, 2.0));

        // Toggle atmosphere environment map intensity
        let lighting_boost = settings.lighting_boost.clamp(0.5, 4.0);
        atmosphere_light.intensity = if settings.atmosphere_enabled {
            1.0 * lighting_boost
        } else {
            0.0
        };
    }

    // Toggle shadows + apply cascade quality on the sun light.
    let desired_map_size = settings.shadow_quality.shadow_map_size();
    if shadow_map.size != desired_map_size {
        shadow_map.size = desired_map_size;
    }
    for (sun_entity, mut sun_light) in sun_query.iter_mut() {
        sun_light.shadows_enabled = settings.shadows_enabled;
        commands
            .entity(sun_entity)
            .insert(settings.shadow_quality.build_cascades());
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
