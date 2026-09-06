//! Capture the real scene-and-UI composition without depending on a swapchain.
//!
//! A locked desktop can return a black window screenshot while the offscreen
//! 3D camera keeps rendering correctly. In opt-in capture runs, the existing
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
pub(crate) struct CapturePresentationTarget {
    pub(super) image: Handle<Image>,
}

/// Runs after production startup has created the real presentation camera.
/// Its UI roots, scale, layout and rendering systems stay on that camera.
pub(crate) fn setup_capture_presentation(
    mut commands: Commands,
    config: Option<Res<CaptureConfig>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut cameras: Query<(Entity, &mut RenderTarget), With<PresentCamera>>,
    mut images: ResMut<Assets<Image>>,
) {
    let Ok(window) = windows.single() else {
        return;
    };
    let (resolution, show_window) = if let Some(config) = config {
        if config.target != CaptureTarget::Window && !config.benchmark {
            return;
        }
        (config.resolution, config.show_window)
    } else if std::env::var_os("FISTWORLD_ARMY_SCENARIO").is_some() {
        ([window.physical_width(), window.physical_height()], true)
    } else {
        return;
    };
    let Ok((camera, mut target)) = cameras.single_mut() else {
        error!("capture: the production presentation camera is unavailable");
        return;
    };
    let image = images.add(Image::new_target_texture(
        resolution[0],
        resolution[1],
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

    if show_window {
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

/// Connected labs start in the launcher and can enter a different fullscreen
/// resolution on connect. Keep their mirror at the real window size; otherwise
/// native UI scale is applied to the old launcher image and clips the controls.
/// Offline scenarios retain their explicitly requested artifact resolution.
pub(crate) fn sync_capture_presentation(
    config: Option<Res<CaptureConfig>>,
    target: Option<Res<CapturePresentationTarget>>,
    windows: Query<&Window, With<PrimaryWindow>>,
    mut cameras: Query<&mut RenderTarget, With<PresentCamera>>,
    mut images: ResMut<Assets<Image>>,
) {
    if config.is_some() {
        return;
    }
    let (Some(target), Ok(window)) = (target, windows.single()) else {
        return;
    };
    let size = UVec2::new(window.physical_width(), window.physical_height());
    if size.min_element() == 0 {
        return;
    }
    if let Some(image) = images.get(&target.image) {
        if image.size() != size {
            let mut extent = image.texture_descriptor.size;
            extent.width = size.x;
            extent.height = size.y;
            images.get_mut(&target.image).unwrap().resize(extent);
        }
    }
    for mut camera in &mut cameras {
        if let RenderTarget::Image(image) = &*camera {
            if image.handle == target.image && image.scale_factor != window.scale_factor() {
                *camera = RenderTarget::Image(ImageRenderTarget {
                    handle: target.image.clone(),
                    scale_factor: window.scale_factor(),
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connected_mirror_follows_fullscreen_resize_and_dpi() {
        let mut app = App::new();
        app.init_resource::<Assets<Image>>()
            .add_systems(Update, sync_capture_presentation);
        let image = app
            .world_mut()
            .resource_mut::<Assets<Image>>()
            .add(Image::new_target_texture(
                1600,
                900,
                TextureFormat::Rgba8UnormSrgb,
                None,
            ));
        app.insert_resource(CapturePresentationTarget {
            image: image.clone(),
        });
        let window = app
            .world_mut()
            .spawn((Window::default(), PrimaryWindow))
            .id();
        let camera = app
            .world_mut()
            .spawn((
                PresentCamera,
                RenderTarget::Image(ImageRenderTarget {
                    handle: image.clone(),
                    scale_factor: 1.0,
                }),
            ))
            .id();
        for (width, height, scale) in [(2940, 1846, 1.0), (1280, 720, 1.5)] {
            {
                let mut window = app.world_mut().get_mut::<Window>(window).unwrap();
                window.resolution.set_scale_factor_override(Some(scale));
                window.resolution.set_physical_resolution(width, height);
            }
            app.update();
            assert_eq!(
                app.world()
                    .resource::<Assets<Image>>()
                    .get(&image)
                    .unwrap()
                    .size(),
                UVec2::new(width, height)
            );
            let RenderTarget::Image(target) = app.world().get::<RenderTarget>(camera).unwrap()
            else {
                panic!("image target")
            };
            assert_eq!(target.scale_factor, scale);
        }
        app.world_mut().clear_trackers();
        app.update();
        assert_eq!(
            app.world_mut()
                .query_filtered::<Entity, Changed<RenderTarget>>()
                .iter(app.world())
                .count(),
            0
        );
    }
}
