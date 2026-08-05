//! Cheap ambient life for residents who currently lack a home or a job.
//!
//! This deliberately does not run a planner on every villager every frame.
//! One shared real-time clock wakes the system four times a second, destinations
//! are deterministic roadside/gathering spots, and ordinary movement reuses the
//! cached village-road routing layer. Regions nobody observes receive no
//! ambient orders at all. The future strategic `AtPlace`/`Travelling` person
//! records can replace that frozen body without changing these local choices.

use std::collections::HashMap;

use bevy::prelude::*;
use shared::components::{
    BuildingDoorUse, CharacterActivity, CharacterKind, CharacterName, Occupation, PlayerPosition,
    PlayerRotation, Settlement, SettlementBuildingKind, VillageRoad, WorldTime,
};
use shared::region::{RegionCoord, SimLevel};
use shared::spatial::SpatialObstacleGrid;
use shared::terrain::WorldTerrain;

use crate::collision::library::{DerivedColliderLibrary, StaticColliders};
use crate::player::hero::{navigation_segment_clear, MoveTarget};
use crate::world::regions::RegionRegistry;
use crate::world::village_roads::{NavigationRoutePending, RoadBuilderRoutine, TravelRoute};

use super::{
    ConstructionMaterialRoutine, FarmerRoutine, FishingRoutine, HomeAssignment, HomeRoutine,
    HouseholdShoppingRoutine, LumberjackRoutine, PierTraversal, VillagerIntent, WorkerOffDuty,
    WorkplaceDoorTransit,
};

/// Ambient decisions run at a bounded wall-clock cadence even under 100x warp.
/// Warp scales the simulated duration represented by a pass, never its firing
/// rate, so stress testing cannot turn 5,000 residents into 500,000 decisions.
const AMBIENT_REAL_INTERVAL: f32 = 0.25;
const AMBIENT_REACH: f32 = 0.65;
const ROADSIDE_SPACING: f32 = 6.0;
const ROADSIDE_MARGIN: f32 = 0.62;
const MAX_AMBIENT_TRAVEL_SECONDS: f32 = 60.0;

#[derive(Resource, Default)]
pub struct AmbientClock {
    real_accumulator: f32,
    had_tactical_observer: bool,
}

/// Roadside candidates are geometry, not a per-person decision. Cache them
/// until a hall, built road prefix, building obstacle or streamed prop set
/// changes so a crowd shares one survey instead of repeating it 5,000 times.
#[derive(Resource, Default)]
pub struct AmbientSpotCache {
    signature: u64,
    initialized: bool,
    by_settlement: HashMap<Entity, Vec<AmbientSpot>>,
}

#[derive(Component, Debug, Clone)]
pub struct AmbientRoutine {
    settlement: Entity,
    cycle: u32,
    phase: AmbientPhase,
}

#[derive(Debug, Clone, Copy)]
enum AmbientPhase {
    Waiting {
        seconds_left: f32,
    },
    Walking {
        destination: Vec3,
        facing: f32,
        sitting: bool,
        rest_seconds: f32,
        travel_seconds_left: f32,
    },
    Resting {
        seconds_left: f32,
        sitting: bool,
    },
    /// Unhoused residents gather at the Moot Hall after dark. Going through
    /// the hall door can replace this once halls gain an interior routine.
    NightShelter {
        destination: Vec3,
    },
}

#[derive(Debug, Clone, Copy)]
struct AmbientSpot {
    point: Vec2,
    facing: f32,
}

fn stable_hash(text: &str) -> u64 {
    text.bytes().fold(1_469_598_103_934_665_603, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(1_099_511_628_211)
    })
}

fn mix(mut value: u64) -> u64 {
    value ^= value >> 30;
    value = value.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94d0_49bb_1331_11eb);
    value ^ (value >> 31)
}

fn duration(seed: u64, min: f32, max: f32) -> f32 {
    let fraction = (mix(seed) >> 40) as f32 / (1u64 << 24) as f32;
    min + (max - min) * fraction
}

fn facing_toward(direction: Vec2) -> f32 {
    f32::atan2(-direction.x, -direction.y)
}

fn raw_roadside_spots(road: &VillageRoad) -> Vec<AmbientSpot> {
    let offset = road.width * 0.5 + ROADSIDE_MARGIN;
    let mut spots = Vec::new();
    for pair in road.built_points().windows(2) {
        let delta = pair[1] - pair[0];
        let length = delta.length();
        if length < 1.0 {
            continue;
        }
        let tangent = delta / length;
        let normal = Vec2::new(-tangent.y, tangent.x);
        let samples = (length / ROADSIDE_SPACING).ceil().max(1.0) as usize;
        for sample in 0..samples {
            let along = (sample as f32 + 0.5) / samples as f32;
            let centre = pair[0].lerp(pair[1], along);
            for side in [-1.0, 1.0] {
                let point = centre + normal * offset * side;
                spots.push(AmbientSpot {
                    point,
                    facing: facing_toward(centre - point),
                });
            }
        }
    }
    spots
}

fn point_is_safe(
    point: Vec2,
    terrain: &WorldTerrain,
    obstacles: Option<&SpatialObstacleGrid>,
    colliders: Option<&StaticColliders>,
    derived: Option<&DerivedColliderLibrary>,
) -> bool {
    let height = terrain.get_height(point.x, point.y);
    if terrain
        .water_level()
        .is_some_and(|water| height < water + super::FREEBOARD)
    {
        return false;
    }
    if 1.0 - terrain.get_normal(point.x, point.y).y.clamp(0.0, 1.0) > 0.24 {
        return false;
    }
    navigation_segment_clear(point, point, obstacles, colliders, derived)
}

fn gathering_spots(
    terrain: &WorldTerrain,
    settlements: &Query<
        (
            Entity,
            &Settlement,
            &PlayerPosition,
            Option<&PlayerRotation>,
        ),
        Without<CharacterKind>,
    >,
    roads: &Query<&VillageRoad>,
    obstacles: Option<&SpatialObstacleGrid>,
    colliders: Option<&StaticColliders>,
    derived: Option<&DerivedColliderLibrary>,
) -> HashMap<Entity, Vec<AmbientSpot>> {
    let mut by_settlement = HashMap::new();
    let mut entity_by_name = HashMap::new();

    for (entity, settlement, hall, rotation) in settlements.iter() {
        entity_by_name.insert(settlement.name.as_str(), entity);
        let yaw = rotation.map_or(0.0, |rotation| rotation.0);
        let door = SettlementBuildingKind::Hall.entrance_position(hall.0, yaw);
        let side_axis = shared::rotation::local_to_world_xz(Vec2::X, yaw);
        let mut spots = Vec::new();
        for side in [-1.0, 1.0] {
            let point = Vec2::new(door.x, door.z) + side_axis * 2.25 * side;
            if point_is_safe(point, terrain, obstacles, colliders, derived) {
                spots.push(AmbientSpot {
                    point,
                    facing: facing_toward(Vec2::new(hall.0.x, hall.0.z) - point),
                });
            }
        }
        by_settlement.insert(entity, spots);
    }

    for road in roads.iter() {
        let Some(entity) = entity_by_name.get(road.settlement.as_str()).copied() else {
            continue;
        };
        let destination = by_settlement.entry(entity).or_insert_with(Vec::new);
        destination.extend(
            raw_roadside_spots(road)
                .into_iter()
                .filter(|spot| point_is_safe(spot.point, terrain, obstacles, colliders, derived)),
        );
    }

    by_settlement
}

fn spot_geometry_signature(
    settlements: &Query<
        (
            Entity,
            &Settlement,
            &PlayerPosition,
            Option<&PlayerRotation>,
        ),
        Without<CharacterKind>,
    >,
    roads: &Query<&VillageRoad>,
    obstacle_version: u64,
    collider_version: u64,
) -> u64 {
    let mut signature = mix(obstacle_version ^ collider_version.rotate_left(17));
    for (entity, settlement, position, rotation) in settlements.iter() {
        let value = entity.to_bits()
            ^ stable_hash(&settlement.name)
            ^ u64::from(position.0.x.to_bits()).rotate_left(7)
            ^ u64::from(position.0.z.to_bits()).rotate_left(19)
            ^ u64::from(rotation.map_or(0.0, |rotation| rotation.0).to_bits()).rotate_left(31);
        signature = signature.wrapping_add(mix(value));
    }
    for road in roads.iter() {
        let value = stable_hash(&road.settlement)
            ^ u64::from(road.built_through).rotate_left(11)
            ^ (road.points.len() as u64).rotate_left(29)
            ^ u64::from(road.width.to_bits()).rotate_left(43);
        signature = signature.wrapping_add(mix(value));
    }
    signature
}

fn clear_owned_movement(commands: &mut Commands, entity: Entity) {
    commands
        .entity(entity)
        .remove::<AmbientRoutine>()
        .remove::<MoveTarget>()
        .remove::<TravelRoute>()
        .remove::<NavigationRoutePending>();
}

fn choose_spot(
    name: &str,
    routine: &mut AmbientRoutine,
    spots: &[AmbientSpot],
    terrain: &WorldTerrain,
) -> Option<(Vec3, f32, bool, f32)> {
    if spots.is_empty() {
        return None;
    }
    let seed = mix(stable_hash(name) ^ u64::from(routine.cycle));
    routine.cycle = routine.cycle.wrapping_add(1);
    let spot = spots[(seed as usize) % spots.len()];
    let point = Vec3::new(
        spot.point.x,
        terrain.get_height(spot.point.x, spot.point.y),
        spot.point.y,
    );
    let sitting = !mix(seed ^ 0x0a11_ce55).is_multiple_of(3);
    let rest = duration(seed ^ 0x5eed, 12.0, 32.0);
    Some((point, spot.facing, sitting, rest))
}

/// Give unemployed or unhoused residents a little visible life when observed.
///
/// This pass is intentionally bounded by wall time and region simulation LOD.
/// It never pathfinds itself: it writes one `MoveTarget`, after which the
/// existing budgeted route queue and shared road graph do the travel work.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn run_ambient_routines(
    simulation_time: crate::world::simulation_time::SimulationTime,
    mut ambient_clock: ResMut<AmbientClock>,
    mut spot_cache: ResMut<AmbientSpotCache>,
    world_time: Query<&WorldTime>,
    terrain: Option<Res<WorldTerrain>>,
    regions: Option<Res<RegionRegistry>>,
    obstacles: Option<Res<SpatialObstacleGrid>>,
    colliders: Option<Res<StaticColliders>>,
    derived: Option<Res<DerivedColliderLibrary>>,
    settlements: Query<
        (
            Entity,
            &Settlement,
            &PlayerPosition,
            Option<&PlayerRotation>,
        ),
        Without<CharacterKind>,
    >,
    roads: Query<&VillageRoad>,
    busy: Query<
        (),
        Or<(
            With<FarmerRoutine>,
            With<FishingRoutine>,
            With<LumberjackRoutine>,
            With<HomeRoutine>,
            With<RoadBuilderRoutine>,
            With<WorkplaceDoorTransit>,
            With<BuildingDoorUse>,
            With<PierTraversal>,
            With<HouseholdShoppingRoutine>,
        )>,
    >,
    mut villagers: Query<
        (
            Entity,
            &CharacterName,
            &PlayerPosition,
            &mut PlayerRotation,
            &RegionCoord,
            &VillagerIntent,
            &Occupation,
            Option<&HomeAssignment>,
            Option<&WorkerOffDuty>,
            Option<&ConstructionMaterialRoutine>,
            &mut CharacterActivity,
            Option<&MoveTarget>,
            Option<&mut AmbientRoutine>,
        ),
        (
            With<CharacterKind>,
            Without<super::strategic::StrategicPerson>,
        ),
    >,
    mut commands: Commands,
) {
    ambient_clock.real_accumulator += simulation_time.real_seconds();
    if ambient_clock.real_accumulator < AMBIENT_REAL_INTERVAL {
        return;
    }
    let real_elapsed = std::mem::take(&mut ambient_clock.real_accumulator);
    let dt = real_elapsed * simulation_time.factor();
    let any_tactical_observer = regions
        .as_ref()
        .is_none_or(|registry| registry.tactical_count() > 0);
    if !any_tactical_observer && !ambient_clock.had_tactical_observer {
        return;
    }
    ambient_clock.had_tactical_observer = any_tactical_observer;
    let Some(terrain) = terrain else { return };
    let Some(daylight) = world_time.iter().next().map(WorldTime::is_day) else {
        return;
    };
    if any_tactical_observer {
        let signature = spot_geometry_signature(
            &settlements,
            &roads,
            obstacles.as_deref().map_or(0, |grid| grid.version),
            colliders
                .as_deref()
                .map_or(0, |colliders| colliders.version),
        );
        if !spot_cache.initialized || spot_cache.signature != signature {
            spot_cache.by_settlement = gathering_spots(
                &terrain,
                &settlements,
                &roads,
                obstacles.as_deref(),
                colliders.as_deref(),
                derived.as_deref(),
            );
            spot_cache.signature = signature;
            spot_cache.initialized = true;
        }
    }

    for (
        entity,
        name,
        position,
        mut facing,
        region,
        intent,
        occupation,
        home,
        off_duty,
        construction,
        mut activity,
        move_target,
        routine,
    ) in villagers.iter_mut()
    {
        let waiting_builder =
            construction.is_some_and(|routine| routine.is_waiting_for_materials());
        let settlement = match intent {
            VillagerIntent::Resident { settlement } => *settlement,
            VillagerIntent::Building { settlement, .. } if waiting_builder => *settlement,
            _ => {
                if routine.is_some() {
                    if *activity == CharacterActivity::Sitting {
                        *activity = CharacterActivity::Idle;
                    }
                    // A real active work/build/home routine now owns any destination.
                    commands.entity(entity).remove::<AmbientRoutine>();
                }
                continue;
            }
        };

        if busy.get(entity).is_ok() {
            if routine.is_some() {
                if *activity == CharacterActivity::Sitting {
                    *activity = CharacterActivity::Idle;
                }
                commands.entity(entity).remove::<AmbientRoutine>();
            }
            continue;
        }

        let needs_ambient_life =
            waiting_builder || off_duty.is_some() || occupation.0.is_none() || home.is_none();
        if !needs_ambient_life {
            if routine.is_some() {
                if *activity == CharacterActivity::Sitting {
                    *activity = CharacterActivity::Idle;
                }
                clear_owned_movement(&mut commands, entity);
            }
            continue;
        }

        // No tactical observer means no ambient decisions, routes, movement or
        // animation. Keeping the last authoritative position is the temporary
        // representation until strategic Person promotion/demotion lands.
        let tactical = regions.as_ref().is_none_or(|registry| {
            registry
                .get(*region)
                .is_some_and(|state| state.sim_level == SimLevel::Tactical)
        });
        if !tactical {
            if routine.is_some() {
                if *activity == CharacterActivity::Sitting {
                    *activity = CharacterActivity::Idle;
                }
                clear_owned_movement(&mut commands, entity);
            }
            continue;
        }

        let hall = settlements.get(settlement).ok();
        if !daylight && home.is_none() {
            let Some((_, _, hall_position, hall_rotation)) = hall else {
                continue;
            };
            let destination = SettlementBuildingKind::Hall.entrance_position(
                hall_position.0,
                hall_rotation.map_or(0.0, |rotation| rotation.0),
            );
            *activity = CharacterActivity::Idle;
            if super::ground_distance(position.0, destination) <= AMBIENT_REACH {
                clear_owned_movement(&mut commands, entity);
                commands.entity(entity).insert(AmbientRoutine {
                    settlement,
                    cycle: routine.as_deref().map_or(0, |routine| routine.cycle),
                    phase: AmbientPhase::NightShelter { destination },
                });
            } else {
                super::ensure_move_target(&mut commands, entity, move_target, destination);
                if let Some(mut routine) = routine {
                    routine.settlement = settlement;
                    routine.phase = AmbientPhase::NightShelter { destination };
                } else {
                    commands.entity(entity).insert(AmbientRoutine {
                        settlement,
                        cycle: 0,
                        phase: AmbientPhase::NightShelter { destination },
                    });
                }
            }
            continue;
        }

        if !daylight {
            // A housed villager should have acquired HomeRoutine earlier in the
            // chained village schedule. If that is delayed for one pass, avoid
            // starting a fresh daytime stroll at night.
            continue;
        }

        let Some(mut routine) = routine else {
            let seed = stable_hash(&name.0) ^ entity.to_bits();
            commands.entity(entity).insert(AmbientRoutine {
                settlement,
                cycle: 0,
                phase: AmbientPhase::Waiting {
                    seconds_left: duration(seed, 2.0, 12.0),
                },
            });
            continue;
        };

        if routine.settlement != settlement {
            routine.settlement = settlement;
            routine.phase = AmbientPhase::Waiting { seconds_left: 1.0 };
        }

        match routine.phase {
            AmbientPhase::NightShelter { destination } => {
                let _ = destination;
                *activity = CharacterActivity::Idle;
                routine.phase = AmbientPhase::Waiting { seconds_left: 1.0 };
            }
            AmbientPhase::Waiting { seconds_left } => {
                *activity = CharacterActivity::Idle;
                commands
                    .entity(entity)
                    .remove::<MoveTarget>()
                    .remove::<TravelRoute>()
                    .remove::<NavigationRoutePending>();
                let left = seconds_left - dt;
                if left > 0.0 {
                    routine.phase = AmbientPhase::Waiting { seconds_left: left };
                    continue;
                }
                let Some((destination, rest_facing, sitting, rest_seconds)) = spot_cache
                    .by_settlement
                    .get(&settlement)
                    .and_then(|spots| choose_spot(&name.0, &mut routine, spots, &terrain))
                else {
                    routine.phase = AmbientPhase::Waiting { seconds_left: 12.0 };
                    continue;
                };
                super::ensure_move_target(&mut commands, entity, move_target, destination);
                routine.phase = AmbientPhase::Walking {
                    destination,
                    facing: rest_facing,
                    sitting,
                    rest_seconds,
                    travel_seconds_left: MAX_AMBIENT_TRAVEL_SECONDS,
                };
            }
            AmbientPhase::Walking {
                destination,
                facing: rest_facing,
                sitting,
                rest_seconds,
                travel_seconds_left,
            } => {
                *activity = CharacterActivity::Idle;
                if super::ground_distance(position.0, destination) <= AMBIENT_REACH {
                    commands
                        .entity(entity)
                        .remove::<MoveTarget>()
                        .remove::<TravelRoute>()
                        .remove::<NavigationRoutePending>();
                    facing.0 = rest_facing;
                    *activity = if sitting {
                        CharacterActivity::Sitting
                    } else {
                        CharacterActivity::Idle
                    };
                    routine.phase = AmbientPhase::Resting {
                        seconds_left: rest_seconds,
                        sitting,
                    };
                    continue;
                }
                let left = travel_seconds_left - dt;
                if left <= 0.0 {
                    commands
                        .entity(entity)
                        .remove::<MoveTarget>()
                        .remove::<TravelRoute>()
                        .remove::<NavigationRoutePending>();
                    routine.phase = AmbientPhase::Waiting { seconds_left: 4.0 };
                } else {
                    super::ensure_move_target(&mut commands, entity, move_target, destination);
                    routine.phase = AmbientPhase::Walking {
                        destination,
                        facing: rest_facing,
                        sitting,
                        rest_seconds,
                        travel_seconds_left: left,
                    };
                }
            }
            AmbientPhase::Resting {
                seconds_left,
                sitting,
            } => {
                commands
                    .entity(entity)
                    .remove::<MoveTarget>()
                    .remove::<TravelRoute>()
                    .remove::<NavigationRoutePending>();
                *activity = if sitting {
                    CharacterActivity::Sitting
                } else {
                    CharacterActivity::Idle
                };
                let left = seconds_left - dt;
                if left > 0.0 {
                    routine.phase = AmbientPhase::Resting {
                        seconds_left: left,
                        sitting,
                    };
                } else {
                    *activity = CharacterActivity::Idle;
                    let seed = stable_hash(&name.0) ^ u64::from(routine.cycle);
                    routine.phase = AmbientPhase::Waiting {
                        seconds_left: duration(seed, 4.0, 14.0),
                    };
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roadside_spots_are_beyond_both_edges_and_face_the_path() {
        let road = VillageRoad {
            settlement: "Test".to_string(),
            builder: "Ada".to_string(),
            points: vec![Vec2::ZERO, Vec2::new(12.0, 0.0)],
            built_through: 2,
            width: 2.6,
            reserved_width: shared::components::RoadClass::Lane.initial_reserved_width(),
            surface: default(),
            class: default(),
            stone_committed: 0,
        };
        let spots = raw_roadside_spots(&road);
        assert_eq!(spots.len(), 4);
        assert!(spots.iter().all(|spot| {
            (spot.point.y.abs() - (road.width * 0.5 + ROADSIDE_MARGIN)).abs() < 1e-4
        }));
        assert!(spots.iter().any(|spot| spot.point.y < 0.0));
        assert!(spots.iter().any(|spot| spot.point.y > 0.0));
        for spot in spots {
            let forward = Vec2::new(-spot.facing.sin(), -spot.facing.cos());
            assert!(forward.dot(Vec2::new(0.0, -spot.point.y).normalize()) > 0.99);
        }
    }

    #[test]
    fn five_thousand_unobserved_residents_receive_no_ambient_work() {
        let mut app = App::new();
        app.init_resource::<Time>();
        app.init_resource::<AmbientClock>();
        app.init_resource::<AmbientSpotCache>();
        app.init_resource::<RegionRegistry>();
        app.insert_resource(WorldTerrain::default());
        app.add_systems(Update, run_ambient_routines);
        app.world_mut().spawn((
            WorldTime::new_default(),
            shared::components::TimeWarp::clamped(1.0),
        ));
        let hall = app
            .world_mut()
            .spawn((
                Settlement {
                    name: "Quiet".to_string(),
                    tier: shared::components::SettlementTier::Hamlet,
                    residents: 1,
                    treasury: 0,
                },
                PlayerPosition(Vec3::ZERO),
                PlayerRotation(0.0),
            ))
            .id();
        let villagers: Vec<_> = (0..5_000)
            .map(|index| {
                app.world_mut()
                    .spawn((
                        CharacterName(format!("Resident{index}")),
                        CharacterKind::Villager,
                        PlayerPosition(Vec3::new(0.0, 0.0, -8.0)),
                        PlayerRotation(0.0),
                        RegionCoord::default(),
                        VillagerIntent::Resident { settlement: hall },
                        Occupation::default(),
                        CharacterActivity::Idle,
                    ))
                    .id()
            })
            .collect();

        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs_f32(0.3));
        app.update();
        assert!(villagers.iter().all(|villager| {
            let resident = app.world().entity(*villager);
            !resident.contains::<AmbientRoutine>() && !resident.contains::<MoveTarget>()
        }));
    }
}
