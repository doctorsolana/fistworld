//! Rendering systems
//!
//! Atmosphere, day/night cycle, and camera setup.

pub mod atmosphere;
pub mod clouds;
pub mod day_night;
pub mod settings;
pub mod setup;

pub use atmosphere::update_atmosphere;
pub use clouds::{
    apply_cloud_texture_sampler, spawn_cloud_cards, update_cloud_cards, update_cloud_cover,
    update_cloud_layers, CloudCard, CloudCover, CloudCoverMode, CloudCoverOverride, CloudLayer,
};
pub use day_night::update_day_night_cycle;
pub use settings::{apply_graphics_settings, GraphicsSettings, InputSettings, LAUNCHER_RESOLUTION};
pub use setup::setup_rendering;

use bevy::asset::RenderAssetUsages;
use bevy::audio::SpatialListener;
use bevy::camera::Exposure;
use bevy::core_pipeline::tonemapping::Tonemapping;
use bevy::image::{Image, ImageAddressMode, ImageFilterMode, ImageSampler, ImageSamplerDescriptor};
use bevy::light::{
    light_consts::lux, AtmosphereEnvironmentMapLight, DirectionalLightShadowMap, NotShadowCaster,
};
use bevy::math::primitives::Plane3d;
use bevy::pbr::{
    Atmosphere, AtmosphereMode, AtmosphereSettings, Falloff, PhaseFunction, ScatteringMedium,
    ScatteringTerm,
};
use bevy::post_process::bloom::{Bloom, BloomCompositeMode};
use bevy::prelude::*;
use bevy::render::render_resource::{Extent3d, TextureDimension, TextureFormat};
use bevy::render::view::{ColorGrading, ColorGradingGlobal, ColorGradingSection, Hdr, Msaa};
use bevy::ui::UiScale;
use bevy::window::{PresentMode, PrimaryWindow};
use noise::{Fbm, MultiFractal, NoiseFn, Perlin};
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

// =============================================================================
// COMPONENTS
// =============================================================================

/// Marker for the sun directional light (driven by day/night cycle)
#[derive(Component)]
pub struct SunLight;

/// Marker for the fill directional light (shadow lift / readability)
#[derive(Component)]
pub struct FillLight;
