//! Settlement and building scene attachment, art selection and ground claims.

use super::animation::{BuildingDoorAnimation, DoorVisualSource, WindmillMotion};
use super::lighting::{BuildingNightLighting, WindowLighting};
use super::stock::BakeryBreadDisplay;
use bevy::prelude::*;
use shared::building::{BuildingPosition, BuildingType, PlacedBuilding};
use shared::components::{
    CivicHallLevel, CivicHallUpgradeWorksite, ConstructionSite, HouseAppearance, MarketLevel,
    PlayerPosition, PlayerRotation, Settlement, SettlementBuilding, SettlementBuildingKind,
};
use shared::terrain::WorldTerrain;

/// Marks a settlement that already has its hall drawn.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct SettlementVisual {
    pub(super) building_type: BuildingType,
}

/// Marks a settlement building that already has its model drawn.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuildingVisual {
    pub(super) building_type: BuildingType,
}

pub(super) fn building_visual_art(
    kind: SettlementBuildingKind,
    market_level: Option<&MarketLevel>,
    house: Option<&HouseAppearance>,
) -> BuildingType {
    if kind == SettlementBuildingKind::Market {
        market_level.copied().unwrap_or_default().building_type()
    } else {
        kind.art_with_house(house)
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
pub(super) fn claim_building_ground(
    mut commands: Commands,
    halls: Query<(
        Entity,
        &Settlement,
        &PlayerPosition,
        Option<&PlayerRotation>,
        Option<&CivicHallLevel>,
        Option<&PlacedBuilding>,
        Option<&BuildingPosition>,
    )>,
    built: Query<(
        Entity,
        &SettlementBuilding,
        &PlayerPosition,
        &PlayerRotation,
        Option<&MarketLevel>,
        Option<&HouseAppearance>,
        Option<&PlacedBuilding>,
        Option<&BuildingPosition>,
    )>,
    sites: Query<
        (
            Entity,
            &ConstructionSite,
            &PlayerPosition,
            Option<&CivicHallUpgradeWorksite>,
            Option<&HouseAppearance>,
        ),
        Without<PlacedBuilding>,
    >,
) {
    for (entity, settlement, position, rotation, level, placed, building_position) in halls.iter() {
        let level = level
            .copied()
            .unwrap_or_else(|| CivicHallLevel::for_tier(settlement.tier));
        let desired = PlacedBuilding {
            building_type: level.building_type(),
            rotation: rotation.map_or(0.0, |rotation| rotation.0),
        };
        if placed != Some(&desired) {
            commands.entity(entity).insert(desired);
        }
        if building_position.is_none_or(|current| current.0 != position.0) {
            commands.entity(entity).insert(BuildingPosition(position.0));
        }
    }
    for (entity, building, position, rotation, market_level, house, placed, building_position) in
        built.iter()
    {
        let desired = PlacedBuilding {
            building_type: building_visual_art(building.kind, market_level, house),
            rotation: rotation.0,
        };
        if placed != Some(&desired) {
            commands.entity(entity).insert(desired);
        }
        if building_position.is_none_or(|current| current.0 != position.0) {
            commands.entity(entity).insert(BuildingPosition(position.0));
        }
    }
    // Sites carry their rotation now, so the cleared patch is turned exactly
    // like the building that will stand on it.
    for (entity, site, position, hall_upgrade, house) in sites.iter() {
        commands.entity(entity).insert((
            PlacedBuilding {
                building_type: hall_upgrade
                    .map(|upgrade| upgrade.target.building_type())
                    .unwrap_or_else(|| site.kind.art_with_house(house)),
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
pub(super) fn attach_building_visuals(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut meshes: ResMut<Assets<Mesh>>,
    mut materials: ResMut<Assets<StandardMaterial>>,
    terrain: Option<Res<WorldTerrain>>,
    built: Query<(
        Entity,
        &SettlementBuilding,
        &PlayerPosition,
        &PlayerRotation,
        Option<&MarketLevel>,
        Option<&HouseAppearance>,
        Option<&BuildingVisual>,
    )>,
) {
    let Some(terrain) = terrain else {
        return;
    };
    for (entity, building, position, rotation, market_level, house, visual) in built.iter() {
        // The semantic kind chooses its own art, so re-skinning a Farmstead
        // never touches a rule.
        let art = building_visual_art(building.kind, market_level, house);
        if visual.is_some_and(|visual| visual.building_type == art) {
            continue;
        }
        let definition = art.definition();
        let ground = terrain.get_height(position.0.x, position.0.z);
        let common = (
            BuildingVisual { building_type: art },
            Name::new(format!(
                "{} ({})",
                building.kind.label(),
                building.settlement
            )),
            Visibility::Inherited,
        );
        // Level changes reuse the authoritative building root. Clear wiring
        // that points into the old scene so asynchronous setup can discover
        // the replacement anchors and animation players.
        commands
            .entity(entity)
            .remove::<BuildingDoorAnimation>()
            .remove::<WindmillMotion>()
            .remove::<BuildingNightLighting>()
            .remove::<BakeryBreadDisplay>()
            .remove::<WindowLighting>()
            .remove::<DoorVisualSource>();
        if let Some(scene) = art.scene_path() {
            let gltf_path = scene.split('#').next().unwrap_or(scene).to_string();
            commands
                .entity(entity)
                .remove::<Mesh3d>()
                .remove::<MeshMaterial3d<StandardMaterial>>()
                .insert((
                    common,
                    DoorVisualSource {
                        kind: building.kind,
                        building_type: art,
                        gltf: asset_server.load(gltf_path),
                    },
                    WorldAssetRoot(asset_server.load(scene)),
                    Transform::from_xyz(position.0.x, ground, position.0.z)
                        .with_rotation(Quat::from_rotation_y(rotation.0)),
                ));
        } else {
            let mesh = meshes.add(Cuboid::new(
                definition.footprint.x,
                definition.height,
                definition.footprint.y,
            ));
            let material = materials.add(StandardMaterial {
                base_color: definition.color,
                perceptual_roughness: 0.92,
                ..default()
            });
            commands.entity(entity).remove::<WorldAssetRoot>().insert((
                common,
                Mesh3d(mesh),
                MeshMaterial3d(material),
                Transform::from_xyz(position.0.x, ground + definition.height * 0.5, position.0.z)
                    .with_rotation(Quat::from_rotation_y(rotation.0)),
            ));
        }
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

/// Draw the physical Hall rung without ever replacing the settlement entity.
///
/// Polling also handles replication arriving in separate batches. Changing a
/// `WorldAssetRoot` is a supported Bevy operation: its spawner removes the old
/// instance and attaches the new one to this same root, preserving selection,
/// inventory, queues and every replicated component.
pub(super) fn attach_settlement_visuals(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    terrain: Option<Res<WorldTerrain>>,
    founded: Query<(
        Entity,
        &Settlement,
        &PlayerPosition,
        Option<&CivicHallLevel>,
        Option<&SettlementVisual>,
    )>,
) {
    let Some(terrain) = terrain else {
        return;
    };
    for (entity, settlement, position, level, visual) in founded.iter() {
        let level = level
            .copied()
            .unwrap_or_else(|| CivicHallLevel::for_tier(settlement.tier));
        let art = level.building_type();
        if visual.is_some_and(|visual| visual.building_type == art) {
            continue;
        }
        // The hall stands on the ground, not at the replicated Y: the server
        // snapped it once at founding, but terrain deltas can move under it.
        let ground = terrain.get_height(position.0.x, position.0.z);
        let Some(scene) = art.scene_path() else {
            continue;
        };
        let gltf_path = scene.split('#').next().unwrap_or(scene).to_string();

        commands
            .entity(entity)
            .remove::<(BuildingDoorAnimation, WindowLighting)>()
            .insert((
                SettlementVisual { building_type: art },
                DoorVisualSource {
                    kind: SettlementBuildingKind::Hall,
                    building_type: art,
                    gltf: asset_server.load(gltf_path),
                },
                Name::new(format!("{} ({})", level.label(), settlement.name)),
                WorldAssetRoot(asset_server.load(scene)),
                Transform::from_xyz(position.0.x, ground, position.0.z),
                Visibility::Inherited,
            ));
        info!(
            "Settlement '{}' ({}) drew {} at {:.0},{:.0}",
            settlement.name,
            settlement.tier.label(),
            level.label(),
            position.0.x,
            position.0.z
        );
    }
}
