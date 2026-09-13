//! The same worn frame serves the editable form and the smaller neutral busy state.
use super::*;
use crate::ui::{
    foundation::UiButtonLabel,
    ledger,
    startup::{widgets, StartupArtwork},
    typography,
};

#[derive(Component)]
pub(super) struct NameForm;
#[derive(Component)]
pub(super) struct NameBusy;

pub(super) fn spawn_name_entry_ui(
    mut commands: Commands,
    art: Res<StartupArtwork>,
    input: Res<PlayerNameInput>,
) {
    commands
        .spawn((NameEntryRoot, widgets::screen()))
        .with_children(|root| {
            widgets::small_wordmark(root, &art);
            root.spawn((
                NameForm,
                widgets::panel(),
                // A continuous backing keeps scenery out of the paper's
                // transparent torn edge, including when feedback grows the form.
                ledger::wood(),
                Node {
                    width: Val::Px(510.0),
                    max_width: Val::Vw(80.0),
                    flex_direction: FlexDirection::Column,
                    margin: UiRect::top(Val::Px(24.0)),
                    ..default()
                },
            ))
            .with_children(|panel| {
                panel
                    .spawn((
                        Name::new("startup-name-header"),
                        Node {
                            width: Val::Percent(100.0),
                            height: Val::Px(112.0),
                            flex_shrink: 0.0,
                            flex_direction: FlexDirection::Column,
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            row_gap: Val::Px(6.0),
                            ..default()
                        },
                        ledger::wood(),
                    ))
                    .with_children(|header| {
                        header.spawn((
                            Name::new("startup-heading"),
                            Text::new("YOUR NAME"),
                            typography::heading(44.0),
                            TextColor(widgets::IVORY),
                            TextShadow {
                                offset: Vec2::new(0.0, 2.0),
                                color: widgets::INK,
                            },
                            Pickable::IGNORE,
                        ));
                        widgets::rule(header, 282.0, false);
                    });
                panel
                    .spawn((
                        Name::new("startup-name-paper"),
                        Node {
                            width: Val::Percent(100.0),
                            flex_direction: FlexDirection::Column,
                            align_items: AlignItems::Center,
                            // Lay the parchment over the wood, rather than
                            // joining two irregular image edges end-to-end.
                            margin: UiRect::top(Val::Px(-8.0)),
                            padding: UiRect::axes(Val::Px(58.0), Val::Px(36.0)),
                            row_gap: Val::Px(23.0),
                            ..default()
                        },
                        art.paper(),
                    ))
                    .with_children(|page| {
                        page.spawn((
                            Text::new("PLAYER NAME"),
                            typography::heading(23.0),
                            TextColor(widgets::INK),
                            Pickable::IGNORE,
                        ));
                        page.spawn((
                            NameInputField,
                            Name::new("startup-name-field"),
                            widgets::field(&art, 80.0),
                        ))
                        .insert(Node {
                            width: Val::Percent(100.0),
                            height: Val::Px(80.0),
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            padding: UiRect::horizontal(Val::Px(12.0)),
                            border: UiRect::all(Val::Px(2.0)),
                            overflow: Overflow::clip(),
                            ..default()
                        })
                        .with_children(|field| {
                            field.spawn((
                                NameInputDisplay,
                                UiButtonLabel,
                                Node {
                                    flex_shrink: 0.0,
                                    ..default()
                                },
                                Text::new(&input.name),
                                typography::reading(35.0),
                                TextColor(widgets::INK),
                                TextLayout::no_wrap(),
                                Pickable::IGNORE,
                            ));
                        });
                        page.spawn((
                            Text::new("Letters, numbers, _ or -"),
                            typography::reading(16.0),
                            TextColor(widgets::INK),
                            Pickable::IGNORE,
                        ));
                        page.spawn((
                            ErrorMessageText,
                            Name::new("startup-error"),
                            Text::new(""),
                            typography::reading(15.0),
                            TextColor(Color::srgb(0.53, 0.10, 0.055)),
                            Node {
                                display: Display::None,
                                max_width: Val::Percent(100.0),
                                ..default()
                            },
                            TextLayout::justify(Justify::Center),
                            Pickable::IGNORE,
                        ));
                        widgets::action_button(
                            page,
                            &art,
                            "JOIN GAME",
                            (SubmitButton, crate::ui::sound::UiSoundHandled),
                            Val::Percent(100.0),
                            84.0,
                            true,
                        );
                        widgets::back_button(page, "Back", BackButton);
                    });
                widgets::frame(panel, &art);
            });
            widgets::loading_panel(root, &art, "Preparing world…", NameBusy);
            root.spawn((
                NameBusy,
                Node {
                    position_type: PositionType::Absolute,
                    top: Val::Percent(50.0),
                    margin: UiRect::top(Val::Px(196.0)),
                    ..default()
                },
            ))
            .with_children(|below| {
                widgets::action_button(
                    below,
                    &art,
                    "CANCEL",
                    BackButton,
                    Val::Px(180.0),
                    46.0,
                    false,
                );
            });
        });
}

pub(super) fn sync_name_entry_ui(
    phase: Res<NameEntryPhase>,
    feedback: Res<NameSubmissionFeedback>,
    mut forms: Query<&mut Node, (With<NameForm>, Without<NameBusy>, Without<ErrorMessageText>)>,
    mut busy: Query<&mut Node, (With<NameBusy>, Without<NameForm>, Without<ErrorMessageText>)>,
    mut errors: Query<
        (&mut Text, &mut Node),
        (
            With<ErrorMessageText>,
            Without<NameForm>,
            Without<NameBusy>,
            Without<widgets::LoadingStatus>,
        ),
    >,
    mut status: Query<&mut Text, (With<widgets::LoadingStatus>, Without<ErrorMessageText>)>,
) {
    let form_display = if phase.is_busy() {
        Display::None
    } else {
        Display::Flex
    };
    let busy_display = if phase.is_busy() {
        Display::Flex
    } else {
        Display::None
    };
    for mut node in &mut forms {
        if node.display != form_display {
            node.display = form_display;
        }
    }
    for mut node in &mut busy {
        if node.display != busy_display {
            node.display = busy_display;
        }
    }
    for (mut text, mut node) in &mut errors {
        let message = feedback.error_message.as_deref().unwrap_or_default();
        if text.0 != message {
            text.0 = message.into();
        }
        let display = if message.is_empty() {
            Display::None
        } else {
            Display::Flex
        };
        if node.display != display {
            node.display = display;
        }
    }
    for mut text in &mut status {
        if text.0 != phase.title() {
            text.0 = phase.title().into();
        }
    }
}

pub(super) fn despawn_name_entry_ui(
    mut commands: Commands,
    roots: Query<Entity, With<NameEntryRoot>>,
) {
    for entity in &roots {
        commands.entity(entity).despawn();
    }
}
