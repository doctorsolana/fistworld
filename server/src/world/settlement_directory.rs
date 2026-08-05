//! Global settlement directory and region-scoped detail tagging.

use bevy::platform::collections::{HashMap, HashSet};
use bevy::prelude::*;
use lightyear::prelude::{NetworkTarget, Replicate};
use shared::components::{
    BuildingOf, ConstructionSite, FarmField, FishingPier, PlayerPosition, Settlement,
    SettlementBuilding, SettlementBuildingKind, SettlementId, SettlementSummary, VillageRoad,
};
use shared::economy::SettlementEconomy;
use shared::region::RegionCoord;
use shared::terrain::TerrainDeltaChunk;

#[derive(Resource, Default)]
pub struct SettlementDirectory {
    entries: HashMap<SettlementId, Entity>,
}

/// Keep one tiny globally replicated record per settlement. Detailed hall
/// state remains on the region-tagged hall entity and therefore only exists on
/// clients who can currently observe it.
pub fn sync_settlement_directory(
    mut commands: Commands,
    mut directory: ResMut<SettlementDirectory>,
    existing: Query<(Entity, &SettlementSummary)>,
    halls: Query<(
        &SettlementId,
        &Settlement,
        &PlayerPosition,
        Option<&SettlementEconomy>,
    )>,
    buildings: Query<(&BuildingOf, &SettlementBuilding)>,
) {
    for (entity, summary) in existing.iter() {
        directory.entries.entry(summary.id).or_insert(entity);
    }

    // Count physical detail once. The previous nested hall/building scan grew
    // as settlements × buildings and made the global directory unnecessarily
    // expensive in large worlds.
    let mut building_counts: HashMap<SettlementId, [u16; 4]> = HashMap::new();
    for (owner, building) in buildings.iter() {
        let index = match building.kind {
            SettlementBuildingKind::House => 0,
            SettlementBuildingKind::Farmstead => 1,
            SettlementBuildingKind::FishermansHut => 2,
            SettlementBuildingKind::LumberjackHut => 3,
            _ => continue,
        };
        let counts = building_counts.entry(owner.0).or_default();
        counts[index] = counts[index].saturating_add(1);
    }

    let mut live = HashSet::new();
    for (id, settlement, position, economy) in halls.iter() {
        live.insert(*id);
        let [houses, farmsteads, fishing_huts, lumber_huts] =
            building_counts.get(id).copied().unwrap_or_default();
        let summary = SettlementSummary {
            id: *id,
            name: settlement.name.clone(),
            tier: settlement.tier,
            residents: settlement.residents,
            treasury: settlement.treasury,
            prosperity: economy.map_or(0.0, |economy| economy.prosperity),
            reserve_days: economy.map_or(0.0, |economy| economy.reserve_days),
            houses,
            farmsteads,
            fishing_huts,
            lumber_huts,
        };
        if let Some(entity) = directory
            .entries
            .get(id)
            .copied()
            .filter(|entity| existing.get(*entity).is_ok())
        {
            let unchanged = existing
                .get(entity)
                .is_ok_and(|(_, current)| *current == summary);
            if !unchanged {
                commands.entity(entity).insert((summary, position.clone()));
            }
        } else {
            let entity = commands
                .spawn((
                    summary,
                    position.clone(),
                    Replicate::to_clients(NetworkTarget::All),
                    Name::new(format!("SettlementSummary({})", settlement.name)),
                ))
                .id();
            directory.entries.insert(*id, entity);
        }
    }

    let stale: Vec<_> = directory
        .entries
        .iter()
        .filter_map(|(id, entity)| (!live.contains(id)).then_some((*id, *entity)))
        .collect();
    for (id, entity) in stale {
        commands.entity(entity).despawn();
        directory.entries.remove(&id);
    }
}

/// Attach interest-management coordinates to all physical settlement detail,
/// including entities loaded from an older save.
pub fn tag_settlement_detail_regions(
    mut commands: Commands,
    details: Query<
        (Entity, &PlayerPosition),
        (
            Without<RegionCoord>,
            Or<(
                With<Settlement>,
                With<SettlementBuilding>,
                With<ConstructionSite>,
                With<FarmField>,
                With<FishingPier>,
            )>,
        ),
    >,
    roads: Query<(Entity, &VillageRoad), Without<RegionCoord>>,
    terrain_deltas: Query<(Entity, &TerrainDeltaChunk), Without<RegionCoord>>,
) {
    for (entity, position) in details.iter() {
        commands
            .entity(entity)
            .insert(RegionCoord::from_world_pos(position.0));
    }
    for (entity, road) in roads.iter() {
        if let Some(point) = road.points.first() {
            commands
                .entity(entity)
                .insert(RegionCoord::from_world_pos(Vec3::new(
                    point.x, 0.0, point.y,
                )));
        }
    }
    for (entity, delta) in terrain_deltas.iter() {
        commands
            .entity(entity)
            .insert(RegionCoord::from_world_pos(delta.coord.world_pos()));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::components::SettlementTier;

    #[test]
    fn directory_keeps_global_summary_and_region_scoped_detail_separate() {
        let mut app = App::new();
        app.init_resource::<SettlementDirectory>().add_systems(
            Update,
            (tag_settlement_detail_regions, sync_settlement_directory).chain(),
        );

        let hall = app
            .world_mut()
            .spawn((
                SettlementId(7),
                Settlement {
                    name: "Directory Test".into(),
                    tier: SettlementTier::Village,
                    residents: 14,
                    treasury: 321,
                },
                PlayerPosition(Vec3::new(600.0, 0.0, -25.0)),
            ))
            .id();
        let farm = app
            .world_mut()
            .spawn((
                BuildingOf(SettlementId(7)),
                SettlementBuilding {
                    kind: SettlementBuildingKind::Farmstead,
                    settlement: "Directory Test".into(),
                    owner: None,
                    quality: 0.8,
                    workers: Vec::new(),
                },
                PlayerPosition(Vec3::new(615.0, 0.0, -20.0)),
            ))
            .id();

        app.update();
        app.update();

        assert!(app.world().entity(hall).contains::<RegionCoord>());
        assert!(app.world().entity(farm).contains::<RegionCoord>());
        let summaries: Vec<(Entity, SettlementSummary)> = app
            .world_mut()
            .query::<(Entity, &SettlementSummary)>()
            .iter(app.world())
            .map(|(entity, summary)| (entity, summary.clone()))
            .collect();
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].1.id, SettlementId(7));
        assert_eq!(summaries[0].1.farmsteads, 1);
        assert!(!app.world().entity(summaries[0].0).contains::<RegionCoord>());

        // If networking or save reconciliation removes the directory entity,
        // the next sync recreates it instead of writing through a stale Entity.
        app.world_mut().despawn(summaries[0].0);
        app.update();
        app.update();
        assert_eq!(
            app.world_mut()
                .query::<&SettlementSummary>()
                .iter(app.world())
                .count(),
            1
        );
    }
}
