//! One physical shopping trip per household, with explicitly owned cargo.

use super::needs::{HearthState, ProvisionNeeds};
use super::provisioning::{plan_basket, purchase_basket};
use super::*;
use shared::components::{BuildingId, HouseholdId, HouseholdMembers};

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn run_household_shopping(
    mut commands: Commands,
    world_time: Query<&WorldTime>,
    mut business_events: ResMut<BusinessEventQueue>,
    mut accounts: Query<(&HouseholdId, &HouseholdMembers, &mut HouseholdEconomy)>,
    mut halls: Query<
        (
            &shared::components::SettlementId,
            Option<&PlayerPosition>,
            Option<&PlayerRotation>,
            &mut GoodsInventory,
            &mut MootMarket,
        ),
        (
            With<Settlement>,
            Without<SettlementBuilding>,
            Without<CharacterKind>,
        ),
    >,
    mut houses: Query<
        (
            Entity,
            &BuildingId,
            &SettlementBuilding,
            &PlayerPosition,
            &PlayerRotation,
            &Household,
            &mut GoodsInventory,
            Option<&HearthState>,
        ),
        (Without<CharacterKind>, Without<Settlement>),
    >,
    mut shoppers: Query<
        (
            Entity,
            &PlayerPosition,
            &mut CharacterActivity,
            &mut GoodsInventory,
            &mut HouseholdShoppingRoutine,
            Option<&MootQueueTicket>,
            Option<&MoveTarget>,
            Option<&HomeRoutine>,
            Option<&NavigationRouteFailed>,
            Option<&ConstructionMaterialRoutine>,
        ),
        With<CharacterKind>,
    >,
) {
    if shoppers.is_empty() {
        return;
    }
    let day = world_time.iter().next().map_or(0, |clock| clock.day);
    for (
        shopper,
        position,
        mut activity,
        mut carrier,
        mut routine,
        ticket,
        target,
        resting,
        failed,
        material,
    ) in &mut shoppers
    {
        if resting.is_some() {
            continue;
        }
        // Admission normally prevents this overlap, but an assigned site or
        // retained save can coexist with an already owned household basket.
        // Use the same safe cargo boundary as paid meals and queue admission.
        if material.is_some_and(ConstructionMaterialRoutine::finishes_before_personal_needs) {
            continue;
        }
        let account_valid = accounts
            .get(routine.account)
            .is_ok_and(|(id, ..)| *id == routine.household);
        let dwelling = accounts
            .get(routine.account)
            .ok()
            .filter(|(id, ..)| **id == routine.household)
            .and_then(|(_, group, _)| group.dwelling);
        let home_entity = dwelling.and_then(|id| {
            if houses
                .get(routine.home)
                .is_ok_and(|(_, actual, ..)| *actual == id)
            {
                Some(routine.home)
            } else {
                houses
                    .iter()
                    .find(|(_, actual, ..)| **actual == id)
                    .map(|(entity, ..)| entity)
            }
        });
        if home_entity.is_none() || routine.phase == HouseholdShoppingPhase::ReturningToMarket {
            if !routine.has_cargo() {
                // An unfilled order owns no goods. Release the member so a
                // missing house cannot prevent other work.
                commands
                    .entity(shopper)
                    .remove::<HouseholdShoppingRoutine>()
                    .remove::<MoveTarget>()
                    .remove::<TravelRoute>()
                    .remove::<NavigationRoutePending>()
                    .remove::<NavigationRouteFailed>()
                    .remove::<MootQueueTicket>();
                continue;
            }
            let Ok((settlement, hall_position, hall_rotation, mut hall, mut market)) =
                halls.get_mut(routine.hall)
            else {
                continue;
            };
            let counter = hall_position.map_or(routine.counter, |position| {
                SettlementBuildingKind::Hall
                    .entrance_position(position.0, hall_rotation.map_or(0.0, |rotation| rotation.0))
            });
            if routine.phase != HouseholdShoppingPhase::ReturningToMarket || failed.is_some() {
                routine.phase = HouseholdShoppingPhase::ReturningToMarket;
                commands
                    .entity(shopper)
                    .remove::<MootQueueTicket>()
                    .remove::<TravelRoute>()
                    .remove::<NavigationRoutePending>()
                    .remove::<NavigationRouteFailed>()
                    .insert(MoveTarget(counter));
            }
            activity.set_if_neq(CharacterActivity::Idle);
            if ground_distance(position.0, counter) > WORK_REACH {
                ensure_move_target(&mut commands, shopper, target, counter);
                continue;
            }
            // This is a consignment, not a refund: no money moves until a real
            // buyer clears these goods. Hall capacity remains the physical limit.
            let seller = if account_valid {
                MarketSeller::Household(routine.household)
            } else {
                MarketSeller::Treasury(*settlement)
            };
            for good in Good::ALL {
                let index = good.index();
                let owned = routine.cargo[index].min(carrier.amount(good));
                let moved = carrier.transfer_to(&mut hall, good, owned);
                if moved > 0 {
                    let price = market.pool(good).ask.max(1);
                    market.consign(seller, good, moved, price);
                }
                routine.cargo[index] = owned - moved;
            }
            if !routine.has_cargo() {
                commands
                    .entity(shopper)
                    .remove::<HouseholdShoppingRoutine>()
                    .remove::<MoveTarget>()
                    .remove::<TravelRoute>()
                    .remove::<NavigationRoutePending>()
                    .remove::<NavigationRouteFailed>()
                    .remove::<MootQueueTicket>();
            }
            continue;
        }
        let home_entity = home_entity.expect("a missing dwelling was handled above");
        let Ok((_, group, mut economy)) = accounts.get_mut(routine.account) else {
            continue;
        };
        if routine.home != home_entity {
            routine.home = home_entity;
            commands
                .entity(shopper)
                .remove::<MoveTarget>()
                .remove::<TravelRoute>()
                .remove::<NavigationRoutePending>()
                .remove::<NavigationRouteFailed>();
        }
        let Ok((_, _, building, home_position, rotation, occupants, mut pantry, hearth)) =
            houses.get_mut(home_entity)
        else {
            continue;
        };
        let entrance = building.kind.entrance_position(home_position.0, rotation.0);
        if failed.is_some() && ticket.is_none() {
            let mut entity = commands.entity(shopper);
            entity
                .remove::<MoveTarget>()
                .remove::<TravelRoute>()
                .remove::<NavigationRoutePending>()
                .remove::<NavigationRouteFailed>();
            match routine.phase {
                HouseholdShoppingPhase::GoingToMarket => {
                    entity.remove::<HouseholdShoppingRoutine>();
                }
                HouseholdShoppingPhase::ReturningHome => {
                    entity.insert(MoveTarget(entrance));
                }
                HouseholdShoppingPhase::ReturningToMarket => unreachable!("handled above"),
            }
            continue;
        }
        activity.set_if_neq(CharacterActivity::Idle);
        match routine.phase {
            HouseholdShoppingPhase::GoingToMarket => {
                let Ok((settlement, _, _, mut hall, mut market)) = halls.get_mut(routine.hall)
                else {
                    commands
                        .entity(shopper)
                        .remove::<HouseholdShoppingRoutine>()
                        .remove::<MootQueueTicket>()
                        .remove::<MoveTarget>();
                    continue;
                };
                if *settlement != group.settlement {
                    commands
                        .entity(shopper)
                        .remove::<HouseholdShoppingRoutine>()
                        .remove::<MootQueueTicket>()
                        .remove::<MoveTarget>();
                    continue;
                }
                if let Some(ticket) = ticket {
                    if !ticket.is_ready() {
                        continue;
                    }
                } else if ground_distance(position.0, routine.counter) > WORK_REACH {
                    ensure_move_target(&mut commands, shopper, target, routine.counter);
                    continue;
                }
                // Stable membership includes people travelling or serving in a
                // battalion. Buy for the people actually occupying this home.
                let n = occupants.resident_ids.len();
                let initial_hearth = HearthState::default();
                let needs = ProvisionNeeds::for_home(
                    n,
                    &economy,
                    &pantry,
                    hearth.unwrap_or(&initial_hearth),
                );
                let basket = plan_basket(
                    &market,
                    &hall,
                    needs,
                    economy.pennies,
                    economy.pennies,
                    carrier.free_bulk().min(pantry.free_bulk()),
                );
                routine.cargo = purchase_basket(
                    &basket,
                    day,
                    *settlement,
                    &mut market,
                    &mut hall,
                    &mut carrier,
                    &mut economy,
                    &mut business_events,
                );
                if routine.cargo.iter().all(|units| *units == 0) {
                    commands
                        .entity(shopper)
                        .remove::<HouseholdShoppingRoutine>()
                        .remove::<MootQueueTicket>()
                        .remove::<MoveTarget>();
                    continue;
                }
                commands
                    .entity(shopper)
                    .insert(MoveTarget(entrance))
                    .remove::<MootQueueTicket>()
                    .remove::<TravelRoute>()
                    .remove::<NavigationRoutePending>()
                    .remove::<NavigationRouteFailed>();
                routine.phase = HouseholdShoppingPhase::ReturningHome;
            }
            HouseholdShoppingPhase::ReturningHome => {
                if ground_distance(position.0, entrance) > DOOR_REACH {
                    ensure_move_target(&mut commands, shopper, target, entrance);
                    continue;
                }
                for good in Good::ALL {
                    let index = good.index();
                    let owned = routine.cargo[index].min(carrier.amount(good));
                    let delivered = carrier.transfer_to(&mut pantry, good, owned);
                    routine.cargo[index] = owned - delivered;
                }
                if routine.cargo.iter().all(|amount| *amount == 0) {
                    commands
                        .entity(shopper)
                        .remove::<HouseholdShoppingRoutine>()
                        .remove::<MoveTarget>();
                }
            }
            HouseholdShoppingPhase::ReturningToMarket => unreachable!("handled above"),
        }
    }
}
