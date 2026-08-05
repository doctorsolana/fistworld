//! Vacancy matching, skill requirements and durable workplace assignment.

use super::*;

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
        &shared::components::BuildingId,
        &shared::components::BuildingOf,
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
    active_builders: Query<(), Or<(With<ConstructionMaterialRoutine>, With<RoadBuilderRoutine>)>>,
) {
    let mut assigned_entities = HashSet::new();
    let mut stable_workers_by_building: HashMap<
        shared::components::BuildingId,
        Vec<(shared::components::PersonId, Entity, String)>,
    > = HashMap::new();
    for (entity, name, _, _, _, _, _, employment, civic_job, person_id) in villagers.iter() {
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
    for (_, mut building, _, _, _, building_id, _) in buildings.iter_mut() {
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
                .filter(|(_, building, _, _, _, building_id, building_of)| {
                    building_of.0 == *settlement_id
                        && worker_counts.get(*building_id).copied().unwrap_or(0)
                            < building.kind.positions() as usize
                })
                .map(
                    |(entity, building, at, wage, requirements, building_id, _)| {
                        (
                            entity,
                            *building_id,
                            building.kind,
                            at.0,
                            wage.map_or(FOUNDING_DAILY_WAGE, |policy| policy.daily_wage),
                            requirements.copied(),
                        )
                    },
                )
                .collect();
            vacancies.sort_by(|a, b| {
                b.4.cmp(&a.4)
                    .then_with(|| a.3.x.total_cmp(&b.3.x))
                    .then_with(|| a.3.z.total_cmp(&b.3.z))
            });

            // Usually the best-paid offer finds somebody in one people scan.
            // Only an unfillable specialist offer falls through to the next;
            // ordinary jobs never scan the whole population once per building.
            let mut placement = None;
            for (vacancy_entity, vacancy_id, kind, plot, offered_wage, requirements) in vacancies {
                let taker = villagers
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
                            _,
                        )| {
                            intent.is_settled()
                                && intent.settlement() == Some(settlement_entity)
                                && occupation.0.is_none()
                                && employed_at.is_none()
                                && civic_job.is_none()
                                && !assigned_entities.contains(entity)
                                && active_builders.get(*entity).is_err()
                                && status.is_none_or(|status| *status == WorkStatus::LookingForWork)
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
                    .map(|(entity, name, _, _, _, _, _, _, _, _)| (entity, name.0.clone()));
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

            let Ok((_, mut building, _, _, _, building_id, _)) = buildings.get_mut(vacancy_entity)
            else {
                break;
            };
            if worker_counts.get(&vacancy_id).copied().unwrap_or(0)
                >= building.kind.positions() as usize
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
