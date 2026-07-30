//! Hero lifecycle + movement: the server-authoritative embodied character.
//!
//! The hero is the first server-simulated mover since the RTS pivot: spawned
//! by a god command, stepped here toward client-sent move targets, streamed
//! to clients through the normal replication + region-interest path. Clients
//! only ever send intent ([`HeroMoveTo`]); position/rotation truth lives here.

use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use lightyear::prelude::server::ClientOf;
use lightyear::prelude::{MessageReceiver, NetworkTarget, PeerId, RemoteId, Replicate};

use shared::components::{
    CharacterKind, CharacterName, Hero, HeroOutfit, PlayerPosition, PlayerRotation,
};
use shared::player_profile::HeroSave;
use shared::player::{HERO_ARRIVE_EPSILON, HERO_MOVE_SPEED};
use shared::protocol::HeroMoveTo;
use shared::region::RegionCoord;
use shared::terrain::WorldTerrain;

/// Latest move order per peer. Later orders replace earlier ones (click-to-move
/// semantics); an entry is removed on arrival.
#[derive(Resource, Default)]
pub struct HeroMoveTargets(pub HashMap<PeerId, Vec3>);

/// Hero entity per player, keyed by lowercase profile NAME rather than peer.
///
/// Peer ids are random per session, so they cannot identify a returning
/// player; the account name can. This is what lets a hero outlive its owner's
/// connection and be re-adopted on reconnect.
#[derive(Resource, Default)]
pub struct HeroIndex {
    pub by_name: HashMap<String, Entity>,
}

/// Spawn a hero for `owner`, terrain-snapped, and register it under `name`.
///
/// Deliberately NO `ControlledBy`: that component's lifetime would despawn the
/// hero when its owner disconnects, and a `ControlledBy` pointing at a
/// despawned client entity is a dangling reference. Ownership lives in
/// [`Hero::owner`], which is re-pointed when the player returns.
pub fn spawn_hero(
    commands: &mut Commands,
    index: &mut HeroIndex,
    terrain: &shared::terrain::WorldTerrain,
    owner: PeerId,
    name_lower: &str,
    display_name: &str,
    position: Vec3,
    rotation: f32,
    outfit: HeroOutfit,
) -> Entity {
    let grounded = Vec3::new(
        position.x,
        terrain.get_height(position.x, position.z),
        position.z,
    );
    let entity = commands
        .spawn((
            Hero { owner },
            // A player's hero takes the player's OWN name rather than a
            // generated one: that is the identity they already chose at login,
            // and the encyclopedia listing it under anything else would read as
            // a stranger.
            CharacterName(display_name.to_string()),
            CharacterKind::Hero,
            outfit,
            // Opt into region interest BEFORE the visibility pass runs.
            shared::region::RegionCoord::from_world_pos(grounded),
            PlayerPosition(grounded),
            PlayerRotation(rotation),
            Replicate::to_clients(NetworkTarget::All),
        ))
        .id();
    index.by_name.insert(name_lower.to_string(), entity);
    entity
}

/// Spawn a villager: a named person who lives in the world and belongs to
/// nobody.
///
/// A test tool until settlements produce their own population (ROADMAP Phase 3),
/// and deliberately NOT persisted -- world-state persistence does not exist yet,
/// so a villager lasts until the server restarts. That is honest for a spawn
/// button; the alternative is a villager who silently evaporates and looks like
/// a bug.
pub fn spawn_villager(
    commands: &mut Commands,
    terrain: &shared::terrain::WorldTerrain,
    seed: u64,
    position: Vec3,
) -> Entity {
    let grounded = Vec3::new(
        position.x,
        terrain.get_height(position.x, position.z),
        position.z,
    );
    // Deterministic in the seed, so the same villager keeps their name.
    let name = shared::names::person_name(seed);
    // Wardrobe varies with the same seed so a crowd is not identical twins.
    let outfit = HeroOutfit::varied(seed);
    commands
        .spawn((
            CharacterName(name),
            CharacterKind::Villager,
            outfit,
            shared::region::RegionCoord::from_world_pos(grounded),
            PlayerPosition(grounded),
            PlayerRotation(seed as f32 % std::f32::consts::TAU),
            Replicate::to_clients(NetworkTarget::All),
        ))
        .id()
}

/// Snapshot a hero for the profile.
pub fn hero_save(position: &PlayerPosition, rotation: &PlayerRotation, outfit: &HeroOutfit) -> HeroSave {
    HeroSave::from_parts(position.0, rotation.0, outfit)
}

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
    warp: Query<&shared::components::TimeWarp>,
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
    // Time warp scales movement too. Without this the world clock and the
    // strategic tick sped up while the hero kept walking at 1x, so god mode's
    // 100x button made everything EXCEPT the thing you were watching go faster.
    // The arrival clamp below is what keeps a huge step from overshooting.
    let factor = warp.iter().next().map(|w| w.0).unwrap_or(1.0);
    let dt = factor / shared::protocol::FIXED_TIMESTEP_HZ as f32;

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

    /// Time warp scales hero movement, and the arrival clamp is what makes that
    /// safe: a 100x step is far larger than the remaining distance, so without
    /// the clamp the hero would rocket past its target and oscillate forever.
    #[test]
    fn warped_steps_land_on_target_instead_of_overshooting() {
        let distance = 3.0_f32;
        for factor in [1.0_f32, 10.0, 100.0] {
            let dt = factor / shared::protocol::FIXED_TIMESTEP_HZ as f32;
            let step = (HERO_MOVE_SPEED * dt).min(distance);
            assert!(
                step <= distance,
                "at {factor}x the step {step} overshot the remaining {distance}"
            );
        }
        // And warp must actually make the hero faster, or the god-mode buttons
        // speed up the world while the thing you are watching crawls.
        let slow = HERO_MOVE_SPEED / shared::protocol::FIXED_TIMESTEP_HZ as f32;
        let fast = HERO_MOVE_SPEED * 100.0 / shared::protocol::FIXED_TIMESTEP_HZ as f32;
        assert!(fast > slow * 50.0, "warp did not scale movement");
    }

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
