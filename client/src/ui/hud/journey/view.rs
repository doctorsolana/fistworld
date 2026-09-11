//! Retained notice button and optional detail tray, composed below the clock.
use super::*;
use crate::ui::{
    foundation::surface_block,
    motion::UiReveal,
    styles::{BRASS, PARCHMENT},
};

pub(in crate::ui::hud) fn view() -> impl Bundle {
    (
        JourneyPlate,
        Name::new("Journey notices"),
        Node {
            display: Display::None,
            width: Val::Px(340.0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::FlexEnd,
            row_gap: Val::Px(8.0),
            ..default()
        },
        Pickable::IGNORE,
        children![
            (
                JourneyAction::Toggle,
                Button,
                Name::new("journey-NOTICES"),
                Node {
                    min_height: Val::Px(36.0),
                    padding: UiRect::axes(Val::Px(12.0), Val::Px(7.0)),
                    column_gap: Val::Px(12.0),
                    align_items: AlignItems::Center,
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(RADIUS)),
                    ..default()
                },
                button_chrome(UiButtonVariant::Inverse),
                plate_shadow(),
                children![
                    (
                        Text::new("NOTICES"),
                        UiButtonLabel,
                        crate::ui::typography::heading(14.0),
                        TextColor(PARCHMENT),
                        Pickable::IGNORE
                    ),
                    (
                        JourneyText::Unread,
                        Text::new(""),
                        crate::ui::typography::body(14.0),
                        TextColor(BRASS),
                        Pickable::IGNORE
                    ),
                    (
                        JourneyText::Toggle,
                        Text::new("+"),
                        crate::ui::typography::body(18.0),
                        TextColor(PARCHMENT),
                        Pickable::IGNORE
                    ),
                ],
            ),
            (
                JourneyDetails,
                Name::new("Notice details"),
                Node {
                    display: Display::None,
                    width: Val::Percent(100.0),
                    max_height: Val::Px(510.0),
                    min_height: Val::Px(0.0),
                    overflow: Overflow::scroll_y(),
                    scrollbar_width: 6.0,
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(10.0),
                    padding: UiRect::all(Val::Px(14.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(RADIUS)),
                    ..default()
                },
                UiReveal::page(),
                Interaction::default(),
                surface_block(),
                BackgroundColor(LIMEWASH),
                BorderColor::all(PLATE_RULE),
                plate_shadow(),
                children![
                    (
                        Node {
                            width: Val::Percent(100.0),
                            justify_content: JustifyContent::SpaceBetween,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        Pickable::IGNORE,
                        children![
                            heading("RECENT MESSAGES"),
                            action(JourneyAction::Clear, "CLEAR", UiButtonVariant::Ghost)
                        ],
                    ),
                    label(JourneyText::Message(0), "No recent messages."),
                    label(JourneyText::Message(1), ""),
                    label(JourneyText::Message(2), ""),
                    divider(),
                    heading("NEARBY"),
                    label(JourneyText::Town, ""),
                    label(JourneyText::Hint, ""),
                    divider(),
                    heading("YOUR CHARACTER"),
                    label(JourneyText::Hero, ""),
                    (
                        Node {
                            column_gap: Val::Px(6.0),
                            row_gap: Val::Px(6.0),
                            flex_wrap: FlexWrap::Wrap,
                            ..default()
                        },
                        Pickable::IGNORE,
                        children![
                            action(
                                JourneyAction::Hero,
                                "HERO [HOME]",
                                UiButtonVariant::Secondary
                            ),
                            action(
                                JourneyAction::Town(None),
                                "VIEW TOWN",
                                UiButtonVariant::Secondary
                            ),
                            action(JourneyAction::Map, "MAP [M]", UiButtonVariant::Secondary),
                        ],
                    ),
                ],
            ),
        ],
    )
}

fn heading(text: &str) -> impl Bundle {
    (
        Text::new(text),
        crate::ui::typography::heading(13.0),
        TextColor(INK_MUTED),
        Pickable::IGNORE,
    )
}

fn label(field: JourneyText, text: &str) -> impl Bundle {
    (
        field,
        Text::new(text),
        crate::ui::typography::body(14.0),
        TextColor(INK),
        Pickable::IGNORE,
    )
}

fn action(action: JourneyAction, label: &str, variant: UiButtonVariant) -> impl Bundle {
    (
        action,
        Button,
        Name::new(format!("journey-{label}")),
        Node {
            min_height: Val::Px(30.0),
            padding: UiRect::axes(Val::Px(7.0), Val::Px(5.0)),
            border: UiRect::all(Val::Px(1.0)),
            align_items: AlignItems::Center,
            ..default()
        },
        button_chrome(variant),
        children![(
            Text::new(label),
            UiButtonLabel,
            crate::ui::typography::body(13.0),
            TextColor(INK),
            Pickable::IGNORE
        )],
    )
}

fn divider() -> impl Bundle {
    (
        Node {
            width: Val::Percent(100.0),
            height: Val::Px(1.0),
            flex_shrink: 0.0,
            ..default()
        },
        BackgroundColor(PLATE_RULE_SOFT),
        Pickable::IGNORE,
    )
}
