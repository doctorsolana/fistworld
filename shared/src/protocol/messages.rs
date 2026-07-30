use bevy::prelude::*;
use serde::{Deserialize, Serialize};

/// Commander view state sent from client to server each tick.
///
/// The commander has no body: the client owns the camera, and the server only needs to
/// know where it is looking so it can stream terrain colliders and (later) resolve
/// interest management around that focus point.
///
/// NOTE: this is a plain derived (de)serialize. The FPS version hand-wrote `Serialize`
/// and `Deserialize` around a bit-packed movement struct, which meant every field change
/// risked silently skewing the wire format. Do not reintroduce that without a roundtrip test.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone, Default)]
pub struct PlayerInput {
    /// Camera yaw in radians.
    pub yaw: f32,
    /// World-space point the camera is centered on.
    pub focus: Vec3,
    /// Roughly how far the camera can see from `focus`, in metres.
    ///
    /// The server turns this into an interest radius, so zooming out widens what gets
    /// replicated instead of leaving the far half of the view empty.
    pub view_radius: f32,
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
            // Display hours: sunset sits at 22:00 on the summer clock; the
            // preset lands just before the boundary for the golden look.
            TimeOfDayPreset::Sunset => 0.895,
        }
    }
}

/// Client -> Server: request a debug time-of-day change.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct SetTimeOfDay {
    pub preset: TimeOfDayPreset,
}

/// Client -> Server: privileged god-mode commands.
///
/// The server drops these unless the sender's connection has god capability (granted via
/// [`DevStatus`] when the server runs in dev mode). One enum so future god tools (spawn
/// settlement, teleport, grant coin) extend the protocol without new message plumbing.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub enum DevCommand {
    /// Set the simulation speed multiplier: 0 = paused, 1 = real time.
    SetTimeWarp(f32),
    /// Spawn the sender's hero at a world position with the chosen outfit.
    /// One hero per player: the server ignores this if the sender already
    /// has one alive.
    SpawnHero {
        pos: Vec3,
        outfit: crate::components::HeroOutfit,
    },
}

/// Client -> Server: order the sender's hero to walk to a terrain point.
/// The server owns the movement; this is pure intent (RTS click-to-move).
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct HeroMoveTo {
    pub target: Vec3,
}

/// Server -> Client: whether this connection may use god mode.
///
/// Sent once after the player's name is accepted. Purely capability discovery for the
/// client UI — every [`DevCommand`] is still validated server-side.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct DevStatus {
    pub god: bool,
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

/// Reliable channel for important messages.
pub struct ReliableChannel;

/// Unreliable channel for frequent input (lowest latency).
pub struct InputChannel;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn commander_input_roundtrips() {
        let input = PlayerInput {
            yaw: 1.2345,
            focus: Vec3::new(120.5, 8.25, -640.0),
            view_radius: 1400.0,
        };

        let bytes = bincode::serialize(&input).unwrap();
        let decoded: PlayerInput = bincode::deserialize(&bytes).unwrap();

        assert_eq!(decoded, input);
    }

    #[test]
    fn dev_command_roundtrips() {
        let command = DevCommand::SetTimeWarp(64.0);

        let bytes = bincode::serialize(&command).unwrap();
        let decoded: DevCommand = bincode::deserialize(&bytes).unwrap();

        assert_eq!(decoded, command);
    }

    #[test]
    fn spawn_hero_roundtrips() {
        let command = DevCommand::SpawnHero {
            pos: Vec3::new(12.0, 3.5, -900.25),
            outfit: crate::components::HeroOutfit {
                slots: [1, 0, 4, 0, 0, 0],
                skin: 3,
            },
        };

        let bytes = bincode::serialize(&command).unwrap();
        let decoded: DevCommand = bincode::deserialize(&bytes).unwrap();

        assert_eq!(decoded, command);
    }

    #[test]
    fn hero_move_to_roundtrips() {
        let msg = HeroMoveTo {
            target: Vec3::new(-64.5, 12.0, 480.0),
        };

        let bytes = bincode::serialize(&msg).unwrap();
        let decoded: HeroMoveTo = bincode::deserialize(&bytes).unwrap();

        assert_eq!(decoded, msg);
    }

    #[test]
    fn dev_status_roundtrips() {
        let status = DevStatus { god: true };

        let bytes = bincode::serialize(&status).unwrap();
        let decoded: DevStatus = bincode::deserialize(&bytes).unwrap();

        assert_eq!(decoded, status);
    }
}
