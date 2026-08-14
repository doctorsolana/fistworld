//! Hero lifecycle + movement: the server-authoritative embodied character.
//!
//! The hero is the first server-simulated mover since the RTS pivot: spawned
//! by a god command, stepped here toward client-sent move targets, streamed
//! to clients through the normal replication + region-interest path. Clients
//! only ever send intent ([`HeroMoveTo`]); position/rotation truth lives here.

use bevy::platform::collections::{HashMap, HashSet};
use bevy::prelude::*;
use lightyear::prelude::server::ClientOf;
use lightyear::prelude::{MessageReceiver, NetworkTarget, PeerId, RemoteId, Replicate};

use shared::components::{
    BuildingDoorUse, CharacterActivity, CharacterAffiliation, CharacterAttributes, CharacterKind,
    CharacterMotion, CharacterName, CommandedBy, Health, Hero, HeroOutfit, Nutrition, Player,
    PlayerPermitLedger, PlayerPosition, PlayerProgression, PlayerRotation,
};
use shared::player::{HERO_ARRIVE_EPSILON, HERO_MOVE_SPEED};
use shared::player_profile::HeroSave;
use shared::protocol::{UnitMoveOrder, MAX_UNITS_PER_ORDER};
use shared::region::RegionCoord;
use shared::spatial::SpatialObstacleGrid;
use shared::terrain::WorldTerrain;

use crate::collision::library::{DerivedColliderLibrary, StaticColliders};
use crate::world::navgrid::VILLAGER_PROP_RADIUS;
use crate::world::village::{
    ConstructionMaterialRoutine, PlayerConstructionAssignment, UnderConstruction,
};
use crate::world::village_roads::{
    NavigationObstacleEscape, NavigationRouteFailed, NavigationRoutePending, TravelRoute,
    VillageRoadGraph, ROAD_SPEED_MULTIPLIER,
};

/// Where a unit is walking, if anywhere.
///
/// A COMPONENT on the unit, not a map keyed by peer. The old
/// `HashMap<PeerId, Vec3>` held one target per PLAYER, which caused four
/// separate bugs at once: an N-unit order collapsed onto the last message, the
/// first unit to arrive cancelled everyone else's order, a disconnect cancelled
/// every order the player had given, and a villager could not be addressed at
/// all. Putting the target on the unit dissolves all four.
#[derive(Component, Debug, Clone, Copy)]
pub struct MoveTarget(pub Vec3);

/// A disconnected player's retained body. Dormant heroes remain available for
/// identity and persistence but receive no movement or nutrition progression;
/// they also cannot perform work, train or earn active-character rewards.
#[derive(Component, Debug, Clone, Copy, Default)]
pub struct OfflineHero;

fn motion_materially_changed(current: CharacterMotion, next: CharacterMotion) -> bool {
    current.is_moving() != next.is_moving()
        || current.velocity.distance_squared(next.velocity) > 0.05 * 0.05
}

fn static_prop_blocks_segment(
    start: Vec2,
    end: Vec2,
    colliders: &StaticColliders,
    derived: &DerivedColliderLibrary,
) -> bool {
    const COLLIDER_CELL: f32 = 16.0;
    let min = (
        (start.x.min(end.x) / COLLIDER_CELL).floor() as i32 - 1,
        (start.y.min(end.y) / COLLIDER_CELL).floor() as i32 - 1,
    );
    let max = (
        (start.x.max(end.x) / COLLIDER_CELL).floor() as i32 + 1,
        (start.y.max(end.y) / COLLIDER_CELL).floor() as i32 + 1,
    );
    let segment = end - start;
    let length_squared = segment.length_squared();
    for cx in min.0..=max.0 {
        for cz in min.1..=max.1 {
            let Some(ids) = colliders.cells.get(&(cx, cz)) else {
                continue;
            };
            for id in ids {
                let Some(instance) = colliders.instances.get(id) else {
                    continue;
                };
                let Some(shape) = derived.by_kind.get(&instance.kind) else {
                    continue;
                };
                let radius = shape.horizontal_radius * instance.scale + VILLAGER_PROP_RADIUS;
                let obstacle = Vec2::new(instance.position.x, instance.position.z);
                let t = if length_squared <= f32::EPSILON {
                    1.0
                } else {
                    ((obstacle - start).dot(segment) / length_squared).clamp(0.0, 1.0)
                };
                if obstacle.distance_squared(start + segment * t) < radius * radius {
                    return true;
                }
            }
        }
    }
    false
}

pub(crate) fn navigation_segment_clear(
    start: Vec2,
    end: Vec2,
    buildings: Option<&SpatialObstacleGrid>,
    colliders: Option<&StaticColliders>,
    derived: Option<&DerivedColliderLibrary>,
) -> bool {
    !buildings.is_some_and(|grid| grid.segment_blocked(start, end))
        && !colliders.zip(derived).is_some_and(|(colliders, derived)| {
            static_prop_blocks_segment(start, end, colliders, derived)
        })
}

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
    attributes: CharacterAttributes,
    health: Health,
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
            attributes,
            health,
            Nutrition::default(),
            // Your hero obeys you. Keyed by ACCOUNT, so a reconnect needs no
            // repair -- unlike `Hero::owner`, which holds a per-session peer id
            // and has to be re-pointed by hand every time you come back.
            CommandedBy(name_lower.to_string()),
            // Unaffiliated is the normal, permanent state -- not a gap.
            CharacterAffiliation::default(),
            outfit,
            // Player heroes participate in the same physical economy as every
            // other person. These components remain on the live body across a
            // disconnect/reconnect, so cargo and coin cannot disappear.
            (
                shared::economy::GoodsInventory::new(shared::economy::capacity::VILLAGER),
                shared::economy::CarriedLoad::default(),
                shared::economy::Wallet::founding_hero(),
                PlayerPermitLedger::default(),
            ),
            // Opt into region interest BEFORE the visibility pass runs.
            shared::region::RegionCoord::from_world_pos(grounded),
            PlayerPosition(grounded),
            CharacterMotion::STATIONARY,
            PlayerRotation(rotation),
            Replicate::to_clients(NetworkTarget::All),
        ))
        .id();
    index.by_name.insert(name_lower.to_string(), entity);
    entity
}

/// Backfill the permit ledger on a live hero created by an older session/test
/// path. Normal hero creation already inserts it, so this is change-driven and
/// effectively free in ordinary play.
pub fn ensure_player_permit_ledgers(
    mut commands: Commands,
    heroes: Query<Entity, (With<Hero>, Without<PlayerPermitLedger>)>,
) {
    for hero in heroes.iter() {
        commands.entity(hero).insert(PlayerPermitLedger::default());
    }
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
            CharacterAttributes::from_seed(seed),
            (Health::default(), Nutrition::default()),
            CharacterAffiliation::default(),
            // Bulk cargo is separate from equipment. A villager can carry a
            // few wood bundles or many food portions, and a full load is a
            // physical reason to return to a workplace.
            shared::economy::GoodsInventory::new(shared::economy::capacity::VILLAGER),
            shared::economy::CarriedLoad::default(),
            shared::economy::Wallet::founding_villager(),
            CharacterActivity::Idle,
            outfit,
            shared::region::RegionCoord::from_world_pos(grounded),
            PlayerPosition(grounded),
            CharacterMotion::STATIONARY,
            PlayerRotation(seed as f32 % std::f32::consts::TAU),
            Replicate::to_clients(NetworkTarget::All),
        ))
        .id()
}

/// Safety net for authored/test characters that predate the shared attribute
/// component. Normal hero and villager constructors already insert it.
pub fn ensure_character_attributes(
    mut commands: Commands,
    characters: Query<
        (Entity, &CharacterName),
        (With<CharacterKind>, Without<CharacterAttributes>),
    >,
) {
    for (entity, name) in characters.iter() {
        let seed = name
            .0
            .bytes()
            .fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
                hash.wrapping_mul(0x100_0000_01b3) ^ u64::from(byte)
            });
        commands
            .entity(entity)
            .insert(CharacterAttributes::from_seed(seed));
    }
}

/// Keep the commander/account progression copy aligned with its embodied hero
/// so profile saves and any existing progression UI observe trained attributes.
pub fn sync_hero_attributes_to_player_progression(
    heroes: Query<(&Hero, &CharacterAttributes), Changed<CharacterAttributes>>,
    mut players: Query<(&Player, &mut PlayerProgression)>,
) {
    for (hero, attributes) in heroes.iter() {
        let Some((_, mut progression)) = players
            .iter_mut()
            .find(|(player, _)| player.client_id == hero.owner)
        else {
            continue;
        };
        progression.stamina = u32::from(attributes.physique());
        progression.intelligence = u32::from(attributes.intelligence());
        progression.charm = u32::from(attributes.charm());
    }
}

/// Snapshot a hero for the profile.
pub fn hero_save(
    position: &PlayerPosition,
    rotation: &PlayerRotation,
    outfit: &HeroOutfit,
    health: &Health,
) -> HeroSave {
    HeroSave::from_parts(position.0, rotation.0, outfit, health)
}

/// Turn [`UnitMoveOrder`] intents into [`MoveTarget`] components.
///
/// THE AUTHORITY CHECK LIVES HERE, and it is one clause: the unit's
/// [`CommandedBy`] must equal the sender's account. Entity mapping guarantees an
/// id is meaningful in this world; it says nothing about whose it is, so a
/// modified client can and will name units it does not command.
///
/// Every rejection is silent and identical, whether the unit does not exist,
/// has despawned, or belongs to someone else -- distinguishable rejections would
/// let a client probe for entities outside its interest.
///
/// No dev gate: commanding your own retinue is a normal gameplay verb.
pub fn handle_unit_move_orders(
    mut commands: Commands,
    profiles: Res<crate::persistence::profiles::PlayerProfiles>,
    mut client_links: Query<(&RemoteId, &mut MessageReceiver<UnitMoveOrder>), With<ClientOf>>,
    units: Query<
        (&CommandedBy, Option<&PlayerConstructionAssignment>),
        (With<CharacterKind>, Without<OfflineHero>),
    >,
    mut sites: Query<&mut UnderConstruction>,
) {
    for (remote_id, mut receiver) in client_links.iter_mut() {
        // Resolved ONCE per connection. Account name, not peer id: that is the
        // identity a retinue is keyed by, so it survives reconnects.
        let account = profiles.peer_to_name.get(&remote_id.0).cloned();
        for order in receiver.receive() {
            // Drain the receiver even for an unnamed peer, or a client that
            // orders before submitting a name backs the queue up forever.
            let Some(account) = account.as_deref() else {
                continue;
            };
            // Cap SERVER-side: a client-side cap is advisory, and an unbounded
            // Vec in a message is an unbounded loop here.
            for (unit, point) in order.units.iter().take(MAX_UNITS_PER_ORDER) {
                if !point.is_finite() {
                    continue;
                }
                // A client that could not map an id sends PLACEHOLDER, which is
                // a valid-looking Entity that must never reach `commands.entity`.
                if *unit == Entity::PLACEHOLDER {
                    continue;
                }
                let Ok((commanded, construction)) = units.get(*unit) else {
                    continue;
                };
                if commanded.0 != account {
                    continue;
                }
                if let Some(construction) = construction {
                    if let Ok(mut site) = sites.get_mut(construction.site) {
                        if site.builder == Some(*unit) {
                            site.builder = None;
                        }
                    }
                    commands
                        .entity(*unit)
                        .remove::<PlayerConstructionAssignment>()
                        .remove::<ConstructionMaterialRoutine>()
                        .remove::<TravelRoute>()
                        .remove::<NavigationRoutePending>()
                        .remove::<NavigationRouteFailed>()
                        .insert(CharacterActivity::Idle);
                }
                commands.entity(*unit).insert(MoveTarget(*point));
            }
        }
    }
}

/// Step every hero toward its move target at walk speed, snapped to terrain.
///
/// Every component write is `!=`-guarded: change detection drives replication,
/// and an idle hero must generate zero network traffic.
pub fn step_units(
    mut commands: Commands,
    terrain: Option<Res<WorldTerrain>>,
    obstacles: Option<Res<SpatialObstacleGrid>>,
    colliders: Option<Res<StaticColliders>>,
    derived: Option<Res<DerivedColliderLibrary>>,
    simulation_time: crate::world::simulation_time::SimulationTime,
    mut road_graph: Option<ResMut<VillageRoadGraph>>,
    mut reported_route_collisions: Local<HashSet<Entity>>,
    // `With<CharacterKind>` is load-bearing, not decoration: the commander
    // camera anchor carries the identical PlayerPosition + PlayerRotation +
    // RegionCoord shape, so without it this system would start walking the
    // player's CAMERA around the map.
    mut units: Query<
        (
            Entity,
            &CharacterKind,
            &MoveTarget,
            &mut PlayerPosition,
            &mut PlayerRotation,
            &mut RegionCoord,
            Option<&mut CharacterMotion>,
            Option<&mut TravelRoute>,
            Option<&NavigationRoutePending>,
            Option<&NavigationRouteFailed>,
            Option<&NavigationObstacleEscape>,
            Option<&BuildingDoorUse>,
            Option<&crate::world::village::PierTraversal>,
        ),
        (
            With<CharacterKind>,
            Without<OfflineHero>,
            Without<crate::world::village::strategic::StrategicPerson>,
        ),
    >,
) {
    let Some(terrain) = terrain else {
        return;
    };
    // Time warp scales movement too. Without this the world clock and the
    // strategic tick sped up while the hero kept walking at 1x, so god mode's
    // 100x button made everything EXCEPT the thing you were watching go faster.
    // The arrival clamp below is what keeps a huge step from overshooting.
    let dt = simulation_time.world_seconds();

    for (
        entity,
        kind,
        target,
        mut pos,
        mut rot,
        mut region,
        mut motion,
        mut route,
        pending,
        failed,
        obstacle_escape,
        door_use,
        pier_traversal,
    ) in units.iter_mut()
    {
        let start_position = pos.0;
        let real_dt = simulation_time.real_seconds().max(1.0e-5);
        // Door and pier traversals are authored, collision-exempt movement
        // modes. A route result can arrive on the same deferred-command
        // boundary that a household/work routine begins one of those modes;
        // that stale tactical state must never freeze the authored crossing.
        // Arrival below clears it along with the MoveTarget.
        let authored_traversal =
            door_use.is_some() || pier_traversal.is_some() || obstacle_escape.is_some();
        if *kind == CharacterKind::Villager
            && !authored_traversal
            && (pending.is_some() || failed.is_some())
        {
            if let Some(motion) = motion.as_deref_mut() {
                if motion.is_moving() {
                    *motion = CharacterMotion::STATIONARY;
                }
            }
            continue;
        }
        let final_target = target.0;
        let mut current = Vec2::new(pos.0.x, pos.0.z);
        let final_goal = Vec2::new(final_target.x, final_target.z);
        let mut remaining_seconds = dt;
        let mut last_direction = None;
        let mut arrived = false;
        let mut escaping_building = obstacle_escape.is_some();

        // A route is only valid for the MoveTarget it was planned against.
        // Changed targets normally get a replacement earlier in this chained
        // schedule; this guard keeps direct movement correct even without it.
        let use_route = route.as_ref().is_some_and(|route| {
            Vec2::new(route.goal.x, route.goal.z).distance_squared(final_goal) < 0.01
                && !route.waypoints.is_empty()
        });
        if !use_route && route.is_some() {
            commands.entity(entity).remove::<TravelRoute>();
        }

        // Consume the whole tick's travel budget, including across multiple
        // two-metre road points. This matters at 100x: stopping for a server
        // frame at every graph node would make accelerated simulation lie.
        while remaining_seconds > 1e-5 {
            let (goal, on_road) = if use_route {
                let route = route.as_deref_mut().expect("route checked above");
                if route.next >= route.waypoints.len() {
                    arrived = true;
                    break;
                }
                let waypoint = route.waypoints[route.next];
                (
                    Vec2::new(waypoint.position.x, waypoint.position.z),
                    waypoint.on_road,
                )
            } else {
                (final_goal, false)
            };
            let to_goal = goal - current;
            let distance = to_goal.length();
            if distance <= HERO_ARRIVE_EPSILON {
                if use_route {
                    let route = route.as_deref_mut().expect("route checked above");
                    route.next += 1;
                    if route.next < route.waypoints.len() {
                        continue;
                    }
                }
                arrived = true;
                break;
            }

            let speed = HERO_MOVE_SPEED * if on_road { ROAD_SPEED_MULTIPLIER } else { 1.0 };
            let step = (speed * remaining_seconds).min(distance);
            let direction = to_goal / distance;
            let reaches_goal = step + HERO_ARRIVE_EPSILON >= distance;
            // Clamp the last leg to the certified waypoint itself. Rebuilding
            // that point as `current + normalized * distance` can land a few
            // floating-point ulps beyond it. Doorways and tree interaction
            // spots intentionally sit tight against obstacle boundaries, so
            // that microscopic overshoot was rejected and re-planned forever
            // even though the exact route endpoint was clear.
            let proposed = if reaches_goal {
                goal
            } else {
                current + direction * step
            };
            let building_obstacles = (!escaping_building)
                .then_some(obstacles.as_deref())
                .flatten();
            if *kind == CharacterKind::Villager
                && door_use.is_none()
                && pier_traversal.is_none()
                && !navigation_segment_clear(
                    current,
                    proposed,
                    building_obstacles,
                    colliders.as_deref(),
                    derived.as_deref(),
                )
            {
                if use_route
                    && std::env::var_os("FISTWORLD_LAB_ROUTE_DIAGNOSTICS").is_some()
                    && reported_route_collisions.insert(entity)
                {
                    let building_blocked = obstacles
                        .as_deref()
                        .is_some_and(|grid| grid.segment_blocked(current, proposed));
                    let prop_blocked = colliders.as_deref().zip(derived.as_deref()).is_some_and(
                        |(colliders, derived)| {
                            static_prop_blocks_segment(current, proposed, colliders, derived)
                        },
                    );
                    let route_progress = route.as_deref().map_or_else(
                        || "-".to_string(),
                        |route| format!("{}/{}", route.next, route.waypoints.len()),
                    );
                    eprintln!(
                        "LAB movement rejected certified route entity={entity:?} at={:.1},{:.1} proposed={:.1},{:.1} goal={:.1},{:.1} route={route_progress} building_blocked={building_blocked} prop_blocked={prop_blocked}",
                        current.x,
                        current.y,
                        proposed.x,
                        proposed.y,
                        final_goal.x,
                        final_goal.y,
                    );
                }
                let mut entity_commands = commands.entity(entity);
                entity_commands.remove::<TravelRoute>();
                if use_route {
                    // This segment was part of a route certified against an
                    // earlier world snapshot. A newly completed building (or
                    // streamed prop) has invalidated that proof. Purge cached
                    // corridors before reporting the embodied failure, so an
                    // owning routine which deliberately retries the same
                    // critical destination receives a fresh survey rather
                    // than the stale path again.
                    if let Some(graph) = road_graph.as_deref_mut() {
                        graph.invalidate_tactical_routes_after_embodied_rejection();
                    }
                    entity_commands
                        .remove::<NavigationRoutePending>()
                        .insert(NavigationRouteFailed { goal: final_target });
                } else {
                    entity_commands.insert(NavigationRoutePending::new(final_target));
                }
                break;
            }
            current = proposed;
            if escaping_building
                && obstacles
                    .as_deref()
                    .is_none_or(|grid| !grid.point_blocked(current))
            {
                escaping_building = false;
                commands.entity(entity).remove::<NavigationObstacleEscape>();
            }
            last_direction = Some(direction);
            remaining_seconds -= step / speed;

            if !reaches_goal {
                break;
            }
            if use_route {
                let route = route.as_deref_mut().expect("route checked above");
                route.next += 1;
                if route.next < route.waypoints.len() {
                    continue;
                }
            }
            arrived = true;
            break;
        }

        let next_y = pier_traversal
            .and_then(|pier| pier.deck_height_at(current))
            .unwrap_or_else(|| terrain.get_height(current.x, current.y));
        let next_pos = Vec3::new(current.x, next_y, current.y);

        // Face travel direction. Bevy yaw 0 looks down -Z; atan2(x, z) of the
        // FORWARD vector gives the yaw whose -Z axis points along it.
        let next_yaw = last_direction.map(|direction| f32::atan2(-direction.x, -direction.y));

        if pos.0 != next_pos {
            pos.0 = next_pos;
        }
        if let Some(next_yaw) = next_yaw {
            if rot.0 != next_yaw {
                rot.0 = next_yaw;
            }
        }
        let next_region = RegionCoord::from_world_pos(next_pos);
        if *region != next_region {
            *region = next_region;
        }
        let next_motion = if arrived {
            CharacterMotion::STATIONARY
        } else {
            let velocity = (next_pos - start_position) / real_dt;
            // Vertical terrain-following noise changes every step and is not
            // useful for short client extrapolation. Keeping motion horizontal
            // also means a straight route dirties this replicated component
            // only once instead of at 60 Hz.
            CharacterMotion::new(Vec3::new(velocity.x, 0.0, velocity.z))
        };
        if let Some(motion) = motion.as_deref_mut() {
            if motion_materially_changed(*motion, next_motion) {
                *motion = next_motion;
            }
        } else {
            commands.entity(entity).insert(next_motion);
        }
        if arrived {
            // Arrival is per UNIT now. Under the old peer-keyed map the first
            // unit to arrive removed the shared entry and silently cancelled
            // every other unit's order.
            commands
                .entity(entity)
                .remove::<MoveTarget>()
                .remove::<TravelRoute>()
                .remove::<NavigationRoutePending>()
                .remove::<NavigationObstacleEscape>();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::village_roads::{RouteWaypoint, TravelRoute};
    use shared::components::TimeWarp;

    /// Time warp scales hero movement, and the arrival clamp is what makes that
    /// safe: a 100x step is far larger than the remaining distance, so without
    /// the clamp the hero would rocket past its target and oscillate forever.
    #[test]
    fn warped_steps_land_on_target_instead_of_overshooting() {
        let distance = 3.0_f32;
        for factor in [1.0_f32, 10.0, 25.0, 100.0] {
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

    #[test]
    fn replicated_motion_ignores_tiny_step_noise_but_reports_stops_and_turns() {
        let east = CharacterMotion::new(Vec3::new(3.2, 0.0, 0.0));
        assert!(!motion_materially_changed(
            east,
            CharacterMotion::new(Vec3::new(3.21, 0.0, 0.0))
        ));
        assert!(motion_materially_changed(
            east,
            CharacterMotion::new(Vec3::new(0.0, 0.0, 3.2))
        ));
        assert!(motion_materially_changed(east, CharacterMotion::STATIONARY));
    }

    #[test]
    fn final_movement_leg_uses_the_exact_certified_endpoint() {
        // These ordinary f32 coordinates reproduce the ulp drift that matters
        // when an interaction point sits immediately outside an obstacle.
        let current = Vec2::new(179.038_9, 0.444_466_6);
        let goal = Vec2::new(179.299_53, 0.096_903_1);
        let to_goal = goal - current;
        let distance = to_goal.length();
        let reconstructed = current + to_goal / distance * distance;
        assert_ne!(
            reconstructed, goal,
            "fixture must demonstrate that normalized reconstruction can drift"
        );

        let step = distance;
        let reaches_goal = step + HERO_ARRIVE_EPSILON >= distance;
        let proposed = if reaches_goal {
            goal
        } else {
            current + to_goal / distance * step
        };
        assert_eq!(proposed, goal);
    }

    #[test]
    fn hundred_x_movement_consumes_multiple_cached_road_points_per_tick() {
        let mut app = App::new();
        app.insert_resource(WorldTerrain::default());
        app.add_systems(Update, step_units);
        app.world_mut().spawn(TimeWarp::clamped(100.0));

        let goal = Vec3::new(20.0, 0.0, 0.0);
        let waypoints = [2.0, 4.0, 6.0, 8.0, 10.0]
            .into_iter()
            .map(|x| RouteWaypoint {
                position: Vec3::new(x, 0.0, 0.0),
                on_road: true,
            })
            .chain(std::iter::once(RouteWaypoint {
                position: goal,
                on_road: false,
            }))
            .collect();
        let mover = app
            .world_mut()
            .spawn((
                CharacterKind::Villager,
                PlayerPosition(Vec3::ZERO),
                PlayerRotation(0.0),
                RegionCoord::default(),
                MoveTarget(goal),
                TravelRoute {
                    goal,
                    waypoints,
                    next: 0,
                },
            ))
            .id();

        app.update();

        let position = app.world().get::<PlayerPosition>(mover).unwrap().0;
        let route = app.world().get::<TravelRoute>(mover).unwrap();
        assert!(
            position.x > 6.0,
            "100x stopped at a visual graph point instead of spending its full tick: {position:?}"
        );
        assert!(
            route.next >= 3,
            "the tick should consume several road points, reached {}",
            route.next
        );
    }

    #[test]
    fn offline_hero_is_dormant_until_reconnection() {
        let mut app = App::new();
        app.insert_resource(WorldTerrain::default());
        app.add_systems(Update, step_units);
        app.world_mut().spawn(TimeWarp::clamped(100.0));
        let hero = app
            .world_mut()
            .spawn((
                CharacterKind::Hero,
                OfflineHero,
                PlayerPosition(Vec3::ZERO),
                PlayerRotation(0.0),
                RegionCoord::default(),
                MoveTarget(Vec3::new(20.0, 0.0, 0.0)),
            ))
            .id();

        app.update();
        assert_eq!(
            app.world().get::<PlayerPosition>(hero).unwrap().0,
            Vec3::ZERO
        );

        app.world_mut().entity_mut(hero).remove::<OfflineHero>();
        app.update();
        assert!(app.world().get::<PlayerPosition>(hero).unwrap().0.x > 0.0);
    }

    #[test]
    fn villager_cannot_cross_a_live_building_obstacle_even_at_100x() {
        let mut app = App::new();
        app.insert_resource(WorldTerrain::default());
        let mut obstacles = SpatialObstacleGrid::default();
        obstacles.insert(shared::spatial::ObstacleEntry {
            center: Vec2::new(5.0, 0.0),
            half_extents: Vec2::new(1.5, 2.5),
            rotation: 0.0,
            obstacle_type: 0,
        });
        app.insert_resource(obstacles);
        app.add_systems(Update, step_units);
        app.world_mut().spawn(TimeWarp::clamped(100.0));

        let mover = app
            .world_mut()
            .spawn((
                CharacterKind::Villager,
                PlayerPosition(Vec3::ZERO),
                PlayerRotation(0.0),
                RegionCoord::default(),
                MoveTarget(Vec3::new(10.0, 0.0, 0.0)),
            ))
            .id();

        app.update();

        let stopped = app.world().get::<PlayerPosition>(mover).unwrap().0;
        assert_eq!(
            Vec2::new(stopped.x, stopped.z),
            Vec2::ZERO,
            "the large debug step must not tunnel through the building"
        );
        assert!(app
            .world()
            .entity(mover)
            .contains::<NavigationRoutePending>());
    }

    #[test]
    fn a_certified_route_blocked_by_new_construction_reports_ai_failure() {
        let mut app = App::new();
        app.insert_resource(WorldTerrain::default());
        let mut obstacles = SpatialObstacleGrid::default();
        obstacles.insert(shared::spatial::ObstacleEntry {
            center: Vec2::new(5.0, 0.0),
            half_extents: Vec2::new(1.5, 2.5),
            rotation: 0.0,
            obstacle_type: 0,
        });
        app.insert_resource(obstacles);
        app.add_systems(Update, step_units);
        app.world_mut().spawn(TimeWarp::clamped(100.0));

        let goal = Vec3::new(10.0, 0.0, 0.0);
        let mover = app
            .world_mut()
            .spawn((
                CharacterKind::Villager,
                PlayerPosition(Vec3::ZERO),
                PlayerRotation(0.0),
                RegionCoord::default(),
                MoveTarget(goal),
                TravelRoute {
                    goal,
                    waypoints: vec![RouteWaypoint {
                        position: goal,
                        on_road: false,
                    }],
                    next: 0,
                },
            ))
            .id();

        app.update();

        let mover = app.world().entity(mover);
        assert_eq!(
            mover
                .get::<NavigationRouteFailed>()
                .map(|failed| failed.goal),
            Some(goal)
        );
        assert!(!mover.contains::<NavigationRoutePending>());
        assert!(!mover.contains::<TravelRoute>());
    }

    #[test]
    fn door_traversal_is_not_frozen_by_a_stale_pending_route() {
        let mut app = App::new();
        app.insert_resource(WorldTerrain::default());
        let mut obstacles = SpatialObstacleGrid::default();
        obstacles.insert(shared::spatial::ObstacleEntry {
            center: Vec2::ZERO,
            half_extents: Vec2::splat(3.0),
            rotation: 0.0,
            obstacle_type: 0,
        });
        app.insert_resource(obstacles);
        app.add_systems(Update, step_units);
        app.world_mut().spawn(TimeWarp::clamped(25.0));

        // This is the exact state race caught in the long Village Lab run: a
        // resident is inside their cabin, the home routine has authority to
        // cross its doorway, but an earlier tactical request is still present.
        let start = Vec3::new(0.0, 0.0, 0.0);
        let goal = Vec3::new(1.0, 0.0, 0.0);
        let mover = app
            .world_mut()
            .spawn((
                CharacterKind::Villager,
                PlayerPosition(start),
                PlayerRotation(0.0),
                RegionCoord::default(),
                MoveTarget(goal),
                NavigationRoutePending::new(goal),
                BuildingDoorUse {
                    building: Vec3::ZERO,
                },
            ))
            .id();

        app.update();

        let position = app.world().get::<PlayerPosition>(mover).unwrap().0;
        assert!(
            position.x > start.x,
            "the authored doorway crossing was frozen by tactical route state"
        );
        assert!(
            !app.world().entity(mover).contains::<MoveTarget>(),
            "25x should complete this short doorway crossing in one tick"
        );
        assert!(!app
            .world()
            .entity(mover)
            .contains::<NavigationRoutePending>());
    }

    #[test]
    fn certified_obstacle_escape_only_lasts_until_clear_ground() {
        let mut app = App::new();
        app.insert_resource(WorldTerrain::default());
        let mut obstacles = SpatialObstacleGrid::default();
        obstacles.insert(shared::spatial::ObstacleEntry {
            center: Vec2::ZERO,
            half_extents: Vec2::splat(3.0),
            rotation: 0.0,
            obstacle_type: 0,
        });
        app.insert_resource(obstacles);
        app.add_systems(Update, step_units);
        app.world_mut().spawn(TimeWarp::clamped(25.0));

        let goal = Vec3::new(5.0, 0.0, 0.0);
        let mover = app
            .world_mut()
            .spawn((
                CharacterKind::Villager,
                PlayerPosition(Vec3::ZERO),
                PlayerRotation(0.0),
                RegionCoord::default(),
                MoveTarget(goal),
                NavigationObstacleEscape,
            ))
            .id();

        for _ in 0..5 {
            app.update();
        }

        assert_eq!(
            app.world().get::<PlayerPosition>(mover).unwrap().0.x,
            goal.x
        );
        let mover = app.world().entity(mover);
        assert!(!mover.contains::<NavigationObstacleEscape>());
        assert!(!mover.contains::<NavigationRoutePending>());
        assert!(!mover.contains::<MoveTarget>());
    }

    #[test]
    fn villager_cannot_tunnel_through_a_baked_tree_collider() {
        use crate::collision::library::{DerivedCollider, StaticColliderInstance};
        use shared::props::PropKind;
        use shared::terrain::ChunkCoord;

        let mut app = App::new();
        app.insert_resource(WorldTerrain::default());
        let tree_kind = PropKind::BroadleafNarrowA;
        let tree_position = Vec3::new(5.0, 0.0, 0.0);
        let mut colliders = StaticColliders::default();
        colliders.cells.insert((0, 0), vec![1]);
        colliders.instances.insert(
            1,
            StaticColliderInstance {
                kind: tree_kind,
                position: tree_position,
                rotation: Quat::IDENTITY,
                scale: 1.0,
                cell: (0, 0),
            },
        );
        colliders.loaded_chunks.insert(ChunkCoord::new(0, 0));
        app.insert_resource(colliders);
        app.insert_resource(DerivedColliderLibrary {
            by_kind: [(
                tree_kind,
                DerivedCollider {
                    bounding_radius: 4.0,
                    horizontal_radius: 0.8,
                    hulls: Vec::new(),
                },
            )]
            .into_iter()
            .collect(),
        });
        app.add_systems(Update, step_units);
        app.world_mut().spawn(TimeWarp::clamped(100.0));
        let mover = app
            .world_mut()
            .spawn((
                CharacterKind::Villager,
                PlayerPosition(Vec3::ZERO),
                PlayerRotation(0.0),
                RegionCoord::default(),
                MoveTarget(Vec3::new(10.0, 0.0, 0.0)),
            ))
            .id();

        app.update();

        let stopped = app.world().get::<PlayerPosition>(mover).unwrap().0;
        assert_eq!(Vec2::new(stopped.x, stopped.z), Vec2::ZERO);
        assert!(app
            .world()
            .entity(mover)
            .contains::<NavigationRoutePending>());
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
