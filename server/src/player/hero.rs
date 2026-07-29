//! Hero lifecycle + movement: the server-authoritative embodied character.
//!
//! The hero is the first server-simulated mover since the RTS pivot: spawned
//! by a god command, stepped here toward client-sent move targets, streamed
//! to clients through the normal replication + region-interest path. Clients
//! only ever send intent ([`HeroMoveTo`]); position/rotation truth lives here.

use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use lightyear::prelude::server::ClientOf;
use lightyear::prelude::{MessageReceiver, PeerId, RemoteId};

use shared::components::{Hero, PlayerPosition, PlayerRotation};
use shared::player::{HERO_ARRIVE_EPSILON, HERO_MOVE_SPEED};
use shared::protocol::HeroMoveTo;
use shared::region::RegionCoord;
use shared::terrain::WorldTerrain;

/// Latest move order per peer. Later orders replace earlier ones (click-to-move
/// semantics); an entry is removed on arrival.
#[derive(Resource, Default)]
pub struct HeroMoveTargets(pub HashMap<PeerId, Vec3>);

/// Drain [`HeroMoveTo`] intents into [`HeroMoveTargets`].
///
/// Unlike god commands this needs no dev gate — moving your own hero is a
/// normal gameplay verb. Orders from peers WITHOUT a hero are dropped here:
/// retaining them would pre-seed a walk order that fires the instant a hero
/// spawns, and (with per-session random peer ids) grow the map forever.
pub fn handle_hero_move_orders(
    mut client_links: Query<(&RemoteId, &mut MessageReceiver<HeroMoveTo>), With<ClientOf>>,
    heroes: Query<&Hero>,
    mut targets: ResMut<HeroMoveTargets>,
) {
    for (remote_id, mut receiver) in client_links.iter_mut() {
        for order in receiver.receive() {
            if !order.target.is_finite() {
                continue;
            }
            if !heroes.iter().any(|h| h.owner == remote_id.0) {
                continue;
            }
            targets.0.insert(remote_id.0, order.target);
        }
    }
}

/// Step every hero toward its move target at walk speed, snapped to terrain.
///
/// Every component write is `!=`-guarded: change detection drives replication,
/// and an idle hero must generate zero network traffic.
pub fn step_heroes(
    terrain: Option<Res<WorldTerrain>>,
    mut targets: ResMut<HeroMoveTargets>,
    mut heroes: Query<(
        &Hero,
        &mut PlayerPosition,
        &mut PlayerRotation,
        &mut RegionCoord,
    )>,
) {
    let Some(terrain) = terrain else {
        return;
    };
    let dt = 1.0 / shared::protocol::FIXED_TIMESTEP_HZ as f32;

    for (hero, mut pos, mut rot, mut region) in heroes.iter_mut() {
        let Some(&target) = targets.0.get(&hero.owner) else {
            continue;
        };

        // Plan in the ground plane; height is derived from the terrain, so a
        // click on a hillside never makes the hero chase an unreachable Y.
        let current = Vec2::new(pos.0.x, pos.0.z);
        let goal = Vec2::new(target.x, target.z);
        let to_goal = goal - current;
        let distance = to_goal.length();

        if distance <= HERO_ARRIVE_EPSILON {
            targets.0.remove(&hero.owner);
            continue;
        }

        let step = (HERO_MOVE_SPEED * dt).min(distance);
        let next = current + to_goal / distance * step;
        let next_pos = Vec3::new(next.x, terrain.get_height(next.x, next.y), next.y);

        // Face travel direction. Bevy yaw 0 looks down -Z; atan2(x, z) of the
        // FORWARD vector gives the yaw whose -Z axis points along it.
        let next_yaw = f32::atan2(-to_goal.x, -to_goal.y);

        if pos.0 != next_pos {
            pos.0 = next_pos;
        }
        if rot.0 != next_yaw {
            rot.0 = next_yaw;
        }
        let next_region = RegionCoord::from_world_pos(next_pos);
        if *region != next_region {
            *region = next_region;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Yaw convention: a hero walking toward +X must face +X, i.e. rotating
    /// Bevy's -Z forward by the yaw must give the travel direction. This is
    /// the mirroring bug class this repo keeps re-fighting — pin it.
    #[test]
    fn hero_yaw_faces_travel_direction() {
        for dir in [
            Vec2::new(1.0, 0.0),
            Vec2::new(-1.0, 0.0),
            Vec2::new(0.0, 1.0),
            Vec2::new(0.0, -1.0),
            Vec2::new(0.7, -0.7),
        ] {
            let yaw = f32::atan2(-dir.x, -dir.y);
            let forward = Quat::from_rotation_y(yaw) * Vec3::NEG_Z;
            let forward2 = Vec2::new(forward.x, forward.z).normalize();
            let dir_n = dir.normalize();
            assert!(
                forward2.distance(dir_n) < 1e-5,
                "dir {dir_n:?} => yaw {yaw} => forward {forward2:?}"
            );
        }
    }
}
