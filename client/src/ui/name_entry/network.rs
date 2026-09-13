//! network systems.

use super::*;
use bevy::ecs::system::SystemParam;
use bevy::tasks::{block_on, poll_once, AsyncComputeTaskPool, Task};
use lightyear::prelude::*;
use shared::map::LoadedMap;
use shared::protocol::{NameRejectionReason, NameSubmissionResult, ReliableChannel};

#[derive(SystemParam)]
pub(super) struct JoinPresentation<'w> {
    input: ResMut<'w, PlayerNameInput>,
    phase: ResMut<'w, NameEntryPhase>,
    feedback: ResMut<'w, NameSubmissionFeedback>,
    connection_feedback: ResMut<'w, crate::render::systems::ConnectionFeedback>,
}

#[derive(Resource, Default)]
pub(super) struct PendingWorldJoin {
    task: Option<Task<(NameSubmissionResult, Result<LoadedMap, String>)>>,
}

pub(super) fn cancel_world_join(mut pending: ResMut<PendingWorldJoin>) {
    pending.task = None;
}

fn load_server_map(result: &NameSubmissionResult) -> Result<LoadedMap, String> {
    let NameSubmissionResult::Accepted {
        map, world_recipe, ..
    } = result
    else {
        return Err("Missing world description.".into());
    };
    let loaded = if let Some(recipe) = world_recipe {
        if map.map_id != shared::map::SESSION_MAP_ID {
            return Err("The server's world description is inconsistent.".into());
        }
        shared::map::load_session_map(recipe)?
    } else {
        if map.map_id.is_empty()
            || !map
                .map_id
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || c == b'_' || c == b'-')
        {
            return Err("The server requested an invalid map.".into());
        }
        shared::map::load_map(&map.map_id)?
    };
    if loaded.content_hash != map.content_hash
        || loaded.definition.bounds.min_vec2() != map.bounds_min
        || loaded.definition.bounds.max_vec2() != map.bounds_max
    {
        return Err("Your map differs from the server. Update the game and reconnect.".into());
    }
    Ok(loaded)
}

pub(super) fn handle_name_submission_result(
    mut next_state: ResMut<NextState<GameState>>,
    mut presentation: JoinPresentation,
    mut client_query: Query<
        (
            Entity,
            &mut MessageReceiver<NameSubmissionResult>,
            &mut MessageSender<shared::protocol::CreateHero>,
        ),
        (
            With<crate::GameClient>,
            With<PlayerNameSubmitted>,
            With<Connected>,
        ),
    >,
    mut commands: Commands,
    mut creator_open: ResMut<crate::ui::hero_creator::HeroCreatorOpen>,
    mut cinematic: ResMut<crate::boat::OpeningCinematic>,
    selected: Res<crate::hero::control::SelectedOutfit>,
    mut pending_view: ResMut<crate::camera_rts::PendingCommanderView>,
    mut pending_world: ResMut<PendingWorldJoin>,
    mut terrain: ResMut<shared::terrain::WorldTerrain>,
    mut sounds: crate::ui::sound::UiActionSounds,
) {
    let Ok((client_entity, mut receiver, mut create_sender)) = client_query.single_mut() else {
        return;
    };

    // Keep networking and the name-entry screen responsive while the worker
    // rebuilds the authoritative recipe. No character is requested early.
    let results: Vec<_> = if let Some(task) = pending_world.task.as_mut() {
        let Some((accepted, loaded)) = block_on(poll_once(task)) else {
            return;
        };
        pending_world.task = None;
        match loaded {
            Ok(loaded) => {
                info!(
                    "Server world prepared: map={} hash={:016x}",
                    loaded.definition.map_id, loaded.content_hash
                );
                terrain.replace_loaded_map(loaded);
                vec![accepted]
            }
            Err(message) => {
                sounds.emit(crate::audio::sfx::SfxCue::UiReject);
                error!("Cannot join server world: {message}");
                presentation.input.submitted = false;
                *presentation.phase = NameEntryPhase::Editing;
                presentation.connection_feedback.error_message = Some(message);
                next_state.set(GameState::MainMenu);
                commands.trigger(Disconnect {
                    entity: client_entity,
                });
                return;
            }
        }
    } else {
        receiver.receive().collect()
    };
    for result in results {
        if let NameSubmissionResult::Accepted { map, .. } = &result {
            if *map != shared::components::ActiveMapState::from_terrain(&terrain) {
                *presentation.phase = NameEntryPhase::Preparing;
                presentation.feedback.error_message = None;
                pending_world.task = Some(AsyncComputeTaskPool::get().spawn(async move {
                    let loaded = load_server_map(&result);
                    (result, loaded)
                }));
                return;
            }
        }
        match result {
            NameSubmissionResult::Accepted {
                profile_loaded,
                needs_hero_creation,
                commander_view,
                ..
            } => {
                sounds.emit(crate::audio::sfx::SfxCue::UiConfirm);
                pending_view.0 = commander_view;
                // Matching recipes do not imply matching mutable worlds: a
                // restarted server can reuse a seed with different earthworks.
                let base_deltas = terrain
                    .generator
                    .loaded_map()
                    .terrain_deltas_by_chunk
                    .clone();
                terrain.replace_delta_chunks(base_deltas);
                commands.queue(|world: &mut World| {
                    crate::terrain::reset_world_streaming(world);
                    crate::props::reset_world_streaming(world);
                    crate::water::reset_world_streaming(world);
                    crate::ui::world_map::reset_world_map(world);
                });
                if profile_loaded {
                    info!("Name accepted! Loaded existing profile");
                } else {
                    info!("Name accepted! Created new profile");
                }

                // Clear the submitted marker
                commands
                    .entity(client_entity)
                    .remove::<PlayerNameSubmitted>();

                // Transition only after the accepted server recipe is ready.
                presentation.feedback.error_message = None;
                next_state.set(GameState::Playing);
                // The automated rendering/smoke harness deliberately uses the
                // old God spawn hook. Do not put a mandatory modal in front of
                // it; ordinary players always receive the voyage creator.
                let automated_god_spawn =
                    std::env::var("FISTWORLD_AUTOSPAWN_HERO").is_ok_and(|value| value == "1");
                let automated_voyage =
                    std::env::var("FISTWORLD_AUTOCREATE_VOYAGE").is_ok_and(|value| value == "1");
                let ux_fixture = std::env::var("FISTWORLD_UX_TOWN").is_ok_and(|value| {
                    matches!(
                        value.trim().to_ascii_lowercase().as_str(),
                        "1" | "true" | "yes" | "on"
                    )
                });
                if needs_hero_creation && automated_voyage {
                    if ux_fixture {
                        cinematic.cancel();
                    } else {
                        cinematic.arm();
                    }
                    create_sender.send::<ReliableChannel>(shared::protocol::CreateHero {
                        outfit: selected.0,
                    });
                    info!("FISTWORLD_AUTOCREATE_VOYAGE: sent normal CreateHero request");
                } else if needs_hero_creation
                    && !automated_god_spawn
                    && std::env::var_os("FISTWORLD_ARMY_SCENARIO").is_none()
                    && std::env::var_os("FISTWORLD_FARM_CAPTURE_DIR").is_none()
                {
                    creator_open.0 = true;
                    if ux_fixture {
                        cinematic.cancel();
                    } else {
                        cinematic.arm();
                    }
                }
            }
            NameSubmissionResult::Rejected { reason } => {
                sounds.emit(crate::audio::sfx::SfxCue::UiReject);
                warn!("Name rejected: {:?}", reason);

                // Clear the submitted marker so user can try again
                commands
                    .entity(client_entity)
                    .remove::<PlayerNameSubmitted>();

                // Show error message
                let error_msg = match reason {
                    NameRejectionReason::InvalidCharacters => {
                        "Name contains invalid characters".to_string()
                    }
                    NameRejectionReason::TooShort => "Name is too short".to_string(),
                    NameRejectionReason::TooLong => "Name is too long".to_string(),
                    NameRejectionReason::Reserved => "This name is reserved".to_string(),
                    NameRejectionReason::AlreadyOnline => "This name is already in use".to_string(),
                };

                presentation.input.submitted = false;
                *presentation.phase = NameEntryPhase::Editing;
                presentation.feedback.error_message = Some(error_msg);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn acceptance(seed: u64) -> NameSubmissionResult {
        let mut recipe = shared::map::new_world_recipe(seed);
        recipe.half_extent = 64.0;
        let loaded = shared::map::load_session_map(&recipe).unwrap();
        NameSubmissionResult::Accepted {
            profile_loaded: false,
            needs_hero_creation: true,
            commander_view: None,
            map: shared::components::ActiveMapState {
                map_id: loaded.definition.map_id,
                content_hash: loaded.content_hash,
                bounds_min: loaded.definition.bounds.min_vec2(),
                bounds_max: loaded.definition.bounds.max_vec2(),
            },
            world_recipe: Some(recipe),
        }
    }

    #[test]
    fn join_rebuilds_server_recipe_and_rejects_content_mismatch() {
        let mut accepted = acceptance(83);
        assert!(load_server_map(&accepted).is_ok());
        if let NameSubmissionResult::Accepted {
            world_recipe: Some(recipe),
            ..
        } = &mut accepted
        {
            recipe.seed += 1;
        }
        assert!(load_server_map(&accepted).is_err());
    }

    #[test]
    fn join_rejects_an_authored_path_outside_the_map_catalog() {
        let mut accepted = acceptance(3);
        if let NameSubmissionResult::Accepted {
            map, world_recipe, ..
        } = &mut accepted
        {
            map.map_id = "../outside".into();
            *world_recipe = None;
        }
        assert!(load_server_map(&accepted).is_err());
    }
}
