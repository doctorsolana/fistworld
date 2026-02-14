//! Audio domain state, resources, components, and tuning constants.

use bevy::prelude::*;
use shared::components::NpcArchetype;
use std::collections::{HashMap, HashSet};

/// Resource holding all loaded audio assets.
#[derive(Resource)]
pub struct GameAudio {
    pub assault_shot: Handle<AudioSource>,
    pub revolver_shot: Handle<AudioSource>,
    pub shotgun_shot: Handle<AudioSource>,
    pub sniper_shot: Handle<AudioSource>,
    pub desert_ambient: Handle<AudioSource>,
    // Vehicle sounds
    pub hover_idle: Handle<AudioSource>,
    pub bike_cruise: Handle<AudioSource>,
}

/// Marker for ambient sound entities.
#[derive(Component)]
pub struct AmbientSound;

/// Marker for vehicle idle hover sound.
#[derive(Component)]
pub struct VehicleIdleSound;

/// Marker for vehicle cruise/driving sound.
#[derive(Component)]
pub struct VehicleCruiseSound;

/// Track vehicle audio state.
#[derive(Resource, Default)]
pub struct VehicleAudioState {
    pub sounds_spawned: bool,
    /// Track if player was in vehicle last frame (for detecting enter/exit)
    pub was_in_vehicle: bool,
}

/// Track audio state.
#[derive(Resource, Default)]
pub struct AudioState {
    pub ambient_spawned: bool,
    pub assets_ready: bool,
}

/// Marker for remote player spatial audio (gunshots, etc.).
#[derive(Component)]
pub struct RemoteSpatialSound;

/// Priority levels for audio - higher value = higher priority (less likely to be dropped).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AudioPriority {
    /// Remote footsteps - lowest priority, drop first.
    Ambient = 0,
    /// Remote gunshots.
    CombatRemote = 1,
    /// NPC dialogue.
    Dialogue = 2,
}

/// Marker component for audio entities managed by AudioManager.
#[derive(Component)]
pub struct ManagedAudioTag {
    pub priority: AudioPriority,
    pub spawn_time: f32,
}

/// A queued dialogue request (NPC wants to speak).
#[derive(Debug, Clone)]
pub struct DialogueRequest {
    pub npc_entity: Entity,
    pub distance_sq: f32,
    pub archetype: NpcArchetype,
}

/// Central audio manager - tracks limits and queues.
#[derive(Resource)]
pub struct AudioManager {
    /// Hard cap on all managed audio entities.
    pub max_total: usize,
    /// Max concurrent dialogue sounds.
    pub max_dialogue: usize,
    /// Max remote gunshot sounds.
    pub max_remote_combat: usize,
    /// Max remote footstep emitters.
    pub max_remote_footsteps: usize,
    /// Queued dialogue requests for this frame.
    pub dialogue_queue: Vec<DialogueRequest>,
}

impl Default for AudioManager {
    fn default() -> Self {
        Self {
            max_total: 32,
            max_dialogue: 4,
            max_remote_combat: 8,
            max_remote_footsteps: 12,
            dialogue_queue: Vec::with_capacity(8),
        }
    }
}

/// Spatial audio loop attached to a remote entity to represent footsteps.
#[derive(Component, Clone, Copy, Debug)]
pub struct RemoteFootstepEmitter {
    pub target: Entity,
}

#[derive(Component, Clone, Copy, Debug)]
pub struct RemoteFootstepState {
    pub last_pos: Vec3,
    pub playing: bool,
}

pub const REMOTE_FOOTSTEP_MAX_SPAWN_DISTANCE: f32 = 90.0;
pub const REMOTE_FOOTSTEP_DESPAWN_DISTANCE: f32 = 130.0;
pub const REMOTE_FOOTSTEP_START_SPEED: f32 = 0.6;
pub const REMOTE_FOOTSTEP_STOP_SPEED: f32 = 0.25;
pub const REMOTE_FOOTSTEP_VOLUME: f32 = 0.22;

#[derive(Component, Clone, Copy, Debug)]
pub struct RemoteVehicleIdleSound {
    pub vehicle: Entity,
}

#[derive(Component, Clone, Copy, Debug)]
pub struct RemoteVehicleCruiseSound {
    pub vehicle: Entity,
}

pub const REMOTE_VEHICLE_MAX_SPAWN_DISTANCE: f32 = 160.0;
pub const REMOTE_VEHICLE_DESPAWN_DISTANCE: f32 = 220.0;

/// Incremental cache for remote audio emitter ownership/membership.
#[derive(Resource, Default)]
pub struct RemoteAudioEmitterIndex {
    pub footstep_targets: HashSet<Entity>,
    pub footstep_by_emitter: HashMap<Entity, Entity>,
    pub vehicle_idle_targets: HashSet<Entity>,
    pub vehicle_idle_by_emitter: HashMap<Entity, Entity>,
    pub vehicle_cruise_targets: HashSet<Entity>,
    pub vehicle_cruise_by_emitter: HashMap<Entity, Entity>,
}
