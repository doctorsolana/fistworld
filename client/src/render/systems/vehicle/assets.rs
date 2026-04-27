//! assets systems.

use super::*;

/// Setup shared scene handles for vehicles.
pub fn setup_vehicle_visual_assets(mut commands: Commands, asset_server: Res<AssetServer>) {
    commands.insert_resource(VehicleVisualAssets {
        hoverbike_scene: asset_server.load(HOVERBIKE_SCENE),
        steam_car_scene: asset_server.load(STEAM_CAR_SCENE),
    });
}
