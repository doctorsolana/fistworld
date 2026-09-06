//! Retained army management: choose a battalion, edit its roster, set policy.
//! Only membership changes replace list rows; vitals and choices bind in place.
use super::{EncyclopediaTab, TabBody};
use crate::army_roster::{ArmyRoster, BattalionFacts};
use bevy::prelude::*;
use bevy::ui::InteractionDisabled;
use shared::components::*;
mod actions;
mod binding;
mod model;
mod view;
pub(crate) use actions::handle_army_buttons;
pub(crate) use binding::sync_army_panel;
use model::*;
pub(crate) use model::{ArmyAction, ArmyManagement, BoundText};
pub(crate) use view::spawn_army_tab;

pub(super) fn army_tab_active(tab: Res<EncyclopediaTab>) -> bool {
    *tab == EncyclopediaTab::Army
}
#[cfg(test)]
mod tests;
