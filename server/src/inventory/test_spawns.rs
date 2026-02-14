//! Startup/debug inventory spawn systems.

use bevy::prelude::*;
use lightyear::prelude::{NetworkTarget, Replicate, ReplicationMode};
use shared::items::{ItemStack, ItemType};
use shared::terrain::WorldTerrain;
use shared::weapons::WeaponType;

use super::chest::spawn_chest;
use super::ground_items::spawn_ground_item;

/// Marker resource to track if test items have been spawned.
#[derive(Resource)]
pub struct TestItemsSpawned;

/// Spawn test items near spawn point.
pub fn spawn_test_items(
    mut commands: Commands,
    terrain: Res<WorldTerrain>,
    spawned: Option<Res<TestItemsSpawned>>,
) {
    if spawned.is_some() {
        return;
    }

    commands.insert_resource(TestItemsSpawned);

    let test_items = [
        (ItemType::RifleAmmo, 30, Vec3::new(3.0, 0.0, 3.0)),
        (ItemType::ShotgunShells, 15, Vec3::new(4.0, 0.0, 3.0)),
        (ItemType::Stone, 25, Vec3::new(5.0, 0.0, 3.0)),
        (ItemType::Wood, 50, Vec3::new(6.0, 0.0, 3.0)),
        (ItemType::GoldCoin, 250, Vec3::new(8.0, 0.0, 3.0)),
    ];

    for (item_type, quantity, offset) in test_items {
        let ground_y = terrain.get_height(offset.x, offset.z);
        let pos = Vec3::new(offset.x, ground_y + 0.5, offset.z);
        spawn_ground_item(&mut commands, item_type, quantity, pos);
        info!(
            "Spawned test item: {}x {} at {:?}",
            quantity,
            item_type.display_name(),
            pos
        );
    }

    let weapon_offset = Vec3::new(7.0, 0.0, 3.0);
    let ground_y = terrain.get_height(weapon_offset.x, weapon_offset.z);
    let weapon_pos = Vec3::new(weapon_offset.x, ground_y + 0.5, weapon_offset.z);
    commands.spawn((
        shared::items::GroundItem::new_weapon(WeaponType::Shotgun, 0),
        shared::items::GroundItemPosition(weapon_pos),
        Replicate::new(ReplicationMode::SingleServer(NetworkTarget::All)),
    ));
    info!(
        "Spawned test weapon: Shotgun (empty mag) at {:?}",
        weapon_pos
    );

    let chest_offset = Vec3::new(3.0, 0.0, 5.0);
    let chest_y = terrain.get_height(chest_offset.x, chest_offset.z);
    let chest_pos = Vec3::new(chest_offset.x, chest_y + 0.5, chest_offset.z);
    spawn_chest(
        &mut commands,
        chest_pos,
        vec![
            ItemStack::new_weapon(WeaponType::Pistol, 15),
            ItemStack::new_weapon(WeaponType::Sniper, 5),
            ItemStack::new(ItemType::SniperRounds, 20),
            ItemStack::new(ItemType::RifleAmmo, 60),
            ItemStack::new(ItemType::GoldCoin, 5000),
        ],
    );
    info!(
        "Spawned test chest at {:?} with Revolver, Sniper, ammo, 5000 gold",
        chest_pos
    );
}
