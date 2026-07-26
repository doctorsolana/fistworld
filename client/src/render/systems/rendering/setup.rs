//! setup systems.

use super::atmosphere::{
    clear_atmosphere_preset, default_bloom_settings, desert_atmosphere_preset,
    desert_atmosphere_settings_perf, scattering_medium_from_preset, AtmosphereMedia,
};
use super::clouds::{setup_cloud_layers, CloudCover, CloudCoverOverride, CloudMaterialCache};
use super::*;

/// Near clip plane for the main 3D camera. Small so geometry close to the camera does
/// not clip out when zoomed in.
const CAMERA_NEAR_CLIP: f32 = 0.001;

/// One-time rendering setup.
pub fn setup_rendering(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    settings: Res<GraphicsSettings>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    mut scattering_media: ResMut<Assets<ScatteringMedium>>,
    mut images: ResMut<Assets<Image>>,
    windows: Query<&Window, With<PrimaryWindow>>,
) {
    // Shadow map resolution follows the shadow quality setting.
    commands.insert_resource(DirectionalLightShadowMap {
        size: settings.shadow_quality.shadow_map_size(),
    });

    // Offscreen scene target (render_scale) + native-res present camera/UI.
    let scene_target_image = super::scaled_target::setup_scene_render_target(
        &mut commands,
        &mut images,
        windows.single().ok(),
        settings.render_scale,
    );

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

    let color_grading =
        super::settings::default_color_grading(settings.grade_exposure.clamp(-2.0, 2.0));

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
    // Render the 3D scene into the scaled offscreen target; the present camera
    // upscales it to the window at native resolution.
    camera.insert(super::scaled_target::scene_camera_target(
        scene_target_image,
    ));
    camera.insert(Projection::Perspective(PerspectiveProjection {
        near: CAMERA_NEAR_CLIP,
        ..default()
    }));
    camera.insert((
        DistanceFog {
            color: Color::srgba(0.56, 0.61, 0.67, 0.12),
            directional_light_color: Color::srgba(1.0, 0.88, 0.68, 0.18),
            directional_light_exponent: 24.0,
            falloff: FogFalloff::from_visibility_colors(
                900.0,
                Color::srgb(0.50, 0.53, 0.55),
                Color::srgb(0.74, 0.76, 0.72),
            ),
        },
        // Keep the quality-focused default explicit so future tuning does not fall back to blockier PCF.
        ShadowFilteringMethod::Gaussian,
    ));
    // SSAO is opt-in: a fullscreen AO pass plus a depth/normal prepass is a
    // heavy default on integrated GPUs.
    if settings.ssao_enabled {
        camera.insert(super::settings::default_ssao_settings());
    }
    // Keep this out of the large tuple to avoid tuple-size bundle limits.

    setup_cloud_layers(&mut commands, &asset_server, &mut meshes, &mut materials);
    commands.insert_resource(CloudCover::default());
    commands.insert_resource(CloudCoverOverride::default());
    commands.insert_resource(CloudMaterialCache::default());

    info!("Client rendering initialized with clear sky + dusty sunsets");
}
