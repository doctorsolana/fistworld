//! Replicated agricultural ground and fishing-pier presentation.

use bevy::prelude::*;
use shared::components::{FishingPier, PlayerPosition, PlayerRotation};

#[path = "farm_fields.rs"]
mod farm_fields;
pub(super) use farm_fields::attach_farm_field_visuals;
pub use farm_fields::FarmFieldVisual;

#[derive(Component)]
pub struct FishingPierVisual;

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
