//! Player input handling
//!
//! Updated for Lightyear 0.26 / Bevy 0.18

use bevy::input::mouse::MouseMotion;
use bevy::prelude::*;
use lightyear::prelude::*;
use shared::components::{Health, LocalPlayer, Player};
use shared::player::MOUSE_SENSITIVITY;
use shared::protocol::{InputChannel, PlayerInput};
use shared::vehicle::{VehicleDriver, VehicleInput};
use std::f32::consts::FRAC_PI_2;

use crate::render::systems::InputSettings;
use crate::states::GameState;

const INPUT_HEARTBEAT_SECS: f32 = 0.10;
const INPUT_CHANGE_BURST_TICKS: u8 = 2;

/// Camera view mode
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub enum CameraMode {
    #[default]
    FirstPerson,
    ThirdPerson,
}

/// Client-side input state
#[derive(Resource)]
pub struct InputState {
    pub forward: bool,
    pub backward: bool,
    pub left: bool,
    pub right: bool,
    /// Jump request (spacebar)
    pub jump: bool,
    /// Mouse-controlled yaw (used when on foot)
    pub yaw: f32,
    /// Mouse-controlled pitch
    pub pitch: f32,
    pub interact: bool,
    pub interact_just_pressed: bool,
    /// Hold Shift to sprint on foot, fly faster in debug fly mode, or do vehicle air tricks.
    pub shift: bool,

    /// Camera mode (toggle with P)
    pub camera_mode: CameraMode,

    // Vehicle camera state
    /// True when we are currently driving a vehicle (used for camera + mouse look behavior)
    pub in_vehicle: bool,
    /// When in vehicle: relative look offset from center (for looking around)
    pub vehicle_look_yaw: f32,
    pub vehicle_look_pitch: f32,

    /// Right-click = Aim Down Sights
    pub aiming: bool,

    /// True when local player is dead (disables movement input)
    pub is_dead: bool,

    /// True when inventory UI is open (disables all gameplay input)
    pub inventory_open: bool,
    /// True when pause menu is open (disables gameplay input)
    pub pause_menu_open: bool,
    /// True when world map is open (disables all gameplay input)
    pub map_open: bool,
    /// True when debug time menu is open (disables all gameplay input)
    pub debug_menu_open: bool,
    /// Debug fly mode toggle
    pub fly_mode: bool,
    /// Fly down (descend)
    pub fly_down: bool,
}

impl Default for InputState {
    fn default() -> Self {
        Self {
            forward: false,
            backward: false,
            left: false,
            right: false,
            jump: false,
            yaw: 0.0,
            pitch: 0.0,
            interact: false,
            interact_just_pressed: false,
            shift: false,
            camera_mode: CameraMode::FirstPerson,
            in_vehicle: false,
            vehicle_look_yaw: 0.0,
            vehicle_look_pitch: 0.0,
            aiming: false,
            is_dead: false,
            inventory_open: false,
            pause_menu_open: false,
            map_open: false,
            debug_menu_open: false,
            fly_mode: false,
            fly_down: false,
        }
    }
}

impl InputState {
    pub(crate) fn ui_blocking(&self) -> bool {
        self.inventory_open || self.pause_menu_open || self.map_open || self.debug_menu_open
    }
}

/// Handle keyboard input for movement
pub fn handle_keyboard_input(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut input_state: ResMut<InputState>,
) {
    // Skip gameplay input if inventory is open
    if input_state.ui_blocking() {
        input_state.forward = false;
        input_state.backward = false;
        input_state.left = false;
        input_state.right = false;
        input_state.jump = false;
        input_state.shift = false;
        input_state.interact_just_pressed = false;
        input_state.interact = false;
        input_state.fly_down = false;
        return;
    }

    input_state.forward = keyboard.pressed(KeyCode::KeyW);
    input_state.backward = keyboard.pressed(KeyCode::KeyS);
    input_state.left = keyboard.pressed(KeyCode::KeyA);
    input_state.right = keyboard.pressed(KeyCode::KeyD);
    input_state.jump = keyboard.pressed(KeyCode::Space);
    input_state.shift =
        keyboard.pressed(KeyCode::ShiftLeft) || keyboard.pressed(KeyCode::ShiftRight);
    input_state.fly_down =
        keyboard.pressed(KeyCode::ControlLeft) || keyboard.pressed(KeyCode::ControlRight);

    input_state.interact_just_pressed = keyboard.just_pressed(KeyCode::KeyE);
    input_state.interact = input_state.interact_just_pressed;

    // Toggle camera mode with P
    if keyboard.just_pressed(KeyCode::KeyP) {
        input_state.camera_mode = match input_state.camera_mode {
            CameraMode::FirstPerson => CameraMode::ThirdPerson,
            CameraMode::ThirdPerson => CameraMode::FirstPerson,
        };
        info!(
            "Camera mode: {:?}",
            if input_state.camera_mode == CameraMode::FirstPerson {
                "First Person"
            } else {
                "Third Person"
            }
        );
    }
}

/// Handle mouse input for looking around
pub fn handle_mouse_input(
    mut mouse_motion: MessageReader<MouseMotion>,
    mouse_button: Res<ButtonInput<MouseButton>>,
    mut input_state: ResMut<InputState>,
    input_settings: Res<InputSettings>,
) {
    // Skip mouse look if inventory is open (but still consume events)
    if input_state.ui_blocking() {
        for _ in mouse_motion.read() {}
        return;
    }

    // Track ADS (right-click to toggle, not hold)
    if mouse_button.just_pressed(MouseButton::Right) && !input_state.in_vehicle {
        input_state.aiming = !input_state.aiming;
    }
    // Disable ADS when entering vehicle
    if input_state.in_vehicle {
        input_state.aiming = false;
    }

    let mut delta = Vec2::ZERO;
    for motion in mouse_motion.read() {
        delta += motion.delta;
    }

    if delta != Vec2::ZERO {
        // Apply user's sensitivity multiplier, and reduce when aiming for more precise control
        let sensitivity = if input_state.aiming {
            MOUSE_SENSITIVITY * input_settings.mouse_sensitivity * 0.5
        } else {
            MOUSE_SENSITIVITY * input_settings.mouse_sensitivity
        };

        if input_state.in_vehicle {
            // In vehicle: update relative look angles
            input_state.vehicle_look_yaw -= delta.x * sensitivity;
            input_state.vehicle_look_pitch -= delta.y * sensitivity;

            // Clamp: can look ~90° left/right, ~60° up/down
            input_state.vehicle_look_yaw =
                input_state.vehicle_look_yaw.clamp(-FRAC_PI_2, FRAC_PI_2);
            input_state.vehicle_look_pitch = input_state.vehicle_look_pitch.clamp(-0.5, 0.7);
        } else {
            // On foot: free look
            input_state.yaw -= delta.x * sensitivity;
            input_state.pitch -= delta.y * sensitivity;
            input_state.pitch = input_state.pitch.clamp(-FRAC_PI_2 + 0.01, FRAC_PI_2 - 0.01);
        }
    }
}

/// Helper to convert PeerId to u64 for driver tracking
fn peer_id_to_u64(peer_id: PeerId) -> u64 {
    match peer_id {
        PeerId::Netcode(id) => id,
        PeerId::Steam(id) => id,
        PeerId::Local(id) => id,
        _ => 0, // Server or other types
    }
}

/// Update input state with whether we're driving a vehicle.
/// (The camera uses the *smoothed vehicle Transform* directly; this is just for mouse-look mode.)
pub fn update_vehicle_state(
    mut input_state: ResMut<InputState>,
    // In Lightyear 0.26, use LocalId to identify the local client peer id
    client_query: Query<&LocalId, (With<crate::GameClient>, With<Connected>)>,
    vehicles: Query<&VehicleDriver>,
) {
    // Get our peer ID from the connected client entity
    let Some(our_peer_id) = client_query.iter().next().map(|r| r.0) else {
        return;
    };

    let is_driving = vehicles
        .iter()
        .any(|driver| driver.driver_id == Some(peer_id_to_u64(our_peer_id)));

    // If we just exited, reset look offsets.
    if input_state.in_vehicle && !is_driving {
        input_state.vehicle_look_yaw = 0.0;
        input_state.vehicle_look_pitch = 0.0;
    }

    input_state.in_vehicle = is_driving;
}

/// Check if local player is dead (updates InputState.is_dead)
pub fn update_death_state(
    mut input_state: ResMut<InputState>,
    local_player: Query<&Health, With<LocalPlayer>>,
) {
    let was_dead = input_state.is_dead;
    input_state.is_dead = local_player
        .iter()
        .next()
        .map(|h| h.is_dead())
        .unwrap_or(false);

    // Log state changes
    if input_state.is_dead && !was_dead {
        info!("Local player died!");
    } else if !input_state.is_dead && was_dead {
        info!("Local player respawned!");
    }
}

/// Send input to server
pub fn handle_send_input_to_server(
    input_state: Res<InputState>,
    game_state: Res<State<GameState>>,
    // In Lightyear 0.26, send messages via MessageSender component - typed on message type
    mut client_query: Query<
        (&LocalId, &mut MessageSender<PlayerInput>),
        (With<crate::GameClient>, With<Connected>),
    >,
    local_player: Query<&Player, With<LocalPlayer>>,
    time: Res<Time>,
    mut last_warn_time: Local<f32>,
    mut last_sent_input: Local<Option<PlayerInput>>,
    mut last_sent_time: Local<f32>,
    mut burst_input: Local<Option<PlayerInput>>,
    mut burst_ticks_remaining: Local<u8>,
) {
    let now = time.elapsed_secs();

    // Get client entity with sender
    let Ok((_local_id, mut sender)) = client_query.single_mut() else {
        // If this fires, input will *never* reach the server, so movement will be frozen.
        if now - *last_warn_time > 1.0 {
            warn!("handle_send_input_to_server: missing GameClient+Connected+LocalId+MessageSender<PlayerInput>; not sending inputs");
            *last_warn_time = now;
        }
        *last_sent_input = None;
        *last_sent_time = now;
        *burst_input = None;
        *burst_ticks_remaining = 0;
        return;
    };

    // If we don't know which Player is ours yet, don't send inputs.
    // This prevents the server from moving a "ghost" player while the client camera is not
    // attached to the correct entity (can happen if initial replication arrives during Connecting).
    let has_local_player = local_player.iter().next().is_some();
    if !has_local_player {
        if now - *last_warn_time > 1.0 {
            warn!("handle_send_input_to_server: no LocalPlayer yet; suppressing input until local player is identified");
            *last_warn_time = now;
        }
        *last_sent_input = None;
        *last_sent_time = now;
        *burst_input = None;
        *burst_ticks_remaining = 0;
        return;
    }

    let mut input = PlayerInput {
        forward: input_state.forward,
        backward: input_state.backward,
        left: input_state.left,
        right: input_state.right,
        jump: input_state.jump,
        fly_mode: input_state.fly_mode,
        fly_down: input_state.fly_down,
        fly_fast: input_state.shift,
        yaw: input_state.yaw,
        vehicle_input: None,
        interact: input_state.interact_just_pressed,
    };

    // Disable all movement input when dead, paused, or inventory open
    if game_state.get() == &GameState::Paused || input_state.is_dead || input_state.ui_blocking() {
        input.forward = false;
        input.backward = false;
        input.left = false;
        input.right = false;
        input.jump = false;
        input.interact = false;
        input.fly_down = false;
        input.fly_fast = false;
        input.vehicle_input = None;
    } else if input_state.in_vehicle {
        input.vehicle_input = Some(VehicleInput {
            throttle: if input_state.forward { 1.0 } else { 0.0 },
            brake: if input_state.backward { 1.0 } else { 0.0 },
            steer: if input_state.left {
                -1.0
            } else if input_state.right {
                1.0
            } else {
                0.0
            },
            air_control: input_state.shift, // Hold Shift for air tricks
        });
        input.forward = false;
        input.backward = false;
        input.left = false;
        input.right = false;
        input.jump = false; // Can't jump while in vehicle
        input.fly_mode = false;
        input.fly_down = false;
        input.fly_fast = false;
    }

    let input_changed = last_sent_input.as_ref() != Some(&input);

    if input_changed {
        sender.send::<InputChannel>(input.clone());
        *last_sent_input = Some(input.clone());
        *last_sent_time = now;
        *burst_input = Some(input);
        *burst_ticks_remaining = INPUT_CHANGE_BURST_TICKS;
        return;
    }

    if *burst_ticks_remaining > 0 {
        let resend = burst_input
            .as_ref()
            .or(last_sent_input.as_ref())
            .cloned()
            .unwrap_or_else(|| input.clone());
        sender.send::<InputChannel>(resend.clone());
        *last_sent_input = Some(resend);
        *last_sent_time = now;
        *burst_ticks_remaining -= 1;
        return;
    }

    let heartbeat_due = now - *last_sent_time >= INPUT_HEARTBEAT_SECS;
    if heartbeat_due {
        sender.send::<InputChannel>(input.clone());
        *last_sent_input = Some(input.clone());
        *last_sent_time = now;
        *burst_input = Some(input);
    }
}
