//! spawn systems.

use super::*;

/// Ensure exactly one `Player` entity is tagged as `LocalPlayer`, based on our `LocalId`.
///
/// Why this exists:
/// - The first replicated `Player` can arrive while we're still in `GameState::Connecting`,
///   and any `Added<Player>` systems gated to `Playing` will miss it.
/// - On higher-latency links (e.g. Fly.io), component insertion order can vary; we want the
///   camera/terrain/UI to *always* converge on the correct local entity.
pub fn ensure_local_player_tag(
    mut commands: Commands,
    client_query: Query<&LocalId, (With<crate::GameClient>, With<Connected>)>,
    players: Query<(Entity, &Player)>,
    existing_local: Query<Entity, With<LocalPlayer>>,
    children_q: Query<&Children>,
    model_roots: Query<Entity, With<PlayerModelRoot>>,
    existing_local_models: Query<Entity, With<LocalPlayerModel>>,
    mut did_log: Local<bool>,
) {
    let Some(our_peer_id) = client_query.iter().next().map(|r| r.0) else {
        return;
    };

    // Find the player entity that belongs to us.
    let mut matches: Vec<Entity> = Vec::new();
    for (e, p) in players.iter() {
        if p.client_id == our_peer_id {
            matches.push(e);
        }
    }

    let Some(local_entity) = matches.first().copied() else {
        return;
    };

    if matches.len() > 1 && !*did_log {
        warn!(
            "Multiple Player entities matched our peer id {:?} (count={}); selecting the first. This usually indicates a reconnect/duplication issue.",
            our_peer_id,
            matches.len()
        );
        *did_log = true;
    }

    // Enforce exactly one LocalPlayer.
    for e in existing_local.iter() {
        if e != local_entity {
            commands.entity(e).remove::<LocalPlayer>();
        }
    }
    commands.entity(local_entity).insert(LocalPlayer);

    // Try to enforce exactly one LocalPlayerModel as well (used by visibility toggling).
    // Model might not exist yet if assets are still loading, so this is best-effort and
    // will converge once the child exists.
    let mut local_model_root: Option<Entity> = None;
    if let Ok(children) = children_q.get(local_entity) {
        for child in children.iter() {
            if model_roots.get(child).is_ok() {
                local_model_root = Some(child);
                break;
            }
        }
    }

    if let Some(model_root) = local_model_root {
        for e in existing_local_models.iter() {
            if e != model_root {
                commands.entity(e).remove::<LocalPlayerModel>();
            }
        }
        commands.entity(model_root).insert(LocalPlayerModel);
    }
}

/// Handle player spawn visuals
pub fn handle_player_spawned(
    mut commands: Commands,
    character_assets: Option<Res<PlayerCharacterAssets>>,
    // In Lightyear 0.25:
    // - `RemoteId` on the client entity refers to the SERVER
    // - `LocalId` refers to US (the local client peer id)
    client_query: Query<&LocalId, (With<crate::GameClient>, With<Connected>)>,
    new_players: Query<(Entity, &Player, &PlayerPosition, Option<&PlayerCharacter>), Added<Player>>,
) {
    let Some(character_assets) = character_assets else {
        // Should exist (loaded at Startup), but don't crash if not.
        return;
    };

    // Get our peer ID from the connected client entity
    let our_peer_id = client_query.iter().next().map(|r| r.0);

    for (entity, player, position, character) in new_players.iter() {
        info!("Player spawned: {:?}", player.client_id);

        let is_local = our_peer_id
            .map(|id| player.client_id == id)
            .unwrap_or(false);
        let character = character.copied().unwrap_or_default();

        // The replicated player entity is server-authoritative; we only add visuals here.
        // IMPORTANT: Full spatial bundle (including GlobalTransform) to avoid B0004 warnings
        // when the model hierarchy is spawned as children.
        commands.entity(entity).insert((
            Transform::from_translation(position.0),
            GlobalTransform::from_translation(position.0),
            Visibility::Inherited,
            InheritedVisibility::default(),
        ));

        if let Some(model_entity) =
            spawn_player_model(&mut commands, character, &character_assets, is_local)
        {
            commands.entity(entity).queue_silenced(
                |mut parent: bevy::ecs::world::EntityWorldMut| {
                    if !parent.contains::<Transform>() {
                        parent.insert(Transform::default());
                    }
                    if !parent.contains::<GlobalTransform>() {
                        parent.insert(GlobalTransform::default());
                    }
                    if !parent.contains::<Visibility>() {
                        parent.insert(Visibility::Inherited);
                    }
                    if !parent.contains::<InheritedVisibility>() {
                        parent.insert(InheritedVisibility::default());
                    }
                },
            );
            commands.entity(entity).add_child(model_entity);
        }

        if is_local {
            commands.entity(entity).insert(LocalPlayer);
            info!("Local player spawned!");
        }
    }
}

/// Ensure the player model matches the replicated PlayerCharacter.
pub fn sync_player_character_models(
    mut commands: Commands,
    character_assets: Option<Res<PlayerCharacterAssets>>,
    players: Query<
        (Entity, &PlayerCharacter, Option<&Children>),
        (
            With<Player>,
            With<GlobalTransform>,
            Or<(Added<PlayerCharacter>, Changed<PlayerCharacter>)>,
        ),
    >,
    model_roots: Query<(Entity, &PlayerModelRoot)>,
    local_players: Query<(), With<LocalPlayer>>,
    children_q: Query<&Children>,
) {
    let Some(character_assets) = character_assets else {
        return;
    };

    for (player_entity, character, children) in players.iter() {
        let mut existing_roots: Vec<(Entity, PlayerCharacter)> = Vec::new();
        if let Some(children) = children {
            for child in children.iter() {
                if let Ok((root_entity, root_info)) = model_roots.get(child) {
                    existing_roots.push((root_entity, root_info.character));
                }
            }
        }

        let is_local = local_players.contains(player_entity);
        let has_matching = existing_roots.iter().any(|(_, c)| *c == *character);

        if has_matching {
            // Clean up any mismatched roots left behind.
            for (root_entity, root_character) in existing_roots {
                if root_character != *character {
                    despawn_with_children(&mut commands, &children_q, root_entity);
                } else if is_local {
                    commands.entity(root_entity).insert(LocalPlayerModel);
                }
            }
            continue;
        }

        // Remove any existing model roots before spawning the new one.
        for (root_entity, _) in existing_roots {
            despawn_with_children(&mut commands, &children_q, root_entity);
        }

        if let Some(model_entity) =
            spawn_player_model(&mut commands, *character, &character_assets, is_local)
        {
            commands.entity(player_entity).queue_silenced(
                |mut parent: bevy::ecs::world::EntityWorldMut| {
                    if !parent.contains::<Transform>() {
                        parent.insert(Transform::default());
                    }
                    if !parent.contains::<GlobalTransform>() {
                        parent.insert(GlobalTransform::default());
                    }
                    if !parent.contains::<Visibility>() {
                        parent.insert(Visibility::Inherited);
                    }
                    if !parent.contains::<InheritedVisibility>() {
                        parent.insert(InheritedVisibility::default());
                    }
                },
            );
            commands.entity(player_entity).add_child(model_entity);
        }
    }
}

fn despawn_with_children(commands: &mut Commands, children_q: &Query<&Children>, entity: Entity) {
    if let Ok(children) = children_q.get(entity) {
        for child in children.iter() {
            despawn_with_children(commands, children_q, child);
        }
    }
    commands.entity(entity).despawn();
}

fn spawn_player_model(
    commands: &mut Commands,
    character: PlayerCharacter,
    character_assets: &PlayerCharacterAssets,
    is_local: bool,
) -> Option<Entity> {
    let Some(assets) = character_assets.characters.get(&character) else {
        warn!("Missing player character assets for {:?}", character);
        return None;
    };

    let model_entity = commands
        .spawn((
            PlayerModelRoot { character },
            NeedsPlayerRigSetup,
            NoFrustumCulling,
            SceneRoot(assets.scene.clone()),
            // PlayerPosition is at capsule center; drop model so feet touch ground.
            Transform::from_xyz(0.0, -PLAYER_HEIGHT * 0.5, 0.0)
                .with_rotation(Quat::from_rotation_y(assets.model_yaw_offset))
                .with_scale(Vec3::splat(assets.model_scale)),
            GlobalTransform::default(),
            Visibility::Inherited,
            InheritedVisibility::default(),
        ))
        .id();

    if is_local {
        commands.entity(model_entity).insert(LocalPlayerModel);
    }

    Some(model_entity)
}
