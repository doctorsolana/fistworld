//! resources systems.

use super::*;

pub fn setup_resources(app: &mut App) {
    // Weapon debug mode resource
    app.init_resource::<WeaponDebugMode>();
    app.init_resource::<weapons::PerfOverlayEnabled>();
    app.init_resource::<weapons::PerfDropMonitor>();
    app.init_resource::<weapons::ShootingState>();
    app.init_resource::<weapons::ShootInputSuppress>();
    app.init_resource::<weapons::ReloadState>();
    app.init_resource::<weapons::DebugBulletTrails>();
    app.init_resource::<weapons::ClientPerfConfig>();
    app.init_resource::<weapons::ClientPerfSnapshot>();
    app.init_resource::<weapons::PlayerOwnerIndex>();
    app.init_resource::<weapons::RemoteMuzzleIndex>();
    app.init_resource::<weapons::WeaponWarmupQueue>();
    app.init_resource::<weapon_view::CurrentWeaponView>();
    app.init_resource::<weapon_view::CurrentThirdPersonWeapon>();
    app.init_resource::<weapon_view::RemoteWeaponIndex>();
    app.init_resource::<audio::RemoteAudioEmitterIndex>();

    // Graphics settings (toggleable from pause menu)
    app.init_resource::<game_systems::GraphicsSettings>();

    // Input settings (controls, sensitivity - adjustable from pause menu)
    app.init_resource::<game_systems::InputSettings>();

    // Input resource
    app.init_resource::<input::InputState>();
    app.init_resource::<game_systems::LastCameraMode>();
}
