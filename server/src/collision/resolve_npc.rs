//! NPC-vs-static collision resolution.

use bevy::prelude::*;

use shared::building::{BuildingPosition, PlacedBuilding};
use shared::components::{Health, Npc, NpcPosition};
use shared::npc::{NPC_HEIGHT, NPC_RADIUS};
use shared::terrain::WorldTerrain;
use std::time::Instant;

use crate::collision::building_geometry::handle_capsule_vs_buildings;
use crate::collision::building_index::BuildingSpatialIndex;
use crate::collision::geometry::handle_capsule_vs_static;
use crate::collision::library::{
    DerivedBuildingColliderLibrary, DerivedColliderLibrary, StaticColliders,
};

/// Resolve NPC collisions against static colliders (server-authoritative).
pub fn handle_npc_static_collisions(
    terrain: Res<WorldTerrain>,
    derived: Option<Res<DerivedColliderLibrary>>,
    building_derived: Option<Res<DerivedBuildingColliderLibrary>>,
    building_index: Option<Res<BuildingSpatialIndex>>,
    colliders: Res<StaticColliders>,
    buildings: Query<(Entity, &PlacedBuilding, &BuildingPosition)>,
    mut perf_monitor: Option<ResMut<crate::telemetry::perf::ServerPerfMonitor>>,
    mut npcs: Query<(&mut NpcPosition, &Health), With<Npc>>,
    mut static_candidates: Local<Vec<u32>>,
    mut building_candidates: Local<Vec<Entity>>,
) {
    let phase_start = Instant::now();
    let Some(derived) = derived else { return };
    let building_lib = building_derived.as_deref();
    let building_index = building_index.as_deref();

    for (mut pos, health) in npcs.iter_mut() {
        if health.is_dead() {
            continue;
        }

        let _ = handle_capsule_vs_static(
            &derived,
            &colliders,
            &mut pos.0,
            None,
            NPC_RADIUS,
            NPC_HEIGHT,
            shared::player::STEP_UP_HEIGHT,
            &mut static_candidates,
        );

        let _ = handle_capsule_vs_buildings(
            building_lib,
            building_index,
            &buildings,
            &mut building_candidates,
            &mut pos.0,
            None,
            NPC_RADIUS,
            NPC_HEIGHT,
            shared::player::STEP_UP_HEIGHT,
        );

        let ground_y = terrain.get_height(pos.0.x, pos.0.z);
        let min_y = ground_y + shared::physics::ground_clearance_center();
        if pos.0.y < min_y {
            pos.0.y = min_y;
        }
    }

    if let Some(perf) = perf_monitor.as_deref_mut() {
        perf.record_collision_ms(phase_start.elapsed().as_secs_f32() * 1000.0);
    }
}
