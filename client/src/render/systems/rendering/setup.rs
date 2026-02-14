//! setup systems.

use super::atmosphere::{
    clear_atmosphere_preset, default_bloom_settings, desert_atmosphere_preset,
    desert_atmosphere_settings_perf, scattering_medium_from_preset, AtmosphereMedia,
};
use super::clouds::{setup_cloud_layers, CloudCover, CloudCoverOverride, CloudMaterialCache};
use super::*;

/// One-time rendering setup.
pub fn setup_rendering(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    settings: Res<GraphicsSettings>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut scattering_media: ResMut<Assets<ScatteringMedium>>,
) {
    // Performance: directional light shadows are expensive, especially with multiple cascades.
    // Use a moderate shadow map size to balance quality and cost.
    commands.insert_resource(DirectionalLightShadowMap { size: 1024 });

    // With Atmosphere enabled, the sky is rendered procedurally, so ClearColor is mostly a fallback.
    commands.insert_resource(ClearColor(Color::BLACK));

    let clear_preset = clear_atmosphere_preset();
    // Use the dusty preset only around sunrise/sunset via blending.
    let dusty_preset = desert_atmosphere_preset();
    let active_medium = scattering_media.add(scattering_medium_from_preset(
        clear_preset,
        "clear_atmosphere",
    ));
    commands.insert_resource(AtmosphereMedia {
        clear: clear_preset,
        dusty: dusty_preset,
        active_medium: active_medium.clone(),
        last_blend: -1.0,
    });

    let color_grading = ColorGrading::with_identical_sections(
        ColorGradingGlobal {
            exposure: settings.grade_exposure.clamp(-2.0, 2.0),
            ..default()
        },
        ColorGradingSection::default(),
    );

    let mut camera = commands.spawn((
        Camera3d::default(),
        Hdr,
        // Performance: Disable MSAA (big win, we use bloom/tonemapping for quality)
        Msaa::Off,
        // Nice highlights rolloff for HDR outdoor scenes.
        settings.tonemapping,
        // Physical light levels (RAW_SUNLIGHT) are bright: slightly higher exposure for harsh desert sun
        Exposure::SUNLIGHT,
        // Color grading for midtone lift (pleasant readability)
        color_grading,
        // Clear blue atmosphere (dust tint only near sunrise/sunset).
        Atmosphere {
            bottom_radius: clear_preset.bottom_radius,
            top_radius: clear_preset.top_radius,
            ground_albedo: clear_preset.ground_albedo,
            medium: active_medium,
        },
        // Atmosphere settings (perf-tuned LUT sizes/samples).
        desert_atmosphere_settings_perf(),
        // Bloom for that blazing desert sun glow
        // Tuned for Metal stability (lower intensity + Additive mode to avoid flickering on M-series)
        default_bloom_settings(),
        // Let the atmosphere drive ambient/IBL lighting for this view.
        // Performance: Tiny cubemap - we mostly use the sun anyway
        AtmosphereEnvironmentMapLight {
            intensity: 1.0,
            affects_lightmapped_mesh_diffuse: true,
            size: UVec2::new(64, 64), // Reduced from 128 for perf
        },
        Transform::from_xyz(0.0, 10.0, 5.0).looking_at(Vec3::ZERO, Vec3::Y),
        // Explicit spatial + visibility components (camera is parent of the first-person weapon model).
        GlobalTransform::default(),
        Visibility::default(),
        InheritedVisibility::default(),
        // Audio listener for spatial audio (other players' sounds)
        // SpatialListener defines where the "ears" are relative to the entity
        SpatialListener::new(0.1), // ~10cm between ears
    ));
    // Keep this out of the large tuple to avoid tuple-size bundle limits.
    camera.insert(crate::render::sniper_fisheye::SniperFisheye::default());

    setup_cloud_layers(&mut commands, &asset_server, &mut meshes, &mut materials);
    commands.insert_resource(CloudCover::default());
    commands.insert_resource(CloudCoverOverride::default());
    commands.insert_resource(CloudMaterialCache::default());

    info!("Client rendering initialized with clear sky + dusty sunsets");
}
