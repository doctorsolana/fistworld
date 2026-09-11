//! Places keeps a tree directory beside a village/building ledger spread.
use super::super::places::*;
use super::super::*;
use crate::ui::foundation::{button_chrome, UiButtonLabel, UiButtonVariant};
use crate::ui::hud::chrome::{self, HudIcon};
use crate::ui::ledger::{self, LedgerIllustration};
use crate::ui::styles::PLATE_RULE_SOFT;
use bevy::prelude::*;

#[derive(Component)]
pub(in crate::ui::encyclopedia) struct PlaceIllustration;

#[derive(Component)]
pub(in crate::ui::encyclopedia) enum PlaceDetailIcon {
    Section(usize),
    Summary(usize),
}

pub(super) fn spawn_places_tab(body: &mut ChildSpawnerCommands<'_>) {
    body.spawn((
        TabBody(EncyclopediaTab::Places),
        Node {
            display: Display::None,
            flex_grow: 1.0,
            min_height: Val::Px(0.0),
            align_items: AlignItems::Stretch,
            overflow: Overflow::clip(),
            ..default()
        },
    ))
    .with_children(|split| {
        split
            .spawn((
                Node {
                    width: Val::Px(super::LIST_WIDTH),
                    flex_shrink: 0.0,
                    min_height: Val::Px(0.0),
                    flex_direction: FlexDirection::Column,
                    border: UiRect::right(Val::Px(1.0)),
                    ..default()
                },
                BorderColor::from(PLATE_RULE_SOFT),
                crate::ui::ledger::directory_paper(),
            ))
            .with_children(|directory| {
                directory.spawn(ledger::directory_gutter());
                directory
                    .spawn(Node {
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::SpaceBetween,
                        padding: UiRect::axes(Val::Px(16.0), Val::Px(18.0)),
                        flex_shrink: 0.0,
                        ..default()
                    })
                    .with_children(|toolbar| {
                        action(
                            toolbar,
                            crate::ui::history::WorldHistoryButton,
                            "WORLD HISTORY",
                            false,
                            UiButtonVariant::Primary,
                            crate::ui::hud::chrome::HudIcon::Book,
                        );
                        toolbar.spawn((PlaceCountText, ledger::body("", 16.0)));
                    });
                directory
                    .spawn((
                        PlacesListViewport,
                        Node {
                            flex_grow: 1.0,
                            min_height: Val::Px(0.0),
                            flex_direction: FlexDirection::Column,
                            overflow: Overflow::scroll_y(),
                            scrollbar_width: 7.0,
                            ..default()
                        },
                    ))
                    .with_child((
                        PlacesListContent,
                        Node {
                            flex_direction: FlexDirection::Column,
                            align_items: AlignItems::Stretch,
                            flex_shrink: 0.0,
                            padding: UiRect::axes(Val::Px(12.0), Val::Px(2.0)),
                            row_gap: Val::Px(3.0),
                            ..default()
                        },
                    ));
            });
        split
            .spawn((
                Node {
                    flex_grow: 1.0,
                    min_width: Val::Px(0.0),
                    min_height: Val::Px(0.0),
                    flex_direction: FlexDirection::Column,
                    overflow: Overflow::scroll_y(),
                    scrollbar_width: 7.0,
                    padding: UiRect::all(Val::Px(28.0)),
                    ..default()
                },
                ledger::paper(),
            ))
            .with_children(|detail| {
                detail
                    .spawn((
                        PlaceDetailEmptyState,
                        Node {
                            flex_grow: 1.0,
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                    ))
                    .with_child(ledger::body("Select a place to open its record", 20.0));
                detail
                    .spawn((
                        PlaceDetailCard,
                        Node {
                            display: Display::None,
                            flex_direction: FlexDirection::Column,
                            flex_shrink: 0.0,
                            align_items: AlignItems::Stretch,
                            row_gap: Val::Px(16.0),
                            ..default()
                        },
                    ))
                    .with_children(spawn_place_card);
            });
    });
}

fn spawn_place_card(card: &mut ChildSpawnerCommands<'_>) {
    card.spawn(Node {
        align_items: AlignItems::Center,
        column_gap: Val::Px(12.0),
        ..default()
    })
    .with_children(|header| {
        header
            .spawn(Node {
                flex_direction: FlexDirection::Column,
                flex_grow: 1.0,
                min_width: Val::Px(0.0),
                row_gap: Val::Px(8.0),
                ..default()
            })
            .with_children(|copy| {
                copy.spawn((PlaceDetailName, ledger::heading("", 42.0)));
                copy.spawn((PlaceDetailSubtitle, ledger::body("", 21.0)));
                copy.spawn((
                    PlaceActionsRow,
                    Node {
                        flex_wrap: FlexWrap::Wrap,
                        column_gap: Val::Px(8.0),
                        row_gap: Val::Px(8.0),
                        margin: UiRect::top(Val::Px(8.0)),
                        ..default()
                    },
                ))
                .with_children(|actions| {
                    actions
                        .spawn((
                            PlaceBackToCompanyAction,
                            Button,
                            action_node(true),
                            button_chrome(UiButtonVariant::Secondary),
                        ))
                        .with_child((
                            PlaceBackToCompanyLabel,
                            UiButtonLabel,
                            ledger::heading("BACK TO COMPANY", 12.0),
                        ));
                    action(
                        actions,
                        PlaceBusinessHistoryAction,
                        "BUSINESS HISTORY",
                        true,
                        UiButtonVariant::Secondary,
                        crate::ui::hud::chrome::HudIcon::Book,
                    );
                    action(
                        actions,
                        crate::ui::market::PlaceMarketAction,
                        "MARKET",
                        true,
                        UiButtonVariant::Primary,
                        crate::ui::hud::chrome::HudIcon::Scales,
                    );
                    action(
                        actions,
                        crate::ui::history::VillageHistoryButton,
                        "SETTLEMENT HISTORY",
                        false,
                        UiButtonVariant::Primary,
                        crate::ui::hud::chrome::HudIcon::Book,
                    );
                });
            });
        header
            .spawn((
                PlaceIllustration,
                ledger::illustration(LedgerIllustration::Village, Vec2::new(310.0, 180.0)),
            ))
            .insert(Node {
                width: Val::Percent(42.0),
                min_width: Val::Px(260.0),
                max_width: Val::Px(480.0),
                height: Val::Px(180.0),
                flex_shrink: 0.0,
                overflow: Overflow::clip(),
                border_radius: BorderRadius::all(Val::Px(3.0)),
                ..default()
            });
    });
    card.spawn(ledger::ornament_rule());
    card.spawn(Node {
        display: Display::Grid,
        grid_template_columns: vec![RepeatedGridTrack::flex(3, 1.0)],
        column_gap: Val::Px(18.0),
        row_gap: Val::Px(12.0),
        ..default()
    })
    .with_children(|tiles| {
        for index in 0..super::PLACE_DETAIL_TILES {
            tiles
                .spawn((
                    PlaceDetailTile(index),
                    Node {
                        display: Display::None,
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(12.0),
                        min_width: Val::Px(0.0),
                        row_gap: Val::Px(4.0),
                        padding: UiRect::axes(Val::Px(10.0), Val::Px(8.0)),
                        border: UiRect::bottom(Val::Px(1.0)),
                        ..default()
                    },
                    BorderColor::from(PLATE_RULE_SOFT),
                ))
                .with_children(|tile| {
                    tile.spawn((
                        PlaceDetailIcon::Summary(index),
                        chrome::icon(HudIcon::Book, 32.0),
                        Visibility::Inherited,
                    ))
                    .insert(ImageNode {
                        color: Color::srgb(0.48, 0.31, 0.15),
                        ..default()
                    });
                    tile.spawn(Node {
                        min_width: Val::Px(0.0),
                        flex_grow: 1.0,
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(4.0),
                        ..default()
                    })
                    .with_children(|copy| {
                        copy.spawn((PlaceDetailTileLabel(index), ledger::body("", 17.0)));
                        copy.spawn((PlaceDetailTileValue(index), ledger::heading("", 23.0)));
                    });
                });
        }
    });
    card.spawn((
        WorksiteAssignButton,
        AssignHeroToWorksite(None),
        Button,
        action_node(true),
        button_chrome(UiButtonVariant::Primary),
    ))
    .with_child((
        UiButtonLabel,
        ledger::heading("SEND MY HERO TO BUILD THIS", 14.0),
    ));
    card.spawn(ledger::ornament_rule());
    card.spawn(Node {
        display: Display::Grid,
        grid_template_columns: vec![RepeatedGridTrack::flex(2, 1.0)],
        column_gap: Val::Px(30.0),
        align_items: AlignItems::Start,
        ..default()
    })
    .with_children(|sections| {
        for index in 0..super::PLACE_DETAIL_LINES {
            sections
                .spawn((
                    PlaceDetailLine(index),
                    Node {
                        align_items: AlignItems::FlexStart,
                        justify_content: JustifyContent::SpaceBetween,
                        min_width: Val::Px(0.0),
                        column_gap: Val::Px(12.0),
                        padding: UiRect::vertical(Val::Px(7.0)),
                        border: UiRect::bottom(Val::Px(1.0)),
                        ..default()
                    },
                    BorderColor::from(PLATE_RULE_SOFT),
                ))
                .with_children(|line| {
                    line.spawn((
                        PlaceDetailIcon::Section(index),
                        chrome::icon(HudIcon::Book, 26.0),
                        Visibility::Hidden,
                    ))
                    .insert(Node {
                        position_type: PositionType::Absolute,
                        left: Val::Px(0.0),
                        top: Val::Px(17.0),
                        width: Val::Px(26.0),
                        height: Val::Px(26.0),
                        ..default()
                    })
                    .insert(ImageNode {
                        color: Color::srgb(0.48, 0.31, 0.15),
                        ..default()
                    });
                    line.spawn((
                        PlaceDetailLabel(index),
                        ledger::body("", 17.0),
                        Node {
                            min_width: Val::Px(0.0),
                            flex_basis: Val::Percent(34.0),
                            flex_shrink: 0.0,
                            ..default()
                        },
                    ));
                    line.spawn((
                        PlaceDetailValue(index),
                        ledger::body("", 17.0),
                        TextLayout::justify(Justify::Right),
                        Node {
                            flex_grow: 1.0,
                            min_width: Val::Px(0.0),
                            flex_basis: Val::Percent(66.0),
                            ..default()
                        },
                    ));
                });
        }
    });
}

fn action_node(hidden: bool) -> Node {
    Node {
        display: if hidden { Display::None } else { Display::Flex },
        min_height: Val::Px(46.0),
        padding: UiRect::horizontal(Val::Px(16.0)),
        column_gap: Val::Px(10.0),
        justify_content: JustifyContent::Center,
        align_items: AlignItems::Center,
        border: UiRect::all(Val::Px(1.0)),
        ..default()
    }
}

fn action(
    parent: &mut ChildSpawnerCommands<'_>,
    marker: impl Bundle,
    label: &str,
    hidden: bool,
    variant: UiButtonVariant,
    symbol: crate::ui::hud::chrome::HudIcon,
) {
    parent
        .spawn((marker, Button, action_node(hidden), button_chrome(variant)))
        .with_children(|button| {
            button.spawn(crate::ui::hud::chrome::icon(symbol, 22.0));
            button.spawn((UiButtonLabel, ledger::heading(label, 14.0)));
        });
}
