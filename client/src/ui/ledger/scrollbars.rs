//! Retained, non-interactive scroll indicators for the encyclopedia's viewports.
//! Wheel input and scroll ownership remain in `ui::scroll`. Tracks are siblings
//! of viewports, so scrolling content cannot move them or enlarge its extent.

use crate::ui::{
    encyclopedia::EncyclopediaPanel,
    styles::{BRASS, BRASS_DARK},
};
use bevy::{
    math::Affine2,
    prelude::*,
    ui::{CalculatedClip, UiGlobalTransform},
};

#[derive(Component)]
pub(super) struct ViewportScrollbar {
    track: Entity,
    thumb: Entity,
}

#[derive(Component)]
pub(super) struct ScrollTrack {
    viewport: Entity,
}

#[derive(Component)]
pub(super) struct ScrollDecoration;

/// Register before UiSystems::Prepare. Discover only changed node/hierarchy
/// inputs; the book has a bounded handful of actual scroll viewports.
pub(super) fn bind_scrollbars(
    mut commands: Commands,
    candidates: Query<
        (Entity, &Node, &ChildOf, Option<&ViewportScrollbar>),
        (
            Or<(Changed<Node>, Changed<ChildOf>)>,
            Without<ScrollDecoration>,
        ),
    >,
    hierarchy: Query<Option<&ChildOf>, With<Node>>,
    panels: Query<(), Or<(With<EncyclopediaPanel>, With<super::LedgerButtonScope>)>>,
    tracks: Query<(Entity, &ScrollTrack, &ChildOf)>,
) {
    for (entity, track, _) in &tracks {
        if hierarchy.get(track.viewport).is_err() {
            commands.entity(entity).despawn();
        }
    }
    for (viewport, node, parent, existing) in &candidates {
        if node.overflow.y != OverflowAxis::Scroll {
            if let Some(existing) = existing {
                commands.entity(existing.track).despawn();
                commands.entity(viewport).remove::<ViewportScrollbar>();
            }
            continue;
        }
        let mut ancestor = Some(viewport);
        let mut inside_book = false;
        while let Some(entity) = ancestor {
            if panels.contains(entity) {
                inside_book = true;
                break;
            }
            ancestor = hierarchy.get(entity).ok().flatten().map(ChildOf::parent);
        }
        if !inside_book {
            if let Some(existing) = existing {
                commands.entity(existing.track).despawn();
                commands.entity(viewport).remove::<ViewportScrollbar>();
            }
            continue;
        }
        if let Some(existing) = existing {
            if let Ok((_, _, current_parent)) = tracks.get(existing.track) {
                if current_parent.parent() != parent.parent() {
                    commands
                        .entity(existing.track)
                        .insert(ChildOf(parent.parent()));
                }
                continue;
            }
        }
        let track = commands
            .spawn((
                ScrollTrack { viewport },
                ScrollDecoration,
                Node {
                    position_type: PositionType::Absolute,
                    display: Display::None,
                    border_radius: BorderRadius::all(Val::Px(4.0)),
                    overflow: Overflow::clip(),
                    ..default()
                },
                BackgroundColor(BRASS_DARK.with_alpha(0.18)),
                Pickable::IGNORE,
                ZIndex(6),
                ChildOf(parent.parent()),
            ))
            .id();
        let thumb = commands
            .spawn((
                ScrollDecoration,
                Node {
                    position_type: PositionType::Absolute,
                    border_radius: BorderRadius::all(Val::Px(4.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    ..default()
                },
                BackgroundColor(BRASS),
                BorderColor::from(BRASS_DARK.with_alpha(0.70)),
                Pickable::IGNORE,
                ChildOf(track),
            ))
            .id();
        commands
            .entity(viewport)
            .insert(ViewportScrollbar { track, thumb });
    }
}

/// Register after UiSystems::PostLayout. Geometry comes from Bevy's resolved
/// viewport and scroll values; only changed values schedule another UI layout.
/// The native clipping inherited from the common parent also clips the track.
pub(super) fn sync_scrollbars(
    viewports: Query<
        (
            &ViewportScrollbar,
            &Node,
            &ComputedNode,
            &ScrollPosition,
            &UiGlobalTransform,
            &InheritedVisibility,
            &ChildOf,
            Option<&CalculatedClip>,
        ),
        Without<ScrollDecoration>,
    >,
    hierarchy: Query<
        (
            &Node,
            &ComputedNode,
            &UiGlobalTransform,
            &InheritedVisibility,
            Option<&ChildOf>,
        ),
        Without<ScrollDecoration>,
    >,
    mut decoration: Query<&mut Node, With<ScrollDecoration>>,
) {
    for (bar, node, computed, scroll, transform, inherited, parent, clip) in &viewports {
        let mut visible = node.overflow.y == OverflowAxis::Scroll
            && node.display != Display::None
            && inherited.get()
            && computed.size().min_element() > 0.0;
        let mut ancestor = Some(parent.parent());
        while visible {
            let Some(entity) = ancestor else { break };
            let Ok((node, _, _, inherited, parent)) = hierarchy.get(entity) else {
                break;
            };
            visible &= node.display != Display::None && inherited.get();
            ancestor = parent.map(ChildOf::parent);
        }
        let world_bounds = transformed_bounds(computed.size(), transform.affine());
        if let Some(clip) = clip {
            visible &= world_bounds.intersect(clip.clip).size().min_element() > 0.0;
        }
        let geometry = (|| {
            if !visible {
                return None;
            }
            let (_, parent_node, parent_transform, _, _) = hierarchy.get(parent.parent()).ok()?;
            let inverse = parent_transform.try_inverse()?;
            let scale = computed.inverse_scale_factor();
            if !scale.is_finite() || scale <= 0.0 {
                return None;
            }
            let mut bounds = transformed_bounds(computed.size(), inverse * transform.affine());
            // Absolute children are positioned inside the parent's border. The
            // measured viewport already includes any parent scroll translation;
            // restore that translation before native layout applies it once.
            let origin = parent_node.size() * 0.5 + parent_node.scroll_position
                - parent_node.border().min_inset;
            bounds.min = (bounds.min + origin) * scale;
            bounds.max = (bounds.max + origin) * scale;
            let border = computed.border();
            let width = node.scrollbar_width.clamp(4.0, 8.0);
            if bounds.width() <= (border.min_inset.x + border.max_inset.x) * scale + width + 2.0
                || bounds.height()
                    <= (border.min_inset.y + border.max_inset.y + computed.scrollbar_size.y) * scale
                        + 8.0
            {
                return None;
            }
            let track = Rect::from_corners(
                Vec2::new(
                    bounds.max.x - border.max_inset.x * scale - width - 1.0,
                    bounds.min.y + border.min_inset.y * scale + 4.0,
                ),
                Vec2::new(
                    bounds.max.x - border.max_inset.x * scale - 1.0,
                    bounds.max.y - (border.max_inset.y + computed.scrollbar_size.y) * scale - 4.0,
                ),
            );
            let (top, length) = thumb_geometry(
                computed.size().y * scale,
                computed.content_size().y * scale,
                scroll.0.y,
                track.height(),
            )?;
            Some((
                track,
                Rect::from_corners(Vec2::new(0.0, top), Vec2::new(width, top + length)),
            ))
        })();
        if let Some((track, thumb)) = geometry {
            if let Ok(node) = decoration.get_mut(bar.track) {
                set_rect(node, track);
            }
            if let Ok(node) = decoration.get_mut(bar.thumb) {
                set_rect(node, thumb);
            }
        } else if let Ok(mut node) = decoration.get_mut(bar.track) {
            if node.display != Display::None {
                node.display = Display::None;
            }
        }
    }
}

fn set_rect(mut node: Mut<'_, Node>, rect: Rect) {
    let (left, top, width, height) = (
        Val::Px(rect.min.x),
        Val::Px(rect.min.y),
        Val::Px(rect.width()),
        Val::Px(rect.height()),
    );
    // Do not dereference Mut<Node> mutably before this comparison: a no-op
    // write would relayout the book every frame, even with an idle scroll bar.
    if node.display == Display::Flex
        && node.left == left
        && node.top == top
        && node.width == width
        && node.height == height
    {
        return;
    }
    node.display = Display::Flex;
    node.left = left;
    node.top = top;
    node.width = width;
    node.height = height;
}

fn transformed_bounds(size: Vec2, transform: Affine2) -> Rect {
    let half = size * 0.5;
    let points = [
        -half,
        Vec2::new(half.x, -half.y),
        half,
        Vec2::new(-half.x, half.y),
    ]
    .map(|point| transform.transform_point2(point));
    Rect::from_corners(
        points.into_iter().reduce(Vec2::min).unwrap(),
        points.into_iter().reduce(Vec2::max).unwrap(),
    )
}

fn thumb_geometry(viewport: f32, content: f32, offset: f32, track: f32) -> Option<(f32, f32)> {
    if ![viewport, content, offset, track]
        .into_iter()
        .all(f32::is_finite)
        || viewport <= 0.0
        || track <= 0.0
        || content - viewport <= 0.5
    {
        return None;
    }
    let length = (track * viewport / content).max(24.0).min(track);
    let fraction = (offset / (content - viewport)).clamp(0.0, 1.0);
    Some(((track - length) * fraction, length))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thumb_matches_visible_fraction_and_reaches_both_scroll_limits() {
        assert_eq!(thumb_geometry(200.0, 800.0, 0.0, 192.0), Some((0.0, 48.0)));
        assert_eq!(
            thumb_geometry(200.0, 800.0, 300.0, 192.0),
            Some((72.0, 48.0))
        );
        assert_eq!(
            thumb_geometry(200.0, 800.0, 600.0, 192.0),
            Some((144.0, 48.0))
        );
        assert_eq!(
            thumb_geometry(200.0, 800.0, -20.0, 192.0),
            Some((0.0, 48.0))
        );
        assert_eq!(
            thumb_geometry(200.0, 800.0, 900.0, 192.0),
            Some((144.0, 48.0))
        );
    }

    #[test]
    fn huge_records_keep_a_visible_thumb_and_empty_or_hidden_viewports_have_none() {
        assert_eq!(
            thumb_geometry(200.0, 20000.0, 0.0, 192.0),
            Some((0.0, 24.0))
        );
        assert_eq!(thumb_geometry(12.0, 1000.0, 500.0, 4.0), Some((0.0, 4.0)));
        for values in [
            (200.0, 200.0, 0.0, 192.0),
            (200.0, 80.0, 0.0, 192.0),
            (0.0, 500.0, 0.0, 192.0),
            (200.0, 500.0, 0.0, 0.0),
            (200.0, 500.0, f32::NAN, 192.0),
        ] {
            assert_eq!(thumb_geometry(values.0, values.1, values.2, values.3), None);
        }
    }
}
