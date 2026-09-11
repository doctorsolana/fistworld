//! Bounded card navigation. The selection card and map controls always retain
//! their own corner space, even when an army has dozens of battalions.

use super::*;
use bevy::ui::InteractionDisabled;

#[derive(Component)]
pub(super) struct CardViewport;

#[derive(Component)]
pub(super) struct PageButton(i8);

pub(super) fn spawn_button(parent: &mut ChildSpawnerCommands<'_>, direction: i8) {
    parent.spawn((
        Name::new(if direction < 0 {
            "Previous battalions"
        } else {
            "Next battalions"
        }),
        PageButton(direction),
        Button,
        Visibility::Hidden,
        button_chrome(UiButtonVariant::Inverse),
        Node {
            width: Val::Px(26.0),
            height: Val::Px(46.0),
            margin: UiRect::bottom(Val::Px(25.0)),
            flex_shrink: 0.0,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(3.0)),
            ..default()
        },
        children![(
            Text::new(if direction < 0 { "‹" } else { "›" }),
            UiButtonLabel,
            crate::ui::typography::heading(22.0),
            TextColor(PARCHMENT),
            Pickable::IGNORE,
        )],
    ));
}

fn viewport_extent(computed: &ComputedNode) -> (f32, f32) {
    let width = computed.size().x * computed.inverse_scale_factor();
    let content = computed.content_size().x * computed.inverse_scale_factor();
    (width, (content - width).max(0.0))
}

pub(super) fn handle_navigation(
    mode: Res<CombatMode>,
    input: Res<crate::input::InputState>,
    buttons: Query<
        (&Interaction, &PageButton),
        (Changed<Interaction>, Without<InteractionDisabled>),
    >,
    mut viewports: Query<(&ComputedNode, &mut ScrollPosition), With<CardViewport>>,
) {
    if !mode.0 || input.ui_blocking() {
        return;
    }
    for (interaction, direction) in &buttons {
        if *interaction != Interaction::Pressed {
            continue;
        }
        for (computed, mut position) in &mut viewports {
            let (width, maximum) = viewport_extent(computed);
            // Retain one card as context when moving to the next page.
            let stride = CARD_WIDTH + CARD_GAP;
            let page = ((width / stride).floor() - 1.0).max(1.0) * stride;
            position.0.x = (position.0.x + direction.0 as f32 * page).clamp(0.0, maximum);
        }
    }
}

/// Bring a newly selected battalion into view, but do not wrestle with a
/// player browsing the dock while the same soldiers remain selected.
fn reveal_card(current: f32, viewport: f32, maximum: f32, index: usize) -> f32 {
    let left = index as f32 * (CARD_WIDTH + CARD_GAP);
    let right = left + CARD_WIDTH;
    if left < current {
        left.clamp(0.0, maximum)
    } else if right > current + viewport {
        (right - viewport).clamp(0.0, maximum)
    } else {
        current.clamp(0.0, maximum)
    }
}

pub(super) fn bind_navigation(
    mut commands: Commands,
    selection: Res<crate::selection::Selection>,
    roster: Res<ArmyRoster>,
    mut viewports: Query<(&ComputedNode, &mut ScrollPosition), With<CardViewport>>,
    mut buttons: Query<(
        Entity,
        &PageButton,
        &mut Visibility,
        Has<InteractionDisabled>,
    )>,
) {
    let Ok((computed, mut position)) = viewports.single_mut() else {
        return;
    };
    let (width, maximum) = viewport_extent(computed);
    if width <= 0.0 {
        return;
    }
    let mut next = position.0.x.clamp(0.0, maximum);
    if selection.is_changed() {
        if let Some(index) = roster.battalions.iter().position(|battalion| {
            battalion
                .members
                .iter()
                .any(|member| selection.is_selected(*member))
        }) {
            next = reveal_card(next, width, maximum, index);
        }
    }
    if position.0.x != next {
        position.0.x = next;
    }
    for (entity, button, mut visibility, disabled) in &mut buttons {
        // Keep the narrow arrow slots in layout while hidden, avoiding an
        // overflow threshold that flips every frame as arrows appear.
        visibility.set_if_neq(if maximum > 1.0 {
            Visibility::Inherited
        } else {
            Visibility::Hidden
        });
        let should_disable = if button.0 < 0 {
            next <= 0.5
        } else {
            next >= maximum - 0.5
        };
        if should_disable != disabled {
            if should_disable {
                commands.entity(entity).insert(InteractionDisabled);
            } else {
                commands.entity(entity).remove::<InteractionDisabled>();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selecting_an_offscreen_battalion_reveals_the_whole_card() {
        assert_eq!(reveal_card(0.0, 360.0, 1_000.0, 8), 460.0);
        assert_eq!(reveal_card(460.0, 360.0, 1_000.0, 0), 0.0);
        assert_eq!(reveal_card(100.0, 360.0, 1_000.0, 3), 100.0);
    }

    #[test]
    fn shrinking_roster_clamps_scroll_without_overshooting_the_last_card() {
        assert_eq!(reveal_card(900.0, 360.0, 50.0, 4), 50.0);
        assert_eq!(reveal_card(900.0, 360.0, 0.0, 0), 0.0);
    }
}
