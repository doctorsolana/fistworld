//! First-person weapon view and 3D weapon models
//!
//! Shows the currently equipped weapon in first-person view and handles weapon switching.
//! Updated for Lightyear 0.26 / Bevy 0.18

mod assets;
mod hotbar_input;
mod third_person;
mod view_model;
mod weapon_hud;

pub use assets::{setup_weapon_model_assets, WeaponModelAssets};
pub use hotbar_input::handle_weapon_switch;
pub use third_person::{
    despawn_remote_third_person_weapons, despawn_third_person_weapon,
    update_remote_third_person_weapons, update_third_person_weapon, CurrentThirdPersonWeapon,
    RemoteThirdPersonWeapon, RemoteWeaponIndex,
};
pub use view_model::{
    disable_first_person_weapon_shadows, update_first_person_weapon, update_weapon_animation,
    CurrentWeaponView, FirstPersonWeapon,
};
pub use weapon_hud::{despawn_weapon_hud, spawn_weapon_hud, update_weapon_hud};
