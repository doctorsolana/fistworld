//! visuals systems.

use super::*;

/// Spawn 3D visuals for new ground items
pub(super) fn spawn_ground_item_visuals(
    mut commands: Commands,
    weapon_models: Option<Res<WeaponModelAssets>>,
    item_models: Option<Res<ItemModelAssets>>,
    new_items: Query<(Entity, &GroundItem, &GroundItemPosition), Added<GroundItemPosition>>,
    existing_visuals: Query<&GroundItemVisual>,
) {
    for (entity, item, pos) in new_items.iter() {
        // Check if visual already exists for this entity
        let already_exists = existing_visuals.iter().any(|v| v.source_entity == entity);
        if already_exists {
            continue;
        }

        let mut transform = Transform::from_translation(pos.0);

        let scene = if let ItemType::Weapon(weapon_type) = item.item_type {
            let weapon_scene = weapon_models
                .as_ref()
                .and_then(|assets| assets.scenes.get(&weapon_type))
                .cloned();
            let Some(weapon_scene) = weapon_scene else {
                continue;
            };
            transform.scale = Vec3::splat(0.45);
            weapon_scene
        } else {
            let ammo_scene = item_models
                .as_ref()
                .and_then(|assets| assets.scenes.get(&item.item_type))
                .cloned();
            let Some(ammo_scene) = ammo_scene else {
                continue;
            };
            transform.scale = Vec3::splat(item_model_scale(item.item_type));
            ammo_scene
        };

        let mut entity_cmd = commands.spawn((
            GroundItemVisual {
                source_entity: entity,
                bob_timer: rand::random::<f32>() * std::f32::consts::TAU, // Random start phase
                base_y: pos.0.y,
            },
            transform,
            GlobalTransform::default(),
            Visibility::Inherited,
            InheritedVisibility::default(),
        ));

        entity_cmd.insert(SceneRoot(scene));

        info!(
            "Spawned visual for ground item: {}x {}",
            item.quantity,
            item.item_type.display_name()
        );
    }
}

/// Animate ground items with bobbing and rotation
pub(super) fn animate_ground_items(
    time: Res<Time>,
    mut visuals: Query<(&mut GroundItemVisual, &mut Transform)>,
) {
    let dt = time.delta_secs();
    let bob_speed = 2.0; // Speed of bobbing
    let bob_height = 0.15; // Height of bob
    let rotation_speed = 1.5; // Rotation speed

    for (mut visual, mut transform) in visuals.iter_mut() {
        // Update timer
        visual.bob_timer += dt * bob_speed;

        // Bob up and down
        let bob_offset = visual.bob_timer.sin() * bob_height;
        transform.translation.y = visual.base_y + bob_offset + 0.3; // +0.3 to float above ground

        // Rotate slowly
        transform.rotate_y(dt * rotation_speed);
    }
}

/// Despawn visuals when their source entity is removed
pub(super) fn despawn_ground_item_visuals(
    mut commands: Commands,
    visuals: Query<(Entity, &GroundItemVisual)>,
    ground_items: Query<Entity, With<GroundItem>>,
) {
    for (visual_entity, visual) in visuals.iter() {
        // If the source entity no longer exists, despawn the visual
        if ground_items.get(visual.source_entity).is_err() {
            commands.entity(visual_entity).despawn();
        }
    }
}

/// Cleanup all item visuals when leaving playing state
pub(super) fn cleanup_item_visuals(
    mut commands: Commands,
    visuals: Query<Entity, With<GroundItemVisual>>,
) {
    for entity in visuals.iter() {
        commands.entity(entity).despawn();
    }
}
