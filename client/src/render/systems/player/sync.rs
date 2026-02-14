//! sync systems.

use super::*;

/// Sync player transforms (visibility is handled by update_local_player_visibility)
pub fn sync_player_transforms(
    time: Res<Time>,
    vehicles: Query<(Entity, &VehicleDriver, &Transform), (With<Vehicle>, Without<Player>)>,
    hover_bobs: Query<(&VehicleHoverBob, &ChildOf)>,
    mut players: Query<
        (&Player, &PlayerPosition, &PlayerRotation, &mut Transform),
        Without<Vehicle>,
    >,
    mut driver_to_vehicle: Local<HashMap<u64, (Entity, Vec3, Quat)>>,
    mut vehicle_bobs: Local<HashMap<Entity, f32>>,
) {
    let dt = time.delta_secs();
    let pos_rate: f32 = 22.0;
    let rot_rate: f32 = 26.0;
    let t_pos = 1.0_f32 - (-pos_rate * dt).exp();
    let t_rot = 1.0_f32 - (-rot_rate * dt).exp();

    // Map: driver_id -> vehicle transform (already smoothed in `sync_vehicle_transforms`)
    driver_to_vehicle.clear();
    for (entity, driver, veh_transform) in vehicles.iter() {
        if let Some(driver_id) = driver.driver_id {
            driver_to_vehicle.insert(
                driver_id,
                (entity, veh_transform.translation, veh_transform.rotation),
            );
        }
    }

    let t = time.elapsed_secs();
    vehicle_bobs.clear();
    for (hover, parent) in hover_bobs.iter() {
        let vehicle_entity = parent.parent();
        let bob = (t * hover.frequency + hover.phase).sin() * hover.amplitude;
        vehicle_bobs.insert(vehicle_entity, bob);
    }

    for (player, position, rotation, mut transform) in players.iter_mut() {
        // If this player is driving a vehicle, attach their visual to the vehicle to eliminate
        // relative jitter between player and bike at high speed.
        if let Some((veh_entity, veh_pos, veh_rot)) =
            driver_to_vehicle.get(&peer_id_to_u64(player.client_id))
        {
            let (bob, is_hover_bike) = match vehicle_bobs.get(veh_entity) {
                Some(bob) => (*bob, true),
                None => (0.0, false),
            };
            let seat_height = if is_hover_bike { 0.55 + 0.35 } else { 0.55 };
            let seat_forward = if is_hover_bike { 0.15 + 0.30 } else { 0.15 };

            // Seat offset: slightly above and forward/back on the vehicle.
            let seat_local = Vec3::new(0.0, seat_height, seat_forward) + Vec3::Y * bob;
            let seat_offset = *veh_rot * seat_local;
            let target_pos = *veh_pos + seat_offset;
            // SNAP directly to vehicle - no lerp needed, vehicle is already smoothed
            transform.translation = target_pos;
            transform.rotation = *veh_rot;
        } else {
            transform.translation = transform.translation.lerp(position.0, t_pos);
            let target_rot = Quat::from_rotation_y(rotation.0);
            transform.rotation = transform.rotation.slerp(target_rot, t_rot);
        }
    }
}
