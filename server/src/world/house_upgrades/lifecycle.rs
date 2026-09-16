use bevy::prelude::*;
use lightyear::prelude::{NetworkTarget, Replicate};
use shared::building::PlacedBuilding;
use shared::components::*;
use shared::economy::{Good, GoodsInventory, MootMarket, Wallet};

use super::finance::*;
use super::labor::*;
use super::placement::{expansion_is_occupied, geometry_is_safe};
use super::project::*;
use crate::player::hero::MoveTarget;
use crate::world::identity::WorldIdentityIndex;
use crate::world::village::{self, BusinessEventQueue};
use crate::world::village_roads::{NavigationRouteFailed, NavigationRoutePending, TravelRoute};

const RETRY_SECONDS: f64 = 15.0;

fn person_entity(world: &mut World, id: PersonId) -> Option<Entity> {
    if let Some(entity) = world
        .get_resource::<WorldIdentityIndex>()
        .and_then(|index| index.people.get(&id))
        .copied()
    {
        if world.get::<PersonId>(entity) == Some(&id) {
            return Some(entity);
        }
    }
    world
        .query::<(Entity, &PersonId)>()
        .iter(world)
        .find_map(|(entity, candidate)| (*candidate == id).then_some(entity))
}

fn building_entity(world: &mut World, id: BuildingId) -> Option<Entity> {
    if let Some(entity) = world
        .get_resource::<WorldIdentityIndex>()
        .and_then(|index| index.buildings.get(&id))
        .copied()
    {
        if world.get::<BuildingId>(entity) == Some(&id) {
            return Some(entity);
        }
    }
    world
        .query::<(Entity, &BuildingId)>()
        .iter(world)
        .find_map(|(entity, candidate)| (*candidate == id).then_some(entity))
}

fn hall_entity(world: &mut World, id: SettlementId) -> Option<Entity> {
    if let Some(entity) = world
        .get_resource::<WorldIdentityIndex>()
        .and_then(|index| index.settlements.get(&id))
        .copied()
    {
        if world.get::<SettlementId>(entity) == Some(&id) {
            return Some(entity);
        }
    }
    world
        .query::<(Entity, &SettlementId)>()
        .iter(world)
        .find_map(|(entity, candidate)| (*candidate == id).then_some(entity))
}

pub(super) fn alive(world: &World, person: Entity) -> bool {
    world
        .get::<Health>(person)
        .is_some_and(|health| health.current > 0.0)
}

fn clock(world: &mut World) -> Option<(WorldTime, f64)> {
    let clock = world.query::<&WorldTime>().iter(world).next()?.clone();
    let now = f64::from(clock.day) * f64::from(clock.cycle_duration())
        + f64::from(clock.seconds_in_cycle);
    Some((clock, now))
}

/// One authoritative entry for both the player command and owner decisions.
/// Validation completes before private capital is moved into project escrow.
pub fn request_upgrade(
    world: &mut World,
    house: BuildingId,
    requester: PersonId,
) -> Result<(), String> {
    world.init_resource::<HouseUpgradeProjects>();
    if world
        .resource::<HouseUpgradeProjects>()
        .contains_house(house)
    {
        return Err("This house already has an extension project.".into());
    }
    let home = building_entity(world, house).ok_or("That house no longer exists.")?;
    if world
        .get::<SettlementBuilding>(home)
        .is_none_or(|building| building.kind != SettlementBuildingKind::House)
        || world.get::<village::UnderConstruction>(home).is_some()
    {
        return Err("Only a completed house can be extended.".into());
    }
    if world.get::<OwnedBy>(home).map(|owner| owner.0) != Some(requester) {
        return Err("Only the owner can commission this extension.".into());
    }
    let original = world
        .get::<HouseAppearance>(home)
        .copied()
        .ok_or("The house appearance is not ready yet.")?;
    if original.level != HouseLevel::Ground {
        return Err("This house already has its upper storey.".into());
    }
    let settlement = world
        .get::<BuildingOf>(home)
        .ok_or("The house has no settlement.")?
        .0;
    let hall = hall_entity(world, settlement).ok_or("The settlement no longer exists.")?;
    let place = world
        .get::<Settlement>(hall)
        .ok_or("The settlement is unavailable.")?;
    if place.tier < SettlementTier::Village {
        return Err("Upper storeys become available when the settlement is a Village.".into());
    }
    let settlement_name = place.name.clone();
    if world.get::<MootMarket>(hall).is_none() || world.get::<GoodsInventory>(hall).is_none() {
        return Err("The settlement market is unavailable.".into());
    }
    let owner_entity = person_entity(world, requester).ok_or("The owner is unavailable.")?;
    if !alive(world, owner_entity) {
        return Err("The owner is no longer alive.".into());
    }
    if world
        .get::<Wallet>(owner_entity)
        .is_none_or(|wallet| wallet.balance() < required_escrow_pennies())
    {
        return Err(format!(
            "Reserve {} from the owner's personal money for wood and paid construction.",
            shared::economy::format_money(required_escrow_pennies())
        ));
    }
    let position = world
        .get::<PlayerPosition>(home)
        .ok_or("The house position is unavailable.")?
        .0;
    let rotation = world
        .get::<PlayerRotation>(home)
        .map_or(0.0, |rotation| rotation.0);
    let hall_position = world
        .get::<PlayerPosition>(hall)
        .ok_or("The Hall position is unavailable.")?
        .0;
    let hall_rotation = world
        .get::<PlayerRotation>(hall)
        .map_or(0.0, |rotation| rotation.0);
    let target = HouseAppearance {
        line: original.line,
        level: HouseLevel::UpperStorey,
    };
    let stand = geometry_is_safe(
        world,
        home,
        settlement,
        target,
        position,
        rotation,
        hall_position,
        hall_rotation,
    )?;
    let (_, now) = clock(world).ok_or("The world clock is unavailable.")?;
    // Hall freight is purchased at its actual service anchor, not a reserved
    // future marketplace beside the building.
    let mut market_stand =
        SettlementBuildingKind::Hall.entrance_position(hall_position, hall_rotation);
    if let Some(terrain) = world.get_resource::<shared::terrain::WorldTerrain>() {
        market_stand.y = terrain.get_height(market_stand.x, market_stand.z);
    }
    if !world
        .get_mut::<Wallet>(owner_entity)
        .expect("validated owner wallet")
        .debit(required_escrow_pennies())
    {
        return Err("The owner's available money changed.".into());
    }
    let worksite = world
        .spawn((
            HouseUpgradeWorksite {
                house,
                owner: requester,
                target,
                wood_required: HOUSE_UPGRADE_WOOD_REQUIRED,
            },
            ConstructionSite {
                kind: SettlementBuildingKind::House,
                settlement: settlement_name,
                raising: false,
                stand,
                rotation,
            },
            BuildingOf(settlement),
            PlayerPosition(position),
            PlayerRotation(rotation),
            GoodsInventory::new(HOUSE_UPGRADE_WOOD_REQUIRED * Good::Wood.bulk_per_unit()),
            Replicate::to_clients(NetworkTarget::All),
        ))
        .id();
    world.init_resource::<BusinessEventQueue>();
    let estate_household = world
        .get::<HouseholdMember>(owner_entity)
        .map(|member| member.0);
    world.resource_mut::<HouseUpgradeProjects>().entries.insert(
        house,
        UpgradeProject {
            house,
            home,
            owner: requester,
            owner_entity,
            estate_household,
            settlement,
            hall,
            worksite,
            original,
            target,
            position,
            rotation,
            stand,
            market_stand,
            escrow: required_escrow_pennies(),
            paid_labor: 0,
            cargo: GoodsInventory::new(HOUSE_UPGRADE_WOOD_REQUIRED * Good::Wood.bulk_per_unit()),
            delivered: 0,
            worker: None,
            phase: Phase::Waiting,
            work_done: 0.0,
            last_time: now,
            next_attempt: now,
            claim: DemandClaim::default(),
        },
    );
    info!(
        "HOUSE_UPGRADE accepted house={} owner={} reserve={} site={:?}",
        house.0,
        requester.0,
        required_escrow_pennies(),
        worksite
    );
    Ok(())
}

/// Exercise explicit cancellation through the same path as runtime invalidation.
#[cfg(test)]
pub(super) fn cancel_upgrade(
    world: &mut World,
    house: BuildingId,
    requester: PersonId,
) -> Result<(), String> {
    let Some(book) = world.get_resource::<HouseUpgradeProjects>() else {
        return Err("No extension project exists.".into());
    };
    if book
        .entries
        .get(&house)
        .is_none_or(|project| project.owner != requester)
    {
        return Err("Only the commissioning owner can cancel this extension.".into());
    }
    world.resource_scope(|world, mut book: Mut<HouseUpgradeProjects>| {
        let mut project = book.entries.remove(&house).expect("validated project");
        begin_refund(world, &mut project);
        if !settle_refund(world, &mut project) {
            book.entries.insert(house, project);
        }
    });
    Ok(())
}

/// Only active projects are visited. Whole-roster candidate searches happen
/// when a contract needs a worker: at most one roster search globally per tick,
/// with round-robin review so accelerated time cannot starve later projects.
pub fn run_house_upgrade_projects(world: &mut World) {
    if world
        .get_resource::<HouseUpgradeProjects>()
        .is_none_or(|book| book.entries.is_empty())
    {
        return;
    }
    let Some((clock, now)) = clock(world) else {
        return;
    };
    let mut personal_needs =
        world.query_filtered::<(), village::worker_activity::PersonalNeedsOwnMovement>();
    world.resource_scope(|world, mut book: Mut<HouseUpgradeProjects>| {
        let ready = |project: &&UpgradeProject| {
            project.phase == Phase::Waiting && now >= project.next_attempt
        };
        let reviewed = if clock.is_day() {
            book.entries
                .iter()
                .filter(|(id, _)| book.last_worker_reviewed.is_none_or(|last| **id > last))
                .find(|(_, project)| ready(project))
                .or_else(|| book.entries.iter().find(|(_, project)| ready(project)))
                .map(|(id, _)| *id)
        } else {
            None
        };
        if reviewed.is_some() {
            book.last_worker_reviewed = reviewed;
        }
        book.entries.retain(|id, project| {
            let dt = (now - project.last_time).clamp(0.0, 60.0) as f32;
            project.last_time = now;
            let needs_own_movement = project
                .worker
                .is_some_and(|worker| personal_needs.get(world, worker).is_ok());
            !step_project(
                world,
                project,
                &clock,
                now,
                dt,
                reviewed == Some(*id),
                needs_own_movement,
            )
        });
    });
}

fn step_project(
    world: &mut World,
    project: &mut UpgradeProject,
    clock: &WorldTime,
    now: f64,
    dt: f32,
    review_worker: bool,
    needs_own_movement: bool,
) -> bool {
    if project.phase == Phase::Refunding {
        if now < project.next_attempt {
            return false;
        }
        project.next_attempt = now + RETRY_SECONDS;
        return settle_refund(world, project);
    }
    let valid = world.get::<BuildingId>(project.home) == Some(&project.house)
        && world.get::<OwnedBy>(project.home).map(|owner| owner.0) == Some(project.owner)
        && world.get::<HouseAppearance>(project.home) == Some(&project.original)
        && world
            .get::<HouseUpgradeWorksite>(project.worksite)
            .is_some()
        && world.get::<ConstructionSite>(project.worksite).is_some()
        && world.get::<GoodsInventory>(project.worksite).is_some()
        && world.get::<MootMarket>(project.hall).is_some()
        && world.get::<GoodsInventory>(project.hall).is_some()
        && world.get::<PlayerPosition>(project.hall).is_some()
        && world.get::<PersonId>(project.owner_entity) == Some(&project.owner)
        && alive(world, project.owner_entity)
        && world
            .get::<Settlement>(project.hall)
            .is_some_and(|hall| hall.tier >= SettlementTier::Village);
    if !valid {
        begin_refund(world, project);
        return settle_refund(world, project);
    }
    if let Some(worker) = project.worker {
        let still_assigned = alive(world, worker)
            && world.get::<Wallet>(worker).is_some()
            && world.get::<PlayerPosition>(worker).is_some()
            && world
                .get::<HouseUpgradeBuilderRoutine>(worker)
                .is_some_and(|routine| routine.project == project.worksite)
            && world.get::<ResidentOf>(worker).map(|of| of.0) == Some(project.settlement)
            && world.get::<EmployedAt>(worker).is_none()
            && world.get::<CivicEmployment>(worker).is_none();
        if !still_assigned {
            // Never strand a worker or resurrect a destroyed body's personal
            // inventory. Transit wood belongs to this retained project.
            begin_refund(world, project);
            return settle_refund(world, project);
        }
        // The paid meal/shopping routine owns the current route, activity and
        // any navigation failure. Keep the project and its cargo/progress, but
        // let last_time advance so the interruption cannot earn building work.
        // Every productive phase rechecks its physical destination on resume.
        if needs_own_movement {
            return false;
        }
        if world.get::<NavigationRouteFailed>(worker).is_some() {
            begin_refund(world, project);
            return settle_refund(world, project);
        }
    }
    if !clock.is_day() && project.cargo.amount(Good::Wood) == 0 {
        release_worker(world, project);
        project.phase = Phase::Waiting;
        return false;
    }
    match project.phase {
        Phase::Waiting => {
            if !review_worker || now < project.next_attempt {
                return false;
            }
            project.next_attempt = now + RETRY_SECONDS;
            refresh_claim(world, project);
            let ready = project.delivered >= HOUSE_UPGRADE_WOOD_REQUIRED;
            if !ready && affordable_batch(world, project) == 0 {
                return false;
            }
            let Some(worker) = choose_worker(world, project) else {
                return false;
            };
            project.worker = Some(worker);
            info!(
                "HOUSE_UPGRADE assigned house={} worker={} name={}",
                project.house.0,
                world.get::<PersonId>(worker).map_or(0, |id| id.0),
                world
                    .get::<CharacterName>(worker)
                    .map_or("unnamed", |name| name.0.as_str())
            );
            world
                .entity_mut(worker)
                .remove::<village::ambient::AmbientRoutine>()
                .remove::<MoveTarget>()
                .remove::<TravelRoute>()
                .remove::<NavigationRoutePending>()
                .remove::<NavigationRouteFailed>()
                .insert(HouseUpgradeBuilderRoutine {
                    project: project.worksite,
                    carrying: false,
                });
            start_trip(
                world,
                project,
                if ready {
                    Phase::ToSite
                } else {
                    Phase::ToMarket
                },
            );
        }
        Phase::ToMarket => {
            if !arrive(world, project, project.market_stand) {
                return false;
            }
            withdraw_claim(world, project);
            let requested = affordable_batch(world, project);
            if requested == 0 {
                release_worker(world, project);
                project.phase = Phase::Waiting;
                project.next_attempt = now + RETRY_SECONDS;
                return false;
            }
            let budget = material_budget(project);
            let purchase = world
                .get_mut::<MootMarket>(project.hall)
                .expect("live market")
                .purchase(
                    Good::Wood,
                    requested,
                    budget,
                    Some(Good::Wood.base_price()),
                    None,
                );
            let amount = world
                .get_mut::<GoodsInventory>(project.hall)
                .expect("live Hall stock")
                .remove(Good::Wood, purchase.trade.units);
            debug_assert_eq!(amount, purchase.trade.units);
            project.escrow -= purchase.trade.pennies;
            info!(
                "HOUSE_UPGRADE pickup house={} wood={} paid={}",
                project.house.0, amount, purchase.trade.pennies
            );
            let stored = project.cargo.add(Good::Wood, amount);
            debug_assert_eq!(stored, amount);
            world
                .resource_mut::<BusinessEventQueue>()
                .record_market_purchase(clock.day, project.settlement, purchase.fills);
            if let Some(worker) = project.worker {
                world
                    .get_mut::<HouseUpgradeBuilderRoutine>(worker)
                    .expect("assigned builder")
                    .carrying = amount > 0;
            }
            start_trip(world, project, Phase::ToSite);
        }
        Phase::ToSite => {
            if !arrive(world, project, project.stand) {
                return false;
            }
            let amount = project.cargo.remove(Good::Wood, u32::MAX);
            let stored = world
                .get_mut::<GoodsInventory>(project.worksite)
                .expect("live worksite")
                .add(Good::Wood, amount);
            debug_assert_eq!(stored, amount);
            project.delivered += stored;
            info!(
                "HOUSE_UPGRADE delivered house={} wood={} total={}",
                project.house.0, stored, project.delivered
            );
            if let Some(worker) = project.worker {
                world
                    .get_mut::<HouseUpgradeBuilderRoutine>(worker)
                    .expect("assigned builder")
                    .carrying = false;
            }
            if project.delivered < HOUSE_UPGRADE_WOOD_REQUIRED {
                start_trip(world, project, Phase::ToMarket);
            } else {
                project.phase = Phase::Working;
            }
        }
        Phase::Working => {
            if !clock.is_day() || !arrive(world, project, project.stand) {
                return false;
            }
            let worker = project.worker.expect("working builder");
            if world.get::<CharacterActivity>(worker) != Some(&CharacterActivity::Building) {
                world.entity_mut(worker).insert(CharacterActivity::Building);
            }
            let direction = project.position - project.stand;
            let yaw = (-direction.x).atan2(-direction.z);
            if world
                .get::<PlayerRotation>(worker)
                .is_none_or(|rotation| rotation.0 != yaw)
            {
                world.entity_mut(worker).insert(PlayerRotation(yaw));
            }
            if world
                .get::<ConstructionSite>(project.worksite)
                .is_some_and(|site| !site.raising)
            {
                world
                    .get_mut::<ConstructionSite>(project.worksite)
                    .expect("live site")
                    .raising = true;
            }
            project.work_done = (project.work_done + dt).min(HOUSE_UPGRADE_WORK_SECONDS);
            let earned = ((f64::from(project.work_done) / f64::from(HOUSE_UPGRADE_WORK_SECONDS))
                * HOUSE_UPGRADE_BUILDER_FEE_PENNIES as f64)
                .floor() as u64;
            let paid = earned
                .saturating_sub(project.paid_labor)
                .min(project.escrow);
            if paid > 0 {
                if let Some(mut wallet) = world.get_mut::<Wallet>(worker) {
                    wallet.credit(paid);
                    project.escrow -= paid;
                    project.paid_labor += paid;
                }
            }
            if project.work_done < HOUSE_UPGRADE_WORK_SECONDS {
                return false;
            }
            if now < project.next_attempt {
                return false;
            }
            project.next_attempt = now + 1.0;
            if expansion_is_occupied(
                world,
                project.original,
                project.target,
                project.position,
                project.rotation,
            ) {
                return false;
            }
            let hall_position = world
                .get::<PlayerPosition>(project.hall)
                .expect("live Hall position")
                .0;
            let hall_rotation = world
                .get::<PlayerRotation>(project.hall)
                .map_or(0.0, |yaw| yaw.0);
            if geometry_is_safe(
                world,
                project.home,
                project.settlement,
                project.target,
                project.position,
                project.rotation,
                hall_position,
                hall_rotation,
            )
            .is_err()
            {
                begin_refund(world, project);
                return settle_refund(world, project);
            }
            let consumed = world
                .get_mut::<GoodsInventory>(project.worksite)
                .expect("staged worksite")
                .remove(Good::Wood, HOUSE_UPGRADE_WOOD_REQUIRED);
            debug_assert_eq!(consumed, HOUSE_UPGRADE_WOOD_REQUIRED);
            project.delivered = 0;
            info!(
                "HOUSE_UPGRADE completed house={} owner={} wood={} labor_paid={}",
                project.house.0, project.owner.0, consumed, project.paid_labor
            );
            world.entity_mut(project.home).insert((
                project.target,
                PlacedBuilding {
                    building_type: project.target.building_type(),
                    rotation: project.rotation,
                },
            ));
            begin_refund(world, project);
            return settle_refund(world, project);
        }
        Phase::Refunding => unreachable!(),
    }
    false
}
