use bevy::prelude::*;
use serde::{Deserialize, Serialize};

use crate::components::{NpcArchetype, PlayerCharacter};

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
    /// Descend while flying or swimming.
    pub fly_down: bool,
    /// Fly fast (speed multiplier)
    pub fly_fast: bool,
    /// Player's facing direction (yaw) for movement calculation
    pub yaw: f32,
    /// Context-sensitive interact request
    pub interact: bool,
}

#[derive(Serialize, Deserialize)]
struct PackedPlayerInput {
    flags: u16,
    yaw_q: u16,
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
// bits 9/10 intentionally vacant (were vehicle input flags).
// bit 11 intentionally vacant (was FLAG_BLOCK); PackedPlayerInput is rewritten in P6.

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
        PackedPlayerInput {
            flags,
            yaw_q: quantize_yaw_u16(self.yaw),
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
            interact: (packed.flags & FLAG_INTERACT) != 0,
        })
    }
}

/// Message sent from client when they want to spawn.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct SpawnPlayer;

/// Message sent from client to request firing a weapon.
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

/// Client -> Server: debug request to spawn NPCs near the requesting player.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct SpawnOilmanDebug {
    pub count: u16,
    /// Which NPC archetype to spawn (Oilman, or Dummy for the gray
    /// ragdoll-reference figure).
    pub archetype: NpcArchetype,
}

/// Client -> Server: debug request to spawn physics test boxes near the requesting player.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct SpawnPhysicsBoxDebug {
    pub count: u16,
    pub anchor_position: Option<Vec3>,
}

/// What the bullet impacted (used for visuals/debug).
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

/// Client -> Server: create or join the player's railroad company.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Copy, Eq, Hash)]
pub enum RagdollBodyId {
    Pelvis,
    SpineLower,
    SpineUpper,
    Head,
    UpperArmL,
    UpperArmR,
    ForearmL,
    ForearmR,
    HandL,
    HandR,
    ThighL,
    ThighR,
    CalfL,
    CalfR,
    FootL,
    FootR,
}

/// Compact quaternion transport (i16 per component).
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Copy, Eq)]
pub struct PackedQuatI16 {
    pub x: i16,
    pub y: i16,
    pub z: i16,
    pub w: i16,
}

#[inline]
fn quantize_quat_component(value: f32) -> i16 {
    (value.clamp(-1.0, 1.0) * i16::MAX as f32).round() as i16
}

#[inline]
fn dequantize_quat_component(value: i16) -> f32 {
    (value as f32 / i16::MAX as f32).clamp(-1.0, 1.0)
}

/// Pack a unit quaternion for transport.
#[inline]
pub fn pack_quat_i16(quat: Quat) -> PackedQuatI16 {
    let normalized = quat.normalize();
    PackedQuatI16 {
        x: quantize_quat_component(normalized.x),
        y: quantize_quat_component(normalized.y),
        z: quantize_quat_component(normalized.z),
        w: quantize_quat_component(normalized.w),
    }
}

/// Unpack a quaternion from transport representation.
#[inline]
pub fn unpack_quat_i16(packed: PackedQuatI16) -> Quat {
    Quat::from_xyzw(
        dequantize_quat_component(packed.x),
        dequantize_quat_component(packed.y),
        dequantize_quat_component(packed.z),
        dequantize_quat_component(packed.w),
    )
    .normalize()
}

/// Per-body pose sample for reduced-body ragdoll sync.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct RagdollBodyPose {
    pub body: RagdollBodyId,
    pub position: Vec3,
    pub rotation: PackedQuatI16,
}

/// Reliable event indicating NPC ragdoll start.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct NpcRagdollStarted {
    pub npc_id: u64,
    pub archetype: NpcArchetype,
    pub started_at: f32,
    pub root_position: Vec3,
    pub root_rotation: PackedQuatI16,
    pub bodies: Vec<RagdollBodyPose>,
}

/// One NPC ragdoll pose sample.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct NpcRagdollPoseSample {
    pub npc_id: u64,
    pub seq: u32,
    pub root_position: Vec3,
    pub root_rotation: PackedQuatI16,
    pub bodies: Vec<RagdollBodyPose>,
}

/// Batched ragdoll pose samples streamed server -> client.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct NpcRagdollPoseBatch {
    pub server_time_ms: u64,
    pub samples: Vec<NpcRagdollPoseSample>,
}

/// Reliable channel for important messages.
pub struct ReliableChannel;

/// Unreliable channel for frequent input (lowest latency).
pub struct InputChannel;

/// Unreliable server->client channel for ragdoll pose snapshots.
pub struct RagdollPoseChannel;

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

        let yaw_step = std::f32::consts::TAU / u16::MAX as f32;
        assert!((decoded.yaw - input.yaw).abs() <= yaw_step);
        assert!(bytes.len() <= 8);
    }

    #[test]
    fn packed_quat_roundtrip_stays_within_quantization_error() {
        let q = Quat::from_rotation_y(1.1) * Quat::from_rotation_x(-0.37);
        let packed = pack_quat_i16(q);
        let decoded = unpack_quat_i16(packed);
        let alignment = q.normalize().dot(decoded).abs();
        assert!(alignment > 0.9999);
    }

    #[test]
    fn npc_ragdoll_messages_roundtrip() {
        let started = NpcRagdollStarted {
            npc_id: 42,
            archetype: NpcArchetype::Oilman,
            started_at: 12.5,
            root_position: Vec3::new(1.0, 2.0, 3.0),
            root_rotation: pack_quat_i16(Quat::from_rotation_y(0.5)),
            bodies: vec![
                RagdollBodyPose {
                    body: RagdollBodyId::Pelvis,
                    position: Vec3::new(1.0, 0.8, 3.0),
                    rotation: pack_quat_i16(Quat::IDENTITY),
                },
                RagdollBodyPose {
                    body: RagdollBodyId::FootR,
                    position: Vec3::new(1.2, 0.1, 3.1),
                    rotation: pack_quat_i16(Quat::from_rotation_x(0.2)),
                },
            ],
        };
        let bytes = bincode::serialize(&started).unwrap();
        let decoded: NpcRagdollStarted = bincode::deserialize(&bytes).unwrap();
        assert_eq!(decoded.npc_id, started.npc_id);
        assert_eq!(decoded.archetype, started.archetype);
        assert_eq!(decoded.bodies.len(), 2);
        assert!(decoded
            .bodies
            .iter()
            .any(|pose| pose.body == RagdollBodyId::FootR));

        let batch = NpcRagdollPoseBatch {
            server_time_ms: 1234,
            samples: vec![NpcRagdollPoseSample {
                npc_id: 42,
                seq: 7,
                root_position: Vec3::new(4.0, 5.0, 6.0),
                root_rotation: pack_quat_i16(Quat::from_rotation_x(0.1)),
                bodies: vec![],
            }],
        };
        let batch_bytes = bincode::serialize(&batch).unwrap();
        let batch_decoded: NpcRagdollPoseBatch = bincode::deserialize(&batch_bytes).unwrap();
        assert_eq!(batch_decoded.server_time_ms, 1234);
        assert_eq!(batch_decoded.samples[0].seq, 7);
    }

}
