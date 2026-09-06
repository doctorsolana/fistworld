//! Selection: left click to select, right click to order.
//!
//! The RTS input contract, and the first thing in this repo that can pick a
//! world ENTITY rather than a point on the heightfield.
//!
//! Deliberately its own module rather than more branches inside
//! `hero::control`: selection is about to cover retinues, caravans and
//! settlements, and none of that belongs in a file named after the hero. What
//! is selectable is expressed by [`Selectable`], so new kinds opt in by
//! spawning a component instead of by editing the picker.

pub mod attack_ring;
pub mod commands;
pub mod formation_preview;
pub mod order;
pub mod pick;
pub mod ring;
mod state;
pub use state::*;

use bevy::prelude::*;

use crate::states::GameState;

/// The whole selection gesture pipeline (tag -> pick -> expand -> order ->
/// rings) runs inside this set, so UI that DRAWS gesture state - the marquee
/// box, the selection plate - can order itself after it. Without that edge
/// the marquee drew one frame behind the cursor on whatever frames the
/// scheduler happened to run it first, which reads as intermittent input lag.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct SelectionGestureSet;

pub struct SelectionPlugin;

impl Plugin for SelectionPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<Selection>();
        app.init_resource::<formation_preview::PreviewReadiness>();
        app.insert_gizmo_config(
            formation_preview::FormationGizmos,
            GizmoConfig {
                depth_bias: -0.001,
                ..default()
            },
        );
        app.init_resource::<commands::CommandMode>();
        app.init_resource::<commands::ControlGroups>();
        app.init_resource::<crate::army_roster::ArmyRoster>();
        app.add_systems(
            Update,
            crate::army_roster::refresh_army_roster
                .in_set(crate::army_roster::ArmyRosterSet)
                .before(SelectionGestureSet)
                .run_if(in_state(GameState::Playing)),
        );
        app.init_resource::<RightDrag>();
        app.init_resource::<DragBox>();
        app.add_systems(
            Update,
            (
                // Order matters: drop a dead selection before anything reads it,
                // then pick, then let the ring follow what is now selected.
                tag_characters_selectable,
                tag_player_boats_selectable,
                retire_wrecked_boats,
                tag_settlements_selectable,
                tag_settlement_buildings_selectable,
                tag_construction_sites_selectable,
                clear_stale_selection,
                pick::pick_on_left_click
                    .after(crate::camera_rts::update_cursor_terrain_hit)
                    .after(crate::hero::sync_hero_transforms),
                expand_standard_bearer_selection,
                commands::handle_command_keys,
                commands::receive_order_feedback,
                crate::capture::drive_live_voyage_click_input,
                order::issue_order_on_right_click,
                formation_preview::draw_formation_preview,
                ring::sync_selection_ring,
                attack_ring::hover_attack_target,
                attack_ring::sync_authoritative_targets,
                attack_ring::sync_attack_rings,
            )
                .chain()
                .in_set(SelectionGestureSet)
                .run_if(in_state(GameState::Playing)),
        );
        app.add_systems(
            OnExit(GameState::Playing),
            (clear_on_exit, commands::reset_commands),
        );
    }
}
