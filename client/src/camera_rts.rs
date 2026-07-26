//! Top-down / RTS commander camera.
//!
//! WASD pan (yaw-relative, zoom-scaled), RMB drag to orbit yaw, wheel zoom, and
//! terrain-following focus clamped to the active map bounds. Also owns the
//! cursor→terrain picking ray used for issuing orders.
//!
//! Harvested from the rail prototype so it outlives that module.

use bevy::input::mouse::{MouseMotion, MouseScrollUnit, MouseWheel};
use bevy::math::Ray3d;
use bevy::prelude::*;
use bevy::window::{CursorOptions, PrimaryWindow};

use lightyear::prelude::*;
use shared::protocol::{InputChannel, PlayerInput};
use shared::terrain::WorldTerrain;

/// Resend the view at least this often even when the camera is still, so a dropped
/// packet cannot strand the server's streaming anchor.
const VIEW_HEARTBEAT_SECS: f32 = 0.25;

/// Network peer id of the local client, published on connect.
///
/// Not read yet — it is the hook for "which units are mine" once unit ownership exists.
#[allow(dead_code)]
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct LocalPeerId(pub u64);

/// World-space point currently under the mouse cursor, if it hits terrain.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct CursorTerrainHit(pub Option<Vec3>);

/// Orbit/pan camera controller for the top-down view.
#[derive(Component)]
pub struct CommanderCamera {
    pub yaw: f32,
    pub focus: Vec3,
    pub pan_speed: f32,
    pub zoom: f32,
    pub zoom_min: f32,
    pub zoom_max: f32,
    pub zoom_speed: f32,
    pub tilt: f32,
    pub look_sensitivity: f32,
}

impl Default for CommanderCamera {
    fn default() -> Self {
        Self {
            yaw: -0.45,
            focus: Vec3::ZERO,
            pan_speed: 120.0,
            zoom: 280.0,
            zoom_min: 55.0,
            zoom_max: 900.0,
            zoom_speed: 22.0,
            tilt: 0.92,
            look_sensitivity: 0.0022,
        }
    }
}

pub fn ensure_commander_camera_controller(
    mut commands: Commands,
    mut cameras: Query<(Entity, &mut Transform), (With<Camera3d>, Without<CommanderCamera>)>,
    terrain: Option<Res<WorldTerrain>>,
) {
    for (entity, mut transform) in cameras.iter_mut() {
        let mut controller = CommanderCamera::default();
        if let Some(terrain) = terrain.as_deref() {
            controller.focus.y = terrain.get_height(controller.focus.x, controller.focus.z);
        }
        apply_commander_transform(&mut transform, &controller, terrain.as_deref());
        commands.entity(entity).insert(controller);
    }
}

pub fn update_commander_camera(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    mut mouse_motion: MessageReader<MouseMotion>,
    mut mouse_wheel: MessageReader<MouseWheel>,
    terrain: Option<Res<WorldTerrain>>,
    mut cameras: Query<(&mut Transform, &mut CommanderCamera), With<Camera3d>>,
) {
    let Ok((mut transform, mut controller)) = cameras.single_mut() else {
        return;
    };

    let mut look_delta = Vec2::ZERO;
    for event in mouse_motion.read() {
        look_delta += event.delta;
    }

    if mouse_buttons.pressed(MouseButton::Right) {
        controller.yaw -= look_delta.x * controller.look_sensitivity;
    }

    let mut scroll_lines = 0.0;
    for event in mouse_wheel.read() {
        let factor = match event.unit {
            MouseScrollUnit::Line => 1.0,
            MouseScrollUnit::Pixel => 0.05,
        };
        scroll_lines += event.y * factor;
    }
    if scroll_lines.abs() > f32::EPSILON {
        controller.zoom = (controller.zoom - scroll_lines * controller.zoom_speed)
            .clamp(controller.zoom_min, controller.zoom_max);
    }

    let mut pan_input = Vec2::ZERO;
    if keys.pressed(KeyCode::KeyW) {
        pan_input.y += 1.0;
    }
    if keys.pressed(KeyCode::KeyS) {
        pan_input.y -= 1.0;
    }
    if keys.pressed(KeyCode::KeyA) {
        pan_input.x -= 1.0;
    }
    if keys.pressed(KeyCode::KeyD) {
        pan_input.x += 1.0;
    }

    if pan_input != Vec2::ZERO {
        pan_input = pan_input.normalize();
        let yaw_rotation = Quat::from_axis_angle(Vec3::Y, controller.yaw);
        let forward = (yaw_rotation * Vec3::NEG_Z).with_y(0.0).normalize_or_zero();
        let right = (yaw_rotation * Vec3::X).with_y(0.0).normalize_or_zero();
        let speed_mult = if keys.pressed(KeyCode::ShiftLeft) {
            2.4
        } else {
            1.0
        };
        let zoom_scale = (controller.zoom / 260.0).clamp(0.45, 3.4);
        let pan_speed = controller.pan_speed;
        controller.focus += (forward * pan_input.y + right * pan_input.x)
            * pan_speed
            * time.delta_secs()
            * speed_mult
            * zoom_scale;
    }

    if let Some(terrain) = terrain.as_deref() {
        let bounds = terrain.generator.active_map_bounds();
        controller.focus.x = controller.focus.x.clamp(bounds.min[0], bounds.max[0]);
        controller.focus.z = controller.focus.z.clamp(bounds.min[1], bounds.max[1]);
    }

    apply_commander_transform(&mut transform, &controller, terrain.as_deref());
}

pub fn release_cursor_for_rts(
    windows: Query<Entity, With<PrimaryWindow>>,
    mut cursor_opts: Query<&mut CursorOptions>,
) {
    let Ok(window_entity) = windows.single() else {
        return;
    };
    let Ok(mut cursor) = cursor_opts.get_mut(window_entity) else {
        return;
    };
    cursor.visible = true;
    cursor.grab_mode = bevy::window::CursorGrabMode::None;
}

pub fn update_cursor_terrain_hit(
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<Camera3d>>,
    terrain: Res<WorldTerrain>,
    mut hit: ResMut<CursorTerrainHit>,
) {
    let Ok(window) = windows.single() else {
        hit.0 = None;
        return;
    };
    let Some(cursor_pos) = window.cursor_position() else {
        hit.0 = None;
        return;
    };
    let Ok((camera, camera_transform)) = cameras.single() else {
        hit.0 = None;
        return;
    };
    // The 3D camera renders to a scaled offscreen target, so window cursor
    // coordinates must be mapped into the camera's own viewport space.
    let window_size = window.size();
    let Some(viewport_size) = camera.logical_viewport_size() else {
        hit.0 = None;
        return;
    };
    if window_size.x <= 0.0 || window_size.y <= 0.0 {
        hit.0 = None;
        return;
    }
    let viewport_pos = cursor_pos / window_size * viewport_size;
    let Ok(ray) = camera.viewport_to_world(camera_transform, viewport_pos) else {
        hit.0 = None;
        return;
    };

    hit.0 = intersect_terrain(ray, &terrain);
}

fn apply_commander_transform(
    transform: &mut Transform,
    controller: &CommanderCamera,
    terrain: Option<&WorldTerrain>,
) {
    let mut focus = controller.focus;
    if let Some(terrain) = terrain {
        focus.y = terrain.get_height(focus.x, focus.z);
    }
    let rotation = Quat::from_axis_angle(Vec3::Y, controller.yaw)
        * Quat::from_axis_angle(Vec3::X, -controller.tilt);
    let offset = rotation * Vec3::new(0.0, 0.0, controller.zoom);
    transform.translation = focus + offset;
    transform.rotation = rotation;
}

/// March a ray against the heightfield, then binary-search the crossing.
pub fn intersect_terrain(ray: Ray3d, terrain: &WorldTerrain) -> Option<Vec3> {
    const RAY_MAX_DISTANCE: f32 = 5000.0;
    const RAY_STEP: f32 = 10.0;
    const RAY_BINARY_STEPS: usize = 12;

    let origin = ray.origin;
    let dir = ray.direction.as_vec3();
    let bounds = terrain.generator.active_map_bounds();
    let mut prev_t = 0.0;
    let mut prev_f = origin.y - terrain.get_height(origin.x, origin.z);

    let mut t = RAY_STEP;
    while t <= RAY_MAX_DISTANCE {
        let pos = origin + dir * t;
        if bounds.contains_xz(pos.x, pos.z) {
            let f = pos.y - terrain.get_height(pos.x, pos.z);
            if prev_f > 0.0 && f <= 0.0 {
                let mut lo = prev_t;
                let mut hi = t;
                for _ in 0..RAY_BINARY_STEPS {
                    let mid = (lo + hi) * 0.5;
                    let mid_pos = origin + dir * mid;
                    let mid_f = mid_pos.y - terrain.get_height(mid_pos.x, mid_pos.z);
                    if mid_f > 0.0 {
                        lo = mid;
                    } else {
                        hi = mid;
                    }
                }
                let hit = origin + dir * ((lo + hi) * 0.5);
                return Some(Vec3::new(hit.x, terrain.get_height(hit.x, hit.z), hit.z));
            }
            prev_f = f;
        }
        prev_t = t;
        t += RAY_STEP;
    }

    None
}

/// Send the commander's view (focus + yaw) to the server each tick.
///
/// The server anchors terrain-collider streaming on this, so it must keep flowing while
/// the camera pans. Only sends on change (plus a heartbeat) to avoid spamming the link.
pub fn send_commander_view(
    time: Res<Time>,
    cameras: Query<&CommanderCamera>,
    mut clients: Query<&mut MessageSender<PlayerInput>, With<crate::GameClient>>,
    mut last_sent: Local<Option<PlayerInput>>,
    mut heartbeat: Local<f32>,
) {
    let Ok(controller) = cameras.single() else {
        return;
    };
    let Ok(mut sender) = clients.single_mut() else {
        return;
    };

    let view = PlayerInput {
        yaw: controller.yaw,
        focus: controller.focus,
    };

    *heartbeat += time.delta_secs();
    let changed = last_sent.as_ref() != Some(&view);
    if !changed && *heartbeat < VIEW_HEARTBEAT_SECS {
        return;
    }

    *heartbeat = 0.0;
    *last_sent = Some(view.clone());
    sender.send::<InputChannel>(view);
}
