//! Fire and weapon switch systems.

use bevy::prelude::*;
use lightyear::prelude::server::*;
use lightyear::prelude::*;

use shared::components::{
    Bullet, BulletPrevPosition, BulletVelocity, EquippedWeapon, Player, PlayerPosition,
};
use shared::player::PLAYER_HEIGHT;
use shared::protocol::{AudioEvent, AudioEventKind, ReliableChannel, ShootRequest};
use shared::weapons::{ballistics, muzzle_offset};

use bevy_rapier3d::plugin::ReadRapierContext;

use crate::combat::reload::WeaponReload;
use crate::net::peer::peer_id_to_u64;
use crate::physics::queries::cast_world_impact;
use crate::player::index::PlayerEntityIndex;

/// Handle shoot requests from clients.
pub fn handle_shoot_requests(
    mut commands: Commands,
    mut client_links: Query<(&RemoteId, &mut MessageReceiver<ShootRequest>), With<ClientOf>>,
    player_index: Res<PlayerEntityIndex>,
    mut players: Query<(&PlayerPosition, &mut EquippedWeapon, Option<&WeaponReload>), With<Player>>,
    mut audio_senders: Query<&mut MessageSender<AudioEvent>, (With<ClientOf>, With<Connected>)>,
    time: Res<Time>,
    rapier: ReadRapierContext,
    mut shots_fired: Local<Vec<(u64, Vec3, shared::weapons::WeaponType)>>,
) {
    let current_time = time.elapsed_secs();
    let rapier_context = rapier.single().ok();

    // Collect shots to broadcast audio events after processing.
    shots_fired.clear();

    for (remote_id, mut receiver) in client_links.iter_mut() {
        let peer_id = remote_id.0;
        let shooter_id = peer_id_to_u64(peer_id);
        let Some(player_entity) = player_index.entity_for_peer(peer_id) else {
            continue;
        };

        for request in receiver.receive() {
            let Ok((position, mut weapon, reload)) = players.get_mut(player_entity) else {
                continue;
            };

            weapon.aiming = request.aiming;

            if let Some(reload) = reload {
                if reload.weapon_type == weapon.weapon_type && current_time < reload.end_time {
                    continue;
                }
            }
            if !weapon.can_fire(current_time) {
                continue;
            }

            let stats = weapon.weapon_type.stats();

            if !weapon.fire(current_time) {
                continue;
            }

            // Calculate spawn position from view-model muzzle offset (camera space).
            let forward = request.direction.normalize();
            let right = forward.cross(Vec3::Y).normalize_or_zero();
            let up = Vec3::Y;
            let muzzle_local = muzzle_offset(weapon.weapon_type);
            let camera_height = PLAYER_HEIGHT * 0.4;
            let eye = position.0 + up * camera_height;
            let spawn_pos = eye
                + up * muzzle_local.y
                + right * muzzle_local.x
                + forward * (-muzzle_local.z);

            // Crosshair convergence: the bullet spawns at the muzzle (offset
            // right/below the eye), so flying parallel to the camera ray
            // would land a constant ~0.3m right-and-low of the crosshair at
            // EVERY distance. Instead, aim the muzzle at the point the
            // camera ray actually hits (or max range in open air) — bullets
            // land on the crosshair while still visibly leaving the gun.
            // Point-blank clamp keeps the aim from inverting when a wall is
            // closer than the muzzle.
            let converge_dist = rapier_context
                .as_ref()
                .and_then(|context| {
                    cast_world_impact(context, eye, forward, ballistics::BULLET_MAX_RANGE)
                        .map(|(_, intersection)| intersection.time_of_impact)
                })
                .unwrap_or(ballistics::BULLET_MAX_RANGE)
                .max(3.0);
            let converge_point = eye + forward * converge_dist;
            let aim_dir = (converge_point - spawn_pos).normalize_or(forward.into());

            let spread = weapon.current_spread();

            for _ in 0..stats.pellet_count {
                let spread_direction = ballistics::apply_spread(aim_dir, spread);
                let velocity = spread_direction * stats.bullet_speed;

                commands.spawn((
                    Bullet {
                        owner_id: shooter_id,
                        weapon_type: weapon.weapon_type,
                        spawn_position: spawn_pos,
                        initial_velocity: velocity,
                        spawn_time: current_time,
                    },
                    BulletVelocity(velocity),
                    BulletPrevPosition(spawn_pos),
                    PlayerPosition(spawn_pos),
                    Transform::from_translation(spawn_pos),
                    Replicate::new(ReplicationMode::SingleServer(NetworkTarget::All)),
                ));
            }

            shots_fired.push((shooter_id, spawn_pos, weapon.weapon_type));

            if crate::telemetry::hotlog_enabled() {
                info!(
                    "Player {:?} fired {:?} (ammo: {}/{})",
                    peer_id, weapon.weapon_type, weapon.ammo_in_mag, stats.magazine_size
                );
            } else {
                trace!(
                    "Player {:?} fired {:?} (ammo: {}/{})",
                    peer_id,
                    weapon.weapon_type,
                    weapon.ammo_in_mag,
                    stats.magazine_size
                );
            }
        }
    }

    for (shooter_id, position, weapon_type) in shots_fired.iter().copied() {
        let audio_event = AudioEvent {
            player_id: shooter_id,
            position,
            kind: AudioEventKind::Gunshot { weapon_type },
        };

        for mut sender in audio_senders.iter_mut() {
            sender.send::<ReliableChannel>(audio_event.clone());
        }
    }
}
