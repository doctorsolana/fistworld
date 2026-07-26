//! Audio domain state, resources, components, and tuning constants.

use bevy::prelude::*;
use std::collections::{HashMap, HashSet};

/// Resource holding all loaded audio assets.
#[derive(Resource)]
pub struct GameAudio {
    pub desert_ambient: Handle<AudioSource>,
}

/// Track audio state.
#[derive(Resource, Default)]
pub struct AudioState {
    pub assets_ready: bool,
}

/// Priority levels for audio - higher value = higher priority (less likely to be dropped).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AudioPriority {
    /// Remote footsteps - lowest priority, drop first.
    Ambient = 0,
}

/// Marker component for audio entities managed by AudioManager.
#[derive(Component)]
pub struct ManagedAudioTag {
    pub priority: AudioPriority,
    pub spawn_time: f32,
}

/// Central audio manager - tracks limits and queues.
#[derive(Resource)]
pub struct AudioManager {
    /// Hard cap on all managed audio entities.
    pub max_total: usize,
    /// Max remote footstep emitters.
    pub max_remote_footsteps: usize,
}

impl Default for AudioManager {
    fn default() -> Self {
        Self {
            max_total: 32,
            max_remote_footsteps: 12,
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


/// Incremental cache for remote audio emitter ownership/membership.
#[derive(Resource, Default)]
pub struct RemoteAudioEmitterIndex {
    pub footstep_targets: HashSet<Entity>,
    pub footstep_by_emitter: HashMap<Entity, Entity>,
}
