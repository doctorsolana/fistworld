//! Shared pointer-wheel scrolling for every Bevy UI surface.
//!
//! `Overflow::scroll_y()` only describes layout; Bevy still needs input to
//! update `ScrollPosition`. Keeping that input in one always-on playing-state
//! plugin means encyclopedia panes, history sheets, trade tables and compact
//! inspection cards all obey the same nested-scroll behaviour.

use bevy::input::mouse::{MouseScrollUnit, MouseWheel};
use bevy::picking::hover::HoverMap;
use bevy::prelude::*;

use crate::states::GameState;

pub struct UiScrollPlugin;

impl Plugin for UiScrollPlugin {
    fn build(&self, app: &mut App) {
        app.add_observer(on_scroll);
        app.add_systems(
            Update,
            send_scroll_events.run_if(in_state(GameState::Playing)),
        );
    }
}

const SCROLL_LINE_HEIGHT: f32 = 28.0;

/// Send wheel input into every UI hierarchy under the pointer.
///
/// The event starts at the hovered leaf and bubbles until a scrollable ancestor
/// consumes it. Nested panes therefore scroll independently and hand input to
/// their parent only after reaching an edge.
fn send_scroll_events(
    mut wheel: MessageReader<MouseWheel>,
    hover_map: Res<HoverMap>,
    mut commands: Commands,
) {
    for event in wheel.read() {
        let mut delta = -Vec2::new(event.x, event.y);
        if event.unit == MouseScrollUnit::Line {
            delta *= SCROLL_LINE_HEIGHT;
        }
        for pointer_map in hover_map.values() {
            for entity in pointer_map.keys().copied() {
                commands.trigger(UiScroll { entity, delta });
            }
        }
    }
}

/// Wheel delta in logical UI pixels, bubbling toward a scrollable ancestor.
#[derive(EntityEvent, Debug)]
#[entity_event(propagate, auto_propagate)]
struct UiScroll {
    entity: Entity,
    delta: Vec2,
}

fn on_scroll(
    mut event: On<UiScroll>,
    mut nodes: Query<(&mut ScrollPosition, &Node, &ComputedNode)>,
) {
    let Ok((mut position, node, computed)) = nodes.get_mut(event.entity) else {
        return;
    };
    let max_offset = (computed.content_size() - computed.size()) * computed.inverse_scale_factor();
    let delta = &mut event.delta;

    if node.overflow.x == OverflowAxis::Scroll && delta.x != 0.0 {
        let at_edge = if delta.x > 0.0 {
            position.x >= max_offset.x
        } else {
            position.x <= 0.0
        };
        if !at_edge {
            position.x = (position.x + delta.x).clamp(0.0, max_offset.x.max(0.0));
            delta.x = 0.0;
        }
    }
    if node.overflow.y == OverflowAxis::Scroll && delta.y != 0.0 {
        let at_edge = if delta.y > 0.0 {
            position.y >= max_offset.y
        } else {
            position.y <= 0.0
        };
        if !at_edge {
            position.y = (position.y + delta.y).clamp(0.0, max_offset.y.max(0.0));
            delta.y = 0.0;
        }
    }
    if *delta == Vec2::ZERO {
        event.propagate(false);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wheel_event_bubbles_to_any_scrollable_ui_viewport() {
        let mut app = App::new();
        app.add_observer(on_scroll);

        let viewport = app
            .world_mut()
            .spawn((
                Node {
                    overflow: Overflow::scroll_y(),
                    ..default()
                },
                ComputedNode {
                    size: Vec2::new(320.0, 100.0),
                    content_size: Vec2::new(320.0, 300.0),
                    inverse_scale_factor: 1.0,
                    ..default()
                },
            ))
            .id();
        let hovered_row = app
            .world_mut()
            .spawn((Node::default(), ChildOf(viewport)))
            .id();

        app.world_mut()
            .entity_mut(hovered_row)
            .trigger(|entity| UiScroll {
                entity,
                delta: Vec2::new(0.0, SCROLL_LINE_HEIGHT),
            });

        assert_eq!(
            app.world().get::<ScrollPosition>(viewport).unwrap().y,
            SCROLL_LINE_HEIGHT
        );
    }
}
