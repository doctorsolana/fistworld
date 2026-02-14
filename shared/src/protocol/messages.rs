use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::components::PlayerCharacter;
use crate::vehicle::VehicleInput;
use crate::weapons::damage::HitZone;

/// Player input sent from client to server each tick.
#[derive(Debug, PartialEq, Clone)]
pub struct PlayerInput {
    pub forward: bool,
    pub backward: bool,
    pub left: bool,
    pub right: bool,
    /// Jump request (spacebar)
    pub jump: bool,
    /// Debug fly mode toggle (server-authoritative movement)
    pub fly_mode: bool,
    /// Fly down (descend)
    pub fly_down: bool,
    /// Fly fast (speed multiplier)
    pub fly_fast: bool,
    /// Player's facing direction (yaw) for movement calculation
    pub yaw: f32,
    /// If in a vehicle, this contains the vehicle input
    pub vehicle_input: Option<VehicleInput>,
    /// Request to enter/exit vehicle
    pub interact: bool,
}

#[derive(Serialize, Deserialize)]
struct PackedPlayerInput {
    flags: u16,
    yaw_q: u16,
    throttle_q: u8,
    brake_q: u8,
    steer_q: i8,
}

const FLAG_FORWARD: u16 = 1 << 0;
const FLAG_BACKWARD: u16 = 1 << 1;
const FLAG_LEFT: u16 = 1 << 2;
const FLAG_RIGHT: u16 = 1 << 3;
const FLAG_JUMP: u16 = 1 << 4;
const FLAG_FLY_MODE: u16 = 1 << 5;
const FLAG_FLY_DOWN: u16 = 1 << 6;
const FLAG_FLY_FAST: u16 = 1 << 7;
const FLAG_INTERACT: u16 = 1 << 8;
const FLAG_HAS_VEHICLE_INPUT: u16 = 1 << 9;
const FLAG_VEHICLE_AIR_CONTROL: u16 = 1 << 10;

#[inline]
fn quantize_unit_u8(value: f32) -> u8 {
    (value.clamp(0.0, 1.0) * 255.0).round() as u8
}

#[inline]
fn dequantize_unit_u8(value: u8) -> f32 {
    value as f32 / 255.0
}

#[inline]
fn quantize_signed_i8(value: f32) -> i8 {
    (value.clamp(-1.0, 1.0) * 127.0).round() as i8
}

#[inline]
fn dequantize_signed_i8(value: i8) -> f32 {
    (value as f32 / 127.0).clamp(-1.0, 1.0)
}

#[inline]
fn quantize_yaw_u16(yaw: f32) -> u16 {
    let normalized =
        ((yaw + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)) / std::f32::consts::TAU;
    (normalized * u16::MAX as f32).round() as u16
}

#[inline]
fn dequantize_yaw_u16(value: u16) -> f32 {
    (value as f32 / u16::MAX as f32) * std::f32::consts::TAU - std::f32::consts::PI
}

impl Default for PlayerInput {
    fn default() -> Self {
        Self {
            forward: false,
            backward: false,
            left: false,
            right: false,
            jump: false,
            fly_mode: false,
            fly_down: false,
            fly_fast: false,
            yaw: 0.0,
            vehicle_input: None,
            interact: false,
        }
    }
}

impl Serialize for PlayerInput {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut flags = 0u16;
        if self.forward {
            flags |= FLAG_FORWARD;
        }
        if self.backward {
            flags |= FLAG_BACKWARD;
        }
        if self.left {
            flags |= FLAG_LEFT;
        }
        if self.right {
            flags |= FLAG_RIGHT;
        }
        if self.jump {
            flags |= FLAG_JUMP;
        }
        if self.fly_mode {
            flags |= FLAG_FLY_MODE;
        }
        if self.fly_down {
            flags |= FLAG_FLY_DOWN;
        }
        if self.fly_fast {
            flags |= FLAG_FLY_FAST;
        }
        if self.interact {
            flags |= FLAG_INTERACT;
        }

        let (throttle_q, brake_q, steer_q) = if let Some(vehicle_input) = &self.vehicle_input {
            flags |= FLAG_HAS_VEHICLE_INPUT;
            if vehicle_input.air_control {
                flags |= FLAG_VEHICLE_AIR_CONTROL;
            }
            (
                quantize_unit_u8(vehicle_input.throttle),
                quantize_unit_u8(vehicle_input.brake),
                quantize_signed_i8(vehicle_input.steer),
            )
        } else {
            (0, 0, 0)
        };

        PackedPlayerInput {
            flags,
            yaw_q: quantize_yaw_u16(self.yaw),
            throttle_q,
            brake_q,
            steer_q,
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for PlayerInput {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let packed = PackedPlayerInput::deserialize(deserializer)?;
        let has_vehicle_input = (packed.flags & FLAG_HAS_VEHICLE_INPUT) != 0;

        Ok(Self {
            forward: (packed.flags & FLAG_FORWARD) != 0,
            backward: (packed.flags & FLAG_BACKWARD) != 0,
            left: (packed.flags & FLAG_LEFT) != 0,
            right: (packed.flags & FLAG_RIGHT) != 0,
            jump: (packed.flags & FLAG_JUMP) != 0,
            fly_mode: (packed.flags & FLAG_FLY_MODE) != 0,
            fly_down: (packed.flags & FLAG_FLY_DOWN) != 0,
            fly_fast: (packed.flags & FLAG_FLY_FAST) != 0,
            yaw: dequantize_yaw_u16(packed.yaw_q),
            vehicle_input: has_vehicle_input.then_some(VehicleInput {
                throttle: dequantize_unit_u8(packed.throttle_q),
                brake: dequantize_unit_u8(packed.brake_q),
                steer: dequantize_signed_i8(packed.steer_q),
                air_control: (packed.flags & FLAG_VEHICLE_AIR_CONTROL) != 0,
            }),
            interact: (packed.flags & FLAG_INTERACT) != 0,
        })
    }
}

/// Message sent from client when they want to spawn.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct SpawnPlayer;

/// Message sent from client to request firing a weapon.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct ShootRequest {
    /// Normalized aim direction in world space
    pub direction: Vec3,
    /// Player's pitch for aiming
    pub pitch: f32,
    /// Whether aiming down sights
    pub aiming: bool,
}

/// Message sent from server to confirm a hit.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct HitConfirm {
    /// ID of the player that was hit
    pub target_id: u64,
    /// Damage dealt
    pub damage: f32,
    /// Was it a headshot
    pub headshot: bool,
    /// Did it kill the target
    pub kill: bool,
    /// Hit zone
    pub hit_zone: HitZone,
}

/// Message sent from server when player takes damage.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct DamageReceived {
    /// Direction damage came from (for hit indicator)
    pub direction: Vec3,
    /// Damage amount
    pub damage: f32,
    /// Current health after damage
    pub health_remaining: f32,
}

/// Message sent from server when player dies.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct PlayerKilled {
    /// ID of player who killed us
    pub killer_id: u64,
    /// Weapon used
    pub weapon: crate::weapons::WeaponType,
    /// Was it a headshot
    pub headshot: bool,
}

/// Message sent from client to switch weapons.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct SwitchWeapon {
    /// Weapon type to switch to
    pub weapon_type: crate::weapons::WeaponType,
}

/// Message sent from client to reload current weapon.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct ReloadRequest;

#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Copy)]
pub enum TimeOfDayPreset {
    Night,
    Morning,
    Midday,
    Sunset,
}

impl TimeOfDayPreset {
    pub fn normalized_time(&self) -> f32 {
        match self {
            TimeOfDayPreset::Night => 0.0,
            TimeOfDayPreset::Morning => 0.25,
            TimeOfDayPreset::Midday => 0.5,
            TimeOfDayPreset::Sunset => 0.75,
        }
    }
}

/// Client -> Server: request a debug time-of-day change.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct SetTimeOfDay {
    pub preset: TimeOfDayPreset,
}

/// Client -> Server: request a player character model swap.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct SetPlayerCharacter {
    pub character: PlayerCharacter,
}

/// Client -> Server: debug request to spawn Oilman NPCs near the requesting player.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct SpawnOilmanDebug {
    pub count: u16,
}

/// What the bullet impacted (used for visuals/debug).
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Copy)]
pub enum BulletImpactSurface {
    Terrain,
    PracticeWall,
    Player,
    Npc,
}

/// Server -> Client: bullet impact (reliable visual feedback independent of bullet replication).
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct BulletImpact {
    pub owner_id: u64,
    pub weapon_type: crate::weapons::WeaponType,
    pub spawn_position: Vec3,
    pub initial_velocity: Vec3,
    pub impact_position: Vec3,
    pub impact_normal: Vec3,
    pub surface: BulletImpactSurface,
}

/// Type of audio event for spatial audio.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Copy)]
pub enum AudioEventKind {
    /// Gunshot sound (weapon type affects which sound to play)
    Gunshot {
        weapon_type: crate::weapons::WeaponType,
    },
}

/// Server -> Client: audio event broadcast for spatial audio.
/// Allows clients to hear other players' sounds (gunshots, etc.)
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct AudioEvent {
    /// ID of the player who made the sound (skip if it's ourselves)
    pub player_id: u64,
    /// World position where the sound originated
    pub position: Vec3,
    /// Type of audio event
    pub kind: AudioEventKind,
}

/// Message sent from client to submit their player name on connection.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct SubmitPlayerName {
    /// Chosen player name (3-16 chars, alphanumeric + _ and -)
    pub name: String,
}

/// Server response to player name submission.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub enum NameSubmissionResult {
    /// Name accepted, player can now spawn
    Accepted {
        /// Whether an existing profile was loaded from disk
        profile_loaded: bool,
    },
    /// Name rejected, must try again
    Rejected {
        /// Reason for rejection
        reason: NameRejectionReason,
    },
}

/// Reasons why a player name was rejected.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Copy)]
pub enum NameRejectionReason {
    /// Name contains invalid characters (only alphanumeric, _, - allowed)
    InvalidCharacters,
    /// Name is too short (< 3 characters)
    TooShort,
    /// Name is too long (> 16 characters)
    TooLong,
    /// Name is reserved (admin, server, etc.)
    Reserved,
    /// Name is already in use by another connected player
    AlreadyOnline,
}

/// Client -> Server: request the full player roster (levels + online status).
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct RequestPlayerRoster;

/// Server -> Client: player roster response.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct PlayerRoster {
    pub entries: Vec<PlayerRosterEntry>,
}

/// Summary info for a player in the roster.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct PlayerRosterEntry {
    pub name: String,
    pub level: u32,
    pub prestige: u32,
    pub online: bool,
}

/// Reliable channel for important messages.
pub struct ReliableChannel;

/// Unreliable channel for frequent input (lowest latency).
pub struct InputChannel;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn player_input_roundtrip_on_foot_preserves_flags_and_has_small_yaw_error() {
        let input = PlayerInput {
            forward: true,
            backward: false,
            left: true,
            right: false,
            jump: true,
            fly_mode: false,
            fly_down: false,
            fly_fast: true,
            yaw: 1.2345,
            vehicle_input: None,
            interact: true,
        };

        let bytes = bincode::serialize(&input).unwrap();
        let decoded: PlayerInput = bincode::deserialize(&bytes).unwrap();

        assert_eq!(decoded.forward, input.forward);
        assert_eq!(decoded.backward, input.backward);
        assert_eq!(decoded.left, input.left);
        assert_eq!(decoded.right, input.right);
        assert_eq!(decoded.jump, input.jump);
        assert_eq!(decoded.fly_mode, input.fly_mode);
        assert_eq!(decoded.fly_down, input.fly_down);
        assert_eq!(decoded.fly_fast, input.fly_fast);
        assert_eq!(decoded.interact, input.interact);
        assert!(decoded.vehicle_input.is_none());

        let yaw_step = std::f32::consts::TAU / u16::MAX as f32;
        assert!((decoded.yaw - input.yaw).abs() <= yaw_step);
        assert!(bytes.len() <= 8);
    }

    #[test]
    fn player_input_roundtrip_vehicle_preserves_air_control_and_quantized_controls() {
        let input = PlayerInput {
            forward: false,
            backward: true,
            left: false,
            right: true,
            jump: false,
            fly_mode: true,
            fly_down: true,
            fly_fast: false,
            yaw: -2.4,
            vehicle_input: Some(VehicleInput {
                throttle: 0.73,
                brake: 0.15,
                steer: -0.44,
                air_control: true,
            }),
            interact: false,
        };

        let bytes = bincode::serialize(&input).unwrap();
        let decoded: PlayerInput = bincode::deserialize(&bytes).unwrap();
        let vehicle = decoded.vehicle_input.expect("vehicle input should decode");
        let expected_vehicle = input.vehicle_input.as_ref().unwrap();

        assert!(vehicle.air_control);
        assert!((vehicle.throttle - expected_vehicle.throttle).abs() <= (1.0 / 255.0));
        assert!((vehicle.brake - expected_vehicle.brake).abs() <= (1.0 / 255.0));
        assert!((vehicle.steer - expected_vehicle.steer).abs() <= (1.0 / 127.0));
    }
}
