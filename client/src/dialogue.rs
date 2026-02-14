//! NPC Dialogue System
//!
//! Handles proximity-based dialogue for special NPCs (Doctor, GarbageMan).
//! When the player gets close, the NPC will play a random voice line
//! and look toward the player.

use bevy::audio::{SpatialAudioSink, Volume};
use bevy::prelude::*;

use shared::components::{Health, LocalPlayer, Npc, NpcArchetype, NpcPosition, PlayerPosition};

use crate::audio::{AudioManager, AudioPriority, DialogueRequest, ManagedAudioTag};
use crate::states::GameState;

// =============================================================================
// CONSTANTS
// =============================================================================

/// Distance at which NPC will start talking
const DIALOGUE_TRIGGER_DISTANCE: f32 = 8.0;

/// Minimum time between dialogue lines (seconds)
const DIALOGUE_COOLDOWN: f32 = 12.0;

/// How long a dialogue line typically lasts (we'll stop "talking" state after this)
const DIALOGUE_DURATION: f32 = 4.0;

// =============================================================================
// RESOURCES
// =============================================================================

/// Holds all loaded dialogue audio clips
#[derive(Resource)]
pub struct DialogueAssets {
    pub oilman_lines: Vec<Handle<AudioSource>>,
}

// =============================================================================
// COMPONENTS
// =============================================================================

/// Marks an NPC as capable of dialogue
#[derive(Component)]
pub struct DialogueNpc {
    /// Which archetype (to select correct voice lines)
    pub archetype: NpcArchetype,
    /// Time since last dialogue (for cooldown)
    pub cooldown_timer: f32,
    /// Currently playing dialogue?
    pub is_talking: bool,
    /// Timer for how long they've been talking
    pub talk_timer: f32,
    /// Index for cycling through lines
    pub line_index: usize,
}

impl DialogueNpc {
    pub(crate) fn new(archetype: NpcArchetype) -> Self {
        Self {
            archetype,
            cooldown_timer: 0.0, // Start ready to talk
            is_talking: false,
            talk_timer: 0.0,
            line_index: 0,
        }
    }
}

/// Marks the audio entity playing a dialogue line
#[derive(Component)]
pub struct DialogueAudio {
    pub npc_entity: Entity,
}

/// Component to track NPC looking at player
#[derive(Component)]
pub struct LookingAtPlayer {
    pub target_yaw: f32,
}

// =============================================================================
// SYSTEMS
// =============================================================================

/// Load dialogue audio files
pub fn setup_dialogue_assets(mut commands: Commands, asset_server: Res<AssetServer>) {
    info!("Loading NPC dialogue audio...");

    // Oilman currently reuses peasant voice lines.
    let oilman_lines = vec![
        asset_server.load("audio/dialogue/peasant/peasant1.ogg"),
        asset_server.load("audio/dialogue/peasant/peasant2.ogg"),
        asset_server.load("audio/dialogue/peasant/peasant3.ogg"),
    ];

    commands.insert_resource(DialogueAssets { oilman_lines });

    info!("Dialogue assets queued for loading");
}

/// Add DialogueNpc component to Oilman NPCs when they spawn
pub fn setup_dialogue_npcs(mut commands: Commands, new_npcs: Query<(Entity, &Npc), Added<Npc>>) {
    for (entity, npc) in new_npcs.iter() {
        if npc.archetype == NpcArchetype::Oilman {
            commands
                .entity(entity)
                .insert(DialogueNpc::new(npc.archetype));
            debug!("Added DialogueNpc component to {:?} NPC", npc.archetype);
        }
    }
}

/// Main dialogue system - queues dialogue requests when player is close
/// Actual spawning is handled by handle_dialogue_queue to respect limits
pub fn update_dialogue(
    mut commands: Commands,
    time: Res<Time>,
    player_query: Query<&PlayerPosition, With<LocalPlayer>>,
    mut dialogue_npcs: Query<(Entity, &NpcPosition, &mut DialogueNpc, Option<&Health>)>,
    existing_audio: Query<&DialogueAudio>,
    mut audio_manager: ResMut<AudioManager>,
) {
    let Ok(player_pos) = player_query.single() else {
        return;
    };

    let dt = time.delta_secs();

    // Clear the dialogue queue from previous frame
    audio_manager.dialogue_queue.clear();

    for (npc_entity, npc_pos, mut dialogue, health_opt) in dialogue_npcs.iter_mut() {
        // Dead NPCs don't talk
        if health_opt.is_some_and(|h| h.is_dead()) {
            // Clean up dialogue state if they were talking
            if dialogue.is_talking {
                dialogue.is_talking = false;
                dialogue.talk_timer = 0.0;
                commands.entity(npc_entity).remove::<LookingAtPlayer>();
            }
            continue;
        }

        // Update cooldown
        if dialogue.cooldown_timer > 0.0 {
            dialogue.cooldown_timer -= dt;
        }

        // Update talk timer
        if dialogue.is_talking {
            dialogue.talk_timer += dt;
            if dialogue.talk_timer > DIALOGUE_DURATION {
                dialogue.is_talking = false;
                dialogue.talk_timer = 0.0;
            }
        }

        // Check if already has audio playing
        let has_active_audio = existing_audio.iter().any(|d| d.npc_entity == npc_entity);

        // Check distance to player
        let distance_sq = npc_pos.0.distance_squared(player_pos.0);
        let distance = distance_sq.sqrt();

        if distance < DIALOGUE_TRIGGER_DISTANCE
            && dialogue.cooldown_timer <= 0.0
            && !dialogue.is_talking
            && !has_active_audio
        {
            // Queue this NPC's dialogue request (will be processed by handle_dialogue_queue)
            audio_manager.dialogue_queue.push(DialogueRequest {
                npc_entity,
                distance_sq,
                archetype: dialogue.archetype,
            });
        }

        // Remove look-at when done talking
        if !dialogue.is_talking {
            commands.entity(npc_entity).remove::<LookingAtPlayer>();
        }
    }
}

/// Process queued dialogue requests, respecting the max_dialogue limit
/// Only the closest NPCs get to speak if there are more requests than slots
pub fn handle_dialogue_queue(
    mut commands: Commands,
    time: Res<Time>,
    assets: Option<Res<DialogueAssets>>,
    mut audio_manager: ResMut<AudioManager>,
    mut dialogue_npcs: Query<(&NpcPosition, &mut DialogueNpc)>,
    existing_dialogue: Query<&DialogueAudio>,
    player_query: Query<&PlayerPosition, With<LocalPlayer>>,
) {
    let Some(assets) = assets else { return };
    let Ok(player_pos) = player_query.single() else {
        return;
    };
    let now = time.elapsed_secs();

    // Count current dialogue audio
    let current_dialogue_count = existing_dialogue.iter().count();
    let available_slots = audio_manager
        .max_dialogue
        .saturating_sub(current_dialogue_count);

    if available_slots == 0 || audio_manager.dialogue_queue.is_empty() {
        return;
    }

    // Sort by distance (closest first)
    audio_manager.dialogue_queue.sort_by(|a, b| {
        a.distance_sq
            .partial_cmp(&b.distance_sq)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Process up to available_slots requests
    let requests_to_process = available_slots.min(audio_manager.dialogue_queue.len());
    for request in audio_manager.dialogue_queue.drain(..requests_to_process) {
        let Ok((npc_pos, mut dialogue)) = dialogue_npcs.get_mut(request.npc_entity) else {
            continue;
        };

        let lines = match request.archetype {
            NpcArchetype::Oilman => &assets.oilman_lines,
            // Legacy archetype keeps existing voice set until dedicated lines are authored.
            NpcArchetype::DesertOutpost => &assets.oilman_lines,
        };

        if lines.is_empty() {
            continue;
        }

        // Cycle through lines
        let line = lines[dialogue.line_index % lines.len()].clone();
        dialogue.line_index = (dialogue.line_index + 1) % lines.len();

        // Spawn spatial audio at NPC position with ManagedAudioTag for limit tracking
        commands.spawn((
            DialogueAudio {
                npc_entity: request.npc_entity,
            },
            ManagedAudioTag {
                priority: AudioPriority::Dialogue,
                spawn_time: now,
            },
            AudioPlayer::new(line),
            PlaybackSettings::DESPAWN
                .with_volume(Volume::Linear(1.0))
                .with_spatial(true),
            Transform::from_translation(npc_pos.0),
            GlobalTransform::from_translation(npc_pos.0),
        ));

        dialogue.is_talking = true;
        dialogue.talk_timer = 0.0;
        dialogue.cooldown_timer = DIALOGUE_COOLDOWN;

        // Add look-at component
        let dir_to_player = player_pos.0 - npc_pos.0;
        let target_yaw = (-dir_to_player.x).atan2(-dir_to_player.z);
        commands
            .entity(request.npc_entity)
            .insert(LookingAtPlayer { target_yaw });

        debug!(
            "{:?} NPC speaking (distance: {:.1}m)",
            request.archetype,
            request.distance_sq.sqrt()
        );
    }
}

/// Update dialogue audio position to follow NPC
pub fn update_dialogue_audio_positions(
    mut commands: Commands,
    npc_positions: Query<&NpcPosition>,
    mut audio_query: Query<(
        Entity,
        &DialogueAudio,
        &mut Transform,
        Option<&SpatialAudioSink>,
    )>,
) {
    for (entity, dialogue, mut transform, sink_opt) in audio_query.iter_mut() {
        // Check if NPC still exists
        let Ok(npc_pos) = npc_positions.get(dialogue.npc_entity) else {
            commands.entity(entity).despawn();
            continue;
        };

        // Update position
        transform.translation = npc_pos.0;

        // Check if audio finished (sink doesn't exist or is empty)
        if let Some(sink) = sink_opt {
            if sink.empty() {
                commands.entity(entity).despawn();
            }
        }
    }
}

/// Override NPC rotation when they should look at player
pub fn apply_look_at_player(
    time: Res<Time>,
    mut npcs: Query<(&mut Transform, &LookingAtPlayer, Option<&Health>), With<DialogueNpc>>,
) {
    let dt = time.delta_secs();
    let turn_speed = 5.0; // Radians per second

    for (mut transform, look_at, health_opt) in npcs.iter_mut() {
        // Dead NPCs don't look at player
        if health_opt.is_some_and(|h| h.is_dead()) {
            continue;
        }
        // Smoothly rotate toward player
        let current_yaw = transform.rotation.to_euler(EulerRot::YXZ).0;
        let target_yaw = look_at.target_yaw;

        // Normalize angle difference
        let mut diff = target_yaw - current_yaw;
        while diff > std::f32::consts::PI {
            diff -= std::f32::consts::TAU;
        }
        while diff < -std::f32::consts::PI {
            diff += std::f32::consts::TAU;
        }

        // Apply rotation
        let turn_amount = turn_speed * dt;
        let new_yaw = if diff.abs() < turn_amount {
            target_yaw
        } else {
            current_yaw + diff.signum() * turn_amount
        };

        transform.rotation = Quat::from_rotation_y(new_yaw);
    }
}

// =============================================================================
// PLUGIN
// =============================================================================

pub struct DialoguePlugin;

impl Plugin for DialoguePlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(Startup, setup_dialogue_assets);
        app.add_systems(
            Update,
            (
                setup_dialogue_npcs,
                update_dialogue,
                handle_dialogue_queue, // Process queued requests (respects max_dialogue limit)
                update_dialogue_audio_positions,
                apply_look_at_player,
            )
                .chain()
                .run_if(in_state(GameState::Playing)),
        );
    }
}
