//! Hero lifecycle + movement: the server-authoritative embodied character.
//!
//! The hero is the first server-simulated mover since the RTS pivot: spawned
//! by a god command, stepped here toward client-sent move targets, streamed
//! to clients through the normal replication + region-interest path. Clients
//! only ever send intent ([`shared::protocol::UnitOrder`]); position/rotation truth lives here.

use bevy::platform::collections::{HashMap, HashSet};
use bevy::prelude::*;
use lightyear::prelude::{NetworkTarget, PeerId, Replicate};

use shared::components::{
    AboardBoat, BuildingDoorUse, CharacterActivity, CharacterAffiliation, CharacterAttributes,
    CharacterKind, CharacterMotion, CharacterName, CommandedBy, Health, Hero, HeroOutfit,
    Nutrition, Player, PlayerPermitLedger, PlayerPosition, PlayerProgression, PlayerRotation,
};
use shared::player::{HERO_ARRIVE_EPSILON, HERO_MOVE_SPEED};
use shared::player_profile::HeroSave;
use shared::region::RegionCoord;
use shared::spatial::SpatialObstacleGrid;
use shared::terrain::WorldTerrain;

use crate::collision::library::{DerivedColliderLibrary, StaticColliders};
use crate::world::navgrid::VILLAGER_PROP_RADIUS;
use crate::world::village::MootQueueTicket;
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

const CROWD_CELL_SIZE: f32 = 2.5;
const CROWD_SEPARATION_RADIUS: f32 = 1.25;
const MAX_CROWD_NEIGHBORS: usize = 12;

#[derive(Clone, Copy)]
struct CrowdAgent {
    entity: Entity,
    point: Vec2,
}

/// Rebuilt once per navigation frame and shared by every mover. Local crowd
/// flavour is therefore O(n) to index and bounded per actor, rather than an
/// O(n²) all-pairs avoidance pass. It changes only the embodied steering; the
/// authoritative task, route and destination remain untouched.
#[derive(Resource, Default)]
pub struct TacticalCrowdGrid {
    cells: HashMap<(i32, i32), Vec<CrowdAgent>>,
}

fn crowd_cell(point: Vec2) -> (i32, i32) {
    (
        (point.x / CROWD_CELL_SIZE).floor() as i32,
        (point.y / CROWD_CELL_SIZE).floor() as i32,
    )
}

pub fn rebuild_tactical_crowd_grid(
    mut grid: Option<ResMut<TacticalCrowdGrid>>,
    movers: Query<
        (),
        (
            With<MoveTarget>,
            With<CharacterKind>,
            Without<OfflineHero>,
            Without<crate::world::village::strategic::StrategicPerson>,
        ),
    >,
    people: Query<
        (Entity, &PlayerPosition),
        (
            With<CharacterKind>,
            Without<OfflineHero>,
            Without<crate::world::village::strategic::StrategicPerson>,
        ),
    >,
) {
    let Some(grid) = grid.as_deref_mut() else {
        return;
    };
    // Only movers consult the grid; an idle town pays nothing for crowd
    // flavour instead of re-indexing every standing villager 60x a second.
    if movers.is_empty() {
        if !grid.cells.is_empty() {
            grid.cells.clear();
        }
        return;
    }
    grid.cells.clear();
    for (entity, position) in people.iter() {
        let point = Vec2::new(position.0.x, position.0.z);
        grid.cells
            .entry(crowd_cell(point))
            .or_default()
            .push(CrowdAgent { entity, point });
    }
}

fn separated_direction(
    grid: &TacticalCrowdGrid,
    entity: Entity,
    current: Vec2,
    preferred: Vec2,
) -> Vec2 {
    let cell = crowd_cell(current);
    let mut separation = Vec2::ZERO;
    let mut neighbors = 0usize;
    for dx in -1..=1 {
        for dz in -1..=1 {
            let Some(agents) = grid.cells.get(&(cell.0 + dx, cell.1 + dz)) else {
                continue;
            };
            for other in agents {
                if other.entity == entity {
                    continue;
                }
                let delta = current - other.point;
                let distance_squared = delta.length_squared();
                if distance_squared >= CROWD_SEPARATION_RADIUS * CROWD_SEPARATION_RADIUS {
                    continue;
                }
                let away = if distance_squared > 1.0e-5 {
                    delta / distance_squared.sqrt()
                } else {
                    // Give exact overlaps a stable, pair-symmetric direction
                    // so a burst fans out instead of choosing one shared side.
                    let a = entity.to_bits().min(other.entity.to_bits());
                    let b = entity.to_bits().max(other.entity.to_bits());
                    let mixed = a
                        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
                        .wrapping_add(b.rotate_left(23));
                    let angle = (mixed as u32) as f32 / u32::MAX as f32 * std::f32::consts::TAU;
                    let direction = Vec2::new(angle.cos(), angle.sin());
                    if entity.to_bits() == a {
                        direction
                    } else {
                        -direction
                    }
                };
                let strength =
                    1.0 - (distance_squared.sqrt() / CROWD_SEPARATION_RADIUS).clamp(0.0, 1.0);
                separation += away * strength;
                neighbors += 1;
                if neighbors >= MAX_CROWD_NEIGHBORS {
                    break;
                }
            }
            if neighbors >= MAX_CROWD_NEIGHBORS {
                break;
            }
        }
        if neighbors >= MAX_CROWD_NEIGHBORS {
            break;
        }
    }
    if separation.length_squared() <= 1.0e-5 {
        return preferred;
    }
    (preferred + separation.normalize() * 0.35).normalize_or_zero()
}

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
            // Activity is part of the standard character contract: systems
            // whose queries demand it (combat, animation-facing writes) must
            // match a hero however it was created - fresh spawn, profile
            // restore, dev spawn. Only the opening voyage overwrites this
            // (with Sitting) after the fact.
            (CharacterMotion::STATIONARY, CharacterActivity::default()),
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
    crowd_grid: Option<Res<TacticalCrowdGrid>>,
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
            Has<MootQueueTicket>,
            Has<crate::world::village::ambient::AmbientDirectTransit>,
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
    let current_geometry_version = crate::world::village_roads::navigation_geometry_version(
        obstacles.as_deref(),
        colliders.as_deref(),
    );

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
        queueing,
        ambient_direct,
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
            // `as_deref_mut()` would call `Mut::deref_mut`, which flags the
            // component Changed BEFORE the guard below decides not to write —
            // and lightyear replicates on the flag, not on the value. Reading
            // through `as_mut()` keeps the read on `Deref`, so a villager
            // waiting on a route no longer re-sends its stationary velocity
            // to every client 30 times a second.
            if let Some(motion) = motion.as_mut() {
                if motion.is_moving() {
                    **motion = CharacterMotion::STATIONARY;
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
        let route_geometry_is_current = use_route
            && route.as_ref().is_some_and(|route| {
                route.geometry_version != 0 && route.geometry_version == current_geometry_version
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
            let preferred_direction = to_goal / distance;
            let reaches_goal = step + HERO_ARRIVE_EPSILON >= distance;
            // Clamp the last leg to the certified waypoint itself. Rebuilding
            // that point as `current + normalized * distance` can land a few
            // floating-point ulps beyond it. Doorways and tree interaction
            // spots intentionally sit tight against obstacle boundaries, so
            // that microscopic overshoot was rejected and re-planned forever
            // even though the exact route endpoint was clear.
            let base_proposed = if reaches_goal {
                goal
            } else {
                current + preferred_direction * step
            };
            let building_obstacles = (!escaping_building)
                .then_some(obstacles.as_deref())
                .flatten();
            let mut direction = preferred_direction;
            let mut proposed = base_proposed;
            if !reaches_goal
                && !queueing
                && !ambient_direct
                && !authored_traversal
                && distance > CROWD_SEPARATION_RADIUS
            {
                if let Some(grid) = crowd_grid.as_deref() {
                    let steered = separated_direction(grid, entity, current, preferred_direction);
                    if steered.length_squared() > 0.5 {
                        let candidate = current + steered * step;
                        if navigation_segment_clear(
                            current,
                            candidate,
                            building_obstacles,
                            colliders.as_deref(),
                            derived.as_deref(),
                        ) {
                            direction = steered;
                            proposed = candidate;
                        }
                    }
                }
            }
            if *kind == CharacterKind::Villager
                && door_use.is_none()
                && pier_traversal.is_none()
                && !route_geometry_is_current
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
                        current.x, current.y, proposed.x, proposed.y, final_goal.x, final_goal.y,
                    );
                }
                let mut entity_commands = commands.entity(entity);
                entity_commands
                    .remove::<TravelRoute>()
                    .remove::<crate::world::village::ambient::AmbientDirectTransit>();
                if use_route {
                    // This segment was part of a route certified against an
                    // earlier world snapshot. A newly completed building (or
                    // streamed prop) has invalidated that proof. Purge cached
                    // corridors before reporting the embodied failure, so an
                    // owning routine which deliberately retries the same
                    // critical destination receives a fresh survey rather
                    // than the stale path again.
                    if let Some(graph) = road_graph.as_deref_mut() {
                        graph
                            .invalidate_tactical_routes_after_embodied_rejection(current, proposed);
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
        // `as_mut()`, not `as_deref_mut()`: see the stationary probe above.
        // The materiality guard only avoids the memory write; it cannot
        // un-flag a component that `deref_mut` already marked Changed.
        if let Some(motion) = motion.as_mut() {
            if motion_materially_changed(**motion, next_motion) {
                **motion = next_motion;
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
                .remove::<NavigationObstacleEscape>()
                .remove::<crate::world::village::ambient::AmbientDirectTransit>();
        }
    }
}

/// Enforce the movement/animation invariant after every tactical movement
/// pass: an embodied villager without a destination is stationary.
///
/// Activity systems deliberately remove `MoveTarget` when somebody reaches a
/// queue place, begins waiting, starts an indoor action, or abandons a route.
/// Such removals can happen before `step_units`, whose mover query naturally
/// no longer sees that character. Without this final pass the last replicated
/// velocity survived indefinitely and clients rendered a stationary person
/// walking in place. Boats are excluded because an embarked character's
/// motion is authored by the vessel synchronization systems.
pub fn settle_villagers_without_targets(
    mut villagers: Query<
        (&CharacterKind, &mut CharacterMotion),
        (
            Without<MoveTarget>,
            Without<AboardBoat>,
            Without<OfflineHero>,
            Without<crate::world::village::strategic::StrategicPerson>,
        ),
    >,
) {
    for (kind, mut motion) in villagers.iter_mut() {
        if *kind == CharacterKind::Villager && motion.is_moving() {
            *motion = CharacterMotion::STATIONARY;
        }
    }
}

/// Straight-segment terrain certification shared by every A*-free walk
/// (ambient strolls, immigration counter exits): rejects water and any
/// sampled rise steeper than 0.48 m per 0.75 m step. Collision clearance is
/// a separate concern - callers pair this with [`navigation_segment_clear`].
pub(crate) fn terrain_segment_walkable(
    terrain: &shared::terrain::WorldTerrain,
    start: Vec2,
    end: Vec2,
) -> bool {
    let steps = (start.distance(end) / 0.75).ceil().max(1.0) as usize;
    let mut previous_height = terrain.get_height(start.x, start.y);
    (1..=steps).all(|step| {
        let point = start.lerp(end, step as f32 / steps as f32);
        if terrain.get_water_height(point.x, point.y).is_some() {
            return false;
        }
        let height = terrain.get_height(point.x, point.y);
        let walkable = (height - previous_height).abs() <= 0.48;
        previous_height = height;
        walkable
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::village_roads::{RouteWaypoint, TravelRoute};
    use shared::components::TimeWarp;

    #[test]
    fn removing_a_villager_target_also_stops_the_walking_animation() {
        let mut app = App::new();
        app.add_systems(Update, settle_villagers_without_targets);
        let villager = app
            .world_mut()
            .spawn((CharacterKind::Villager, CharacterMotion::new(Vec3::X)))
            .id();
        let moving_hero = app
            .world_mut()
            .spawn((CharacterKind::Hero, CharacterMotion::new(Vec3::X)))
            .id();

        app.update();

        assert_eq!(
            *app.world()
                .entity(villager)
                .get::<CharacterMotion>()
                .unwrap(),
            CharacterMotion::STATIONARY
        );
        assert!(app
            .world()
            .entity(moving_hero)
            .get::<CharacterMotion>()
            .unwrap()
            .is_moving());
    }

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
        let east = CharacterMotion::new(Vec3::new(HERO_MOVE_SPEED, 0.0, 0.0));
        assert!(!motion_materially_changed(
            east,
            CharacterMotion::new(Vec3::new(HERO_MOVE_SPEED + 0.01, 0.0, 0.0))
        ));
        assert!(motion_materially_changed(
            east,
            CharacterMotion::new(Vec3::new(0.0, 0.0, HERO_MOVE_SPEED))
        ));
        assert!(motion_materially_changed(east, CharacterMotion::STATIONARY));
    }

    #[test]
    fn overlapped_movers_receive_bounded_individual_crowd_steering() {
        let mut app = App::new();
        app.insert_resource(WorldTerrain::default());
        app.init_resource::<TacticalCrowdGrid>();
        app.add_systems(Update, (rebuild_tactical_crowd_grid, step_units).chain());
        app.world_mut().spawn(TimeWarp::clamped(1.0));
        let goal = Vec3::new(20.0, 0.0, 0.0);
        let first = app
            .world_mut()
            .spawn((
                CharacterKind::Villager,
                PlayerPosition(Vec3::ZERO),
                PlayerRotation(0.0),
                RegionCoord::default(),
                MoveTarget(goal),
            ))
            .id();
        let second = app
            .world_mut()
            .spawn((
                CharacterKind::Villager,
                PlayerPosition(Vec3::ZERO),
                PlayerRotation(0.0),
                RegionCoord::default(),
                MoveTarget(goal),
            ))
            .id();

        app.update();

        let a = app.world().get::<PlayerPosition>(first).unwrap().0;
        let b = app.world().get::<PlayerPosition>(second).unwrap().0;
        assert!(a.x > 0.0 && b.x > 0.0, "steering stopped forward travel");
        assert!(
            a.distance_squared(b) > 1.0e-6,
            "overlapped villagers chose the same embodied step"
        );
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
                    geometry_version: 0,
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
                    geometry_version: 0,
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
                    horizontal_radius: 0.8,
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

    /// Replication sends on Bevy's *change flag*, not on a value difference, so
    /// probing a component with `as_deref_mut()` re-sends it every tick even
    /// when the guard inside declines to write. This pinned ~150 standing
    /// villagers into every replication packet on the 1,000-villager world.
    #[test]
    fn a_villager_waiting_for_a_route_never_dirties_its_replicated_motion() {
        #[derive(Resource, Default)]
        struct MotionDirty(usize);

        fn count_dirty(
            mut dirty: ResMut<MotionDirty>,
            moved: Query<Entity, Changed<CharacterMotion>>,
        ) {
            dirty.0 += moved.iter().count();
        }

        let mut app = App::new();
        app.insert_resource(WorldTerrain::default());
        app.init_resource::<MotionDirty>();
        app.add_systems(Update, (step_units, count_dirty).chain());

        let waiting = app
            .world_mut()
            .spawn((
                CharacterKind::Villager,
                PlayerPosition(Vec3::ZERO),
                PlayerRotation(0.0),
                RegionCoord::default(),
                CharacterMotion::STATIONARY,
                MoveTarget(Vec3::new(40.0, 0.0, 0.0)),
                NavigationRoutePending::new(Vec3::new(40.0, 0.0, 0.0)),
            ))
            .id();

        // The spawn itself legitimately marks every component Changed.
        app.update();
        app.world_mut().resource_mut::<MotionDirty>().0 = 0;

        for _ in 0..4 {
            app.update();
        }

        assert_eq!(
            app.world().resource::<MotionDirty>().0,
            0,
            "a villager standing still while its route is planned re-replicated CharacterMotion"
        );
        assert_eq!(
            app.world().get::<CharacterMotion>(waiting).copied(),
            Some(CharacterMotion::STATIONARY),
            "the stationary clamp itself regressed"
        );
    }
}
