use bevy::prelude::*;
use std::collections::{HashMap, HashSet};

use shared::terrain::ChunkCoord;
use shared::debug::DebugGizmoMode;

use crate::props::is_tree_kind;

use super::{ClientDerivedColliderLibrary, EnvironmentProp, PropKindTag};

/// Debug overrides for prop LOD visibility.
#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum PropLodDebugMode {
    #[default]
    Off,
    ForceLod0,
    ForceLod1,
}

/// Cycle prop LOD debug modes with F5.
pub(super) fn toggle_prop_lod_debug(
    keyboard: Res<ButtonInput<KeyCode>>,
    mut mode: ResMut<PropLodDebugMode>,
) {
    if !keyboard.just_pressed(KeyCode::F5) {
        return;
    }

    *mode = match *mode {
        PropLodDebugMode::Off => PropLodDebugMode::ForceLod0,
        PropLodDebugMode::ForceLod0 => PropLodDebugMode::ForceLod1,
        PropLodDebugMode::ForceLod1 => PropLodDebugMode::Off,
    };

    let label = match *mode {
        PropLodDebugMode::Off => "OFF",
        PropLodDebugMode::ForceLod0 => "FORCE LOD0",
        PropLodDebugMode::ForceLod1 => "FORCE LOD1",
    };
    info!("Prop LOD debug mode: {label} (F5 to cycle)");
}

/// Log prop density/duplication stats with F6.
pub(super) fn log_prop_density_snapshot(
    keyboard: Res<ButtonInput<KeyCode>>,
    camera: Query<&GlobalTransform, With<Camera3d>>,
    props: Query<(&EnvironmentProp, &Transform, &PropKindTag)>,
) {
    if !keyboard.just_pressed(KeyCode::F6) {
        return;
    }

    let Ok(camera) = camera.single() else {
        return;
    };
    let cam_pos = camera.translation();

    let mut total = 0usize;
    let mut within_50 = 0usize;
    let mut within_100 = 0usize;
    let mut within_200 = 0usize;
    let mut tree_total = 0usize;
    let mut tree_50 = 0usize;
    let mut tree_100 = 0usize;
    let mut tree_200 = 0usize;

    let mut by_chunk: HashMap<ChunkCoord, usize> = HashMap::new();
    let mut by_kind: HashMap<shared::props::PropKind, usize> = HashMap::new();
    let mut seen_positions: HashSet<(ChunkCoord, i32, i32, i32)> = HashSet::new();
    let mut duplicate_positions = 0usize;

    for (prop, transform, kind) in props.iter() {
        total += 1;
        *by_chunk.entry(prop.chunk).or_default() += 1;
        *by_kind.entry(kind.0).or_default() += 1;

        let pos = transform.translation;
        let dist_sq = (pos - cam_pos).length_squared();
        if dist_sq <= 50.0 * 50.0 {
            within_50 += 1;
        }
        if dist_sq <= 100.0 * 100.0 {
            within_100 += 1;
        }
        if dist_sq <= 200.0 * 200.0 {
            within_200 += 1;
        }

        if is_tree_kind(kind.0) {
            tree_total += 1;
            if dist_sq <= 50.0 * 50.0 {
                tree_50 += 1;
            }
            if dist_sq <= 100.0 * 100.0 {
                tree_100 += 1;
            }
            if dist_sq <= 200.0 * 200.0 {
                tree_200 += 1;
            }
        }

        let key = (
            prop.chunk,
            (pos.x * 10.0).round() as i32,
            (pos.y * 10.0).round() as i32,
            (pos.z * 10.0).round() as i32,
        );
        if !seen_positions.insert(key) {
            duplicate_positions += 1;
        }
    }

    let (max_chunk, max_count) = by_chunk
        .iter()
        .max_by_key(|(_, count)| *count)
        .map(|(c, v)| (*c, *v))
        .unwrap_or((ChunkCoord::new(0, 0), 0));

    let mut kind_counts: Vec<(shared::props::PropKind, usize)> = by_kind.into_iter().collect();
    kind_counts.sort_by(|a, b| b.1.cmp(&a.1));
    let top_kinds: Vec<String> = kind_counts
        .into_iter()
        .take(12)
        .map(|(kind, count)| format!("{}={}", kind.id(), count))
        .collect();

    info!(
        "Props snapshot (F6): total={} trees={} | within 50m: {} (trees {}), 100m: {} (trees {}), 200m: {} (trees {}) | max chunk {:?} = {} props | dup positions={} (0.1m grid) | top kinds: {}",
        total,
        tree_total,
        within_50,
        tree_50,
        within_100,
        tree_100,
        within_200,
        tree_200,
        max_chunk,
        max_count,
        duplicate_positions,
        top_kinds.join(", ")
    );
}

/// Draw client-side prop collider gizmos (debug-only).
pub(super) fn debug_draw_prop_colliders(
    mut gizmos: Gizmos,
    debug_mode: Res<DebugGizmoMode>,
    library: Option<Res<ClientDerivedColliderLibrary>>,
    camera: Query<&Transform, With<Camera3d>>,
    props: Query<(&PropKindTag, &Transform), With<EnvironmentProp>>,
) {
    if !debug_mode.0 {
        return;
    }
    let Some(library) = library else { return };

    let Ok(camera) = camera.single() else { return };
    let cam_pos = camera.translation;
    let max_dist = 120.0;
    let max_dist2 = max_dist * max_dist;

    let color = Color::srgba(0.0, 1.0, 1.0, 0.5);

    for (kind, transform) in props.iter() {
        let Some(shape) = library.by_kind.get(&kind.0) else {
            continue;
        };

        let pos = transform.translation;
        if (pos - cam_pos).length_squared() > max_dist2 {
            continue;
        }

        let s = transform.scale.x;

        if !shape.hulls.is_empty() {
            // Draw actual 3D convex hull wireframe with rotation applied.
            let rot = transform.rotation;
            for hull in &shape.hulls {
                for face in &hull.hull_faces {
                    // Apply rotation then scale, then translate.
                    let v0 = pos + rot * (face.vertices[0] * s);
                    let v1 = pos + rot * (face.vertices[1] * s);
                    let v2 = pos + rot * (face.vertices[2] * s);

                    gizmos.line(v0, v1, color);
                    gizmos.line(v1, v2, color);
                    gizmos.line(v2, v0, color);
                }
            }
        } else {
            // Fallback: draw bounding sphere.
            let circle_rot = Quat::from_rotation_x(std::f32::consts::FRAC_PI_2);
            let r = (shape.bounding_radius * s).max(0.05);
            let iso = Isometry3d::new(pos, circle_rot);
            gizmos.circle(iso, r, color).resolution(24);
        }
    }
}
