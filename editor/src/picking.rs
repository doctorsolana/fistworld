use bevy::math::Ray3d;
use bevy::prelude::*;
use bevy::window::PrimaryWindow;

use shared::terrain::WorldTerrain;

use crate::session::{CursorTerrainHit, EditorMainCamera};

const RAY_MAX_DISTANCE: f32 = 4000.0;
const RAY_STEP: f32 = 8.0;
const RAY_BINARY_STEPS: usize = 12;

pub fn update_cursor_terrain_hit(
    windows: Query<&Window, With<PrimaryWindow>>,
    cameras: Query<(&Camera, &GlobalTransform), With<EditorMainCamera>>,
    terrain: Res<WorldTerrain>,
    mut hit: ResMut<CursorTerrainHit>,
) {
    let Ok(window) = windows.single() else {
        hit.0 = None;
        return;
    };
    let Some(cursor_pos) = window.cursor_position() else {
        hit.0 = None;
        return;
    };

    let Ok((camera, camera_transform)) = cameras.single() else {
        hit.0 = None;
        return;
    };

    let Ok(ray) = camera.viewport_to_world(camera_transform, cursor_pos) else {
        hit.0 = None;
        return;
    };

    hit.0 = intersect_terrain(ray, &terrain);
}

fn intersect_terrain(ray: Ray3d, terrain: &WorldTerrain) -> Option<Vec3> {
    let origin = ray.origin;
    let dir = ray.direction.as_vec3();
    let bounds = terrain.generator.active_map_bounds();

    let mut prev_t = 0.0;
    let mut prev_pos = origin;
    let mut prev_f = prev_pos.y - terrain.get_height(prev_pos.x, prev_pos.z);

    let mut t = RAY_STEP;
    while t <= RAY_MAX_DISTANCE {
        let pos = origin + dir * t;
        if !bounds.contains_xz(pos.x, pos.z) {
            prev_t = t;
            prev_pos = pos;
            prev_f = pos.y - terrain.get_height(pos.x, pos.z);
            t += RAY_STEP;
            continue;
        }

        let f = pos.y - terrain.get_height(pos.x, pos.z);
        if prev_f > 0.0 && f <= 0.0 {
            let mut lo = prev_t;
            let mut hi = t;

            for _ in 0..RAY_BINARY_STEPS {
                let mid = (lo + hi) * 0.5;
                let mid_pos = origin + dir * mid;
                let mid_f = mid_pos.y - terrain.get_height(mid_pos.x, mid_pos.z);
                if mid_f > 0.0 {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }

            let final_t = (lo + hi) * 0.5;
            let hit_pos = origin + dir * final_t;
            let ground_y = terrain.get_height(hit_pos.x, hit_pos.z);
            return Some(Vec3::new(hit_pos.x, ground_y, hit_pos.z));
        }

        prev_t = t;
        prev_pos = pos;
        prev_f = f;
        t += RAY_STEP;
    }

    if dir.y.abs() > f32::EPSILON {
        let t_plane = -origin.y / dir.y;
        if t_plane >= 0.0 && t_plane <= RAY_MAX_DISTANCE {
            let pos = origin + dir * t_plane;
            if bounds.contains_xz(pos.x, pos.z) {
                let ground_y = terrain.get_height(pos.x, pos.z);
                return Some(Vec3::new(pos.x, ground_y, pos.z));
            }
        }
    }

    let _ = prev_pos;
    None
}
