//! NPC-vs-static collision resolution.

use bevy::prelude::*;

use shared::building::{BuildingPosition, PlacedBuilding};
use shared::components::{Health, Npc, NpcPosition};
use shared::npc::{NPC_HEIGHT, NPC_RADIUS};
use shared::terrain::WorldTerrain;
use std::time::Instant;

use crate::ai::ragdoll::{CorpseBodyPoint, CorpseCollisionIndex};
use crate::collision::building_geometry::handle_capsule_vs_buildings;
use crate::collision::building_index::BuildingSpatialIndex;
use crate::collision::geometry::{handle_capsule_vs_corpse_spheres, handle_capsule_vs_static};
use crate::collision::library::{
    DerivedBuildingColliderLibrary, DerivedColliderLibrary, StaticColliders,
};

/// Resolve NPC collisions against static colliders (server-authoritative).
pub fn handle_npc_static_collisions(
    terrain: Res<WorldTerrain>,
    derived: Option<Res<DerivedColliderLibrary>>,
    building_derived: Option<Res<DerivedBuildingColliderLibrary>>,
    building_index: Option<Res<BuildingSpatialIndex>>,
    corpse_index: Res<CorpseCollisionIndex>,
    colliders: Res<StaticColliders>,
    buildings: Query<(Entity, &PlacedBuilding, &BuildingPosition)>,
    mut perf_monitor: Option<ResMut<crate::telemetry::perf::ServerPerfMonitor>>,
    mut npcs: Query<(&mut NpcPosition, &Health), With<Npc>>,
    mut static_candidates: Local<Vec<u32>>,
    mut building_candidates: Local<Vec<Entity>>,
    mut corpse_candidates: Local<Vec<CorpseBodyPoint>>,
) {
    const POS_WRITE_EPS_SQ: f32 = 1.0e-10;
    let phase_start = Instant::now();
    let Some(derived) = derived else { return };
    let building_lib = building_derived.as_deref();
    let building_index = building_index.as_deref();

    for (mut pos, health) in npcs.iter_mut() {
        if health.is_dead() {
            continue;
        }

        let start_pos = pos.0;
        let mut resolved_pos = start_pos;

        let _ = handle_capsule_vs_static(
            &derived,
            &colliders,
            &mut resolved_pos,
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
            &mut resolved_pos,
            None,
            NPC_RADIUS,
            NPC_HEIGHT,
            shared::player::STEP_UP_HEIGHT,
        );

        handle_capsule_vs_corpse_spheres(
            &corpse_index,
            &mut resolved_pos,
            None,
            NPC_RADIUS,
            NPC_HEIGHT,
            &mut corpse_candidates,
        );

        let ground_y = terrain.get_height(resolved_pos.x, resolved_pos.z);
        let min_y = ground_y + shared::physics::ground_clearance_center();
        if resolved_pos.y < min_y {
            resolved_pos.y = min_y;
        }

        if (resolved_pos - start_pos).length_squared() > POS_WRITE_EPS_SQ {
            pos.0 = resolved_pos;
        }
    }

    if let Some(perf) = perf_monitor.as_deref_mut() {
        perf.record_collision_ms(phase_start.elapsed().as_secs_f32() * 1000.0);
    }
}
