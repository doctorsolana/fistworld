//! Bell composed into the clock row, with an optional recent-notice drawer.
use super::*;
use crate::ui::{
    foundation::{UiButtonLabel, UiButtonVariant, button_chrome, surface_block},
    hud::chrome::{HudIcon, icon, wood_panel},
    motion::UiReveal,
    styles::{BRASS, CRIMSON, PARCHMENT, plate_shadow},
};

pub(in crate::ui::hud) fn notice_button() -> impl Bundle {
    (
        NoticeButton,
        JourneyAction::Toggle,
        Button,
        Name::new("journey-NOTICES"),
        Node {
            min_width: Val::Px(36.0),
            height: Val::Px(30.0),
            padding: UiRect::axes(Val::Px(6.0), Val::Px(3.0)),
            column_gap: Val::Px(3.0),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border: UiRect::left(Val::Px(1.0)),
            ..default()
        },
        button_chrome(UiButtonVariant::Ribbon),
        children![
            icon(HudIcon::Bell, 22.0),
            (
                JourneyText::Unread,
                Text::new(""),
                crate::ui::typography::heading(11.0),
                TextColor(PARCHMENT),
                TextLayout::justify(Justify::Center),
                Node {
                    display: Display::None,
                    min_width: Val::Px(15.0),
                    padding: UiRect::horizontal(Val::Px(3.0)),
                    border_radius: BorderRadius::all(Val::Px(8.0)),
                    ..default()
                },
                BackgroundColor(CRIMSON),
                Pickable::IGNORE,
            ),
        ],
    )
}

pub(in crate::ui::hud) fn view() -> impl Bundle {
    (
        JourneyDetails,
        Name::new("Notice details"),
        Node {
            display: Display::None,
            width: Val::Px(340.0),
            min_height: Val::Px(156.0),
            max_height: Val::Px(360.0),
            overflow: Overflow::scroll_y(),
            scrollbar_width: 6.0,
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(6.0),
            padding: UiRect::all(Val::Px(18.0)),
            ..default()
        },
        UiReveal::panel(),
        Interaction::default(),
        surface_block(),
        wood_panel(),
        plate_shadow(),
        children![
            (
                Node {
                    width: Val::Percent(100.0),
                    justify_content: JustifyContent::SpaceBetween,
                    align_items: AlignItems::Center,
                    padding: UiRect::bottom(Val::Px(6.0)),
                    ..default()
                },
                Pickable::IGNORE,
                children![
                    (
                        Text::new("RECENT NOTICES"),
                        crate::ui::typography::heading(13.0),
                        TextColor(PARCHMENT),
                        Pickable::IGNORE,
                    ),
                    (
                        JourneyAction::Clear,
                        Button,
                        Name::new("journey-CLEAR"),
                        Node {
                            min_height: Val::Px(24.0),
                            padding: UiRect::axes(Val::Px(6.0), Val::Px(4.0)),
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        button_chrome(UiButtonVariant::Ribbon),
                        children![(
                            Text::new("CLEAR"),
                            UiButtonLabel,
                            crate::ui::typography::heading(10.0),
                            TextColor(PARCHMENT),
                            Pickable::IGNORE,
                        )],
                    ),
                ],
            ),
            message(0, "No recent messages."),
            message(1, ""),
            message(2, ""),
        ],
    )
}

fn message(index: usize, text: &str) -> impl Bundle {
    (
        JourneyText::Message(index),
        Text::new(text),
        crate::ui::typography::body(14.0),
        TextColor(PARCHMENT),
        Node {
            display: if index == 0 {
                Display::Flex
            } else {
                Display::None
            },
            width: Val::Percent(100.0),
            flex_shrink: 0.0,
            padding: UiRect::vertical(Val::Px(8.0)),
            border: UiRect::top(Val::Px(1.0)),
            ..default()
        },
        BorderColor::all(BRASS.with_alpha(0.3)),
        Pickable::IGNORE,
    )
}
