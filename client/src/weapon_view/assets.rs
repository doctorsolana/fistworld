use bevy::prelude::*;
use shared::weapons::WeaponType;
use std::collections::HashMap;

/// Loaded weapon model scenes.
#[derive(Resource, Clone)]
pub struct WeaponModelAssets {
    pub scenes: HashMap<WeaponType, Handle<Scene>>,
}

/// Load weapon model assets at startup.
pub fn setup_weapon_model_assets(mut commands: Commands, asset_server: Res<AssetServer>) {
    let mut scenes = HashMap::new();

    scenes.insert(
        WeaponType::AssaultRifle,
        asset_server.load("game_assets/weapons/automatic_rifle.glb#Scene0"),
    );
    scenes.insert(
        WeaponType::Shotgun,
        asset_server.load("game_assets/weapons/shotgun.glb#Scene0"),
    );
    scenes.insert(
        WeaponType::Sniper,
        asset_server.load("game_assets/weapons/sniper.glb#Scene0"),
    );
    scenes.insert(
        WeaponType::Pistol,
        asset_server.load("game_assets/weapons/revolver.glb#Scene0"),
    );

    commands.insert_resource(WeaponModelAssets { scenes });
}
