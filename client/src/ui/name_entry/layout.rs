//! layout systems.

use super::*;

pub(super) fn spawn_name_entry_ui(mut commands: Commands, mut name_input: ResMut<PlayerNameInput>) {
    // Reset input state
    name_input.name.clear();
    name_input.submitted = false;

    commands
        .spawn((
            NameEntryRoot,
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                ..default()
            },
            BackgroundColor(Color::srgba(0.0, 0.0, 0.0, 0.85)),
        ))
        .with_children(|root| {
            // Panel
            root.spawn(Node {
                flex_direction: FlexDirection::Column,
                padding: UiRect::all(Val::Px(40.0)),
                row_gap: Val::Px(20.0),
                ..default()
            })
            .with_children(|panel| {
                // Title
                panel
                    .spawn(Node {
                        margin: UiRect::bottom(Val::Px(20.0)),
                        ..default()
                    })
                    .insert(Text::new("Enter Player Name"))
                    .insert(TextFont {
                        font_size: FontSize::Px(32.0),
                        ..default()
                    })
                    .insert(TextColor(INK_INVERSE));

                // Input field
                panel
                    .spawn(Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(8.0),
                        ..default()
                    })
                    .with_children(|input_col| {
                        input_col
                            .spawn(Text::new("Name:"))
                            .insert(TextFont {
                                font_size: FontSize::Px(18.0),
                                ..default()
                            })
                            .insert(TextColor(INK_INVERSE));

                        input_col
                            .spawn((
                                Node {
                                    width: Val::Px(300.0),
                                    height: Val::Px(40.0),
                                    padding: UiRect::all(Val::Px(10.0)),
                                    border_radius: BorderRadius::all(Val::Px(4.0)),
                                    ..default()
                                },
                                BackgroundColor(SLATE),
                            ))
                            .with_children(|input_box| {
                                input_box
                                    .spawn(NameInputDisplay)
                                    .insert(Text::new(""))
                                    .insert(TextFont {
                                        font_size: FontSize::Px(18.0),
                                        ..default()
                                    })
                                    .insert(TextColor(INK_INVERSE));
                            });

                        // Helper text
                        input_col
                            .spawn(Text::new("3-16 characters, alphanumeric only"))
                            .insert(TextFont {
                                font_size: FontSize::Px(12.0),
                                ..default()
                            })
                            .insert(TextColor(Color::srgba(0.7, 0.7, 0.7, 0.8)));
                    });

                // Error message (initially hidden)
                panel
                    .spawn(ErrorMessageText)
                    .insert(Text::new(""))
                    .insert(TextFont {
                        font_size: FontSize::Px(14.0),
                        ..default()
                    })
                    .insert(TextColor(ACCENT_RED))
                    .insert(Node {
                        min_height: Val::Px(20.0),
                        ..default()
                    });

                // Submit button
                panel
                    .spawn((
                        SubmitButton,
                        Button,
                        Node {
                            width: Val::Px(150.0),
                            height: Val::Px(45.0),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            margin: UiRect::top(Val::Px(10.0)),
                            border_radius: BorderRadius::all(Val::Px(4.0)),
                            ..default()
                        },
                        button_chrome(UiButtonVariant::Primary),
                    ))
                    .with_children(|btn| {
                        btn.spawn(Text::new("Join Game"))
                            .insert(UiButtonLabel)
                            .insert(TextFont {
                                font_size: FontSize::Px(20.0),
                                ..default()
                            })
                            .insert(TextColor(INK_INVERSE));
                    });
            });
        });
}

pub(super) fn despawn_name_entry_ui(
    mut commands: Commands,
    query: Query<Entity, With<NameEntryRoot>>,
) {
    for entity in query.iter() {
        commands.entity(entity).despawn();
    }
}
