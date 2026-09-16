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
    yards: Query<(
        &shared::components::HouseholdYard,
        &shared::components::PlayerPosition,
        &shared::components::PlayerRotation,
    )>,
    changed_yards: Query<
        (),
        (
            With<shared::components::HouseholdYard>,
            Or<(
                Changed<shared::components::HouseholdYard>,
                Changed<shared::components::PlayerPosition>,
                Changed<shared::components::PlayerRotation>,
            )>,
        ),
    >,
    mut removed_yards: RemovedComponents<shared::components::HouseholdYard>,
    mut fields: crate::world::farm_boundaries::FarmBoundarySource,
    ports: Query<&shared::components::SettlementPort>,
    changed_ports: Query<(), Changed<shared::components::SettlementPort>>,
    mut removed_ports: RemovedComponents<shared::components::SettlementPort>,
) {
    let fields_changed = fields.refresh();
    let walls_removed = removed_walls.read().count() > 0;
    let yards_removed = removed_yards.read().count() > 0;
    let ports_removed = removed_ports.read().count() > 0;
    if state.last_building_version == Some(building_index.version)
        && changed_walls.is_empty()
        && !walls_removed
        && changed_yards.is_empty()
        && !yards_removed
        && !fields_changed
        && changed_ports.is_empty()
        && !ports_removed
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
        if building.building_type == shared::building::BuildingType::Tavern {
            for obstacle in
                shared::building::tavern::table_obstacles(building.position, building.rotation)
            {
                grid.insert(obstacle);
            }
        }
    }

    for wall in &walls {
        for obstacle in wall.ground_obstacles() {
            grid.insert(obstacle);
        }
    }
    for (yard, position, rotation) in &yards {
        for obstacle in yard.ground_obstacles(position.0, rotation.0) {
            grid.insert(obstacle);
        }
    }

    for obstacle in fields.obstacles() {
        grid.insert(obstacle);
    }
    for port in &ports {
        if port.built {
            for obstacle in port.geometry.ground_obstacles() {
                grid.insert(obstacle);
            }
        }
    }

    if !buildings.is_empty() {
        trace!("Rebuilt spatial grid with {} obstacles", buildings.len());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collision::building_index::{BuildingSpatialIndex, sync_building_spatial_index};
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
    fn authored_port_solids_block_pawns_keep_walking_lane_and_follow_completion_removal() {
        use shared::components::{
            PORT_OBSTACLE_TYPE, PortGeometry, SettlementId, SettlementPort, ShipKind,
        };
        let mut app = navigation_app();
        let yaw = 0.73_f32;
        let sea = Vec3::new(-yaw.sin(), 0., -yaw.cos());
        let shore = Vec3::new(80., 1., -50.);
        let geometry = PortGeometry {
            shore,
            pier_end: shore + sea * 34.,
            berth: shore + sea * 40. - Vec3::Y,
            departure: shore + sea * 40. + Quat::from_rotation_y(yaw) * Vec3::X * 15. - Vec3::Y,
            yaw: yaw - std::f32::consts::FRAC_PI_2,
            maximum_ship: ShipKind::Cog,
        };
        let entity = app
            .world_mut()
            .spawn(SettlementPort {
                settlement: SettlementId(1),
                geometry,
                built: false,
            })
            .id();
        let office_start = geometry.project_asset_point(Vec3::new(-9., 1., 2.)).xz();
        let office_end = geometry.project_asset_point(Vec3::new(-1., 1., 2.)).xz();
        app.update();
        assert!(
            !app.world()
                .resource::<SpatialObstacleGrid>()
                .segment_blocked_by_type(office_start, office_end, PORT_OBSTACLE_TYPE)
        );
        app.world_mut()
            .get_mut::<SettlementPort>(entity)
            .unwrap()
            .built = true;
        app.update();
        let grid = app.world().resource::<SpatialObstacleGrid>();
        assert!(grid.segment_blocked_by_type(office_start, office_end, PORT_OBSTACLE_TYPE));
        for (first, last) in [
            (Vec3::new(3., 1., 2.7), Vec3::new(6., 1., 2.7)),
            (Vec3::new(2., 1., -0.25), Vec3::new(3.3, 1., -0.25)),
        ] {
            assert!(grid.segment_blocked_by_type(
                geometry.project_asset_point(first).xz(),
                geometry.project_asset_point(last).xz(),
                PORT_OBSTACLE_TYPE
            ));
        }
        // The complete 2.4 m central aisle remains available to pawn centres
        // after the ordinary capsule radius is added to each actual solid.
        for x in [-1.2, 0., 1.2] {
            assert!(!grid.segment_blocked_by_type(
                geometry.project_asset_point(Vec3::new(x, 1., 5.4)).xz(),
                geometry.project_asset_point(Vec3::new(x, 1., -20.)).xz(),
                PORT_OBSTACLE_TYPE,
            ));
        }
        let version = grid.version;
        for _ in 0..4 {
            app.update();
        }
        assert_eq!(
            app.world().resource::<SpatialObstacleGrid>().version,
            version
        );
        let shift = Vec3::X * 60.;
        {
            let mut port = app.world_mut().get_mut::<SettlementPort>(entity).unwrap();
            port.geometry.shore += shift;
            port.geometry.pier_end += shift;
            port.geometry.berth += shift;
            port.geometry.departure += shift;
        }
        app.update();
        let grid = app.world().resource::<SpatialObstacleGrid>();
        assert!(!grid.segment_blocked_by_type(office_start, office_end, PORT_OBSTACLE_TYPE));
        assert!(grid.segment_blocked_by_type(
            office_start + shift.xz(),
            office_end + shift.xz(),
            PORT_OBSTACLE_TYPE
        ));
        app.world_mut()
            .entity_mut(entity)
            .remove::<SettlementPort>();
        app.update();
        assert!(
            !app.world()
                .resource::<SpatialObstacleGrid>()
                .segment_blocked_by_type(
                    office_start + shift.xz(),
                    office_end + shift.xz(),
                    PORT_OBSTACLE_TYPE
                )
        );
    }

    #[test]
    fn farm_fences_block_the_boundary_keep_gate_open_and_ignore_quality_churn() {
        use shared::components::{
            FARM_FENCE_OBSTACLE_TYPE, FarmField, FarmFieldShape, PlayerPosition, PlayerRotation,
        };
        let mut app = navigation_app();
        let origin = Vec3::new(-4.45, 0., 9.);
        let shape = FarmFieldShape::legacy_rectangle();
        let entity = app
            .world_mut()
            .spawn((
                FarmField {
                    settlement: "Farm".into(),
                    farmstead: Vec3::ZERO,
                    plot_index: 0,
                    quality: 1.,
                    shape: Some(shape),
                    layout_version: 2,
                },
                PlayerPosition(origin),
                PlayerRotation(0.),
            ))
            .id();
        app.update();
        let grid = app.world().resource::<SpatialObstacleGrid>();
        assert!(grid.segment_blocked_by_type(
            Vec2::new(-10., 9.),
            Vec2::new(-7., 9.),
            FARM_FENCE_OBSTACLE_TYPE
        ));
        assert!(!grid.segment_blocked_by_type(
            Vec2::new(-2., 2.),
            Vec2::new(-2., 6.),
            FARM_FENCE_OBSTACLE_TYPE
        ));
        let revision = grid.version;
        app.world_mut()
            .get_mut::<FarmField>(entity)
            .unwrap()
            .quality = 0.3;
        app.update();
        assert_eq!(
            app.world().resource::<SpatialObstacleGrid>().version,
            revision
        );
        app.world_mut().get_mut::<PlayerPosition>(entity).unwrap().0 += Vec3::X * 50.;
        app.update();
        assert!(
            !app.world()
                .resource::<SpatialObstacleGrid>()
                .segment_blocked_by_type(
                    Vec2::new(-10., 9.),
                    Vec2::new(-7., 9.),
                    FARM_FENCE_OBSTACLE_TYPE
                )
        );
        app.world_mut().entity_mut(entity).remove::<FarmField>();
        app.update();
        assert!(
            !app.world()
                .resource::<SpatialObstacleGrid>()
                .segment_blocked_by_type(
                    Vec2::new(40., 9.),
                    Vec2::new(43., 9.),
                    FARM_FENCE_OBSTACLE_TYPE
                )
        );
    }

    #[test]
    fn yard_fences_track_transform_and_removal_while_the_entrance_stays_open() {
        use shared::components::{
            HouseholdYard, PlayerPosition, PlayerRotation, YARD_OBSTACLE_TYPE, YardSide, YardUse,
        };
        let mut app = navigation_app();
        let entity = app
            .world_mut()
            .spawn((
                HouseholdYard {
                    entry: None,
                    approach: None,
                    house: None,
                    boundary: Vec::new(),
                    minimum: Vec2::new(5., -2.),
                    maximum: Vec2::new(7., 3.),
                    side: YardSide::Right,
                    use_kind: YardUse::Laundry,
                    seed: 1,
                },
                PlayerPosition(Vec3::ZERO),
                PlayerRotation(0.0),
            ))
            .id();
        app.update();
        let grid = app.world().resource::<SpatialObstacleGrid>();
        assert!(grid.segment_blocked_by_type(
            Vec2::new(6., 0.),
            Vec2::new(8., 0.),
            YARD_OBSTACLE_TYPE
        ));
        assert!(!grid.segment_blocked(Vec2::new(4., 0.), Vec2::new(6., 0.)));
        let version = grid.version;
        app.update();
        assert_eq!(
            app.world().resource::<SpatialObstacleGrid>().version,
            version
        );

        app.world_mut().entity_mut(entity).insert((
            PlayerPosition(Vec3::new(100., 0., 0.)),
            PlayerRotation(std::f32::consts::FRAC_PI_2),
        ));
        app.update();
        let grid = app.world().resource::<SpatialObstacleGrid>();
        assert!(!grid.point_blocked(Vec2::new(7., 0.)));
        assert!(grid.point_blocked(Vec2::new(100., -7.)));
        assert!(!grid.segment_blocked(Vec2::new(100., -4.), Vec2::new(100., -6.)));

        app.world_mut().entity_mut(entity).remove::<HouseholdYard>();
        app.update();
        assert!(app.world().resource::<SpatialObstacleGrid>().is_empty());
        let version = app.world().resource::<SpatialObstacleGrid>().version;
        app.update();
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
        assert!(
            !app.world()
                .resource::<SpatialObstacleGrid>()
                .point_blocked(Vec2::new(-7.0, 0.0))
        );
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
        assert!(
            !app.world()
                .resource::<SpatialObstacleGrid>()
                .point_blocked(Vec2::new(-7.0, 0.0))
        );
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
        assert!(
            app.world()
                .resource::<SpatialObstacleGrid>()
                .point_blocked(Vec2::ZERO)
        );

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
                (BuildingType::Tavern, SettlementBuildingKind::Tavern),
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
