//! setup systems.

use super::atmosphere::{
    clear_atmosphere_preset, default_bloom_settings, desert_atmosphere_preset,
    desert_atmosphere_settings_perf, scattering_medium_from_preset, AtmosphereMedia,
};
use super::clouds::{CloudCover, CloudCoverOverride};
use super::*;

/// Near clip plane for the main 3D camera. Small so geometry close to the camera does
/// not clip out when zoomed in.
const CAMERA_NEAR_CLIP: f32 = 0.001;

/// One-time rendering setup.
pub fn setup_rendering(
    mut commands: Commands,
    settings: Res<GraphicsSettings>,
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
        // Out-of-range sentinel: the medium key spans [-1, 1] (dust - night),
        // so -1.0 would read as "deep night already built" on a night join.
        last_blend: -10.0,
    });

    let color_grading =
        super::settings::default_color_grading(settings.grade_exposure.clamp(-2.0, 2.0));

    // Clear blue atmosphere (dust tint only near sunrise/sunset). Since bevy 0.19 the
    // atmosphere is its own entity (the camera picks up the nearest one), and its
    // GlobalTransform is the PLANET CENTER. Sea level (y = 0) must sit on the planet
    // surface, so the center goes one bottom_radius straight down — an identity
    // transform would put the planet center in the middle of the map, snapping the
    // sky's up-axis to `normalize(camera_pos)` and anchoring every scattering
    // feature to the map origin. Both presets share bottom_radius (the per-frame
    // blend never moves it), so this anchor is fixed at spawn. AtmosphereSettings
    // stays on the camera below and is what enables the effect per view.
    commands.spawn((
        Atmosphere {
            inner_radius: clear_preset.bottom_radius,
            outer_radius: clear_preset.top_radius,
            ground_albedo: clear_preset.ground_albedo,
            medium: active_medium,
        },
        Transform::from_xyz(0.0, -clear_preset.bottom_radius, 0.0),
    ));

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
        // Placeholder fog until WorldTime replicates: update_day_night_cycle is
        // the single writer for DistanceFog and replaces color/visibility with
        // its own time-of-day curves on its first run — tune fog THERE.
        DistanceFog {
            color: Color::srgba(0.72, 0.82, 0.92, 0.05),
            directional_light_color: Color::srgba(1.0, 0.94, 0.82, 0.22),
            directional_light_exponent: 20.0,
            falloff: FogFalloff::from_visibility_colors(
                2000.0,
                Color::srgb(0.70, 0.80, 0.90),
                Color::srgb(0.88, 0.92, 0.96),
            ),
        },
        // Measurement verdict (2026-08-19, M-series, 1920x1200, real connected
        // game): Gaussian's wide per-fragment PCF kernel held the steady state
        // at ~56-72 fps; Hardware2x2 ran ~87-99 fps — about +50% — and at RTS
        // zooms the shadow edges read equally well (arguably crisper). Default
        // is Hardware2x2; FISTFORCE_SHADOW_FILTER=gaussian is the ablation
        // hook to compare the soft filter again.
        if std::env::var("FISTFORCE_SHADOW_FILTER").is_ok_and(|v| v == "gaussian") {
            ShadowFilteringMethod::Gaussian
        } else {
            ShadowFilteringMethod::Hardware2x2
        },
    ));
    // SSAO is opt-in: a fullscreen AO pass plus a depth/normal prepass is a
    // heavy default on integrated GPUs.
    if settings.ssao_enabled {
        camera.insert(super::settings::default_ssao_settings());
    }
    // Keep this out of the large tuple to avoid tuple-size bundle limits.

    // init (not insert): the capture harness pre-seeds forced weather in its own
    // Startup system, and a blind insert here clobbers it (flush order between
    // unordered Startup systems put this one last).
    commands.init_resource::<CloudCover>();
    commands.init_resource::<CloudCoverOverride>();

    info!("Client rendering initialized with clear sky + dusty sunsets");
}
