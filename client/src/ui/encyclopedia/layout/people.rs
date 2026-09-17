//! The People spread: portrait directory and a readable personal record.
use super::super::*;
use crate::ui::foundation::{UiButtonLabel, UiButtonVariant, button_chrome};
use crate::ui::hud::chrome::{self, HudIcon};
use crate::ui::ledger;
use crate::ui::styles::{PLATE_RULE_SOFT, STATUS_GOOD};
use bevy::prelude::*;

#[derive(Component)]
pub(in crate::ui::encyclopedia) struct DetailPortrait;
#[derive(Component)]
pub(in crate::ui::encyclopedia) struct DetailHealthFill;
#[derive(Component)]
pub(in crate::ui::encyclopedia) struct DetailAttribute(pub usize);
#[derive(Component)]
pub(in crate::ui::encyclopedia) struct DetailSection(pub DetailField);
#[derive(Component)]
pub(in crate::ui::encyclopedia) struct DetailPrivateNote;
#[derive(Component)]
pub(in crate::ui::encyclopedia) struct DetailInventoryGoods;

fn spawn_filter(parent: &mut ChildSpawnerCommands<'_>, filter: PeopleFilter) {
    parent
        .spawn((
            Button,
            FilterButton(filter),
            Node {
                min_width: Val::Px(74.0),
                height: Val::Px(34.0),
                padding: UiRect::horizontal(Val::Px(10.0)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            button_chrome(UiButtonVariant::Tab),
        ))
        .with_child((ledger::heading(filter.label(), 13.5), UiButtonLabel));
}

pub(super) fn spawn_people_tab(body: &mut ChildSpawnerCommands<'_>) {
    body.spawn((
        TabBody(EncyclopediaTab::People),
        Node {
            flex_grow: 1.0,
            min_height: Val::Px(0.0),
            overflow: Overflow::clip(),
            align_items: AlignItems::Stretch,
            ..default()
        },
    ))
    .with_children(|split| {
        split
            .spawn((
                Node {
                    width: Val::Px(super::LIST_WIDTH),
                    min_height: Val::Px(0.0),
                    flex_shrink: 0.0,
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
                        column_gap: Val::Px(7.0),
                        ..default()
                    })
                    .with_children(|toolbar| {
                        toolbar
                            .spawn(Node {
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
                        toolbar.spawn((PeopleCountText, ledger::body("", 16.0)));
                    });
                person_links::spawn_return_link(directory);
                search::spawn(directory, EncyclopediaTab::People, search::DIRECTORY_MARGIN);
                directory
                    .spawn((
                        PeopleListViewport,
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
                        PeopleListContent,
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
                    padding: UiRect::all(Val::Px(28.0)),
                    overflow: Overflow::scroll_y(),
                    scrollbar_width: 7.0,
                    ..default()
                },
                ledger::paper(),
            ))
            .with_children(|detail| {
                spawn_detail_card(detail);
                detail
                    .spawn((
                        DetailEmptyState,
                        Node {
                            flex_grow: 1.0,
                            align_items: AlignItems::Center,
                            justify_content: JustifyContent::Center,
                            ..default()
                        },
                    ))
                    .with_child(ledger::body("Select a person to open their record", 20.0));
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
                flex_shrink: 0.0,
                align_items: AlignItems::Stretch,
                row_gap: Val::Px(18.0),
                ..default()
            },
        ))
        .with_children(|card| {
            card.spawn(Node {
                align_items: AlignItems::Center,
                column_gap: Val::Px(26.0),
                ..default()
            })
            .with_children(|header| {
                header.spawn(ledger::portrait_frame(184.0)).with_child((
                    crate::ui::portraits::person(shared::components::PersonId::default(), 172.0),
                    DetailPortrait,
                ));
                header
                    .spawn(Node {
                        flex_grow: 1.0,
                        min_width: Val::Px(0.0),
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(8.0),
                        ..default()
                    })
                    .with_children(|copy| {
                        copy.spawn((DetailName, ledger::heading("", 42.0)));
                        copy.spawn((DetailSubtitle, ledger::body("", 22.0)));
                        copy.spawn((
                            DetailRow(DetailField::Health),
                            Node {
                                align_items: AlignItems::Center,
                                column_gap: Val::Px(14.0),
                                margin: UiRect::top(Val::Px(5.0)),
                                ..default()
                            },
                        ))
                        .with_children(|health| {
                            health
                                .spawn((Name::new("Health"), chrome::icon(HudIcon::Heart, 23.0)))
                                .insert(ImageNode {
                                    color: Color::srgb(0.48, 0.31, 0.15),
                                    ..default()
                                });
                            health
                                .spawn((
                                    Node {
                                        width: Val::Px(158.0),
                                        height: Val::Px(10.0),
                                        border: UiRect::all(Val::Px(1.0)),
                                        overflow: Overflow::clip(),
                                        ..default()
                                    },
                                    BackgroundColor(Color::srgba(0.23, 0.20, 0.13, 0.22)),
                                    BorderColor::from(PLATE_RULE_SOFT),
                                ))
                                .with_child((
                                    DetailHealthFill,
                                    Node {
                                        width: Val::Percent(100.0),
                                        height: Val::Percent(100.0),
                                        ..default()
                                    },
                                    BackgroundColor(STATUS_GOOD),
                                ));
                            health.spawn((DetailStat(DetailField::Health), ledger::body("", 18.0)));
                        });
                        spawn_retinue_button(copy);
                    });
            });
            card.spawn(ledger::ornament_rule());
            card.spawn(Node {
                column_gap: Val::Px(16.0),
                align_items: AlignItems::FlexStart,
                ..default()
            })
            .with_children(|spread| {
                spread.spawn(detail_column()).with_children(|column| {
                    section(column, "Attributes", DetailField::Attributes, |part| {
                        part.spawn(Node {
                            width: Val::Percent(100.0),
                            padding: UiRect::vertical(Val::Px(10.0)),
                            ..default()
                        })
                        .with_children(|attributes| {
                            for (index, label) in ["Physique", "Intelligence", "Charm"]
                                .into_iter()
                                .enumerate()
                            {
                                attributes
                                    .spawn((
                                        Node {
                                            flex_grow: 1.0,
                                            flex_basis: Val::Percent(33.3),
                                            flex_direction: FlexDirection::Column,
                                            align_items: AlignItems::Center,
                                            row_gap: Val::Px(3.0),
                                            border: UiRect::right(Val::Px(if index < 2 {
                                                1.0
                                            } else {
                                                0.0
                                            })),
                                            ..default()
                                        },
                                        BorderColor::from(PLATE_RULE_SOFT),
                                    ))
                                    .with_children(|attribute| {
                                        attribute.spawn((
                                            DetailAttribute(index),
                                            ledger::heading("—", 30.0),
                                        ));
                                        attribute.spawn(ledger::body(label, 17.0));
                                    });
                            }
                        });
                    });
                    section(column, "Possessions", DetailField::Wealth, |part| {
                        part.spawn((
                            DetailPrivateNote,
                            ledger::body("Personal belongings are private.", 17.0),
                            Node {
                                margin: UiRect::top(Val::Px(10.0)),
                                ..default()
                            },
                        ));
                        for field in [DetailField::Wealth, DetailField::Inventory] {
                            spawn_detail_stat(part, field);
                        }
                    });
                    section(
                        column,
                        "Allegiance & knowledge",
                        DetailField::Affiliation,
                        |part| {
                            for field in [
                                DetailField::Affiliation,
                                DetailField::Knowledge,
                                DetailField::Standing,
                                DetailField::Status,
                            ] {
                                spawn_detail_stat(part, field);
                            }
                        },
                    );
                });
                spread.spawn((
                    Node {
                        width: Val::Px(1.0),
                        flex_shrink: 0.0,
                        align_self: AlignSelf::Stretch,
                        margin: UiRect::vertical(Val::Px(4.0)),
                        ..default()
                    },
                    BackgroundColor(PLATE_RULE_SOFT),
                    Pickable::IGNORE,
                ));
                spread.spawn(detail_column()).with_children(|column| {
                    section(column, "Daily life", DetailField::Home, |part| {
                        for field in [
                            DetailField::Home,
                            DetailField::Work,
                            DetailField::Employment,
                            DetailField::Hunger,
                        ] {
                            spawn_detail_stat(part, field);
                        }
                    });
                    section(column, "Routine", DetailField::Activity, |part| {
                        for field in [DetailField::Activity, DetailField::Schedule] {
                            spawn_detail_stat(part, field);
                        }
                    });
                });
            });
        });
}

fn detail_column() -> Node {
    Node {
        flex_grow: 1.0,
        flex_basis: Val::Px(0.0),
        min_width: Val::Px(0.0),
        flex_direction: FlexDirection::Column,
        row_gap: Val::Px(26.0),
        ..default()
    }
}

fn section(
    parent: &mut ChildSpawnerCommands<'_>,
    title: &str,
    field: DetailField,
    contents: impl FnOnce(&mut ChildSpawnerCommands<'_>),
) {
    parent
        .spawn((
            DetailSection(field),
            Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Stretch,
                min_width: Val::Px(0.0),
                ..default()
            },
        ))
        .with_children(|part| {
            part.spawn(Node {
                align_items: AlignItems::Center,
                column_gap: Val::Px(9.0),
                margin: UiRect::bottom(Val::Px(6.0)),
                ..default()
            })
            .with_children(|header| {
                let icon = match field {
                    DetailField::Attributes => HudIcon::Person,
                    DetailField::Wealth => HudIcon::Purse,
                    DetailField::Affiliation => HudIcon::Crest,
                    DetailField::Home => HudIcon::Pin,
                    DetailField::Activity => HudIcon::Sun,
                    _ => HudIcon::Book,
                };
                header.spawn(chrome::icon(icon, 27.0)).insert(ImageNode {
                    color: Color::srgb(0.48, 0.31, 0.15),
                    ..default()
                });
                header.spawn(ledger::heading(title, 22.0));
            });
            part.spawn(ledger::rule());
            contents(part);
        });
}

fn spawn_detail_stat(parent: &mut ChildSpawnerCommands<'_>, field: DetailField) {
    parent
        .spawn((
            DetailRow(field),
            Node {
                align_items: AlignItems::FlexStart,
                column_gap: Val::Px(12.0),
                padding: UiRect::vertical(Val::Px(7.0)),
                border: UiRect::bottom(Val::Px(1.0)),
                ..default()
            },
            BorderColor::from(PLATE_RULE_SOFT),
        ))
        .with_children(|row| {
            let label = match field {
                DetailField::Home => "Home",
                DetailField::Work => "Work",
                DetailField::Employment => "Employment",
                DetailField::Hunger => "Food",
                DetailField::Wealth => "Money",
                DetailField::Inventory => "Inventory",
                DetailField::Activity => "Now",
                DetailField::Schedule => "Today",
                DetailField::Affiliation => "Affiliation",
                DetailField::Knowledge => "Knowledge",
                DetailField::Standing => "Standing",
                DetailField::Status => "Status",
                DetailField::Attributes => "Attributes",
                DetailField::Health => "Health",
            };
            row.spawn((
                ledger::body(label, 17.0),
                Node {
                    width: Val::Px(120.0),
                    flex_shrink: 0.0,
                    ..default()
                },
            ));
            row.spawn(Node {
                flex_grow: 1.0,
                min_width: Val::Px(0.0),
                column_gap: Val::Px(5.0),
                row_gap: Val::Px(7.0),
                flex_direction: if field == DetailField::Inventory {
                    FlexDirection::Column
                } else {
                    FlexDirection::Row
                },
                ..default()
            })
            .with_children(|value| {
                if field == DetailField::Affiliation {
                    spawn_banner_button(value, "‹", -1);
                }
                value.spawn((
                    DetailStat(field),
                    ledger::body("—", 17.0),
                    Node {
                        flex_grow: 1.0,
                        min_width: Val::Px(0.0),
                        ..default()
                    },
                ));
                if field == DetailField::Affiliation {
                    spawn_banner_button(value, "›", 1);
                }
                if field == DetailField::Inventory {
                    value.spawn((
                        DetailInventoryGoods,
                        Node {
                            flex_wrap: FlexWrap::Wrap,
                            column_gap: Val::Px(14.0),
                            row_gap: Val::Px(6.0),
                            ..default()
                        },
                    ));
                }
            });
        });
}

fn spawn_retinue_button(parent: &mut ChildSpawnerCommands<'_>) {
    parent
        .spawn((
            RetinueButton,
            Button,
            Node {
                display: Display::None,
                align_self: AlignSelf::FlexStart,
                height: Val::Px(32.0),
                padding: UiRect::horizontal(Val::Px(14.0)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            button_chrome(UiButtonVariant::Secondary),
        ))
        .with_child((
            RetinueLabel,
            UiButtonLabel,
            ledger::heading("CONSCRIPT", 13.0),
        ));
}

fn spawn_banner_button(parent: &mut ChildSpawnerCommands<'_>, glyph: &str, step: i16) {
    parent
        .spawn((
            Button,
            BannerButton(step),
            Node {
                display: Display::None,
                width: Val::Px(22.0),
                height: Val::Px(22.0),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            button_chrome(UiButtonVariant::Ghost),
        ))
        .with_child((UiButtonLabel, ledger::body(glyph, 16.0)));
}
