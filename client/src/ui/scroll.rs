//! Shared pointer-wheel scrolling for every Bevy UI surface.
//!
//! `Overflow::scroll_y()` only describes layout; Bevy still needs input to
//! update `ScrollPosition`. Keeping that input in one always-on playing-state
//! plugin means encyclopedia panes, history sheets, trade tables and compact
//! inspection cards all obey the same nested-scroll behaviour.
//!
//! Scrolling is EASED: the wheel drives a per-viewport target and the actual
//! `ScrollPosition` chases it with an exponential blend. Discrete wheel lines
//! glide instead of stepping, trackpad input stays effectively 1:1 (the time
//! constant is well under a pixel-event interval's worth of perception), and
//! every scrollable surface shares the identical feel.

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
            (send_scroll_events, animate_smooth_scroll)
                .chain()
                .run_if(in_state(GameState::Playing)),
        );
    }
}

const SCROLL_LINE_HEIGHT: f32 = 28.0;
/// Time constant of the ease toward the wheel target. Small enough that a
/// trackpad still feels glued to the finger; large enough that a mouse-wheel
/// line reads as a glide rather than a jump.
const SCROLL_SMOOTHING_SECONDS: f32 = 0.09;
/// Once this close to the target, land exactly on it and stop dirtying
/// `ScrollPosition` (every write re-lays-out the scrolled subtree).
const SCROLL_SNAP_EPSILON: f32 = 0.5;

/// Eased wheel state for one scrollable viewport, inserted on first use.
///
/// `target` is where the wheel wants the viewport; `ScrollPosition` chases it.
/// `last_applied` detects writes from OTHER systems (capture fixtures,
/// rebuild-retained positions): when `ScrollPosition` is not where this
/// animator left it, the outside write is adopted as the new resting point
/// instead of being fought.
#[derive(Component, Debug)]
pub struct SmoothScroll {
    target: Vec2,
    last_applied: Vec2,
}

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
    mut commands: Commands,
    mut nodes: Query<(
        &ScrollPosition,
        &Node,
        &ComputedNode,
        Option<&mut SmoothScroll>,
    )>,
) {
    let Ok((position, node, computed, smooth)) = nodes.get_mut(event.entity) else {
        return;
    };
    let max_offset = ((computed.content_size() - computed.size())
        * computed.inverse_scale_factor())
    .max(Vec2::ZERO);
    // Edges are judged against the TARGET, not the eased position: holding
    // the wheel at an edge must hand the surplus to the parent pane rather
    // than repeatedly topping up an animation that is already pinned there.
    let current = smooth.as_ref().map_or(position.0, |smooth| smooth.target);
    let mut next = current;
    let target_entity = event.entity;
    let delta = &mut event.delta;

    if node.overflow.x == OverflowAxis::Scroll && delta.x != 0.0 {
        let at_edge = if delta.x > 0.0 {
            current.x >= max_offset.x
        } else {
            current.x <= 0.0
        };
        if !at_edge {
            next.x = (current.x + delta.x).clamp(0.0, max_offset.x);
            delta.x = 0.0;
        }
    }
    if node.overflow.y == OverflowAxis::Scroll && delta.y != 0.0 {
        let at_edge = if delta.y > 0.0 {
            current.y >= max_offset.y
        } else {
            current.y <= 0.0
        };
        if !at_edge {
            next.y = (current.y + delta.y).clamp(0.0, max_offset.y);
            delta.y = 0.0;
        }
    }
    if next != current {
        match smooth {
            Some(mut smooth) => smooth.target = next,
            None => {
                commands.entity(target_entity).insert(SmoothScroll {
                    target: next,
                    last_applied: position.0,
                });
            }
        }
    }
    if *delta == Vec2::ZERO {
        event.propagate(false);
    }
}

/// Chase each viewport's wheel target with a frame-rate-independent ease.
fn animate_smooth_scroll(
    time: Res<Time>,
    mut viewports: Query<(&mut ScrollPosition, &ComputedNode, &mut SmoothScroll)>,
) {
    let blend = 1.0 - (-time.delta_secs() / SCROLL_SMOOTHING_SECONDS).exp();
    for (mut position, computed, mut smooth) in viewports.iter_mut() {
        if position.0 != smooth.last_applied {
            // Someone else moved the viewport (fixture, retained rebuild).
            // Their word is final; ease from there on the next wheel input.
            smooth.target = position.0;
            smooth.last_applied = position.0;
            continue;
        }
        // Content shrinks (rows despawn, filters apply): keep the target
        // legal so the ease never chases a point past the new end.
        let max_offset = ((computed.content_size() - computed.size())
            * computed.inverse_scale_factor())
        .max(Vec2::ZERO);
        smooth.target = smooth.target.clamp(Vec2::ZERO, max_offset);
        if position.0 == smooth.target {
            continue;
        }
        let mut next = position.0.lerp(smooth.target, blend);
        if next.distance_squared(smooth.target) < SCROLL_SNAP_EPSILON * SCROLL_SNAP_EPSILON {
            next = smooth.target;
        }
        position.0 = next;
        smooth.last_applied = next;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scroll_viewport(app: &mut App) -> Entity {
        app.world_mut()
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
            .id()
    }

    #[test]
    fn wheel_event_bubbles_to_any_scrollable_ui_viewport() {
        let mut app = App::new();
        app.add_observer(on_scroll);

        let viewport = scroll_viewport(&mut app);
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
        app.world_mut().flush();

        // The wheel drives the eased TARGET; the position itself follows in
        // `animate_smooth_scroll`.
        assert_eq!(
            app.world().get::<SmoothScroll>(viewport).unwrap().target.y,
            SCROLL_LINE_HEIGHT
        );
    }

    #[test]
    fn eased_position_reaches_the_wheel_target_and_stops() {
        let mut app = App::new();
        app.add_observer(on_scroll);
        app.init_resource::<Time>();

        let viewport = scroll_viewport(&mut app);
        app.world_mut()
            .entity_mut(viewport)
            .trigger(|entity| UiScroll {
                entity,
                delta: Vec2::new(0.0, 120.0),
            });
        app.world_mut().flush();

        for _ in 0..120 {
            app.world_mut()
                .resource_mut::<Time>()
                .advance_by(std::time::Duration::from_millis(16));
            let _ = app
                .world_mut()
                .run_system_cached(animate_smooth_scroll)
                .unwrap();
        }
        assert_eq!(
            app.world().get::<ScrollPosition>(viewport).unwrap().y,
            120.0
        );
    }

    /// A wheel tick already pinned at the target edge must NOT be consumed:
    /// nested panes hand surplus scroll to their parents at the edges.
    #[test]
    fn a_wheel_tick_at_the_target_edge_bubbles_to_the_parent_pane() {
        let mut app = App::new();
        app.add_observer(on_scroll);

        let outer = scroll_viewport(&mut app);
        let inner = app
            .world_mut()
            .spawn((
                Node {
                    overflow: Overflow::scroll_y(),
                    ..default()
                },
                ComputedNode {
                    size: Vec2::new(320.0, 100.0),
                    content_size: Vec2::new(320.0, 150.0),
                    inverse_scale_factor: 1.0,
                    ..default()
                },
                ChildOf(outer),
            ))
            .id();

        // First tick: consumed by the inner pane (target hits its 50px max).
        app.world_mut()
            .entity_mut(inner)
            .trigger(|entity| UiScroll {
                entity,
                delta: Vec2::new(0.0, 60.0),
            });
        app.world_mut().flush();
        assert_eq!(
            app.world().get::<SmoothScroll>(inner).unwrap().target.y,
            50.0
        );
        // Second tick: the inner TARGET is at its edge even though the eased
        // position has not moved yet - the tick must reach the outer pane.
        app.world_mut()
            .entity_mut(inner)
            .trigger(|entity| UiScroll {
                entity,
                delta: Vec2::new(0.0, 60.0),
            });
        app.world_mut().flush();
        assert_eq!(
            app.world().get::<SmoothScroll>(outer).unwrap().target.y,
            60.0
        );
    }

    /// Capture fixtures and rebuild-retention write `ScrollPosition` directly;
    /// the animator must adopt such writes, never tug the pane back.
    #[test]
    fn an_external_scroll_write_is_adopted_not_fought() {
        let mut app = App::new();
        app.add_observer(on_scroll);
        app.init_resource::<Time>();

        let viewport = scroll_viewport(&mut app);
        app.world_mut()
            .entity_mut(viewport)
            .trigger(|entity| UiScroll {
                entity,
                delta: Vec2::new(0.0, 40.0),
            });
        app.world_mut().flush();
        // Some other system seizes the viewport.
        app.world_mut()
            .get_mut::<ScrollPosition>(viewport)
            .unwrap()
            .0 = Vec2::new(0.0, 175.0);

        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_millis(16));
        let _ = app
            .world_mut()
            .run_system_cached(animate_smooth_scroll)
            .unwrap();

        assert_eq!(
            app.world().get::<ScrollPosition>(viewport).unwrap().y,
            175.0
        );
        assert_eq!(
            app.world().get::<SmoothScroll>(viewport).unwrap().target.y,
            175.0
        );
    }

    /// Pin for the vendored bevy_ui fix (vendor/bevy_ui/src/focus.rs).
    ///
    /// Upstream 0.19.0 stopped the hit-test clip walk at the first ancestor
    /// that does not clip, so a list row separated from its scroll viewport by
    /// a plain content wrapper kept its FULL click rect when scrolled out of
    /// view — and stole clicks aimed at header buttons drawn over it ("clicked
    /// KNOWN TO YOU, selected the person behind it"). The walk must reach the
    /// viewport through any number of plain wrappers.
    #[test]
    fn a_row_scrolled_out_of_its_viewport_cannot_be_hit_through_a_plain_wrapper() {
        use bevy::ecs::system::SystemState;
        use bevy::math::Affine2;
        use bevy::ui::{clip_check_recursive, OverrideClip, UiGlobalTransform};

        let mut world = World::new();
        // Scroll viewport centred at the origin, 320x100, clipping Y.
        let viewport = world
            .spawn((
                Node {
                    overflow: Overflow::scroll_y(),
                    ..default()
                },
                ComputedNode {
                    size: Vec2::new(320.0, 100.0),
                    inverse_scale_factor: 1.0,
                    ..default()
                },
                UiGlobalTransform::from(Affine2::IDENTITY),
            ))
            .id();
        // Plain content wrapper: no clipping of its own — the exact shape of
        // every list's inner column node.
        let wrapper = world
            .spawn((
                Node::default(),
                ComputedNode {
                    size: Vec2::new(320.0, 400.0),
                    inverse_scale_factor: 1.0,
                    ..default()
                },
                UiGlobalTransform::from(Affine2::IDENTITY),
                ChildOf(viewport),
            ))
            .id();
        // A row whose rect has scrolled up above the viewport.
        let row = world
            .spawn((
                Node::default(),
                ComputedNode {
                    size: Vec2::new(320.0, 40.0),
                    inverse_scale_factor: 1.0,
                    ..default()
                },
                UiGlobalTransform::from(Affine2::from_translation(Vec2::new(0.0, -80.0))),
                ChildOf(wrapper),
            ))
            .id();

        let mut state: SystemState<(
            Query<(&ComputedNode, &UiGlobalTransform, &Node)>,
            Query<&ChildOf, Without<OverrideClip>>,
        )> = SystemState::new(&mut world);
        let (clipping, child_of) = state.get(&world).unwrap();

        // Inside the viewport the row remains clickable...
        assert!(clip_check_recursive(
            Vec2::new(0.0, 20.0),
            row,
            &clipping,
            &child_of
        ));
        // ...but the point above the viewport (where a header button lives)
        // must be clipped away. Upstream 0.19.0 returned true here.
        assert!(!clip_check_recursive(
            Vec2::new(0.0, -80.0),
            row,
            &clipping,
            &child_of
        ));
    }
}
