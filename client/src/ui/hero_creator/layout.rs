//! The character creator's stable book frame. Values bind in place in actions.rs.
use super::*;
use crate::ui::{
    foundation::{button_chrome, UiButtonLabel, UiButtonVariant},
    ledger,
    modal::{modal_backdrop_chrome, modal_root_chrome, ModalRoot},
    styles::{BRASS_DARK, INK_INVERSE_HEADING, PARCHMENT},
};
use bevy::ui::{FocusPolicy, RelativeCursorPosition};

pub(super) fn spawn_creator(
    mut commands: Commands,
    manifest: Res<crate::hero::HeroManifest>,
    art: Res<CreatorArtwork>,
    roots: Query<(), With<CreatorRoot>>,
) {
    if !roots.is_empty() {
        return;
    }
    commands
        .spawn((CreatorRoot, ModalRoot, modal_root_chrome()))
        .with_children(|root| {
            // The live portrait is rendered by the main 3D view. Leave its cutout
            // transparent; the diorama supplies the fullscreen dark backdrop.
            root.spawn((
                Name::new("creator-backdrop"),
                modal_backdrop_chrome(Color::NONE),
            ));
            root.spawn((
                Name::new("creator-panel"),
                CreatorPanel,
                ledger::LedgerButtonScope,
                crate::ui::motion::UiReveal::panel(),
                Node {
                    width: Val::Px(1240.0),
                    max_width: Val::Vw(88.0),
                    height: Val::Px(848.0),
                    max_height: Val::Vh(90.0),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::all(Val::Px(10.0)),
                    ..default()
                },
                FocusPolicy::Block,
                Pickable::default(),
                // The shared plate shadow fills its whole rectangle and would
                // darken the live 3D cutout. The diorama supplies its surround.
            ))
            .with_children(|panel| {
                spawn_header(panel);
                panel
                    .spawn(Node {
                        width: Val::Percent(100.0),
                        flex_grow: 1.0,
                        min_height: Val::Px(0.0),
                        ..default()
                    })
                    .with_children(|body| {
                        spawn_portrait(body);
                        body.spawn((
                            Node {
                                width: Val::Percent(54.0),
                                min_width: Val::Px(0.0),
                                height: Val::Percent(100.0),
                                flex_direction: FlexDirection::Column,
                                padding: UiRect::axes(Val::Px(48.0), Val::Px(26.0)),
                                row_gap: Val::Px(12.0),
                                ..default()
                            },
                            art.paper(),
                        ))
                        .with_children(|page| {
                            page.spawn((
                                ledger::heading("APPEARANCE", 36.0),
                                Node {
                                    align_self: AlignSelf::Center,
                                    ..default()
                                },
                            ));
                            page.spawn(ledger::ornament_rule());
                            page.spawn(Node {
                                flex_direction: FlexDirection::Column,
                                justify_content: JustifyContent::SpaceBetween,
                                align_self: AlignSelf::Center,
                                width: Val::Percent(100.0),
                                max_width: Val::Px(404.0),
                                flex_grow: 1.0,
                                min_height: Val::Px(0.0),
                                overflow: Overflow::scroll_y(),
                                row_gap: Val::Px(10.0),
                                padding: UiRect::axes(Val::Px(4.0), Val::Px(6.0)),
                                ..default()
                            })
                            .with_children(|rows| {
                                // Familiar appearance-first ordering without baking wardrobe
                                // capacity into the screen. New manifest slots remain reachable.
                                if let Some(index) =
                                    manifest.slots.iter().position(|s| s.name == "hair")
                                {
                                    spawn_slot_row(rows, "HAIR", CreatorRow::Slot(index), &art);
                                }
                                spawn_slot_row(rows, "SKIN", CreatorRow::Skin, &art);
                                for name in ["top", "bottom", "headgear"] {
                                    if let Some(index) =
                                        manifest.slots.iter().position(|s| s.name == name)
                                    {
                                        spawn_slot_row(
                                            rows,
                                            &name.to_uppercase(),
                                            CreatorRow::Slot(index),
                                            &art,
                                        );
                                    }
                                }
                                for (index, slot) in manifest.slots.iter().enumerate() {
                                    if !["hair", "top", "bottom", "headgear"]
                                        .contains(&slot.name.as_str())
                                    {
                                        spawn_slot_row(
                                            rows,
                                            &slot.name.to_uppercase(),
                                            CreatorRow::Slot(index),
                                            &art,
                                        );
                                    }
                                }
                            });
                            page.spawn(ledger::ornament_rule());
                        });
                    });
                spawn_footer(panel, &art);
                spawn_binding(panel, &art);
            });
        });
}

fn spawn_header(panel: &mut ChildSpawnerCommands<'_>) {
    panel
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                height: Val::Px(100.0),
                flex_shrink: 0.0,
                border: UiRect::bottom(Val::Px(2.0)),
                padding: UiRect::horizontal(Val::Px(28.0)),
                ..default()
            },
            ledger::wood(),
            BorderColor::all(BRASS_DARK),
        ))
        .with_children(|header| {
            header
                .spawn(ledger::heading("CREATE YOUR HERO", 42.0))
                .insert(TextColor(INK_INVERSE_HEADING));
        });
}

fn spawn_portrait(body: &mut ChildSpawnerCommands<'_>) {
    body.spawn((
        Name::new("creator-preview"),
        preview::CreatorPreviewPane,
        Node {
            width: Val::Percent(46.0),
            min_width: Val::Px(0.0),
            height: Val::Percent(100.0),
            ..default()
        },
        FocusPolicy::Block,
    ))
    .with_children(|left| {
        left.spawn((
            Node {
                position_type: PositionType::Absolute,
                left: Val::Px(24.0),
                right: Val::Px(24.0),
                bottom: Val::Px(26.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Center,
                row_gap: Val::Px(9.0),
                ..default()
            },
            Pickable::IGNORE,
        ))
        .with_children(|caption| {
            caption.spawn((
                Node {
                    width: Val::Percent(70.0),
                    ..default()
                },
                children![ledger::binding_ornament_rule()],
            ));
            caption
                .spawn(ledger::heading("YOUR HERO", 23.0))
                .insert(TextColor(INK_INVERSE_HEADING));
        });
    });
}

fn spawn_slot_row(
    parent: &mut ChildSpawnerCommands<'_>,
    label: &str,
    row: CreatorRow,
    art: &CreatorArtwork,
) {
    parent
        .spawn(Node {
            width: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Center,
            row_gap: Val::Px(5.0),
            flex_shrink: 0.0,
            ..default()
        })
        .with_children(|group| {
            group.spawn(ledger::heading(label, 17.0));
            group
                .spawn(Node {
                    width: Val::Percent(100.0),
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(12.0),
                    ..default()
                })
                .with_children(|line| {
                    spawn_arrow(line, row, -1, art);
                    line.spawn((
                        Node {
                            flex_grow: 1.0,
                            min_width: Val::Px(0.0),
                            height: Val::Px(52.0),
                            padding: UiRect::horizontal(Val::Px(6.0)),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            ..default()
                        },
                        art.inset(),
                        Pickable::IGNORE,
                    ))
                    .with_children(|value| {
                        value.spawn((
                            SlotValueText(row),
                            ledger::body_strong("-", 24.0),
                            TextLayout::justify(Justify::Center),
                        ));
                    });
                    spawn_arrow(line, row, 1, art);
                });
        });
}

fn spawn_arrow(
    parent: &mut ChildSpawnerCommands<'_>,
    row: CreatorRow,
    dir: i8,
    art: &CreatorArtwork,
) {
    let stem = match row {
        CreatorRow::Slot(index) => format!("slot-{index}"),
        CreatorRow::Skin => "skin".to_string(),
    };
    let direction = if dir < 0 { "previous" } else { "next" };
    parent
        .spawn((
            Button,
            Name::new(format!("creator-{stem}-{direction}")),
            ArrowButton { row, dir },
            crate::ui::sound::UiSoundHandled,
            RelativeCursorPosition::default(),
            Node {
                width: Val::Px(52.0),
                height: Val::Px(52.0),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                ..default()
            },
            button_chrome(UiButtonVariant::Secondary),
            ledger::LedgerButtonFace(art.brass()),
        ))
        .with_children(|button| {
            spawn_chevron(button, dir);
        });
}

fn spawn_chevron(button: &mut ChildSpawnerCommands<'_>, dir: i8) {
    button
        .spawn((
            Node {
                width: Val::Px(20.0),
                height: Val::Px(24.0),
                flex_shrink: 0.0,
                ..default()
            },
            Pickable::IGNORE,
        ))
        .with_children(|glyph| {
            // Two open strokes form a 10 × 20 chevron. The offset pale edge
            // gives the dark engraving relief without depending on a font.
            let length = 10.0 * std::f32::consts::SQRT_2;
            for (offset, color) in [
                (1.0, Color::srgba(0.91, 0.77, 0.47, 0.75)),
                (0.0, Color::srgb(0.16, 0.105, 0.04)),
            ] {
                for (center_y, angle) in [(7.0, -45.0), (17.0, 45.0)] {
                    glyph.spawn((
                        Node {
                            position_type: PositionType::Absolute,
                            left: Val::Px(10.0 - length * 0.5 + offset),
                            top: Val::Px(center_y - 1.0 + offset),
                            width: Val::Px(length),
                            height: Val::Px(2.0),
                            ..default()
                        },
                        UiTransform::from_rotation(Rot2::degrees(if dir < 0 {
                            angle
                        } else {
                            -angle
                        })),
                        BackgroundColor(color),
                        Pickable::IGNORE,
                    ));
                }
            }
        });
}

fn spawn_footer(panel: &mut ChildSpawnerCommands<'_>, art: &CreatorArtwork) {
    panel
        .spawn((
            Node {
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                height: Val::Px(88.0),
                flex_shrink: 0.0,
                column_gap: Val::Px(20.0),
                padding: UiRect::axes(Val::Px(30.0), Val::Px(12.0)),
                border: UiRect::top(Val::Px(2.0)),
                ..default()
            },
            ledger::wood(),
            BorderColor::all(BRASS_DARK),
        ))
        .with_children(|footer| {
            footer
                .spawn((
                    Name::new("creator-status"),
                    CreatorStatusText,
                    ledger::body("", 13.0),
                    Node {
                        flex_grow: 1.0,
                        min_width: Val::Px(0.0),
                        ..default()
                    },
                ))
                .insert(TextColor(INK_INVERSE_HEADING));
            spawn_begin_journey(footer, art);
        });
}

fn spawn_begin_journey(parent: &mut ChildSpawnerCommands<'_>, art: &CreatorArtwork) {
    parent
        .spawn((
            Button,
            Name::new("creator-BEGIN JOURNEY"),
            BeginJourneyButton,
            crate::ui::sound::UiSoundHandled,
            RelativeCursorPosition::default(),
            Node {
                width: Val::Px(356.0),
                height: Val::Px(64.0),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
                padding: UiRect::horizontal(Val::Px(12.0)),
                ..default()
            },
            button_chrome(UiButtonVariant::Primary),
            ledger::LedgerButtonFace(art.journey()),
        ))
        .with_children(|button| {
            for left in [true, false] {
                button.spawn((
                    Node {
                        position_type: PositionType::Absolute,
                        left: if left { Val::Px(18.0) } else { Val::Auto },
                        right: if left { Val::Auto } else { Val::Px(18.0) },
                        top: Val::Percent(50.0),
                        margin: UiRect::top(Val::Px(-4.5)),
                        width: Val::Px(9.0),
                        height: Val::Px(9.0),
                        border: UiRect::all(Val::Px(1.0)),
                        ..default()
                    },
                    UiTransform::from_rotation(Rot2::degrees(45.0)),
                    BorderColor::all(PARCHMENT),
                    Pickable::IGNORE,
                ));
            }
            button
                .spawn((
                    UiButtonLabel,
                    ledger::heading("BEGIN JOURNEY", 25.0),
                    TextShadow {
                        offset: Vec2::new(0.0, 1.5),
                        color: Color::srgb(0.12, 0.07, 0.025),
                    },
                ))
                .insert(TextColor(PARCHMENT));
        });
}

fn spawn_binding(panel: &mut ChildSpawnerCommands<'_>, art: &CreatorArtwork) {
    // Decoration projects outside the panel and must never intercept its inputs.
    panel.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(-12.0),
            right: Val::Px(-12.0),
            top: Val::Px(-12.0),
            bottom: Val::Px(-12.0),
            ..default()
        },
        art.frame(),
        ZIndex(19),
        Pickable::IGNORE,
    ));
}

pub(super) fn despawn_creator(mut commands: Commands, roots: Query<Entity, With<CreatorRoot>>) {
    for root in &roots {
        commands.entity(root).despawn();
    }
}
