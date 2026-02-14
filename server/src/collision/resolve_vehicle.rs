//! Vehicle-vs-static collision resolution.

use bevy::prelude::*;

use shared::building::{BuildingPosition, PlacedBuilding};
use shared::vehicle::{vehicle_def, Vehicle, VehicleState, VehicleType};
use std::time::Instant;

use crate::collision::building_geometry::handle_capsule_vs_buildings;
use crate::collision::building_index::BuildingSpatialIndex;
use crate::collision::geometry::handle_vehicle_vs_static;
use crate::collision::library::{
    DerivedBuildingColliderLibrary, DerivedColliderLibrary, StaticColliders,
};

/// Resolve vehicle collisions against static colliders (server-authoritative).
pub fn handle_vehicle_static_collisions(
    derived: Option<Res<DerivedColliderLibrary>>,
    building_derived: Option<Res<DerivedBuildingColliderLibrary>>,
    building_index: Option<Res<BuildingSpatialIndex>>,
    colliders: Res<StaticColliders>,
    buildings: Query<(Entity, &PlacedBuilding, &BuildingPosition)>,
    mut perf_monitor: Option<ResMut<crate::telemetry::perf::ServerPerfMonitor>>,
    mut vehicles: Query<(&Vehicle, &mut VehicleState)>,
    mut static_candidates: Local<Vec<u32>>,
    mut building_candidates: Local<Vec<Entity>>,
) {
    let phase_start = Instant::now();
    let Some(derived) = derived else { return };
    let building_lib = building_derived.as_deref();
    let building_index = building_index.as_deref();

    // Hover bikes skip small obstacles (rocks) to preserve hover feel.
    const MIN_HOVER_COLLISION_RADIUS: f32 = 1.2;

    for (vehicle, mut state) in vehicles.iter_mut() {
        let def = vehicle_def(vehicle.vehicle_type);
        let radius = ((def.size.x * 0.5).powi(2) + (def.size.z * 0.5).powi(2)).sqrt();
        let height = def.size.y;
        let min_collision_radius = if vehicle.vehicle_type == VehicleType::Motorbike {
            MIN_HOVER_COLLISION_RADIUS
        } else {
            0.0
        };

        let mut pos = state.position;
        let mut vel = state.velocity;

        handle_vehicle_vs_static(
            &derived,
            &colliders,
            &mut pos,
            Some(&mut vel),
            radius,
            height,
            min_collision_radius,
            &mut static_candidates,
        );

        let _ = handle_capsule_vs_buildings(
            building_lib,
            building_index,
            &buildings,
            &mut building_candidates,
            &mut pos,
            Some(&mut vel),
            radius,
            height,
            0.0,
        );

        state.position = pos;
        state.velocity = vel;
    }

    if let Some(perf) = perf_monitor.as_deref_mut() {
        perf.record_collision_ms(phase_start.elapsed().as_secs_f32() * 1000.0);
    }
}
