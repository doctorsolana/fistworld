//! hover systems.

use super::*;

/// Apply a subtle hover bob to hoverbike visuals.
pub fn update_vehicle_hover(
    time: Res<Time>,
    player_query: Query<&PlayerPosition, With<LocalPlayer>>,
    mut visuals: Query<(
        &VehicleHoverBob,
        &mut Transform,
        Option<&ViewVisibility>,
        &GlobalTransform,
    )>,
) {
    let t = time.elapsed_secs();
    let Ok(player_pos) = player_query.single() else {
        return;
    };
    for (hover, mut transform, view_vis, global) in visuals.iter_mut() {
        if let Some(view_vis) = view_vis {
            if !view_vis.get() {
                continue;
            }
        }
        let dist_sq = (global.translation() - player_pos.0).length_squared();
        if dist_sq > VEHICLE_SHADOW_RANGE_SQ {
            continue;
        }
        let bob = (t * hover.frequency + hover.phase).sin() * hover.amplitude;
        transform.translation = hover.base_offset + Vec3::Y * bob;
    }
}
