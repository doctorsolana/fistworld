use super::*;
use crate::ui::foundation::{
    button_chrome, type_scale as text_size, UiButtonLabel, UiButtonVariant,
};
use crate::ui::styles::{EMBER, INK, INK_MUTED, LIMEWASH_DETAIL, PLATE_RULE_SOFT};

fn label(parent: &mut ChildSpawnerCommands<'_>, slot: BoundText, size: f32, muted: bool) {
    parent.spawn((
        slot,
        Text::new(""),
        crate::ui::typography::text(size),
        TextColor(if muted { INK_MUTED } else { INK }),
        Pickable::IGNORE,
    ));
}
fn copy(parent: &mut ChildSpawnerCommands<'_>, value: &str, size: f32, color: Color) {
    parent.spawn((
        Text::new(value),
        crate::ui::typography::text(size),
        TextColor(color),
        Pickable::IGNORE,
    ));
}
fn button(parent: &mut ChildSpawnerCommands<'_>, action: ArmyAction, variant: UiButtonVariant) {
    parent
        .spawn((
            action,
            Button,
            Node {
                min_height: Val::Px(32.0),
                flex_shrink: 0.0,
                padding: UiRect::axes(Val::Px(10.0), Val::Px(6.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(3.0)),
                ..default()
            },
            button_chrome(variant),
        ))
        .with_children(|b| {
            b.spawn((
                BoundText::Button(action),
                UiButtonLabel,
                Text::new(""),
                crate::ui::typography::text(text_size::BODY),
                TextColor(INK),
                Pickable::IGNORE,
            ));
        });
}
fn row() -> Node {
    Node {
        column_gap: Val::Px(8.0),
        align_items: AlignItems::Center,
        flex_shrink: 0.0,
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
        .spawn(Node {
            flex_grow: 1.0,
            min_height: Val::Px(0.0),
            overflow: Overflow::scroll_y(),
            scrollbar_width: 8.0,
            ..column()
        })
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
            padding: UiRect::all(Val::Px(16.0)),
            ..column()
        },
    ))
    .with_children(|page| {
        page.spawn(Node {
            justify_content: JustifyContent::SpaceBetween,
            ..row()
        })
        .with_children(|head| {
            head.spawn(column()).with_children(|c| {
                copy(c, "YOUR ARMY", text_size::HEADING, EMBER);
                label(c, BoundText::Summary, text_size::VALUE, true);
            });
            button(head, ArmyAction::New, UiButtonVariant::Primary);
        });
        page.spawn(Node {
            flex_grow: 1.0,
            min_height: Val::Px(0.0),
            align_items: AlignItems::Stretch,
            column_gap: Val::Px(18.0),
            ..default()
        })
        .with_children(|body| {
            body.spawn(Node {
                width: Val::Px(210.0),
                flex_shrink: 0.0,
                border: UiRect::right(Val::Px(1.0)),
                padding: UiRect::right(Val::Px(12.0)),
                ..column()
            })
            .insert(BorderColor::from(PLATE_RULE_SOFT))
            .with_children(|side| {
                copy(side, "BATTALIONS", text_size::BODY, INK_MUTED);
                copy(side, "Choose one to manage", text_size::VALUE, INK_MUTED);
                list(side, ListKind::Battalions);
            });
            body.spawn(Node {
                flex_grow: 1.0,
                flex_basis: Val::Px(0.0),
                min_height: Val::Px(0.0),
                ..column()
            })
            .with_children(|detail| {
                detail
                    .spawn(Node {
                        justify_content: JustifyContent::SpaceBetween,
                        flex_wrap: FlexWrap::Wrap,
                        row_gap: Val::Px(8.0),
                        ..row()
                    })
                    .with_children(|head| {
                        head.spawn(column()).with_children(|title| {
                            label(title, BoundText::Title, text_size::TITLE, false);
                            label(title, BoundText::Capacity, text_size::VALUE, true);
                        });
                        head.spawn(row()).with_children(|actions| {
                            button(actions, ArmyAction::SelectMap, UiButtonVariant::Secondary);
                            button(actions, ArmyAction::Locate, UiButtonVariant::Ghost);
                        });
                    });
                detail
                    .spawn((
                        Node {
                            padding: UiRect::all(Val::Px(12.0)),
                            border: UiRect::all(Val::Px(1.0)),
                            ..column()
                        },
                        BackgroundColor(LIMEWASH_DETAIL),
                        BorderColor::from(PLATE_RULE_SOFT),
                    ))
                    .with_children(|policy| {
                        policy.spawn(row()).with_children(|r| {
                            copy(r, "STANDING STANCE", text_size::BODY, INK_MUTED);
                            button(
                                r,
                                ArmyAction::Stance(BattalionStance::Defensive),
                                UiButtonVariant::Secondary,
                            );
                            button(
                                r,
                                ArmyAction::Stance(BattalionStance::HoldLine),
                                UiButtonVariant::Secondary,
                            );
                        });
                        label(policy, BoundText::Policy, text_size::VALUE, false);
                        policy
                            .spawn(Node {
                                flex_wrap: FlexWrap::Wrap,
                                row_gap: Val::Px(6.0),
                                ..row()
                            })
                            .with_children(|r| {
                                for role in [SoldierRole::Infantry, SoldierRole::Archer] {
                                    button(r, ArmyAction::Role(role), UiButtonVariant::Secondary);
                                }
                                for fire in [FirePolicy::FireAtWill, FirePolicy::HoldFire] {
                                    button(r, ArmyAction::Fire(fire), UiButtonVariant::Secondary);
                                }
                                button(r, ArmyAction::Rearm, UiButtonVariant::Ghost);
                            });
                        label(policy, BoundText::Equipment, text_size::BODY, false);
                    });
                detail
                    .spawn(Node {
                        flex_grow: 1.0,
                        min_height: Val::Px(0.0),
                        align_items: AlignItems::Stretch,
                        column_gap: Val::Px(14.0),
                        ..default()
                    })
                    .with_children(|panes| {
                        for members in [true, false] {
                            panes
                                .spawn(Node {
                                    flex_grow: 1.0,
                                    flex_basis: Val::Px(0.0),
                                    min_height: Val::Px(0.0),
                                    ..column()
                                })
                                .with_children(|pane| {
                                    label(
                                        pane,
                                        if members {
                                            BoundText::Members
                                        } else {
                                            BoundText::Available
                                        },
                                        text_size::HEADING,
                                        false,
                                    );
                                    pane.spawn(Node {
                                        min_height: Val::Px(32.0),
                                        flex_wrap: FlexWrap::Wrap,
                                        ..row()
                                    })
                                    .with_children(|tools| {
                                        if members {
                                            copy(
                                                tools,
                                                "Remove keeps troops in your army",
                                                text_size::BODY,
                                                INK_MUTED,
                                            );
                                        } else {
                                            button(
                                                tools,
                                                ArmyAction::Source(false),
                                                UiButtonVariant::Tab,
                                            );
                                            button(
                                                tools,
                                                ArmyAction::Source(true),
                                                UiButtonVariant::Tab,
                                            );
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
                                        row_gap: Val::Px(6.0),
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
                    });
                detail
                    .spawn(Node {
                        justify_content: JustifyContent::SpaceBetween,
                        border: UiRect::top(Val::Px(1.0)),
                        padding: UiRect::top(Val::Px(8.0)),
                        flex_wrap: FlexWrap::Wrap,
                        ..row()
                    })
                    .insert(BorderColor::from(PLATE_RULE_SOFT))
                    .with_children(|foot| {
                        button(foot, ArmyAction::Fill, UiButtonVariant::Secondary);
                        foot.spawn(row()).with_children(|actions| {
                            button(actions, ArmyAction::Disband, UiButtonVariant::Danger);
                            button(actions, ArmyAction::CancelDisband, UiButtonVariant::Ghost);
                        });
                    });
                label(detail, BoundText::Notice, text_size::VALUE, true);
            });
        });
    });
}

pub(super) fn spawn_entries(parent: &mut ChildSpawnerCommands<'_>, kind: ListKind, ids: &[Entity]) {
    if ids.is_empty() {
        copy(
            parent,
            match kind {
                ListKind::Battalions => "Create a battalion to begin.",
                ListKind::Members => "No members yet. Add troops from the right.",
                ListKind::Available => "No troops in this group.",
            },
            text_size::VALUE,
            INK_MUTED,
        );
    }
    for &entity in ids {
        if kind == ListKind::Battalions {
            parent
                .spawn((
                    ArmyAction::Choose(entity),
                    Button,
                    Node {
                        padding: UiRect::all(Val::Px(10.0)),
                        flex_shrink: 0.0,
                        border: UiRect::all(Val::Px(1.0)),
                        ..column()
                    },
                    button_chrome(UiButtonVariant::Row),
                ))
                .with_children(|b| {
                    b.spawn((
                        BoundText::BattalionName(entity),
                        UiButtonLabel,
                        Text::new(""),
                        crate::ui::typography::text(text_size::VALUE),
                        TextColor(INK),
                        Pickable::IGNORE,
                    ));
                    label(
                        b,
                        BoundText::BattalionSummary(entity),
                        text_size::BODY,
                        true,
                    );
                });
        } else {
            parent
                .spawn((
                    Node {
                        padding: UiRect::axes(Val::Px(0.0), Val::Px(6.0)),
                        border: UiRect::bottom(Val::Px(1.0)),
                        ..row()
                    },
                    BorderColor::from(PLATE_RULE_SOFT),
                ))
                .with_children(|r| {
                    r.spawn(Node {
                        flex_grow: 1.0,
                        flex_basis: Val::Px(0.0),
                        ..column()
                    })
                    .with_children(|person| {
                        button(person, ArmyAction::Toggle(entity), UiButtonVariant::Row);
                        label(
                            person,
                            BoundText::SoldierInfo(entity),
                            text_size::BODY,
                            true,
                        );
                    });
                    button(
                        r,
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
