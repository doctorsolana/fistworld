//! Bounded public pay review and competition for ordinary local workers.

use super::*;
use shared::components::{
    BuildingId, BuildingOf, CivicEmployment, EmployedAt, PersonId, SettlementId,
};

#[derive(Clone, Copy)]
struct PrivateOffer {
    entity: Entity,
    building: BuildingId,
    kind: SettlementBuildingKind,
    wage: u64,
    vacancies: usize,
    requirements: Option<WorkforceRequirements>,
}

impl PrivateOffer {
    fn eligible(&self, attributes: Option<&CharacterAttributes>) -> bool {
        self.vacancies > 0
            && self.requirements.is_none_or(|requirements| {
                attributes.is_some_and(|attributes| requirements.is_met_by(*attributes))
            })
    }
}

#[derive(Resource, Default)]
pub(crate) struct CivicLaborMarket {
    offers: HashMap<SettlementId, Vec<PrivateOffer>>,
    funded: HashSet<BuildingId>,
    last_hour: Option<(u32, u32)>,
    switched: HashMap<PersonId, u32>,
}

impl CivicLaborMarket {
    pub(crate) fn is_funded(&self, building: BuildingId) -> bool {
        self.funded.contains(&building)
    }

    pub(crate) fn prefers_private(
        &self,
        settlement: SettlementId,
        public_wage: u64,
        attributes: Option<&CharacterAttributes>,
    ) -> bool {
        self.offers
            .get(&settlement)
            .into_iter()
            .flatten()
            .any(|offer| offer.eligible(attributes) && offer.wage > public_wage)
    }
}

/// One world-wide census each simulated hour. Hiring paths consume this small
/// local cache instead of adding another full business scan per applicant.
#[allow(clippy::type_complexity)]
pub(crate) fn review_civic_labor_market(
    world_time: Query<&WorldTime>,
    workers: Query<&EmployedAt>,
    companies: Query<(
        &shared::components::CompanyId,
        &shared::economy::CompanyAccount,
    )>,
    businesses: Query<(
        Entity,
        &BuildingId,
        &BuildingOf,
        &SettlementBuilding,
        Option<&BusinessWagePolicy>,
        Option<&BusinessAccount>,
        Option<&BusinessStaffingPolicy>,
        Option<&BusinessCondition>,
        Option<&WorkforceRequirements>,
        Option<&shared::components::OperatedBy>,
    )>,
    mut market: ResMut<CivicLaborMarket>,
) {
    let Some(clock) = world_time.iter().next() else {
        return;
    };
    let hour = (clock.seconds_in_cycle / clock.cycle_duration().max(0.001) * 24.0).floor() as u32;
    if market.last_hour == Some((clock.day, hour)) {
        return;
    }
    if market.last_hour.is_none_or(|(day, _)| day != clock.day) {
        market.switched.clear();
    }
    market.last_hour = Some((clock.day, hour));
    let mut filled = HashMap::<BuildingId, usize>::new();
    for job in &workers {
        *filled.entry(job.0).or_default() += 1;
    }
    market.offers.clear();
    market.funded.clear();
    let mut commitments = HashMap::<shared::components::CompanyId, u64>::new();
    for (_, id, _, building, wage, account, staffing, condition, _, company) in &businesses {
        let Some(company) = company else { continue };
        let target = if condition.is_none_or(|state| state.state.accepts_new_workers()) {
            usize::from(staffing.map_or_else(
                || building.kind.positions(),
                |policy| policy.target_for(building.kind),
            ))
        } else {
            0
        };
        let roster = filled.get(id).copied().unwrap_or(0);
        let payroll = wage
            .map_or(FOUNDING_DAILY_WAGE, |wage| wage.daily_wage)
            .saturating_mul(target.max(roster) as u64);
        let debts = account.map_or(0, |account| {
            account.wage_arrears.saturating_add(account.tax_arrears)
        });
        let reserved = commitments.entry(company.0).or_default();
        *reserved = reserved.saturating_add(payroll).saturating_add(debts);
    }
    let funded_companies: HashSet<_> = companies
        .iter()
        .filter(|(id, account)| account.cash >= commitments.get(*id).copied().unwrap_or(u64::MAX))
        .map(|(id, _)| *id)
        .collect();
    for (entity, id, town, building, wage, account, staffing, condition, requirements, company) in
        &businesses
    {
        if company.is_some_and(|company| funded_companies.contains(&company.0)) {
            market.funded.insert(*id);
        }
        if !is_private_business(building.kind)
            || !market.is_funded(*id)
            || account.is_some_and(|account| account.wage_arrears > 0)
            || condition.is_some_and(|condition| !condition.state.accepts_new_workers())
        {
            continue;
        }
        let target = usize::from(staffing.map_or_else(
            || building.kind.positions(),
            |staffing| staffing.target_for(building.kind),
        ));
        let vacancies = target.saturating_sub(filled.get(id).copied().unwrap_or(0));
        if vacancies == 0 {
            continue;
        }
        market.offers.entry(town.0).or_default().push(PrivateOffer {
            entity,
            building: *id,
            kind: building.kind,
            wage: wage.map_or(FOUNDING_DAILY_WAGE, |wage| wage.daily_wage),
            vacancies,
            requirements: requirements.copied(),
        });
    }
    for offers in market.offers.values_mut() {
        offers.sort_unstable_by_key(|offer| (std::cmp::Reverse(offer.wage), offer.building));
    }
}

fn revised_civic_offer(current: u64, target: u64, affordable: u64) -> u64 {
    let step = current.div_ceil(10).max(5);
    let target = target
        .min(affordable)
        .clamp(MINIMUM_BUSINESS_DAILY_WAGE, MAXIMUM_BUSINESS_DAILY_WAGE);
    let next = if target > current {
        current.saturating_add(step).min(target)
    } else {
        current.saturating_sub(step).max(target)
    };
    next.min(affordable.max(MINIMUM_BUSINESS_DAILY_WAGE))
}

/// The enacted offer changes only after old-wage accrual. Cash may support a
/// higher public bid, but neither private quotes nor hunger create treasury.
#[allow(clippy::type_complexity)]
pub(crate) fn review_civic_wages(
    world_time: Query<&WorldTime>,
    market: Res<CivicLaborMarket>,
    mut halls: Query<(
        &SettlementId,
        &Settlement,
        &SettlementPolicies,
        &mut MootAdministration,
        Option<&MootMarket>,
        Option<&GoodsInventory>,
    )>,
    mut last_day: Local<Option<u32>>,
) {
    let Some(day) = world_time.iter().next().map(|clock| clock.day) else {
        return;
    };
    if *last_day == Some(day) {
        return;
    }
    *last_day = Some(day);
    for (id, settlement, policies, mut office, exchange, stock) in &mut halls {
        if !policies.autopilot {
            continue;
        }
        let current = office.steward_daily_salary.max(MINIMUM_BUSINESS_DAILY_WAGE);
        let local_meal = exchange
            .zip(stock)
            .map(|(exchange, stock)| households::household_ration_price(stock, exchange));
        let private = market
            .offers
            .get(id)
            .into_iter()
            .flatten()
            .map(|offer| offer.wage)
            .max()
            .unwrap_or(current);
        let desired = local_meal
            .map_or(current, |price| price.saturating_mul(5).div_ceil(4))
            .max(private);
        let protected_positions = civic::filled_civic_positions(&office).max(1) as u64;
        let runway = u64::from(policies.civic_payroll_reserve_days.max(1));
        let affordable = settlement.treasury.saturating_sub(office.wage_arrears)
            / protected_positions.saturating_mul(runway).max(1);
        let desired = if office.wage_arrears > 0 {
            desired.min(current.saturating_sub(current.div_ceil(10)))
        } else {
            desired
        };
        let next = revised_civic_offer(current, desired, affordable);
        if next != office.steward_daily_salary {
            office.steward_daily_salary = next;
        }
    }
}

/// Public workers may accept a better eligible private vacancy once daily,
/// retrying hourly until their current cargo or construction work is complete.
#[allow(clippy::type_complexity)]
pub(crate) fn review_civic_job_choices(
    mut commands: Commands,
    world_time: Query<&WorldTime>,
    mut market: ResMut<CivicLaborMarket>,
    mut halls: Query<(&SettlementId, &mut MootAdministration)>,
    employed: Query<&EmployedAt>,
    businesses: Query<(
        &BuildingId,
        &BuildingOf,
        &SettlementBuilding,
        Option<&BusinessWagePolicy>,
        Option<&BusinessAccount>,
        Option<&BusinessStaffingPolicy>,
        Option<&BusinessCondition>,
        Option<&WorkforceRequirements>,
    )>,
    mut workers: Query<(
        Entity,
        &PersonId,
        &CivicEmployment,
        &mut Occupation,
        &mut WorkStatus,
        Option<&GoodsInventory>,
        Option<&CharacterAttributes>,
    )>,
    busy: Query<(), super::worker_activity::JobChangeBlocked>,
    mut last_hour: Local<Option<(u32, u32)>>,
) {
    let Some(clock) = world_time.iter().next() else {
        return;
    };
    let hour = (clock.seconds_in_cycle / clock.cycle_duration().max(0.001) * 24.0).floor() as u32;
    if *last_hour == Some((clock.day, hour)) {
        return;
    }
    *last_hour = Some((clock.day, hour));
    let mut live_filled = HashMap::<BuildingId, usize>::new();
    for job in &employed {
        *live_filled.entry(job.0).or_default() += 1;
    }
    let offices: HashMap<_, _> = halls
        .iter()
        .map(|(id, office)| (*id, office.steward_daily_salary))
        .collect();
    let mut ordered: Vec<_> = workers
        .iter()
        .map(|(entity, id, ..)| (*id, entity))
        .collect();
    ordered.sort_unstable_by_key(|(id, _)| *id);
    for (id, entity) in ordered {
        if market.switched.get(&id) == Some(&clock.day) || busy.contains(entity) {
            continue;
        }
        let Ok((_, _, job, mut occupation, mut status, cargo, attributes)) =
            workers.get_mut(entity)
        else {
            continue;
        };
        if cargo.is_some_and(|cargo| !cargo.is_empty()) {
            continue;
        }
        let Some(current) = offices.get(&job.settlement) else {
            continue;
        };
        let raise = current.div_ceil(10).max(10);
        let next = market.offers.get(&job.settlement).and_then(|offers| {
            offers
                .iter()
                .filter_map(|offer| {
                    let Ok((
                        building_id,
                        town,
                        building,
                        wage,
                        account,
                        staffing,
                        condition,
                        requirements,
                    )) = businesses.get(offer.entity)
                    else {
                        return None;
                    };
                    let wage = wage.map_or(FOUNDING_DAILY_WAGE, |wage| wage.daily_wage);
                    let target = usize::from(staffing.map_or_else(
                        || building.kind.positions(),
                        |policy| policy.target_for(building.kind),
                    ));
                    if *building_id != offer.building
                        || !market.is_funded(*building_id)
                        || town.0 != job.settlement
                        || wage < current.saturating_add(raise)
                        || account.is_some_and(|account| account.wage_arrears > 0)
                        || condition.is_some_and(|condition| !condition.state.accepts_new_workers())
                        || requirements.is_some_and(|requirements| {
                            attributes.is_none_or(|attributes| !requirements.is_met_by(*attributes))
                        })
                        || live_filled.get(building_id).copied().unwrap_or(0) >= target
                    {
                        return None;
                    }
                    Some(PrivateOffer {
                        wage,
                        kind: building.kind,
                        ..*offer
                    })
                })
                .max_by_key(|offer| (offer.wage, std::cmp::Reverse(offer.building)))
        });
        market.switched.insert(id, clock.day);
        let Some(next) = next else { continue };
        *live_filled.entry(next.building).or_default() += 1;
        if let Some(offer) = market.offers.get_mut(&job.settlement).and_then(|offers| {
            offers
                .iter_mut()
                .find(|offer| offer.building == next.building)
        }) {
            offer.vacancies = offer.vacancies.saturating_sub(1);
        }
        for (town, mut office) in &mut halls {
            if *town != job.settlement {
                continue;
            }
            for entry in &mut office.payroll {
                if entry.person_id == id {
                    entry.active = false;
                }
            }
        }
        occupation.0 = next.kind.trade().map(str::to_string);
        *status = WorkStatus::Employed;
        commands
            .entity(entity)
            .remove::<CivicEmployment>()
            .remove::<MootSteward>()
            .remove::<MoveTarget>()
            .remove::<TravelRoute>()
            .remove::<NavigationRoutePending>()
            .remove::<NavigationRouteFailed>()
            .insert(EmployedAt(next.building));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::components::CivicRole;

    #[test]
    fn public_wage_moves_gradually_but_never_raises_beyond_funded_runway() {
        assert_eq!(revised_civic_offer(100, 200, 1000), 110);
        assert_eq!(revised_civic_offer(100, 200, 105), 105);
        assert_eq!(revised_civic_offer(100, 200, 80), 80);
        assert_eq!(revised_civic_offer(100, 0, 0), MINIMUM_BUSINESS_DAILY_WAGE);
    }

    fn labor_fixture(cash: u64) -> (App, Entity, Entity, Entity) {
        let mut app = App::new();
        app.init_resource::<CivicLaborMarket>();
        app.add_systems(
            Update,
            (review_civic_labor_market, review_civic_job_choices).chain(),
        );
        let clock = app.world_mut().spawn(WorldTime::new_default()).id();
        let town = SettlementId(880);
        let company = shared::components::CompanyId(880);
        app.world_mut().spawn((
            company,
            shared::economy::CompanyAccount {
                cash: 10_000,
                ..default()
            },
        ));
        let hall = app
            .world_mut()
            .spawn((
                town,
                MootAdministration {
                    steward_daily_salary: 100,
                    payroll: vec![shared::components::CivicPayrollEntry {
                        person_id: PersonId(882),
                        name: "Ari".into(),
                        role: CivicRole::MootSteward,
                        daily_wage: 100,
                        arrears: cash,
                        last_accrual_day: 0,
                        active: true,
                    }],
                    ..default()
                },
            ))
            .id();
        app.world_mut().spawn((
            BuildingId(881),
            BuildingOf(town),
            shared::components::OperatedBy(company),
            SettlementBuilding {
                kind: SettlementBuildingKind::FishermansHut,
                settlement: "Laborford".into(),
                owner: None,
                quality: 1.0,
                workers: vec![],
            },
            BusinessWagePolicy {
                daily_wage: 200,
                ..default()
            },
        ));
        let worker = app
            .world_mut()
            .spawn((
                PersonId(882),
                CivicEmployment {
                    settlement: town,
                    role: CivicRole::MootSteward,
                },
                MootSteward { settlement: hall },
                Occupation(Some("Moot Steward".into())),
                WorkStatus::Employed,
                GoodsInventory::new(24),
                CharacterAttributes::default(),
            ))
            .id();
        (app, clock, hall, worker)
    }

    #[test]
    fn a_public_worker_accepts_a_better_private_offer_without_erasing_old_debt() {
        let (mut app, _, hall, worker) = labor_fixture(75);
        app.update();
        assert!(app.world().get::<CivicEmployment>(worker).is_none());
        assert!(app.world().get::<MootSteward>(worker).is_none());
        assert_eq!(
            app.world().get::<EmployedAt>(worker),
            Some(&EmployedAt(BuildingId(881)))
        );
        let claim = &app.world().get::<MootAdministration>(hall).unwrap().payroll[0];
        assert_eq!(claim.arrears, 75);
        assert!(!claim.active);
    }

    #[test]
    fn a_loaded_public_worker_retries_after_delivery_instead_of_losing_cargo() {
        let (mut app, clock, _, worker) = labor_fixture(0);
        app.world_mut()
            .get_mut::<GoodsInventory>(worker)
            .unwrap()
            .add(Good::Wood, 1);
        app.update();
        assert!(app.world().get::<CivicEmployment>(worker).is_some());
        assert_eq!(
            app.world()
                .get::<GoodsInventory>(worker)
                .unwrap()
                .amount(Good::Wood),
            1
        );
        app.world_mut()
            .get_mut::<GoodsInventory>(worker)
            .unwrap()
            .remove(Good::Wood, 1);
        let mut time = app.world_mut().get_mut::<WorldTime>(clock).unwrap();
        time.seconds_in_cycle += time.cycle_duration() / 12.0;
        drop(time);
        app.update();
        assert!(app.world().get::<CivicEmployment>(worker).is_none());
        assert!(app.world().get::<EmployedAt>(worker).is_some());
    }

    #[test]
    fn public_recruitment_waits_until_its_offer_matches_a_private_alternative() {
        let mut app = App::new();
        app.init_resource::<CivicLaborMarket>();
        app.add_systems(
            Update,
            (
                review_civic_labor_market,
                super::super::commerce::staff_moot_hall_roles,
            )
                .chain(),
        );
        app.world_mut().spawn(WorldTime::new_default());
        let town = SettlementId(800);
        let company = shared::components::CompanyId(800);
        app.world_mut().spawn((
            company,
            shared::economy::CompanyAccount {
                cash: 10_000,
                ..default()
            },
        ));
        let hall = app
            .world_mut()
            .spawn((
                town,
                Settlement {
                    name: "Offerford".into(),
                    tier: shared::components::SettlementTier::Hamlet,
                    residents: 2,
                    treasury: 10_000,
                },
                MootAdministration::default(),
                SettlementPolicies::default(),
            ))
            .id();
        app.world_mut().spawn((
            BuildingId(801),
            BuildingOf(town),
            shared::components::OperatedBy(company),
            SettlementBuilding {
                kind: SettlementBuildingKind::FishermansHut,
                settlement: "Offerford".into(),
                owner: None,
                quality: 1.0,
                workers: vec![],
            },
            BusinessWagePolicy {
                daily_wage: 200,
                ..default()
            },
        ));
        let person = app
            .world_mut()
            .spawn((
                PersonId(802),
                CharacterName("Jo".into()),
                VillagerIntent::Resident { settlement: hall },
                Occupation(None),
                WorkStatus::LookingForWork,
            ))
            .id();
        app.update();
        assert!(app.world().get::<CivicEmployment>(person).is_none());
        app.world_mut()
            .get_mut::<MootAdministration>(hall)
            .unwrap()
            .steward_daily_salary = 200;
        app.update();
        assert!(app.world().get::<CivicEmployment>(person).is_some());
    }
    #[test]
    fn public_switch_rechecks_staffing_after_the_hourly_offer_snapshot() {
        let (mut app, _, _, worker) = labor_fixture(0);
        let mut sites = app.world_mut().query_filtered::<Entity, With<BuildingId>>();
        let site = sites.single(app.world()).unwrap();
        app.world_mut()
            .entity_mut(site)
            .insert(BusinessStaffingPolicy::new(1));
        fn close_vacancy(mut sites: Query<&mut BusinessStaffingPolicy>) {
            for mut site in &mut sites {
                site.enabled_positions = 0;
            }
        }
        app.add_systems(
            Update,
            close_vacancy
                .after(review_civic_labor_market)
                .before(review_civic_job_choices),
        );
        app.update();
        assert!(app.world().get::<CivicEmployment>(worker).is_some());
        assert!(app.world().get::<EmployedAt>(worker).is_none());
    }

    #[test]
    fn public_switch_rechecks_a_vacancy_filled_after_the_cached_offer() {
        let (mut app, _, _, worker) = labor_fixture(0);
        let mut sites = app.world_mut().query_filtered::<Entity, With<BuildingId>>();
        let site = sites.single(app.world()).unwrap();
        app.world_mut()
            .entity_mut(site)
            .insert(BusinessStaffingPolicy::new(1));
        fn hire_other(mut commands: Commands) {
            commands.spawn(EmployedAt(BuildingId(881)));
        }
        app.add_systems(
            Update,
            hire_other
                .after(review_civic_labor_market)
                .before(review_civic_job_choices),
        );
        app.update();
        assert!(app.world().get::<CivicEmployment>(worker).is_some());
        assert!(app.world().get::<EmployedAt>(worker).is_none());
    }

    #[test]
    fn private_quotes_reserve_all_company_payroll_and_site_debts() {
        for cash in [449, 450] {
            let (mut app, _, _, _) = labor_fixture(0);
            let mut companies = app
                .world_mut()
                .query::<&mut shared::economy::CompanyAccount>();
            companies.single_mut(app.world_mut()).unwrap().cash = cash;
            let mut sites = app.world_mut().query_filtered::<Entity, With<BuildingId>>();
            let site = sites.single(app.world()).unwrap();
            app.world_mut()
                .entity_mut(site)
                .insert(BusinessStaffingPolicy::new(1));
            // The second site's two real employees still need pay even though
            // its owner has closed every advertised vacancy.
            app.world_mut().spawn((
                BuildingId(883),
                BuildingOf(SettlementId(880)),
                shared::components::OperatedBy(shared::components::CompanyId(880)),
                SettlementBuilding {
                    kind: SettlementBuildingKind::Farmstead,
                    settlement: "Laborford".into(),
                    owner: None,
                    quality: 1.0,
                    workers: vec![],
                },
                BusinessStaffingPolicy::new(0),
                BusinessWagePolicy::default(),
                BusinessAccount {
                    wage_arrears: 20,
                    tax_arrears: 30,
                    ..default()
                },
            ));
            app.world_mut().spawn(EmployedAt(BuildingId(883)));
            app.world_mut().spawn(EmployedAt(BuildingId(883)));
            app.update();
            assert_eq!(
                app.world()
                    .resource::<CivicLaborMarket>()
                    .is_funded(BuildingId(881)),
                cash == 450,
            );
        }
    }
}
