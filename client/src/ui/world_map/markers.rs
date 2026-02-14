//! markers systems.

use super::*;

pub(super) fn update_player_marker(
    map_open: Res<MapOpen>,
    map_config: Res<MapUiConfig>,
    player_query: Query<(&PlayerPosition, &PlayerRotation), With<LocalPlayer>>,
    mut markers: Query<(&mut Node, &mut UiTransform), With<MapPlayerMarker>>,
) {
    if !map_open.0 {
        return;
    }
    let Some(bounds) = map_config.bounds else {
        return;
    };
    let Ok((player_pos, player_rot)) = player_query.single() else {
        return;
    };

    let Some((map_x, map_y)) = world_to_map(player_pos.0.x, player_pos.0.z, bounds) else {
        return;
    };
    let map_x = map_x - PLAYER_ARROW_SIZE * 0.5;
    let map_y = map_y - PLAYER_ARROW_SIZE * 0.5;

    // PlayerRotation.0 is the yaw angle from physics.rs:
    // yaw=0 → facing North (-Z in world)
    // yaw increases counterclockwise when viewed from above
    //
    // On the map: +X is East (right), +Z is South (down on screen since map Y grows down)
    // Arrow image points UP by default (North)
    //
    // For Rot2 in UI: positive angle = counterclockwise rotation on screen
    // We want: facing North (yaw=0) → arrow points up → rotation = 0
    //          facing East (yaw=-PI/2) → arrow points right → rotation = -PI/2
    //          facing South (yaw=PI) → arrow points down → rotation = PI
    //          facing West (yaw=PI/2) → arrow points left → rotation = PI/2
    //
    // So the UI rotation angle is simply the negative of the player yaw
    let angle = -player_rot.0;

    for (mut node, mut ui_transform) in markers.iter_mut() {
        node.left = Val::Px(map_x);
        node.top = Val::Px(map_y);
        ui_transform.rotation = Rot2::radians(angle);
    }
}
