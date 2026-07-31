//! Settlements on the client: draw the moot hall, and say where places are.
//!
//! A settlement replicates as a NAME, a TIER and a POSITION -- not as a bag of
//! buildings. Its moot hall is drawn here from that position, because buildings
//! are how a settlement's plan gets expressed rather than what constitutes it
//! (WORLD-DESIGN section 1). That is also why the hall is not replicated: it is
//! derivable, so sending it would be sending the same fact twice.

use bevy::prelude::*;

use shared::building::BuildingType;
use shared::components::{PlayerPosition, PlayerRotation, Settlement, SettlementBuilding};
use shared::terrain::WorldTerrain;

use crate::states::GameState;

pub struct SettlementPlugin;

impl Plugin for SettlementPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (attach_settlement_visuals, attach_building_visuals)
                .run_if(in_state(GameState::Playing)),
        );
    }
}

/// Marks a settlement that already has its hall drawn.
#[derive(Component)]
pub struct SettlementVisual;

/// Marks a settlement building that already has its model drawn.
#[derive(Component)]
pub struct BuildingVisual;

/// Give every replicated settlement building its model.
///
/// The client knows nothing about permits, needs or siting -- it receives a
/// building that exists and draws it. Every decision that put it there was the
/// villagers', on the server.
fn attach_building_visuals(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    terrain: Option<Res<WorldTerrain>>,
    built: Query<
        (Entity, &SettlementBuilding, &PlayerPosition, &PlayerRotation),
        Without<BuildingVisual>,
    >,
) {
    let Some(terrain) = terrain else {
        return;
    };
    for (entity, building, position, rotation) in built.iter() {
        // The semantic kind chooses its own art, so re-skinning a Farmstead
        // never touches a rule.
        let Some(scene) = building.kind.art().scene_path() else {
            continue;
        };
        let ground = terrain.get_height(position.0.x, position.0.z);
        commands.entity(entity).insert((
            BuildingVisual,
            Name::new(format!("{} ({})", building.kind.label(), building.settlement)),
            WorldAssetRoot(asset_server.load(scene)),
            Transform::from_xyz(position.0.x, ground, position.0.z)
                .with_rotation(Quat::from_rotation_y(rotation.0)),
            Visibility::Inherited,
        ));
        info!(
            "{} of {} drawn at {:.0},{:.0}{}",
            building.kind.label(),
            building.settlement,
            position.0.x,
            position.0.z,
            building
                .owner
                .as_deref()
                .map(|owner| format!(" (owner: {owner})"))
                .unwrap_or_default(),
        );
    }
}

/// Give every replicated settlement a moot hall.
///
/// Polls `Without<SettlementVisual>` rather than reacting to `Added<Settlement>`
/// because replication delivers a settlement's components in separate batches --
/// the same reason the character visual path polls. A one-shot on `Added` would
/// miss any settlement whose position arrived on a later tick, and it would
/// stay invisible forever.
fn attach_settlement_visuals(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    terrain: Option<Res<WorldTerrain>>,
    founded: Query<(Entity, &Settlement, &PlayerPosition), Without<SettlementVisual>>,
) {
    let Some(terrain) = terrain else {
        return;
    };
    for (entity, settlement, position) in founded.iter() {
        // The hall stands on the ground, not at the replicated Y: the server
        // snapped it once at founding, but terrain deltas can move under it.
        let ground = terrain.get_height(position.0.x, position.0.z);
        // The hall is a LogCabin for now -- a placeholder with the right
        // silhouette until a real hall model exists.
        let Some(scene) = BuildingType::LogCabin.scene_path() else {
            continue;
        };

        commands.entity(entity).insert((
            SettlementVisual,
            Name::new(format!("Settlement({})", settlement.name)),
            WorldAssetRoot(asset_server.load(scene)),
            Transform::from_xyz(position.0.x, ground, position.0.z),
            Visibility::Inherited,
        ));
        info!(
            "Settlement '{}' ({}) drawn at {:.0},{:.0}",
            settlement.name,
            settlement.tier.label(),
            position.0.x,
            position.0.z
        );
    }
}
