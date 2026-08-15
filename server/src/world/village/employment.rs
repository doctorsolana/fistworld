//! Vacancy matching, skill requirements and durable workplace assignment.

use super::*;

/// Equal-wage founding businesses must form a usable production chain before
/// a two-seat workplace monopolises a very small Hamlet's labour. This is only
/// a tie-breaker: an owner can still recruit differently by changing wages.
const fn founding_job_priority(kind: SettlementBuildingKind) -> u8 {
    match kind {
        SettlementBuildingKind::Farmstead | SettlementBuildingKind::FishermansHut => 0,
        SettlementBuildingKind::Windmill => 1,
        SettlementBuildingKind::LumberjackHut => 2,
        SettlementBuildingKind::Bakery => 3,
        _ => 4,
    }
}

/// Cheap daily staffing decision used by NPC/autopilot sites. Manual Company
/// Masters keep their exact target. A depot begins with one porter and adds
/// capacity only when its own physical store is busy; distressed productive
/// sites contract instead of accumulating payroll for empty positions.
pub fn review_automatic_staffing(
    world_time: Query<&WorldTime>,
    mut last_day: Local<Option<u32>>,
    mut buildings: Query<(
        &SettlementBuilding,
        &GoodsInventory,
        &BusinessManagementPolicy,
        Option<&BusinessCondition>,
        &BusinessAccount,
        &mut BusinessStaffingPolicy,
    )>,
) {
    let day = world_time.iter().next().map_or(0, |clock| clock.day);
    if *last_day == Some(day) {
        return;
    }
    *last_day = Some(day);
    for (building, inventory, management, condition, account, mut staffing) in buildings.iter_mut()
    {
        if !management.autopilot {
            continue;
        }
        let state = condition.map_or(BusinessState::Operating, |condition| condition.state);
        let target = if matches!(
            state,
            BusinessState::Insolvent
                | BusinessState::Liquidating
                | BusinessState::ForSale
                | BusinessState::Closed
        ) {
            0
        } else if building.kind == SettlementBuildingKind::StorageHall {
            let utilisation =
                inventory.used_bulk().saturating_mul(100) / inventory.bulk_capacity().max(1);
            match utilisation {
                90.. => building.kind.positions(),
                70..=89 => 3.min(building.kind.positions()),
                35..=69 => 2.min(building.kind.positions()),
                _ => 1.min(building.kind.positions()),
            }
        } else if matches!(state, BusinessState::Distressed | BusinessState::CashTight) {
            1.min(building.kind.positions())
        } else if state == BusinessState::New
            && account.gross_revenue == 0
            && account.current_day.produced_units == 0
            && account.previous_day.produced_units == 0
        {
            automatic_opening_positions(building.kind)
        } else {
            building.kind.positions()
        };
        staffing.enabled_positions = target;
    }
}

/// Residents take vacant positions in their own settlement.
///
/// This is the smallest honest version of WORLD-DESIGN section 1a's rule that
/// production is people in jobs rather than population times a multiplier. A
/// position is held by a durable PersonId, so renaming somebody cannot vacate,
/// duplicate or transfer their job.
///
/// Founding workplaces have no skill minimum. If a future specialist building
/// carries [`WorkforceRequirements`], only matching residents can fill it.
/// Among jobs a resident can do, the highest daily wage recruits first and
/// distance breaks the choice of which resident takes that offer.
pub fn fill_vacancies(
    mut commands: Commands,
    mut buildings: Query<(
        Entity,
        &mut SettlementBuilding,
        &PlayerPosition,
        Option<&BusinessWagePolicy>,
        Option<&WorkforceRequirements>,
        Option<&BusinessCondition>,
        &shared::components::BuildingId,
        &shared::components::BuildingOf,
        Option<&shared::components::OperatedBy>,
    )>,
    mut villagers: Query<(
        Entity,
        &CharacterName,
        &VillagerIntent,
        &PlayerPosition,
        &mut Occupation,
        Option<&WorkStatus>,
        Option<&CharacterAttributes>,
        Option<&shared::components::EmployedAt>,
        Option<&shared::components::CivicEmployment>,
        &shared::components::PersonId,
    )>,
    settlements: Query<(Entity, &Settlement, &shared::components::SettlementId)>,
    companies: Query<(
        &shared::components::CompanyId,
        &shared::components::CompanyLeadership,
    )>,
    staffing_policies: Query<(&shared::components::BuildingId, &BusinessStaffingPolicy)>,
    active_builders: Query<
        (),
        Or<(
            With<ConstructionMaterialRoutine>,
            With<RoadBuilderRoutine>,
            With<moot_services::PermitPickupRoutine>,
        )>,
    >,
) {
    let staffing_targets: HashMap<shared::components::BuildingId, usize> = staffing_policies
        .iter()
        .map(|(building, policy)| (*building, usize::from(policy.enabled_positions)))
        .collect();
    let mut assigned_entities = HashSet::new();
    let mut people_by_id = HashMap::new();
    let mut stable_workers_by_building: HashMap<
        shared::components::BuildingId,
        Vec<(shared::components::PersonId, Entity, String)>,
    > = HashMap::new();
    for (entity, name, _, _, _, _, _, employment, civic_job, person_id) in villagers.iter() {
        people_by_id.insert(*person_id, entity);
        if employment.is_some() || civic_job.is_some() {
            assigned_entities.insert(entity);
        }
        if employment.is_some() && civic_job.is_some() {
            // The public post owns the working day. Commands are applied before
            // routine assignment later in the shared chained schedule.
            commands
                .entity(entity)
                .remove::<shared::components::EmployedAt>();
        } else if let Some(employment) = employment {
            stable_workers_by_building
                .entry(employment.0)
                .or_default()
                .push((*person_id, entity, name.0.clone()));
        }
    }
    for workers in stable_workers_by_building.values_mut() {
        workers.sort_unstable_by_key(|(person_id, entity, _)| (person_id.0, entity.to_bits()));
    }
    let mut worker_counts: HashMap<shared::components::BuildingId, usize> =
        stable_workers_by_building
            .iter()
            .map(|(building_id, workers)| (*building_id, workers.len()))
            .collect();

    // Stable employment is authoritative and the readable roster is derived
    // from it. This preserves two different people with the same display name
    // and cleans civic/business double assignment without guessing by name.
    for (_, mut building, _, _, _, _, building_id, _, _) in buildings.iter_mut() {
        let roster: Vec<String> = stable_workers_by_building
            .get(building_id)
            .into_iter()
            .flatten()
            .map(|(_, _, name)| name.clone())
            .collect();
        if building.workers != roster {
            building.workers = roster;
        }
    }
    let master_by_company: HashMap<_, _> = companies
        .iter()
        .map(|(company, leadership)| (*company, leadership.master))
        .collect();
    if !villagers.iter().any(
        |(entity, _, intent, _, occupation, status, _, employed_at, civic_job, _)| {
            intent.is_settled()
                && occupation.0.is_none()
                && employed_at.is_none()
                && civic_job.is_none()
                && !assigned_entities.contains(&entity)
                && active_builders.get(entity).is_err()
                && status.is_none_or(|status| *status == WorkStatus::LookingForWork)
        },
    ) {
        return;
    }

    for (settlement_entity, settlement, settlement_id) in settlements.iter() {
        loop {
            // Best offer in this settlement. Skill requirements remain absent
            // on every founding business, but are part of the vacancy now so
            // later specialist buildings do not need a parallel job system.
            let mut vacancies: Vec<_> = buildings
                .iter()
                .filter(
                    |(_, building, _, _, _, condition, building_id, building_of, _)| {
                        building_of.0 == *settlement_id
                            && !condition
                                .is_some_and(|condition| !condition.state.accepts_new_workers())
                            && worker_counts.get(*building_id).copied().unwrap_or(0)
                                < staffing_targets
                                    .get(*building_id)
                                    .copied()
                                    .unwrap_or(building.kind.positions() as usize)
                                    .min(building.kind.positions() as usize)
                    },
                )
                .map(
                    |(entity, building, at, wage, requirements, _, building_id, _, operated_by)| {
                        (
                            entity,
                            *building_id,
                            building.kind,
                            at.0,
                            wage.map_or(FOUNDING_DAILY_WAGE, |policy| policy.daily_wage),
                            requirements.copied(),
                            worker_counts.get(building_id).copied().unwrap_or(0),
                            operated_by.copied(),
                        )
                    },
                )
                .collect();
            vacancies.sort_by(|a, b| {
                b.4.cmp(&a.4)
                    .then_with(|| usize::from(a.6 > 0).cmp(&usize::from(b.6 > 0)))
                    .then_with(|| founding_job_priority(a.2).cmp(&founding_job_priority(b.2)))
                    .then_with(|| a.3.x.total_cmp(&b.3.x))
                    .then_with(|| a.3.z.total_cmp(&b.3.z))
            });

            // A small founding firm should normally begin as the familiar
            // owner-master-worker shop. Protect an otherwise eligible Company
            // Master from being recruited by a neighbouring business while
            // one of their own sites has a vacancy. This is deliberately a
            // first-refusal preference, not an exemption from skill rules or
            // the one-person/one-job invariant. If a company has several open
            // sites, its best ordinary vacancy (the ordering above) wins.
            let mut protected_master_sites = HashMap::new();
            let mut master_by_site = HashMap::new();
            for vacancy in &vacancies {
                let Some(company) = vacancy.7.map(|operated_by| operated_by.0) else {
                    continue;
                };
                let Some(master) = master_by_company.get(&company).copied() else {
                    continue;
                };
                if protected_master_sites.contains_key(&master) {
                    continue;
                }
                let Some(master_entity) = people_by_id.get(&master).copied() else {
                    continue;
                };
                let eligible = villagers.get(master_entity).is_ok_and(
                    |(
                        entity,
                        _,
                        intent,
                        _,
                        occupation,
                        status,
                        attributes,
                        employed_at,
                        civic_job,
                        _,
                    )| {
                        intent.is_settled()
                            && intent.settlement() == Some(settlement_entity)
                            && occupation.0.is_none()
                            && employed_at.is_none()
                            && civic_job.is_none()
                            && !assigned_entities.contains(&entity)
                            && active_builders.get(entity).is_err()
                            && status.is_none_or(|status| *status == WorkStatus::LookingForWork)
                            && vacancy.5.is_none_or(|requirements| {
                                attributes
                                    .is_some_and(|attributes| requirements.is_met_by(*attributes))
                            })
                    },
                );
                if eligible {
                    protected_master_sites.insert(master, vacancy.1);
                    master_by_site.insert(vacancy.1, master);
                }
            }
            vacancies.sort_by(|a, b| {
                usize::from(master_by_site.contains_key(&b.1))
                    .cmp(&usize::from(master_by_site.contains_key(&a.1)))
                    .then_with(|| b.4.cmp(&a.4))
                    .then_with(|| usize::from(a.6 > 0).cmp(&usize::from(b.6 > 0)))
                    .then_with(|| founding_job_priority(a.2).cmp(&founding_job_priority(b.2)))
                    .then_with(|| a.3.x.total_cmp(&b.3.x))
                    .then_with(|| a.3.z.total_cmp(&b.3.z))
            });

            // Usually the best-paid offer finds somebody in one people scan.
            // Only an unfillable specialist offer falls through to the next;
            // ordinary jobs never scan the whole population once per building.
            let mut placement = None;
            for (vacancy_entity, vacancy_id, kind, plot, offered_wage, requirements, _, _) in
                vacancies
            {
                let preferred_master = master_by_site
                    .get(&vacancy_id)
                    .and_then(|master| people_by_id.get(master))
                    .and_then(|entity| villagers.get(*entity).ok())
                    .map(|(entity, name, _, _, _, _, _, _, _, _)| (entity, name.0.clone()));
                let taker = preferred_master.or_else(|| {
                    villagers
                        .iter()
                        .filter(
                            |(
                                entity,
                                _,
                                intent,
                                _,
                                occupation,
                                status,
                                attributes,
                                employed_at,
                                civic_job,
                                person_id,
                            )| {
                                intent.is_settled()
                                    && intent.settlement() == Some(settlement_entity)
                                    && occupation.0.is_none()
                                    && employed_at.is_none()
                                    && civic_job.is_none()
                                    && !assigned_entities.contains(entity)
                                    && !protected_master_sites.contains_key(*person_id)
                                    && active_builders.get(*entity).is_err()
                                    && status
                                        .is_none_or(|status| *status == WorkStatus::LookingForWork)
                                    && requirements.is_none_or(|requirements| {
                                        attributes.is_some_and(|attributes| {
                                            requirements.is_met_by(*attributes)
                                        })
                                    })
                            },
                        )
                        .min_by(|a, b| {
                            a.3 .0
                                .distance_squared(plot)
                                .total_cmp(&b.3 .0.distance_squared(plot))
                        })
                        .map(|(entity, name, _, _, _, _, _, _, _, _)| (entity, name.0.clone()))
                });
                if let Some((taker_entity, taker)) = taker {
                    placement = Some((
                        vacancy_entity,
                        vacancy_id,
                        kind,
                        offered_wage,
                        taker_entity,
                        taker,
                    ));
                    break;
                }
            }
            let Some((vacancy_entity, vacancy_id, kind, offered_wage, taker_entity, taker)) =
                placement
            else {
                break;
            };

            let Ok((_, mut building, _, _, _, _, building_id, _, _)) =
                buildings.get_mut(vacancy_entity)
            else {
                break;
            };
            if worker_counts.get(&vacancy_id).copied().unwrap_or(0)
                >= staffing_targets
                    .get(&vacancy_id)
                    .copied()
                    .unwrap_or(building.kind.positions() as usize)
                    .min(building.kind.positions() as usize)
            {
                break;
            }
            building.workers.push(taker.clone());
            *worker_counts.entry(vacancy_id).or_default() += 1;
            assigned_entities.insert(taker_entity);
            if let Ok((_, _, _, _, mut occupation, _, _, _, _, _)) = villagers.get_mut(taker_entity)
            {
                let title = kind.trade().unwrap_or("Villager").to_string();
                if occupation.0.as_deref() != Some(title.as_str()) {
                    occupation.0 = Some(title);
                }
            }
            let mut employee = commands.entity(taker_entity);
            employee.insert(WorkStatus::Employed);
            employee.insert(shared::components::EmployedAt(*building_id));
            info!(
                "Village '{}': {taker} took work as a {} for {} coin/day",
                settlement.name,
                kind.trade().unwrap_or("hand"),
                shared::economy::format_money(offered_wage),
            );
        }
    }
}

/// Release positions closed by an operator before the vacancy matcher runs.
/// The lowest stable PersonIds retain their jobs, making retries and save/load
/// deterministic. Released workers re-enter the ordinary labour market and
/// all job-specific movement state is cleared in the same ordered pass.
pub fn enforce_staffing_targets(
    mut commands: Commands,
    buildings: Query<(
        &shared::components::BuildingId,
        &SettlementBuilding,
        &BusinessStaffingPolicy,
    )>,
    mut villagers: Query<(
        Entity,
        &shared::components::PersonId,
        &shared::components::EmployedAt,
        &mut Occupation,
        &mut WorkStatus,
        &GoodsInventory,
        Has<InternalDeliveryRoutine>,
        Has<MarketCollectionRoutine>,
    )>,
) {
    let targets: HashMap<_, _> = buildings
        .iter()
        .map(|(id, building, policy)| (*id, policy.target_for(building.kind) as usize))
        .collect();
    let mut workers: HashMap<
        shared::components::BuildingId,
        Vec<(shared::components::PersonId, Entity)>,
    > = HashMap::new();
    for (entity, person, employment, ..) in villagers.iter() {
        if targets.contains_key(&employment.0) {
            workers
                .entry(employment.0)
                .or_default()
                .push((*person, entity));
        }
    }
    for (building, roster) in workers.iter_mut() {
        roster.sort_unstable_by_key(|(person, entity)| (*person, entity.to_bits()));
        let target = targets.get(building).copied().unwrap_or(roster.len());
        for (_, entity) in roster.iter().skip(target) {
            let Ok((_, _, _, mut occupation, mut status, carrier, internal, market)) =
                villagers.get_mut(*entity)
            else {
                continue;
            };
            // Closing a porter position is graceful. Keep the employment until
            // an already-promised shipment reaches its destination and the
            // carrier is empty; otherwise the same person can be hired into a
            // second job while physically holding somebody else's goods.
            if internal || market || !carrier.is_empty() {
                continue;
            }
            occupation.0 = None;
            *status = WorkStatus::LookingForWork;
            commands
                .entity(*entity)
                .remove::<shared::components::EmployedAt>()
                .remove::<FarmerRoutine>()
                .remove::<FishingRoutine>()
                .remove::<LumberjackRoutine>()
                .remove::<ProcessingRoutine>()
                .remove::<WorkplaceDoorTransit>()
                .remove::<BuildingDoorUse>()
                .remove::<PierTraversal>()
                .remove::<MoveTarget>()
                .remove::<TravelRoute>()
                .remove::<NavigationRoutePending>()
                .remove::<NavigationRouteFailed>()
                .remove::<WorkerOffDuty>();
        }
    }
}

/// Derive private logistics authority from durable employment. Storage Hall
/// workers are ordinary one-job employees; this marker only grants their work
/// routine permission and disappears immediately when that employment ends.
pub fn sync_company_porters(
    mut commands: Commands,
    halls: Query<(Entity, &shared::components::SettlementId), With<Settlement>>,
    storage_halls: Query<(
        &shared::components::BuildingId,
        &shared::components::BuildingOf,
        &shared::components::OperatedBy,
        &SettlementBuilding,
    )>,
    workers: Query<
        (
            Entity,
            Option<&shared::components::EmployedAt>,
            Option<&CompanyPorter>,
            Option<&shared::components::CivicEmployment>,
            &GoodsInventory,
            Has<InternalDeliveryRoutine>,
            Has<MarketCollectionRoutine>,
        ),
        With<CharacterKind>,
    >,
) {
    let hall_by_settlement: HashMap<_, _> = halls
        .iter()
        .map(|(entity, settlement)| (*settlement, entity))
        .collect();
    let storage_by_id: HashMap<_, _> = storage_halls
        .iter()
        .filter(|(_, _, _, building)| building.kind == SettlementBuildingKind::StorageHall)
        .filter_map(|(id, building_of, company, _)| {
            hall_by_settlement.get(&building_of.0).copied().map(|hall| {
                (
                    *id,
                    CompanyPorter {
                        settlement: hall,
                        settlement_id: building_of.0,
                        company: company.0,
                        storage_hall: *id,
                    },
                )
            })
        })
        .collect();
    for (entity, employment, current, civic_job, carrier, internal, market) in workers.iter() {
        let wanted = employment
            .and_then(|employment| storage_by_id.get(&employment.0))
            .copied()
            .filter(|_| civic_job.is_none());
        if wanted == current.copied() {
            continue;
        }
        if let Some(wanted) = wanted {
            commands.entity(entity).insert(wanted);
        } else if current.is_some() {
            // Employment normally disappears only after the graceful staffing
            // boundary above. Keep this fallback for death, sale and unusual
            // lifecycle changes so an in-flight shipment can still complete.
            if internal || market || !carrier.is_empty() {
                continue;
            }
            commands
                .entity(entity)
                .remove::<CompanyPorter>()
                .remove::<InternalDeliveryRoutine>()
                .remove::<MarketCollectionRoutine>()
                .remove::<MoveTarget>();
        }
    }
}
