//! Compact selected-person card. The single expansion control opens the full record.
use super::chrome::{self, HudIcon};
use super::*;
use crate::ui::foundation::surface_block;
use crate::ui::styles::{BRASS_DARK, INK_INVERSE, INK_INVERSE_MUTED, SIGN_WOOD};
use bevy::ui::InteractionDisabled;
use shared::components::{Health, PersonId};

#[derive(Component)]
struct HealthValue;

pub(super) fn install(app: &mut App) {
    app.add_systems(
        Update,
        bind.after(crate::selection::SelectionGestureSet)
            .run_if(in_state(GameState::Playing)),
    );
}

pub(super) fn view() -> impl Bundle {
    (
        SelectionPlate,
        Name::new("Selected character card"),
        Node {
            display: Display::None,
            position_type: PositionType::Absolute,
            left: Val::Px(18.0),
            bottom: Val::Px(18.0),
            width: Val::Px(400.0),
            height: Val::Px(108.0),
            padding: UiRect::all(Val::Px(12.0)),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(8.0),
            ..default()
        },
        chrome::wood_panel(),
        surface_block(),
        Interaction::default(),
        plate_shadow(),
        crate::ui::motion::UiReveal::page(),
        children![(
            Node {
                height: Val::Px(76.0),
                align_items: AlignItems::Center,
                column_gap: Val::Px(12.0),
                ..default()
            },
            Pickable::IGNORE,
            children![
                portrait(),
                (
                    Node {
                        flex_grow: 1.0,
                        min_width: Val::Px(0.0),
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(5.0),
                        overflow: Overflow::clip(),
                        ..default()
                    },
                    Pickable::IGNORE,
                    children![
                        (
                            SelectionNameText,
                            Text::new(""),
                            crate::ui::typography::heading(21.0),
                            TextColor(INK_INVERSE),
                            TextLayout::no_wrap(),
                            Pickable::IGNORE
                        ),
                        (
                            SelectionStatusText,
                            Text::new(""),
                            crate::ui::typography::body(13.0),
                            TextColor(INK_INVERSE_MUTED),
                            TextLayout::no_wrap(),
                            Pickable::IGNORE
                        ),
                        health(),
                    ],
                ),
                (
                    SelectionExpandButton,
                    Name::new("hud-EXPAND"),
                    AccessibleLabel::new("Open selected character details"),
                    Button,
                    Node {
                        width: Val::Px(25.0),
                        height: Val::Px(32.0),
                        align_self: AlignSelf::FlexStart,
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        border_radius: BorderRadius::all(Val::Px(3.0)),
                        ..default()
                    },
                    button_chrome(UiButtonVariant::Ribbon),
                    children![chrome::icon(HudIcon::Chevron, 16.0)],
                )
            ],
        ),],
    )
}
fn portrait() -> impl Bundle {
    (
        Node {
            width: Val::Px(78.0),
            height: Val::Px(78.0),
            flex_shrink: 0.0,
            padding: UiRect::all(Val::Px(5.0)),
            ..default()
        },
        chrome::medallion(),
        Pickable::IGNORE,
        children![(
            Node {
                width: Val::Percent(100.0),
                height: Val::Percent(100.0),
                border_radius: BorderRadius::MAX,
                overflow: Overflow::clip(),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            BackgroundColor(SIGN_WOOD),
            Pickable::IGNORE,
            children![
                chrome::icon(HudIcon::Crest, 44.0),
                (
                    super::portrait::PortraitImage,
                    Node {
                        position_type: PositionType::Absolute,
                        width: Val::Percent(100.0),
                        height: Val::Percent(100.0),
                        ..default()
                    },
                    ImageNode {
                        color: Color::NONE,
                        ..default()
                    },
                    Pickable::IGNORE,
                )
            ],
        )],
    )
}
fn health() -> impl Bundle {
    (
        SelectionHealthTrack,
        Node {
            height: Val::Px(17.0),
            align_items: AlignItems::Center,
            column_gap: Val::Px(6.0),
            ..default()
        },
        Pickable::IGNORE,
        children![
            chrome::icon(HudIcon::Heart, 13.0),
            (
                Node {
                    width: Val::Px(120.0),
                    height: Val::Px(7.0),
                    border: UiRect::all(Val::Px(1.0)),
                    ..default()
                },
                BackgroundColor(SIGN_WOOD),
                BorderColor::all(BRASS_DARK),
                Pickable::IGNORE,
                children![(
                    SelectionHealthFill,
                    Node {
                        width: Val::Percent(100.0),
                        height: Val::Percent(100.0),
                        ..default()
                    },
                    BackgroundColor(Color::srgb(0.29, 0.58, 0.29)),
                    Pickable::IGNORE
                )],
            ),
            (
                HealthValue,
                Text::new(""),
                crate::ui::typography::body(12.0),
                TextColor(INK_INVERSE),
                Pickable::IGNORE
            )
        ],
    )
}

fn bind(
    mut commands: Commands,
    selection: Res<crate::selection::Selection>,
    people: Query<(Option<&Health>, Option<&PersonId>)>,
    mut labels: Query<&mut Text, With<HealthValue>>,
    buttons: Query<(Entity, Has<InteractionDisabled>), With<SelectionExpandButton>>,
) {
    let data = selection
        .primary()
        .filter(|_| selection.len() == 1)
        .and_then(|entity| people.get(entity).ok());
    let has_record = data
        .and_then(|data| data.1)
        .is_some_and(|id| id.is_assigned());
    for (entity, disabled) in &buttons {
        if has_record && disabled {
            commands.entity(entity).remove::<InteractionDisabled>();
        }
        if !has_record && !disabled {
            commands.entity(entity).insert(InteractionDisabled);
        }
    }
    let value = data
        .and_then(|data| data.0)
        .map_or_else(String::new, |health| {
            format!("{:.0}/{:.0}", health.current.max(0.0), health.max)
        });
    for mut text in &mut labels {
        if text.0 != value {
            text.0 = value.clone();
        }
    }
}
