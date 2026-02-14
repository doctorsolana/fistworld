//! assets systems.

use super::*;

pub(super) fn setup_item_model_assets(mut commands: Commands, asset_server: Res<AssetServer>) {
    let mut scenes = HashMap::new();
    scenes.insert(
        ItemType::RifleAmmo,
        asset_server.load("game_assets/items/bullet.glb#Scene0"),
    );
    scenes.insert(
        ItemType::SniperRounds,
        asset_server.load("game_assets/items/rifle_bullet.glb#Scene0"),
    );
    scenes.insert(
        ItemType::ShotgunShells,
        asset_server.load("game_assets/items/shotgun_bullet.glb#Scene0"),
    );
    scenes.insert(
        ItemType::Stone,
        asset_server.load("game_assets/items/stone.glb#Scene0"),
    );
    scenes.insert(
        ItemType::Wood,
        asset_server.load("game_assets/items/wood.glb#Scene0"),
    );
    scenes.insert(
        ItemType::GoldCoin,
        asset_server.load("game_assets/items/bullet.glb#Scene0"),
    );

    commands.insert_resource(ItemModelAssets { scenes });
}
