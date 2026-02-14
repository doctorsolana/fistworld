use bevy::prelude::*;
use shared::components::{EquippedWeapon, LocalPlayer, Player};
use shared::weapons::WeaponType;
use std::collections::HashMap;

use crate::input::{CameraMode, InputState};

use super::WeaponModelAssets;

/// Marker for the third-person weapon model (attached to player).
#[derive(Component)]
pub struct ThirdPersonWeapon;

/// Marker for third-person weapons on remote players.
#[derive(Component)]
pub struct RemoteThirdPersonWeapon {
    pub owner: Entity,
}

/// Incremental index of remote third-person weapon ownership.
#[derive(Resource, Default)]
pub struct RemoteWeaponIndex {
    pub by_owner: HashMap<Entity, (Entity, WeaponType)>,
    pub by_weapon: HashMap<Entity, Entity>,
}

/// Track which weapon the third-person model is showing.
#[derive(Resource, Default)]
pub struct CurrentThirdPersonWeapon {
    pub weapon_type: Option<WeaponType>,
}

/// Update third-person weapon model (visible on character in third-person view).
pub fn update_third_person_weapon(
    mut commands: Commands,
    local_player: Query<(Entity, &EquippedWeapon), (With<LocalPlayer>, With<GlobalTransform>)>,
    existing_weapon: Query<Entity, With<ThirdPersonWeapon>>,
    weapon_models: Option<Res<WeaponModelAssets>>,
    mut current_tp_weapon: ResMut<CurrentThirdPersonWeapon>,
    input_state: Res<InputState>,
) {
    let Some((player_entity, weapon)) = local_player.iter().next() else {
        return;
    };

    // Only show in third-person and not in vehicle.
    let should_show = input_state.camera_mode == CameraMode::ThirdPerson
        && !input_state.in_vehicle
        && weapon.weapon_type != WeaponType::Unarmed;

    // Check if we need to spawn (not already showing this weapon).
    let already_showing = current_tp_weapon.weapon_type == Some(weapon.weapon_type);

    // Despawn old weapon if switching weapons or hiding.
    if !should_show || !already_showing {
        for entity in existing_weapon.iter() {
            commands.entity(entity).despawn();
        }
        current_tp_weapon.weapon_type = None;
    }

    if !should_show {
        return;
    }

    // Spawn new weapon model if not already showing.
    if current_tp_weapon.weapon_type.is_none() {
        info!("Spawning third-person weapon: {:?}", weapon.weapon_type);
        spawn_third_person_weapon(
            &mut commands,
            weapon_models.as_deref(),
            weapon.weapon_type,
            player_entity,
        );
        current_tp_weapon.weapon_type = Some(weapon.weapon_type);
    }
}

/// Update third-person weapon models for remote players.
pub fn update_remote_third_person_weapons(
    mut commands: Commands,
    mut index: ResMut<RemoteWeaponIndex>,
    players_to_sync: Query<
        (Entity, &EquippedWeapon),
        (
            With<Player>,
            With<GlobalTransform>,
            Without<LocalPlayer>,
            Or<(Added<Player>, Changed<EquippedWeapon>)>,
        ),
    >,
    weapon_entities: Query<(), With<RemoteThirdPersonWeapon>>,
    mut removed_players: RemovedComponents<Player>,
    mut removed_weapons: RemovedComponents<RemoteThirdPersonWeapon>,
    weapon_models: Option<Res<WeaponModelAssets>>,
) {
    for removed_weapon in removed_weapons.read() {
        if let Some(owner) = index.by_weapon.remove(&removed_weapon) {
            if index
                .by_owner
                .get(&owner)
                .is_some_and(|(entity, _)| *entity == removed_weapon)
            {
                index.by_owner.remove(&owner);
            }
        }
    }

    for removed_owner in removed_players.read() {
        if let Some((weapon_entity, _)) = index.by_owner.remove(&removed_owner) {
            index.by_weapon.remove(&weapon_entity);
            commands.entity(weapon_entity).despawn();
        }
    }

    for (player_entity, weapon) in players_to_sync.iter() {
        let desired_weapon = weapon.weapon_type;
        let existing = index.by_owner.get(&player_entity).copied();

        if desired_weapon == WeaponType::Unarmed {
            if let Some((weapon_entity, _)) = existing {
                index.by_owner.remove(&player_entity);
                index.by_weapon.remove(&weapon_entity);
                commands.entity(weapon_entity).despawn();
            }
            continue;
        }

        if let Some((weapon_entity, existing_type)) = existing {
            if existing_type == desired_weapon && weapon_entities.get(weapon_entity).is_ok() {
                continue;
            }

            index.by_owner.remove(&player_entity);
            index.by_weapon.remove(&weapon_entity);
            commands.entity(weapon_entity).despawn();
        }

        if let Some(weapon_entity) = spawn_remote_third_person_weapon(
            &mut commands,
            weapon_models.as_deref(),
            desired_weapon,
            player_entity,
        ) {
            index
                .by_owner
                .insert(player_entity, (weapon_entity, desired_weapon));
            index.by_weapon.insert(weapon_entity, player_entity);
        }
    }
}

/// Spawn a simplified third-person weapon model attached to the player.
fn spawn_third_person_weapon(
    commands: &mut Commands,
    weapon_models: Option<&WeaponModelAssets>,
    weapon_type: WeaponType,
    player_entity: Entity,
) {
    let Some(assets) = weapon_models else { return };
    let Some(scene) = assets.scenes.get(&weapon_type) else {
        return;
    };

    // Position relative to player - roughly where hands would hold a weapon.
    // Offset: slightly forward, to the right, and at chest height.
    let base_offset = Vec3::new(0.2, 0.15, -0.35);

    let weapon_entity = commands
        .spawn((
            ThirdPersonWeapon,
            Transform::from_translation(base_offset).with_rotation(Quat::from_rotation_y(-0.1)), // Slight angle.
            GlobalTransform::default(),
            Visibility::Inherited,
            InheritedVisibility::default(),
        ))
        .id();

    let model_scale = 0.6;
    commands.entity(weapon_entity).with_children(|parent| {
        parent.spawn((
            SceneRoot(scene.clone()),
            GlobalTransform::default(),
            Transform::from_scale(Vec3::splat(model_scale)),
            Visibility::Inherited,
            InheritedVisibility::default(),
        ));
    });

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

    // Make weapon a child of the player so it follows them.
    commands.entity(player_entity).add_child(weapon_entity);
}

/// Spawn a simplified third-person weapon model for a remote player.
fn spawn_remote_third_person_weapon(
    commands: &mut Commands,
    weapon_models: Option<&WeaponModelAssets>,
    weapon_type: WeaponType,
    player_entity: Entity,
) -> Option<Entity> {
    let assets = weapon_models?;
    let scene = assets.scenes.get(&weapon_type)?;

    // Position relative to player - roughly where hands would hold a weapon.
    let base_offset = Vec3::new(0.2, 0.15, -0.35);

    let weapon_entity = commands
        .spawn((
            RemoteThirdPersonWeapon {
                owner: player_entity,
            },
            Transform::from_translation(base_offset).with_rotation(Quat::from_rotation_y(-0.1)),
            GlobalTransform::default(),
            Visibility::Inherited,
            InheritedVisibility::default(),
        ))
        .id();

    let model_scale = 0.55;
    commands.entity(weapon_entity).with_children(|parent| {
        parent.spawn((
            SceneRoot(scene.clone()),
            GlobalTransform::default(),
            Transform::from_scale(Vec3::splat(model_scale)),
            Visibility::Inherited,
            InheritedVisibility::default(),
        ));
    });

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

    // Make weapon a child of the player so it follows them.
    commands.entity(player_entity).add_child(weapon_entity);
    Some(weapon_entity)
}

/// Despawn third-person weapon when leaving gameplay.
pub fn despawn_third_person_weapon(
    mut commands: Commands,
    weapons: Query<Entity, With<ThirdPersonWeapon>>,
    mut current_tp_weapon: ResMut<CurrentThirdPersonWeapon>,
) {
    for entity in weapons.iter() {
        commands.entity(entity).despawn();
    }
    current_tp_weapon.weapon_type = None;
}

/// Despawn remote third-person weapons when leaving gameplay.
pub fn despawn_remote_third_person_weapons(
    mut commands: Commands,
    weapons: Query<Entity, With<RemoteThirdPersonWeapon>>,
    mut index: ResMut<RemoteWeaponIndex>,
) {
    for entity in weapons.iter() {
        commands.entity(entity).despawn();
    }
    index.by_owner.clear();
    index.by_weapon.clear();
}
