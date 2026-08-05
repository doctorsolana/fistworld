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
    BuildingDoorUse, CharacterActivity, CharacterAffiliation, CharacterAttributes, CharacterKind,
    CharacterName, CommandedBy, Hero, HeroOutfit, Player, PlayerPosition, PlayerProgression,
    PlayerRotation,
};
use shared::player::{HERO_ARRIVE_EPSILON, HERO_MOVE_SPEED};
use shared::player_profile::HeroSave;
use shared::protocol::{UnitMoveOrder, MAX_UNITS_PER_ORDER};
use shared::region::RegionCoord;
use shared::spatial::SpatialObstacleGrid;
use shared::terrain::WorldTerrain;

use crate::collision::library::{DerivedColliderLibrary, StaticColliders};
use crate::world::navgrid::VILLAGER_PROP_RADIUS;
use crate::world::village_roads::{
    NavigationRouteFailed, NavigationRoutePending, TravelRoute, ROAD_SPEED_MULTIPLIER,
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
            // Your hero obeys you. Keyed by ACCOUNT, so a reconnect needs no
            // repair -- unlike `Hero::owner`, which holds a per-session peer id
            // and has to be re-pointed by hand every time you come back.
            CommandedBy(name_lower.to_string()),
            // Unaffiliated is the normal, permanent state -- not a gap.
            CharacterAffiliation::default(),
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
            CharacterAttributes::from_seed(seed),
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
) -> HeroSave {
    HeroSave::from_parts(position.0, rotation.0, outfit)
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
    units: Query<&CommandedBy, With<CharacterKind>>,
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
                let Ok(commanded) = units.get(*unit) else {
                    continue;
                };
                if commanded.0 != account {
                    continue;
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
            Option<&mut TravelRoute>,
            Option<&NavigationRoutePending>,
            Option<&NavigationRouteFailed>,
            Option<&BuildingDoorUse>,
            Option<&crate::world::village::PierTraversal>,
        ),
        (
            With<CharacterKind>,
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
        mut route,
        pending,
        failed,
        door_use,
        pier_traversal,
    ) in units.iter_mut()
    {
        if *kind == CharacterKind::Villager && (pending.is_some() || failed.is_some()) {
            continue;
        }
        let final_target = target.0;
        let mut current = Vec2::new(pos.0.x, pos.0.z);
        let final_goal = Vec2::new(final_target.x, final_target.z);
        let mut remaining_seconds = dt;
        let mut last_direction = None;
        let mut arrived = false;

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
            if *kind == CharacterKind::Villager
                && door_use.is_none()
                && pier_traversal.is_none()
                && !navigation_segment_clear(
                    current,
                    proposed,
                    obstacles.as_deref(),
                    colliders.as_deref(),
                    derived.as_deref(),
                )
            {
                commands
                    .entity(entity)
                    .remove::<TravelRoute>()
                    .insert(NavigationRoutePending::new(final_target));
                break;
            }
            current = proposed;
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
        if arrived {
            // Arrival is per UNIT now. Under the old peer-keyed map the first
            // unit to arrive removed the shared entry and silently cancelled
            // every other unit's order.
            commands
                .entity(entity)
                .remove::<MoveTarget>()
                .remove::<TravelRoute>()
                .remove::<NavigationRoutePending>();
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
