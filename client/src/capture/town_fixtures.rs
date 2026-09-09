//! Render accepted town-growth snapshots through the ordinary settlement consumers.
//!
//! This fixture imports geometry; it never reruns a city generator or simulates
//! absent residents. Connected captures remain the proof for NPC movement.

use super::CaptureConfig;
use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::world_serialization::WorldInstance;
use shared::components::{
    BuildingDoorDemand, BuildingOf, PlayerPosition, PlayerRotation, RoadOf, Settlement,
    SettlementBuilding, WorldTime,
};
use shared::settlement_snapshot::TownSnapshot;
use shared::terrain::WorldTerrain;

/// Only imported roots whose normal presentation must instantiate a scene.
/// Unraised worksite stakes are procedural meshes, so they do not wait on GLB
/// instances that intentionally do not exist yet.
#[derive(Resource, Default)]
pub(super) struct TownCaptureScenes(pub(super) Vec<Entity>);

#[derive(SystemParam)]
pub(super) struct TownSceneReadiness<'w, 's> {
    expected: Option<Res<'w, TownCaptureScenes>>,
    assets: Res<'w, AssetServer>,
    spawner: Res<'w, WorldInstanceSpawner>,
    roots: Query<
        'w,
        's,
        (
            Option<&'static WorldAssetRoot>,
            Option<&'static WorldInstance>,
        ),
    >,
    walls: Query<
        'w,
        's,
        (
            &'static shared::components::FortificationSegment,
            Option<&'static crate::settlement::fortifications::FortificationVisual>,
        ),
    >,
}

impl TownSceneReadiness<'_, '_> {
    /// Terrain readiness alone can succeed while every building GLB is still
    /// loading or has failed. Reuse the scenario's existing frame budget, but
    /// require both loaded dependencies and an instantiated world for this town.
    pub(super) fn ready(&self, frames: u32, maximum_frames: u32) -> Result<bool, String> {
        let Some(expected) = self.expected.as_deref() else {
            return Ok(true);
        };
        let mut ready = 0;
        for entity in &expected.0 {
            let Ok((Some(root), instance)) = self.roots.get(*entity) else {
                continue;
            };
            if let Some((asset, dependencies, recursive)) = self.assets.get_load_states(root.id()) {
                use bevy::asset::{DependencyLoadState, LoadState, RecursiveDependencyLoadState};
                let failure = match (asset, dependencies, recursive) {
                    (LoadState::Failed(error), _, _)
                    | (_, DependencyLoadState::Failed(error), _)
                    | (_, _, RecursiveDependencyLoadState::Failed(error)) => Some(error),
                    _ => None,
                };
                if let Some(error) = failure {
                    return Err(format!("town scene {:?} failed: {error}", root.0));
                }
            }
            if self.assets.is_loaded_with_dependencies(root.id())
                && instance.is_some_and(|instance| self.spawner.instance_is_ready(**instance))
            {
                ready += 1;
            }
        }
        let walls_ready = self
            .walls
            .iter()
            .all(|(wall, visual)| crate::settlement::fortifications::visual_matches(wall, visual));
        if ready == expected.0.len() && walls_ready {
            return Ok(true);
        }
        if frames >= maximum_frames {
            return Err(format!("town scene readiness timed out: {ready}/{} imported scenes instantiated with loaded dependencies; defense meshes ready={walls_ready}", expected.0.len()));
        }
        Ok(false)
    }
}

pub(super) fn stage_capture_town(mut commands: Commands) {
    let Ok(path) = std::env::var("FISTFORCE_CAPTURE_TOWN_SNAPSHOT") else {
        return;
    };
    commands.queue(move |world: &mut World| {
        let snapshot = TownSnapshot::read(&path)
            .unwrap_or_else(|error| panic!("capture town snapshot {path}: {error}"));
        import_town(world, &snapshot)
            .unwrap_or_else(|error| panic!("capture town snapshot {path}: {error}"));
        // Keep the exact input alongside the PNG/metadata, including terrain
        // earthworks, seed and simulation time. Later comparison must not
        // silently substitute the next run's similarly named final snapshot.
        let output = world
            .resource::<CaptureConfig>()
            .out_dir
            .join("town-source.json");
        snapshot
            .write(&output)
            .unwrap_or_else(|error| panic!("cannot record {}: {error}", output.display()));
        info!(
            "capture: imported town seed={} profile={} day={} complete={} pending={} roads={}",
            snapshot.seed,
            snapshot.profile,
            snapshot.day,
            snapshot.metrics.completed_buildings,
            snapshot.metrics.pending_buildings,
            snapshot.roads.len(),
        );
    });
}

fn import_town(world: &mut World, snapshot: &TownSnapshot) -> Result<(), String> {
    snapshot.validate()?;
    let mut terrain = world.resource_mut::<WorldTerrain>();
    if terrain.generator.active_map_id() != snapshot.map_id
        || terrain.generator.active_map_content_hash() != snapshot.map_content_hash
    {
        return Err(format!(
            "snapshot map {} ({:016x}) differs from capture map {} ({:016x}); use the snapshot's map",
            snapshot.map_id, snapshot.map_content_hash,
            terrain.generator.active_map_id(), terrain.generator.active_map_content_hash(),
        ));
    }
    terrain.replace_delta_chunks(
        snapshot
            .terrain_deltas
            .iter()
            .map(|chunk| (chunk.coord, chunk.to_delta_data()))
            .collect(),
    );
    for entry in &snapshot.settlements {
        let ground = terrain.get_height(entry.position.x, entry.position.z);
        if terrain
            .water_surface_height(entry.position.x, entry.position.z)
            .is_some_and(|water| ground <= water)
        {
            return Err(format!(
                "snapshot settlement '{}' at {:.1},{:.1} is not on dry land in map {}",
                entry.name, entry.position.x, entry.position.z, snapshot.map_id,
            ));
        }
    }

    for mut clock in world.query::<&mut WorldTime>().iter_mut(world) {
        clock.day = snapshot.day;
    }
    let mut scenes = TownCaptureScenes::default();
    for entry in &snapshot.settlements {
        let entity = world
            .spawn((
                Settlement {
                    name: entry.name.clone(),
                    tier: entry.tier,
                    residents: entry.residents,
                    treasury: entry.treasury,
                },
                entry.id,
                entry.hall_level,
                entry.development.clone(),
                PlayerPosition(entry.position),
                PlayerRotation(entry.rotation),
                BuildingDoorDemand::default(),
            ))
            .id();
        if entry.hall_level.building_type().scene_path().is_some() {
            scenes.0.push(entity);
        }
        if let Some(defenses) = &entry.defenses {
            world.entity_mut(entity).insert(defenses.clone());
        }
        if let Some(square) = &entry.civic_square {
            world.entity_mut(entity).insert(square.clone());
        }
    }
    for entry in &snapshot.buildings {
        let settlement = snapshot
            .settlements
            .iter()
            .find(|town| town.id == entry.settlement_id)
            .ok_or_else(|| {
                format!(
                    "building refers to absent settlement {:?}",
                    entry.settlement_id
                )
            })?;
        let mut entity = world.spawn((
            BuildingOf(entry.settlement_id),
            PlayerPosition(entry.position),
            PlayerRotation(entry.rotation),
            BuildingDoorDemand::default(),
        ));
        if let Some(site) = &entry.construction {
            entity.insert(site.clone());
        } else {
            entity.insert(SettlementBuilding {
                kind: entry.kind,
                settlement: settlement.name.clone(),
                owner: None,
                quality: entry.quality,
                workers: Vec::new(),
            });
        }
        if let Some(id) = entry.id {
            entity.insert(id);
        }
        if let Some(house) = entry.house {
            entity.insert(house);
        }
        if let Some(level) = entry.market_level {
            entity.insert(level);
        }
        if let Some(inventory) = &entry.inventory {
            entity.insert(inventory.clone());
        }
        if let Some(upgrade) = entry.civic_upgrade {
            entity.insert(upgrade);
        }
        let art = entry
            .civic_upgrade
            .map(|upgrade| upgrade.target.building_type())
            .unwrap_or_else(|| {
                if entry.kind == shared::components::SettlementBuildingKind::Market {
                    entry.market_level.unwrap_or_default().building_type()
                } else {
                    entry.kind.art_with_house(entry.house.as_ref())
                }
            });
        if entry.construction.as_ref().is_none_or(|site| site.raising) && art.scene_path().is_some()
        {
            scenes.0.push(entity.id());
        }
    }
    for entry in &snapshot.roads {
        world.spawn((RoadOf(entry.settlement_id), entry.road.clone()));
    }
    for section in &snapshot.fortifications {
        world.spawn((section.clone(), PlayerPosition(section.midpoint())));
    }
    for entry in &snapshot.fields {
        let entity = world
            .spawn((
                entry.component.clone(),
                PlayerPosition(entry.position),
                PlayerRotation(entry.rotation),
            ))
            .id();
        scenes.0.push(entity);
    }
    for entry in &snapshot.pastures {
        world.spawn((
            entry.component.clone(),
            PlayerPosition(entry.position),
            PlayerRotation(entry.rotation),
        ));
    }
    for entry in &snapshot.piers {
        let entity = world
            .spawn((
                entry.component.clone(),
                PlayerPosition(entry.position),
                PlayerRotation(entry.rotation),
            ))
            .id();
        scenes.0.push(entity);
    }
    world.insert_resource(scenes);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::components::{
        BuildingId, CivicHallLevel, ConstructionSite, FarmField, HouseAppearance, HouseLevel,
        HouseLine, SettlementBuildingKind as Kind, SettlementDevelopment, SettlementId,
        SettlementTier, VillageRoad,
    };
    use shared::economy::{Good, GoodsInventory};
    use shared::settlement_snapshot::{
        SnapshotBuilding, SnapshotField, SnapshotRoad, SnapshotSettlement, TownMetrics,
        TOWN_SNAPSHOT_VERSION,
    };
    use shared::terrain::{ChunkCoord, TerrainDeltaChunk, TerrainDeltaData};

    fn snapshot(terrain: &WorldTerrain) -> TownSnapshot {
        let town = SettlementId(7);
        let origin = Vec3::new(1720.0, terrain.get_height(1720.0, 0.0), 0.0);
        let appearance = HouseAppearance {
            line: HouseLine::LongCabin,
            level: HouseLevel::UpperStorey,
        };
        let plot = |kind: Kind, offset: Vec3, id: Option<BuildingId>| {
            let position = origin + offset;
            SnapshotBuilding {
                id,
                settlement_id: town,
                kind,
                position,
                rotation: 0.4,
                house: (kind == Kind::House).then_some(appearance),
                market_level: None,
                construction: None,
                civic_upgrade: None,
                inventory: None,
                quality: 0.75,
                footprint: kind.placement_definition().footprint,
                footprint_center: kind
                    .placement_definition()
                    .world_footprint_center(position, 0.4),
                door: kind.entrance_position(position, 0.4),
            }
        };
        let house = plot(
            Kind::House,
            Vec3::new(30.0, 2.0, 20.0),
            Some(BuildingId(10)),
        );
        let farm = plot(
            Kind::Farmstead,
            Vec3::new(65.0, 2.0, 40.0),
            Some(BuildingId(11)),
        );
        let mut pending = plot(Kind::House, Vec3::new(44.0, 2.0, 20.0), None);
        pending.construction = Some(ConstructionSite {
            kind: Kind::House,
            settlement: "Snapshot".into(),
            raising: false,
            stand: pending.door,
            rotation: pending.rotation,
        });
        let mut inventory = GoodsInventory::new(100);
        inventory.add(Good::Wood, 3);
        pending.inventory = Some(inventory);
        let mut deltas = TerrainDeltaData::default();
        deltas.deltas[0] = 1.37;
        deltas.version = 9;
        TownSnapshot {
            version: TOWN_SNAPSHOT_VERSION,
            map_id: terrain.generator.active_map_id().into(),
            map_content_hash: terrain.generator.active_map_content_hash(),
            profile: "low".into(),
            seed: 23,
            elapsed_world_seconds: 120.0,
            fortifications: Vec::new(),
            districts: Vec::new(),
            day: 7,
            seconds_in_cycle: 12.0,
            settlements: vec![SnapshotSettlement {
                id: town,
                name: "Snapshot".into(),
                tier: SettlementTier::Village,
                residents: 4,
                treasury: 200,
                position: origin,
                rotation: 0.0,
                development: SettlementDevelopment::from_seed(23, 7),
                hall_level: CivicHallLevel::Village,
                footprint: CivicHallLevel::Village
                    .building_type()
                    .definition()
                    .footprint,
                footprint_center: origin.xz(),
                defenses: None,
                civic_square: None,
            }],
            fields: vec![SnapshotField {
                position: origin + Vec3::new(70.0, 2.0, 52.0),
                rotation: farm.rotation,
                component: FarmField {
                    settlement: "Snapshot".into(),
                    farmstead: farm.position,
                    plot_index: 1,
                    quality: 0.75,
                },
                footprint: Vec2::new(8.0, 11.0),
                footprint_center: origin.xz() + Vec2::new(70.0, 52.0),
            }],
            buildings: vec![house, farm, pending],
            roads: vec![SnapshotRoad {
                settlement_id: town,
                road: VillageRoad {
                    settlement: "Snapshot".into(),
                    builder: "Mara".into(),
                    points: vec![origin.xz(), origin.xz() + Vec2::new(30.0, 12.0)],
                    built_through: 2,
                    width: 2.0,
                    reserved_width: 4.0,
                    surface: default(),
                    class: default(),
                    stone_committed: 0,
                },
            }],
            pastures: Vec::new(),
            piers: Vec::new(),
            terrain_deltas: vec![TerrainDeltaChunk::from_delta_data(
                ChunkCoord::new(0, 0),
                &deltas,
            )],
            metrics: TownMetrics {
                residents: 4,
                housed: 4,
                completed_buildings: 2,
                pending_buildings: 1,
                completed_roads: 1,
                houses: 1,
                ..default()
            },
        }
    }

    #[test]
    fn imported_snapshot_retains_real_art_worksites_roads_fields_and_earthworks() {
        let terrain = WorldTerrain::default();
        let original = snapshot(&terrain);
        let state: TownSnapshot =
            serde_json::from_str(&serde_json::to_string(&original).unwrap()).unwrap();
        let mut world = World::new();
        world.insert_resource(terrain);
        world.spawn(WorldTime::new_default());
        import_town(&mut world, &state).unwrap();

        let houses: Vec<_> = world
            .query::<(
                &SettlementBuilding,
                &BuildingId,
                &HouseAppearance,
                &PlayerPosition,
            )>()
            .iter(&world)
            .collect();
        assert_eq!(houses.len(), 1);
        assert_eq!(houses[0].1, &BuildingId(10));
        assert_eq!(Some(*houses[0].2), state.buildings[0].house);
        assert_eq!(houses[0].3 .0, state.buildings[0].position);
        assert_eq!(world.query::<&SettlementBuilding>().iter(&world).count(), 2);
        let sites: Vec<_> = world
            .query::<(
                &ConstructionSite,
                &GoodsInventory,
                Option<&SettlementBuilding>,
                &HouseAppearance,
            )>()
            .iter(&world)
            .collect();
        assert_eq!(sites.len(), 1);
        assert_eq!(Some(sites[0].0), state.buildings[2].construction.as_ref());
        assert_eq!(sites[0].1.amount(Good::Wood), 3);
        assert!(
            sites[0].2.is_none(),
            "pending worksites must not become completed buildings"
        );
        assert_eq!(Some(*sites[0].3), state.buildings[2].house);

        let roads: Vec<_> = world
            .query::<(&VillageRoad, &RoadOf, Option<&BuildingOf>)>()
            .iter(&world)
            .collect();
        assert_eq!(roads.len(), 1);
        assert_eq!(roads[0].0, &state.roads[0].road);
        assert_eq!(roads[0].1 .0, SettlementId(7));
        assert!(
            roads[0].2.is_none(),
            "roads retain their road relationship, not a building relationship"
        );
        let fields: Vec<_> = world
            .query::<(&FarmField, &PlayerPosition, &PlayerRotation)>()
            .iter(&world)
            .collect();
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].0, &state.fields[0].component);
        assert_eq!(fields[0].1 .0, state.fields[0].position);
        assert_eq!(fields[0].2 .0, state.fields[0].rotation);
        let ground = &world.resource::<WorldTerrain>().delta_chunks()[&ChunkCoord::new(0, 0)];
        assert!((ground.deltas[0] - 1.37).abs() < 1e-6);
        assert_eq!(ground.version, 9);
        assert_eq!(world.query::<&WorldTime>().single(&world).unwrap().day, 7);
        assert_eq!(world.resource::<TownCaptureScenes>().0.len(), 4,
            "Hall, two completed buildings and crop field require scenes; unraised worksite does not");
    }

    #[test]
    fn mismatched_snapshot_map_is_rejected_before_mutating_the_world() {
        let terrain = WorldTerrain::default();
        let mut state = snapshot(&terrain);
        state.map_content_hash ^= 1;
        let version = terrain.modification_version();
        let mut world = World::new();
        world.insert_resource(terrain);
        // Bevy 0.19 stores resource cells as entities; rejection must leave the
        // existing entity count unchanged rather than require an empty world.
        let entities_before = world.query::<Entity>().iter(&world).count();
        assert!(import_town(&mut world, &state)
            .unwrap_err()
            .contains("differs"));
        assert_eq!(
            world.resource::<WorldTerrain>().modification_version(),
            version
        );
        assert_eq!(world.query::<&Settlement>().iter(&world).count(), 0);
        assert_eq!(
            world.query::<Entity>().iter(&world).count(),
            entities_before
        );
        assert!(!world.contains_resource::<TownCaptureScenes>());
    }

    #[test]
    fn submerged_snapshot_anchor_is_rejected_before_spawning_the_town() {
        let terrain = WorldTerrain::default();
        let bounds = terrain.generator.active_map_bounds();
        // Sample real, in-bounds terrain without changing the process-wide map
        // selection. Keep the search bounded even if the default map grows.
        let submerged = (0..64)
            .flat_map(|z| (0..64).map(move |x| (x, z)))
            .map(|(x, z)| {
                bounds.min_vec2()
                    + Vec2::new(
                        (x as f32 + 0.5) * bounds.width() / 64.0,
                        (z as f32 + 0.5) * bounds.depth() / 64.0,
                    )
            })
            .find(|point| {
                terrain
                    .water_surface_height(point.x, point.y)
                    .is_some_and(|water| terrain.generator.get_height(point.x, point.y) < water)
            })
            .expect("the default map must expose an underwater point for this regression");
        let mut state = snapshot(&terrain);
        state.terrain_deltas.clear();
        state.settlements[0].position = Vec3::new(
            submerged.x,
            terrain.generator.get_height(submerged.x, submerged.y),
            submerged.y,
        );
        state.settlements[0].footprint_center = submerged;
        let mut world = World::new();
        world.insert_resource(terrain);
        // The terrain resource already contributes resource-backed entities.
        let entities_before = world.query::<Entity>().iter(&world).count();

        let error = import_town(&mut world, &state).unwrap_err();
        assert!(error.contains("not on dry land"), "{error}");
        assert_eq!(world.query::<&Settlement>().iter(&world).count(), 0);
        assert_eq!(
            world.query::<Entity>().iter(&world).count(),
            entities_before
        );
        assert!(!world.contains_resource::<TownCaptureScenes>());
    }
}
