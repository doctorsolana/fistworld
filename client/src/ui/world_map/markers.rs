//! markers systems.

use super::*;

pub(super) fn update_player_marker(
    map_open: Res<MapOpen>,
    map_config: Res<MapUiConfig>,
    local: Option<Res<crate::camera_rts::LocalPeerId>>,
    heroes: Query<(&Hero, &PlayerPosition, &PlayerRotation)>,
    mut markers: Query<(&mut Node, &mut UiTransform), With<MapPlayerMarker>>,
) {
    if !map_open.0 {
        return;
    }

    let pose = local
        .as_deref()
        .zip(map_config.bounds)
        .and_then(|(local, bounds)| {
            heroes
                .iter()
                .find(|(hero, _, _)| shared::player::peer_id_to_u64(hero.owner) == local.0)
                .and_then(|(_, position, rotation)| player_marker_pose(position, rotation, bounds))
        });

    for (mut node, mut ui_transform) in markers.iter_mut() {
        let Some((position, angle)) = pose else {
            node.display = Display::None;
            continue;
        };
        node.display = Display::Flex;
        node.left = Val::Px(position.x - PLAYER_ARROW_SIZE * 0.5);
        node.top = Val::Px(position.y - PLAYER_ARROW_SIZE * 0.5);
        // UiTransform rotates clockwise because UI Y grows downward. Hero yaw
        // is Bevy world yaw: zero faces -Z/north and -PI/2 faces +X/east.
        ui_transform.rotation = Rot2::radians(angle);
    }
}

fn player_marker_pose(
    position: &PlayerPosition,
    rotation: &PlayerRotation,
    bounds: MapBounds,
) -> Option<(Vec2, f32)> {
    let (x, y) = world_to_map(position.0.x, position.0.z, bounds)?;
    Some((Vec2::new(x, y), -rotation.0))
}

pub(super) fn update_camera_viewport(
    map_open: Res<MapOpen>,
    map_config: Res<MapUiConfig>,
    cameras: Query<(&Camera, &Transform, &crate::camera_rts::CommanderCamera), With<Camera3d>>,
    mut edges: Query<(&MapViewportEdge, &mut Node, &mut UiTransform)>,
) {
    if !map_open.0 {
        return;
    }

    let projected = map_config.bounds.and_then(|bounds| {
        let (camera, transform, controller) = cameras.single().ok()?;
        let focus = transform.translation + Vec3::from(transform.forward()) * controller.zoom;
        let world_corners = camera_ground_footprint(camera, transform, focus.y)?;
        let mut map_corners = [Vec2::ZERO; 4];
        for (index, corner) in world_corners.into_iter().enumerate() {
            let (x, y) = world_to_map(corner.x, corner.y, bounds)?;
            map_corners[index] = Vec2::new(x, y);
        }
        Some(map_corners)
    });

    for (edge, mut node, mut transform) in edges.iter_mut() {
        let Some(corners) = projected else {
            node.display = Display::None;
            continue;
        };
        let start = corners[usize::from(edge.0) % 4];
        let end = corners[(usize::from(edge.0) + 1) % 4];
        let Some(layout) = viewport_edge_layout(start, end) else {
            node.display = Display::None;
            continue;
        };

        node.display = Display::Flex;
        node.left = Val::Px(layout.left);
        node.top = Val::Px(layout.top);
        node.width = Val::Px(layout.length);
        node.height = Val::Px(VIEWPORT_EDGE_THICKNESS);
        transform.rotation = Rot2::radians(layout.angle);
    }
}

/// Project all four real screen corners onto the camera focus plane.
///
/// A tilted perspective camera sees a trapezoid on the ground, not a square:
/// the top of the screen reaches farther and spans more world space than the
/// bottom. Using the actual rays also preserves the window aspect ratio and
/// follows changes to FOV, tilt, orbit and render-target dimensions.
fn camera_ground_footprint(
    camera: &Camera,
    transform: &Transform,
    focus_y: f32,
) -> Option<[Vec2; 4]> {
    let size = camera.logical_viewport_size()?;
    if size.x <= 0.0 || size.y <= 0.0 {
        return None;
    }
    let camera_global = GlobalTransform::from(*transform);
    let screen_corners = [
        Vec2::ZERO,
        Vec2::new(size.x, 0.0),
        size,
        Vec2::new(0.0, size.y),
    ];
    let mut ground = [Vec2::ZERO; 4];
    for (index, screen) in screen_corners.into_iter().enumerate() {
        let ray = camera.viewport_to_world(&camera_global, screen).ok()?;
        let vertical = ray.direction.y;
        if vertical.abs() <= f32::EPSILON {
            return None;
        }
        let distance = (focus_y - ray.origin.y) / vertical;
        if !distance.is_finite() || distance <= 0.0 {
            return None;
        }
        let hit = ray.get_point(distance);
        ground[index] = Vec2::new(hit.x, hit.z);
    }
    Some(ground)
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ViewportEdgeLayout {
    left: f32,
    top: f32,
    length: f32,
    angle: f32,
}

fn viewport_edge_layout(start: Vec2, end: Vec2) -> Option<ViewportEdgeLayout> {
    let delta = end - start;
    let length = delta.length();
    if !length.is_finite() || length <= f32::EPSILON {
        return None;
    }
    let midpoint = (start + end) * 0.5;
    Some(ViewportEdgeLayout {
        left: midpoint.x - length * 0.5,
        top: midpoint.y - VIEWPORT_EDGE_THICKNESS * 0.5,
        length,
        angle: delta.y.atan2(delta.x),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bounds() -> MapBounds {
        MapBounds {
            min: [-100.0, -200.0],
            max: [100.0, 200.0],
        }
    }

    #[test]
    fn hero_marker_uses_world_position_and_facing() {
        let (position, angle) = player_marker_pose(
            &PlayerPosition(Vec3::new(50.0, 0.0, -100.0)),
            &PlayerRotation(-std::f32::consts::FRAC_PI_2),
            bounds(),
        )
        .unwrap();

        assert_eq!(position, Vec2::new(384.0, 128.0));
        assert_eq!(angle, std::f32::consts::FRAC_PI_2);
    }

    #[test]
    fn viewport_edge_is_centered_between_projected_corners() {
        let layout = viewport_edge_layout(Vec2::new(10.0, 20.0), Vec2::new(40.0, 60.0)).unwrap();
        assert_eq!(layout.length, 50.0);
        assert_eq!(layout.left, 0.0);
        assert_eq!(layout.top, 39.25);
        assert!((layout.angle - 40.0f32.atan2(30.0)).abs() < 1e-6);
    }
}
