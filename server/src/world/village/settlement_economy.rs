//! Settlement-level markets, food security, prosperity and tier advancement.
//!
//! This module owns aggregate economic state. Embodied trade work remains in
//! `commerce` and physical resource creation remains in `trades`.

use super::*;

const FOOD_HISTORY_DAYS: usize = 3;

#[derive(Debug)]
struct SettlementEconomyDay {
    last_world_day: u32,
    produced_today: u32,
    production_history: [u32; FOOD_HISTORY_DAYS],
    consumption_history: [u32; FOOD_HISTORY_DAYS],
    recorded_days: usize,
}

impl SettlementEconomyDay {
    fn new(day: u32) -> Self {
        Self {
            last_world_day: day,
            produced_today: 0,
            production_history: [0; FOOD_HISTORY_DAYS],
            consumption_history: [0; FOOD_HISTORY_DAYS],
            recorded_days: 0,
        }
    }

    fn finish_day(&mut self, consumed: u32) {
        self.production_history.rotate_right(1);
        self.consumption_history.rotate_right(1);
        self.production_history[0] = std::mem::take(&mut self.produced_today);
        self.consumption_history[0] = consumed;
        self.recorded_days = (self.recorded_days + 1).min(FOOD_HISTORY_DAYS);
    }

    fn recent_average(values: &[u32; FOOD_HISTORY_DAYS], days: usize) -> f32 {
        if days == 0 {
            return 0.0;
        }
        values[..days].iter().sum::<u32>() as f32 / days as f32
    }

    /// The just-finished day's production plus enough history to retain the
    /// same three-day window used by the public economy reading.
    fn recent_production_including_today(&self) -> f32 {
        let previous_days = self.recorded_days.min(FOOD_HISTORY_DAYS.saturating_sub(1));
        let total = self.produced_today.saturating_add(
            self.production_history[..previous_days]
                .iter()
                .copied()
                .sum(),
        );
        total as f32 / (previous_days + 1) as f32
    }
}

/// Server-only daily buckets behind the small replicated economy summary.
#[derive(Resource, Default)]
pub struct SettlementEconomyRuntime {
    by_settlement: HashMap<Entity, SettlementEconomyDay>,
}

pub(super) fn sell_carried_to_moot(
    _commands: &mut Commands,
    _settlement: Entity,
    worker: shared::components::PersonId,
    _worker_wallet: Option<&mut Wallet>,
    owner: Option<shared::components::PersonId>,
    good: Good,
    carrier: &mut GoodsInventory,
    hall: &mut GoodsInventory,
    market: &mut MootMarket,
) -> u32 {
    let room = hall.free_bulk() / good.bulk_per_unit();
    let offered = carrier.amount(good).min(room);
    let moved = carrier.transfer_to(hall, good, offered);
    if moved > 0 {
        market.consign(
            shared::economy::MarketSeller::Person(owner.unwrap_or(worker)),
            good,
            moved,
            market.suggested_price(good),
        );
    }
    moved
}

pub(super) fn buy_from_moot(
    day: u32,
    settlement: shared::components::SettlementId,
    good: Good,
    requested: u32,
    buyer: &mut Wallet,
    hall: &mut GoodsInventory,
    carrier: &mut GoodsInventory,
    market: &mut MootMarket,
    business_events: &mut BusinessEventQueue,
) -> u32 {
    let room = carrier.free_bulk() / good.bulk_per_unit();
    let available = requested.min(room).min(hall.amount(good));
    let purchase = market.purchase(good, available, buyer.balance(), None, None);
    if purchase.trade.units == 0 || !buyer.debit(purchase.trade.pennies) {
        return 0;
    }
    let moved = hall.transfer_to(carrier, good, purchase.trade.units);
    debug_assert_eq!(moved, purchase.trade.units);
    business_events.record_market_purchase(day, settlement, purchase.fills);
    moved
}

/// Backfill monetary state on old/test entities and initialise new foundations.
pub fn ensure_village_finances(
    mut commands: Commands,
    mut settlements: Query<(Entity, &mut Settlement, Option<&MootMarket>)>,
    villagers: Query<(Entity, &CharacterKind, Option<&Wallet>)>,
) {
    for (entity, mut settlement, market) in settlements.iter_mut() {
        if market.is_none() {
            if settlement.treasury == 0 {
                settlement.treasury = STARTING_TREASURY_MONEY;
            }
            commands.entity(entity).insert(MootMarket::founding());
        }
    }
    for (entity, kind, wallet) in villagers.iter() {
        if *kind == CharacterKind::Villager && wallet.is_none() {
            commands.entity(entity).insert(Wallet::founding_villager());
        }
    }
}

/// Reprice each Moot against real inventory and measurable local demand.
pub fn update_moot_market_targets(
    mut halls: Query<(
        &shared::components::SettlementId,
        &Settlement,
        &GoodsInventory,
        &mut MootMarket,
    )>,
    sites: Query<(&UnderConstruction, &GoodsInventory)>,
) {
    for (settlement_id, settlement, inventory, mut market) in halls.iter_mut() {
        let outstanding_wood = sites
            .iter()
            .filter(|(site, _)| site.settlement_id == *settlement_id)
            .map(|(site, materials)| {
                site.kind
                    .construction_wood_required()
                    .saturating_sub(materials.amount(Good::Wood))
            })
            .fold(0u32, u32::saturating_add);
        market.set_targets(settlement.residents, outstanding_wood);
        market.reconcile_inventory(*settlement_id, inventory);
        market.refresh_all(inventory);
    }
}

impl SettlementEconomyRuntime {
    pub(super) fn historical_food(&self, settlement: Entity, history_index: usize) -> (u32, u32) {
        self.by_settlement.get(&settlement).map_or((0, 0), |state| {
            (
                state
                    .production_history
                    .get(history_index)
                    .copied()
                    .unwrap_or(0),
                state
                    .consumption_history
                    .get(history_index)
                    .copied()
                    .unwrap_or(0),
            )
        })
    }

    pub(super) fn record_food_production(&mut self, settlement: Entity, amount: u32) {
        if amount == 0 {
            return;
        }
        if let Some(day) = self.by_settlement.get_mut(&settlement) {
            day.produced_today = day.produced_today.saturating_add(amount);
        }
    }
}

/// Every hall owns one replicated economic reading; physical stock remains in
/// the inventories of the hall and its completed buildings.
pub fn ensure_settlement_economies(
    mut commands: Commands,
    settlements: Query<Entity, (With<Settlement>, Without<SettlementEconomy>)>,
) {
    for entity in settlements.iter() {
        commands.entity(entity).insert(SettlementEconomy::default());
    }
}

/// Consume one edible portion per resident at each world-day boundary, keep a
/// three-day production/consumption reading, derive prosperity from visible
/// facts, and promote food-secure Hamlets.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn update_settlement_economies(
    mut commands: Commands,
    world_time: Query<&WorldTime>,
    mut runtime: ResMut<SettlementEconomyRuntime>,
    mut business_events: ResMut<BusinessEventQueue>,
    mut queue_clock: Option<ResMut<MootQueueClock>>,
    regions: Option<Res<RegionRegistry>>,
    mut settlements: Query<(
        Entity,
        &shared::components::SettlementId,
        &mut Settlement,
        &mut SettlementEconomy,
        Option<&mut MootMarket>,
        Option<&SettlementPolicies>,
        Option<&mut shared::economy::CivicAccount>,
    )>,
    buildings: Query<(Entity, &SettlementBuilding, &shared::components::BuildingOf)>,
    employment: Query<
        (
            &shared::components::ResidentOf,
            Option<&shared::components::EmployedAt>,
            Option<&shared::components::CivicEmployment>,
        ),
        With<CharacterKind>,
    >,
    mut residents: Query<
        (
            Entity,
            &VillagerIntent,
            Option<&mut Wallet>,
            Option<&mut Nutrition>,
            Option<&HomeAssignment>,
        ),
        With<CharacterKind>,
    >,
    service_state: Query<
        (
            Option<&RegionCoord>,
            Option<&MootMealRoutine>,
            Option<&MootQueueTicket>,
        ),
        With<CharacterKind>,
    >,
    mut inventories: ParamSet<(
        Query<&mut GoodsInventory, (With<Settlement>, Without<SettlementBuilding>)>,
        Query<&mut GoodsInventory, With<SettlementBuilding>>,
    )>,
) {
    let Some(day) = world_time.iter().next().map(|time| time.day) else {
        return;
    };

    let mut stores: HashMap<shared::components::SettlementId, Vec<Entity>> = HashMap::new();
    let mut pantries: HashMap<shared::components::SettlementId, Vec<Entity>> = HashMap::new();
    let mut housing: HashMap<shared::components::SettlementId, u32> = HashMap::new();
    let mut employed: HashMap<shared::components::SettlementId, u32> = HashMap::new();
    for (entity, building, building_of) in buildings.iter() {
        stores.entry(building_of.0).or_default().push(entity);
        if building.kind == SettlementBuildingKind::House {
            pantries.entry(building_of.0).or_default().push(entity);
        }
        *housing.entry(building_of.0).or_default() += u32::from(building.kind.housing_capacity());
    }
    for (resident_of, employed_at, civic_job) in employment.iter() {
        if employed_at.is_some() || civic_job.is_some() {
            *employed.entry(resident_of.0).or_default() += 1;
        }
    }
    for store_entities in stores.values_mut() {
        store_entities.sort_unstable_by_key(|entity| entity.to_bits());
    }
    for pantry_entities in pantries.values_mut() {
        pantry_entities.sort_unstable_by_key(|entity| entity.to_bits());
    }

    for (
        entity,
        settlement_id,
        mut settlement,
        mut economy,
        mut market,
        policies,
        mut civic_account,
    ) in settlements.iter_mut()
    {
        let day_state = runtime
            .by_settlement
            .entry(entity)
            .or_insert_with(|| SettlementEconomyDay::new(day));
        let elapsed_days = day.saturating_sub(day_state.last_world_day);
        let mut advanced_day = false;

        for _ in 0..elapsed_days {
            let demand = settlement.residents;
            let meal_day = day_state.last_world_day.saturating_add(1);
            let consumed = if let Some(market) = market.as_deref_mut() {
                // A ration is now a real purchase. Only goods the Moot bought
                // into its physical hall inventory are market stock; a farmer's
                // private shed is not silently raided at midnight.
                let mut consumed = 0u32;
                let mut unaffordable = Vec::new();
                let mut individual_buyers = Vec::new();
                let mut relieved_residents = HashSet::new();
                // Housed residents eat from their shared physical pantry.
                // Unhoused residents still buy one ration personally at the
                // Moot because they have no household budget or storage.
                for (resident, intent, _, nutrition, home) in residents.iter_mut() {
                    if intent.settlement() != Some(entity) {
                        continue;
                    }
                    let fed = home.is_some_and(|home| {
                        inventories
                            .p1()
                            .get_mut(home.home)
                            .is_ok_and(|mut pantry| pantry.remove_edible(1) == 1)
                    });
                    if fed {
                        consumed = consumed.saturating_add(1);
                        if let Some(mut nutrition) = nutrition {
                            nutrition.record_meal(meal_day);
                        }
                    } else if home.is_some() {
                        unaffordable.push(resident);
                    } else {
                        individual_buyers.push(resident);
                    }
                }
                if let Ok(mut hall) = inventories.p0().get_mut(entity) {
                    for resident in individual_buyers.iter().copied() {
                        let Ok((_, _, wallet, nutrition, _)) = residents.get_mut(resident) else {
                            continue;
                        };
                        let Some(mut wallet) = wallet else {
                            unaffordable.push(resident);
                            continue;
                        };
                        let mut fed = false;
                        let mut collecting = false;
                        for good in Good::READY_TO_EAT_PRIORITY {
                            if hall.amount(good) == 0 {
                                continue;
                            }
                            let purchase = market.purchase_recording_demand(
                                good,
                                1,
                                wallet.balance(),
                                None,
                                None,
                            );
                            if purchase.trade.units == 1 && wallet.debit(purchase.trade.pennies) {
                                debug_assert_eq!(hall.remove(good, 1), 1);
                                business_events.record_market_purchase(
                                    meal_day,
                                    *settlement_id,
                                    purchase.fills,
                                );
                                consumed = consumed.saturating_add(1);
                                fed = true;
                                let tactical = queue_clock.is_some()
                                    && service_state.get(resident).is_ok_and(
                                        |(region, meal, ticket)| {
                                            meal.is_none()
                                                && ticket.is_none()
                                                && region.is_some_and(|region| {
                                                    regions.as_ref().is_some_and(|registry| {
                                                        registry.get(*region).is_some_and(|state| {
                                                            state.sim_level == SimLevel::Tactical
                                                        })
                                                    })
                                                })
                                        },
                                    );
                                if tactical {
                                    if let Some(queue_clock) = queue_clock.as_deref_mut() {
                                        moot_services::reserve_meal(
                                            &mut commands,
                                            queue_clock,
                                            resident,
                                            entity,
                                            MootServiceKind::PersonalMeal,
                                            good,
                                            meal_day,
                                        );
                                        collecting = true;
                                    }
                                }
                                break;
                            }
                        }
                        if let Some(mut nutrition) = nutrition {
                            if fed && !collecting {
                                nutrition.record_meal(meal_day);
                            }
                        }
                        if !fed {
                            unaffordable.push(resident);
                        }
                    }

                    // Solvent residents buy first. Poor Relief then sees the
                    // real remaining surplus and cannot consume the emergency
                    // floor out from under ordinary daily demand.
                    if let Some(policy) =
                        policies.filter(|policy| policy.poor_relief.allows_purchase())
                    {
                        let reserve_floor = demand
                            .saturating_mul(u32::from(policy.food_reserve_target_days.max(1)));
                        let production_is_sustainable =
                            day_state.recent_production_including_today() >= demand as f32;

                        unaffordable.sort_unstable_by_key(|resident| resident.to_bits());
                        for resident in unaffordable.iter().copied() {
                            // Public relief is a real market purchase, but only
                            // sustainable surplus is eligible: production must
                            // cover the roster and the purchase must leave the
                            // configured number of full resident-days intact.
                            if !production_is_sustainable
                                || hall.ready_to_eat_amount() == 0
                                || hall.edible_amount().saturating_sub(1) < reserve_floor
                            {
                                break;
                            }
                            let mut relieved = false;
                            for good in Good::READY_TO_EAT_PRIORITY {
                                let purchase =
                                    market.purchase(good, 1, settlement.treasury, None, None);
                                if purchase.trade.units == 1
                                    && settlement.treasury >= purchase.trade.pennies
                                {
                                    settlement.treasury -= purchase.trade.pennies;
                                    if let Some(account) = civic_account.as_deref_mut() {
                                        account.record_poor_relief_expense(
                                            meal_day,
                                            purchase.trade.pennies,
                                        );
                                    }
                                    debug_assert_eq!(hall.remove(good, 1), 1);
                                    business_events.record_market_purchase(
                                        meal_day,
                                        *settlement_id,
                                        purchase.fills,
                                    );
                                    consumed = consumed.saturating_add(1);
                                    relieved = true;
                                    let tactical = queue_clock.is_some()
                                        && service_state.get(resident).is_ok_and(
                                            |(region, meal, ticket)| {
                                                meal.is_none()
                                                    && ticket.is_none()
                                                    && region.is_some_and(|region| {
                                                        regions.as_ref().is_some_and(|registry| {
                                                            registry.get(*region).is_some_and(
                                                                |state| {
                                                                    state.sim_level
                                                                        == SimLevel::Tactical
                                                                },
                                                            )
                                                        })
                                                    })
                                            },
                                        );
                                    if tactical {
                                        if let Some(queue_clock) = queue_clock.as_deref_mut() {
                                            moot_services::reserve_meal(
                                                &mut commands,
                                                queue_clock,
                                                resident,
                                                entity,
                                                MootServiceKind::PoorRelief,
                                                good,
                                                meal_day,
                                            );
                                        }
                                    } else if let Ok((_, _, _, Some(mut nutrition), _)) =
                                        residents.get_mut(resident)
                                    {
                                        nutrition.record_meal(meal_day);
                                    }
                                    break;
                                }
                            }
                            if relieved {
                                relieved_residents.insert(resident);
                            }
                        }
                    }
                } else {
                    unaffordable.extend(individual_buyers.iter().copied());
                }
                for resident in unaffordable {
                    if relieved_residents.contains(&resident) {
                        continue;
                    }
                    if let Ok((_, _, _, Some(mut nutrition), _)) = residents.get_mut(resident) {
                        nutrition.record_missed_meal();
                    }
                }
                consumed.min(demand)
            } else {
                // Migration/test compatibility for a malformed pre-market
                // world. Production wiring always installs a market first.
                let mut remaining = demand;
                if remaining > 0 {
                    if let Ok(mut hall) = inventories.p0().get_mut(entity) {
                        remaining = remaining.saturating_sub(hall.remove_edible(remaining));
                    }
                    if remaining > 0 {
                        for store in stores.get(settlement_id).into_iter().flatten() {
                            if let Ok(mut inventory) = inventories.p1().get_mut(*store) {
                                remaining =
                                    remaining.saturating_sub(inventory.remove_edible(remaining));
                            }
                            if remaining == 0 {
                                break;
                            }
                        }
                    }
                }
                let consumed = demand.saturating_sub(remaining);
                // Old/no-market worlds still produce an honest per-person
                // result. Entity order keeps the fallback deterministic; live
                // worlds use the wallet-aware branch above.
                let mut local_residents: Vec<Entity> = residents
                    .iter_mut()
                    .filter_map(|(resident, intent, _, _, _)| {
                        (intent.settlement() == Some(entity)).then_some(resident)
                    })
                    .collect();
                local_residents.sort_unstable_by_key(|resident| resident.to_bits());
                for (index, resident) in local_residents.into_iter().enumerate() {
                    if let Ok((_, _, _, Some(mut nutrition), _)) = residents.get_mut(resident) {
                        if index < consumed as usize {
                            nutrition.record_meal(meal_day);
                        } else {
                            nutrition.record_missed_meal();
                        }
                    }
                }
                consumed
            };
            economy.unmet_food = demand.saturating_sub(consumed);
            day_state.finish_day(consumed);
            day_state.last_world_day = day_state.last_world_day.saturating_add(1);
            advanced_day = true;
        }

        let mut stock = inventories
            .p0()
            .get_mut(entity)
            .map_or(0, |inventory| inventory.edible_amount());
        for pantry in pantries.get(settlement_id).into_iter().flatten() {
            stock = stock.saturating_add(
                inventories
                    .p1()
                    .get_mut(*pantry)
                    .map_or(0, |inventory| inventory.edible_amount()),
            );
        }
        // Before finance initialisation, retain the old aggregate reading for
        // focused unit tests. In a live market, only Moot-owned stock is a
        // reserve residents can actually purchase.
        if market.is_none() {
            for store in stores.get(settlement_id).into_iter().flatten() {
                if pantries
                    .get(settlement_id)
                    .is_some_and(|houses| houses.contains(store))
                {
                    continue;
                }
                stock = stock.saturating_add(
                    inventories
                        .p1()
                        .get_mut(*store)
                        .map_or(0, |inventory| inventory.edible_amount()),
                );
            }
        }

        let residents = settlement.residents;
        economy.edible_stock = stock;
        economy.reserve_days = if residents == 0 {
            0.0
        } else {
            stock as f32 / residents as f32
        };
        economy.recent_food_production = SettlementEconomyDay::recent_average(
            &day_state.production_history,
            day_state.recorded_days,
        );
        economy.recent_food_consumption = SettlementEconomyDay::recent_average(
            &day_state.consumption_history,
            day_state.recorded_days,
        );
        economy.observed_days = economy
            .observed_days
            .saturating_add(elapsed_days.min(u32::from(u16::MAX)) as u16);

        let demand = residents.max(1) as f32;
        economy.reserve_prosperity =
            (economy.reserve_days / FOOD_SECURITY_TARGET_DAYS).clamp(0.0, 1.0) * 40.0;
        economy.production_prosperity =
            (economy.recent_food_production / demand).clamp(0.0, 1.0) * 30.0;
        economy.housing_prosperity =
            (housing.get(settlement_id).copied().unwrap_or(0) as f32 / demand).clamp(0.0, 1.0)
                * 20.0;
        economy.employment_prosperity =
            (employed.get(settlement_id).copied().unwrap_or(0) as f32 / demand).clamp(0.0, 1.0)
                * 10.0;
        economy.hunger_penalty = -(economy.unmet_food as f32 / demand).clamp(0.0, 1.0) * 30.0;
        economy.prosperity = (economy.reserve_prosperity
            + economy.production_prosperity
            + economy.housing_prosperity
            + economy.employment_prosperity
            + economy.hunger_penalty)
            .clamp(0.0, 100.0);

        if advanced_day {
            let secure = residents > 0
                && economy.reserve_days >= FOOD_SECURITY_TARGET_DAYS
                && economy.recent_food_production >= residents as f32
                && economy.unmet_food == 0;
            economy.food_secure_days = if secure {
                economy.food_secure_days.saturating_add(1)
            } else {
                0
            };

            if settlement.tier == shared::components::SettlementTier::Hamlet
                && residents >= VILLAGE_MIN_RESIDENTS
                && economy.food_secure_days >= VILLAGE_REQUIRED_SECURE_DAYS
                && economy.prosperity >= VILLAGE_MIN_PROSPERITY
            {
                settlement.tier = shared::components::SettlementTier::Village;
                info!(
                    "Village '{}': advanced from Hamlet to Village with {:.0} prosperity and {:.1} reserve days",
                    settlement.name, economy.prosperity, economy.reserve_days
                );
            }
        }
    }
}
