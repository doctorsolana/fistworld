//! The bound-book window and navigation; page layouts live beside it.
use super::*;
use crate::ui::{
    foundation::{UiButtonLabel, UiButtonVariant, button_chrome},
    modal::{ModalLayout, spawn_modal},
    styles::{BRASS, BRASS_DARK, PARCHMENT, RADIUS, SIGN_WOOD},
};
use bevy::prelude::*;
const PANEL_SIZE: Vec2 = Vec2::new(1240.0, 820.0);

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
        || std::env::var("FISTFORCE_CAPTURE_TRADE").is_ok_and(|value| value == "1")
        || std::env::var("FISTFORCE_CAPTURE_MEDIEVAL_HUD")
            .is_ok_and(|value| matches!(value.as_str(), "exploration" | "combat"));
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
            border: UiRect::all(Val::Px(3.0)),
            border_radius: BorderRadius::all(Val::Px(RADIUS)),
            overflow: Overflow::clip(),
            ..default()
        },
        BackgroundColor(LIMEWASH),
        BorderColor::from(BRASS_DARK),
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
        super::layout::spawn_body(panel);
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
            BackgroundColor(SIGN_WOOD),
            BorderColor::from(BRASS),
        ))
        .with_children(|header| {
            header
                .spawn(Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(3.0),
                    ..default()
                })
                .with_children(|title| {
                    title.spawn((
                        Text::new("ENCYCLOPEDIA"),
                        crate::ui::typography::heading(23.0),
                        TextColor(PARCHMENT),
                    ));
                    title.spawn((
                        Text::new("People, places & the affairs of your realm"),
                        crate::ui::typography::body(13.0),
                        TextColor(BRASS),
                    ));
                });
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
                                border_radius: BorderRadius::all(Val::Px(RADIUS)),
                                ..default()
                            },
                            button_chrome(UiButtonVariant::Inverse),
                        ))
                        .with_child((
                            Text::new("X"),
                            UiButtonLabel,
                            crate::ui::typography::text(16.0),
                            TextColor(PARCHMENT),
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
                border_radius: BorderRadius::all(Val::Px(RADIUS)),
                ..default()
            },
            button_chrome(UiButtonVariant::Ribbon),
        ))
        .with_children(|button| {
            button.spawn((
                Text::new(tab.label()),
                UiButtonLabel,
                crate::ui::typography::heading(14.0),
                TextColor(PARCHMENT),
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
            BackgroundColor(SIGN_WOOD),
            BorderColor::from(BRASS),
        ))
        .with_children(|footer| {
            footer.spawn((
                Text::new("Chronicles of the realm"),
                crate::ui::typography::body(13.5),
                TextColor(BRASS),
            ));
            footer.spawn((
                Text::new("N / ESC   Close book"),
                crate::ui::typography::text(13.5),
                TextColor(PARCHMENT),
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
