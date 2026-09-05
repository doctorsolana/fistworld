//! Replicated farm-field and fishing-pier scene attachment.

use bevy::prelude::*;
use shared::components::{FarmField, FishingPier, PlayerPosition, PlayerRotation};

#[derive(Component)]
pub struct FarmFieldVisual;

#[derive(Component)]
pub struct FishingPierVisual;

/// Draw the collider-free wheat plot paired with a completed Farmstead.
pub(super) fn attach_farm_field_visuals(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    fields: Query<(Entity, &FarmField, &PlayerPosition, &PlayerRotation), Without<FarmFieldVisual>>,
) {
    for (entity, field, position, rotation) in fields.iter() {
        let scene = shared::props::PropKind::WheatField.scene_path();
        commands.entity(entity).insert((
            FarmFieldVisual,
            Name::new(format!("Wheat field ({})", field.settlement)),
            WorldAssetRoot(asset_server.load(scene)),
            Transform::from_translation(position.0)
                .with_rotation(Quat::from_rotation_y(rotation.0)),
            Visibility::Inherited,
        ));
    }
}

/// Draw the authored, collider-free pier paired with a Fisherman's Hut.
pub(super) fn attach_fishing_pier_visuals(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    piers: Query<
        (Entity, &FishingPier, &PlayerPosition, &PlayerRotation),
        Without<FishingPierVisual>,
    >,
) {
    for (entity, pier, position, rotation) in piers.iter() {
        let scene = shared::props::PropKind::FishingPier.scene_path();
        commands.entity(entity).insert((
            FishingPierVisual,
            Name::new(format!("Fishing pier ({})", pier.settlement)),
            WorldAssetRoot(asset_server.load(scene)),
            Transform::from_translation(position.0)
                .with_rotation(Quat::from_rotation_y(rotation.0)),
            Visibility::Inherited,
        ));
    }
}
