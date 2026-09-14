//! Scheduled household procurement. Real cash funds a basket, and repeated
//! visits never multiply one household's daily shortage into fictitious demand.

use super::needs::{fair_contributions, HearthState};
use super::*;
use shared::components::{BuildingId, HouseholdId, HouseholdMembers, PersonId, SettlementId};

#[derive(Default)]
struct Claim {
    good: Option<Good>,
    counts: [u32; 3],
}

#[derive(Default)]
struct Review {
    next_minute: u64,
    market: Option<Entity>,
    market_epoch: Option<u64>,
    food_claim: Claim,
    fuel_claim: Claim,
}

impl Review {
    fn withdraw(&mut self, market: &mut MootMarket) {
        if self.market_epoch == Some(market.demand_epoch()) {
            for claim in [&mut self.food_claim, &mut self.fuel_claim] {
                if let Some(good) = claim.good.take() {
                    market.withdraw_unmet_demand(
                        good,
                        claim.counts[0],
                        claim.counts[1],
                        claim.counts[2],
                    );
                }
            }
        }
        self.food_claim = Claim::default();
        self.fuel_claim = Claim::default();
    }
}

#[derive(Default)]
pub(crate) struct ProvisioningClock {
    minute: Option<u64>,
    reviews: HashMap<Entity, Review>,
}

pub(super) struct Basket {
    pub amounts: [u32; Good::COUNT],
    pub pennies: u64,
    pub food_remaining: u32,
    pub fuel_remaining: u32,
}

pub(super) fn plan_basket(
    market: &MootMarket,
    hall: &GoodsInventory,
    food: u32,
    fuel: u32,
    budget: u64,
    room: u32,
) -> Basket {
    let mut basket = Basket {
        amounts: [0; Good::COUNT],
        pennies: 0,
        food_remaining: food,
        fuel_remaining: fuel,
    };
    let mut room = room;
    for good in household_food_purchase_order(hall, market) {
        let requested = basket
            .food_remaining
            .min(room / good.bulk_per_unit())
            .min(hall.amount(good));
        let trade = market.preview_purchase(
            good,
            requested,
            budget.saturating_sub(basket.pennies),
            None,
            None,
        );
        basket.amounts[good.index()] = trade.units;
        basket.pennies = basket.pennies.saturating_add(trade.pennies);
        basket.food_remaining -= trade.units;
        room = room.saturating_sub(trade.units * good.bulk_per_unit());
    }
    // A reserve of household fuel must never outbid its missing food reserve.
    if basket.food_remaining == 0 {
        let good = Good::Wood;
        let requested = fuel.min(room / good.bulk_per_unit()).min(hall.amount(good));
        let trade = market.preview_purchase(
            good,
            requested,
            budget.saturating_sub(basket.pennies),
            None,
            None,
        );
        basket.amounts[good.index()] = trade.units;
        basket.pennies = basket.pennies.saturating_add(trade.pennies);
        basket.fuel_remaining -= trade.units;
    }
    basket
}

pub(super) fn purchase_basket(
    basket: &Basket,
    day: u32,
    settlement: SettlementId,
    market: &mut MootMarket,
    hall: &mut GoodsInventory,
    destination: &mut GoodsInventory,
    economy: &mut HouseholdEconomy,
    events: &mut BusinessEventQueue,
) -> [u32; Good::COUNT] {
    let mut cargo = [0; Good::COUNT];
    for good in Good::HOUSEHOLD_FOOD_PRIORITY
        .into_iter()
        .chain(std::iter::once(Good::Wood))
    {
        let requested = basket.amounts[good.index()]
            .min(destination.free_bulk() / good.bulk_per_unit())
            .min(hall.amount(good));
        if requested == 0 {
            continue;
        }
        let purchase = market.purchase(good, requested, economy.pennies, None, None);
        economy.pennies -= purchase.trade.pennies;
        let moved = hall.transfer_to(destination, good, purchase.trade.units);
        debug_assert_eq!(moved, purchase.trade.units);
        cargo[good.index()] = moved;
        events.record_market_purchase(day, settlement, purchase.fills);
    }
    cargo
}

fn record_claim(
    market: &mut MootMarket,
    good: Good,
    missing: u32,
    stock: u32,
    budget: u64,
    previous: &mut Claim,
) {
    // Replace only this household's current outstanding observation. A restock
    // can turn unavailable units into price rejection, change the cheapest
    // substitute or satisfy them entirely; none is a second order.
    if let Some(old_good) = previous.good {
        market.withdraw_unmet_demand(
            old_good,
            previous.counts[0],
            previous.counts[1],
            previous.counts[2],
        );
    }
    *previous = Claim::default();
    if missing == 0 || !market.can_trade(good) {
        return;
    }
    let unavailable = missing.saturating_sub(stock);
    let unaffordable = missing.min(stock);
    let price = good.base_price().max(1);
    let funded = missing.min((budget / price).min(u64::from(u32::MAX)) as u32);
    market.record_unmet_demand(good, unavailable, unaffordable, funded, price);
    previous.good = Some(good);
    previous.counts = [unavailable, unaffordable, funded];
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn update_household_budgets_and_pantries(
    mut commands: Commands,
    world_time: Query<&WorldTime>,
    mut runtime: Local<ProvisioningClock>,
    mut business_events: ResMut<BusinessEventQueue>,
    mut queue_clock: Option<ResMut<MootQueueClock>>,
    regions: Option<Res<RegionRegistry>>,
    mut accounts: Query<(
        Entity,
        &HouseholdId,
        &HouseholdMembers,
        &mut HouseholdEconomy,
    )>,
    mut halls: Query<
        (
            Entity,
            &SettlementId,
            &PlayerPosition,
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
            &mut GoodsInventory,
            Option<&mut HearthState>,
        ),
        (Without<CharacterKind>, Without<Settlement>),
    >,
    marketplaces: Query<
        (
            &SettlementBuilding,
            &shared::components::BuildingOf,
            &PlayerPosition,
            &PlayerRotation,
        ),
        Without<Household>,
    >,
    mut residents: Query<(
        Entity,
        &PersonId,
        &mut Wallet,
        Option<&WorkStatus>,
        Option<&HomeAssignment>,
        Option<&RegionCoord>,
        Option<&HouseholdShoppingRoutine>,
    )>,
    busy: Query<
        (),
        Or<(
            With<ConstructionMaterialRoutine>,
            With<RoadBuilderRoutine>,
            With<HomeRoutine>,
            With<WorkplaceDoorTransit>,
            With<MarketCollectionRoutine>,
            With<InternalDeliveryRoutine>,
            With<MootQueueTicket>,
            With<MootMealRoutine>,
            With<TradeRouteRoutine>,
            With<crate::world::house_upgrades::HouseUpgradeBuilderRoutine>,
        )>,
    >,
) {
    let Some(clock) = world_time.iter().next() else {
        return;
    };
    let minute = u64::from(clock.day) * 1440
        + (clock.seconds_in_cycle / clock.cycle_duration().max(0.001) * 1440.0).floor() as u64;
    // One cheap gate before any indexes or roster scans. The 60 Hz schedule
    // does not mean 60 Hz economic decisions, including under time warp.
    if runtime.minute == Some(minute) {
        return;
    }
    runtime.minute = Some(minute);
    let day = clock.day;
    runtime.reviews.retain(|entity, review| {
        if accounts.contains(*entity) {
            return true;
        }
        if let Some(hall) = review.market {
            if let Ok((_, _, _, _, _, mut market)) = halls.get_mut(hall) {
                review.withdraw(&mut market);
            }
        }
        false
    });
    let occupied: HashSet<_> = accounts
        .iter()
        .filter_map(|(_, _, group, _)| group.dwelling)
        .collect();
    for (_, id, _, _, _, _, hearth) in &mut houses {
        if !occupied.contains(id) {
            if let Some(mut hearth) = hearth {
                hearth.vacant_until(day);
            }
        }
    }
    let house_by_id: HashMap<_, _> = houses
        .iter()
        .map(|(entity, id, ..)| (*id, entity))
        .collect();
    let hall_by_id: HashMap<_, _> = halls.iter().map(|(entity, id, ..)| (*id, entity)).collect();
    let resident_by_id: HashMap<_, _> = residents
        .iter()
        .map(|(entity, id, ..)| (*id, entity))
        .collect();
    let active_trips: HashSet<_> = residents
        .iter()
        .filter_map(|(_, _, _, _, _, _, routine)| routine.map(|routine| routine.household))
        .collect();
    for (account_entity, id, group, mut economy) in &mut accounts {
        let review = runtime.reviews.entry(account_entity).or_default();
        if minute < review.next_minute {
            continue;
        }
        review.next_minute = minute + 60 + (id.0 % 13);
        let Some(hall_entity) = hall_by_id.get(&group.settlement).copied() else {
            continue;
        };
        if review.market != Some(hall_entity) {
            if let Some(previous_hall) = review.market {
                if let Ok((_, _, _, _, _, mut previous_market)) = halls.get_mut(previous_hall) {
                    review.withdraw(&mut previous_market);
                }
            }
            review.market = Some(hall_entity);
            review.market_epoch = None;
        }
        let Ok((_, _, hall_position, hall_rotation, mut hall_store, mut market)) =
            halls.get_mut(hall_entity)
        else {
            continue;
        };
        if review.market_epoch != Some(market.demand_epoch()) {
            review.market_epoch = Some(market.demand_epoch());
            review.food_claim = Claim::default();
            review.fuel_claim = Claim::default();
        }
        let Some(home_entity) = group
            .dwelling
            .and_then(|home| house_by_id.get(&home).copied())
        else {
            review.withdraw(&mut market);
            // The house is gone, but the shared account still supports its
            // members. Their ordinary unhoused meal purchases remain physical.
            let meal_price = Good::READY_TO_EAT_PRIORITY
                .into_iter()
                .filter_map(|good| {
                    market
                        .listings()
                        .iter()
                        .filter(|listing| listing.good == good && listing.units > 0)
                        .map(|listing| listing.unit_price)
                        .min()
                })
                .min();
            if let Some(price) = meal_price {
                for person in &group.resident_ids {
                    let Some(entity) = resident_by_id.get(person).copied() else {
                        continue;
                    };
                    let Ok((_, _, mut wallet, _, _, _, _)) = residents.get_mut(entity) else {
                        continue;
                    };
                    let needed = price.saturating_sub(wallet.balance()).min(economy.pennies);
                    if needed > 0 {
                        economy.pennies -= needed;
                        wallet.credit(needed);
                    }
                }
            }
            continue;
        };
        let Ok((_, _, _, home_position, _, mut pantry, hearth)) = houses.get_mut(home_entity)
        else {
            continue;
        };
        let members: Vec<_> = group
            .resident_ids
            .iter()
            .filter_map(|person| {
                let entity = resident_by_id.get(person).copied()?;
                let (_, _, wallet, work, home, region, _) = residents.get(entity).ok()?;
                if !home.is_some_and(|home| home.home == home_entity) {
                    return None;
                }
                Some((
                    *person,
                    entity,
                    wallet.balance(),
                    work.copied().unwrap_or_default(),
                    region.copied(),
                ))
            })
            .collect();
        let n = members.len();
        let mut initial_hearth = HearthState::default();
        let mut existing_hearth = hearth;
        let hearth = existing_hearth
            .as_deref_mut()
            .unwrap_or(&mut initial_hearth);
        hearth.advance(day, n, &mut pantry, &mut economy);
        let fuel_deficit = hearth.deficit(n, economy.fuel_target_days, &pantry);
        if existing_hearth.is_none() {
            commands.entity(home_entity).insert(initial_hearth);
        }
        if n == 0 {
            review.withdraw(&mut market);
            continue;
        }
        economy.last_budget_day = day;
        let food_deficit = (n as u32)
            .saturating_mul(u32::from(economy.pantry_target_days))
            .saturating_sub(pantry.edible_amount());
        if pantry.edible_amount() < n as u32 {
            review.next_minute = minute + 12 + (id.0 % 7);
        }
        if active_trips.contains(id) {
            continue;
        }
        if food_deficit == 0 && fuel_deficit == 0 {
            review.withdraw(&mut market);
            continue;
        }
        let floor = if pantry.edible_amount() < n as u32 {
            0
        } else {
            2 * PENNIES_PER_COIN
        };
        let available: Vec<_> = members
            .iter()
            .map(|(person, _, cash, ..)| (*person, cash.saturating_sub(floor)))
            .collect();
        let spendable = available
            .iter()
            .map(|(_, amount)| *amount)
            .fold(economy.pennies, u64::saturating_add);
        let basket = plan_basket(
            &market,
            &hall_store,
            food_deficit,
            fuel_deficit,
            spendable,
            pantry.free_bulk(),
        );
        let missing_food_good = household_food_purchase_order(&hall_store, &market)
            .into_iter()
            .find(|good| {
                hall_store
                    .amount(*good)
                    .saturating_sub(basket.amounts[good.index()])
                    > 0
            })
            .unwrap_or(Good::Bread);
        let food_stock = hall_store
            .amount(missing_food_good)
            .saturating_sub(basket.amounts[missing_food_good.index()]);
        let after_basket = spendable.saturating_sub(basket.pennies);
        // Storage blockage is a domestic need, not demand that a cheaper
        // offer or additional production can satisfy. Only publish units
        // which could still fit after the planned purchase.
        let remaining_room = pantry.free_bulk().saturating_sub(
            Good::ALL
                .into_iter()
                .map(|good| basket.amounts[good.index()] * good.bulk_per_unit())
                .sum::<u32>(),
        );
        let food_shortfall = basket
            .food_remaining
            .min(remaining_room / missing_food_good.bulk_per_unit());
        record_claim(
            &mut market,
            missing_food_good,
            food_shortfall,
            food_stock,
            after_basket,
            &mut review.food_claim,
        );
        let food_protection =
            u64::from(basket.food_remaining).saturating_mul(missing_food_good.base_price());
        let fuel_budget = after_basket.saturating_sub(food_protection);
        if basket.food_remaining == 0 {
            let remaining_stock = hall_store
                .amount(Good::Wood)
                .saturating_sub(basket.amounts[Good::Wood.index()]);
            record_claim(
                &mut market,
                Good::Wood,
                basket
                    .fuel_remaining
                    .min(remaining_room / Good::Wood.bulk_per_unit()),
                remaining_stock,
                fuel_budget,
                &mut review.fuel_claim,
            );
        } else {
            record_claim(&mut market, Good::Wood, 0, 0, 0, &mut review.fuel_claim);
        }
        if basket.pennies == 0 {
            continue;
        }
        let tactical = members.iter().any(|(_, _, _, _, region)| {
            region.is_some_and(|region| {
                regions.as_ref().is_some_and(|registry| {
                    registry
                        .get(region)
                        .is_some_and(|state| state.sim_level == SimLevel::Tactical)
                })
            })
        });
        let shopper = members
            .iter()
            .filter(|(_, entity, ..)| busy.get(*entity).is_err())
            .min_by_key(|(person, _, _, work, _)| {
                let rank = match work {
                    WorkStatus::Chilling => 0,
                    WorkStatus::LookingForWork => 1,
                    WorkStatus::Employed => 2,
                };
                (
                    rank,
                    shared::worldgen::splitmix64(person.0 ^ u64::from(day)),
                )
            });
        if tactical && (!clock.is_day() || shopper.is_none()) {
            continue;
        }
        let needed = basket.pennies.saturating_sub(economy.pennies);
        for (person, amount) in fair_contributions(&available, needed, day) {
            let Some(entity) = resident_by_id.get(&person).copied() else {
                continue;
            };
            let Ok((_, _, mut wallet, ..)) = residents.get_mut(entity) else {
                continue;
            };
            if wallet.debit(amount) {
                economy.pennies = economy.pennies.saturating_add(amount);
            }
        }
        if tactical {
            let Some((person, shopper, ..)) = shopper else {
                continue;
            };
            economy.shopper = Some(*person);
            let entrance = SettlementBuildingKind::Hall.entrance_position(
                hall_position.0,
                hall_rotation.map_or(0.0, |rotation| rotation.0),
            );
            let counter = nearest_public_market_entrance(
                home_position.0,
                entrance,
                marketplaces
                    .iter()
                    .filter(|(building, owner, ..)| {
                        building.kind == SettlementBuildingKind::Market
                            && owner.0 == group.settlement
                    })
                    .map(|(building, _, position, rotation)| {
                        building.kind.entrance_position(position.0, rotation.0)
                    }),
            );
            commands.entity(*shopper).insert(HouseholdShoppingRoutine {
                account: account_entity,
                household: *id,
                home: home_entity,
                hall: hall_entity,
                counter,
                phase: HouseholdShoppingPhase::GoingToMarket,
                cargo: [0; Good::COUNT],
            });
            if counter == entrance {
                if let Some(queue_clock) = queue_clock.as_deref_mut() {
                    moot_services::enqueue_moot_service(
                        &mut commands,
                        queue_clock,
                        *shopper,
                        hall_entity,
                        MootServiceKind::HouseholdShopping,
                    );
                } else {
                    commands.entity(*shopper).insert(MoveTarget(counter));
                }
            } else {
                commands.entity(*shopper).insert(MoveTarget(counter));
            }
        } else {
            purchase_basket(
                &basket,
                day,
                group.settlement,
                &mut market,
                &mut hall_store,
                &mut pantry,
                &mut economy,
                &mut business_events,
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn repeated_shortage_reviews_do_not_multiply_demand() {
        let mut market = MootMarket::default();
        let mut claim = Claim::default();
        for _ in 0..100 {
            record_claim(&mut market, Good::Wood, 2, 0, 100, &mut claim);
        }
        assert_eq!(market.pool(Good::Wood).day.unavailable_units, 2);
        assert_eq!(market.pool(Good::Wood).day.funded_unmet_units, 2);
        record_claim(&mut market, Good::Wood, 3, 0, 150, &mut claim);
        assert_eq!(market.pool(Good::Wood).day.unavailable_units, 3);
    }
    #[test]
    fn shortage_classification_and_substitutes_replace_the_same_order() {
        let mut market = MootMarket::founding();
        let mut claim = Claim::default();
        record_claim(&mut market, Good::Wood, 3, 0, 0, &mut claim);
        record_claim(&mut market, Good::Wood, 3, 3, 0, &mut claim);
        assert_eq!(market.pool(Good::Wood).day.unavailable_units, 0);
        assert_eq!(market.pool(Good::Wood).day.unaffordable_units, 3);
        record_claim(&mut market, Good::Bread, 3, 0, 540, &mut claim);
        assert_eq!(market.pool(Good::Wood).day.unmet_units(), 0);
        assert_eq!(market.pool(Good::Bread).day.funded_unmet_units, 3);
        record_claim(&mut market, Good::Bread, 0, 0, 0, &mut claim);
        assert_eq!(market.pool(Good::Bread).day.unmet_units(), 0);
        assert_eq!(market.pool(Good::Bread).day.funded_unmet_units, 0);
    }

    #[test]
    fn old_epoch_claim_cannot_withdraw_another_days_orders() {
        let mut market = MootMarket::founding();
        let mut review = Review {
            market_epoch: Some(market.demand_epoch()),
            ..default()
        };
        record_claim(&mut market, Good::Wood, 3, 0, 150, &mut review.fuel_claim);
        market.begin_new_day();
        market.record_unmet_demand(Good::Wood, 1, 0, 1, 50);
        review.withdraw(&mut market);
        assert_eq!(market.pool(Good::Wood).day.unmet_units(), 1);
        assert_eq!(market.pool(Good::Wood).day.funded_unmet_units, 1);
    }
}
