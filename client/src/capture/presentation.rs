//! Capture the real scene-and-UI composition without depending on a swapchain.
//!
//! A locked desktop can return a black window screenshot while the offscreen
//! 3D camera keeps rendering correctly. In the capture app only, the existing
//! presentation camera draws into a native-resolution image. A visible harness
//! window mirrors that image; screenshots always read the owned render target.

use bevy::camera::{ImageRenderTarget, RenderTarget, ShadowLodOrigin};
use bevy::prelude::*;
use bevy::render::render_resource::TextureFormat;
use bevy::ui::widget::NodeImageMode;
use bevy::window::PrimaryWindow;

use super::{CaptureConfig, CaptureTarget};
use crate::render::systems::scaled_target::PresentCamera;

#[derive(Resource)]
pub(super) struct CapturePresentationTarget {
    pub(super) image: Handle<Image>,
}

/// Runs after production startup has created the real presentation camera.
/// Its UI roots, scale, layout and rendering systems stay on that camera.
pub(super) fn setup_capture_presentation(
    mut commands: Commands,
    config: Res<CaptureConfig>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut cameras: Query<(Entity, &mut RenderTarget), With<PresentCamera>>,
    mut images: ResMut<Assets<Image>>,
) {
    // A hidden macOS swapchain may block even with vsync disabled. Timed
    // scene runs must also present into an owned image, so timing reflects
    // the renderer instead of hidden-window drawable availability.
    if config.target != CaptureTarget::Window && !config.benchmark {
        return;
    }
    let Ok(window) = windows.single() else {
        error!("capture: the presentation window is unavailable");
        return;
    };
    let Ok((camera, mut target)) = cameras.single_mut() else {
        error!("capture: the production presentation camera is unavailable");
        return;
    };
    let image = images.add(Image::new_target_texture(
        config.resolution[0],
        config.resolution[1],
        TextureFormat::Rgba8UnormSrgb,
        None,
    ));
    *target = RenderTarget::Image(ImageRenderTarget {
        handle: image.clone(),
        scale_factor: window.scale_factor(),
    });
    // Preserve the window presentation camera's shadow-LOD role now that
    // its render target is an image, including when no mirror window is shown.
    commands.entity(camera).insert(ShadowLodOrigin);
    commands.insert_resource(CapturePresentationTarget {
        image: image.clone(),
    });

    if config.show_window {
        // The mirror has one explicitly targeted UI root. It never becomes
        // the default UI camera or draws back into the captured image.
        let mirror = commands
            .spawn((
                Camera2d,
                Camera {
                    order: 20,
                    ..default()
                },
                Msaa::Off,
            ))
            .id();
        commands.spawn((
            UiTargetCamera(mirror),
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(0.0),
                top: Val::Px(0.0),
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                ..default()
            },
            ImageNode::new(image).with_mode(NodeImageMode::Stretch),
            Pickable::IGNORE,
        ));
    }
}
