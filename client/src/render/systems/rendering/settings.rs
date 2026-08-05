//! settings systems.

use super::atmosphere::default_bloom_settings;
use super::cloud_layer::CloudLayerPlane;
use super::clouds::CloudLayer;
use super::*;
use crate::camera_rts::CommanderCamera;
use bevy::pbr::ContactShadows;

/// Directional-shadow quality tier. Drives cascade count, cascade range, and
/// shadow map resolution together so they stay coherent.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
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

    /// Floor for the zoom-scaled cascade span, in metres of view distance.
    /// Also the span used at spawn, before the zoom sync has run.
    fn base_distance(self) -> f32 {
        match self {
            ShadowQuality::Low => 48.0,
            ShadowQuality::Medium => 80.0,
            ShadowQuality::High => 120.0,
        }
    }

    /// Cascade layout at the base distance. `sync_shadow_cascades_to_zoom`
    /// replaces this with a zoom-scaled span once the camera exists.
    pub fn build_cascades(self) -> CascadeShadowConfig {
        self.build_cascades_for(self.base_distance())
    }

    /// Cascade layout covering `maximum_distance` metres of view. Every cascade
    /// re-renders the scene into a shadow map each frame, so the cascade count
    /// stays a per-quality constant and only the covered span stretches; the
    /// first cascade bound scales with the span to keep the split ratios.
    fn build_cascades_for(self, maximum_distance: f32) -> CascadeShadowConfig {
        let (num_cascades, base_first_bound, overlap_proportion) = match self {
            ShadowQuality::Low => (2, 12.0, 0.2),
            ShadowQuality::Medium => (2, 14.0, 0.2),
            ShadowQuality::High => (3, 10.0, 0.22),
        };
        let maximum_distance = maximum_distance.max(self.base_distance());
        CascadeShadowConfigBuilder {
            num_cascades,
            maximum_distance,
            first_cascade_far_bound: base_first_bound * (maximum_distance / self.base_distance()),
            overlap_proportion,
            ..Default::default()
        }
        .build()
    }
}

/// Runtime-toggleable graphics settings for troubleshooting and optimization.
/// Players can adjust these in the pause menu to fix flickering or improve FPS.
/// Persisted to [`SETTINGS_FILE`]; unknown/missing fields fall back to the
/// (env-aware) defaults so old files survive new fields.
#[derive(Resource, Clone, serde::Serialize, serde::Deserialize)]
#[serde(default)]
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
    /// Dev-only (no UI): grade + lighting are tuned for AgX. Not persisted.
    #[serde(skip, default = "default_tonemapping")]
    pub tonemapping: Tonemapping,
    /// Color grading exposure offset (EV). Range: -1.0..2.0 (matches the
    /// pause-menu steps). Default: 0.20.
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
/// Grade for the top-down look: keep colour, soften light slightly.
///
/// Two failed attempts are worth recording, because both are tempting:
/// 1. Pulling saturation down across every band ("pastel") turned the world muddy olive.
///    Dreamy art is *saturated*; the softness comes from light, not from removing colour.
/// 2. Lifting shadows hard (0.038) to get haze washed the entire frame. A top-down camera
///    sees mostly midtones, so a global lift flattens everything rather than just shadows.
///
/// Style beyond this belongs in shading and terrain colour, not in the grade — a tone
/// curve cannot make photographic terrain textures look low-poly.
pub fn default_color_grading(exposure: f32) -> ColorGrading {
    ColorGrading {
        global: ColorGradingGlobal {
            exposure,
            temperature: 0.030,
            tint: -0.010,
            post_saturation: 1.06,
            ..Default::default()
        },
        shadows: ColorGradingSection {
            saturation: 1.04,
            contrast: 0.99,
            lift: 0.008,
            ..Default::default()
        },
        midtones: ColorGradingSection {
            saturation: 1.05,
            contrast: 1.01,
            ..Default::default()
        },
        highlights: ColorGradingSection {
            // Soft highlight rolloff is the one part of the dreamy pass that helped.
            saturation: 1.00,
            contrast: 0.98,
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

fn default_tonemapping() -> Tonemapping {
    Tonemapping::AgX
}

/// User settings file (RON). Env test hooks override whatever it says.
pub const SETTINGS_FILE: &str = "client_data/settings.ron";

impl Default for GraphicsSettings {
    fn default() -> Self {
        let mut settings = Self {
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
            // Fullscreen-on-play is the shipped default; the toggle persists
            // the player's preference from there.
            fullscreen_enabled: true,
            tonemapping: default_tonemapping(),
            grade_exposure: 0.2,
            view_distance: 8,
            prop_render_multiplier: 1.0,
            lighting_boost: 1.0,
        };
        settings.apply_env_overrides();
        settings
    }
}

impl GraphicsSettings {
    /// Test hooks: headless profiling runs ablate one subsystem at a time
    /// without input automation. Absent vars leave the current values, so
    /// these apply cleanly on top of the settings file too (env wins).
    fn apply_env_overrides(&mut self) {
        let env_bool = |name: &str, current: bool| -> bool {
            std::env::var(name)
                .ok()
                .map(|raw| raw == "1" || raw.eq_ignore_ascii_case("true"))
                .unwrap_or(current)
        };
        if let Some(scale) = std::env::var("FISTFORCE_RENDER_SCALE")
            .ok()
            .and_then(|raw| raw.parse::<f32>().ok())
            .filter(|s| s.is_finite())
        {
            // Floor matches scaled_target_extent's clamp so the resource
            // never claims a scale the target refuses to render at.
            self.render_scale = scale.clamp(0.5, 1.0);
        }
        self.shadows_enabled = env_bool("FISTFORCE_SHADOWS", self.shadows_enabled);
        self.atmosphere_enabled = env_bool("FISTFORCE_ATMOSPHERE", self.atmosphere_enabled);
        self.clouds_enabled = env_bool("FISTFORCE_CLOUDS", self.clouds_enabled);
        self.props_enabled = env_bool("FISTFORCE_PROPS", self.props_enabled);
        self.vsync_enabled = env_bool("FISTFORCE_VSYNC", self.vsync_enabled);
        self.fullscreen_enabled = env_bool("FISTFORCE_FULLSCREEN", self.fullscreen_enabled);
    }

    /// Settings for this run: the saved file (if any) under the env overrides.
    /// FISTFORCE_NO_SETTINGS_FILE skips the file for reproducible captures.
    pub fn load_or_default() -> Self {
        if std::env::var("FISTFORCE_NO_SETTINGS_FILE").is_ok() {
            return Self::default();
        }
        let mut settings = std::fs::read_to_string(SETTINGS_FILE)
            .ok()
            .and_then(|text| match ron::from_str::<GraphicsSettings>(&text) {
                Ok(parsed) => Some(parsed),
                Err(err) => {
                    warn!("Ignoring malformed {SETTINGS_FILE}: {err}");
                    None
                }
            })
            .unwrap_or_default();
        settings.apply_env_overrides();
        settings
    }
}

/// Persist settings ~a second after the last change, so slider drags don't
/// write a file per step.
pub fn save_graphics_settings(
    settings: Res<GraphicsSettings>,
    time: Res<Time>,
    mut deadline: Local<Option<f32>>,
) {
    if settings.is_changed() && !settings.is_added() {
        *deadline = Some(time.elapsed_secs() + 1.0);
    }
    let Some(due) = *deadline else {
        return;
    };
    if time.elapsed_secs() < due {
        return;
    }
    *deadline = None;
    let serialized = match ron::ser::to_string_pretty(&*settings, ron::ser::PrettyConfig::default())
    {
        Ok(text) => text,
        Err(err) => {
            warn!("Could not serialize graphics settings: {err}");
            return;
        }
    };
    if let Some(parent) = std::path::Path::new(SETTINGS_FILE).parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    match std::fs::write(SETTINGS_FILE, serialized) {
        Ok(()) => info!("Saved graphics settings to {SETTINGS_FILE}"),
        Err(err) => warn!("Could not write {SETTINGS_FILE}: {err}"),
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
            Has<ContactShadows>,
            &mut ColorGrading,
            &mut Tonemapping,
        ),
        With<Camera3d>,
    >,
    mut sun_query: Query<&mut DirectionalLight, With<SunLight>>,
    mut clouds: Query<&mut Visibility, Or<(With<CloudLayer>, With<CloudLayerPlane>)>>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    monitors: Query<&bevy::window::Monitor, With<bevy::window::PrimaryMonitor>>,
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
    for (entity, bloom_opt, ssao_opt, has_contact_shadows, mut color_grading, mut tonemapping) in
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
        // costing a full extra geometry pass, so strip both unconditionally —
        // SSAO is their only remaining consumer (directional shadow maps never
        // needed the camera prepass; the old shadows_enabled condition was a
        // leftover from the removed ContactShadows experiment and stranded a
        // permanent DepthPrepass after any SSAO on->off toggle).
        if settings.ssao_enabled {
            if ssao_opt.is_none() {
                commands.entity(entity).insert(default_ssao_settings());
            }
        } else if ssao_opt.is_some() {
            commands
                .entity(entity)
                .remove::<(ScreenSpaceAmbientOcclusion, NormalPrepass, DepthPrepass)>();
        }

        // Contact shadows are deliberately NOT enabled: the 0.19 contact-shadow
        // view-layout variant breaks the custom wind-foliage/toon ExtendedMaterial
        // shaders (foliage renders base-white — verified by bisect 2026-07-28).
        // Revisit once those shaders handle the variant. The removal branch cleans
        // up if one was ever inserted.
        if has_contact_shadows {
            commands.entity(entity).remove::<ContactShadows>();
        }

        *tonemapping = settings.tonemapping;
        *color_grading = default_color_grading(settings.grade_exposure.clamp(-2.0, 2.0));

        // NOTE: AtmosphereEnvironmentMapLight.intensity is owned solely by
        // update_day_night_cycle (which respects atmosphere_enabled); a second
        // writer here used to fight it, unordered.
    }

    // Toggle shadows on the sun light. Cascade config is owned by
    // sync_shadow_cascades_to_zoom, which also reacts to quality changes.
    let desired_map_size = settings.shadow_quality.shadow_map_size();
    if shadow_map.size != desired_map_size {
        shadow_map.size = desired_map_size;
    }
    for mut sun_light in sun_query.iter_mut() {
        sun_light.shadow_maps_enabled = settings.shadows_enabled;
        // Keep contact_shadows_enabled at its false default — see the camera-side
        // note about the wind/toon shader incompatibility.
        if sun_light.contact_shadows_enabled {
            sun_light.contact_shadows_enabled = false;
        }
    }

    let cloud_visibility = if settings.clouds_enabled {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    for mut vis in clouds.iter_mut() {
        *vis = cloud_visibility;
    }

    // Apply vsync (present mode) and the fullscreen preference.
    let monitor = monitors.iter().next();
    for mut window in windows.iter_mut() {
        let wants_fullscreen = settings.fullscreen_enabled;
        let is_fullscreen = !matches!(window.mode, WindowMode::Windowed);
        if wants_fullscreen != is_fullscreen {
            crate::app_wiring::apply_window_mode(
                &mut window,
                &mut ui_scale,
                monitor,
                wants_fullscreen,
            );
        } else {
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

/// Cascade span per metre of camera zoom. The camera looks down at the focus
/// from `zoom` metres away, so visible ground spans roughly `zoom..2.5*zoom`
/// of view distance.
const SHADOW_DISTANCE_PER_ZOOM: f32 = 2.5;
/// Caps the span at "whole map from max zoom" (8 km world seen from 12 km).
/// Far-zoom texels get coarse (metres) — acceptable for terrain relief, while
/// prop casters are culled by zoom instead (see `render::shadow_cull`), which
/// is also what keeps the shadow passes affordable at map scale.
const SHADOW_DISTANCE_MAX: f32 = 16_000.0;
/// Geometric ratio between zoom bands. Cascades are only rebuilt when the zoom
/// crosses into a new band.
const SHADOW_ZOOM_BAND_RATIO: f32 = 1.5;
/// Hysteresis in band units. Must exceed 0.5 (the midpoint where a freshly
/// entered band would otherwise flip straight back).
const SHADOW_ZOOM_BAND_STICKINESS: f32 = 0.65;

/// Stretch the sun's cascade span to follow camera zoom (12 m..12 km), so
/// shadows exist at RTS zoom instead of stopping at the FPS-era 48-120 m.
///
/// Rebuilding CascadeShadowConfig re-fits every shadow map, so zoom is
/// quantized into geometric bands with hysteresis and the config is only
/// rebuilt on a band change (or when the quality setting / sun entity change).
pub fn sync_shadow_cascades_to_zoom(
    mut commands: Commands,
    settings: Res<GraphicsSettings>,
    cameras: Query<&CommanderCamera>,
    suns: Query<Entity, With<SunLight>>,
    mut applied: Local<Option<(Entity, ShadowQuality, i32)>>,
) {
    let Ok(camera) = cameras.single() else {
        return;
    };
    let Ok(sun) = suns.single() else {
        return;
    };

    let quality = settings.shadow_quality;
    let base = quality.base_distance();
    let desired = (camera.zoom * SHADOW_DISTANCE_PER_ZOOM).clamp(base, SHADOW_DISTANCE_MAX);
    // Continuous band position: geometric steps of the band ratio above base.
    let band_pos = (desired / base).ln() / SHADOW_ZOOM_BAND_RATIO.ln();

    if let Some((entity, applied_quality, band)) = *applied {
        let same_target = entity == sun && applied_quality == quality;
        if same_target && (band_pos - band as f32).abs() <= SHADOW_ZOOM_BAND_STICKINESS {
            return;
        }
    }

    let band = band_pos.round() as i32;
    *applied = Some((sun, quality, band));
    let distance = (base * SHADOW_ZOOM_BAND_RATIO.powi(band)).min(SHADOW_DISTANCE_MAX);
    commands
        .entity(sun)
        .insert(quality.build_cascades_for(distance));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_roundtrip_through_ron() {
        let settings = GraphicsSettings {
            render_scale: 0.6,
            fullscreen_enabled: false,
            view_distance: 12,
            ..Default::default()
        };
        let text = ron::ser::to_string_pretty(&settings, ron::ser::PrettyConfig::default())
            .expect("serialize");
        let parsed: GraphicsSettings = ron::from_str(&text).expect("parse");
        assert_eq!(parsed.render_scale, 0.6);
        assert!(!parsed.fullscreen_enabled);
        assert_eq!(parsed.view_distance, 12);
        // Skipped field falls back to the pinned default.
        assert_eq!(parsed.tonemapping, Tonemapping::AgX);
    }

    #[test]
    fn old_settings_files_survive_new_fields() {
        // A minimal file (as if written before most fields existed) must parse
        // with defaults filling the gaps.
        let parsed: GraphicsSettings = ron::from_str("(render_scale: 0.55)").expect("parse");
        assert_eq!(parsed.render_scale, 0.55);
        assert!(parsed.shadows_enabled);
    }
}
