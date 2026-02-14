use bevy::prelude::*;
use shared::components::{EquippedWeapon, LocalPlayer};
use shared::items::HotbarSelection;
use shared::weapons::WeaponType;

use crate::input::InputState;

use super::{CurrentWeaponView, FirstPersonWeapon};

/// Marker for weapon HUD root.
#[derive(Component)]
pub struct WeaponHUD;

/// Marker for ammo text.
#[derive(Component)]
pub struct AmmoText;

/// Marker for weapon name text.
#[derive(Component)]
pub struct WeaponNameText;

/// Marker for hotbar slots display.
#[derive(Component)]
pub struct HotbarSlotsText;

/// Spawn the weapon HUD.
pub fn spawn_weapon_hud(mut commands: Commands) {
    // HUD container at bottom-right.
    commands
        .spawn((
            WeaponHUD,
            Node {
                position_type: PositionType::Absolute,
                right: Val::Px(20.0),
                bottom: Val::Px(20.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::End,
                row_gap: Val::Px(5.0),
                ..default()
            },
        ))
        .with_children(|parent| {
            // Weapon name.
            parent.spawn((
                WeaponNameText,
                Text::new("Assault Rifle"),
                TextFont {
                    font_size: 24.0,
                    ..default()
                },
                TextColor(Color::srgba(1.0, 1.0, 1.0, 0.9)),
            ));

            // Ammo counter.
            parent.spawn((
                AmmoText,
                Text::new("30 / 90"),
                TextFont {
                    font_size: 32.0,
                    ..default()
                },
                TextColor(Color::srgba(1.0, 0.9, 0.6, 1.0)),
            ));

            // Hotbar slots display (dynamically updated).
            parent.spawn((
                HotbarSlotsText,
                Text::new("[1] -  [2] -  [3] -  [4] -  [5] -  [6] -"),
                TextFont {
                    font_size: 12.0,
                    ..default()
                },
                TextColor(Color::srgba(0.7, 0.7, 0.7, 0.6)),
            ));
        });
}

/// Despawn weapon HUD and first-person weapon model when leaving gameplay.
pub fn despawn_weapon_hud(
    mut commands: Commands,
    hud: Query<Entity, With<WeaponHUD>>,
    weapon_models: Query<Entity, With<FirstPersonWeapon>>,
    mut current_view: ResMut<CurrentWeaponView>,
) {
    for entity in hud.iter() {
        commands.entity(entity).despawn();
    }
    for entity in weapon_models.iter() {
        commands.entity(entity).despawn();
    }
    // Reset the weapon view state.
    current_view.weapon_type = None;
}

/// Update HUD to show current weapon and ammo.
pub fn update_weapon_hud(
    local_player: Query<
        (&EquippedWeapon, &shared::items::Inventory, &HotbarSelection),
        With<LocalPlayer>,
    >,
    mut weapon_text: Query<
        &mut Text,
        (
            With<WeaponNameText>,
            Without<AmmoText>,
            Without<HotbarSlotsText>,
        ),
    >,
    mut ammo_text: Query<
        &mut Text,
        (
            With<AmmoText>,
            Without<WeaponNameText>,
            Without<HotbarSlotsText>,
        ),
    >,
    mut hotbar_text: Query<
        &mut Text,
        (
            With<HotbarSlotsText>,
            Without<WeaponNameText>,
            Without<AmmoText>,
        ),
    >,
    mut hud_visibility: Query<&mut Visibility, With<WeaponHUD>>,
    input_state: Res<InputState>,
) {
    // Hide HUD in vehicle or while modal UI is open.
    for mut vis in hud_visibility.iter_mut() {
        *vis = if input_state.in_vehicle || input_state.ui_blocking() {
            Visibility::Hidden
        } else {
            Visibility::Visible
        };
    }

    let Some((weapon, inventory, hotbar_selection)) = local_player.iter().next() else {
        return;
    };

    // Update weapon name.
    for mut text in weapon_text.iter_mut() {
        **text = weapon_name(weapon.weapon_type);
    }

    // Update hotbar slots display.
    for mut text in hotbar_text.iter_mut() {
        let mut slots = Vec::new();
        for i in 0..shared::items::HOTBAR_SLOTS {
            let slot_name = if let Some(stack) = inventory.get_slot(i) {
                hotbar_item_name(&stack.item_type)
            } else {
                "-".to_string()
            };
            // Highlight active slot.
            if i == hotbar_selection.index as usize {
                slots.push(format!(">[{}] {}<", i + 1, slot_name));
            } else {
                slots.push(format!("[{}] {}", i + 1, slot_name));
            }
        }
        **text = slots.join("  ");
    }

    // Update ammo - reserve comes from inventory (no ammo display when Unarmed).
    if weapon.weapon_type == WeaponType::Unarmed {
        for mut text in ammo_text.iter_mut() {
            **text = String::new();
        }
        return;
    }

    let reserve_ammo = inventory.count_item(weapon.weapon_type.ammo_type());
    for mut text in ammo_text.iter_mut() {
        **text = format!("{} / {}", weapon.ammo_in_mag, reserve_ammo);
    }
}

/// Get display name for weapon type.
fn weapon_name(weapon: WeaponType) -> String {
    match weapon {
        WeaponType::Pistol => "Revolver".to_string(),
        WeaponType::AssaultRifle => "Automatic Rifle".to_string(),
        WeaponType::Sniper => "Sniper Rifle".to_string(),
        WeaponType::Shotgun => "Shotgun".to_string(),
        WeaponType::Unarmed => "Unarmed".to_string(),
    }
}

/// Get short display name for hotbar items.
fn hotbar_item_name(item_type: &shared::items::ItemType) -> String {
    use shared::items::ItemType;
    match item_type {
        ItemType::Weapon(w) => match w {
            WeaponType::Pistol => "Revolver".to_string(),
            WeaponType::AssaultRifle => "AR".to_string(),
            WeaponType::Sniper => "Sniper".to_string(),
            WeaponType::Shotgun => "Shotgun".to_string(),
            WeaponType::Unarmed => "-".to_string(),
        },
        ItemType::RifleAmmo => "Bullets".to_string(),
        ItemType::ShotgunShells => "Shells".to_string(),
        ItemType::SniperRounds => ".308".to_string(),
        ItemType::Wood => "Wood".to_string(),
        ItemType::Stone => "Stone".to_string(),
        ItemType::GoldCoin => "Gold".to_string(),
    }
}
