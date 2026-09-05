//! Encyclopedia window layout.
//!
//! Master/detail rather than one wide table: a flat list carrying every field
//! is unreadable the moment the world holds two hundred villagers, and the
//! detail pane is where per-person depth (holdings, relations, grudges) grows
//! without touching the list.

use bevy::prelude::*;

use super::*;
use crate::ui::foundation::{button_chrome, UiButtonLabel, UiButtonVariant};
use crate::ui::modal::{spawn_modal, ModalLayout};
use crate::ui::styles::{EMBER, INK, INK_MUTED, PLATE_RULE, RADIUS};

const PANEL_SIZE: Vec2 = Vec2::new(1240.0, 820.0);
const LIST_WIDTH: f32 = 340.0;
// Civic and workplace records are intentionally deeper than the compact map
// card. The pane scrolls, so a real permit/market/staffing record should not be
// squeezed into six generic rows.
const PLACE_DETAIL_LINES: usize = 48;
/// Key-figure tiles across the top of a place page.
const PLACE_DETAIL_TILES: usize = 6;

pub(super) fn spawn_encyclopedia(
    mut commands: Commands,
    roots: Query<(), With<EncyclopediaRoot>>,
    capture: Option<Res<crate::capture::CaptureConfig>>,
) {
    if !roots.is_empty() {
        return;
    }
    // Capture runs photograph the world, not the UI — except when a capture
    // explicitly opens this window to verify it.
    let capture_opts_in = std::env::var("FISTFORCE_CAPTURE_ENCYCLOPEDIA")
        .is_ok_and(|value| !value.trim().is_empty())
        || std::env::var("FISTFORCE_CAPTURE_TRADE").is_ok_and(|value| value == "1");
    if capture.is_some() && !capture_opts_in {
        return;
    }

    let nodes = spawn_modal(
        &mut commands,
        EncyclopediaRoot,
        EncyclopediaBackdrop,
        EncyclopediaPanel,
        ModalLayout {
            panel_size: PANEL_SIZE,
            panel_padding: 0.0,
        },
    );

    // Own the panel's layout: the shared helper centres its children, and this
    // window wants a flush header / body / footer column.
    commands.entity(nodes.panel).insert((
        Node {
            width: Val::Vw(94.0),
            max_width: Val::Px(PANEL_SIZE.x),
            height: Val::Vh(90.0),
            max_height: Val::Px(PANEL_SIZE.y),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(10.0)),
            overflow: Overflow::clip(),
            ..default()
        },
        BackgroundColor(LIMEWASH),
        BorderColor::from(PLATE_RULE),
        // Lifts the window off the world instead of sitting flat on it.
        BoxShadow::new(
            Color::srgba(0.0, 0.0, 0.0, 0.55),
            Val::Px(0.0),
            Val::Px(10.0),
            Val::Px(2.0),
            Val::Px(28.0),
        ),
    ));

    commands.entity(nodes.panel).with_children(|panel| {
        spawn_header(panel);
        spawn_body(panel);
        spawn_footer(panel);
    });
}

fn spawn_header(panel: &mut ChildSpawnerCommands<'_>) {
    panel
        .spawn((
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                flex_shrink: 0.0,
                padding: UiRect::axes(Val::Px(20.0), Val::Px(14.0)),
                border: UiRect::bottom(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(LIMEWASH_HEADER),
            BorderColor::from(PLATE_RULE_SOFT),
        ))
        .with_children(|header| {
            header.spawn((
                Text::new("ENCYCLOPEDIA"),
                TextFont {
                    font_size: FontSize::Px(22.0),
                    ..default()
                },
                TextColor(EMBER),
            ));
            header
                .spawn(Node {
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(10.0),
                    ..default()
                })
                .with_children(|controls| {
                    controls
                        .spawn(Node {
                            flex_direction: FlexDirection::Row,
                            column_gap: Val::Px(6.0),
                            ..default()
                        })
                        .with_children(|tabs| {
                            for tab in EncyclopediaTab::ALL {
                                spawn_tab(tabs, tab);
                            }
                        });
                    controls
                        .spawn((
                            Button,
                            EncyclopediaCloseButton,
                            Node {
                                width: Val::Px(38.0),
                                height: Val::Px(38.0),
                                flex_shrink: 0.0,
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                border: UiRect::all(Val::Px(1.0)),
                                border_radius: BorderRadius::all(Val::Px(6.0)),
                                ..default()
                            },
                            button_chrome(UiButtonVariant::Ghost),
                        ))
                        .with_child((
                            Text::new("X"),
                            UiButtonLabel,
                            TextFont {
                                font_size: FontSize::Px(16.0),
                                ..default()
                            },
                            TextColor(INK_MUTED),
                        ));
                });
        });
}

fn spawn_tab(parent: &mut ChildSpawnerCommands<'_>, tab: EncyclopediaTab) {
    parent
        .spawn((
            Button,
            TabButton(tab),
            Node {
                padding: UiRect::axes(Val::Px(16.0), Val::Px(8.0)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(6.0)),
                ..default()
            },
            button_chrome(UiButtonVariant::Tab),
        ))
        .with_children(|button| {
            button.spawn((
                Text::new(tab.label()),
                UiButtonLabel,
                TextFont {
                    font_size: FontSize::Px(15.0),
                    ..default()
                },
                TextColor(INK_MUTED),
            ));
        });
}

fn spawn_filter(parent: &mut ChildSpawnerCommands<'_>, filter: PeopleFilter) {
    parent
        .spawn((
            Button,
            FilterButton(filter),
            Node {
                padding: UiRect::axes(Val::Px(12.0), Val::Px(5.0)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(11.0)),
                ..default()
            },
            button_chrome(UiButtonVariant::Tab),
        ))
        .with_children(|chip| {
            chip.spawn((
                Text::new(filter.label()),
                UiButtonLabel,
                TextFont {
                    font_size: FontSize::Px(13.5),
                    ..default()
                },
                TextColor(INK_MUTED),
            ));
        });
}

fn spawn_body(panel: &mut ChildSpawnerCommands<'_>) {
    panel
        .spawn(Node {
            flex_grow: 1.0,
            min_height: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            overflow: Overflow::clip(),
            ..default()
        })
        .with_children(|body| {
            spawn_people_tab(body);
            spawn_places_tab(body);
            super::retinue::spawn_retinue_tab(body);
            super::army::spawn_army_tab(body);
            super::companies::spawn_companies_tab(body);
            // Pages (ledgers, company controls) render here, full size, under
            // one BACK bar. See `EncyclopediaPageHost`.
            body.spawn((
                EncyclopediaPageHost,
                Node {
                    display: Display::None,
                    flex_grow: 1.0,
                    min_height: Val::Px(0.0),
                    flex_direction: FlexDirection::Column,
                    align_items: AlignItems::Stretch,
                    overflow: Overflow::clip(),
                    ..default()
                },
            ))
            .with_children(|host| {
                host.spawn((
                    Node {
                        width: Val::Percent(100.0),
                        flex_shrink: 0.0,
                        align_items: AlignItems::Center,
                        padding: UiRect::axes(Val::Px(20.0), Val::Px(8.0)),
                        border: UiRect::bottom(Val::Px(1.0)),
                        ..default()
                    },
                    BackgroundColor(LIMEWASH_HEADER),
                    BorderColor::from(PLATE_RULE_SOFT),
                ))
                .with_children(|bar| {
                    bar.spawn((
                        EncyclopediaPageBack,
                        Button,
                        Node {
                            min_height: Val::Px(36.0),
                            padding: UiRect::axes(Val::Px(14.0), Val::Px(8.0)),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            border: UiRect::all(Val::Px(1.0)),
                            border_radius: BorderRadius::all(Val::Px(RADIUS)),
                            ..default()
                        },
                        button_chrome(UiButtonVariant::Secondary),
                    ))
                    .with_child((
                        EncyclopediaPageBackLabel,
                        Text::new("BACK"),
                        UiButtonLabel,
                        TextFont {
                            font_size: FontSize::Px(14.0),
                            ..default()
                        },
                        TextColor(INK),
                        Pickable::IGNORE,
                    ));
                });
            });
        });
}

/// The PLACES tab: master list of settlements, detail on the right.
///
/// Mirrors the people tab's shape on purpose -- the two answer the same kind of
/// question, and a player who has learned one list should not have to learn a
/// second layout to read the other.
fn spawn_places_tab(body: &mut ChildSpawnerCommands<'_>) {
    use super::places::{
        PlaceBackToCompanyAction, PlaceBackToCompanyLabel, PlaceBusinessHistoryAction,
        PlaceCountText, PlaceDetailCard, PlaceDetailEmptyState, PlaceDetailLabel, PlaceDetailLine,
        PlaceDetailName, PlaceDetailSubtitle, PlaceDetailTile, PlaceDetailTileLabel,
        PlaceDetailTileValue, PlaceDetailValue, PlacesListContent, PlacesListViewport,
    };

    body.spawn((
        TabBody(EncyclopediaTab::Places),
        Node {
            display: Display::None,
            flex_grow: 1.0,
            min_height: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            overflow: Overflow::clip(),
            ..default()
        },
    ))
    .with_children(|tab| {
        // Header strip: just a count. No filters, because there is no
        // known/unknown distinction for places -- settlement summaries are the
        // map screen, so you know them all.
        tab.spawn((
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                flex_shrink: 0.0,
                padding: UiRect::axes(Val::Px(20.0), Val::Px(10.0)),
                border: UiRect::bottom(Val::Px(1.0)),
                ..default()
            },
            BorderColor::from(PLATE_RULE_SOFT),
        ))
        .with_children(|row| {
            row.spawn((
                crate::ui::history::WorldHistoryButton,
                Button,
                Node {
                    width: Val::Px(156.0),
                    height: Val::Px(36.0),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(RADIUS)),
                    ..default()
                },
                button_chrome(UiButtonVariant::Secondary),
            ))
            .with_child((
                Text::new("WORLD HISTORY"),
                UiButtonLabel,
                TextFont {
                    font_size: FontSize::Px(12.0),
                    ..default()
                },
                TextColor(INK),
                Pickable::IGNORE,
            ));
            row.spawn((
                PlaceCountText,
                Text::new(""),
                TextFont {
                    font_size: FontSize::Px(14.0),
                    ..default()
                },
                TextColor(INK_MUTED),
            ));
        });

        // Master / detail
        tab.spawn(Node {
            flex_grow: 1.0,
            min_height: Val::Px(0.0),
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Stretch,
            overflow: Overflow::clip(),
            ..default()
        })
        .with_children(|split| {
            // list
            split
                .spawn((
                    PlacesListViewport,
                    Node {
                        width: Val::Px(360.0),
                        min_height: Val::Px(0.0),
                        flex_shrink: 0.0,
                        flex_direction: FlexDirection::Column,
                        border: UiRect::right(Val::Px(1.0)),
                        overflow: Overflow::scroll_y(),
                        scrollbar_width: 8.0,
                        ..default()
                    },
                    BorderColor::from(PLATE_RULE_SOFT),
                ))
                .with_children(|list| {
                    list.spawn((
                        PlacesListContent,
                        Node {
                            flex_direction: FlexDirection::Column,
                            align_items: AlignItems::Stretch,
                            // A flex item is shrinkable by default. The list must
                            // instead grow to its rows' natural height so its
                            // parent has real overflow to scroll.
                            flex_shrink: 0.0,
                            padding: UiRect::axes(Val::Px(10.0), Val::Px(10.0)),
                            row_gap: Val::Px(2.0),
                            ..default()
                        },
                    ));
                });

            // detail
            split
                .spawn((
                    Node {
                        flex_grow: 1.0,
                        min_height: Val::Px(0.0),
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Stretch,
                        overflow: Overflow::scroll_y(),
                        scrollbar_width: 8.0,
                        ..default()
                    },
                    BackgroundColor(LIMEWASH_DETAIL),
                ))
                .with_children(|detail| {
                    detail.spawn((
                        PlaceDetailEmptyState,
                        Node {
                            flex_grow: 1.0,
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        children![(
                            Text::new("Select a place"),
                            TextFont {
                                font_size: FontSize::Px(15.0),
                                ..default()
                            },
                            TextColor(INK_MUTED),
                        )],
                    ));

                    detail
                        .spawn((
                            PlaceDetailCard,
                            Node {
                                display: Display::None,
                                flex_direction: FlexDirection::Column,
                                align_items: AlignItems::Stretch,
                                padding: UiRect::all(Val::Px(24.0)),
                                ..default()
                            },
                        ))
                        .with_children(|card| {
                            card.spawn((
                                PlaceDetailName,
                                Text::new(""),
                                TextFont {
                                    font_size: FontSize::Px(26.0),
                                    ..default()
                                },
                                TextColor(INK),
                            ));
                            card.spawn((
                                PlaceDetailSubtitle,
                                Text::new(""),
                                TextFont {
                                    font_size: FontSize::Px(14.0),
                                    ..default()
                                },
                                TextColor(EMBER),
                                Node {
                                    margin: UiRect::bottom(Val::Px(18.0)),
                                    ..default()
                                },
                            ));
                            card.spawn((
                                super::places::PlaceActionsRow,
                                Node {
                                    align_self: AlignSelf::FlexEnd,
                                    flex_direction: FlexDirection::Row,
                                    flex_wrap: FlexWrap::Wrap,
                                    justify_content: JustifyContent::FlexEnd,
                                    column_gap: Val::Px(7.0),
                                    row_gap: Val::Px(7.0),
                                    margin: UiRect::bottom(Val::Px(10.0)),
                                    ..default()
                                },
                            ))
                            .with_children(|actions| {
                                actions
                                    .spawn((
                                        PlaceBackToCompanyAction,
                                        Button,
                                        Node {
                                            display: Display::None,
                                            height: Val::Px(30.0),
                                            padding: UiRect::horizontal(Val::Px(10.0)),
                                            justify_content: JustifyContent::Center,
                                            align_items: AlignItems::Center,
                                            border: UiRect::all(Val::Px(1.0)),
                                            border_radius: BorderRadius::all(Val::Px(RADIUS)),
                                            ..default()
                                        },
                                        button_chrome(UiButtonVariant::Secondary),
                                    ))
                                    .with_child((
                                        PlaceBackToCompanyLabel,
                                        Text::new("BACK TO COMPANY"),
                                        UiButtonLabel,
                                        TextFont {
                                            font_size: FontSize::Px(12.0),
                                            ..default()
                                        },
                                        TextColor(INK),
                                        Pickable::IGNORE,
                                    ));
                                actions
                                    .spawn((
                                        PlaceBusinessHistoryAction,
                                        Button,
                                        Node {
                                            display: Display::None,
                                            width: Val::Px(170.0),
                                            height: Val::Px(30.0),
                                            justify_content: JustifyContent::Center,
                                            align_items: AlignItems::Center,
                                            border: UiRect::all(Val::Px(1.0)),
                                            border_radius: BorderRadius::all(Val::Px(RADIUS)),
                                            ..default()
                                        },
                                        button_chrome(UiButtonVariant::Secondary),
                                    ))
                                    .with_child((
                                        Text::new("BUSINESS HISTORY"),
                                        UiButtonLabel,
                                        TextFont {
                                            font_size: FontSize::Px(12.0),
                                            ..default()
                                        },
                                        TextColor(INK),
                                        Pickable::IGNORE,
                                    ));
                                actions
                                    .spawn((
                                        crate::ui::market::PlaceMarketAction,
                                        Button,
                                        Node {
                                            display: Display::None,
                                            width: Val::Px(116.0),
                                            height: Val::Px(30.0),
                                            justify_content: JustifyContent::Center,
                                            align_items: AlignItems::Center,
                                            border: UiRect::all(Val::Px(1.0)),
                                            border_radius: BorderRadius::all(Val::Px(RADIUS)),
                                            ..default()
                                        },
                                        button_chrome(UiButtonVariant::Primary),
                                    ))
                                    .with_child((
                                        Text::new("MARKET"),
                                        UiButtonLabel,
                                        TextFont {
                                            font_size: FontSize::Px(12.0),
                                            ..default()
                                        },
                                        TextColor(INK),
                                        Pickable::IGNORE,
                                    ));
                                actions
                                    .spawn((
                                        crate::ui::history::VillageHistoryButton,
                                        Button,
                                        Node {
                                            width: Val::Px(182.0),
                                            height: Val::Px(30.0),
                                            justify_content: JustifyContent::Center,
                                            align_items: AlignItems::Center,
                                            border: UiRect::all(Val::Px(1.0)),
                                            border_radius: BorderRadius::all(Val::Px(RADIUS)),
                                            ..default()
                                        },
                                        button_chrome(UiButtonVariant::Secondary),
                                    ))
                                    .with_child((
                                        Text::new("SETTLEMENT HISTORY"),
                                        UiButtonLabel,
                                        TextFont {
                                            font_size: FontSize::Px(12.0),
                                            ..default()
                                        },
                                        TextColor(INK),
                                        Pickable::IGNORE,
                                    ));
                            });
                            card.spawn(Node {
                                width: Val::Percent(100.0),
                                flex_direction: FlexDirection::Row,
                                flex_wrap: FlexWrap::Wrap,
                                column_gap: Val::Px(10.0),
                                row_gap: Val::Px(10.0),
                                margin: UiRect::bottom(Val::Px(6.0)),
                                ..default()
                            })
                            .with_children(|tiles| {
                                for index in 0..PLACE_DETAIL_TILES {
                                    tiles.spawn((
                                        PlaceDetailTile(index),
                                        Node {
                                            display: Display::None,
                                            flex_grow: 1.0,
                                            // Three tiles per row: two clean rows of
                                            // six, never a lone tile wrapping alone.
                                            flex_basis: Val::Percent(30.0),
                                            flex_direction: FlexDirection::Column,
                                            row_gap: Val::Px(4.0),
                                            padding: UiRect::axes(Val::Px(14.0), Val::Px(10.0)),
                                            border: UiRect::all(Val::Px(1.0)),
                                            border_radius: BorderRadius::all(Val::Px(RADIUS)),
                                            ..default()
                                        },
                                        BackgroundColor(LIMEWASH),
                                        BorderColor::from(PLATE_RULE_SOFT),
                                        children![
                                            (
                                                PlaceDetailTileLabel(index),
                                                Text::new(""),
                                                TextFont {
                                                    font_size: FontSize::Px(11.5),
                                                    ..default()
                                                },
                                                TextColor(INK_MUTED),
                                            ),
                                            (
                                                PlaceDetailTileValue(index),
                                                Text::new(""),
                                                TextFont {
                                                    font_size: FontSize::Px(20.0),
                                                    ..default()
                                                },
                                                TextColor(INK),
                                            ),
                                        ],
                                    ));
                                }
                            });
                            card.spawn((
                                super::places::WorksiteAssignButton,
                                super::places::AssignHeroToWorksite(None),
                                Button,
                                Node {
                                    display: Display::None,
                                    height: Val::Px(40.0),
                                    align_items: AlignItems::Center,
                                    justify_content: JustifyContent::Center,
                                    margin: UiRect::bottom(Val::Px(6.0)),
                                    border: UiRect::all(Val::Px(1.0)),
                                    border_radius: BorderRadius::all(Val::Px(RADIUS)),
                                    ..default()
                                },
                                crate::ui::foundation::button_chrome(
                                    crate::ui::foundation::UiButtonVariant::Primary,
                                ),
                                children![(
                                    Text::new("SEND MY HERO TO BUILD THIS"),
                                    crate::ui::foundation::UiButtonLabel,
                                    TextFont {
                                        font_size: FontSize::Px(14.0),
                                        ..default()
                                    },
                                    TextColor(INK),
                                    Pickable::IGNORE,
                                )],
                            ));
                            for index in 0..PLACE_DETAIL_LINES {
                                card.spawn((
                                    PlaceDetailLine(index),
                                    Node {
                                        flex_direction: FlexDirection::Row,
                                        align_items: AlignItems::FlexStart,
                                        justify_content: JustifyContent::SpaceBetween,
                                        padding: UiRect::vertical(Val::Px(9.0)),
                                        border: UiRect::bottom(Val::Px(1.0)),
                                        ..default()
                                    },
                                    BorderColor::from(PLATE_RULE_SOFT),
                                    children![
                                        (
                                            PlaceDetailLabel(index),
                                            Text::new(""),
                                            TextFont {
                                                font_size: FontSize::Px(13.5),
                                                ..default()
                                            },
                                            TextColor(INK_MUTED),
                                            // Never compress the label: a long
                                            // value would otherwise wrap "TO
                                            // ADVANCE" onto two lines and the
                                            // row would read as broken.
                                            Node {
                                                width: Val::Px(160.0),
                                                flex_shrink: 0.0,
                                                margin: UiRect::right(Val::Px(16.0)),
                                                ..default()
                                            },
                                        ),
                                        (
                                            PlaceDetailValue(index),
                                            Text::new(""),
                                            TextFont {
                                                font_size: FontSize::Px(16.0),
                                                ..default()
                                            },
                                            TextColor(INK),
                                            // The value wraps instead, right-aligned
                                            // so the column edge stays straight.
                                            TextLayout::justify(Justify::Right),
                                            Node {
                                                flex_grow: 1.0,
                                                flex_shrink: 1.0,
                                                ..default()
                                            },
                                        ),
                                    ],
                                ));
                            }
                        });
                });
        });
    });
}

fn spawn_people_tab(body: &mut ChildSpawnerCommands<'_>) {
    body.spawn((
        TabBody(EncyclopediaTab::People),
        Node {
            flex_grow: 1.0,
            min_height: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            overflow: Overflow::clip(),
            ..default()
        },
    ))
    .with_children(|tab| {
        // Filter row
        tab.spawn((
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                flex_shrink: 0.0,
                padding: UiRect::axes(Val::Px(20.0), Val::Px(10.0)),
                border: UiRect::bottom(Val::Px(1.0)),
                ..default()
            },
            BorderColor::from(PLATE_RULE_SOFT),
        ))
        .with_children(|row| {
            row.spawn(Node {
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(6.0),
                ..default()
            })
            .with_children(|filters| {
                for filter in [
                    PeopleFilter::All,
                    PeopleFilter::Known,
                    PeopleFilter::Unknown,
                ] {
                    spawn_filter(filters, filter);
                }
            });
            row.spawn((
                PeopleCountText,
                Text::new(""),
                TextFont {
                    font_size: FontSize::Px(14.0),
                    ..default()
                },
                TextColor(INK_MUTED),
            ));
        });

        // Master / detail
        tab.spawn(Node {
            flex_grow: 1.0,
            min_height: Val::Px(0.0),
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Stretch,
            overflow: Overflow::clip(),
            ..default()
        })
        .with_children(|split| {
            // Scrolling list
            split
                .spawn((
                    PeopleListViewport,
                    Node {
                        width: Val::Px(LIST_WIDTH),
                        min_height: Val::Px(0.0),
                        flex_shrink: 0.0,
                        flex_direction: FlexDirection::Column,
                        overflow: Overflow::scroll_y(),
                        scrollbar_width: 8.0,
                        border: UiRect::right(Val::Px(1.0)),
                        ..default()
                    },
                    BorderColor::from(PLATE_RULE_SOFT),
                ))
                .with_children(|viewport| {
                    viewport.spawn((
                        PeopleListContent,
                        Node {
                            flex_direction: FlexDirection::Column,
                            align_items: AlignItems::Stretch,
                            flex_shrink: 0.0,
                            padding: UiRect::all(Val::Px(8.0)),
                            row_gap: Val::Px(2.0),
                            ..default()
                        },
                    ));
                });

            // Detail pane
            split
                .spawn((
                    Node {
                        flex_grow: 1.0,
                        min_height: Val::Px(0.0),
                        flex_direction: FlexDirection::Column,
                        padding: UiRect::all(Val::Px(22.0)),
                        overflow: Overflow::scroll_y(),
                        scrollbar_width: 8.0,
                        ..default()
                    },
                    BackgroundColor(LIMEWASH_DETAIL),
                ))
                .with_children(|detail| {
                    spawn_detail_card(detail);
                    detail
                        .spawn((
                            DetailEmptyState,
                            Node {
                                flex_grow: 1.0,
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                ..default()
                            },
                        ))
                        .with_children(|empty| {
                            empty.spawn((
                                Text::new("Select a name"),
                                TextFont {
                                    font_size: FontSize::Px(16.0),
                                    ..default()
                                },
                                TextColor(INK_MUTED),
                            ));
                        });
                });
        });
    });
}

fn spawn_detail_card(detail: &mut ChildSpawnerCommands<'_>) {
    detail
        .spawn((
            DetailCard,
            Node {
                display: Display::None,
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Stretch,
                row_gap: Val::Px(4.0),
                ..default()
            },
        ))
        .with_children(|card| {
            card.spawn((
                DetailName,
                Text::new(""),
                TextFont {
                    font_size: FontSize::Px(30.0),
                    ..default()
                },
                TextColor(INK),
            ));
            card.spawn((
                DetailSubtitle,
                Text::new(""),
                TextFont {
                    font_size: FontSize::Px(15.0),
                    ..default()
                },
                TextColor(EMBER),
                Node {
                    margin: UiRect::bottom(Val::Px(18.0)),
                    ..default()
                },
            ));
            spawn_retinue_button(card);
            for field in DetailField::ALL {
                spawn_detail_stat(card, field);
            }
        });
}

fn spawn_detail_stat(card: &mut ChildSpawnerCommands<'_>, field: DetailField) {
    card.spawn((
        DetailRow(field),
        Node {
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Center,
            justify_content: JustifyContent::SpaceBetween,
            padding: UiRect::vertical(Val::Px(9.0)),
            border: UiRect::bottom(Val::Px(1.0)),
            ..default()
        },
        BorderColor::from(PLATE_RULE_SOFT),
    ))
    .with_children(|row| {
        row.spawn((
            Text::new(field.label()),
            TextFont {
                font_size: FontSize::Px(13.5),
                ..default()
            },
            TextColor(INK_MUTED),
        ));
        row.spawn((
            Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: Val::Px(8.0),
                ..default()
            },
            children![],
        ))
        .with_children(|value_row| {
            // God-only banner control, left of the value. Only the affiliation
            // row gets one: it is the only field the world lets you change.
            if field == DetailField::Affiliation {
                spawn_banner_button(value_row, "<", -1);
            }
            value_row.spawn((
                DetailStat(field),
                Text::new("-"),
                TextFont {
                    font_size: FontSize::Px(16.0),
                    ..default()
                },
                TextColor(INK),
            ));
            if field == DetailField::Affiliation {
                spawn_banner_button(value_row, ">", 1);
            }
        });
    });
}

/// God-only retinue toggle. Hidden for heroes and outside god mode.
fn spawn_retinue_button(card: &mut ChildSpawnerCommands<'_>) {
    card.spawn((
        RetinueButton,
        Button,
        Node {
            display: Display::None,
            align_self: AlignSelf::FlexStart,
            padding: UiRect::axes(Val::Px(12.0), Val::Px(6.0)),
            margin: UiRect::bottom(Val::Px(10.0)),
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(2.0)),
            ..default()
        },
        button_chrome(UiButtonVariant::Secondary),
    ))
    .with_children(|btn| {
        btn.spawn((
            RetinueLabel,
            UiButtonLabel,
            Text::new("CONSCRIPT"),
            TextFont {
                font_size: FontSize::Px(14.0),
                ..default()
            },
            TextColor(INK),
        ));
    });
}

/// One step of the banner cycle. Hidden unless the player has god capability.
fn spawn_banner_button(parent: &mut ChildSpawnerCommands<'_>, glyph: &str, step: i16) {
    parent
        .spawn((
            Button,
            BannerButton(step),
            Node {
                // Hidden until god capability; sized so showing it does not
                // reflow the row it sits in.
                display: Display::None,
                width: Val::Px(18.0),
                height: Val::Px(18.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(2.0)),
                ..default()
            },
            button_chrome(UiButtonVariant::Ghost),
        ))
        .with_children(|btn| {
            btn.spawn((
                Text::new(glyph),
                UiButtonLabel,
                TextFont {
                    font_size: FontSize::Px(14.0),
                    ..default()
                },
                TextColor(INK_MUTED),
            ));
        });
}

fn spawn_footer(panel: &mut ChildSpawnerCommands<'_>) {
    panel
        .spawn((
            Node {
                flex_direction: FlexDirection::Row,
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                flex_shrink: 0.0,
                padding: UiRect::axes(Val::Px(20.0), Val::Px(10.0)),
                border: UiRect::top(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(LIMEWASH_HEADER),
            BorderColor::from(PLATE_RULE_SOFT),
        ))
        .with_children(|footer| {
            footer.spawn((
                Text::new("N or ESC  close"),
                TextFont {
                    font_size: FontSize::Px(13.5),
                    ..default()
                },
                TextColor(INK_MUTED),
            ));
        });
}

pub(super) fn despawn_encyclopedia(
    mut commands: Commands,
    roots: Query<Entity, With<EncyclopediaRoot>>,
    mut people: ResMut<KnownPeople>,
) {
    let mut despawned = false;
    for root in roots.iter() {
        commands.entity(root).despawn();
        despawned = true;
    }
    // Re-request the roster next time it opens so it never shows stale levels.
    if despawned {
        people.requested = false;
    }
}

#[cfg(test)]
mod tests {
    use bevy::ecs::system::RunSystemOnce;
    use bevy::ui::FocusPolicy;

    use super::*;
    use crate::ui::encyclopedia::companies::{CompanyListViewport, CompanyPortfolioContent};
    use crate::ui::encyclopedia::places::{PlaceBusinessHistoryAction, PlaceDetailLine};

    #[test]
    fn encyclopedia_panel_captures_clicks_and_people_content_can_overflow() {
        let mut world = World::new();
        world.run_system_once(spawn_encyclopedia).unwrap();

        let mut panels =
            world.query_filtered::<(&FocusPolicy, &Pickable), With<EncyclopediaPanel>>();
        let (focus, pickable) = panels.single(&world).unwrap();
        assert_eq!(*focus, FocusPolicy::Block);
        assert!(pickable.should_block_lower);

        let mut contents = world.query_filtered::<&Node, With<PeopleListContent>>();
        assert_eq!(contents.single(&world).unwrap().flex_shrink, 0.0);

        let mut viewports = world.query_filtered::<&Node, With<PeopleListViewport>>();
        let viewport = viewports.single(&world).unwrap();
        assert_eq!(viewport.min_height, Val::Px(0.0));
        assert_eq!(viewport.overflow.y, OverflowAxis::Scroll);

        let mut company_viewports = world.query_filtered::<&Node, With<CompanyListViewport>>();
        let company_viewport = company_viewports.single(&world).unwrap();
        assert_eq!(company_viewport.min_height, Val::Px(0.0));
        assert_eq!(company_viewport.overflow.y, OverflowAxis::Scroll);
        let mut portfolio = world.query_filtered::<Entity, With<CompanyPortfolioContent>>();
        assert_eq!(portfolio.iter(&world).count(), 1);

        let mut business_history =
            world.query_filtered::<&Node, With<PlaceBusinessHistoryAction>>();
        assert_eq!(
            business_history.single(&world).unwrap().display,
            Display::None
        );

        let mut place_lines = world.query_filtered::<Entity, With<PlaceDetailLine>>();
        assert_eq!(place_lines.iter(&world).count(), PLACE_DETAIL_LINES);

        let mut close_buttons = world.query_filtered::<Entity, With<EncyclopediaCloseButton>>();
        assert_eq!(close_buttons.iter(&world).count(), 1);
    }
}
