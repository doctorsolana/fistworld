//! Menu composition using existing compressed launcher/ledger artwork.
use super::*;
use crate::ui::{ledger, startup::StartupArtwork, typography};

#[derive(Component, Clone, Copy, PartialEq, Eq)]
#[require(crate::ui::foundation::UiArtworkFocus)]
pub(super) enum ControlFace {
    Brass,
    Choice,
    Dark,
    Gold,
    Danger,
}

#[derive(Component)]
pub(super) struct NavigationSelection;

pub(super) fn bind_control_faces(
    mut commands: Commands,
    art: Res<StartupArtwork>,
    buttons: Query<
        (Entity, &ControlFace, &UiButtonStyle),
        Or<(Changed<ControlFace>, Changed<UiButtonStyle>)>,
    >,
    mut indicators: Query<(&ChildOf, &mut Node), With<NavigationSelection>>,
) {
    for (entity, face, style) in &buttons {
        let mut image = match face {
            ControlFace::Brass => art.brass(),
            ControlFace::Gold => art.gold(),
            ControlFace::Choice if style.selected => art.gold(),
            _ => art.dark_button(),
        };
        if *face == ControlFace::Danger {
            image.color = Color::srgb(1.0, 0.72, 0.63);
        }
        commands
            .entity(entity)
            .insert(ledger::LedgerButtonFace(image));
        for (parent, mut node) in &mut indicators {
            if parent.parent() == entity {
                node.display = if style.selected {
                    Display::Flex
                } else {
                    Display::None
                };
            }
        }
    }
}

pub(super) fn label(parent: &mut ChildSpawnerCommands<'_>, text: &str, size: f32) {
    parent.spawn((
        Text::new(text),
        typography::reading(size),
        TextColor(INK),
        Pickable::IGNORE,
    ));
}
pub(super) fn section(parent: &mut ChildSpawnerCommands<'_>, text: &str) {
    parent
        .spawn((
            Node {
                align_items: AlignItems::Center,
                column_gap: Val::Px(10.0),
                margin: UiRect::top(Val::Px(8.0)),
                flex_shrink: 0.0,
                ..default()
            },
            Pickable::IGNORE,
        ))
        .with_children(|row| {
            icons::section(row, text);
            row.spawn((
                Text::new(text),
                typography::heading(24.0),
                TextColor(INK),
                Pickable::IGNORE,
            ));
        });
    parent.spawn(ledger::rule());
}
pub(super) fn rule(parent: &mut ChildSpawnerCommands<'_>) {
    parent.spawn(ledger::ornament_rule()).insert(Node {
        width: Val::Percent(100.0),
        height: Val::Px(1.0),
        flex_shrink: 0.0,
        margin: UiRect::vertical(Val::Px(10.0)),
        ..default()
    });
}

pub(super) fn action_button(
    parent: &mut ChildSpawnerCommands<'_>,
    text: &str,
    action: PauseButton,
    height: f32,
    show_icon: bool,
) {
    let (variant, face) = match action {
        PauseButton::Resume => (UiButtonVariant::Primary, ControlFace::Gold),
        PauseButton::Disconnect | PauseButton::Exit if show_icon => {
            (UiButtonVariant::Danger, ControlFace::Danger)
        }
        PauseButton::Disconnect | PauseButton::Exit => (UiButtonVariant::Danger, ControlFace::Dark),
        _ => (UiButtonVariant::Inverse, ControlFace::Choice),
    };
    parent
        .spawn((
            Button,
            action,
            face,
            Name::new(format!("pause-{text}")),
            bevy::ui::RelativeCursorPosition::default(),
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(height),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                padding: UiRect::horizontal(Val::Px(20.0)),
                ..default()
            },
            button_chrome(variant),
        ))
        .with_children(|button| {
            if show_icon {
                button
                    .spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(24.0),
                            width: Val::Px(26.0),
                            height: Val::Px(26.0),
                            ..default()
                        },
                        Pickable::IGNORE,
                    ))
                    .with_children(|slot| {
                        icons::navigation(slot, action, crate::ui::startup::widgets::IVORY, 26.0);
                    });
            } else if matches!(
                action,
                PauseButton::Graphics | PauseButton::Audio | PauseButton::Controls
            ) {
                button.spawn((
                    NavigationSelection,
                    Node {
                        display: Display::None,
                        position_type: PositionType::Absolute,
                        right: Val::Px(-13.0),
                        width: Val::Px(7.0),
                        height: Val::Px(7.0),
                        ..default()
                    },
                    UiTransform::from_rotation(Rot2::radians(std::f32::consts::FRAC_PI_4)),
                    BackgroundColor(crate::ui::startup::widgets::GOLD),
                    Pickable::IGNORE,
                ));
            }
            button.spawn((
                UiButtonLabel,
                Text::new(text),
                typography::heading(if show_icon { 22.0 } else { 24.0 }),
                TextColor(INK_INVERSE),
                TextLayout::no_wrap(),
                Pickable::IGNORE,
            ));
        });
}
