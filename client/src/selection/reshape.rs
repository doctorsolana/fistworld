//! Explicit in-place width/rotation controls, separate from destination clicks.
use bevy::prelude::*;
use lightyear::prelude::{Connected, MessageSender};
use shared::{components::*, protocol::*};

pub fn reshape_selected(
    keys: Res<ButtonInput<KeyCode>>,
    combat: Res<crate::combat_mode::CombatMode>,
    input: Res<crate::input::InputState>,
    placement: Res<crate::hero::control::WorldPlacementMode>,
    drag: Res<super::RightDrag>,
    selection: Res<super::Selection>,
    roster: Res<crate::army_roster::ArmyRoster>,
    people: Query<(&PlayerPosition, &PlayerRotation)>,
    mut sender: Query<&mut MessageSender<UnitOrder>, (With<crate::GameClient>, With<Connected>)>,
    mut notice: ResMut<crate::ui::hud::GodNotice>,
) {
    let width = i32::from(keys.just_pressed(KeyCode::BracketRight))
        - i32::from(keys.just_pressed(KeyCode::BracketLeft));
    let turn = i32::from(keys.just_pressed(KeyCode::Period))
        - i32::from(keys.just_pressed(KeyCode::Comma));
    if (width == 0 && turn == 0)
        || !combat.0
        || input.ui_blocking()
        || drag.formation
        || crate::hero::control::placement_armed(&placement)
    {
        return;
    }
    let Ok(mut sender) = sender.single_mut() else {
        return;
    };
    let selected = roster.selection(&selection.entities);
    let mut count = 0;
    for battalion in &roster.battalions {
        if !selected.battalions.contains(&battalion.id) || battalion.members.is_empty() {
            continue;
        }
        let mut centre = Vec3::ZERO;
        let mut facing = Vec2::ZERO;
        let mut living: usize = 0;
        for entity in &battalion.members {
            if let Ok((p, r)) = people.get(*entity) {
                centre += p.0;
                facing += Vec2::new(-r.0.sin(), -r.0.cos());
                living += 1;
            }
        }
        if living == 0 {
            continue;
        }
        centre /= living as f32;
        facing = facing.try_normalize().unwrap_or(Vec2::Y);
        facing = Mat2::from_angle(-(turn as f32) * 15.0_f32.to_radians()) * facing;
        let files = (i32::from(battalion.formation.files) + width * 2)
            .clamp(2.min(living) as i32, living as i32) as usize;
        let depth = (living.div_ceil(files) - 1) as f32 * shared::formation::RANK_SPACING;
        let target = centre + Vec3::new(facing.x, 0.0, facing.y) * depth * 0.5;
        sender.send::<ReliableChannel>(UnitOrder {
            selection: UnitSelection {
                units: vec![],
                battalions: vec![battalion.id],
            },
            command: UnitCommand::Move {
                target,
                frontage: Some(FormationFrontage {
                    facing,
                    width: ((files - 1) as f32 * battalion.formation.spacing).max(1.0),
                }),
                mode: MovementMode::Move,
            },
        });
        count += 1;
    }
    if count > 0 {
        notice.show(if width != 0 {
            "Reshaping selected battalions in place"
        } else {
            "Turning selected battalions 15 degrees"
        });
    }
}
