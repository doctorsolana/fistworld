//! One retained wood panel beside the army dock and above the selected-person card.

use std::collections::HashSet;

use bevy::prelude::*;
use shared::components::{ConstructionSite, Settlement, SettlementBuilding};

use crate::ui::{
    foundation::{layer, surface_block},
    hud::chrome::wood_panel,
    motion::UiReveal,
    styles::{BRASS, BRASS_DARK, CRIMSON, INK_INVERSE_MUTED, PARCHMENT, SIGN_WOOD},
    typography,
};
use crate::{input::InputState, selection::Selection};

use super::state::{ChatState, preview_alpha};

const HISTORY_ROW_GAP: f32 = 9.0;

#[derive(Component)]
pub(crate) struct ChatRoot;
#[derive(Component)]
pub(super) struct ChatDraftField;
#[derive(Component)]
pub(super) struct History;
#[derive(Component)]
pub(super) struct Recent;
#[derive(Component)]
pub(super) struct Composer;
#[derive(Component)]
pub(super) struct Title;
#[derive(Component)]
pub(super) struct Count;
#[derive(Component)]
pub(super) struct Hint;
#[derive(Component)]
pub(super) struct Feedback;
#[derive(Component)]
pub(super) struct EmptyHistory;
#[derive(Component)]
pub(super) struct DraftPrefix;
#[derive(Component)]
pub(super) struct DraftCaret;
#[derive(Component)]
pub(super) struct DraftSuffix;
#[derive(Component)]
pub(super) struct HistoryRow(u64);
#[derive(Component)]
pub(super) struct PreviewRow {
    index: usize,
    sequence: Option<u64>,
}
#[derive(Component)]
pub(super) struct PreviewBody;
#[derive(Component)]
pub(super) struct PreviewName;
#[derive(Component, Default)]
pub(super) struct FollowNewest(bool);

pub(super) fn spawn(mut commands: Commands) {
    commands
        .spawn((
            ChatRoot,
            Name::new("Chat panel"),
            Node {
                position_type: PositionType::Absolute,
                left: px(18),
                bottom: px(146),
                width: px(400),
                min_width: px(0),
                max_height: vh(60),
                overflow: Overflow::clip(),
                padding: UiRect::all(px(14)),
                display: Display::None,
                flex_direction: FlexDirection::Column,
                row_gap: px(8),
                ..default()
            },
            GlobalZIndex(layer::FLOATING_PANEL + 10),
            wood_panel(),
            Interaction::None,
            surface_block(),
            UiReveal::page(),
        ))
        .with_children(|panel| {
            panel
                .spawn((
                    Node {
                        width: percent(100),
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::SpaceBetween,
                        ..default()
                    },
                    Pickable::IGNORE,
                ))
                .with_children(|header| {
                    header.spawn((
                        Title,
                        Text::new("SERVER CHAT"),
                        typography::heading(12.0),
                        TextColor(BRASS),
                        Pickable::IGNORE,
                    ));
                    header.spawn((
                        Count,
                        Text::default(),
                        typography::reading(12.0),
                        TextColor(INK_INVERSE_MUTED),
                        Pickable::IGNORE,
                    ));
                });
            panel
                .spawn((
                    History,
                    Name::new("Chat scrollback"),
                    FollowNewest::default(),
                    Node {
                        display: Display::None,
                        width: percent(100),
                        min_width: px(0),
                        max_width: percent(100),
                        height: px(218),
                        min_height: px(72),
                        flex_shrink: 1.0,
                        flex_direction: FlexDirection::Column,
                        row_gap: px(HISTORY_ROW_GAP),
                        overflow: Overflow {
                            x: OverflowAxis::Clip,
                            y: OverflowAxis::Scroll,
                        },
                        ..default()
                    },
                    ScrollPosition::default(),
                    UiReveal::page(),
                    Interaction::None,
                    surface_block(),
                ))
                .with_children(|history| {
                    history.spawn((
                        EmptyHistory,
                        Text::new("No messages yet."),
                        typography::reading(13.0),
                        TextColor(INK_INVERSE_MUTED),
                        Pickable::IGNORE,
                    ));
                });
            panel
                .spawn((
                    Recent,
                    Node {
                        width: percent(100),
                        min_width: px(0),
                        max_width: percent(100),
                        flex_direction: FlexDirection::Column,
                        row_gap: px(6),
                        ..default()
                    },
                    Pickable::IGNORE,
                ))
                .with_children(|recent| {
                    for index in 0..3 {
                        recent
                            .spawn((
                                PreviewRow {
                                    index,
                                    sequence: None,
                                },
                                Node {
                                    display: Display::None,
                                    width: percent(100),
                                    min_width: px(0),
                                    max_width: percent(100),
                                    max_height: px(32),
                                    flex_shrink: 0.0,
                                    overflow: Overflow::clip(),
                                    ..default()
                                },
                                Pickable::IGNORE,
                            ))
                            .with_children(|row| {
                                // A node's overflow clips descendants, not its own Text glyphs.
                                // Keep the text below the bounded, two-line preview viewport.
                                row.spawn((
                                    PreviewName,
                                    Node {
                                        width: percent(100),
                                        min_width: px(0),
                                        max_width: percent(100),
                                        flex_shrink: 0.0,
                                        ..default()
                                    },
                                    Text::default(),
                                    TextLayout::linebreak(bevy::text::LineBreak::WordOrCharacter),
                                    bevy::text::LineHeight::Px(16.0),
                                    typography::reading_strong(13.0),
                                    TextColor(BRASS),
                                    Pickable::IGNORE,
                                ))
                                .with_children(|text| {
                                    text.spawn((
                                        PreviewBody,
                                        TextSpan::default(),
                                        bevy::text::LineHeight::Px(16.0),
                                        typography::reading(13.0),
                                        TextColor(PARCHMENT),
                                    ));
                                });
                            });
                    }
                });
            panel
                .spawn((
                    Composer,
                    Node {
                        display: Display::None,
                        width: percent(100),
                        min_width: px(0),
                        max_width: percent(100),
                        flex_direction: FlexDirection::Column,
                        row_gap: px(7),
                        border: UiRect::top(px(1)),
                        padding: UiRect::top(px(10)),
                        ..default()
                    },
                    BorderColor::all(BRASS_DARK),
                    UiReveal::page(),
                    Pickable::IGNORE,
                ))
                .with_children(|composer| {
                    composer
                        .spawn((
                            ChatDraftField,
                            Name::new("Chat draft"),
                            Node {
                                width: percent(100),
                                min_width: px(0),
                                max_width: percent(100),
                                height: px(36),
                                padding: UiRect::axes(px(8), px(7)),
                                border: UiRect::all(px(1)),
                                overflow: Overflow::clip(),
                                ..default()
                            },
                            BackgroundColor(SIGN_WOOD),
                            BorderColor::all(BRASS),
                            Interaction::None,
                            surface_block(),
                        ))
                        .with_children(|field| {
                            field
                                .spawn((
                                    DraftPrefix,
                                    Text::default(),
                                    typography::reading(14.0),
                                    TextColor(PARCHMENT),
                                    TextLayout::no_wrap(),
                                    Node {
                                        // Absolute text cannot expand its flex ancestors to
                                        // the intrinsic width of a long single-line draft.
                                        position_type: PositionType::Absolute,
                                        left: px(8),
                                        top: px(7),
                                        ..default()
                                    },
                                    UiTransform::default(),
                                    Pickable::IGNORE,
                                ))
                                .with_children(|text| {
                                    text.spawn((
                                        DraftCaret,
                                        TextSpan::new("|"),
                                        typography::reading(14.0),
                                        TextColor(BRASS),
                                    ));
                                    text.spawn((
                                        DraftSuffix,
                                        TextSpan::default(),
                                        typography::reading(14.0),
                                        TextColor(PARCHMENT),
                                    ));
                                });
                        });
                    composer.spawn((
                        Hint,
                        Text::new("Enter · send     Esc · keep draft"),
                        typography::reading(12.0),
                        TextColor(INK_INVERSE_MUTED),
                        Pickable::IGNORE,
                    ));
                });
            panel.spawn((
                Feedback,
                Node {
                    display: Display::None,
                    width: percent(100),
                    ..default()
                },
                Text::default(),
                typography::reading(12.0),
                TextColor(CRIMSON),
                Pickable::IGNORE,
            ));
        });
}

pub(super) fn despawn(mut commands: Commands, roots: Query<Entity, With<ChatRoot>>) {
    for entity in &roots {
        commands.entity(entity).despawn();
    }
}

pub(super) fn sync_visibility(
    mut state: ResMut<ChatState>,
    input: Res<InputState>,
    time: Res<Time<Real>>,
    selection: Option<Res<Selection>>,
    inspectors: Query<
        (),
        Or<(
            With<Settlement>,
            With<SettlementBuilding>,
            With<ConstructionSite>,
        )>,
    >,
    opening: Option<Res<crate::boat::OpeningCinematic>>,
    mut nodes: Query<
        (
            &mut Node,
            Option<&ChatRoot>,
            Option<&History>,
            Option<&Recent>,
            Option<&Composer>,
            Option<&Feedback>,
            Option<&EmptyHistory>,
        ),
        Or<(
            With<ChatRoot>,
            With<History>,
            With<Recent>,
            With<Composer>,
            With<Feedback>,
            With<EmptyHistory>,
        )>,
    >,
    mut viewports: Query<&mut FollowNewest>,
    mut last_open: Local<bool>,
) {
    let allowed = !input.ui_blocking() && !opening.as_ref().is_some_and(|o| o.is_active());
    let inspector = selection
        .as_ref()
        .and_then(|s| s.primary())
        .is_some_and(|e| inspectors.contains(e));
    let has_preview = state
        .messages
        .iter()
        .rev()
        .take(3)
        .any(|line| preview_alpha(line.received_at, time.elapsed_secs_f64()) > 0.0);
    let visible = allowed
        && (state.open || !inspector)
        && (state.open || has_preview || state.feedback.is_some());
    state.root_visible = visible;
    state.preview_visible = visible && !state.open && has_preview;
    for (mut node, root, history, recent, composer, feedback, empty) in &mut nodes {
        let show = if root.is_some() {
            visible
        } else if history.is_some() || composer.is_some() {
            state.open
        } else if recent.is_some() {
            !state.open && has_preview
        } else if feedback.is_some() {
            state.feedback.is_some()
        } else if empty.is_some() {
            state.messages.is_empty()
        } else {
            continue;
        };
        let display = if show { Display::Flex } else { Display::None };
        if node.display != display {
            node.display = display;
        }
    }
    if state.open && !*last_open {
        for mut follow in &mut viewports {
            follow.0 = true;
        }
    }
    *last_open = state.open;
}

pub(super) fn sync_history(
    mut commands: Commands,
    state: Res<ChatState>,
    rows: Query<(Entity, &HistoryRow, &ComputedNode)>,
    mut viewport: Query<
        (
            Entity,
            &ComputedNode,
            &mut ScrollPosition,
            &mut FollowNewest,
        ),
        With<History>,
    >,
    mut previous: Local<Option<(Entity, u64)>>,
) {
    let Ok((entity, computed, mut position, mut follow)) = viewport.single_mut() else {
        return;
    };
    if *previous == Some((entity, state.history_revision)) {
        return;
    }
    *previous = Some((entity, state.history_revision));
    let max = ((computed.content_size().y - computed.size().y) * computed.inverse_scale_factor())
        .max(0.0);
    if max - position.y < 12.0 {
        follow.0 = true;
    }
    let retained: HashSet<_> = state.messages.iter().map(|line| line.sequence).collect();
    let mut existing = HashSet::new();
    let mut removed_height = 0.0;
    for (entity, row, row_size) in &rows {
        if !retained.contains(&row.0) {
            removed_height += row_size.size().y * row_size.inverse_scale_factor() + HISTORY_ROW_GAP;
            commands.entity(entity).despawn();
        } else {
            existing.insert(row.0);
        }
    }
    // Preserve the screen position of retained text when the reader has scrolled up.
    if !follow.0 && removed_height > 0.0 {
        position.y = (position.y - removed_height).max(0.0);
    }
    commands.entity(entity).with_children(|history| {
        for line in &state.messages {
            if existing.contains(&line.sequence) {
                continue;
            }
            history
                .spawn((
                    HistoryRow(line.sequence),
                    Node {
                        width: percent(100),
                        min_width: px(0),
                        max_width: percent(100),
                        overflow: Overflow::clip(),
                        flex_shrink: 0.0,
                        ..default()
                    },
                    Text::new(format!("{}: ", line.sender)),
                    TextLayout::linebreak(bevy::text::LineBreak::WordOrCharacter),
                    typography::reading_strong(13.0),
                    TextColor(BRASS),
                    Pickable::IGNORE,
                ))
                .with_children(|row| {
                    row.spawn((
                        TextSpan::new(&line.text),
                        typography::reading(13.0),
                        TextColor(PARCHMENT),
                    ));
                });
        }
    });
}

pub(super) fn sync_preview(
    state: Res<ChatState>,
    time: Res<Time<Real>>,
    mut rows: Query<(&mut PreviewRow, &mut Node, &Children)>,
    mut names: Query<(&mut Text, &mut TextColor, &Children), With<PreviewName>>,
    mut bodies: Query<(&mut TextSpan, &mut TextColor), (With<PreviewBody>, Without<PreviewName>)>,
) {
    let offset = state.messages.len().saturating_sub(3);
    for (mut row, mut node, children) in &mut rows {
        let line = state.messages.get(offset + row.index);
        let alpha = line.map_or(0.0, |line| {
            preview_alpha(line.received_at, time.elapsed_secs_f64())
        });
        let display = if alpha > 0.0 {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
        let sequence = line.map(|line| line.sequence);
        for child in children.iter() {
            let Ok((mut text, mut color, spans)) = names.get_mut(child) else {
                continue;
            };
            if row.sequence != sequence {
                text.0 = line
                    .map(|line| format!("{}: ", line.sender))
                    .unwrap_or_default();
            }
            let tint = BRASS.with_alpha(alpha);
            if color.0 != tint {
                color.0 = tint;
            }
            for span in spans.iter() {
                if let Ok((mut body, mut color)) = bodies.get_mut(span) {
                    let value = line.map_or("", |line| line.text.as_str());
                    if body.0 != value {
                        body.0 = value.into();
                    }
                    let tint = PARCHMENT.with_alpha(alpha);
                    if color.0 != tint {
                        color.0 = tint;
                    }
                }
            }
        }
        if row.sequence != sequence {
            row.sequence = sequence;
        }
    }
}

pub(super) fn sync_draft(
    state: Res<ChatState>,
    time: Res<Time<Real>>,
    mut labels: Query<
        (
            Entity,
            &mut Text,
            Option<&Count>,
            Option<&Hint>,
            Option<&Feedback>,
            Option<&DraftPrefix>,
        ),
        Or<(With<Count>, With<Hint>, With<Feedback>, With<DraftPrefix>)>,
    >,
    mut suffix: Query<&mut TextSpan, With<DraftSuffix>>,
    mut caret: Query<&mut TextColor, With<DraftCaret>>,
    mut previous: Local<Option<(Entity, u64, usize, usize, bool, bool, Option<String>)>>,
) {
    let color = BRASS.with_alpha(if (time.elapsed_secs_f64() * 2.0) as u64 % 2 == 0 {
        1.0
    } else {
        0.0
    });
    for mut current in &mut caret {
        if current.0 != color {
            current.0 = color;
        }
    }
    let Some(root) = labels
        .iter()
        .find_map(|(entity, _, _, _, _, prefix)| prefix.map(|_| entity))
    else {
        return;
    };
    let unchanged = previous.as_ref().is_some_and(|old| {
        old.0 == root
            && old.1 == state.draft.revision
            && old.2 == state.draft.cursor
            && old.3 == state.draft.anchor
            && old.4 == state.open
            && old.5 == state.pending.is_some()
            && old.6 == state.feedback
    });
    if unchanged {
        return;
    }
    *previous = Some((
        root,
        state.draft.revision,
        state.draft.cursor,
        state.draft.anchor,
        state.open,
        state.pending.is_some(),
        state.feedback.clone(),
    ));
    let selected = state.draft.text[state.draft.selection()].chars().count();
    for (_, mut text, count, hint, feedback, prefix) in &mut labels {
        let value = if count.is_some() {
            if state.open {
                format!("{} / 280", state.draft.text.chars().count())
            } else {
                String::new()
            }
        } else if hint.is_some() {
            if state.pending.is_some() {
                "Sending… Your draft is kept until it arrives.".into()
            } else if selected > 0 {
                format!("{selected} selected     Enter · send     Esc · keep draft")
            } else {
                "Enter · send     Esc · keep draft".into()
            }
        } else if feedback.is_some() {
            state.feedback.clone().unwrap_or_default()
        } else if prefix.is_some() {
            state.draft.text[..state.draft.cursor].to_owned()
        } else {
            continue;
        };
        if text.0 != value {
            text.0 = value;
        }
    }
    for mut text in &mut suffix {
        let value = &state.draft.text[state.draft.cursor..];
        if text.0 != value {
            text.0 = value.into();
        }
    }
}

/// Follow only on opening or when the reader was already at the bottom.
pub(super) fn scroll_to_newest(
    mut viewports: Query<(&ComputedNode, &mut ScrollPosition, &mut FollowNewest), With<History>>,
) {
    for (computed, mut position, mut follow) in &mut viewports {
        if !follow.0 || computed.size().y <= 0.0 {
            continue;
        }
        let bottom = ((computed.content_size().y - computed.size().y)
            * computed.inverse_scale_factor())
        .max(0.0);
        position.y = bottom;
        follow.0 = false;
    }
}

/// Horizontal caret following uses measured glyphs, so long drafts retain readable type.
pub(super) fn fit_draft(
    fields: Query<&ComputedNode, With<ChatDraftField>>,
    mut text: Query<(&bevy::text::TextLayoutInfo, &ChildOf, &mut UiTransform), With<DraftPrefix>>,
) {
    for (layout, parent, mut transform) in &mut text {
        let Ok(field) = fields.get(parent.parent()) else {
            continue;
        };
        let available = field.content_box().width() * field.inverse_scale_factor();
        if available <= 0.0 {
            continue;
        }
        let Some(caret) = layout.glyphs.iter().find(|glyph| glyph.section_index == 1) else {
            continue;
        };
        let x = caret.position.x / layout.scale_factor;
        let offset = (x + 12.0 - available).max(0.0);
        let next = bevy::ui::Val2::px(-offset, 0.0);
        if transform.translation != next {
            transform.translation = next;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;
    use shared::protocol::ChatEvent;

    #[test]
    fn idle_chat_is_invisible_and_last_preview_fades_completely() {
        let mut app = App::new();
        app.init_resource::<ChatState>()
            .init_resource::<InputState>()
            .init_resource::<Time<Real>>()
            .add_systems(Startup, spawn)
            .add_systems(Update, sync_visibility);
        app.update();
        assert!(!app.world().resource::<ChatState>().root_visible);
        assert!(
            app.world_mut()
                .query_filtered::<&Node, With<ChatRoot>>()
                .iter(app.world())
                .all(|node| node.display == Display::None)
        );
        app.world_mut().resource_mut::<ChatState>().open = true;
        app.update();
        assert!(app.world().resource::<ChatState>().root_visible);
        app.world_mut().resource_mut::<ChatState>().open = false;
        app.world_mut().resource_mut::<ChatState>().receive(
            ChatEvent::Message {
                sequence: 1,
                sender: "Alice".into(),
                text: "hello".into(),
            },
            0.0,
            "Alice",
        );
        app.update();
        assert!(app.world().resource::<ChatState>().preview_visible);
        app.world_mut()
            .resource_mut::<Time<Real>>()
            .advance_by(std::time::Duration::from_secs(14));
        app.update();
        let state = app.world().resource::<ChatState>();
        assert!(!state.root_visible);
        assert!(!state.preview_visible);
        assert_eq!(state.messages.len(), 1);
    }

    #[test]
    fn evicting_old_history_keeps_a_scrolled_reader_anchored() {
        let mut app = App::new();
        app.init_resource::<ChatState>()
            .add_systems(Update, sync_history);
        let viewport = app
            .world_mut()
            .spawn((
                History,
                ComputedNode {
                    size: Vec2::new(372.0, 218.0),
                    content_size: Vec2::new(372.0, 2900.0),
                    ..default()
                },
                ScrollPosition(Vec2::new(0.0, 290.0)),
                FollowNewest(false),
            ))
            .id();
        let event = |sequence| ChatEvent::Message {
            sequence,
            sender: "Alice".into(),
            text: "hello".into(),
        };
        for sequence in 1..=100 {
            app.world_mut()
                .resource_mut::<ChatState>()
                .receive(event(sequence), 0.0, "Alice");
        }
        app.update();
        let rows: Vec<_> = app
            .world_mut()
            .query_filtered::<Entity, With<HistoryRow>>()
            .iter(app.world())
            .collect();
        for row in rows {
            app.world_mut().get_mut::<ComputedNode>(row).unwrap().size.y = 20.0;
        }
        app.world_mut()
            .resource_mut::<ChatState>()
            .receive(event(101), 0.0, "Alice");
        app.update();
        assert_eq!(
            app.world().get::<ScrollPosition>(viewport).unwrap().y,
            261.0
        );
        assert!(!app.world().get::<FollowNewest>(viewport).unwrap().0);
        assert_eq!(
            app.world_mut()
                .query_filtered::<Entity, With<HistoryRow>>()
                .iter(app.world())
                .count(),
            100
        );
    }

    #[test]
    fn retained_view_initializes_and_keeps_one_row_per_authoritative_message() {
        let mut app = App::new();
        app.init_resource::<ChatState>()
            .init_resource::<InputState>()
            .init_resource::<Time<Real>>()
            .add_systems(Startup, spawn)
            .add_systems(
                Update,
                (sync_visibility, sync_history, sync_draft, sync_preview).chain(),
            );
        let event = |sequence| ChatEvent::Message {
            sequence,
            sender: "Alice".into(),
            text: "The western road is safe.".into(),
        };
        app.world_mut()
            .resource_mut::<ChatState>()
            .receive(event(1), 0.0, "Alice");
        app.update();
        app.world_mut()
            .resource_mut::<ChatState>()
            .receive(event(1), 0.0, "Alice");
        app.update();
        assert_eq!(
            app.world_mut()
                .query_filtered::<Entity, With<HistoryRow>>()
                .iter(app.world())
                .count(),
            1
        );
        app.world_mut().resource_mut::<ChatState>().open = true;
        app.update();
        assert!(
            app.world_mut()
                .query_filtered::<&Node, With<Composer>>()
                .iter(app.world())
                .all(|node| node.display == Display::Flex)
        );
        let root = app
            .world_mut()
            .query_filtered::<Entity, With<ChatRoot>>()
            .single(app.world())
            .unwrap();
        app.world_mut().despawn(root);
        app.world_mut().resource_mut::<ChatState>().reset_session();
        app.world_mut()
            .resource_mut::<ChatState>()
            .receive(event(1), 0.0, "Alice");
        app.world_mut().run_system_once(spawn).unwrap();
        app.update();
        assert_eq!(
            app.world_mut()
                .query_filtered::<Entity, With<HistoryRow>>()
                .iter(app.world())
                .count(),
            1
        );
    }
}
