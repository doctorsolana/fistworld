//! The authored tavern courtyard. Blender and simulation read the same layout.
use bevy::prelude::*;
use serde::Deserialize;
use std::sync::OnceLock;

pub const OUTDOOR_SEATS: u8 = 8;

#[derive(Deserialize)]
pub struct TavernLayout {
    pub table_centers: [[f32; 2]; 2],
    /// Width, depth and tabletop height, in metres.
    pub table_size: [f32; 3],
    pub bench_offset: f32,
    pub bench_height: f32,
    pub seat_offsets: [f32; 2],
}

pub fn layout() -> &'static TavernLayout {
    static LAYOUT: OnceLock<TavernLayout> = OnceLock::new();
    LAYOUT.get_or_init(|| {
        serde_json::from_str(include_str!("tavern_layout.json")).expect("authored tavern layout")
    })
}

#[derive(Clone, Copy, Debug)]
pub struct OutdoorSeat {
    pub position: Vec3,
    pub approach: Vec3,
    pub facing: f32,
}

/// Actor root/contact plane for the existing ground-seated clip, on a bench.
pub fn outdoor_seat(index: u8, origin: Vec3, yaw: f32) -> Option<OutdoorSeat> {
    if index >= OUTDOOR_SEATS {
        return None;
    }
    let layout = layout();
    let table = Vec2::from_array(layout.table_centers[index as usize / 4]);
    let side = if (index / 2) % 2 == 0 { -1.0 } else { 1.0 };
    let local = table
        + Vec2::new(
            layout.seat_offsets[index as usize % 2],
            side * layout.bench_offset,
        );
    let offset = crate::rotation::local_to_world_xz(local, yaw);
    let approach = crate::rotation::local_to_world_xz(local + Vec2::new(0.0, side * 0.65), yaw);
    Some(OutdoorSeat {
        position: origin + Vec3::new(offset.x, layout.bench_height, offset.y),
        approach: origin + Vec3::new(approach.x, 0.0, approach.y),
        facing: yaw
            + if side > 0.0 {
                0.0
            } else {
                std::f32::consts::PI
            },
    })
}

/// Only the tabletops obstruct the courtyard; doors, aisles and seats stay walkable.
pub fn table_obstacles(origin: Vec3, yaw: f32) -> [crate::spatial::ObstacleEntry; 2] {
    let layout = layout();
    layout.table_centers.map(|center| {
        let offset = crate::rotation::local_to_world_xz(Vec2::from_array(center), yaw);
        crate::spatial::ObstacleEntry {
            center: Vec2::new(origin.x, origin.z) + offset,
            half_extents: Vec2::new(layout.table_size[0], layout.table_size[1]) * 0.5
                + Vec2::splat(crate::physics::CHARACTER_NAV_RADIUS),
            rotation: yaw,
            obstacle_type: super::BuildingType::Tavern as u32,
        }
    })
}
