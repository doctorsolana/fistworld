use bevy::input::mouse::MouseMotion;
use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::prelude::*;

use shared::terrain::WorldTerrain;

use crate::city::CityEditorState;
use crate::session::{EditorMainCamera, EditorUiState, ToolMode};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorCameraMode {
    Rts,
    Free,
}

#[derive(Component)]
pub struct EditorCameraController {
    pub mode: EditorCameraMode,
    pub last_mode: EditorCameraMode,
    pub yaw: f32,
    pub pitch: f32,
    pub free_speed: f32,
    pub look_sensitivity: f32,
    pub rts_focus: Vec3,
    pub rts_pan_speed: f32,
    pub rts_zoom: f32,
    pub rts_zoom_min: f32,
    pub rts_zoom_max: f32,
    pub rts_zoom_speed: f32,
    pub rts_tilt: f32,
}

impl Default for EditorCameraController {
    fn default() -> Self {
        Self {
            mode: EditorCameraMode::Rts,
            last_mode: EditorCameraMode::Rts,
            yaw: -0.5,
            pitch: -0.35,
            free_speed: 80.0,
            look_sensitivity: 0.0025,
            rts_focus: Vec3::ZERO,
            rts_pan_speed: 110.0,
            rts_zoom: 260.0,
            rts_zoom_min: 35.0,
            rts_zoom_max: 650.0,
            rts_zoom_speed: 18.0,
            rts_tilt: 0.88,
        }
    }
}

pub fn spawn_editor_camera(mut commands: Commands) {
    let controller = EditorCameraController::default();
    let mut transform = Transform::IDENTITY;
    apply_rts_transform(&mut transform, &controller, None);

    commands.spawn((
        Name::new("EditorCamera"),
        Camera3d::default(),
        transform,
        controller,
        EditorMainCamera,
    ));
}

pub fn update_editor_camera(
    time: Res<Time>,
    keys: Res<ButtonInput<KeyCode>>,
    mouse_buttons: Res<ButtonInput<MouseButton>>,
    mut mouse_motion: MessageReader<MouseMotion>,
    mut mouse_wheel: MessageReader<MouseWheel>,
    ui_state: Res<EditorUiState>,
    city_state: Option<Res<CityEditorState>>,
    terrain: Option<Res<WorldTerrain>>,
    mut query: Query<(&mut Transform, &mut EditorCameraController), With<EditorMainCamera>>,
) {
    let Ok((mut transform, mut controller)) = query.single_mut() else {
        return;
    };

    let mut look_delta = Vec2::ZERO;
    for event in mouse_motion.read() {
        look_delta += event.delta;
    }

    let mut scroll_lines = 0.0;
    let allow_camera_scroll = !ui_state.pointer_over_ui;
    let road_tool_consumes_rmb = city_state
        .as_deref()
        .map(|state| ui_state.tool == ToolMode::Road && !state.draft_road_points.is_empty())
        .unwrap_or(false);
    for event in mouse_wheel.read() {
        let factor = match event.unit {
            MouseScrollUnit::Line => 1.0,
            MouseScrollUnit::Pixel => 0.05,
        };
        if allow_camera_scroll {
            scroll_lines += event.y * factor;
        }
    }

    if keys.just_pressed(KeyCode::F5) {
        controller.mode = match controller.mode {
            EditorCameraMode::Rts => EditorCameraMode::Free,
            EditorCameraMode::Free => EditorCameraMode::Rts,
        };
    }

    let terrain_ref = terrain.as_deref();
    if controller.mode != controller.last_mode {
        handle_mode_transition(&mut transform, &mut controller, terrain_ref);
        controller.last_mode = controller.mode;
    }

    match controller.mode {
        EditorCameraMode::Rts => {
            update_rts_camera(
                &mut transform,
                &mut controller,
                terrain_ref,
                &keys,
                &mouse_buttons,
                road_tool_consumes_rmb,
                look_delta,
                scroll_lines,
                time.delta_secs(),
            );
        }
        EditorCameraMode::Free => {
            update_free_camera(
                &mut transform,
                &mut controller,
                &keys,
                &mouse_buttons,
                road_tool_consumes_rmb,
                look_delta,
                time.delta_secs(),
            );
        }
    }
}

fn update_free_camera(
    transform: &mut Transform,
    controller: &mut EditorCameraController,
    keys: &ButtonInput<KeyCode>,
    mouse_buttons: &ButtonInput<MouseButton>,
    road_tool_consumes_rmb: bool,
    look_delta: Vec2,
    dt: f32,
) {
    if mouse_buttons.pressed(MouseButton::Right) && !road_tool_consumes_rmb {
        controller.yaw -= look_delta.x * controller.look_sensitivity;
        controller.pitch =
            (controller.pitch - look_delta.y * controller.look_sensitivity).clamp(-1.45, 1.45);
    }

    let yaw_rotation = Quat::from_axis_angle(Vec3::Y, controller.yaw);
    let pitch_rotation = Quat::from_axis_angle(Vec3::X, controller.pitch);
    transform.rotation = yaw_rotation * pitch_rotation;

    let mut move_dir = Vec3::ZERO;
    if keys.pressed(KeyCode::KeyW) {
        move_dir.z += 1.0;
    }
    if keys.pressed(KeyCode::KeyS) {
        move_dir.z -= 1.0;
    }
    if keys.pressed(KeyCode::KeyA) {
        move_dir.x -= 1.0;
    }
    if keys.pressed(KeyCode::KeyD) {
        move_dir.x += 1.0;
    }
    if keys.pressed(KeyCode::KeyE) {
        move_dir.y += 1.0;
    }
    if keys.pressed(KeyCode::KeyQ) {
        move_dir.y -= 1.0;
    }

    if move_dir == Vec3::ZERO {
        return;
    }

    let speed_mult = if keys.pressed(KeyCode::ShiftLeft) {
        2.5
    } else {
        1.0
    };
    let forward = transform.forward();
    let right = transform.right();
    let up = Vec3::Y;
    let movement =
        (forward * move_dir.z + right * move_dir.x + up * move_dir.y) * controller.free_speed * dt;
    transform.translation += movement * speed_mult;
}

fn update_rts_camera(
    transform: &mut Transform,
    controller: &mut EditorCameraController,
    terrain: Option<&WorldTerrain>,
    keys: &ButtonInput<KeyCode>,
    mouse_buttons: &ButtonInput<MouseButton>,
    road_tool_consumes_rmb: bool,
    look_delta: Vec2,
    scroll_lines: f32,
    dt: f32,
) {
    if mouse_buttons.pressed(MouseButton::Right) && !road_tool_consumes_rmb {
        controller.yaw -= look_delta.x * controller.look_sensitivity;
    }

    if scroll_lines.abs() > f32::EPSILON {
        controller.rts_zoom = (controller.rts_zoom - scroll_lines * controller.rts_zoom_speed)
            .clamp(controller.rts_zoom_min, controller.rts_zoom_max);
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
        let basis = rts_basis(controller.yaw);
        let mut forward = basis.0;
        forward.y = 0.0;
        forward = forward.normalize_or_zero();
        let mut right = basis.1;
        right.y = 0.0;
        right = right.normalize_or_zero();

        let speed_mult = if keys.pressed(KeyCode::ShiftLeft) {
            2.5
        } else {
            1.0
        };
        let zoom_scale = (controller.rts_zoom / 220.0).clamp(0.5, 3.0);
        let move_vec = (forward * pan_input.y + right * pan_input.x)
            * controller.rts_pan_speed
            * dt
            * speed_mult
            * zoom_scale;
        controller.rts_focus += move_vec;
    }

    if let Some(terrain) = terrain {
        let bounds = terrain.generator.active_map_bounds();
        controller.rts_focus.x = controller.rts_focus.x.clamp(bounds.min[0], bounds.max[0]);
        controller.rts_focus.z = controller.rts_focus.z.clamp(bounds.min[1], bounds.max[1]);
    }

    apply_rts_transform(transform, controller, terrain);
}

fn handle_mode_transition(
    transform: &mut Transform,
    controller: &mut EditorCameraController,
    terrain: Option<&WorldTerrain>,
) {
    match controller.mode {
        EditorCameraMode::Rts => {
            let origin = transform.translation;
            let dir = transform.forward().as_vec3();
            let mut focus = focus_from_ray(origin, dir);
            if let Some(terrain) = terrain {
                focus.y = terrain.get_height(focus.x, focus.z);
            } else {
                focus.y = 0.0;
            }
            controller.rts_focus = focus;
            controller.rts_zoom = origin
                .distance(focus)
                .clamp(controller.rts_zoom_min, controller.rts_zoom_max);

            let mut horizontal = Vec3::new(dir.x, 0.0, dir.z);
            if horizontal.length_squared() > 1e-6 {
                horizontal = horizontal.normalize();
                controller.yaw = horizontal.x.atan2(-horizontal.z);
            }
            apply_rts_transform(transform, controller, terrain);
        }
        EditorCameraMode::Free => {
            let (yaw, pitch, _roll) = transform.rotation.to_euler(EulerRot::YXZ);
            controller.yaw = yaw;
            controller.pitch = pitch;
        }
    }
}

fn focus_from_ray(origin: Vec3, dir: Vec3) -> Vec3 {
    if dir.y.abs() > f32::EPSILON {
        let t = -origin.y / dir.y;
        if t.is_finite() && t > 0.0 {
            return origin + dir * t;
        }
    }
    Vec3::new(origin.x, 0.0, origin.z)
}

fn rts_basis(yaw: f32) -> (Vec3, Vec3) {
    let yaw_rotation = Quat::from_axis_angle(Vec3::Y, yaw);
    let forward = yaw_rotation * Vec3::NEG_Z;
    let right = yaw_rotation * Vec3::X;
    (forward, right)
}

fn apply_rts_transform(
    transform: &mut Transform,
    controller: &EditorCameraController,
    terrain: Option<&WorldTerrain>,
) {
    let mut focus = controller.rts_focus;
    if let Some(terrain) = terrain {
        focus.y = terrain.get_height(focus.x, focus.z);
    } else {
        focus.y = 0.0;
    }

    let rotation = Quat::from_axis_angle(Vec3::Y, controller.yaw)
        * Quat::from_axis_angle(Vec3::X, -controller.rts_tilt);
    let offset = rotation * Vec3::new(0.0, 0.0, controller.rts_zoom);
    transform.translation = focus + offset;
    transform.rotation = rotation;
}
