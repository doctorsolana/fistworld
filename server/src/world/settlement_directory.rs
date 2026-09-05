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
    building_counts: HashMap<SettlementId, [u16; 7]>,
    counts_initialized: bool,
}

/// Keep one tiny globally replicated record per settlement. Detailed hall
/// state remains on the region-tagged hall entity and therefore only exists on
/// clients who can currently observe it.
pub fn sync_settlement_directory(
    mut commands: Commands,
    mut directory: ResMut<SettlementDirectory>,
    existing: Query<(Entity, &SettlementSummary, Option<&PlayerPosition>)>,
    halls: Query<(
        &SettlementId,
        &Settlement,
        &PlayerPosition,
        Option<&SettlementEconomy>,
    )>,
    buildings: Query<(Ref<BuildingOf>, Ref<SettlementBuilding>)>,
    mut removed_buildings: RemovedComponents<SettlementBuilding>,
    mut removed_building_owners: RemovedComponents<BuildingOf>,
) {
    for (entity, summary, _) in existing.iter() {
        directory.entries.entry(summary.id).or_insert(entity);
    }

    let removed_any = !removed_buildings.is_empty() || !removed_building_owners.is_empty();
    removed_buildings.clear();
    removed_building_owners.clear();
    // Check change ticks rather than rebuilding the count map at 60 Hz. A
    // complete rebuild on edits preserves saturating counts and handles moves
    // between settlements, kind changes, component removal and despawning.
    if !directory.counts_initialized
        || removed_any
        || buildings
            .iter()
            .any(|(owner, building)| owner.is_changed() || building.is_changed())
    {
        directory.building_counts.clear();
        for (owner, building) in buildings.iter() {
            let index = match building.kind {
                SettlementBuildingKind::House => 0,
                SettlementBuildingKind::Farmstead => 1,
                SettlementBuildingKind::FishermansHut => 2,
                SettlementBuildingKind::LumberjackHut => 3,
                SettlementBuildingKind::Windmill => 4,
                SettlementBuildingKind::Bakery => 5,
                SettlementBuildingKind::Market => 6,
                _ => continue,
            };
            let counts = directory.building_counts.entry(owner.0).or_default();
            counts[index] = counts[index].saturating_add(1);
        }
        directory.counts_initialized = true;
    }

    let mut live = HashSet::new();
    for (id, settlement, position, economy) in halls.iter() {
        live.insert(*id);
        let [houses, farmsteads, fishing_huts, lumber_huts, windmills, bakeries, marketplaces] =
            directory
                .building_counts
                .get(id)
                .copied()
                .unwrap_or_default();
        let summary = SettlementSummary {
            id: *id,
            name: settlement.name.clone(),
            tier: settlement.tier,
            residents: settlement.residents,
            treasury: settlement.treasury,
            prosperity: economy.map_or(0.0, |economy| economy.prosperity),
            reserve_days: economy.map_or(0.0, |economy| economy.reserve_days),
            recent_food_production: economy.map_or(0.0, |economy| economy.recent_food_production),
            recent_food_consumption: economy.map_or(0.0, |economy| economy.recent_food_consumption),
            hungry: economy.map_or(0, |economy| economy.unmet_food),
            housing_capacity: economy.map_or(0, |economy| economy.housing_capacity),
            homeless: economy.map_or(0, |economy| economy.homeless_residents),
            job_seekers: economy.map_or(0, |economy| economy.job_seekers),
            unpaid_workers: economy.map_or(0, |economy| economy.unpaid_workers),
            unrest: economy.map_or(0.0, |economy| economy.unrest),
            unrest_change: economy.map_or(0.0, |economy| economy.unrest_change),
            unrest_target: economy.map_or(0.0, |economy| economy.unrest_target),
            unrest_hunger_pressure: economy.map_or(0.0, |economy| economy.unrest_hunger_pressure),
            unrest_housing_pressure: economy.map_or(0.0, |economy| economy.unrest_housing_pressure),
            unrest_wage_pressure: economy.map_or(0.0, |economy| economy.unrest_wage_pressure),
            houses,
            farmsteads,
            fishing_huts,
            lumber_huts,
            windmills,
            bakeries,
            has_marketplace: marketplaces > 0,
        };
        if let Some((entity, current, current_position)) = directory
            .entries
            .get(id)
            .and_then(|entity| existing.get(*entity).ok())
        {
            if *current != summary {
                commands.entity(entity).insert(summary);
            }
            if current_position != Some(position) {
                commands.entity(entity).insert(position.clone());
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

    #[derive(Resource, Default, Debug, PartialEq)]
    struct DirectoryChanges {
        summaries: usize,
        positions: usize,
    }

    fn count_directory_changes(
        mut changes: ResMut<DirectoryChanges>,
        summaries: Query<(), Changed<SettlementSummary>>,
        positions: Query<(), (With<SettlementSummary>, Changed<PlayerPosition>)>,
    ) {
        changes.summaries = summaries.iter().count();
        changes.positions = positions.iter().count();
    }

    fn spawn_hall(app: &mut App, id: SettlementId) -> Entity {
        app.world_mut()
            .spawn((
                id,
                Settlement {
                    name: format!("Settlement {}", id.0),
                    tier: SettlementTier::Village,
                    residents: 14,
                    treasury: 321,
                },
                PlayerPosition(Vec3::new(600.0, 0.0, -25.0)),
                SettlementEconomy::default(),
            ))
            .id()
    }

    #[test]
    fn directory_replication_only_changes_the_component_that_changed() {
        let mut app = App::new();
        app.init_resource::<SettlementDirectory>()
            .init_resource::<DirectoryChanges>()
            .add_systems(
                Update,
                (sync_settlement_directory, count_directory_changes).chain(),
            );
        let hall = spawn_hall(&mut app, SettlementId(7));
        app.update();
        let summary = app.world().resource::<SettlementDirectory>().entries[&SettlementId(7)];

        app.update();
        assert_eq!(
            *app.world().resource::<DirectoryChanges>(),
            DirectoryChanges::default()
        );

        app.world_mut()
            .get_mut::<SettlementEconomy>(hall)
            .unwrap()
            .prosperity = 50.0;
        app.update();
        assert_eq!(
            *app.world().resource::<DirectoryChanges>(),
            DirectoryChanges {
                summaries: 1,
                positions: 0
            },
            "statistics must not re-replicate an unchanged map position"
        );
        assert_eq!(
            app.world()
                .get::<SettlementSummary>(summary)
                .unwrap()
                .prosperity,
            50.0
        );

        let moved = PlayerPosition(Vec3::new(650.0, 0.0, -25.0));
        app.world_mut().entity_mut(hall).insert(moved.clone());
        app.update();
        assert_eq!(
            *app.world().resource::<DirectoryChanges>(),
            DirectoryChanges {
                summaries: 0,
                positions: 1
            }
        );
        assert_eq!(app.world().get::<PlayerPosition>(summary), Some(&moved));

        app.world_mut()
            .entity_mut(summary)
            .remove::<PlayerPosition>();
        app.update();
        assert_eq!(app.world().get::<PlayerPosition>(summary), Some(&moved));
    }

    #[test]
    fn cached_building_counts_follow_edits_reassignment_and_removal_batches() {
        let mut app = App::new();
        app.init_resource::<SettlementDirectory>()
            .add_systems(Update, sync_settlement_directory);
        let hall = spawn_hall(&mut app, SettlementId(7));
        spawn_hall(&mut app, SettlementId(8));
        let house = SettlementBuilding {
            kind: SettlementBuildingKind::House,
            settlement: "Settlement 7".into(),
            owner: None,
            quality: 0.8,
            workers: Vec::new(),
        };
        let first = app
            .world_mut()
            .spawn((BuildingOf(SettlementId(7)), house.clone()))
            .id();
        let second = app
            .world_mut()
            .spawn((BuildingOf(SettlementId(7)), house.clone()))
            .id();
        let counts = |app: &App| {
            [SettlementId(7), SettlementId(8)].map(|id| {
                let entity = app.world().resource::<SettlementDirectory>().entries[&id];
                let summary = app.world().get::<SettlementSummary>(entity).unwrap();
                [summary.houses, summary.farmsteads]
            })
        };

        app.update();
        assert_eq!(counts(&app), [[2, 0], [0, 0]]);
        app.update();
        assert_eq!(counts(&app), [[2, 0], [0, 0]]);
        app.world_mut()
            .get_mut::<SettlementBuilding>(first)
            .unwrap()
            .kind = SettlementBuildingKind::Farmstead;
        app.update();
        assert_eq!(counts(&app), [[1, 1], [0, 0]]);
        app.world_mut()
            .entity_mut(first)
            .insert(BuildingOf(SettlementId(8)));
        app.update();
        assert_eq!(counts(&app), [[1, 0], [0, 1]]);

        app.world_mut().entity_mut(first).remove::<BuildingOf>();
        app.world_mut()
            .entity_mut(second)
            .remove::<SettlementBuilding>();
        app.update();
        assert_eq!(counts(&app), [[0, 0], [0, 0]]);
        app.update();
        assert_eq!(counts(&app), [[0, 0], [0, 0]]);
        app.world_mut()
            .entity_mut(first)
            .insert(BuildingOf(SettlementId(7)));
        app.world_mut().entity_mut(second).insert(house);
        app.update();
        assert_eq!(counts(&app), [[1, 1], [0, 0]]);

        app.world_mut().despawn(first);
        app.world_mut().despawn(second);
        app.update();
        assert_eq!(counts(&app), [[0, 0], [0, 0]]);
        let summary = app.world().resource::<SettlementDirectory>().entries[&SettlementId(7)];
        app.world_mut().despawn(hall);
        app.update();
        assert!(app.world().get_entity(summary).is_err());
    }

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
