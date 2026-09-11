//! Battalion directory and two retained membership panes, on the open army ledger.
use super::*;
use crate::ui::foundation::{button_chrome, UiButtonLabel, UiButtonVariant};
use crate::ui::ledger;
use crate::ui::styles::{BRASS_DARK, INK, INK_MUTED, PARCHMENT, PLATE_RULE_SOFT};

#[derive(Component, Default)]
pub(crate) struct BattalionPortrait(pub Option<PersonId>);

#[derive(Component)]
pub(crate) struct TroopPortrait(pub Entity);

#[derive(Component)]
pub(crate) struct TroopCheckbox {
    pub soldier: Entity,
    pub checked: bool,
}

fn label(parent: &mut ChildSpawnerCommands<'_>, slot: BoundText, size: f32, muted: bool) {
    parent.spawn((
        slot,
        Text::new(""),
        if muted {
            ledger::reading(size)
        } else {
            ledger::reading_strong(size)
        },
        TextColor(if muted { INK_MUTED } else { INK }),
    ));
}
fn heading(parent: &mut ChildSpawnerCommands<'_>, value: &str, size: f32) {
    parent.spawn(ledger::heading(value, size));
}
fn button(parent: &mut ChildSpawnerCommands<'_>, action: ArmyAction, variant: UiButtonVariant) {
    parent
        .spawn((
            action,
            Button,
            Node {
                min_height: Val::Px(32.0),
                flex_shrink: 0.0,
                padding: UiRect::axes(Val::Px(9.0), Val::Px(5.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            button_chrome(variant),
        ))
        .with_child((
            BoundText::Button(action),
            UiButtonLabel,
            ledger::body_strong("", 13.0),
        ));
}
fn row() -> Node {
    Node {
        column_gap: Val::Px(7.0),
        align_items: AlignItems::Center,
        flex_shrink: 0.0,
        min_width: Val::Px(0.0),
        ..default()
    }
}
fn column() -> Node {
    Node {
        flex_direction: FlexDirection::Column,
        row_gap: Val::Px(8.0),
        min_width: Val::Px(0.0),
        ..default()
    }
}
fn list(parent: &mut ChildSpawnerCommands<'_>, kind: ListKind) {
    parent
        .spawn((
            Name::new(match kind {
                ListKind::Battalions => "Army battalion directory",
                ListKind::Members => "Army member viewport",
                ListKind::Available => "Army available viewport",
            }),
            Node {
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                overflow: Overflow::scroll_y(),
                scrollbar_width: 8.0,
                ..column()
            },
        ))
        .with_child((
            kind,
            ListSignature::default(),
            Node {
                flex_shrink: 0.0,
                ..column()
            },
        ));
}
pub(crate) fn spawn_army_tab(body: &mut ChildSpawnerCommands<'_>) {
    body.spawn((
        TabBody(EncyclopediaTab::Army),
        Node {
            flex_grow: 1.0,
            min_height: Val::Px(0.0),
            display: Display::None,
            overflow: Overflow::clip(),
            ..default()
        },
    ))
    .with_children(|page| {
        page.spawn((
            Node {
                width: Val::Percent(28.0),
                min_width: Val::Px(290.0),
                max_width: Val::Px(390.0),
                min_height: Val::Px(0.0),
                flex_shrink: 0.0,
                border: UiRect::right(Val::Px(1.0)),
                padding: UiRect::all(Val::Px(16.0)),
                ..column()
            },
            BorderColor::from(PLATE_RULE_SOFT),
            crate::ui::ledger::directory_paper(),
        ))
        .with_children(|side| {
            side.spawn(ledger::directory_gutter());
            side.spawn(Node {
                justify_content: JustifyContent::SpaceBetween,
                flex_wrap: FlexWrap::Wrap,
                row_gap: Val::Px(8.0),
                ..row()
            })
            .with_children(|head| {
                heading(head, "Your Army", 23.0);
                button(head, ArmyAction::New, UiButtonVariant::Primary);
            });
            label(side, BoundText::Summary, 14.0, false);
            side.spawn(ledger::ornament_rule());
            list(side, ListKind::Battalions);
        });
        page.spawn(Node {
            flex_grow: 1.0,
            flex_basis: Val::Px(0.0),
            min_height: Val::Px(0.0),
            padding: UiRect::all(Val::Px(16.0)),
            ..column()
        })
        .with_children(|detail| {
            detail
                .spawn(Node {
                    align_items: AlignItems::FlexStart,
                    column_gap: Val::Px(16.0),
                    ..row()
                })
                .with_children(|header| {
                    header
                        .spawn((
                            BattalionPortrait::default(),
                            Node {
                                width: Val::Px(138.0),
                                height: Val::Px(138.0),
                                flex_shrink: 0.0,
                                ..default()
                            },
                        ))
                        .with_child(ledger::person_portrait(PersonId::UNASSIGNED, 138.0));
                    header
                        .spawn(Node {
                            flex_grow: 1.0,
                            flex_basis: Val::Px(0.0),
                            ..column()
                        })
                        .with_children(|summary| {
                            summary
                                .spawn(Node {
                                    justify_content: JustifyContent::SpaceBetween,
                                    flex_wrap: FlexWrap::Wrap,
                                    row_gap: Val::Px(6.0),
                                    ..row()
                                })
                                .with_children(|head| {
                                    head.spawn((BoundText::Title, ledger::heading("", 30.0)));
                                    head.spawn(row()).with_children(|actions| {
                                        button(
                                            actions,
                                            ArmyAction::SelectMap,
                                            UiButtonVariant::Secondary,
                                        );
                                        button(
                                            actions,
                                            ArmyAction::Locate,
                                            UiButtonVariant::Secondary,
                                        );
                                    });
                                });
                            label(summary, BoundText::Capacity, 19.0, false);
                            summary.spawn(ledger::ornament_rule());
                            summary
                                .spawn(Node {
                                    column_gap: Val::Px(16.0),
                                    align_items: AlignItems::Stretch,
                                    ..row()
                                })
                                .with_children(|policies| {
                                    policies
                                        .spawn(Node {
                                            flex_grow: 1.0,
                                            flex_basis: Val::Px(0.0),
                                            ..column()
                                        })
                                        .with_children(|stance| {
                                            heading(stance, "Standing stance", 16.0);
                                            stance
                                                .spawn(Node {
                                                    flex_wrap: FlexWrap::Wrap,
                                                    row_gap: Val::Px(5.0),
                                                    ..row()
                                                })
                                                .with_children(|buttons| {
                                                    for policy in [
                                                        BattalionStance::Defensive,
                                                        BattalionStance::HoldLine,
                                                    ] {
                                                        button(
                                                            buttons,
                                                            ArmyAction::Stance(policy),
                                                            UiButtonVariant::Secondary,
                                                        );
                                                    }
                                                });
                                            label(stance, BoundText::Policy, 12.0, true);
                                        });
                                    policies
                                        .spawn((
                                            Node {
                                                flex_grow: 1.0,
                                                flex_basis: Val::Px(0.0),
                                                border: UiRect::left(Val::Px(1.0)),
                                                padding: UiRect::left(Val::Px(14.0)),
                                                ..column()
                                            },
                                            BorderColor::from(PLATE_RULE_SOFT),
                                        ))
                                        .with_children(|equipment| {
                                            heading(equipment, "Equipment", 16.0);
                                            equipment
                                                .spawn(Node {
                                                    flex_wrap: FlexWrap::Wrap,
                                                    row_gap: Val::Px(5.0),
                                                    ..row()
                                                })
                                                .with_children(|buttons| {
                                                    for role in
                                                        [SoldierRole::Infantry, SoldierRole::Archer]
                                                    {
                                                        button(
                                                            buttons,
                                                            ArmyAction::Role(role),
                                                            UiButtonVariant::Secondary,
                                                        );
                                                    }
                                                });
                                            heading(equipment, "Archer fire orders", 14.0);
                                            equipment
                                                .spawn(Node {
                                                    flex_wrap: FlexWrap::Wrap,
                                                    row_gap: Val::Px(5.0),
                                                    ..row()
                                                })
                                                .with_children(|buttons| {
                                                    for fire in [
                                                        FirePolicy::FireAtWill,
                                                        FirePolicy::HoldFire,
                                                    ] {
                                                        button(
                                                            buttons,
                                                            ArmyAction::Fire(fire),
                                                            UiButtonVariant::Secondary,
                                                        );
                                                    }
                                                    button(
                                                        buttons,
                                                        ArmyAction::Rearm,
                                                        UiButtonVariant::Ghost,
                                                    );
                                                });
                                        });
                                });
                        });
                });
            label(detail, BoundText::Equipment, 12.0, true);
            detail.spawn(ledger::ornament_rule());
            detail
                .spawn(Node {
                    flex_grow: 1.0,
                    min_height: Val::Px(0.0),
                    align_items: AlignItems::Stretch,
                    column_gap: Val::Px(12.0),
                    ..default()
                })
                .with_children(|panes| {
                    membership_pane(panes, true);
                    membership_pane(panes, false);
                });
            detail
                .spawn(Node {
                    justify_content: JustifyContent::SpaceBetween,
                    flex_wrap: FlexWrap::Wrap,
                    row_gap: Val::Px(5.0),
                    ..row()
                })
                .with_children(|foot| {
                    button(foot, ArmyAction::Fill, UiButtonVariant::Primary);
                    foot.spawn(row()).with_children(|actions| {
                        button(actions, ArmyAction::Disband, UiButtonVariant::Danger);
                        button(actions, ArmyAction::CancelDisband, UiButtonVariant::Ghost);
                    });
                });
            label(detail, BoundText::Notice, 12.0, true);
        });
    });
}

fn membership_pane(parent: &mut ChildSpawnerCommands<'_>, members: bool) {
    parent
        .spawn((
            Node {
                flex_grow: 1.0,
                flex_basis: Val::Px(0.0),
                min_height: Val::Px(0.0),
                padding: UiRect::all(Val::Px(9.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..column()
            },
            BorderColor::from(PLATE_RULE_SOFT),
        ))
        .with_children(|pane| {
            pane.spawn((
                if members {
                    BoundText::Members
                } else {
                    BoundText::Available
                },
                ledger::heading("", 17.0),
            ));
            pane.spawn(ledger::ornament_rule());
            pane.spawn(Node {
                min_height: Val::Px(32.0),
                flex_wrap: FlexWrap::Wrap,
                row_gap: Val::Px(5.0),
                ..row()
            })
            .with_children(|tools| {
                if members {
                    tools.spawn((
                        Text::new("Remove keeps troops in your army."),
                        ledger::reading(12.0),
                        TextColor(INK_MUTED),
                    ));
                } else {
                    button(tools, ArmyAction::Source(false), UiButtonVariant::Tab);
                    button(tools, ArmyAction::Source(true), UiButtonVariant::Tab);
                }
            });
            list(
                pane,
                if members {
                    ListKind::Members
                } else {
                    ListKind::Available
                },
            );
            pane.spawn(Node {
                flex_wrap: FlexWrap::Wrap,
                row_gap: Val::Px(5.0),
                ..row()
            })
            .with_children(|tools| {
                button(
                    tools,
                    if members {
                        ArmyAction::SelectMembers
                    } else {
                        ArmyAction::SelectAvailable
                    },
                    UiButtonVariant::Ghost,
                );
                button(
                    tools,
                    if members {
                        ArmyAction::RemoveChecked
                    } else {
                        ArmyAction::AddChecked
                    },
                    if members {
                        UiButtonVariant::Secondary
                    } else {
                        UiButtonVariant::Primary
                    },
                );
            });
        });
}

pub(super) fn spawn_entries(
    parent: &mut ChildSpawnerCommands<'_>,
    kind: ListKind,
    ids: &[Entity],
    roster: &ArmyRoster,
) {
    if ids.is_empty() {
        parent.spawn((
            Text::new(match kind {
                ListKind::Battalions => "Create a battalion to begin.",
                ListKind::Members => "No members yet. Add troops from the right.",
                ListKind::Available => "No troops in this group.",
            }),
            ledger::reading(14.0),
            TextColor(INK_MUTED),
        ));
    }
    for &entity in ids {
        if kind == ListKind::Battalions {
            parent
                .spawn((
                    ArmyAction::Choose(entity),
                    Button,
                    Node {
                        padding: UiRect::all(Val::Px(9.0)),
                        min_height: Val::Px(98.0),
                        border: UiRect::all(Val::Px(1.0)),
                        column_gap: Val::Px(12.0),
                        ..row()
                    },
                    button_chrome(UiButtonVariant::Row),
                ))
                .with_children(|entry| {
                    if let Some(battalion) = roster.battalions.iter().find(|b| b.entity == entity) {
                        entry.spawn(ledger::pennant(
                            &crate::battalion_bar::roman_numeral(battalion.ordinal),
                            Vec2::new(58.0, 80.0),
                        ));
                    }
                    entry
                        .spawn(Node {
                            flex_grow: 1.0,
                            flex_basis: Val::Px(0.0),
                            ..column()
                        })
                        .with_children(|copy| {
                            copy.spawn((
                                BoundText::BattalionName(entity),
                                UiButtonLabel,
                                ledger::heading("", 17.0),
                            ));
                            label(copy, BoundText::BattalionSummary(entity), 14.0, false);
                        });
                });
        } else {
            parent
                .spawn((
                    Node {
                        padding: UiRect::vertical(Val::Px(6.0)),
                        border: UiRect::bottom(Val::Px(1.0)),
                        ..row()
                    },
                    BorderColor::from(PLATE_RULE_SOFT),
                ))
                .with_children(|row| {
                    troop_selection(row, entity, roster);
                    button(
                        row,
                        if kind == ListKind::Members {
                            ArmyAction::Remove(entity)
                        } else {
                            ArmyAction::Add(entity)
                        },
                        UiButtonVariant::Ghost,
                    );
                });
        }
    }
}

fn troop_selection(parent: &mut ChildSpawnerCommands<'_>, entity: Entity, roster: &ArmyRoster) {
    parent
        .spawn((
            ArmyAction::Toggle(entity),
            Button,
            Node {
                flex_grow: 1.0,
                flex_basis: Val::Px(0.0),
                min_height: Val::Px(48.0),
                column_gap: Val::Px(8.0),
                align_items: AlignItems::Center,
                min_width: Val::Px(0.0),
                ..default()
            },
            button_chrome(UiButtonVariant::Row),
        ))
        .with_children(|choice| {
            choice
                .spawn((
                    TroopCheckbox {
                        soldier: entity,
                        checked: false,
                    },
                    Node {
                        width: Val::Px(18.0),
                        height: Val::Px(18.0),
                        flex_shrink: 0.0,
                        // Keep every edge visible below 1x UI scale.
                        border: UiRect::all(Val::Px(2.0)),
                        border_radius: BorderRadius::all(Val::Px(2.0)),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    BackgroundColor(Color::NONE),
                    BorderColor::all(BRASS_DARK),
                    Pickable::IGNORE,
                ))
                .with_child((
                    Node {
                        width: Val::Px(10.0),
                        height: Val::Px(6.0),
                        border: UiRect {
                            left: Val::Px(2.0),
                            bottom: Val::Px(2.0),
                            ..default()
                        },
                        ..default()
                    },
                    UiTransform::from_rotation(Rot2::degrees(-45.0)),
                    BorderColor::all(PARCHMENT),
                    Visibility::Hidden,
                    Pickable::IGNORE,
                ));
            if let Some(soldier) = roster.soldiers.get(&entity) {
                choice.spawn(ledger::portrait_frame(48.0)).with_child((
                    crate::ui::portraits::person(soldier.person_id.unwrap_or_default(), 36.0),
                    TroopPortrait(entity),
                ));
            }
            choice
                .spawn(Node {
                    flex_grow: 1.0,
                    flex_basis: Val::Px(0.0),
                    row_gap: Val::Px(3.0),
                    overflow: Overflow::clip_x(),
                    ..column()
                })
                .with_children(|person| {
                    person.spawn((
                        BoundText::Button(ArmyAction::Toggle(entity)),
                        UiButtonLabel,
                        ledger::body_strong("", 16.0),
                        TextLayout {
                            linebreak: LineBreak::NoWrap,
                            ..default()
                        },
                    ));
                    label(person, BoundText::SoldierInfo(entity), 12.0, true);
                });
        });
}
