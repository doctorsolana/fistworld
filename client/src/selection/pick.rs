//! Left click selects one; left DRAG box-selects many.
//!
//! Both gestures come off the same press, and which one it was is only knowable
//! at release — so selection resolves on release, and the box is drawn while the
//! button is down.
//!
//! The two gestures answer different questions, and so have different rules:
//!
//! - A **drag** means "I want to command these", so it only ever grabs units
//!   under your own banner. Dragging across a village full of other people's
//!   heroes and villagers picks up exactly your own and nothing else, instead of
//!   handing you a group where most of it silently refuses orders.
//! - A **click** means "what is that?", so it selects anything — someone else's
//!   hero, a villager — for inspection. It just cannot be ordered.

use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use shared::components::{CharacterActivity, CommandedBy, PlayerPosition};

use super::{
    can_command, pick_radius_at, ray_vs_vertical_segment, DragBox, Selectable, SelectableShape,
    Selection, BOX_MIN_PX,
};
use crate::camera_rts::{CursorRay, CursorTerrainHit};
use crate::hero::control::{placement_armed, WorldPlacementMode};
use crate::input::InputState;

/// Project a world point into WINDOW pixels, or `None` if it is off screen.
///
/// The conversion is not just `world_to_viewport`: the 3D camera renders to a
/// scaled offscreen image, so that call returns IMAGE pixels while the cursor —
/// and therefore the drag box — is in window pixels. Mapping back through the
/// viewport/window ratio is what keeps a box drawn around a unit actually
/// containing that unit. Getting this wrong gives a selection box that is
/// subtly offset only at non-1.0 render scale: near-invisible in testing and
/// infuriating in play.
fn world_to_window(
    camera: &Camera,
    camera_transform: &GlobalTransform,
    window_size: Vec2,
    world: Vec3,
) -> Option<Vec2> {
    let viewport_size = camera.logical_viewport_size()?;
    if viewport_size.x <= 0.0 || viewport_size.y <= 0.0 {
        return None;
    }
    // Errors here mean "outside the frustum", which rejects off-screen units for
    // free rather than needing a separate visibility test.
    let viewport_pos = camera.world_to_viewport(camera_transform, world).ok()?;
    Some(viewport_pos / viewport_size * window_size)
}

fn screen_rects_overlap(min_a: Vec2, max_a: Vec2, min_b: Vec2, max_b: Vec2) -> bool {
    min_a.x <= max_b.x && max_a.x >= min_b.x && min_a.y <= max_b.y && max_a.y >= min_b.y
}

/// Marquee selection follows the visible body rather than one projected point.
/// This makes a box that crosses a hero's legs or head count even when the
/// exact body midpoint happens to sit one pixel outside its edge.
fn projected_person_overlaps_rect(
    camera: &Camera,
    camera_transform: &GlobalTransform,
    window_size: Vec2,
    base: Vec3,
    radius: f32,
    height: f32,
    min: Vec2,
    max: Vec2,
) -> bool {
    let centre = base + Vec3::Y * height * 0.5;
    let Some(feet) = world_to_window(camera, camera_transform, window_size, base) else {
        return false;
    };
    let Some(head) = world_to_window(
        camera,
        camera_transform,
        window_size,
        base + Vec3::Y * height,
    ) else {
        return false;
    };
    let Some(centre_screen) = world_to_window(camera, camera_transform, window_size, centre) else {
        return false;
    };
    let projected_radius = [Vec3::X, Vec3::Z]
        .into_iter()
        .filter_map(|axis| {
            world_to_window(
                camera,
                camera_transform,
                window_size,
                centre + axis * radius,
            )
            .map(|edge| edge.distance(centre_screen))
        })
        .fold(4.0_f32, f32::max)
        .clamp(4.0, 18.0);
    let body_min = feet.min(head) - Vec2::splat(projected_radius);
    let body_max = feet.max(head) + Vec2::splat(projected_radius);
    screen_rects_overlap(body_min, body_max, min, max)
}

/// Ray intersection with a rotated box, returning distance along the ray.
fn ray_vs_oriented_box(
    ray_origin: Vec3,
    ray_dir: Vec3,
    center: Vec3,
    rotation: f32,
    half_extents: Vec3,
) -> Option<f32> {
    const EPSILON: f32 = 1.0e-6;
    let inverse = Quat::from_rotation_y(rotation).inverse();
    let local_origin = inverse * (ray_origin - center);
    let local_dir = inverse * ray_dir;
    let mut near = 0.0_f32;
    let mut far = f32::INFINITY;
    for (origin, direction, half) in [
        (local_origin.x, local_dir.x, half_extents.x),
        (local_origin.y, local_dir.y, half_extents.y),
        (local_origin.z, local_dir.z, half_extents.z),
    ] {
        if direction.abs() <= EPSILON {
            if origin < -half || origin > half {
                return None;
            }
            continue;
        }
        let mut first = (-half - origin) / direction;
        let mut second = (half - origin) / direction;
        if first > second {
            std::mem::swap(&mut first, &mut second);
        }
        near = near.max(first);
        far = far.min(second);
        if near > far {
            return None;
        }
    }
    if far < 0.0 {
        None
    } else if near >= 0.0 {
        Some(near)
    } else {
        Some(far)
    }
}

pub(super) fn selectable_ray_distance(
    selectable: &Selectable,
    base: Vec3,
    ray_origin: Vec3,
    ray_dir: Vec3,
) -> Option<f32> {
    match selectable.shape {
        SelectableShape::Person => {
            let (distance, gap) =
                ray_vs_vertical_segment(ray_origin, ray_dir, base, selectable.height)?;
            (gap <= pick_radius_at(selectable.radius, distance)).then_some(distance)
        }
        SelectableShape::Footprint {
            half_extents,
            centre_offset,
            rotation,
        } => {
            let centre_offset = shared::rotation::local_to_world_xz(centre_offset, rotation);
            let center = Vec3::new(
                base.x + centre_offset.x,
                base.y + selectable.height * 0.5,
                base.z + centre_offset.y,
            );
            // Twenty centimetres forgives roof overhang/low-resolution edges,
            // but unlike the old enclosing circle it does not claim empty yard.
            ray_vs_oriented_box(
                ray_origin,
                ray_dir,
                center,
                rotation,
                Vec3::new(
                    half_extents.x + 0.2,
                    selectable.height * 0.5,
                    half_extents.y + 0.2,
                ),
            )
        }
    }
}

pub(super) fn selectable_base(
    selectable: &Selectable,
    position: &PlayerPosition,
    visual: Option<&Transform>,
) -> Vec3 {
    match selectable.shape {
        // Characters are network-smoothed, so click the body on screen rather
        // than its last authoritative snapshot.
        SelectableShape::Person => visual.map_or(position.0, |visual| visual.translation),
        // Static authored building roots sit at ground level, while primitive
        // fallback meshes are centred vertically. PlayerPosition is the shared
        // ground anchor for both and therefore the only unambiguous base.
        SelectableShape::Footprint { .. } => position.0,
    }
}

#[cfg(test)]
// The focused geometry/picking tests sit beside the helpers they exercise;
// the large ECS click system follows as a separate section of this module.
#[allow(clippy::items_after_test_module)]
mod tests {
    use bevy::camera::{PerspectiveProjection, Projection, RenderTargetInfo, Viewport};
    use bevy::ecs::system::RunSystemOnce;
    use bevy::math::{Dir3, Ray3d};

    use super::*;

    #[test]
    fn elongated_building_does_not_claim_the_empty_part_of_its_old_pick_circle() {
        let selectable = Selectable {
            radius: Vec2::new(1.0, 4.0).length(),
            height: 4.0,
            shape: SelectableShape::Footprint {
                half_extents: Vec2::new(1.0, 4.0),
                centre_offset: Vec2::ZERO,
                rotation: 0.0,
            },
        };

        // x=3 was inside the former 4.12m enclosing circle, but is visibly
        // two metres beside this one-metre half-width building.
        assert!(selectable_ray_distance(
            &selectable,
            Vec3::ZERO,
            Vec3::new(3.0, 10.0, 0.0),
            Vec3::NEG_Y,
        )
        .is_none());
    }

    #[test]
    fn rotated_building_pick_volume_rotates_with_the_visible_asset() {
        let selectable = Selectable {
            radius: Vec2::new(1.0, 4.0).length(),
            height: 4.0,
            shape: SelectableShape::Footprint {
                half_extents: Vec2::new(1.0, 4.0),
                centre_offset: Vec2::ZERO,
                rotation: std::f32::consts::FRAC_PI_2,
            },
        };

        assert!(
            selectable_ray_distance(
                &selectable,
                Vec3::ZERO,
                Vec3::new(3.0, 10.0, 0.0),
                Vec3::NEG_Y,
            )
            .is_some(),
            "the long side should lie along world X after a quarter turn"
        );
        assert!(
            selectable_ray_distance(
                &selectable,
                Vec3::ZERO,
                Vec3::new(0.0, 10.0, 3.0),
                Vec3::NEG_Y,
            )
            .is_none(),
            "the short side should lie along world Z after a quarter turn"
        );
    }

    #[test]
    fn marquee_counts_visible_body_overlap_not_only_the_midpoint() {
        assert!(screen_rects_overlap(
            Vec2::new(100.0, 100.0),
            Vec2::new(112.0, 180.0),
            Vec2::new(108.0, 175.0),
            Vec2::new(150.0, 220.0),
        ));
        assert!(!screen_rects_overlap(
            Vec2::new(100.0, 100.0),
            Vec2::new(112.0, 180.0),
            Vec2::new(113.0, 175.0),
            Vec2::new(150.0, 220.0),
        ));
    }

    fn click_world(world: &mut World) {
        world
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
        world.run_system_once(pick_on_left_click).unwrap();
        {
            let mut mouse = world.resource_mut::<ButtonInput<MouseButton>>();
            mouse.clear_just_pressed(MouseButton::Left);
            mouse.release(MouseButton::Left);
        }
        world.run_system_once(pick_on_left_click).unwrap();
    }

    fn click_test_world(ray_origin: Vec3) -> World {
        let mut world = World::new();
        world.insert_resource(ButtonInput::<MouseButton>::default());
        world.insert_resource(InputState::default());
        world.insert_resource(WorldPlacementMode::default());
        world.insert_resource(CursorRay(Some(Ray3d::new(ray_origin, Dir3::NEG_Y))));
        world.insert_resource(CursorTerrainHit(Some(Vec3::new(
            ray_origin.x,
            0.0,
            ray_origin.z,
        ))));
        world.insert_resource(DragBox::default());
        world.insert_resource(Selection::default());

        let mut window = Window::default();
        window.resolution.set(1600.0, 900.0);
        window.set_cursor_position(Some(Vec2::new(800.0, 450.0)));
        world.spawn((window, PrimaryWindow));
        world.spawn(Camera3d::default());
        world
    }

    fn perspective_camera(viewport_size: UVec2) -> Camera {
        let viewport = Viewport {
            physical_size: viewport_size,
            ..default()
        };
        let mut projection = Projection::Perspective(PerspectiveProjection::default());
        projection.update(viewport_size.x as f32, viewport_size.y as f32);
        let mut camera = Camera {
            viewport: Some(viewport),
            ..default()
        };
        camera.computed.target_info = Some(RenderTargetInfo {
            physical_size: viewport_size,
            scale_factor: 1.0,
        });
        camera.computed.clip_from_view = projection.get_clip_from_view();
        camera
    }

    #[test]
    fn real_click_selects_the_visible_person_beside_a_dense_building() {
        let mut world = click_test_world(Vec3::new(3.0, 10.0, 0.0));
        let person = world
            .spawn((
                Selectable::person(),
                // Deliberately stale authoritative position: selection must
                // follow the smoothly rendered root the player actually sees.
                PlayerPosition(Vec3::new(8.0, 0.0, 0.0)),
                Transform::from_translation(Vec3::new(3.0, 0.0, 0.0)),
                CharacterActivity::Idle,
            ))
            .id();
        world.spawn((
            Selectable {
                radius: Vec2::new(1.0, 4.0).length(),
                height: 4.0,
                shape: SelectableShape::Footprint {
                    half_extents: Vec2::new(1.0, 4.0),
                    centre_offset: Vec2::ZERO,
                    rotation: 0.0,
                },
            },
            PlayerPosition(Vec3::ZERO),
            Transform::default(),
        ));

        click_world(&mut world);

        assert_eq!(world.resource::<Selection>().primary(), Some(person));
    }

    #[test]
    fn an_indoor_person_cannot_invisibly_steal_a_click() {
        let mut world = click_test_world(Vec3::new(0.0, 10.0, 0.0));
        world.spawn((
            Selectable::person(),
            PlayerPosition(Vec3::ZERO),
            Transform::default(),
            CharacterActivity::Indoors,
        ));

        click_world(&mut world);

        assert!(world.resource::<Selection>().is_empty());
    }

    #[test]
    fn real_drag_selects_the_owned_hero_at_half_render_resolution() {
        let mut world = World::new();
        let mut mouse = ButtonInput::<MouseButton>::default();
        mouse.press(MouseButton::Left);
        mouse.clear_just_pressed(MouseButton::Left);
        mouse.release(MouseButton::Left);
        world.insert_resource(mouse);
        world.insert_resource(InputState::default());
        world.insert_resource(WorldPlacementMode::default());
        world.insert_resource(CursorRay::default());
        world.insert_resource(CursorTerrainHit::default());
        world.insert_resource(DragBox {
            start: Some(Vec2::new(760.0, 380.0)),
            current: Vec2::new(840.0, 470.0),
            active: true,
        });
        world.insert_resource(Selection::default());
        world.insert_resource(crate::ui::name_entry::PlayerNameInput {
            name: "Aldric".into(),
            submitted: true,
        });

        let mut window = Window::default();
        window.resolution.set(1600.0, 900.0);
        window.set_cursor_position(Some(Vec2::new(840.0, 470.0)));
        world.spawn((window, PrimaryWindow));
        let camera_transform = Transform::from_xyz(0.0, 10.0, 10.0).looking_at(Vec3::ZERO, Vec3::Y);
        world.spawn((
            Camera3d::default(),
            perspective_camera(UVec2::new(800, 450)),
            camera_transform,
        ));

        let hero = world
            .spawn((
                Selectable::person(),
                PlayerPosition(Vec3::ZERO),
                Transform::default(),
                CommandedBy("aldric".into()),
                CharacterActivity::Idle,
            ))
            .id();
        // Also visibly inside the marquee, but not ours and therefore not
        // eligible for a command-oriented box selection.
        world.spawn((
            Selectable::person(),
            PlayerPosition(Vec3::new(0.2, 0.0, 0.0)),
            Transform::from_xyz(0.2, 0.0, 0.0),
            CharacterActivity::Idle,
        ));

        world.run_system_once(pick_on_left_click).unwrap();

        assert_eq!(world.resource::<Selection>().entities, vec![hero]);
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) fn pick_on_left_click(
    mouse: Res<ButtonInput<MouseButton>>,
    input_state: Res<InputState>,
    placement: Res<WorldPlacementMode>,
    cursor_ray: Res<CursorRay>,
    terrain_hit: Res<CursorTerrainHit>,
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &Transform), With<Camera3d>>,
    ui_blockers: Query<&Interaction>,
    account: Option<Res<crate::ui::name_entry::PlayerNameInput>>,
    candidates: Query<(
        Entity,
        &Selectable,
        &PlayerPosition,
        Option<&Transform>,
        Option<&CommandedBy>,
        Option<&CharacterActivity>,
    )>,
    mut drag: ResMut<DragBox>,
    mut selection: ResMut<Selection>,
) {
    let cursor = windows.single().ok().and_then(|w| w.cursor_position());

    // --- track the drag -----------------------------------------------------
    if mouse.just_pressed(MouseButton::Left) {
        // A press that starts on the UI, or during an armed placement, can never
        // become a selection. Latched AT PRESS rather than re-tested at release,
        // so press-on-HUD then drag into the world cannot marquee the world.
        let blocked = input_state.ui_blocking()
            || placement_armed(&placement)
            || crate::ui::pointer_over_ui(&ui_blockers);
        *drag = DragBox {
            start: if blocked { None } else { cursor },
            current: cursor.unwrap_or_default(),
            active: false,
        };
    }

    if mouse.pressed(MouseButton::Left) {
        if let (Some(start), Some(now)) = (drag.start, cursor) {
            drag.current = now;
            if start.distance(now) > BOX_MIN_PX {
                drag.active = true;
            }
        }
    }

    if !mouse.just_released(MouseButton::Left) {
        return;
    }

    let box_rect = drag.rect();
    let had_press = drag.start.is_some();
    *drag = DragBox::default();

    if !had_press {
        return;
    }

    let Ok(window) = windows.single() else {
        return;
    };
    let Ok((camera, camera_transform)) = cameras.single() else {
        return;
    };
    // Like CursorRay, marquee projection must use this frame's unparented
    // camera Transform. GlobalTransform is propagated in PostUpdate and is one
    // rendered frame behind while the commander camera is easing.
    let camera_transform = GlobalTransform::from(*camera_transform);

    // --- box select ---------------------------------------------------------
    if let Some((min, max)) = box_rect {
        let window_size = window.size();
        let my_account = account.as_ref().map(|i| i.name.trim().to_lowercase());
        let mut hits: Vec<(Entity, f32)> = Vec::new();
        for (entity, selectable, position, visual, commanded, activity) in candidates.iter() {
            // Yours only. A drag is a command gesture, so anything you cannot
            // order has no business being in the result.
            if !can_command(commanded, my_account.as_deref()) {
                continue;
            }
            if activity.is_some_and(|activity| *activity == CharacterActivity::Indoors) {
                continue;
            }
            let base = selectable_base(selectable, position, visual);
            if matches!(selectable.shape, SelectableShape::Person)
                && projected_person_overlaps_rect(
                    camera,
                    &camera_transform,
                    window_size,
                    base,
                    selectable.radius,
                    selectable.height,
                    min,
                    max,
                )
            {
                // Sorted by depth so the order is stable and front-most first,
                // which is what `primary()` should name.
                hits.push((
                    entity,
                    camera_transform
                        .translation()
                        .distance(base + Vec3::Y * selectable.height * 0.5),
                ));
            }
        }
        hits.sort_by(|a, b| a.1.total_cmp(&b.1));
        selection.set(hits.into_iter().map(|(entity, _)| entity).collect());
        return;
    }

    // --- single click -------------------------------------------------------
    let Some(ray) = cursor_ray.0 else {
        return;
    };
    let origin = ray.origin;
    let dir = ray.direction.as_vec3();

    // How far along the ray the ground is. Anything beyond it is behind a hill
    // and must not be selectable through it -- picking a unit you cannot see
    // reads as the game ignoring the terrain.
    let terrain_distance = terrain_hit
        .0
        .map(|point| (point - origin).dot(dir))
        .filter(|d| *d > 0.0);

    let mut best: Option<(Entity, f32)> = None;
    for (entity, selectable, position, visual, _commanded, activity) in candidates.iter() {
        if activity.is_some_and(|activity| *activity == CharacterActivity::Indoors) {
            continue;
        }
        let base = selectable_base(selectable, position, visual);
        let Some(distance) = selectable_ray_distance(selectable, base, origin, dir) else {
            continue;
        };
        // Tolerance past the ground hit: a character's feet sit AT the terrain,
        // so an exact comparison rejects the unit that was clicked.
        if let Some(ground) = terrain_distance {
            if distance > ground + selectable.height.max(1.0) {
                continue;
            }
        }
        if best.is_none_or(|(_, best_distance)| distance < best_distance) {
            best = Some((entity, distance));
        }
    }

    // Clicking empty ground clears -- the standard RTS deselect.
    selection.set(best.into_iter().map(|(entity, _)| entity).collect());
}
