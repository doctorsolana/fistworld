//! Small native UI ornaments. They never carry text, input or a screen bitmap.
use super::styles::BRASS;
use bevy::prelude::*;

pub fn corners(parent: &mut ChildSpawnerCommands<'_>) {
    for (left, top) in [(true, true), (false, true), (true, false), (false, false)] {
        parent.spawn((
            Pickable::IGNORE,
            ZIndex(10),
            Node {
                position_type: PositionType::Absolute,
                left: if left { Val::Px(5.0) } else { Val::Auto },
                right: if left { Val::Auto } else { Val::Px(5.0) },
                top: if top { Val::Px(5.0) } else { Val::Auto },
                bottom: if top { Val::Auto } else { Val::Px(5.0) },
                width: Val::Px(13.0),
                height: Val::Px(13.0),
                border: UiRect {
                    left: Val::Px(if left { 2.0 } else { 0.0 }),
                    right: Val::Px(if left { 0.0 } else { 2.0 }),
                    top: Val::Px(if top { 2.0 } else { 0.0 }),
                    bottom: Val::Px(if top { 0.0 } else { 2.0 }),
                },
                ..default()
            },
            BorderColor::all(BRASS),
        ));
    }
}
