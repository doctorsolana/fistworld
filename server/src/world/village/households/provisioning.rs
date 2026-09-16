//! Scheduled household procurement. Real cash funds a basket, and repeated
//! visits never multiply one household's daily shortage into fictitious demand.

use super::needs::{fair_contributions, HearthState, ProvisionNeeds, PERSONAL_RESERVE_RATION_DAYS};
use super::*;
use shared::components::{BuildingId, HouseholdId, HouseholdMembers, PersonId, SettlementId};

#[derive(Clone, Copy, Default)]
struct ClaimSlice {
    counts: [u32; 3],
    reference_price: u64,
}

#[derive(Default)]
struct Claim {
    good: Option<Good>,
    slices: [ClaimSlice; 2],
}

impl Claim {
    fn withdraw(&mut self, market: &mut MootMarket) {
        if let Some(good) = self.good.take() {
            for slice in self.slices {
                market.withdraw_unmet_demand(
                    good,
                    slice.counts[0],
                    slice.counts[1],
                    slice.counts[2],
                    slice.reference_price,
                );
            }
        }
        self.slices = [ClaimSlice::default(); 2];
    }
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
                claim.withdraw(market);
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
    pub today_food_remaining: u32,
    pub today_fuel_remaining: u32,
}

/// Quote once per review, then allocate the physical offers across priority
/// phases. Listing remainders prevent a cheap unit being quoted twice when
/// today's necessities and reserve restocking use the same good.
pub(super) fn plan_basket(
    market: &MootMarket,
    hall: &GoodsInventory,
    needs: ProvisionNeeds,
    budget: u64,
    restock_budget: u64,
    room: u32,
) -> Basket {
    let mut basket = Basket {
        amounts: [0; Good::COUNT],
        pennies: 0,
        food_remaining: needs.food,
        fuel_remaining: needs.fuel,
        today_food_remaining: needs.today_food,
        today_fuel_remaining: needs.today_fuel,
    };
    let mut physical = Good::ALL.map(|good| hall.amount(good));
    let mut offers: Vec<_> = market
        .listings()
        .iter()
        .filter(|listing| {
            market.can_trade(listing.good)
                && (Good::HOUSEHOLD_FOOD_PRIORITY.contains(&listing.good)
                    || listing.good == Good::Wood)
        })
        .filter_map(|listing| {
            let units = listing.units.min(physical[listing.good.index()]);
            physical[listing.good.index()] -= units;
            (units > 0).then_some((listing.good, units, listing.unit_price))
        })
        .collect();
    offers.sort_by_key(|(good, _, price)| {
        (
            *price,
            Good::HOUSEHOLD_FOOD_PRIORITY
                .iter()
                .position(|food| food == good)
                .unwrap_or(4),
        )
    });
    let mut room = room;
    for (food_phase, requested, limit) in [
        (true, needs.today_food, budget),
        (false, needs.today_fuel, budget),
        (
            true,
            needs.food.saturating_sub(needs.today_food),
            restock_budget.min(budget),
        ),
        (
            false,
            needs.fuel.saturating_sub(needs.today_fuel),
            restock_budget.min(budget),
        ),
    ] {
        // Savings may fund immediate warmth only once today's meal is covered.
        if !food_phase && basket.today_food_remaining > 0 {
            continue;
        }
        let mut remaining = requested;
        for (good, units, price) in &mut offers {
            if (*good != Good::Wood) != food_phase || *units == 0 {
                continue;
            }
            let affordable = limit.saturating_sub(basket.pennies) / (*price).max(1);
            let bought = remaining
                .min(*units)
                .min(room / good.bulk_per_unit())
                .min(affordable.min(u64::from(u32::MAX)) as u32);
            basket.amounts[good.index()] += bought;
            basket.pennies += u64::from(bought) * *price;
            *units -= bought;
            remaining -= bought;
            room -= bought * good.bulk_per_unit();
            if food_phase {
                basket.food_remaining -= bought;
                basket.today_food_remaining = basket.today_food_remaining.saturating_sub(bought);
            } else {
                basket.fuel_remaining -= bought;
                basket.today_fuel_remaining = basket.today_fuel_remaining.saturating_sub(bought);
            }
            if remaining == 0 {
                break;
            }
        }
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

#[derive(Clone, Copy, Debug, Default)]
struct ClaimBid {
    missing: u32,
    funded: u32,
    unit_price: u64,
}

fn record_claim(
    market: &mut MootMarket,
    good: Good,
    bids: [ClaimBid; 2],
    mut stock: u32,
    previous: &mut Claim,
) {
    // Replace this household's two original bids exactly. Today and restocking
    // may have different purchasing power; merging them would either hide a
    // poor household's meal bid or pledge its protected savings to stockpiling.
    previous.withdraw(market);
    if !market.can_trade(good) {
        return;
    }
    previous.good = Some(good);
    for (slice, bid) in previous.slices.iter_mut().zip(bids) {
        let unavailable = bid.missing.saturating_sub(stock);
        let unaffordable = bid.missing.min(stock);
        stock = stock.saturating_sub(bid.missing);
        market.record_unmet_demand(good, unavailable, unaffordable, bid.funded, bid.unit_price);
        slice.counts = [unavailable, unaffordable, bid.funded];
        slice.reference_price = bid.unit_price;
    }
}

/// Each need retains two bounded bids: today's necessities and reserve
/// restocking. The same pennies cannot fund both goods or both time horizons.
/// A market quote is a ceiling, not a minimum: ten pennies still back a ten-
/// penny meal request even when the available loaf costs twenty pennies.
fn claim_bids(
    basket: &Basket,
    food: u32,
    fuel: u32,
    budget: u64,
    ordinary: u64,
    food_price: u64,
    fuel_price: u64,
) -> ([ClaimBid; 2], [ClaimBid; 2]) {
    let mut remaining = budget.saturating_sub(basket.pennies);
    let mut ordinary = ordinary.saturating_sub(basket.pennies);
    let mut allocate = |missing: u32, quote: u64, savings: bool| {
        if missing == 0 {
            return ClaimBid::default();
        }
        let limit = if savings {
            remaining
        } else {
            ordinary.min(remaining)
        };
        let unit_price = (limit / u64::from(missing)).max(1).min(quote.max(1));
        let funded = u64::from(missing).min(limit / unit_price) as u32;
        let cash = u64::from(funded).saturating_mul(unit_price);
        remaining -= cash;
        ordinary = ordinary.saturating_sub(cash);
        ClaimBid {
            missing,
            funded,
            unit_price,
        }
    };
    let today_food = basket.today_food_remaining.min(food);
    let today_fuel = if basket.today_food_remaining == 0 {
        basket.today_fuel_remaining.min(fuel)
    } else {
        0
    };
    let food_now = allocate(today_food, food_price, true);
    let fuel_now = allocate(today_fuel, fuel_price, true);
    let food_reserve = allocate(food.saturating_sub(today_food), food_price, false);
    let fuel_reserve = if basket.today_food_remaining == 0 {
        allocate(fuel.saturating_sub(today_fuel), fuel_price, false)
    } else {
        ClaimBid::default()
    };
    ([food_now, food_reserve], [fuel_now, fuel_reserve])
}

#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn update_household_budgets_and_pantries(
    mut commands: Commands,
    world_time: Query<&WorldTime>,
    mut runtime: Local<ProvisioningClock>,
    mut queue_clock: ResMut<MootQueueClock>,
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
        &PlayerPosition,
        Option<&HouseholdShoppingRoutine>,
    )>,
    busy: Query<(), super::super::worker_activity::NeedsStartBlocked>,
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
        let Ok((_, _, hall_position, hall_rotation, hall_store, mut market)) =
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
            let meal_price = cheapest_ready_food(&hall_store, &market).map(|good| {
                market
                    .preview_purchase(good, 1, u64::MAX, None, None)
                    .pennies
            });
            if let Some(price) = meal_price {
                for person in &group.resident_ids {
                    let Some(entity) = resident_by_id.get(person).copied() else {
                        continue;
                    };
                    let Ok((_, _, mut wallet, ..)) = residents.get_mut(entity) else {
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
                let (_, _, wallet, work, home, _, _) = residents.get(entity).ok()?;
                if !home.is_some_and(|home| home.home == home_entity) {
                    return None;
                }
                Some((
                    *person,
                    entity,
                    wallet.balance(),
                    work.copied().unwrap_or_default(),
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
        let needs = ProvisionNeeds::for_home(n, &economy, &pantry, hearth);
        if existing_hearth.is_none() {
            commands.entity(home_entity).insert(initial_hearth);
        }
        if n == 0 {
            review.withdraw(&mut market);
            continue;
        }
        economy.last_budget_day = day;
        if pantry.edible_amount() < n as u32 {
            review.next_minute = minute + 12 + (id.0 % 7);
        }
        if active_trips.contains(id) {
            continue;
        }
        if needs.food == 0 && needs.fuel == 0 {
            review.withdraw(&mut market);
            continue;
        }
        let floor = household_ration_price(&hall_store, &market)
            .saturating_mul(PERSONAL_RESERVE_RATION_DAYS);
        let available: Vec<_> = members
            .iter()
            .map(|(person, _, cash, ..)| (*person, cash.saturating_sub(floor)))
            .collect();
        let spendable = available
            .iter()
            .map(|(_, amount)| *amount)
            .fold(economy.pennies, u64::saturating_add);
        let emergency_spendable = members
            .iter()
            .map(|(_, _, cash, ..)| *cash)
            .fold(economy.pennies, u64::saturating_add);
        let basket = plan_basket(
            &market,
            &hall_store,
            needs,
            emergency_spendable,
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
        let fuel_shortfall = basket
            .fuel_remaining
            .min(remaining_room / Good::Wood.bulk_per_unit());
        let (food_bids, fuel_bids) = claim_bids(
            &basket,
            food_shortfall,
            fuel_shortfall,
            emergency_spendable,
            spendable,
            market.suggested_price(missing_food_good),
            market.suggested_price(Good::Wood),
        );
        record_claim(
            &mut market,
            missing_food_good,
            food_bids,
            food_stock,
            &mut review.food_claim,
        );
        if basket.today_food_remaining == 0 {
            let remaining_stock = hall_store
                .amount(Good::Wood)
                .saturating_sub(basket.amounts[Good::Wood.index()]);
            record_claim(
                &mut market,
                Good::Wood,
                fuel_bids,
                remaining_stock,
                &mut review.fuel_claim,
            );
        } else {
            review.fuel_claim.withdraw(&mut market);
        }
        if basket.pennies == 0 {
            continue;
        }
        let shopper = members
            .iter()
            .filter(|(_, entity, _, _)| busy.get(*entity).is_err())
            .min_by_key(|(person, _, _, work)| {
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
        if !clock.is_day() || shopper.is_none() {
            continue;
        }
        let needed = basket.pennies.saturating_sub(economy.pennies);
        let mut contributions = fair_contributions(&available, needed, day);
        let contributed = contributions
            .iter()
            .map(|(_, pennies)| *pennies)
            .sum::<u64>();
        if contributed < needed {
            let unspent: Vec<_> = members
                .iter()
                .map(|(person, _, cash, ..)| {
                    let paid = contributions
                        .iter()
                        .find(|(id, _)| id == person)
                        .map_or(0, |(_, paid)| *paid);
                    (*person, cash.saturating_sub(paid))
                })
                .collect();
            contributions.extend(fair_contributions(&unspent, needed - contributed, day));
        }
        for (person, amount) in contributions {
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
                    building.kind == SettlementBuildingKind::Market && owner.0 == group.settlement
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
            moot_services::enqueue_moot_service(
                &mut commands,
                &mut queue_clock,
                *shopper,
                hall_entity,
                MootServiceKind::HouseholdShopping,
            );
        } else {
            commands.entity(*shopper).insert(MoveTarget(counter));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn single_bid(missing: u32, funded: u32, unit_price: u64) -> [ClaimBid; 2] {
        [
            ClaimBid {
                missing,
                funded,
                unit_price,
            },
            ClaimBid::default(),
        ]
    }

    fn empty_basket() -> Basket {
        Basket {
            amounts: [0; Good::COUNT],
            pennies: 0,
            food_remaining: 3,
            fuel_remaining: 2,
            today_food_remaining: 1,
            today_fuel_remaining: 1,
        }
    }

    fn pledged(bids: [ClaimBid; 2]) -> u64 {
        bids.into_iter()
            .map(|bid| u64::from(bid.funded) * bid.unit_price)
            .sum()
    }

    #[test]
    fn a_poor_household_bids_its_real_ten_pennies_for_todays_meal() {
        let mut market = MootMarket::founding();
        let (food, fuel) = claim_bids(&empty_basket(), 3, 2, 10, 0, 20, 5);
        record_claim(&mut market, Good::Bread, food, 3, &mut Claim::default());
        let demand = market.pool(Good::Bread).day;
        assert_eq!(demand.unaffordable_units, 3);
        assert_eq!(demand.funded_unmet_at(10), 1);
        assert_eq!(demand.funded_unmet_at(11), 0);
        assert_eq!(
            food[1].funded, 0,
            "protected savings cannot fund stockpiling"
        );
        assert_eq!(pledged(food), 10);
        assert_eq!(pledged(fuel), 0, "today's unfed resident retains priority");
    }

    #[test]
    fn savings_back_only_immediate_needs_and_cash_is_not_claimed_twice() {
        let mut basket = empty_basket();
        let (food, fuel) = claim_bids(&basket, 3, 2, 100, 0, 20, 5);
        assert_eq!(pledged(food), 20);
        assert_eq!(food[1].funded, 0);
        assert_eq!(pledged(fuel), 0);
        basket.today_food_remaining = 0;
        let (food, fuel) = claim_bids(&basket, 3, 2, 30, 20, 20, 5);
        assert_eq!(food[0].funded, 0);
        assert_eq!(fuel[0].funded, 1);
        assert_eq!(fuel[0].unit_price, 5);
        assert!(pledged(food) + pledged(fuel) <= 20);
    }

    #[test]
    fn claim_bids_respect_local_quotes_and_money_already_in_the_basket() {
        let mut basket = empty_basket();
        basket.pennies = 4;
        let (food, _) = claim_bids(&basket, 3, 0, 100, 4, 7, 5);
        assert_eq!(food[0].unit_price, 7);
        assert_eq!(food[0].funded, 1);
        assert_eq!(food[1].funded, 0);
        let (food, _) = claim_bids(&basket, 3, 0, 7, 4, 20, 5);
        assert_eq!(food[0].unit_price, 3);
        assert_eq!(pledged(food), 3);
    }

    #[test]
    fn absent_offers_never_move_reserve_cash_into_a_basket() {
        let basket = plan_basket(
            &MootMarket::founding(),
            &GoodsInventory::new(100),
            ProvisionNeeds {
                food: 3,
                fuel: 2,
                today_food: 1,
                today_fuel: 1,
            },
            1000,
            0,
            100,
        );
        assert_eq!(basket.pennies, 0);
        assert_eq!(basket.amounts, [0; Good::COUNT]);
    }

    #[test]
    fn repeated_shortage_reviews_do_not_multiply_demand() {
        let mut market = MootMarket::default();
        let mut claim = Claim::default();
        for _ in 0..100 {
            record_claim(&mut market, Good::Wood, single_bid(2, 2, 50), 0, &mut claim);
        }
        assert_eq!(market.pool(Good::Wood).day.unavailable_units, 2);
        assert_eq!(market.pool(Good::Wood).day.funded_unmet_units, 2);
        record_claim(&mut market, Good::Wood, single_bid(3, 3, 50), 0, &mut claim);
        assert_eq!(market.pool(Good::Wood).day.unavailable_units, 3);
    }

    #[test]
    fn shortage_classification_and_substitutes_replace_the_same_order() {
        let mut market = MootMarket::founding();
        let mut claim = Claim::default();
        record_claim(&mut market, Good::Wood, single_bid(3, 0, 50), 0, &mut claim);
        record_claim(&mut market, Good::Wood, single_bid(3, 0, 50), 3, &mut claim);
        assert_eq!(market.pool(Good::Wood).day.unavailable_units, 0);
        assert_eq!(market.pool(Good::Wood).day.unaffordable_units, 3);
        record_claim(
            &mut market,
            Good::Bread,
            single_bid(3, 3, 180),
            0,
            &mut claim,
        );
        assert_eq!(market.pool(Good::Wood).day.unmet_units(), 0);
        assert_eq!(market.pool(Good::Bread).day.funded_unmet_units, 3);
        claim.withdraw(&mut market);
        assert_eq!(market.pool(Good::Bread).day.unmet_units(), 0);
        assert_eq!(market.pool(Good::Bread).day.funded_unmet_units, 0);
    }

    #[test]
    fn withdrawing_two_prices_preserves_other_households_bids() {
        let mut market = MootMarket::founding();
        let mut claim = Claim::default();
        let bids = [
            ClaimBid {
                missing: 1,
                funded: 1,
                unit_price: 20,
            },
            ClaimBid {
                missing: 2,
                funded: 2,
                unit_price: 5,
            },
        ];
        record_claim(&mut market, Good::Bread, bids, 1, &mut claim);
        market.record_unmet_demand(Good::Bread, 1, 0, 1, 10);
        assert_eq!(market.pool(Good::Bread).day.unavailable_units, 3);
        assert_eq!(market.pool(Good::Bread).day.unaffordable_units, 1);
        claim.withdraw(&mut market);
        let day = market.pool(Good::Bread).day;
        assert_eq!(day.unavailable_units, 1);
        assert_eq!(day.unaffordable_units, 0);
        assert_eq!(day.funded_unmet_at(10), 1);
        assert_eq!(day.funded_unmet_at(11), 0);
        assert_eq!(day.funded_unmet_at(5), 1);
    }

    #[test]
    fn old_epoch_claim_cannot_withdraw_another_days_orders() {
        let mut market = MootMarket::founding();
        let mut review = Review {
            market_epoch: Some(market.demand_epoch()),
            ..default()
        };
        record_claim(
            &mut market,
            Good::Wood,
            single_bid(3, 3, 50),
            0,
            &mut review.fuel_claim,
        );
        market.begin_new_day();
        market.record_unmet_demand(Good::Wood, 1, 0, 1, 50);
        review.withdraw(&mut market);
        assert_eq!(market.pool(Good::Wood).day.unmet_units(), 1);
        assert_eq!(market.pool(Good::Wood).day.funded_unmet_units, 1);
    }
}
