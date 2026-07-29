//! Rendering systems
//!
//! Atmosphere, day/night cycle, and camera setup.

pub mod atmosphere;
pub mod cloud_layer;
pub mod cloud_shadows;
pub mod clouds;
pub mod day_night;
pub mod scaled_target;
pub mod settings;
pub mod setup;

pub use atmosphere::{sync_atmosphere_enabled, update_atmosphere};
pub use cloud_layer::{spawn_cloud_plane, update_cloud_plane, CloudLayerMaterial, CloudLayerPlane};
pub use cloud_shadows::sync_cloud_shadow_params;
pub use clouds::{
    apply_cloud_texture_sampler, update_cloud_cover, update_cloud_layers, CloudCover,
    CloudCoverMode, CloudCoverOverride, CloudLayer,
};
pub use day_night::update_day_night_cycle;
pub use scaled_target::sync_scene_render_target;
pub use settings::{
    apply_graphics_settings, save_graphics_settings, sync_shadow_cascades_to_zoom,
    GraphicsSettings, InputSettings, LAUNCHER_RESOLUTION,
};
pub use setup::setup_rendering;

use bevy::audio::SpatialListener;
use bevy::camera::{Exposure, Hdr};
use bevy::core_pipeline::prepass::{DepthPrepass, NormalPrepass};
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::image::{Image, ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::light::atmosphere::{Falloff, PhaseFunction, ScatteringMedium, ScatteringTerm};
use bevy::light::{
    light_consts::lux, Atmosphere, AtmosphereEnvironmentMapLight, CascadeShadowConfig,
    CascadeShadowConfigBuilder, DirectionalLightShadowMap, NotShadowCaster, ShadowFilteringMethod,
};
use bevy::math::primitives::Plane3d;
use bevy::pbr::{
    AtmosphereMode, AtmosphereSettings, DistanceFog, FogFalloff, ScreenSpaceAmbientOcclusion,
    ScreenSpaceAmbientOcclusionQualityLevel,
};
use bevy::post_process::bloom::{Bloom, BloomCompositeMode};
use bevy::prelude::*;
use bevy::render::render_resource::Extent3d;
use bevy::render::view::{ColorGrading, ColorGradingGlobal, ColorGradingSection, Msaa};
use bevy::ui::UiScale;
use bevy::window::{PresentMode, PrimaryWindow, WindowMode};

// =============================================================================
// COMPONENTS
// =============================================================================

/// Marker for the sun directional light (driven by day/night cycle)
#[derive(Component)]
pub struct SunLight;

/// Marker for the fill directional light (shadow lift / readability)
#[derive(Component)]
pub struct FillLight;
