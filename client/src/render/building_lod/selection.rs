//! Screen size, not distance from the streaming focus, determines building detail.
use bevy::prelude::*;

pub(super) const DETAIL_PIXELS: f32 = 120.0;
pub(super) const HIDE_PIXELS: f32 = 4.0;
const HYSTERESIS: f32 = 0.12;

pub(super) fn select_level(pixels: f32, current: usize) -> usize {
    let thresholds = [DETAIL_PIXELS, HIDE_PIXELS];
    let mut level = current.min(super::HIDDEN);
    while level > 0 && pixels > thresholds[level - 1] * (1.0 + HYSTERESIS) {
        level -= 1;
    }
    while level < thresholds.len() && pixels < thresholds[level] * (1.0 - HYSTERESIS) {
        level += 1;
    }
    level
}

pub(super) fn projected_diameter(
    center: Vec3,
    radius: f32,
    camera: &GlobalTransform,
    projection: &Projection,
    viewport_height: f32,
) -> f32 {
    match projection {
        Projection::Perspective(p) => {
            let depth = (center - camera.translation()).dot(*camera.forward());
            if depth < -radius {
                return 0.0;
            }
            // Crossing the near plane always restores detail, never divides by zero.
            radius * viewport_height / ((depth - radius).max(0.01) * (p.fov * 0.5).tan())
        }
        Projection::Orthographic(p) => 2.0 * radius * viewport_height / p.area.height().max(0.01),
        _ => f32::INFINITY,
    }
}

use super::{BuildingLod, BuildingLodOverride};

pub(super) fn select(
    time: Res<Time>,
    cameras: Query<
        (&Camera, &Projection, &GlobalTransform),
        With<crate::camera_rts::CommanderCamera>,
    >,
    mut roots: Query<(
        &GlobalTransform,
        &mut BuildingLod,
        &mut Visibility,
        Option<&BuildingLodOverride>,
    )>,
    mut primitives: Query<&mut Mesh3d>,
    mut elapsed: Local<f32>,
) {
    // No hierarchy walks, allocations or string matching in the steady selection pass.
    *elapsed += time.delta_secs();
    if *elapsed < 0.10 {
        return;
    }
    *elapsed = 0.0;
    let Ok((camera, projection, view)) = cameras.single() else {
        return;
    };
    let Some(viewport) = camera.physical_viewport_size() else {
        return;
    };
    for (transform, mut lod, mut visibility, forced) in &mut roots {
        if !lod.ready {
            continue;
        }
        let (scale, _, _) = transform.to_scale_rotation_translation();
        let pixels = projected_diameter(
            transform.transform_point(lod.library.center),
            lod.library.radius * scale.abs().max_element(),
            view,
            projection,
            viewport.y as f32,
        );
        let level = forced.map_or_else(
            || select_level(pixels, lod.level),
            |f| f.0.min(super::HIDDEN),
        );
        if level == lod.level && !lod.needs_refresh {
            continue;
        }
        if level == super::HIDDEN {
            lod.visibility_before_hide.get_or_insert(*visibility);
            *visibility = Visibility::Hidden;
        } else if let Some(original) = lod.visibility_before_hide.take() {
            *visibility = original;
        }
        let mut valid = true;
        for &(entity, index) in &lod.bindings {
            if let Ok(mut mesh) = primitives.get_mut(entity) {
                // Hidden buildings retain their mesh and animation pose for the return.
                if level != super::HIDDEN {
                    mesh.0 = lod.library.meshes[index][level].clone();
                }
            } else {
                valid = false;
            }
        }
        lod.level = level;
        lod.ready = valid;
        lod.needs_refresh = false;
    }
}
