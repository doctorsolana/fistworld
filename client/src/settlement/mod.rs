//! Settlements on the client: draw the city hall, and say where places are.
//!
//! A settlement replicates as a NAME, a TIER and a POSITION -- not as a bag of
//! buildings. Its city hall is drawn here from that position, because buildings
//! are how a settlement's plan gets expressed rather than what constitutes it
//! (WORLD-DESIGN section 1). That is also why the hall is not replicated: it is
//! derivable, so sending it would be sending the same fact twice.

use bevy::prelude::*;

use shared::building::BuildingType;
use shared::components::{PlayerPosition, Settlement};
use shared::terrain::WorldTerrain;

use crate::states::GameState;

pub struct SettlementPlugin;

impl Plugin for SettlementPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            attach_settlement_visuals.run_if(in_state(GameState::Playing)),
        );
    }
}

/// Marks a settlement that already has its hall drawn.
#[derive(Component)]
pub struct SettlementVisual;

/// Give every replicated settlement a city hall.
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
