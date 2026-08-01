//! Settlements on the client: draw the moot hall, and say where places are.
//!
//! A settlement replicates as a NAME, a TIER and a POSITION -- not as a bag of
//! buildings. Its moot hall is drawn here from that position, because buildings
//! are how a settlement's plan gets expressed rather than what constitutes it
//! (WORLD-DESIGN section 1). That is also why the hall is not replicated: it is
//! derivable, so sending it would be sending the same fact twice.

use bevy::prelude::*;

use shared::building::{BuildingPosition, PlacedBuilding};
use shared::components::{
    ConstructionSite, PlayerPosition, PlayerRotation, Settlement, SettlementBuilding,
    SettlementBuildingKind,
};
use shared::terrain::WorldTerrain;

use crate::states::GameState;

pub struct SettlementPlugin;

impl Plugin for SettlementPlugin {
    fn build(&self, app: &mut App) {
        app.add_systems(
            Update,
            (
                attach_settlement_visuals,
                attach_building_visuals,
                claim_building_ground,
                raise_construction_visuals,
            )
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

/// A building part-way out of the ground, with its own clock.
#[derive(Component)]
struct RaisingVisual {
    elapsed: f32,
    /// How far it started below the ground, so the lerp has a floor.
    sunk: f32,
}

/// Draw a building rising out of its plot while it is being raised.
///
/// The server sends ONE bit — `raising` flips true when the ground is cleared —
/// and the clock runs here. Streaming a progress float instead would re-send
/// every site to every client at tick rate, because sites replicate globally.
/// The cost of the local clock is that it starts a network hop late, which at
/// ten seconds nobody can see.
fn raise_construction_visuals(
    mut commands: Commands,
    time: Res<Time>,
    asset_server: Res<AssetServer>,
    terrain: Option<Res<WorldTerrain>>,
    mut sites: Query<(
        Entity,
        &ConstructionSite,
        &PlayerPosition,
        Option<&mut RaisingVisual>,
        Option<&BuildingVisual>,
    )>,
    mut transforms: Query<&mut Transform>,
) {
    let Some(terrain) = terrain else {
        return;
    };
    for (entity, site, position, raising, drawn) in sites.iter_mut() {
        if !site.raising {
            continue;
        }
        let ground = terrain.get_height(position.0.x, position.0.z);
        let Some(mut raising) = raising else {
            // First frame of the raise: put the model in, fully underground.
            let Some(scene) = site.kind.art().scene_path() else {
                continue;
            };
            let sunk = site.kind.art().definition().height.max(1.0);
            if drawn.is_none() {
                commands.entity(entity).insert((
                    BuildingVisual,
                    RaisingVisual { elapsed: 0.0, sunk },
                    Name::new(format!("{} rising", site.kind.label())),
                    WorldAssetRoot(asset_server.load(scene)),
                    Transform::from_xyz(position.0.x, ground - sunk, position.0.z)
                        .with_rotation(Quat::from_rotation_y(site.rotation)),
                    Visibility::Inherited,
                ));
            }
            continue;
        };

        raising.elapsed += time.delta_secs();
        let t = (raising.elapsed / shared::components::SETTLEMENT_RAISE_SECONDS).clamp(0.0, 1.0);
        // Ease out: it breaks ground quickly and settles, which reads as being
        // pushed up rather than as a linear lift.
        let eased = 1.0 - (1.0 - t) * (1.0 - t);
        if let Ok(mut transform) = transforms.get_mut(entity) {
            transform.translation.y = ground - raising.sunk * (1.0 - eased);
        }
    }
}

/// Claim the ground under anything a settlement has built or is building.
///
/// DERIVED, not replicated. `PlacedBuilding` + `BuildingPosition` are what the
/// build-zone system keys on to stop scattering props inside a building, and
/// every input needed to produce them — kind, position, rotation — already
/// arrives with the building itself. Replicating them as well would be sending
/// the same fact twice, which is the same reason the moot hall is drawn from
/// the settlement's position rather than sent as its own entity.
///
/// It applies to CONSTRUCTION SITES too, and that is the point: the plot is
/// claimed and cleared while the frame is still going up, so the building never
/// appears standing in a thicket.
fn claim_building_ground(
    mut commands: Commands,
    built: Query<
        (Entity, &SettlementBuilding, &PlayerPosition, &PlayerRotation),
        Without<PlacedBuilding>,
    >,
    sites: Query<(Entity, &ConstructionSite, &PlayerPosition), Without<PlacedBuilding>>,
) {
    for (entity, building, position, rotation) in built.iter() {
        commands.entity(entity).insert((
            PlacedBuilding {
                building_type: building.kind.art(),
                rotation: rotation.0,
            },
            BuildingPosition(position.0),
        ));
    }
    // Sites carry their rotation now, so the cleared patch is turned exactly
    // like the building that will stand on it.
    for (entity, site, position) in sites.iter() {
        commands.entity(entity).insert((
            PlacedBuilding {
                building_type: site.kind.art(),
                rotation: site.rotation,
            },
            BuildingPosition(position.0),
        ));
    }
}

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
        // Through the KIND, not a hardcoded model: the hall was a LogCabin
        // placeholder and is now the TownHall, and a second copy of that fact
        // here is how the hall and the panel end up disagreeing about what a
        // hall is.
        let Some(scene) = SettlementBuildingKind::Hall.art().scene_path() else {
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
