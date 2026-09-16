//! One employed company porter can crew one ship. The berth is the only
//! handoff between land movement and the vessel; food is paid, carried stock.

use bevy::prelude::*;
use shared::components::*;
use shared::economy::{BusinessAccount, CompanyAccount, Good, GoodsInventory, MootMarket};
use shared::region::RegionCoord;

use crate::player::hero::MoveTarget;
use crate::world::simulation_time::SimulationDelta;
use crate::world::village::{BusinessEventQueue, CompanyPorter};
use crate::world::village_roads::{NavigationRouteFailed, NavigationRoutePending, TravelRoute};

const BOARDING_SPEED: f32 = 1.8;
const PROVISION_DAYS: u32 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CrewPhase {
    CollectingProvisions,
    Approaching,
    BoardingPier,
    BoardingHull,
    Aboard,
    LeavingHull,
    LeavingPier,
    ResupplyLeavingHull,
    ResupplyLeavingPier,
}

#[derive(Clone, Copy, Debug)]
struct ProvisionPickup {
    hall: Entity,
    counter: Entity,
}

/// Owns the person's activity from accepting the assignment until a safe shore
/// exit. Employment and its existing payroll remain on the home warehouse.
#[derive(Component, Clone, Copy, Debug)]
pub(crate) struct ShipCrew {
    pub(crate) ship: Entity,
    port: SettlementPort,
    phase: CrewPhase,
    provisions: Option<ProvisionPickup>,
    provision_retry_in: f32,
    /// One optional top-up per physical berth visit. An empty basket can still
    /// request essential resupply while the ship is waiting to depart.
    provisioned_this_berth: bool,
}

impl ShipCrew {
    pub(crate) fn diagnostic_phase(&self) -> String {
        format!("{:?}", self.phase)
    }

    pub(crate) fn diagnostic_provision_counter(&self, world: &World) -> Option<Vec3> {
        self.provisions
            .and_then(|pickup| provision_counter(world, pickup))
    }

    pub(crate) fn objective(&self) -> CharacterObjective {
        match self.phase {
            CrewPhase::CollectingProvisions
            | CrewPhase::Approaching
            | CrewPhase::BoardingPier
            | CrewPhase::BoardingHull => CharacterObjective::BoardingTradeShip,
            CrewPhase::Aboard => CharacterObjective::SailingTradeShip,
            CrewPhase::LeavingHull
            | CrewPhase::LeavingPier
            | CrewPhase::ResupplyLeavingHull
            | CrewPhase::ResupplyLeavingPier => CharacterObjective::LeavingTradeShip,
        }
    }
}

#[derive(Component, Clone, Copy, Debug)]
struct CrewAssignment {
    person: Entity,
}

pub(crate) fn assigned_person(world: &World, ship: Entity) -> Option<PersonId> {
    world
        .get::<CrewAssignment>(ship)
        .and_then(|assignment| world.get::<PersonId>(assignment.person))
        .copied()
}

pub(crate) fn crew_ready(world: &World, ship: Entity) -> bool {
    world.get::<CrewAssignment>(ship).is_some_and(|assignment| {
        world
            .get::<ShipCrew>(assignment.person)
            .is_some_and(|crew| crew.phase == CrewPhase::Aboard)
            && world.get::<CompanyPorter>(assignment.person).is_some()
            && world
                .get::<crate::world::village::worker_activity::EmploymentReleaseRequested>(
                    assignment.person,
                )
                .is_none()
            && world
                .get::<GoodsInventory>(assignment.person)
                .is_some_and(|food| food.edible_amount() >= 1)
            && world
                .get::<Health>(assignment.person)
                .is_none_or(|health| health.current > 0.0)
    })
}

pub(crate) fn acquire_crew(
    world: &mut World,
    ship: Entity,
    company: CompanyId,
    warehouse: BuildingId,
    port: &SettlementPort,
) -> bool {
    if let Some(assignment) = world.get::<CrewAssignment>(ship).copied() {
        if world.get::<ShipCrew>(assignment.person).is_some() {
            let still_employed =
                world
                    .get::<CompanyPorter>(assignment.person)
                    .is_some_and(|porter| {
                        porter.company == company && porter.storage_hall == warehouse
                    });
            if !still_employed
                || world
                    .get::<crate::world::village::worker_activity::EmploymentReleaseRequested>(
                        assignment.person,
                    )
                    .is_some()
            {
                release_crew(world, ship, port);
                return false;
            }
            if world
                .get::<ShipCrew>(assignment.person)
                .is_some_and(|crew| crew.phase == CrewPhase::Aboard)
                && at_berth(world, ship, port)
            {
                let held = world
                    .get::<GoodsInventory>(assignment.person)
                    .map_or(0, GoodsInventory::edible_amount);
                let prior = *world.get::<ShipCrew>(assignment.person).unwrap();
                let still_at_previous_berth = at_berth(world, ship, &prior.port);
                let attempted = prior.provisioned_this_berth && still_at_previous_berth;
                {
                    let mut crew = world.get_mut::<ShipCrew>(assignment.person).unwrap();
                    crew.port = *port;
                    crew.provisioned_this_berth = attempted || held >= PROVISION_DAYS;
                }
                if held == 0 || (held < PROVISION_DAYS && !attempted) {
                    if let Some(pickup) =
                        provision_pickup(world, port.settlement, port.geometry.shore)
                    {
                        let mut crew = world.get_mut::<ShipCrew>(assignment.person).unwrap();
                        crew.port = *port;
                        crew.provisions = Some(pickup);
                        crew.provision_retry_in = 0.0;
                        crew.provisioned_this_berth = true;
                        crew.phase = CrewPhase::ResupplyLeavingHull;
                    }
                }
            }
            return crew_ready(world, ship);
        }
        world.entity_mut(ship).remove::<CrewAssignment>();
    }
    if !at_berth(world, ship, port) {
        return false;
    }
    let candidate = {
        let mut query = world
            .query_filtered::<(Entity, &CompanyPorter, &PlayerPosition, &PersonId), (
                Without<ShipCrew>,
                Without<MoveTarget>,
                Without<TravelRoute>,
                Without<NavigationRoutePending>,
                Without<Hero>,
                Without<MemberOfBattalion>,
                Without<crate::world::village::worker_activity::EmploymentReleaseRequested>,
            )>();
        let candidates: Vec<_> = query
            .iter(world)
            .filter(|(_, porter, _, _)| {
                porter.company == company && porter.storage_hall == warehouse
            })
            .map(|(person, _, position, id)| {
                (
                    person,
                    position.0.distance_squared(port.geometry.shore),
                    *id,
                )
            })
            .collect();
        // This remains the same employer's transport assignment. An idle
        // occupied workplace can hand off through its real exit; an active
        // doorway, service, cargo or personal errand is still unavailable.
        let mut busy = world
            .query_filtered::<Entity, crate::world::village::worker_activity::TransportStartBlocked>();
        candidates
            .into_iter()
            .filter(|(person, _, _)| {
                busy.get(world, *person).is_err()
                    && world.get::<GoodsInventory>(*person).is_some_and(|cargo| {
                        Good::ALL.into_iter().all(|good| {
                            cargo.amount(good) == 0 || Good::READY_TO_EAT_PRIORITY.contains(&good)
                        })
                    })
            })
            .min_by(|a, b| a.1.total_cmp(&b.1).then(a.2.cmp(&b.2)))
            .map(|(person, _, _)| person)
    };
    let Some(person) = candidate else {
        return false;
    };
    let origin = world.get::<PlayerPosition>(person).unwrap().0;
    let provisions = (world
        .get::<GoodsInventory>(person)
        .map_or(0, GoodsInventory::edible_amount)
        < PROVISION_DAYS)
        .then(|| provision_pickup(world, port.settlement, origin))
        .flatten();
    let (phase, destination) = if let Some(pickup) = provisions {
        let Some(counter) = provision_counter(world, pickup) else {
            return false;
        };
        (CrewPhase::CollectingProvisions, counter)
    } else {
        if world
            .get::<GoodsInventory>(person)
            .is_none_or(|food| food.edible_amount() == 0)
        {
            return false;
        }
        (CrewPhase::Approaching, port.geometry.shore)
    };
    world
        .entity_mut(person)
        .remove::<(TravelRoute, NavigationRoutePending, NavigationRouteFailed)>()
        .insert(ShipCrew {
            ship,
            port: *port,
            phase,
            provisions,
            provision_retry_in: 0.0,
            provisioned_this_berth: provisions.is_none(),
        });
    begin_land_trip(world, person, destination);
    world.entity_mut(ship).insert(CrewAssignment { person });
    false
}

fn at_berth(world: &World, ship: Entity, port: &SettlementPort) -> bool {
    world.get::<PlayerPosition>(ship).is_some_and(|position| {
        position.0.xz().distance_squared(port.geometry.berth.xz()) <= 2.0 * 2.0
    })
}

fn settle_land_activity(world: &mut World, person: Entity, stationary: bool) {
    if let Some(mut activity) = world.get_mut::<CharacterActivity>(person) {
        activity.set_if_neq(CharacterActivity::Idle);
    }
    if stationary {
        if let Some(mut motion) = world.get_mut::<CharacterMotion>(person) {
            motion.set_if_neq(CharacterMotion::STATIONARY);
        }
    }
}

fn begin_land_trip(world: &mut World, person: Entity, destination: Vec3) {
    let interior = world
        .get::<crate::world::village::WorkplaceInterior>(person)
        .copied();
    settle_land_activity(world, person, true);
    crate::world::village::worker_activity::lifecycle::begin_trip(
        &mut world.commands(),
        person,
        destination,
        interior.as_ref(),
    );
    world.flush();
}

pub(crate) fn release_crew(world: &mut World, ship: Entity, port: &SettlementPort) {
    if !at_berth(world, ship, port) {
        return;
    }
    let Some(assignment) = world.get::<CrewAssignment>(ship).copied() else {
        return;
    };
    let Some(mut crew) = world.get_mut::<ShipCrew>(assignment.person) else {
        return;
    };
    if crew.phase == CrewPhase::Aboard || crew.phase == CrewPhase::ResupplyLeavingHull {
        crew.port = *port;
        crew.phase = CrewPhase::LeavingHull;
    } else if crew.phase == CrewPhase::BoardingHull {
        crew.phase = CrewPhase::LeavingHull;
    } else if crew.phase == CrewPhase::BoardingPier || crew.phase == CrewPhase::ResupplyLeavingPier
    {
        crew.phase = CrewPhase::LeavingPier;
    } else if crew.phase == CrewPhase::Approaching || crew.phase == CrewPhase::CollectingProvisions
    {
        if let Some(mut crossing) =
            world.get_mut::<crate::world::village::WorkplaceDoorTransit>(assignment.person)
        {
            // Cancel the later errand, not the crossing already owned by the
            // shared door system. Its completion will clear the movement.
            crossing.destination_after_exit = None;
            world.entity_mut(assignment.person).remove::<ShipCrew>();
            world.entity_mut(ship).remove::<CrewAssignment>();
            return;
        }
        world.entity_mut(assignment.person).remove::<(
            ShipCrew,
            MoveTarget,
            TravelRoute,
            NavigationRoutePending,
            NavigationRouteFailed,
        )>();
        settle_land_activity(world, assignment.person, true);
        world.entity_mut(ship).remove::<CrewAssignment>();
    }
}

/// Shared coordinates with the rendered deck; bows face local -Z.
fn helm(world: &World, ship: Entity) -> Option<Vec3> {
    let hull = world.get::<CompanyShip>(ship)?;
    let position = world.get::<PlayerPosition>(ship)?.0;
    let yaw = world.get::<PlayerRotation>(ship)?.0;
    Some(position + Quat::from_rotation_y(yaw) * Vec3::new(0.0, 0.65, hull.kind.length() * 0.28))
}

pub(crate) fn advance_crew(world: &mut World) {
    let dt = world
        .get_resource::<SimulationDelta>()
        .copied()
        .unwrap_or_default()
        .world_seconds();
    let mut query = world.query::<(Entity, &ShipCrew)>();
    let crews: Vec<_> = query
        .iter(world)
        .map(|(person, crew)| (person, *crew))
        .collect();
    for (person, mut crew) in crews {
        if world.get::<Health>(person).is_some_and(Health::is_dead) {
            world.entity_mut(person).remove::<(ShipCrew, AboardShip)>();
            if let Ok(mut ship) = world.get_entity_mut(crew.ship) {
                ship.remove::<CrewAssignment>();
            }
            continue;
        }
        let Some(hull_position) = helm(world, crew.ship) else {
            // A deleted hull cannot put its crew on a remote shore. Keep the
            // body and food at the last authoritative position.
            world.entity_mut(person).remove::<(ShipCrew, AboardShip)>();
            continue;
        };
        let Some(position) = world.get::<PlayerPosition>(person).map(|p| p.0) else {
            continue;
        };
        let pier = crew.port.geometry.pier_end;
        let on_land = matches!(
            crew.phase,
            CrewPhase::CollectingProvisions | CrewPhase::Approaching
        );
        if on_land {
            // Door movement has priority until its exterior clearance is
            // physically reached. The market/shore goal must not replace it.
            if world
                .get::<crate::world::village::WorkplaceDoorTransit>(person)
                .is_some()
                || world.get::<BuildingDoorUse>(person).is_some()
            {
                if world.get::<MoveTarget>(person).is_none() {
                    settle_land_activity(world, person, true);
                }
                continue;
            }
            settle_land_activity(world, person, false);
        }
        match crew.phase {
            CrewPhase::CollectingProvisions => {
                let Some(pickup) = crew.provisions else {
                    continue;
                };
                let Some(counter) = provision_counter(world, pickup) else {
                    // No transaction has occurred; losing the selected counter
                    // releases a person on land without moving body or stock.
                    release_crew(world, crew.ship, &crew.port);
                    continue;
                };
                if !at_counter(position, counter) {
                    if world
                        .get::<MoveTarget>(person)
                        .is_none_or(|target| target.0.distance_squared(counter) > 0.01)
                    {
                        world.entity_mut(person).insert(MoveTarget(counter));
                    }
                } else {
                    world.entity_mut(person).remove::<(
                        MoveTarget,
                        TravelRoute,
                        NavigationRoutePending,
                        NavigationRouteFailed,
                    )>();
                    settle_land_activity(world, person, true);
                    crew.provision_retry_in = (crew.provision_retry_in - dt).max(0.0);
                    if crew.provision_retry_in <= 0.0 {
                        crew.provisioned_this_berth = true;
                        if let Some(porter) = world.get::<CompanyPorter>(person).copied() {
                            provision(
                                world,
                                person,
                                porter.company,
                                porter.storage_hall,
                                crew.port.settlement,
                                pickup,
                            );
                        }
                        crew.provision_retry_in = 5.0;
                        if world
                            .get::<GoodsInventory>(person)
                            .is_some_and(|food| food.edible_amount() > 0)
                        {
                            crew.provisions = None;
                            crew.phase = CrewPhase::Approaching;
                            world
                                .entity_mut(person)
                                .insert(MoveTarget(crew.port.geometry.shore));
                        }
                    }
                }
            }
            CrewPhase::Approaching => {
                if position
                    .xz()
                    .distance_squared(crew.port.geometry.shore.xz())
                    > 0.8 * 0.8
                {
                    if world.get::<MoveTarget>(person).is_none_or(|target| {
                        target.0.distance_squared(crew.port.geometry.shore) > 0.01
                    }) {
                        world
                            .entity_mut(person)
                            .insert(MoveTarget(crew.port.geometry.shore));
                    }
                    continue;
                }
                world
                    .entity_mut(person)
                    .remove::<(MoveTarget, TravelRoute, NavigationRoutePending)>();
                settle_land_activity(world, person, true);
                crew.phase = CrewPhase::BoardingPier;
            }
            CrewPhase::BoardingPier => {
                if step_pier(world, person, position, crew.port.geometry, true, dt) {
                    crew.phase = CrewPhase::BoardingHull;
                }
            }
            CrewPhase::BoardingHull => {
                if step_person(world, person, position, hull_position, dt) {
                    crew.phase = CrewPhase::Aboard;
                    if let Some(ship) = world.get::<ShipId>(crew.ship).copied() {
                        world.entity_mut(person).insert(AboardShip { ship });
                    }
                }
            }
            CrewPhase::Aboard => {
                if !at_berth(world, crew.ship, &crew.port) {
                    crew.provisioned_this_berth = false;
                }
                let yaw = world.get::<PlayerRotation>(crew.ship).cloned();
                if let Some(mut position) = world.get_mut::<PlayerPosition>(person) {
                    position.set_if_neq(PlayerPosition(hull_position));
                }
                if let Some(mut region) = world.get_mut::<RegionCoord>(person) {
                    region.set_if_neq(RegionCoord::from_world_pos(hull_position));
                }
                if let Some(mut activity) = world.get_mut::<CharacterActivity>(person) {
                    activity.set_if_neq(CharacterActivity::Idle);
                }
                if let Some(mut motion) = world.get_mut::<CharacterMotion>(person) {
                    motion.set_if_neq(CharacterMotion::STATIONARY);
                }
                if let Some(yaw) = yaw {
                    if let Some(mut rotation) = world.get_mut::<PlayerRotation>(person) {
                        rotation.set_if_neq(yaw);
                    }
                }
            }
            CrewPhase::LeavingHull | CrewPhase::ResupplyLeavingHull => {
                world.entity_mut(person).remove::<AboardShip>();
                if step_person(world, person, position, pier, dt) {
                    crew.phase = if crew.phase == CrewPhase::ResupplyLeavingHull {
                        CrewPhase::ResupplyLeavingPier
                    } else {
                        CrewPhase::LeavingPier
                    };
                }
            }
            CrewPhase::LeavingPier | CrewPhase::ResupplyLeavingPier => {
                if step_pier(world, person, position, crew.port.geometry, false, dt) {
                    if crew.phase == CrewPhase::ResupplyLeavingPier {
                        crew.phase = CrewPhase::CollectingProvisions;
                    } else {
                        world.entity_mut(person).remove::<ShipCrew>();
                        world.entity_mut(crew.ship).remove::<CrewAssignment>();
                        continue;
                    }
                }
            }
        }
        world.entity_mut(person).insert(crew);
    }
}

/// Follow the exact fixed landing / sloped approach / fixed T-head surface.
/// At high warp consume the distance across at most three profile segments.
fn pier_step(
    port: PortGeometry,
    mut from: Vec3,
    outward: bool,
    mut remaining: f32,
) -> (Vec3, bool) {
    let length = port.length();
    let points = if outward {
        [PORT_SHORE_FRONT, length - PORT_HEAD_DEPTH, length]
    } else {
        [length - PORT_HEAD_DEPTH, PORT_SHORE_FRONT, 0.]
    };
    let along = (from.xz() - port.shore.xz()).dot(port.seaward());
    for distance in points {
        if (outward && distance < along - 0.01) || (!outward && distance > along + 0.01) {
            continue;
        }
        let goal = port.deck_point(distance);
        let span = from.distance(goal);
        if span > remaining {
            return (from.move_towards(goal, remaining), false);
        }
        from = goal;
        remaining -= span;
    }
    (from, true)
}
fn step_pier(
    world: &mut World,
    person: Entity,
    from: Vec3,
    port: PortGeometry,
    outward: bool,
    dt: f32,
) -> bool {
    let (next, arrived) = pier_step(port, from, outward, BOARDING_SPEED * dt);
    step_person(world, person, from, next, dt);
    arrived
}

fn step_person(world: &mut World, person: Entity, from: Vec3, to: Vec3, dt: f32) -> bool {
    let distance = from.distance(to);
    let step = BOARDING_SPEED * dt;
    let next = from.move_towards(to, step);
    let direction = (to - from).xz().normalize_or_zero();
    let yaw = (-direction.x).atan2(-direction.y);
    world.entity_mut(person).insert((
        PlayerPosition(next),
        PlayerRotation(yaw),
        RegionCoord::from_world_pos(next),
        CharacterActivity::Idle,
        CharacterMotion::new(if dt > 0.0 {
            (next - from) / dt
        } else {
            Vec3::ZERO
        }),
    ));
    distance <= step.max(0.05)
}

fn at_counter(position: Vec3, counter: Vec3) -> bool {
    position.xz().distance_squared(counter.xz()) <= 0.7 * 0.7
        && (position.y - counter.y).abs() <= 1.5
}

fn provision_counter(world: &World, pickup: ProvisionPickup) -> Option<Vec3> {
    let settlement = world.get::<SettlementId>(pickup.hall)?;
    let position = world.get::<PlayerPosition>(pickup.counter)?.0;
    let rotation = world
        .get::<PlayerRotation>(pickup.counter)
        .map_or(0.0, |rotation| rotation.0);
    let kind = if pickup.counter == pickup.hall {
        SettlementBuildingKind::Hall
    } else {
        let building = world.get::<SettlementBuilding>(pickup.counter)?;
        if building.kind != SettlementBuildingKind::Market
            || world.get::<BuildingOf>(pickup.counter)?.0 != *settlement
            || world
                .get::<crate::world::village::UnderConstruction>(pickup.counter)
                .is_some()
        {
            return None;
        }
        building.kind
    };
    let mut point = kind.entrance_position(position, rotation);
    if let Some(terrain) = world.get_resource::<shared::terrain::WorldTerrain>() {
        point.y = terrain.get_height(point.x, point.z);
    }
    Some(point)
}

fn provision_pickup(
    world: &mut World,
    settlement: SettlementId,
    origin: Vec3,
) -> Option<ProvisionPickup> {
    let hall = world
        .query_filtered::<(Entity, &SettlementId), With<Settlement>>()
        .iter(world)
        .find(|(_, id)| **id == settlement)
        .map(|(entity, _)| entity)?;
    let fallback = ProvisionPickup {
        hall,
        counter: hall,
    };
    let hall_entrance = provision_counter(world, fallback)?;
    let candidates: Vec<_> = world.query_filtered::<(Entity, &SettlementBuilding, &BuildingOf), Without<crate::world::village::UnderConstruction>>()
        .iter(world).filter(|(_, building, owner)| building.kind == SettlementBuildingKind::Market && owner.0 == settlement)
        .filter_map(|(entity, _, _)| {
            let pickup = ProvisionPickup { hall, counter: entity };
            provision_counter(world, pickup).map(|point| (pickup, point))
        }).collect();
    let nearest = crate::world::village::nearest_public_market_entrance(
        origin,
        hall_entrance,
        candidates.iter().map(|(_, point)| *point),
    );
    Some(
        candidates
            .into_iter()
            .find(|(_, point)| *point == nearest)
            .map_or(fallback, |(pickup, _)| pickup),
    )
}

/// A company-funded purchase occurs only at the captain's real Hall/Marketplace
/// counter. Preview and commit are one transaction; berth/shore proximity alone
/// never authorizes moving pooled market stock into personal cargo.
fn provision(
    world: &mut World,
    person: Entity,
    company: CompanyId,
    warehouse: BuildingId,
    settlement: SettlementId,
    pickup: ProvisionPickup,
) {
    let Some(counter) = provision_counter(world, pickup) else {
        return;
    };
    if world.get::<SettlementId>(pickup.hall).copied() != Some(settlement)
        || !world
            .get::<PlayerPosition>(person)
            .is_some_and(|position| at_counter(position.0, counter))
    {
        return;
    }
    let held = world
        .get::<GoodsInventory>(person)
        .map_or(0, GoodsInventory::edible_amount);
    if held >= PROVISION_DAYS {
        return;
    }
    let hall = pickup.hall;
    let company_entity = world
        .query::<(Entity, &CompanyId, &CompanyAccount)>()
        .iter(world)
        .find(|(_, id, _)| **id == company)
        .map(|(entity, _, _)| entity);
    let Some(company_entity) = company_entity else {
        return;
    };
    let budget = world
        .get::<CompanyAccount>(company_entity)
        .map_or(0, |account| account.cash);
    let Some(mut market) = world.get::<MootMarket>(hall).cloned() else {
        return;
    };
    let Some(mut stock) = world.get::<GoodsInventory>(hall).cloned() else {
        return;
    };
    let Some(mut carried) = world.get::<GoodsInventory>(person).cloned() else {
        return;
    };
    let choice = Good::READY_TO_EAT_PRIORITY
        .into_iter()
        .filter_map(|good| {
            let quote = market.preview_purchase(good, 1, budget, None, None);
            (stock.amount(good) > 0 && quote.units == 1).then_some((quote.pennies, good))
        })
        .min_by_key(|(price, good)| (*price, *good as u8));
    let Some((_, good)) = choice else { return };
    let requested = (PROVISION_DAYS - held)
        .min(stock.amount(good))
        .min(carried.free_units(good));
    let purchase = market.purchase_recording_demand(good, requested, budget, None, None);
    if purchase.trade.units == 0 {
        return;
    }
    let units = purchase.trade.units;
    if stock.remove(good, units) != units || carried.add(good, units) != units {
        return;
    }
    if !world
        .get_mut::<CompanyAccount>(company_entity)
        .unwrap()
        .debit(purchase.trade.pennies)
    {
        return;
    }
    world.entity_mut(hall).insert((market, stock));
    world.entity_mut(person).insert(carried);
    let day = world
        .query::<&WorldTime>()
        .iter(world)
        .next()
        .map_or(0, |clock| clock.day);
    let cost_centre = world
        .query::<(Entity, &BuildingId, &BusinessAccount)>()
        .iter(world)
        .find(|(_, id, _)| **id == warehouse)
        .map(|(entity, _, _)| entity);
    if let Some(entity) = cost_centre {
        world
            .get_mut::<BusinessAccount>(entity)
            .unwrap()
            .record_input_purchase(day, purchase.trade.pennies, units);
    }
    world
        .resource_mut::<BusinessEventQueue>()
        .record_market_purchase(day, settlement, purchase.fills);
}

#[cfg(test)]
pub(super) mod tests;
