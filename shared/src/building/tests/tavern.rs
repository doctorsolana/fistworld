//! The server's courtyard layout must agree with the exported Blender anchors.
use super::*;
use bevy::prelude::*;

#[test]
fn tavern_scene_has_reachable_door_and_eight_matching_seat_anchors() {
    let kind = BuildingType::Tavern;
    assert_animated_building_contract(
        kind,
        crate::components::SettlementBuildingKind::Tavern.door_offset(),
        "TavernDoor",
        2,
        17_000,
    );
    let doc = glb_document(
        &Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../client/assets/game_assets/buildings/village/Tavern.glb"),
    );
    let nodes = doc["nodes"].as_array().unwrap();
    assert_eq!(doc["meshes"].as_array().unwrap().len(), 4);
    for index in 0..crate::building::tavern::OUTDOOR_SEATS {
        let node = nodes
            .iter()
            .find(|node| node["name"] == format!("Anchor_Seat.{index:02}"))
            .unwrap();
        let position = Vec3::new(
            node["translation"][0].as_f64().unwrap() as f32,
            node["translation"][1].as_f64().unwrap() as f32,
            node["translation"][2].as_f64().unwrap() as f32,
        );
        let expected = crate::building::tavern::outdoor_seat(index, Vec3::ZERO, 0.0).unwrap();
        assert!(
            position.distance(expected.position) < 0.0001,
            "seat {index}"
        );
    }
}

#[test]
fn tavern_seats_and_approaches_stay_outside_solids_at_every_rotation() {
    use crate::{
        components::SettlementBuildingKind,
        spatial::{ObstacleEntry, SpatialObstacleGrid},
    };
    let kind = SettlementBuildingKind::Tavern;
    let def = kind.art().definition();
    let origin = Vec3::new(14., 3., -31.);
    for step in 0..24 {
        let yaw = step as f32 * std::f32::consts::TAU / 24.;
        let mut grid = SpatialObstacleGrid::default();
        grid.insert(ObstacleEntry {
            center: def.world_footprint_center(origin, yaw),
            half_extents: def.footprint * 0.5 + Vec2::splat(crate::physics::CHARACTER_NAV_RADIUS),
            rotation: yaw,
            obstacle_type: 0,
        });
        for table in crate::building::tavern::table_obstacles(origin, yaw) {
            grid.insert(table);
        }
        assert!(!grid.point_blocked(kind.entrance_position(origin, yaw).xz()));
        for index in 0..8 {
            let seat = crate::building::tavern::outdoor_seat(index, origin, yaw).unwrap();
            assert!(
                !grid.point_blocked(seat.position.xz()),
                "seat {index}, yaw {yaw}"
            );
            assert!(
                !grid.point_blocked(seat.approach.xz()),
                "approach {index}, yaw {yaw}"
            );
            let toward_table = (seat.position - seat.approach).xz().normalize();
            let forward = Quat::from_rotation_y(seat.facing) * Vec3::NEG_Z;
            assert!(toward_table.dot(forward.xz()) > 0.99);
        }
    }
}
