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
    /// Spawn a villager at a world position. A test tool for now: villagers have
    /// a name and stand there, so the encyclopedia and selection have real
    /// non-player people to list before settlements exist to produce them.
    SpawnNpc { pos: Vec3 },
    /// Set a character's banner. Targeted by NAME rather than entity, because
    /// the encyclopedia lists people the client may not currently have
    /// replicated -- interest management only delivers who is nearby.
    SetAffiliation {
        character: String,
        banner: Option<u8>,
    },
    /// Found a settlement: raise a city hall here and name the place.
    ///
    /// The founding act per WORLD-DESIGN section 1. The server enforces the
    /// spacing rule -- a client-side check is advisory.
    FoundSettlement { pos: Vec3, name: String },
    /// Take a villager into the sender's retinue, or dismiss it.
    ///
    /// Targeted by ENTITY, not by name: `shared::names::person_name` produces
    /// its first duplicate around the fifty-first villager, and command has to
    /// be exact. Affiliation can afford to be name-targeted because it addresses
    /// people the client has never replicated; command cannot.
    SetRetinue { unit: Entity, commanded: bool },
}

impl bevy::ecs::entity::MapEntities for DevCommand {
    fn map_entities<M: bevy::ecs::entity::EntityMapper>(&mut self, mapper: &mut M) {
        if let DevCommand::SetRetinue { unit, .. } = self {
            *unit = mapper.get_mapped(*unit);
        }
    }
}

/// Client -> Server: walk these specific units to these specific points.
///
/// Replaces the old `HeroMoveTo`, which carried NO unit identity -- the server
/// inferred "the sender's hero" from the peer id, which is exactly why a
/// villager could never be moved and why an N-unit order collapsed onto one
/// unit.
///
/// Carries real `Entity` ids. `.add_map_entities()` in the protocol plugin makes
/// the CLIENT rewrite each id into the server's id before it goes on the wire.
/// Mapping makes an id MEANINGFUL; it does not make it YOURS -- authority is a
/// separate check on the server, see `CommandedBy`.
///
/// `Entity` is acceptable as a unit reference only because nothing it can name
/// is persisted: villagers are not saved, and a hero's entity is recreated and
/// re-replicated on connect. When world-state persistence lands (ROADMAP Phase
/// 1) this should become a stable id.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct UnitMoveOrder {
    /// (unit, arrival point). PAIRED rather than two parallel Vecs, so a
    /// truncated or hostile message cannot desynchronise units from targets.
    pub units: Vec<(Entity, Vec3)>,
}

/// Hard cap on units per order, enforced SERVER-side.
///
/// A hostile client can put anything in a Vec; a client-side cap is advisory
/// only. This is what stops one packet costing an unbounded loop.
pub const MAX_UNITS_PER_ORDER: usize = 256;

impl bevy::ecs::entity::MapEntities for UnitMoveOrder {
    fn map_entities<M: bevy::ecs::entity::EntityMapper>(&mut self, mapper: &mut M) {
        for (unit, _) in self.units.iter_mut() {
            *unit = mapper.get_mapped(*unit);
        }
    }
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
pub struct RequestCharacterRoster;

/// Server -> Client: every PERSON in the world.
///
/// Deliberately characters, not accounts. Interest management means a client
/// only ever receives entities near it, so a client-side registry built purely
/// from replication would show whoever is standing nearby and nothing else --
/// which is not an encyclopedia. This is the full picture the server has, sent
/// on request, and the client merges it with what it has actually seen.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct CharacterRoster {
    pub entries: Vec<CharacterRosterEntry>,
}

/// One person in the world.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct CharacterRosterEntry {
    pub name: String,
    pub kind: crate::components::CharacterKind,
    pub affiliation: crate::components::CharacterAffiliation,
    pub attributes: crate::components::CharacterAttributes,
    /// For a hero, whether its owner is connected right now. Villagers are never
    /// "online" -- they are simply present, which is a different thing.
    pub online: bool,
    /// True when this is the requesting player's own hero.
    pub is_self: bool,
}

/// Client -> server: fetch the bounded session history for one replicated
/// settlement. History is intentionally pull-based; it does not make every
/// market resend a year of daily records whenever one new day closes.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct RequestSettlementHistory {
    pub settlement: Entity,
}

impl bevy::ecs::entity::MapEntities for RequestSettlementHistory {
    fn map_entities<M: bevy::ecs::entity::EntityMapper>(&mut self, mapper: &mut M) {
        self.settlement = mapper.get_mapped(self.settlement);
    }
}

/// Server -> client response to [`RequestSettlementHistory`].
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct SettlementHistoryResponse {
    pub settlement: Entity,
    pub archive: crate::economy::SettlementHistoryArchive,
}

impl bevy::ecs::entity::MapEntities for SettlementHistoryResponse {
    fn map_entities<M: bevy::ecs::entity::EntityMapper>(&mut self, mapper: &mut M) {
        self.settlement = mapper.get_mapped(self.settlement);
    }
}

/// Client -> server: fetch the bounded world-wide daily rollup.
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct RequestWorldHistory;

/// Server -> client response to [`RequestWorldHistory`].
#[derive(Serialize, Deserialize, Debug, PartialEq, Clone)]
pub struct WorldHistoryResponse {
    pub archive: crate::economy::WorldHistoryArchive,
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
    fn unit_move_order_roundtrips() {
        let msg = UnitMoveOrder {
            units: vec![
                (
                    Entity::from_raw_u32(7).unwrap(),
                    Vec3::new(-64.5, 12.0, 480.0),
                ),
                (Entity::from_raw_u32(9).unwrap(), Vec3::new(1.0, 2.0, 3.0)),
            ],
        };

        let bytes = bincode::serialize(&msg).unwrap();
        let decoded: UnitMoveOrder = bincode::deserialize(&bytes).unwrap();

        assert_eq!(decoded, msg);
    }

    /// Every unit in the order must be remapped, not just the first: a partially
    /// mapped order would move some units and silently drop the rest.
    #[test]
    fn unit_move_order_maps_every_entity() {
        use bevy::ecs::entity::MapEntities;

        // Hands back a fixed replacement per call, in order, so the assertion
        // does not depend on how `EntityIndex` happens to be represented.
        struct SeqMapper {
            next: u32,
        }
        impl bevy::ecs::entity::EntityMapper for SeqMapper {
            fn get_mapped(&mut self, _entity: Entity) -> Entity {
                self.next += 1;
                Entity::from_raw_u32(self.next).unwrap()
            }
            fn set_mapped(&mut self, _source: Entity, _target: Entity) {}
        }

        let mut msg = UnitMoveOrder {
            units: vec![
                (Entity::from_raw_u32(50).unwrap(), Vec3::ZERO),
                (Entity::from_raw_u32(60).unwrap(), Vec3::ONE),
                (Entity::from_raw_u32(70).unwrap(), Vec3::X),
            ],
        };
        msg.map_entities(&mut SeqMapper { next: 0 });

        let mapped: Vec<Entity> = msg.units.iter().map(|(e, _)| *e).collect();
        assert_eq!(
            mapped,
            vec![
                Entity::from_raw_u32(1).unwrap(),
                Entity::from_raw_u32(2).unwrap(),
                Entity::from_raw_u32(3).unwrap(),
            ],
            "not every unit was remapped"
        );
        // Targets must be untouched: mapping addresses, not destinations.
        assert_eq!(msg.units[1].1, Vec3::ONE);
    }

    #[test]
    fn dev_status_roundtrips() {
        let status = DevStatus { god: true };

        let bytes = bincode::serialize(&status).unwrap();
        let decoded: DevStatus = bincode::deserialize(&bytes).unwrap();

        assert_eq!(decoded, status);
    }
}
