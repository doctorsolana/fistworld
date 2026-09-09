//! Navigation obstacle grid: keeps `SpatialObstacleGrid` in sync with authored buildings.
//!
//! It contains no unit types: each authored building footprint receives a small
//! embodied-agent clearance and enters the shared spatial grid used by route
//! planning and movement-time anti-tunnelling checks. Landscaping flatten radii
//! are deliberately unrelated; they are much too large for navigation and cover
//! the authored doors.

use bevy::prelude::*;
use shared::components::FortificationSegment;
use shared::physics::CHARACTER_NAV_RADIUS;
use shared::spatial::{ObstacleEntry, SpatialObstacleGrid};

use crate::collision::building_index::BuildingSpatialIndex;

/// Horizontal body clearance used around authored props. Keep this shared by
/// route surveying, interaction-point selection and movement-time collision;
/// three almost-identical constants here previously let a planned route end at
/// a point the embodied villager could never actually occupy.
pub const VILLAGER_PROP_RADIUS: f32 = 0.35;

/// Spatial sampling interval for both route certification and movement-time
/// anti-tunnelling. A coarser planner used to miss thin rotated-building
/// corners that embodied movement then rejected, causing an endless replan of
/// the same nominally valid route.
pub const NAVIGATION_SAMPLE_STEP: f32 = 0.2;

/// Tracks the last known building index version to detect authored changes.
#[derive(Resource, Default)]
pub struct ObstacleGridState {
    // An empty grid is a valid completed rebuild. Track initialization
    // separately from obstacle count so empty worlds retain cached routes.
    last_building_version: Option<u64>,
}

/// Sync the `SpatialObstacleGrid` with current buildings.
/// Rebuilds only when the authored building index changes.
pub fn sync_obstacle_grid(
    mut grid: ResMut<SpatialObstacleGrid>,
    mut state: ResMut<ObstacleGridState>,
    building_index: Res<BuildingSpatialIndex>,
    walls: Query<&FortificationSegment>,
    changed_walls: Query<(), Changed<FortificationSegment>>,
    mut removed_walls: RemovedComponents<FortificationSegment>,
) {
    let walls_removed = removed_walls.read().count() > 0;
    if state.last_building_version == Some(building_index.version)
        && changed_walls.is_empty()
        && !walls_removed
    {
        return;
    }

    state.last_building_version = Some(building_index.version);
    grid.clear();

    let buildings = building_index.snapshot();
    for building in buildings {
        if !building.building_type.blocks_ground_navigation() {
            continue;
        }
        let def = building.building_type.definition();
        let half_extents = Vec2::new(
            def.footprint.x / 2.0 + CHARACTER_NAV_RADIUS,
            def.footprint.y / 2.0 + CHARACTER_NAV_RADIUS,
        );

        grid.insert(ObstacleEntry {
            center: def.world_footprint_center(building.position, building.rotation),
            half_extents,
            rotation: building.rotation,
            obstacle_type: building.building_type as u32,
        });
    }

    for wall in &walls {
        for obstacle in wall.ground_obstacles() {
            grid.insert(obstacle);
        }
    }

    if !buildings.is_empty() {
        trace!("Rebuilt spatial grid with {} obstacles", buildings.len());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collision::building_index::{sync_building_spatial_index, BuildingSpatialIndex};
    use shared::building::BuildingType;
    use shared::building::{BuildingPosition, PlacedBuilding};
    use shared::components::SettlementBuildingKind;

    fn navigation_app() -> App {
        let mut app = App::new();
        app.init_resource::<BuildingSpatialIndex>();
        app.init_resource::<SpatialObstacleGrid>();
        app.init_resource::<ObstacleGridState>();
        app.add_systems(
            Update,
            (sync_building_spatial_index, sync_obstacle_grid).chain(),
        );
        app
    }

    #[test]
    fn empty_navigation_world_keeps_its_obstacle_version() {
        let mut app = navigation_app();
        app.update();
        let version = app.world().resource::<SpatialObstacleGrid>().version;
        for _ in 0..8 {
            app.update();
        }
        assert_eq!(
            app.world().resource::<SpatialObstacleGrid>().version,
            version
        );
    }

    #[test]
    fn defenses_block_only_completed_solid_sections_and_invalidate_on_removal() {
        use shared::components::{FortificationKind, FortificationMaterial, SettlementId};
        let mut app = navigation_app();
        let wall = app
            .world_mut()
            .spawn(FortificationSegment {
                settlement_id: SettlementId(1),
                circuit: 0,
                start: Vec3::new(-10.0, 0.0, 0.0),
                end: Vec3::new(-4.0, 0.0, 0.0),
                kind: FortificationKind::Wall,
                material: FortificationMaterial::Palisade,
                complete: false,
            })
            .id();
        app.update();
        assert!(!app
            .world()
            .resource::<SpatialObstacleGrid>()
            .point_blocked(Vec2::new(-7.0, 0.0)));
        app.world_mut()
            .get_mut::<FortificationSegment>(wall)
            .unwrap()
            .complete = true;
        app.update();
        let grid = app.world().resource::<SpatialObstacleGrid>();
        assert!(grid.segment_blocked(Vec2::new(-7.0, -3.0), Vec2::new(-7.0, 3.0)));
        assert!(!grid.segment_blocked(Vec2::new(0.0, -3.0), Vec2::new(0.0, 3.0)));
        let version = grid.version;
        app.update();
        assert_eq!(
            app.world().resource::<SpatialObstacleGrid>().version,
            version
        );
        app.world_mut().despawn(wall);
        app.update();
        assert!(!app
            .world()
            .resource::<SpatialObstacleGrid>()
            .point_blocked(Vec2::new(-7.0, 0.0)));
    }

    #[test]
    fn navigation_grid_tracks_movement_and_removal_without_empty_rebuilds() {
        let mut app = navigation_app();
        let building = app
            .world_mut()
            .spawn((
                PlacedBuilding {
                    building_type: BuildingType::MootHall,
                    rotation: 0.0,
                },
                BuildingPosition(Vec3::ZERO),
            ))
            .id();
        app.update();
        assert!(app
            .world()
            .resource::<SpatialObstacleGrid>()
            .point_blocked(Vec2::ZERO));

        app.world_mut()
            .get_mut::<BuildingPosition>(building)
            .unwrap()
            .0
            .x = 100.0;
        app.update();
        let grid = app.world().resource::<SpatialObstacleGrid>();
        assert!(!grid.point_blocked(Vec2::ZERO));
        assert!(grid.point_blocked(Vec2::new(100.0, 0.0)));

        app.world_mut().despawn(building);
        app.update();
        assert!(app.world().resource::<SpatialObstacleGrid>().is_empty());
        let version = app.world().resource::<SpatialObstacleGrid>().version;
        for _ in 0..8 {
            app.update();
        }
        assert_eq!(
            app.world().resource::<SpatialObstacleGrid>().version,
            version
        );
    }

    #[test]
    fn every_authored_door_sits_outside_its_navigation_blocker() {
        for step in 0..16 {
            let rotation = std::f32::consts::TAU * step as f32 / 16.0;
            for (building_type, kind) in [
                (BuildingType::LogCabin, SettlementBuildingKind::House),
                (
                    BuildingType::LumberjackHut,
                    SettlementBuildingKind::LumberjackHut,
                ),
                (BuildingType::Farmstead, SettlementBuildingKind::Farmstead),
                (BuildingType::MootHall, SettlementBuildingKind::Hall),
                (BuildingType::VillageHall, SettlementBuildingKind::Hall),
                (BuildingType::TownHall, SettlementBuildingKind::Hall),
                (BuildingType::LongCabin, SettlementBuildingKind::House),
                (BuildingType::CabinL2, SettlementBuildingKind::House),
                (BuildingType::LongCabinL2, SettlementBuildingKind::House),
                (
                    BuildingType::FishermansHut,
                    SettlementBuildingKind::FishermansHut,
                ),
                (BuildingType::Windmill, SettlementBuildingKind::Windmill),
                (BuildingType::Bakery, SettlementBuildingKind::Bakery),
                (
                    BuildingType::LivestockFarm,
                    SettlementBuildingKind::LivestockFarm,
                ),
                (
                    BuildingType::PlaceholderTavern,
                    SettlementBuildingKind::Tavern,
                ),
                (BuildingType::Church, SettlementBuildingKind::Church),
                (
                    BuildingType::StorageHall,
                    SettlementBuildingKind::StorageHall,
                ),
                (
                    BuildingType::StoneQuarry,
                    SettlementBuildingKind::StoneQuarry,
                ),
            ] {
                let mut grid = SpatialObstacleGrid::default();
                let definition = building_type.definition();
                grid.insert(ObstacleEntry {
                    center: definition.world_footprint_center(Vec3::ZERO, rotation),
                    half_extents: definition.footprint * 0.5 + Vec2::splat(CHARACTER_NAV_RADIUS),
                    rotation,
                    obstacle_type: building_type as u32,
                });
                let door = kind.entrance_position(Vec3::ZERO, rotation);
                assert!(grid.point_blocked(Vec2::ZERO));
                assert!(
                    !grid.point_blocked(Vec2::new(door.x, door.z)),
                    "{} door is inside its nav blocker at rotation {rotation}",
                    definition.display_name
                );
            }
        }
    }

    #[test]
    fn open_air_market_is_not_a_navigation_blocker() {
        let mut app = navigation_app();
        app.world_mut().spawn((
            PlacedBuilding {
                building_type: BuildingType::Market,
                rotation: 0.0,
            },
            BuildingPosition(Vec3::ZERO),
        ));

        app.update();

        assert!(app.world().resource::<SpatialObstacleGrid>().is_empty());
        let version = app.world().resource::<SpatialObstacleGrid>().version;
        app.update();
        assert_eq!(
            app.world().resource::<SpatialObstacleGrid>().version,
            version
        );
    }
}
