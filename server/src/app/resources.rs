//! Application resource initialization.

use bevy::prelude::*;

use shared::city::AuthoredCityLayout;
use shared::spatial::SpatialObstacleGrid;
use shared::terrain::WorldTerrain;

use crate::collision;
use crate::net;
use crate::persistence;
use crate::telemetry;

pub(crate) fn setup_resources(app: &mut App) {
    app.init_resource::<WorldTerrain>();
    app.init_resource::<AuthoredCityLayout>();
    app.init_resource::<net::input::ClientInputs>();
    app.init_resource::<crate::player::hero::HeroIndex>();
    app.init_resource::<crate::player::boat::VesselNavigationQueue>();
    app.init_resource::<crate::player::permits::PermitIdAllocator>();
    app.init_resource::<crate::world::dev::VillagerSeed>();
    app.init_resource::<net::input::ClientInputIngressStats>();
    app.init_resource::<SpatialObstacleGrid>();
    app.init_resource::<crate::world::navgrid::ObstacleGridState>();
    app.init_resource::<crate::world::regions::RegionRegistry>();
    app.init_resource::<crate::world::regions::ClientInterest>();
    app.init_resource::<crate::world::regions::StrategicClock>();
    app.init_resource::<crate::world::regions::StrategicStep>();
    app.init_resource::<crate::world::identity::WorldIdAllocator>();
    app.init_resource::<crate::world::identity::WorldIdentityIndex>();
    app.init_resource::<crate::world::settlement_directory::SettlementDirectory>();
    app.init_resource::<crate::world::village::VillageClock>();
    app.init_resource::<crate::world::village::SettlementEconomyRuntime>();
    app.init_resource::<crate::world::village::history::SettlementHistoryRuntime>();
    app.init_resource::<crate::world::village::ambient::AmbientClock>();
    app.init_resource::<crate::world::village::ambient::AmbientSpotCache>();
    app.init_resource::<crate::world::village::PublishedTerrainDeltas>();
    app.init_resource::<crate::world::village::strategic::StrategicProductionProgress>();
    app.init_resource::<crate::world::village_roads::VillageRoadGraph>();
    app.init_resource::<crate::world::pathfinding::PathfindingBudgetSettings>();
    app.init_resource::<crate::world::dev::DevMode>();
    app.init_resource::<collision::building_index::BuildingSpatialIndex>();
    app.init_resource::<collision::streaming::ColliderStreamingState>();
    // World and account state share one lifetime: reconnecting to this running
    // process restores the live hero, retinue and view, while restarting the
    // server deliberately starts clean and ignores old profile files.
    app.insert_resource(persistence::profiles::PlayerProfiles::new_session());
    app.init_resource::<telemetry::perf::ServerPerfMonitor>();
    app.init_resource::<telemetry::network::ServerNetDebugWindow>();
}
