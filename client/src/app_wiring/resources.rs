//! resources systems.

use super::*;

pub fn setup_resources(app: &mut App) {
    // Debug gizmo toggle (F4)
    app.init_resource::<DebugGizmoMode>();

    // Top-down commander camera
    app.init_resource::<camera_rts::CursorTerrainHit>();
    app.init_resource::<camera_rts::CursorRay>();
    app.init_resource::<crate::hero::control::NpcSpawnArm>();
    app.init_resource::<terrain::map_view::MapViewBlend>();
    app.init_resource::<perf_overlay::PerfOverlayEnabled>();
    app.init_resource::<perf_overlay::PerfDropMonitor>();
    app.init_resource::<perf_overlay::ClientPerfConfig>();
    app.init_resource::<perf_overlay::ClientPerfSnapshot>();
    app.init_resource::<audio::RemoteAudioEmitterIndex>();

    // Graphics settings (toggleable from pause menu)
    app.insert_resource(game_systems::GraphicsSettings::load_or_default());

    // Input settings (controls, sensitivity - adjustable from pause menu)
    app.init_resource::<game_systems::InputSettings>();

    // Input resource
    app.init_resource::<input::InputState>();
}
