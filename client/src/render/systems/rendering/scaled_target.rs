//! Scaled offscreen scene target with native-resolution presentation.
//!
//! The 3D camera renders into an offscreen image sized
//! `window_physical_size * render_scale`. A separate 2D camera draws that image
//! as a fullscreen UI node (bilinear upscale) and hosts all game UI at native
//! resolution. On Retina MacBooks this is the single biggest GPU lever: every
//! fullscreen pass (main, prepass, SSAO, bloom, atmosphere, tonemap) scales
//! with the square of `render_scale`, while text/HUD stay crisp.

use super::*;
use bevy::camera::{ImageRenderTarget, RenderTarget};
use bevy::render::render_resource::TextureFormat;
use bevy::ui::widget::NodeImageMode;
use bevy::ui::{GlobalZIndex, IsDefaultUiCamera};

/// Handle to the offscreen image the 3D scene renders into.
#[derive(Resource, Clone)]
pub struct SceneRenderTarget {
    pub image: Handle<Image>,
}

/// Marker for the native-resolution 2D camera that presents the scene + UI.
#[derive(Component)]
pub struct PresentCamera;

/// Marker for the fullscreen node displaying the scaled scene.
#[derive(Component)]
pub struct PresentSurface;

pub(super) fn scaled_target_extent(window: &Window, render_scale: f32) -> Extent3d {
    let scale = render_scale.clamp(0.5, 1.0);
    Extent3d {
        width: ((window.physical_width() as f32 * scale).round() as u32).max(1),
        height: ((window.physical_height() as f32 * scale).round() as u32).max(1),
        depth_or_array_layers: 1,
    }
}

/// Create the offscreen target and the present camera + fullscreen surface.
/// Returns the image handle for the 3D camera's render target.
pub(super) fn setup_scene_render_target(
    commands: &mut Commands,
    images: &mut Assets<Image>,
    window: Option<&Window>,
    render_scale: f32,
) -> Handle<Image> {
    let extent = window
        .map(|window| scaled_target_extent(window, render_scale))
        .unwrap_or(Extent3d {
            width: LAUNCHER_RESOLUTION.0,
            height: LAUNCHER_RESOLUTION.1,
            depth_or_array_layers: 1,
        });

    let image = images.add(Image::new_target_texture(
        extent.width,
        extent.height,
        TextureFormat::Rgba8UnormSrgb,
        None,
    ));

    commands.insert_resource(SceneRenderTarget {
        image: image.clone(),
    });

    // Native-resolution present camera. Renders after the 3D camera and hosts
    // all UI (IsDefaultUiCamera) so HUD/menus stay sharp at any render scale.
    commands.spawn((
        PresentCamera,
        Camera2d,
        Camera {
            order: 10,
            ..default()
        },
        Msaa::Off,
        IsDefaultUiCamera,
    ));

    // Fullscreen upscale surface, kept behind every other UI root.
    commands.spawn((
        PresentSurface,
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            top: Val::Px(0.0),
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..default()
        },
        ImageNode::new(image.clone()).with_mode(NodeImageMode::Stretch),
        GlobalZIndex(crate::ui::foundation::layer::PRESENT_SURFACE),
    ));

    image
}

/// Keep the offscreen target sized to `window_physical * render_scale`.
/// Runs every frame but only touches the asset when the size actually changes
/// (window resize, fullscreen transition, or render-scale setting change).
pub fn sync_scene_render_target(
    windows: Query<&Window, With<PrimaryWindow>>,
    target: Option<Res<SceneRenderTarget>>,
    settings: Res<GraphicsSettings>,
    mut images: ResMut<Assets<Image>>,
    mut applied: Local<Option<Extent3d>>,
) {
    let Some(target) = target else {
        return;
    };
    let Ok(window) = windows.single() else {
        return;
    };

    let desired = scaled_target_extent(window, settings.render_scale);
    if *applied == Some(desired) {
        return;
    }

    let Some(mut image) = images.get_mut(&target.image) else {
        return;
    };
    if image.width() != desired.width || image.height() != desired.height {
        image.resize(desired);
        info!(
            "Scene render target resized to {}x{} (window {}x{} @ scale {:.2})",
            desired.width,
            desired.height,
            window.physical_width(),
            window.physical_height(),
            settings.render_scale
        );
    }
    *applied = Some(desired);
}

/// Build the `RenderTarget` pointing the 3D camera at the offscreen image.
pub(super) fn scene_camera_target(image: Handle<Image>) -> RenderTarget {
    RenderTarget::Image(ImageRenderTarget {
        handle: image,
        scale_factor: 1.0,
    })
}
