//! Businesses, civic market work, payroll and employment-state reconciliation.
//!
//! Physical production belongs to `trades`; this module owns the commercial
//! decisions and transfers wrapped around that production.

use super::*;

pub(super) fn business_output(kind: SettlementBuildingKind) -> Option<Good> {
    match kind {
        SettlementBuildingKind::Farmstead => Some(Good::Wheat),
        SettlementBuildingKind::FishermansHut => Some(Good::Food),
        SettlementBuildingKind::LumberjackHut => Some(Good::Wood),
        _ => None,
    }
}

/// Add the small ledgers used by every productive workplace. Founding working
/// capital is transferred from the owner when possible, never minted.
pub fn ensure_business_economies(
    mut commands: Commands,
    buildings: Query<(
        Entity,
        &SettlementBuilding,
        Option<&shared::components::OwnedBy>,
        Option<&BusinessAccount>,
        Option<&BusinessSalePolicy>,
        Option<&BusinessWagePolicy>,
    )>,
    mut owners: Query<(&shared::components::PersonId, &mut Wallet)>,
) {
    for (entity, building, owner_id, account, policy, wage_policy) in buildings.iter() {
        if business_output(building.kind).is_none() {
            continue;
        }
        let mut entity_commands = commands.entity(entity);
        if account.is_none() {
            let mut opening_cash = 0;
            if let Some((_, mut wallet)) = owners
                .iter_mut()
                .find(|(person_id, _)| owner_id.is_some_and(|owner| **person_id == owner.0))
            {
                let wanted = 2 * PENNIES_PER_COIN;
                if wallet.debit(wanted) {
                    opening_cash = wanted;
                }
            }
            entity_commands.insert(BusinessAccount {
                cash: opening_cash,
                ..default()
            });
        }
        if policy.is_none() {
            entity_commands.insert(BusinessSalePolicy::default());
        }
        if wage_policy.is_none() {
            entity_commands.insert(BusinessWagePolicy::default());
        }
    }
}

/// The Moot has three founding positions: Reeve, market porter and road
/// steward. The road system owns the steward; this pass fills the other two
/// without stealing business workers or residents who chose to chill.
pub fn staff_moot_hall_roles(
    mut commands: Commands,
    world_time: Query<&WorldTime>,
    mut last_payday: Local<HashMap<Entity, u32>>,
    mut halls: Query<(
        Entity,
        &mut Settlement,
        &mut MootAdministration,
        &shared::components::SettlementId,
    )>,
    mut villagers: Query<(
        Entity,
        &shared::components::PersonId,
        &CharacterName,
        &VillagerIntent,
        &mut Occupation,
        Option<&WorkStatus>,
        Option<&MarketPorter>,
        Option<&shared::components::EmployedAt>,
        Option<&shared::components::CivicEmployment>,
    )>,
    mut wallets: Query<&mut Wallet>,
) {
    let day = world_time.iter().next().map_or(0, |clock| clock.day);
    for (hall, mut settlement, mut administration, settlement_id) in halls.iter_mut() {
        let holder = |role: shared::components::CivicRole| {
            villagers
                .iter()
                .filter(|(_, _, _, intent, _, _, _, _, civic_job)| {
                    intent.settlement() == Some(hall)
                        && intent.counts_as_resident()
                        && civic_job
                            .is_some_and(|job| job.settlement == *settlement_id && job.role == role)
                })
                .min_by_key(|(_, person_id, ..)| **person_id)
                .map(|(entity, _, name, ..)| (entity, name.0.clone()))
        };
        let mut porter = holder(shared::components::CivicRole::MarketPorter);
        let mut reeve = holder(shared::components::CivicRole::Reeve);
        administration.market_porter = porter.as_ref().map(|(_, name)| name.clone());
        administration.reeve = reeve.as_ref().map(|(_, name)| name.clone());

        // Logistics comes before clerical comfort in a tiny foundation. Keep
        // at least one resident outside the Moot staff so three founders can
        // still apply for permits and operate the first workplace.
        for (title, role) in [
            ("Market Porter", shared::components::CivicRole::MarketPorter),
            ("Reeve", shared::components::CivicRole::Reeve),
        ] {
            let road_filled = villagers
                .iter()
                .any(|(_, _, _, intent, _, _, _, _, civic_job)| {
                    intent.settlement() == Some(hall)
                        && civic_job.is_some_and(|job| {
                            job.settlement == *settlement_id
                                && job.role == shared::components::CivicRole::RoadSteward
                        })
                });
            let founding_staff = usize::from(road_filled)
                + usize::from(porter.is_some())
                + usize::from(reeve.is_some());
            if founding_staff >= settlement.residents.saturating_sub(1) as usize {
                continue;
            }
            let filled = match role {
                shared::components::CivicRole::MarketPorter => porter.is_some(),
                shared::components::CivicRole::Reeve => reeve.is_some(),
                _ => unreachable!(),
            };
            if filled {
                continue;
            }
            let candidate = villagers
                .iter()
                .filter(|(_, _, _, intent, occupation, status, _, employed_at, civic_job)| {
                    matches!(intent, VillagerIntent::Resident { settlement } if *settlement == hall)
                        && occupation.0.is_none()
                        && employed_at.is_none()
                        && civic_job.is_none()
                        && status.is_none_or(|status| *status == WorkStatus::LookingForWork)
                })
                .min_by_key(|(_, person_id, ..)| **person_id)
                .map(|(entity, ..)| entity);
            let Some(candidate) = candidate else { continue };
            let Ok((_, _, name, _, mut occupation, _, marker, _, _)) = villagers.get_mut(candidate)
            else {
                continue;
            };
            occupation.0 = Some(title.to_string());
            commands.entity(candidate).insert((
                WorkStatus::Employed,
                shared::components::CivicEmployment {
                    settlement: *settlement_id,
                    role,
                },
            ));
            if role == shared::components::CivicRole::MarketPorter {
                administration.market_porter = Some(name.0.clone());
                if marker.is_none_or(|porter| porter.settlement != hall) {
                    commands
                        .entity(candidate)
                        .insert(MarketPorter { settlement: hall });
                }
                porter = Some((candidate, name.0.clone()));
            } else {
                administration.reeve = Some(name.0.clone());
                reeve = Some((candidate, name.0.clone()));
            }
            info!(
                "Village '{}': {} took the {} position",
                settlement.name, name.0, title
            );
        }

        let previous = last_payday.entry(hall).or_insert(day);
        let elapsed = day.saturating_sub(*previous);
        if elapsed > 0 {
            *previous = day;
            let due = FOUNDING_DAILY_WAGE.saturating_mul(u64::from(elapsed));
            for employee in [reeve.as_ref(), porter.as_ref()].into_iter().flatten() {
                let payment = settlement.treasury.min(due);
                if payment == 0 {
                    break;
                }
                if let Ok(mut wallet) = wallets.get_mut(employee.0) {
                    settlement.treasury -= payment;
                    wallet.credit(payment);
                }
            }
        }
    }
}

/// Keep the compact work-state component aligned with real rosters. `Chilling`
/// is an intentional choice and is never silently converted back into job
/// seeking merely because the occupation title is empty.
pub fn reconcile_work_statuses(
    mut villagers: Query<
        (
            &Occupation,
            Option<&shared::components::EmployedAt>,
            Option<&shared::components::CivicEmployment>,
            &mut WorkStatus,
        ),
        With<CharacterKind>,
    >,
) {
    for (occupation, employed_at, civic_job, mut status) in villagers.iter_mut() {
        let next = if occupation.0.is_some() || employed_at.is_some() || civic_job.is_some() {
            WorkStatus::Employed
        } else if *status == WorkStatus::Chilling {
            WorkStatus::Chilling
        } else {
            WorkStatus::LookingForWork
        };
        if *status != next {
            *status = next;
        }
    }
}

/// Collect saleable stock with the Moot porter. The market reserves its exact
/// cash at dispatch, goods remain at the business until the porter reaches it,
/// and the business account is credited only after physical delivery.
#[allow(clippy::too_many_arguments, clippy::type_complexity)]
pub fn run_market_collections(
    mut commands: Commands,
    mut halls: Query<
        (
            &shared::components::SettlementId,
            &PlayerPosition,
            Option<&PlayerRotation>,
            &mut GoodsInventory,
            &mut MootMarket,
        ),
        (Without<SettlementBuilding>, Without<CharacterKind>),
    >,
    mut businesses: Query<
        (
            Entity,
            &SettlementBuilding,
            &shared::components::BuildingOf,
            &PlayerPosition,
            &PlayerRotation,
            &mut GoodsInventory,
            &BusinessSalePolicy,
            &mut BusinessAccount,
        ),
        Without<CharacterKind>,
    >,
    mut porters: Query<
        (
            Entity,
            &MarketPorter,
            &PlayerPosition,
            &mut CharacterActivity,
            &mut GoodsInventory,
            Option<&MoveTarget>,
            Option<&mut MarketCollectionRoutine>,
            Option<&HomeRoutine>,
            Option<&RoadBuilderRoutine>,
            Option<&HouseholdShoppingRoutine>,
            Option<&NavigationRouteFailed>,
        ),
        (With<CharacterKind>, Without<strategic::StrategicPerson>),
    >,
) {
    for (
        porter_entity,
        porter,
        position,
        mut activity,
        mut carrier,
        move_target,
        routine,
        home,
        road_work,
        shopping,
        route_failed,
    ) in porters.iter_mut()
    {
        if home.is_some() || road_work.is_some() || shopping.is_some() {
            continue;
        }
        let Ok((settlement_id, hall_position, hall_rotation, mut hall_store, mut market)) =
            halls.get_mut(porter.settlement)
        else {
            continue;
        };
        let hall_entrance = SettlementBuildingKind::Hall.entrance_position(
            hall_position.0,
            hall_rotation.map_or(0.0, |rotation| rotation.0),
        );

        if let Some(failed) = route_failed {
            match routine.as_deref() {
                Some(active) if active.phase == MarketCollectionPhase::GoingToBusiness => {
                    // The market reserved its cash at dispatch. If the porter
                    // cannot reach that seller, unwind the reservation instead
                    // of leaving the entire early economy permanently short of
                    // both its porter and those pennies.
                    warn!(
                        "Market porter could not reach business at {:.1},{:.1}; cancelling that collection so another offer can be tried",
                        failed.goal.x, failed.goal.z
                    );
                    market.cancel_producer_purchase(
                        active.good,
                        hall_store.amount(active.good),
                        shared::economy::MarketTrade {
                            units: active.reserved_units,
                            pennies: active.reserved_pennies,
                        },
                    );
                    commands
                        .entity(porter_entity)
                        .remove::<MarketCollectionRoutine>()
                        .remove::<MoveTarget>()
                        .remove::<TravelRoute>()
                        .remove::<NavigationRoutePending>()
                        .remove::<NavigationRouteFailed>();
                }
                Some(active) => {
                    // Once stock is physically aboard it must reach the hall.
                    // Clear the static failure and explicitly dirty the target
                    // so the bounded planner gets a fresh request next tick.
                    warn!(
                        "Market porter retrying a loaded return to the Moot Hall after route failure at {:.1},{:.1}",
                        failed.goal.x, failed.goal.z
                    );
                    debug_assert_eq!(active.phase, MarketCollectionPhase::ReturningToHall);
                    commands
                        .entity(porter_entity)
                        .remove::<TravelRoute>()
                        .remove::<NavigationRoutePending>()
                        .remove::<NavigationRouteFailed>()
                        .insert(MoveTarget(hall_entrance));
                }
                None => {
                    // A stale failure without a live transaction must never
                    // prevent this unique civic worker from accepting work.
                    let mut porter_commands = commands.entity(porter_entity);
                    porter_commands
                        .remove::<TravelRoute>()
                        .remove::<NavigationRoutePending>()
                        .remove::<NavigationRouteFailed>();
                    if carrier.used_bulk() > 0 {
                        porter_commands.insert(MoveTarget(hall_entrance));
                    } else {
                        porter_commands.remove::<MoveTarget>();
                    }
                }
            }
            continue;
        }

        let Some(mut routine) = routine else {
            if carrier.used_bulk() > 0 {
                // Recover an old interrupted load before reserving another.
                ensure_move_target(&mut commands, porter_entity, move_target, hall_entrance);
                continue;
            }
            let mut offers: Vec<(Entity, Good, u32, Vec3)> = businesses
                .iter()
                .filter_map(
                    |(entity, building, building_of, at, rotation, inventory, policy, _)| {
                        if building_of.0 != *settlement_id || !policy.collection_enabled {
                            return None;
                        }
                        let good = business_output(building.kind)?;
                        if market.pool(good).bid < policy.minimum_unit_price {
                            return None;
                        }
                        let surplus = inventory.amount(good).saturating_sub(policy.keep_units);
                        let carrier_room = carrier.free_bulk() / good.bulk_per_unit();
                        let hall_room = hall_store.free_bulk() / good.bulk_per_unit();
                        let units = surplus
                            .min(policy.max_units_per_collection)
                            .min(carrier_room)
                            .min(hall_room);
                        (units > 0).then_some((
                            entity,
                            good,
                            units,
                            building.kind.entrance_position(at.0, rotation.0),
                        ))
                    },
                )
                .collect();
            offers.sort_unstable_by_key(|(entity, _, _, _)| entity.to_bits());
            let Some((business, good, offered, entrance)) = offers.into_iter().next() else {
                *activity = CharacterActivity::Indoors;
                commands.entity(porter_entity).remove::<MoveTarget>();
                continue;
            };
            let trade = market.buy_from_producer(good, hall_store.amount(good), offered);
            if trade.units == 0 {
                continue;
            }
            *activity = CharacterActivity::Idle;
            commands.entity(porter_entity).insert((
                MarketCollectionRoutine {
                    business,
                    hall: porter.settlement,
                    good,
                    reserved_units: trade.units,
                    reserved_pennies: trade.pennies,
                    phase: MarketCollectionPhase::GoingToBusiness,
                },
                MoveTarget(entrance),
            ));
            continue;
        };

        if routine.hall != porter.settlement {
            commands
                .entity(porter_entity)
                .remove::<MarketCollectionRoutine>()
                .remove::<MoveTarget>();
            continue;
        }
        match routine.phase {
            MarketCollectionPhase::GoingToBusiness => {
                let Ok((_, building, _, at, rotation, mut store, _, _)) =
                    businesses.get_mut(routine.business)
                else {
                    market.cancel_producer_purchase(
                        routine.good,
                        hall_store.amount(routine.good),
                        shared::economy::MarketTrade {
                            units: routine.reserved_units,
                            pennies: routine.reserved_pennies,
                        },
                    );
                    commands
                        .entity(porter_entity)
                        .remove::<MarketCollectionRoutine>()
                        .remove::<MoveTarget>();
                    continue;
                };
                let entrance = building.kind.entrance_position(at.0, rotation.0);
                if ground_distance(position.0, entrance) > WORK_REACH {
                    ensure_move_target(&mut commands, porter_entity, move_target, entrance);
                    continue;
                }
                let moved = store.transfer_to(&mut carrier, routine.good, routine.reserved_units);
                if moved != routine.reserved_units {
                    market.cancel_producer_purchase(
                        routine.good,
                        hall_store.amount(routine.good),
                        shared::economy::MarketTrade {
                            units: routine.reserved_units,
                            pennies: routine.reserved_pennies,
                        },
                    );
                    let replacement = market.buy_from_producer(
                        routine.good,
                        hall_store.amount(routine.good),
                        moved,
                    );
                    routine.reserved_units = replacement.units;
                    routine.reserved_pennies = replacement.pennies;
                }
                if routine.reserved_units == 0 {
                    commands
                        .entity(porter_entity)
                        .remove::<MarketCollectionRoutine>()
                        .remove::<MoveTarget>();
                    continue;
                }
                *activity = CharacterActivity::Idle;
                commands
                    .entity(porter_entity)
                    .insert(MoveTarget(hall_entrance));
                routine.phase = MarketCollectionPhase::ReturningToHall;
            }
            MarketCollectionPhase::ReturningToHall => {
                if ground_distance(position.0, hall_entrance) > WORK_REACH {
                    ensure_move_target(&mut commands, porter_entity, move_target, hall_entrance);
                    continue;
                }
                let delivered =
                    carrier.transfer_to(&mut hall_store, routine.good, routine.reserved_units);
                if let Ok((_, _, _, _, _, _, _, mut account)) = businesses.get_mut(routine.business)
                {
                    if delivered == routine.reserved_units {
                        account.cash = account.cash.saturating_add(routine.reserved_pennies);
                    }
                }
                *activity = CharacterActivity::Indoors;
                commands
                    .entity(porter_entity)
                    .remove::<MarketCollectionRoutine>()
                    .remove::<MoveTarget>();
            }
        }
    }
}

/// Pay daily wages from business cash, distribute only genuine surplus to the
/// owner, and let a wealthy owner leave hands-on work when a replacement is
/// ready. This is daily O(people + workplaces), not per-frame decision search.
pub fn run_business_payroll_and_owner_leisure(
    mut commands: Commands,
    world_time: Query<&WorldTime>,
    settlements: Query<(Entity, &shared::components::SettlementId)>,
    mut businesses: Query<(
        Entity,
        &SettlementBuilding,
        &mut BusinessAccount,
        &mut BusinessWagePolicy,
        &shared::components::BuildingId,
        &shared::components::BuildingOf,
        Option<&shared::components::OwnedBy>,
    )>,
    mut villagers: Query<(
        Entity,
        &CharacterName,
        &VillagerIntent,
        &shared::components::PersonId,
        Option<&shared::components::EmployedAt>,
        &mut Wallet,
        &mut Occupation,
        &mut WorkStatus,
    )>,
    mut processed_day: Local<Option<u32>>,
) {
    let Some(day) = world_time.iter().next().map(|clock| clock.day) else {
        return;
    };
    if *processed_day == Some(day) {
        return;
    }
    *processed_day = Some(day);
    let settlements_by_id: HashMap<shared::components::SettlementId, Entity> = settlements
        .iter()
        .map(|(entity, settlement_id)| (*settlement_id, entity))
        .collect();
    // Build one daily index. Looking up every roster name by scanning all
    // villagers made payroll O(businesses * population), and the wealthy-owner
    // check repeated that cost even on ticks with no day boundary.
    let mut people_by_id: HashMap<shared::components::PersonId, Entity> = HashMap::new();
    let mut workers_by_building: HashMap<shared::components::BuildingId, Vec<Entity>> =
        HashMap::new();
    let mut available_replacements: HashMap<Entity, usize> = HashMap::new();
    for (entity, _name, intent, person_id, employed_at, _, occupation, status) in villagers.iter() {
        let Some(settlement) = intent.settlement() else {
            continue;
        };
        people_by_id.insert(*person_id, entity);
        if let Some(employment) = employed_at {
            workers_by_building
                .entry(employment.0)
                .or_default()
                .push(entity);
        }
        if occupation.0.is_none() && employed_at.is_none() && *status == WorkStatus::LookingForWork
        {
            *available_replacements.entry(settlement).or_default() += 1;
        }
    }

    for (
        business_entity,
        building,
        mut account,
        mut wage_policy,
        building_id,
        building_of,
        owner_id,
    ) in businesses.iter_mut()
    {
        if business_output(building.kind).is_none() {
            continue;
        }
        let Some(settlement) = settlements_by_id.get(&building_of.0).copied() else {
            continue;
        };
        let mut worker_entities: Vec<Entity> = workers_by_building
            .get(building_id)
            .cloned()
            .unwrap_or_default();
        worker_entities.sort_unstable_by_key(|entity| entity.to_bits());
        worker_entities.dedup();
        let worker_count = worker_entities.len();
        let owner_entity = owner_id.and_then(|owner| people_by_id.get(&owner.0).copied());
        if account.last_payroll_day == u32::MAX {
            account.last_payroll_day = day;
        }
        let elapsed = day.saturating_sub(account.last_payroll_day);
        if elapsed > 0 {
            account.last_payroll_day = day;
            wage_policy.daily_wage = wage_policy
                .daily_wage
                .clamp(MINIMUM_BUSINESS_DAILY_WAGE, MAXIMUM_BUSINESS_DAILY_WAGE);
            let per_worker = wage_policy.daily_wage.saturating_mul(u64::from(elapsed));
            account.wage_arrears = account
                .wage_arrears
                .saturating_add(per_worker.saturating_mul(worker_count as u64));

            // Arrears are a real workplace liability, not merely a warning
            // counter. When later sales make cash available, distribute the
            // entire affordable obligation evenly so no alphabetically-early
            // worker is always paid while everybody else starves.
            if !worker_entities.is_empty() {
                let payment_budget = account.cash.min(account.wage_arrears);
                let worker_count = worker_entities.len() as u64;
                let equal_share = payment_budget / worker_count;
                let remainder = payment_budget % worker_count;
                let mut paid = 0_u64;
                for (index, worker) in worker_entities.iter().copied().enumerate() {
                    let payment = equal_share + u64::from((index as u64) < remainder);
                    let Ok((_, _, _, _, _, mut wallet, _, _)) = villagers.get_mut(worker) else {
                        continue;
                    };
                    wallet.credit(payment);
                    paid = paid.saturating_add(payment);
                }
                account.cash = account.cash.saturating_sub(paid);
                account.wage_arrears = account.wage_arrears.saturating_sub(paid);
            }

            if owner_id.is_some() {
                let payroll_reserve = wage_policy
                    .daily_wage
                    .saturating_mul(worker_count as u64)
                    .saturating_mul(2)
                    .saturating_add(2 * PENNIES_PER_COIN)
                    .saturating_add(account.wage_arrears);
                let draw = account.cash.saturating_sub(payroll_reserve);
                if draw > 0 {
                    if let Some(owner_entity) = owner_entity {
                        let Ok((_, _, _, _, _, mut wallet, _, _)) = villagers.get_mut(owner_entity)
                        else {
                            continue;
                        };
                        account.cash -= draw;
                        wallet.credit(draw);
                    }
                }
            }

            review_automatic_wage_offer(
                &mut wage_policy,
                elapsed,
                worker_count,
                building.kind.positions() as usize,
                account.cash,
                account.wage_arrears,
            );
        }

        let Some(_owner_id) = owner_id else {
            continue;
        };
        let Some(owner_entity) = owner_entity else {
            continue;
        };
        let owner_is_worker =
            villagers
                .get(owner_entity)
                .is_ok_and(|(_, _, _, _, employed_at, _, _, _)| {
                    employed_at.copied() == Some(shared::components::EmployedAt(*building_id))
                });
        if !owner_is_worker {
            continue;
        }
        let replacement_exists = available_replacements
            .get(&settlement)
            .copied()
            .unwrap_or(0)
            > 0;
        let owner_is_wealthy = villagers
            .get(owner_entity)
            .is_ok_and(|(_, _, _, _, _, wallet, _, _)| wallet.balance() >= WEALTHY_OWNER_MONEY);
        let payroll_secure = account.cash
            >= wage_policy
                .daily_wage
                .saturating_mul(worker_count as u64)
                .saturating_mul(2);
        if !replacement_exists || !owner_is_wealthy || !payroll_secure {
            continue;
        }
        if let Ok((_, owner_name, _, _, _, _, mut occupation, mut status)) =
            villagers.get_mut(owner_entity)
        {
            occupation.0 = None;
            *status = WorkStatus::Chilling;
            commands
                .entity(owner_entity)
                .remove::<shared::components::EmployedAt>()
                .remove::<FarmerRoutine>()
                .remove::<FishingRoutine>()
                .remove::<LumberjackRoutine>()
                .remove::<WorkplaceDoorTransit>()
                .remove::<MoveTarget>();
            info!(
                "Village '{}': {} became a wealthy owner and left daily {} work",
                building.settlement,
                owner_name.0,
                building.kind.trade().unwrap_or("business")
            );
        }
        let _ = business_entity;
    }
}
