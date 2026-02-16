//! Application resource initialization.

use bevy::prelude::*;

use shared::spatial::SpatialObstacleGrid;
use shared::terrain::WorldTerrain;

use crate::ai;
use crate::collision;
use crate::combat;
use crate::inventory;
use crate::net;
use crate::persistence;
use crate::physics;
use crate::player;
use crate::telemetry;

pub(crate) fn setup_resources(app: &mut App) {
    let profile_storage_dir = std::path::PathBuf::from("server_data/players");

    app.init_resource::<WorldTerrain>();
    app.init_resource::<net::input::ClientInputs>();
    app.init_resource::<net::input::ClientInputIngressStats>();
    app.init_resource::<player::index::PlayerEntityIndex>();
    app.init_resource::<player::spatial::PlayerSpatialIndex>();
    app.init_resource::<inventory::chest::OpenChests>();
    app.init_resource::<SpatialObstacleGrid>();
    app.init_resource::<ai::obstacles::ObstacleGridState>();
    app.init_resource::<ai::ragdoll::CorpseBudget>();
    app.init_resource::<ai::ragdoll::RagdollPoseStream>();
    app.init_resource::<ai::ragdoll::CorpseCollisionIndex>();
    app.init_resource::<ai::ragdoll::RagdollTelemetry>();
    app.init_resource::<collision::building_index::BuildingSpatialIndex>();
    app.init_resource::<collision::streaming::ColliderStreamingState>();
    app.init_resource::<physics::terrain_colliders::TerrainColliderSettings>();
    app.init_resource::<physics::terrain_colliders::TerrainColliderRegistry>();
    app.init_resource::<physics::static_world_colliders::StaticWorldColliderRegistry>();
    app.init_resource::<combat::target_index::HittableSpatialIndex>();
    app.insert_resource(persistence::profiles::PlayerProfiles::new(
        profile_storage_dir.clone(),
    ));
    app.insert_resource(player::roster_cache::PlayerRosterCache::from_storage_dir(
        &profile_storage_dir,
    ));
    app.insert_resource(persistence::io_queue::ProfileIoQueue::new(
        profile_storage_dir,
    ));
    app.init_resource::<telemetry::perf::ServerPerfMonitor>();
    app.init_resource::<telemetry::network::ServerNetDebugWindow>();
}
