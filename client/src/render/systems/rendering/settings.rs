//! settings systems.

mod persistence;
pub use persistence::{save_graphics_settings, GraphicsSettingsStore};

use super::atmosphere::default_bloom_settings;
use super::cloud_layer::CloudLayerPlane;
use super::*;
use crate::camera_rts::CommanderCamera;
use bevy::pbr::ContactShadows;
use bevy::window::{Monitor, VideoMode};

/// Player-facing display mode. `Borderless` deliberately uses the monitor's
/// current/native mode; only `ExclusiveFullscreen` is allowed to change the
/// monitor's video mode and therefore apply a lower physical resolution.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum DisplayMode {
    Windowed,
    Borderless,
    ExclusiveFullscreen,
}

impl DisplayMode {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Windowed => "Windowed",
            Self::Borderless => "Borderless Fullscreen",
            Self::ExclusiveFullscreen => "Exclusive Fullscreen",
        }
    }
}

/// Requested client-area resolution in physical pixels.
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct DisplayResolution {
    pub width: u32,
    pub height: u32,
}

impl DisplayResolution {
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }

    pub fn label(self) -> String {
        format!("{} x {}", self.width, self.height)
    }
}

pub const DISPLAY_CONFIRMATION_SECONDS: f32 = 15.0;

/// A reversible output-mode change. Rendering applies the candidate
/// immediately, while settings persistence waits until the player keeps it.
#[derive(Resource, Debug, Clone, Copy)]
pub struct PendingDisplayChange {
    pub previous_mode: DisplayMode,
    pub previous_resolution: DisplayResolution,
    pub seconds_left: f32,
}

impl PendingDisplayChange {
    pub fn new(settings: &GraphicsSettings) -> Self {
        Self {
            previous_mode: settings.display_mode(),
            previous_resolution: settings.display_resolution,
            seconds_left: DISPLAY_CONFIRMATION_SECONDS,
        }
    }

    pub fn restart_countdown(&mut self) {
        self.seconds_left = DISPLAY_CONFIRMATION_SECONDS;
    }
}

const COMMON_WINDOWED_RESOLUTIONS: &[DisplayResolution] = &[
    DisplayResolution::new(1024, 576),
    DisplayResolution::new(1280, 720),
    DisplayResolution::new(1280, 800),
    DisplayResolution::new(1366, 768),
    DisplayResolution::new(1440, 900),
    DisplayResolution::new(1600, 900),
    DisplayResolution::new(1680, 1050),
    DisplayResolution::new(1920, 1080),
    DisplayResolution::new(1920, 1200),
    DisplayResolution::new(2560, 1440),
    DisplayResolution::new(2560, 1600),
    DisplayResolution::new(3840, 2160),
];

/// Resolutions the current display mode can genuinely apply.
///
/// Exclusive fullscreen only exposes exact modes reported by the OS. Passing
/// an invented mode to Bevy 0.19 is rejected by `bevy_winit`, so this list is
/// also the authoritative safety boundary for the pause-menu selector.
pub fn available_display_resolutions(
    display_mode: DisplayMode,
    monitor: Option<&Monitor>,
    current: DisplayResolution,
) -> Vec<DisplayResolution> {
    let mut resolutions = match display_mode {
        DisplayMode::Borderless => monitor
            .map(|monitor| {
                vec![DisplayResolution::new(
                    monitor.physical_width,
                    monitor.physical_height,
                )]
            })
            .unwrap_or_else(|| vec![current]),
        DisplayMode::ExclusiveFullscreen => monitor
            .map(|monitor| {
                monitor
                    .video_modes
                    .iter()
                    .filter(|mode| mode.physical_size.x >= 1024 && mode.physical_size.y >= 576)
                    .map(|mode| DisplayResolution::new(mode.physical_size.x, mode.physical_size.y))
                    .collect()
            })
            .unwrap_or_else(|| vec![current]),
        DisplayMode::Windowed => COMMON_WINDOWED_RESOLUTIONS
            .iter()
            .copied()
            .filter(|resolution| {
                monitor.is_none_or(|monitor| {
                    resolution.width <= monitor.physical_width
                        && resolution.height <= monitor.physical_height
                })
            })
            .chain(std::iter::once(current))
            .collect(),
    };
    resolutions.sort_unstable_by_key(|resolution| {
        (
            u64::from(resolution.width) * u64::from(resolution.height),
            resolution.width,
            resolution.height,
        )
    });
    resolutions.dedup();
    if resolutions.is_empty() {
        resolutions.push(current);
    }
    resolutions
}

/// Pick the exact Bevy video mode for an exclusive-fullscreen request.
/// Duplicate resolutions are common; prefer the highest refresh rate, then
/// the greatest bit depth, just as a conventional game would when refresh is
/// not exposed as a separate setting.
pub fn best_fullscreen_video_mode(
    monitor: &Monitor,
    requested: DisplayResolution,
) -> Option<VideoMode> {
    monitor
        .video_modes
        .iter()
        .copied()
        .filter(|mode| {
            mode.physical_size.x == requested.width && mode.physical_size.y == requested.height
        })
        .max_by_key(|mode| (mode.refresh_rate_millihertz, mode.bit_depth))
}

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
#[serde(default = "GraphicsSettings::shipped_defaults")]
pub struct GraphicsSettings {
    /// 3D resolution scale. The scene renders into an offscreen target of
    /// `window_physical_size * render_scale` and is upscaled to the window, so
    /// GPU fragment cost scales with the square of this value. UI stays native.
    /// Range: 0.25-1.0. Default: 0.60 on macOS, 0.75 elsewhere.
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
    /// Software frame cap in frames per second, independent of vsync
    /// (0 = uncapped). Frames that finish early sleep until the target period;
    /// slow frames are never delayed, so unlike vsync it cannot quantize a
    /// 17 ms frame down to 30 fps. Measured on the M5 MacBook: running
    /// uncapped drives the SoC into its power limit within ~90 s and EVERY
    /// frame then costs 2-3x more; capped at the display rate the machine
    /// stays cool and frame time stays flat. Default: 60.
    pub frame_cap_fps: u32,
    /// Legacy-compatible half of the three-way display mode. Old settings
    /// files contain only this field, so retaining it preserves the player's
    /// previous Windowed/Borderless choice during migration.
    pub fullscreen_enabled: bool,
    /// When fullscreen is enabled, choose an exact monitor video mode instead
    /// of native-resolution borderless fullscreen.
    pub exclusive_fullscreen_enabled: bool,
    /// Physical output resolution for Windowed and exclusive Fullscreen.
    /// Borderless fullscreen is always native by platform definition.
    pub display_resolution: DisplayResolution,
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
        let mut settings = Self::shipped_defaults();
        settings.apply_env_overrides();
        settings
    }
}

impl GraphicsSettings {
    /// The literal shipped defaults, with NO env overrides applied. This is
    /// the persistence baseline: env-forced values must never reach the
    /// settings file (see [`revert_env_forced`]).
    fn shipped_defaults() -> Self {
        Self {
            // Borderless fullscreen keeps normal macOS app/Space switching,
            // but its native Retina output is much larger than the common
            // exclusive modes. A 60% 3D target keeps the default workload
            // sensible while the HUD remains native-resolution and crisp.
            render_scale: if cfg!(target_os = "macos") {
                0.60
            } else {
                0.75
            },
            ssao_enabled: false,
            shadow_quality: ShadowQuality::Medium,
            foliage_cutout_enabled: true,
            bloom_enabled: true,
            shadows_enabled: true,
            atmosphere_enabled: true,
            clouds_enabled: true,
            far_terrain_enabled: true,
            props_enabled: true,
            frame_cap_fps: 60,
            // Off on macOS: the compositor already prevents tearing, while
            // strict FIFO vsync quantizes missed refreshes (60 -> 30 -> 20),
            // which punishes weaker Macs hardest — a 45fps-capable machine
            // gets locked to 30. On other platforms tearing is real, so the
            // safe default stays on. The pause-menu toggle persists per user.
            vsync_enabled: cfg!(not(target_os = "macos")),
            // Fullscreen-on-play is the shipped default; the toggle persists
            // the player's preference from there.
            fullscreen_enabled: true,
            exclusive_fullscreen_enabled: false,
            display_resolution: DisplayResolution::new(
                LAUNCHER_RESOLUTION.0,
                LAUNCHER_RESOLUTION.1,
            ),
            tonemapping: default_tonemapping(),
            grade_exposure: 0.2,
            view_distance: 8,
            prop_render_multiplier: 1.0,
            lighting_boost: 1.0,
        }
    }

    pub const fn display_mode(&self) -> DisplayMode {
        match (self.fullscreen_enabled, self.exclusive_fullscreen_enabled) {
            (false, _) => DisplayMode::Windowed,
            (true, false) => DisplayMode::Borderless,
            (true, true) => DisplayMode::ExclusiveFullscreen,
        }
    }

    pub fn set_display_mode(&mut self, mode: DisplayMode) {
        match mode {
            DisplayMode::Windowed => {
                self.fullscreen_enabled = false;
                self.exclusive_fullscreen_enabled = false;
            }
            DisplayMode::Borderless => {
                self.fullscreen_enabled = true;
                self.exclusive_fullscreen_enabled = false;
            }
            DisplayMode::ExclusiveFullscreen => {
                self.fullscreen_enabled = true;
                self.exclusive_fullscreen_enabled = true;
            }
        }
    }

    pub fn displayed_resolution_label(&self, monitor: Option<&Monitor>) -> String {
        let prefix = match self.display_mode() {
            DisplayMode::Borderless => Some("Native"),
            DisplayMode::ExclusiveFullscreen
                if monitor
                    .and_then(|monitor| {
                        best_fullscreen_video_mode(monitor, self.display_resolution)
                    })
                    .is_none() =>
            {
                // apply_window_mode uses VideoModeSelection::Current when an
                // old preference is unavailable on this monitor. Describe the
                // fallback instead of claiming the stale request was applied.
                Some("Current")
            }
            _ => None,
        };
        if let Some(prefix) = prefix {
            monitor.map_or_else(
                || prefix.to_string(),
                |monitor| {
                    format!(
                        "{prefix} {} x {}",
                        monitor.physical_width, monitor.physical_height
                    )
                },
            )
        } else {
            self.display_resolution.label()
        }
    }

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
            self.render_scale = super::scaled_target::clamped_render_scale(scale);
        }
        self.shadows_enabled = env_bool("FISTFORCE_SHADOWS", self.shadows_enabled);
        self.atmosphere_enabled = env_bool("FISTFORCE_ATMOSPHERE", self.atmosphere_enabled);
        self.clouds_enabled = env_bool("FISTFORCE_CLOUDS", self.clouds_enabled);
        self.props_enabled = env_bool("FISTFORCE_PROPS", self.props_enabled);
        self.vsync_enabled = env_bool("FISTFORCE_VSYNC", self.vsync_enabled);
        if let Some(cap) = std::env::var("FISTFORCE_FRAME_CAP")
            .ok()
            .and_then(|raw| raw.trim().parse::<u32>().ok())
        {
            self.frame_cap_fps = cap;
        }
        self.fullscreen_enabled = env_bool("FISTFORCE_FULLSCREEN", self.fullscreen_enabled);
        self.exclusive_fullscreen_enabled = env_bool(
            "FISTFORCE_EXCLUSIVE_FULLSCREEN",
            self.exclusive_fullscreen_enabled,
        );
        if let Ok(raw) = std::env::var("FISTFORCE_DISPLAY_MODE") {
            let mode = match raw.trim().to_ascii_lowercase().as_str() {
                "windowed" | "window" => Some(DisplayMode::Windowed),
                "borderless" | "borderless-fullscreen" => Some(DisplayMode::Borderless),
                "fullscreen" | "exclusive" | "exclusive-fullscreen" => {
                    Some(DisplayMode::ExclusiveFullscreen)
                }
                _ => None,
            };
            if let Some(mode) = mode {
                self.set_display_mode(mode);
            } else {
                warn!("Ignoring invalid FISTFORCE_DISPLAY_MODE='{raw}'");
            }
        }
        if let Ok(raw) = std::env::var("FISTFORCE_RESOLUTION") {
            let parsed = raw
                .trim()
                .to_ascii_lowercase()
                .split_once('x')
                .and_then(|(width, height)| {
                    Some((
                        width.trim().parse::<u32>().ok()?,
                        height.trim().parse::<u32>().ok()?,
                    ))
                })
                .filter(|(width, height)| *width >= 1024 && *height >= 576);
            if let Some((width, height)) = parsed {
                self.display_resolution = DisplayResolution::new(width, height);
            } else {
                warn!("Ignoring invalid FISTFORCE_RESOLUTION='{raw}'; expected WIDTHxHEIGHT");
            }
        }
    }

    /// Undo [`Self::apply_env_overrides`] for persistence: every field an env
    /// var is forcing THIS session reverts to `baseline` (the settings file as
    /// loaded, or the shipped defaults). Without this, one profiling run with
    /// FISTFORCE_ATMOSPHERE=0 that happened to save its settings would bake
    /// "atmosphere off" into the file — and every later session would join a
    /// skyless world with no idea why (this actually happened; the sky and
    /// clouds were silently off for days). Env overrides are session-only.
    fn revert_env_forced(&mut self, baseline: &Self) {
        let forced = |name: &str| std::env::var(name).is_ok();
        if forced("FISTFORCE_RENDER_SCALE") {
            self.render_scale = baseline.render_scale;
        }
        if forced("FISTFORCE_FRAME_CAP") {
            self.frame_cap_fps = baseline.frame_cap_fps;
        }
        if forced("FISTFORCE_SHADOWS") {
            self.shadows_enabled = baseline.shadows_enabled;
        }
        if forced("FISTFORCE_ATMOSPHERE") {
            self.atmosphere_enabled = baseline.atmosphere_enabled;
        }
        if forced("FISTFORCE_CLOUDS") {
            self.clouds_enabled = baseline.clouds_enabled;
        }
        if forced("FISTFORCE_PROPS") {
            self.props_enabled = baseline.props_enabled;
        }
        if forced("FISTFORCE_VSYNC") {
            self.vsync_enabled = baseline.vsync_enabled;
        }
        if forced("FISTFORCE_FULLSCREEN") || forced("FISTFORCE_DISPLAY_MODE") {
            self.fullscreen_enabled = baseline.fullscreen_enabled;
        }
        if forced("FISTFORCE_EXCLUSIVE_FULLSCREEN") || forced("FISTFORCE_DISPLAY_MODE") {
            self.exclusive_fullscreen_enabled = baseline.exclusive_fullscreen_enabled;
        }
        if forced("FISTFORCE_RESOLUTION") {
            self.display_resolution = baseline.display_resolution;
        }
    }
}

/// Automatically restore the last confirmed output settings when a candidate
/// is not confirmed. This runs even if the pause menu is closed after making
/// the change, so the player cannot accidentally strand the countdown UI.
pub fn tick_display_change_confirmation(
    time: Res<Time>,
    pending: Option<ResMut<PendingDisplayChange>>,
    mut settings: ResMut<GraphicsSettings>,
    mut commands: Commands,
) {
    let Some(mut pending) = pending else {
        return;
    };
    pending.seconds_left -= time.delta_secs();
    if pending.seconds_left > 0.0 {
        return;
    }
    let previous_mode = pending.previous_mode;
    let previous_resolution = pending.previous_resolution;
    settings.set_display_mode(previous_mode);
    settings.display_resolution = previous_resolution;
    commands.remove_resource::<PendingDisplayChange>();
    warn!(
        "Display change was not confirmed; restored {} at {}",
        previous_mode.label(),
        previous_resolution.label()
    );
}

/// Runtime-adjustable input settings for player controls.
/// Players can adjust these in the pause menu under Controls.
#[derive(Resource, Clone)]
pub struct InputSettings {
    /// Mouse sensitivity multiplier. Range: 0.1-3.0. Default: 1.0.
    /// Multiplies the RTS camera's yaw look sensitivity (camera_rts.rs).
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
    mut clouds: Query<&mut Visibility, With<CloudLayerPlane>>,
    mut windows: Query<&mut Window, With<PrimaryWindow>>,
    monitors: Query<&bevy::window::Monitor, With<bevy::window::PrimaryMonitor>>,
    mut ui_scale: ResMut<UiScale>,
) {
    // Only run when settings actually changed
    if !settings.is_changed() {
        return;
    }

    info!(
        "Applying graphics settings: render_scale={:.2} ssao={} shadow_quality={:?} foliage_cutout={} bloom={}, shadows={}, atmosphere={}, clouds={}, far_terrain={}, vsync={}, frame_cap={}, display_mode={:?}, resolution={}, tonemapping={:?}, exposure={:.2}",
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
        settings.frame_cap_fps,
        settings.display_mode(),
        settings.display_resolution.label(),
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

    // Apply VSync and the complete persisted display request. The helper is
    // idempotent, so unrelated graphics changes never retrigger an OS
    // fullscreen transition.
    let monitor = monitors.iter().next();
    for mut window in windows.iter_mut() {
        crate::app_wiring::apply_window_mode(
            &mut window,
            &mut ui_scale,
            monitor,
            settings.display_mode(),
            settings.display_resolution,
        );
        window.present_mode = if settings.vsync_enabled {
            PresentMode::AutoVsync
        } else {
            PresentMode::AutoNoVsync
        };

        info!(
            "Window request: mode={:?} selected={} physical_now={}x{} logical_now={:.0}x{:.0} scale(base={:.2} override={:?}) ui_scale={:.2}",
            window.mode,
            settings.displayed_resolution_label(monitor),
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
/// prop casters are culled by the prop LOD system, which also keeps the shadow
/// passes affordable at map scale.
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
            exclusive_fullscreen_enabled: false,
            display_resolution: DisplayResolution::new(1280, 720),
            view_distance: 12,
            ..Default::default()
        };
        let text = ron::ser::to_string_pretty(&settings, ron::ser::PrettyConfig::default())
            .expect("serialize");
        let parsed: GraphicsSettings = ron::from_str(&text).expect("parse");
        assert_eq!(parsed.render_scale, 0.6);
        assert!(!parsed.fullscreen_enabled);
        assert_eq!(parsed.display_mode(), DisplayMode::Windowed);
        assert_eq!(parsed.display_resolution, DisplayResolution::new(1280, 720));
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
        assert_eq!(parsed.display_mode(), DisplayMode::Borderless);
        assert_eq!(
            parsed.display_resolution,
            DisplayResolution::new(LAUNCHER_RESOLUTION.0, LAUNCHER_RESOLUTION.1)
        );
    }

    #[test]
    fn legacy_fullscreen_preference_maps_into_the_three_display_modes() {
        let windowed: GraphicsSettings =
            ron::from_str("(fullscreen_enabled: false)").expect("parse old windowed settings");
        let borderless: GraphicsSettings =
            ron::from_str("(fullscreen_enabled: true)").expect("parse old fullscreen settings");
        assert_eq!(windowed.display_mode(), DisplayMode::Windowed);
        assert_eq!(borderless.display_mode(), DisplayMode::Borderless);

        let mut settings = borderless;
        settings.set_display_mode(DisplayMode::ExclusiveFullscreen);
        assert!(settings.fullscreen_enabled);
        assert!(settings.exclusive_fullscreen_enabled);
        assert_eq!(settings.display_mode(), DisplayMode::ExclusiveFullscreen);
    }

    #[test]
    fn exclusive_resolution_list_contains_only_monitor_video_modes() {
        let monitor = Monitor {
            name: None,
            physical_height: 1440,
            physical_width: 2560,
            physical_position: IVec2::ZERO,
            refresh_rate_millihertz: Some(60_000),
            scale_factor: 1.0,
            video_modes: vec![
                VideoMode {
                    physical_size: UVec2::new(1920, 1080),
                    bit_depth: 24,
                    refresh_rate_millihertz: 60_000,
                },
                VideoMode {
                    physical_size: UVec2::new(1920, 1080),
                    bit_depth: 30,
                    refresh_rate_millihertz: 120_000,
                },
                VideoMode {
                    physical_size: UVec2::new(2560, 1440),
                    bit_depth: 30,
                    refresh_rate_millihertz: 60_000,
                },
            ],
        };
        assert_eq!(
            available_display_resolutions(
                DisplayMode::ExclusiveFullscreen,
                Some(&monitor),
                DisplayResolution::new(1600, 900),
            ),
            vec![
                DisplayResolution::new(1920, 1080),
                DisplayResolution::new(2560, 1440),
            ]
        );
        let best = best_fullscreen_video_mode(&monitor, DisplayResolution::new(1920, 1080))
            .expect("supported mode");
        assert_eq!(best.refresh_rate_millihertz, 120_000);
    }

    #[test]
    fn unconfirmed_display_change_restores_the_last_safe_settings() {
        let mut original = GraphicsSettings::default();
        original.set_display_mode(DisplayMode::Windowed);
        original.display_resolution = DisplayResolution::new(1600, 900);
        let mut candidate = original.clone();
        candidate.set_display_mode(DisplayMode::ExclusiveFullscreen);
        candidate.display_resolution = DisplayResolution::new(1920, 1080);

        let mut pending = PendingDisplayChange::new(&original);
        pending.seconds_left = 0.01;
        let mut app = App::new();
        app.init_resource::<Time>();
        app.insert_resource(candidate);
        app.insert_resource(pending);
        app.add_systems(Update, tick_display_change_confirmation);
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs_f32(0.1));
        app.update();

        let restored = app.world().resource::<GraphicsSettings>();
        assert_eq!(restored.display_mode(), DisplayMode::Windowed);
        assert_eq!(
            restored.display_resolution,
            DisplayResolution::new(1600, 900)
        );
        assert!(!app.world().contains_resource::<PendingDisplayChange>());
    }
}
