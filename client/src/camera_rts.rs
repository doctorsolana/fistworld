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

/// Camera tilt at minimum and maximum zoom, in radians (0 = horizon, PI/2 = straight down).
const TILT_CLOSE: f32 = 0.55;
const TILT_FAR: f32 = 1.45;

/// Network peer id of the local client, published on connect.
///
/// Not read yet — it is the hook for "which units are mine" once unit ownership exists.
#[allow(dead_code)]
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct LocalPeerId(pub u64);

/// World-space point currently under the mouse cursor, if it hits terrain.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct CursorTerrainHit(pub Option<Vec3>);

/// The world-space ray under the mouse cursor.
///
/// Published here rather than recomputed by every picker, because turning a
/// window cursor position into a camera ray is NOT trivial in this app: the 3D
/// camera renders to a scaled offscreen target, so cursor coordinates have to
/// be mapped from window space into the camera's own viewport space first. Any
/// second copy of that mapping is a picking offset that only appears at
/// non-1.0 render scale -- exactly the kind of bug that ships unnoticed.
#[derive(Resource, Debug, Clone, Copy, Default)]
pub struct CursorRay(pub Option<Ray3d>);

/// How fast the camera catches up to what the input asked for, as a time
/// constant in seconds: after `tau` the remaining error is down to ~37%.
///
/// Input drives a TARGET and the camera eases toward it, rather than input
/// moving the camera directly. Direct movement is what made panning read as
/// jaggedy: every frame the focus jumped by `speed * dt`, so any hitch in
/// frame time showed up as a visible stutter, and starting or stopping was
/// instantaneous in a way no RTS camera is.
///
/// Smaller = snappier and closer to the old behaviour. Around 0.05-0.12 is
/// the usual RTS range; below ~0.03 the smoothing stops being perceptible.
const PAN_SMOOTH_TAU: f32 = 0.085;
/// Zoom eases a little slower — wheel notches are discrete, so they benefit
/// from more smoothing than continuous WASD input.
const ZOOM_SMOOTH_TAU: f32 = 0.11;
/// Orbit is driven by raw mouse deltas, which are already noisy; a short tau
/// takes the edge off without feeling like input lag.
const YAW_SMOOTH_TAU: f32 = 0.05;

/// Frame-rate-independent exponential smoothing factor.
///
/// `lerp(current, target, t)` with a fixed `t` per frame is wrong: it makes
/// the camera converge faster at high frame rates, so the feel changes with
/// load. This derives `t` from elapsed time instead, giving identical motion
/// at 30fps and 240fps.
fn smoothing_factor(tau: f32, dt: f32) -> f32 {
    if tau <= 0.0 {
        return 1.0;
    }
    1.0 - (-dt / tau).exp()
}

/// Orbit/pan camera controller for the top-down view.
///
/// The `*_target` fields are what input writes; the plain fields are what the
/// camera actually renders this frame and are what everything else should
/// read. They converge within a few frames of the input stopping.
#[derive(Component)]
pub struct CommanderCamera {
    pub yaw: f32,
    pub yaw_target: f32,
    pub focus: Vec3,
    pub focus_target: Vec3,
    pub pan_speed: f32,
    pub zoom: f32,
    pub zoom_target: f32,
    pub zoom_min: f32,
    pub zoom_max: f32,
    pub zoom_speed: f32,
    pub tilt: f32,
    pub look_sensitivity: f32,
}

impl Default for CommanderCamera {
    fn default() -> Self {
        // Test hook: perf runs need a reproducible zoom without input automation.
        let start_zoom = std::env::var("FISTFORCE_START_ZOOM")
            .ok()
            .and_then(|raw| raw.parse::<f32>().ok())
            .filter(|z| z.is_finite())
            .unwrap_or(280.0);
        Self {
            yaw: -0.45,
            yaw_target: -0.45,
            focus: Vec3::ZERO,
            focus_target: Vec3::ZERO,
            pan_speed: 120.0,
            zoom: start_zoom,
            zoom_target: start_zoom,
            // Range spans "one character" to "see your realm". The old 55..900 window was
            // sized for a squad-scale RTS; a persistent world with regions needs to pull
            // back far enough to read territory.
            zoom_min: 12.0,
            zoom_max: 12_000.0,
            // Proportional zoom: a fixed metres-per-notch step is unusable across three
            // orders of magnitude — glacial when far out, jumpy when close in.
            zoom_speed: 0.12,
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
        // Start converged, or the camera would visibly ease in from the origin
        // on the first frame.
        controller.focus_target = controller.focus;
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
        controller.yaw_target -= look_delta.x * controller.look_sensitivity;
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
        // Multiplicative: each notch changes zoom by a constant *fraction*, so the felt
        // speed is the same whether you are inspecting a soldier or looking at a realm.
        let factor = (1.0 + controller.zoom_speed).powf(-scroll_lines);
        controller.zoom_target =
            (controller.zoom_target * factor).clamp(controller.zoom_min, controller.zoom_max);
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
        // Pan along the yaw the player SEES, not the one they are turning
        // toward, or the pan direction would swim while the orbit settles.
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
        controller.focus_target += (forward * pan_input.y + right * pan_input.x)
            * pan_speed
            * time.delta_secs()
            * speed_mult
            * zoom_scale;
    }

    // Clamp the TARGET, so holding a direction at the map edge cannot bank up
    // an out-of-bounds target that the camera then has to unwind.
    if let Some(terrain) = terrain.as_deref() {
        let bounds = terrain.generator.active_map_bounds();
        controller.focus_target.x = controller.focus_target.x.clamp(bounds.min[0], bounds.max[0]);
        controller.focus_target.z = controller.focus_target.z.clamp(bounds.min[1], bounds.max[1]);
    }

    // Ease the rendered camera toward what input asked for. This is the whole
    // smoothing step; everything above only moves targets.
    let dt = time.delta_secs();
    let pan_t = smoothing_factor(PAN_SMOOTH_TAU, dt);
    let zoom_t_factor = smoothing_factor(ZOOM_SMOOTH_TAU, dt);
    let yaw_t = smoothing_factor(YAW_SMOOTH_TAU, dt);

    controller.focus.x += (controller.focus_target.x - controller.focus.x) * pan_t;
    controller.focus.z += (controller.focus_target.z - controller.focus.z) * pan_t;
    controller.zoom += (controller.zoom_target - controller.zoom) * zoom_t_factor;
    controller.yaw += (controller.yaw_target - controller.yaw) * yaw_t;

    // Ease toward straight-down as the camera pulls back. A shallow angle is fine for
    // watching a fight but turns into an unreadable smear of terrain at map scale.
    let zoom_t = ((controller.zoom - controller.zoom_min)
        / (controller.zoom_max - controller.zoom_min))
        .clamp(0.0, 1.0);
    controller.tilt = TILT_CLOSE + (TILT_FAR - TILT_CLOSE) * zoom_t.powf(0.45);

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
    mut cursor_ray: ResMut<CursorRay>,
    mut last_inputs: Local<Option<(Vec2, Vec3, Quat)>>,
) {
    let Ok(window) = windows.single() else {
        hit.0 = None;
        cursor_ray.0 = None;
        return;
    };
    let Some(cursor_pos) = window.cursor_position() else {
        hit.0 = None;
        cursor_ray.0 = None;
        return;
    };
    let Ok((camera, camera_transform)) = cameras.single() else {
        hit.0 = None;
        cursor_ray.0 = None;
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
        cursor_ray.0 = None;
        return;
    }
    // The ray march is ~300 heightfield samples; with cursor, camera, and
    // terrain all unchanged the result is identical, so an idle frame must
    // not pay for it (this runs every Update frame forever).
    let inputs = (
        cursor_pos,
        camera_transform.translation(),
        camera_transform.rotation(),
    );
    if *last_inputs == Some(inputs) && !terrain.is_changed() {
        return;
    }
    *last_inputs = Some(inputs);

    let viewport_pos = cursor_pos / window_size * viewport_size;
    let Ok(ray) = camera.viewport_to_world(camera_transform, viewport_pos) else {
        hit.0 = None;
        cursor_ray.0 = None;
        return;
    };

    cursor_ray.0 = Some(ray);
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
    // Must cover the camera at max zoom (12km) or clicks silently die in
    // the upper zoom range. The step grows with distance (binary refinement
    // restores precision), so the longer reach costs ~2x, not ~3x.
    const RAY_MAX_DISTANCE: f32 = 14000.0;
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
        t += RAY_STEP + t * 0.008;
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
        // Ground covered by the view grows with camera distance; pad it so content exists
        // slightly beyond the frame rather than popping in at the screen edge.
        view_radius: controller.zoom * 1.35,
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
