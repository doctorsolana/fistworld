//! Synthetic subsystem cost probes for the canonical village simulation.
//!
//! These schedules are deliberately not a complete server tick or a growth/
//! balance test. Production staging and daily resets are reported explicitly;
//! movement uses a separate dispersed cohort on authored clear dry ground.
//! Run optimised with `cargo village-scale-lab`, then measure the normal server
//! separately before claiming sustainable whole-world capacity.

mod active;

use std::process::Command;
use std::time::{Duration, Instant};

use bevy::ecs::schedule::ScheduleLabel;
use bevy::prelude::*;
use shared::components::{
    AttachedTo, BuildingId, BuildingOf, CharacterActivity, CharacterAffiliation,
    CharacterAttributes, CharacterKind, CharacterName, CivicEmployment, CivicRole, Company,
    CompanyId, CompanyLeadership, CompanyOwnership, EmployedAt, FarmField, Health, Household,
    HouseholdId, HouseholdMember, HouseholdMembers, LivesAt, MootAdministration, Nutrition,
    Occupation, OccupiedByHousehold, OperatedBy, OwnedBy, PersonId, PlayerPosition, PlayerRotation,
    Residence, ResidentOf, Settlement, SettlementBuilding, SettlementBuildingKind, SettlementId,
    SettlementPolicies, SettlementTier, TimeWarp, WorkStatus, WorldTime,
};
use shared::economy::{
    BusinessAccount, BusinessCondition, BusinessManagementPolicy, BusinessProcurementPolicy,
    BusinessSalePolicy, BusinessWagePolicy, CarriedLoad, CivicAccount, CompanyAccount,
    CompanyDecisionHistory, CompanyManagementPolicy, Good, GoodsInventory, HouseholdEconomy,
    MootMarket, PENNIES_PER_COIN, STARTING_TREASURY_MONEY, SettlementEconomy, Wallet,
};
use shared::region::RegionCoord;
use shared::spatial::SpatialObstacleGrid;
use shared::terrain::WorldTerrain;

use super::ambient::{self, AmbientClock, AmbientRoutine, AmbientSpotCache};
use super::collect_business_profit_taxes;
use super::history::{SettlementHistoryRuntime, capture_settlement_history};
use super::{BusinessEventQueue, CompanyDividendQueue, apply_business_events};
use super::{
    FarmerPhase, FarmerRoutine, HomeAssignment, SettlementEconomyRuntime, VillagerIntent,
    advance_nutrition_health, apply_nutrition_condition, assign_farmer_routines, assign_households,
    ensure_farm_fields, fill_vacancies, post_site_capital_to_company, reconcile_work_statuses,
    recount_residents, refresh_company_accounts, review_business_management, review_civic_policies,
    review_company_finance, review_company_strategies, run_business_payroll_and_owner_leisure,
    run_civic_payroll, run_farmer_routines, run_household_schedules, run_workplace_door_transits,
    sync_building_door_demands, sync_carried_load, sync_civic_market_policy,
    sync_public_market_storage, update_household_budgets_and_pantries, update_moot_market_targets,
    update_settlement_economies,
};
use crate::collision::library::StaticColliders;
use crate::player::hero::{MoveTarget, step_units};
use crate::world::pathfinding::PathfindingBudgetSettings;
use crate::world::regions::RegionRegistry;
use crate::world::village_roads::{
    NavigationRoutePending, VillageRoadGraph, plan_villager_travel_routes,
    queue_villager_travel_routes,
};

const DEFAULT_NPCS: usize = 5_000;
const DEFAULT_TOWNS: usize = 30;
const DEFAULT_SAMPLES: usize = 60;
const DEFAULT_MOVING_NPCS: usize = 512;
const WARMUP_RUNS: usize = 5;
const FIXED_BUDGET: Duration = Duration::from_nanos(16_666_667);

#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
struct RecountBench;
#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
struct HousingBench;
#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
struct FieldBench;
#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
struct VacancyBench;
#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
struct RoutineAssignmentBench;
#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
struct PhysicalWorkBench;
#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
struct EconomyIdleBench;
#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
struct EconomyDailyBench;
#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
struct NutritionHealthBench;
#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
struct MovementBench;
#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
struct RoutingBench;
#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
struct VillageSteadyBench;
#[derive(ScheduleLabel, Clone, Debug, PartialEq, Eq, Hash)]
struct IdentitySteadyBench;

fn env_usize(name: &str, fallback: usize) -> usize {
    std::env::var(name)
        .ok()
        .and_then(|raw| raw.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(fallback)
}

fn resident_count_for(town: usize, towns: usize, npcs: usize) -> usize {
    npcs / towns + usize::from(town < npcs % towns)
}

fn spawn_fixture(world: &mut World, towns: usize, npcs: usize) {
    let mut next_person_id = 1_u64;
    let mut next_building_id = 1_u64;
    for town_index in 0..towns {
        let resident_count = resident_count_for(town_index, towns, npcs);
        let place = format!("ScaleTown{town_index:02}");
        let town_x = (town_index % 6) as f32 * 500.0;
        let town_z = (town_index / 6) as f32 * 500.0;
        let hall_position = Vec3::new(town_x, 5.0, town_z);
        let mut hall_inventory = GoodsInventory::new_partitioned(shared::economy::capacity::HALL);
        hall_inventory.add(Good::Food, 600);
        let hall = world
            .spawn((
                SettlementId(town_index as u64 + 1),
                Settlement {
                    name: place.clone(),
                    tier: SettlementTier::Village,
                    residents: resident_count as u32,
                    treasury: STARTING_TREASURY_MONEY,
                },
                SettlementEconomy::default(),
                SettlementPolicies::default(),
                MootAdministration {
                    lead_steward: Some(name_for_porter(town_index, 0)),
                    city_workers: vec![
                        name_for_porter(town_index, 0),
                        name_for_porter(town_index, 1),
                    ],
                    ..default()
                },
                CivicAccount::default(),
                MootMarket::founding(),
                hall_inventory,
                PlayerPosition(hall_position),
                PlayerRotation(0.0),
            ))
            .id();

        let names: Vec<String> = (0..resident_count)
            .map(|resident| format!("T{town_index:02}Resident{resident:03}"))
            .collect();

        let mut houses = Vec::with_capacity(resident_count.div_ceil(4));
        for (house_index, roster) in names.chunks(4).enumerate() {
            let angle = house_index as f32 * 0.61;
            let radius = 18.0 + (house_index % 5) as f32 * 3.0;
            let position =
                hall_position + Vec3::new(angle.cos() * radius, 0.0, angle.sin() * radius);
            let building_id = BuildingId(next_building_id);
            next_building_id += 1;
            let household_id = HouseholdId(building_id.0);
            let resident_ids: Vec<_> = (0..roster.len())
                .map(|offset| PersonId(next_person_id + (house_index * 4 + offset) as u64))
                .collect();
            world.spawn((
                household_id,
                HouseholdMembers {
                    resident_ids: resident_ids.clone(),
                    settlement: SettlementId(town_index as u64 + 1),
                    dwelling: Some(building_id),
                },
                HouseholdEconomy::default(),
            ));
            let house = world
                .spawn((
                    building_id,
                    BuildingOf(SettlementId(town_index as u64 + 1)),
                    SettlementBuilding {
                        kind: SettlementBuildingKind::House,
                        settlement: place.clone(),
                        owner: roster.first().cloned(),
                        quality: 0.5,
                        workers: Vec::new(),
                    },
                    Household {
                        resident_ids,
                        residents: roster.to_vec(),
                    },
                    OccupiedByHousehold(household_id),
                    GoodsInventory::new(shared::economy::capacity::HOUSE),
                    PlayerPosition(position),
                    PlayerRotation(angle),
                ))
                .id();
            houses.push((house, building_id, household_id));
        }

        let farm_workers = names.get(2..).unwrap_or_default();
        let mut farms = Vec::with_capacity(farm_workers.len().div_ceil(2));
        for (farm_index, workers) in farm_workers.chunks(2).enumerate() {
            let row = farm_index / 14;
            let column = farm_index % 14;
            let position = hall_position
                + Vec3::new(column as f32 * 18.0 - 115.0, 0.0, 65.0 + row as f32 * 24.0);
            let building_id = BuildingId(next_building_id);
            next_building_id += 1;
            let owner_id = PersonId(next_person_id + 2 + (farm_index * 2) as u64);
            let company_id = CompanyId(1_000_000 + building_id.0);
            world.spawn((
                company_id,
                Company {
                    name: format!("Scale Company {}", company_id.0),
                    founded_day: 0,
                },
                CompanyOwnership::sole(owner_id),
                CompanyLeadership { master: owner_id },
                CompanyAccount::default(),
                CompanyManagementPolicy::default(),
                CompanyDecisionHistory::default(),
            ));
            let farm = world
                .spawn((
                    building_id,
                    BuildingOf(SettlementId(town_index as u64 + 1)),
                    OwnedBy(owner_id),
                    OperatedBy(company_id),
                    SettlementBuilding {
                        kind: SettlementBuildingKind::Farmstead,
                        settlement: place.clone(),
                        owner: workers.first().cloned(),
                        quality: 0.8,
                        workers: workers.to_vec(),
                    },
                    GoodsInventory::new(shared::economy::capacity::FARMSTEAD),
                    BusinessAccount {
                        unposted_company_capital: 100 * PENNIES_PER_COIN,
                        ..default()
                    },
                    BusinessSalePolicy::default(),
                    BusinessWagePolicy::default(),
                    BusinessProcurementPolicy::default(),
                    BusinessManagementPolicy::default(),
                    BusinessCondition::default(),
                    PlayerPosition(position),
                    PlayerRotation(0.0),
                ))
                .id();
            let fields = [0, 1].map(|plot_index| {
                let field_position = SettlementBuildingKind::Farmstead
                    .field_position_at(position, 0.0, plot_index)
                    .expect("farmstead has two field anchors");
                world
                    .spawn((
                        AttachedTo(building_id),
                        FarmField {
                            shape: None,
                            settlement: place.clone(),
                            farmstead: position,
                            plot_index,
                            layout_version: 0,
                            quality: 0.8,
                        },
                        PlayerPosition(field_position),
                        PlayerRotation(0.0),
                    ))
                    .id()
            });
            farms.push((farm, building_id, fields, position));
        }

        for resident in 0..resident_count {
            let name = names[resident].clone();
            let (home, home_id, household_id) = houses[resident / 4];
            let farmer = resident.checked_sub(2).map(|worker_index| {
                let (farmstead, farm_id, fields, farm_position) = farms[worker_index / 2];
                let field = fields[worker_index % fields.len()];
                (farmstead, farm_id, field, farm_position)
            });
            let position = farmer.map_or(hall_position, |(_, _, _, farm_position)| {
                SettlementBuildingKind::Farmstead.interior_door_position(farm_position, 0.0)
            });
            let mut person = world.spawn((
                CharacterName(name.clone()),
                CharacterKind::Villager,
                CharacterAffiliation::default(),
                Residence(place.clone()),
                VillagerIntent::Resident { settlement: hall },
                HomeAssignment { home },
                GoodsInventory::new(shared::economy::capacity::VILLAGER),
                CarriedLoad::default(),
                Wallet::new(1_000_000),
                Health::default(),
                Nutrition::default(),
                PlayerPosition(position),
                PlayerRotation(0.0),
            ));
            person.insert((
                RegionCoord::from_world_pos(position),
                WorkStatus::Employed,
                CharacterAttributes::from_seed(((town_index as u64) << 32) | resident as u64),
                PersonId(next_person_id),
                ResidentOf(SettlementId(town_index as u64 + 1)),
                LivesAt(home_id),
                HouseholdMember(household_id),
            ));
            if let Some((farmstead, farm_id, field, _)) = farmer {
                person.insert((
                    CharacterActivity::Indoors,
                    Occupation(Some("Farmer".to_string())),
                    FarmerRoutine {
                        farmstead,
                        field,
                        hall,
                        work_stand: position,
                        harvest_seconds: 0.0,
                        failed_workplace_routes: 0,
                        production_day: u32::MAX,
                        produced_today: 0,
                        phase: FarmerPhase::Inside {
                            seconds_left: 1_000_000.0,
                        },
                    },
                    EmployedAt(farm_id),
                ));
            } else {
                person.insert((
                    CharacterActivity::Idle,
                    Occupation(Some("Moot Steward".to_string())),
                    super::MootSteward { settlement: hall },
                    CivicEmployment {
                        settlement: SettlementId(town_index as u64 + 1),
                        role: CivicRole::MootSteward,
                    },
                ));
            }
            next_person_id += 1;
        }
    }

    let mut clock = WorldTime::new_default();
    clock.seconds_in_cycle = clock.ordinary_work_start_seconds() + 60.0;
    world.spawn((clock, TimeWarp::clamped(1.0)));
}

fn name_for_porter(town_index: usize, porter_index: usize) -> String {
    format!("T{town_index:02}Resident{porter_index:03}")
}

fn configure_app(towns: usize, npcs: usize) -> App {
    let mut app = App::new();
    app.init_resource::<Time>();
    app.init_resource::<SettlementEconomyRuntime>();
    app.init_resource::<super::MootQueueClock>();
    app.init_resource::<SettlementHistoryRuntime>();
    app.init_resource::<BusinessEventQueue>();
    app.init_resource::<CompanyDividendQueue>();
    app.init_resource::<crate::world::identity::WorldIdAllocator>();
    app.init_resource::<crate::world::identity::WorldIdentityIndex>();
    app.init_resource::<AmbientClock>();
    app.init_resource::<AmbientSpotCache>();
    app.init_resource::<RegionRegistry>();
    app.init_resource::<SpatialObstacleGrid>();
    app.init_resource::<StaticColliders>();
    app.init_resource::<VillageRoadGraph>();
    app.insert_resource(PathfindingBudgetSettings {
        max_requests_per_tick: 16,
        ..default()
    });
    app.insert_resource(WorldTerrain::default());

    app.add_systems(RecountBench, recount_residents);
    app.add_systems(HousingBench, assign_households);
    app.add_systems(FieldBench, ensure_farm_fields);
    app.add_systems(VacancyBench, fill_vacancies);
    app.add_systems(RoutineAssignmentBench, assign_farmer_routines);
    app.add_systems(
        PhysicalWorkBench,
        (run_farmer_routines, apply_business_events).chain(),
    );
    app.add_systems(
        EconomyIdleBench,
        (
            reconcile_work_statuses,
            run_civic_payroll,
            sync_civic_market_policy,
            sync_public_market_storage,
            update_moot_market_targets,
            post_site_capital_to_company,
            update_household_budgets_and_pantries,
            update_settlement_economies,
            apply_nutrition_condition,
            advance_nutrition_health,
            run_business_payroll_and_owner_leisure,
            collect_business_profit_taxes,
            review_company_strategies,
            review_business_management,
            review_company_finance,
            apply_business_events,
            refresh_company_accounts,
            review_civic_policies,
            capture_settlement_history,
        )
            .chain(),
    );
    app.add_systems(
        EconomyDailyBench,
        (
            reconcile_work_statuses,
            run_civic_payroll,
            sync_civic_market_policy,
            sync_public_market_storage,
            update_moot_market_targets,
            post_site_capital_to_company,
            update_household_budgets_and_pantries,
            update_settlement_economies,
            apply_nutrition_condition,
            advance_nutrition_health,
            run_business_payroll_and_owner_leisure,
            collect_business_profit_taxes,
            review_company_strategies,
            review_business_management,
            review_company_finance,
            apply_business_events,
            refresh_company_accounts,
            review_civic_policies,
            capture_settlement_history,
        )
            .chain(),
    );
    app.add_systems(NutritionHealthBench, advance_nutrition_health);
    app.add_systems(
        IdentitySteadyBench,
        (
            crate::world::identity::assign_stable_world_ids,
            crate::world::identity::rebuild_world_identity_index,
            crate::world::identity::reconcile_stable_world_relationships,
            crate::world::identity::reconcile_stable_adjunct_relationships,
            crate::world::identity::reconcile_stable_civic_employment,
        )
            .chain(),
    );
    app.add_systems(
        VillageSteadyBench,
        (
            (
                recount_residents,
                reconcile_work_statuses,
                run_civic_payroll,
                sync_civic_market_policy,
                sync_public_market_storage,
                update_moot_market_targets,
                post_site_capital_to_company,
                update_household_budgets_and_pantries,
                update_settlement_economies,
                apply_nutrition_condition,
                advance_nutrition_health,
                run_business_payroll_and_owner_leisure,
                collect_business_profit_taxes,
                review_company_strategies,
                review_business_management,
                review_company_finance,
            )
                .chain(),
            fill_vacancies,
            ensure_farm_fields,
            assign_households,
            run_household_schedules,
            run_workplace_door_transits,
            (
                assign_farmer_routines,
                run_farmer_routines,
                apply_business_events,
                refresh_company_accounts,
                ambient::run_ambient_routines,
                (sync_carried_load, sync_building_door_demands).chain(),
            )
                .chain(),
            review_civic_policies,
            capture_settlement_history,
        )
            .chain(),
    );

    spawn_fixture(app.world_mut(), towns, npcs);
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(Duration::from_secs_f32(1.0 / 60.0));
    app
}

#[derive(Debug)]
struct Timing {
    average: Duration,
    p50: Duration,
    p95: Duration,
    p99: Duration,
    max: Duration,
}

fn percentile(sorted: &[Duration], percentile: usize) -> Duration {
    let index = ((sorted.len() - 1) * percentile / 100).min(sorted.len() - 1);
    sorted[index]
}

fn advance_clock(world: &mut World, seconds: f32) {
    world
        .resource_mut::<Time>()
        .advance_by(Duration::from_secs_f32(seconds));
    for mut clock in world.query::<&mut WorldTime>().iter_mut(world) {
        clock.advance(seconds, seconds);
    }
}

fn bench_schedule<L: ScheduleLabel + Clone>(world: &mut World, label: L, samples: usize) -> Timing {
    for _ in 0..WARMUP_RUNS {
        advance_clock(world, 1.0 / 60.0);
        world.run_schedule(label.clone());
    }
    let mut durations = Vec::with_capacity(samples);
    for _ in 0..samples {
        advance_clock(world, 1.0 / 60.0);
        let started = Instant::now();
        world.run_schedule(label.clone());
        durations.push(started.elapsed());
    }
    durations.sort_unstable();
    let total: Duration = durations.iter().copied().sum();
    Timing {
        average: total / samples as u32,
        p50: percentile(&durations, 50),
        p95: percentile(&durations, 95),
        p99: percentile(&durations, 99),
        max: *durations.last().expect("at least one sample"),
    }
}

fn prepare_next_economy_day(world: &mut World) {
    let mut clock = world
        .query::<&mut WorldTime>()
        .single_mut(world)
        .expect("scale fixture has one world clock");
    clock.day = clock.day.saturating_add(1);

    let halls: Vec<Entity> = world
        .query_filtered::<Entity, With<Settlement>>()
        .iter(world)
        .collect();
    for hall in halls {
        if let Some(mut inventory) = world.get_mut::<GoodsInventory>(hall) {
            inventory.add(Good::Food, u32::MAX);
        }
    }
}

fn bench_daily_economy(world: &mut World, samples: usize) -> Timing {
    // Establish the runtime's day-zero bucket before timing day transitions.
    world.run_schedule(EconomyDailyBench);
    let mut durations = Vec::with_capacity(samples);
    for _ in 0..samples {
        prepare_next_economy_day(world);
        let started = Instant::now();
        world.run_schedule(EconomyDailyBench);
        durations.push(started.elapsed());
    }
    durations.sort_unstable();
    let total: Duration = durations.iter().copied().sum();
    Timing {
        average: total / samples as u32,
        p50: percentile(&durations, 50),
        p95: percentile(&durations, 95),
        p99: percentile(&durations, 99),
        max: *durations.last().expect("at least one sample"),
    }
}

fn bench_nutrition_health(world: &mut World, samples: usize) -> Timing {
    let residents: Vec<Entity> = world
        .query_filtered::<Entity, With<CharacterKind>>()
        .iter(world)
        .collect();
    let mut durations = Vec::with_capacity(samples);
    for serial in 0..(WARMUP_RUNS + samples) {
        for resident in &residents {
            let mut entity = world.entity_mut(*resident);
            entity.get_mut::<Health>().expect("fixture Health").current = 50.0;
            entity.insert(super::mortality::NutritionHealthAdjustment::recovering());
        }
        advance_clock(world, 5.0);
        let started = Instant::now();
        world.run_schedule(NutritionHealthBench);
        if serial >= WARMUP_RUNS {
            durations.push(started.elapsed());
        }
    }
    durations.sort_unstable();
    let total: Duration = durations.iter().copied().sum();
    Timing {
        average: total / samples as u32,
        p50: percentile(&durations, 50),
        p95: percentile(&durations, 95),
        p99: percentile(&durations, 99),
        max: *durations.last().expect("at least one sample"),
    }
}

fn milliseconds(duration: Duration) -> f64 {
    duration.as_secs_f64() * 1_000.0
}

fn print_timing(name: &str, timing: &Timing) {
    println!(
        "SCALE system={name:<22} avg={:>8.3}ms p50={:>8.3}ms p95={:>8.3}ms p99={:>8.3}ms max={:>8.3}ms budget={:>6.1}%",
        milliseconds(timing.average),
        milliseconds(timing.p50),
        milliseconds(timing.p95),
        milliseconds(timing.p99),
        milliseconds(timing.max),
        timing.average.as_secs_f64() / FIXED_BUDGET.as_secs_f64() * 100.0,
    );
}

fn resident_memory_mb() -> Option<f64> {
    let output = Command::new("ps")
        .args(["-o", "rss=", "-p", &std::process::id().to_string()])
        .output()
        .ok()?;
    let kib = String::from_utf8(output.stdout)
        .ok()?
        .trim()
        .parse::<f64>()
        .ok()?;
    Some(kib / 1024.0)
}

fn assert_ambient_preserves_committed_work(world: &mut World) -> (usize, usize) {
    let ambient = world.query::<&AmbientRoutine>().iter(world).count();
    let farmers = world.query::<&FarmerRoutine>().iter(world).count();
    let interrupted: Vec<_> = world
        .query_filtered::<(&PersonId, &CharacterName), (
            With<AmbientRoutine>,
            super::worker_activity::AmbientStartBlocked,
        )>()
        .iter(world)
        .map(|(id, name)| (*id, name.0.clone()))
        .collect();
    assert!(
        interrupted.is_empty(),
        "ambient work interrupted committed production/service owners: {interrupted:?}"
    );
    (ambient, farmers)
}

#[test]
fn scale_ambient_probe_allows_idle_people_without_interrupting_work() {
    let mut app = configure_app(1, 12);
    let world = app.world_mut();
    // Explicitly stage one released civic worker; daily-review samples can
    // produce this same legitimate idle state after unpaid wages. The other
    // steward and all ten farmers still own their normal committed work.
    let idle = world
        .query::<(Entity, &PersonId)>()
        .iter(world)
        .find(|(_, id)| id.0 == 1)
        .unwrap()
        .0;
    world
        .entity_mut(idle)
        .remove::<super::MootSteward>()
        .remove::<CivicEmployment>()
        .insert((Occupation(None), WorkStatus::LookingForWork));
    let workers: Vec<_> = world
        .query::<(Entity, &FarmerRoutine)>()
        .iter(world)
        .map(|(entity, routine)| {
            let FarmerPhase::Inside { seconds_left } = routine.phase else {
                panic!("fixture farmer is not waiting inside its workplace");
            };
            (entity, seconds_left)
        })
        .collect();
    app.add_systems(Update, ambient::run_ambient_routines);
    app.update();
    assert!(app.world().get::<AmbientRoutine>(idle).is_some());
    let (ambient, farmers) = assert_ambient_preserves_committed_work(app.world_mut());
    assert_eq!((ambient, farmers), (1, 10));
    for (worker, remaining) in workers {
        assert!(matches!(
            app.world().get::<FarmerRoutine>(worker).unwrap().phase,
            FarmerPhase::Inside { seconds_left } if seconds_left == remaining
        ));
        assert!(app.world().get::<MoveTarget>(worker).is_none());
    }
}

#[test]
#[ignore = "run with `cargo village-scale-lab` so workspace code is release-optimised"]
fn village_scale_lab() {
    let npcs = env_usize("FISTWORLD_SCALE_NPCS", DEFAULT_NPCS);
    let towns = env_usize("FISTWORLD_SCALE_TOWNS", DEFAULT_TOWNS).min(npcs);
    let samples = env_usize("FISTWORLD_SCALE_SAMPLES", DEFAULT_SAMPLES);
    let moving_npcs = env_usize("FISTWORLD_SCALE_MOVING_NPCS", DEFAULT_MOVING_NPCS).min(npcs);
    println!(
        "SCALE scope=synthetic_subsystems not_full_server_tick=true daily_probe=calendar_jump_and_food_refill active_farm_probe=staged_batch_completion"
    );
    active::bench_farm_batches(towns, npcs, samples);
    let mut app = configure_app(towns, npcs);
    let entities_before = app.world().entities().len();
    let archetypes = app.world().archetypes().len();

    println!(
        "SCALE fixture npcs={npcs} towns={towns} entities={entities_before} archetypes={archetypes} samples={samples} rss={:.1}MiB",
        resident_memory_mb().unwrap_or(f64::NAN),
    );

    for (name, timing) in [
        (
            "resident recount",
            bench_schedule(app.world_mut(), RecountBench, samples),
        ),
        (
            "household assignment",
            bench_schedule(app.world_mut(), HousingBench, samples),
        ),
        (
            "field reconciliation",
            bench_schedule(app.world_mut(), FieldBench, samples),
        ),
        (
            "vacancy fill (steady)",
            bench_schedule(app.world_mut(), VacancyBench, samples),
        ),
        (
            "routine assignment",
            bench_schedule(app.world_mut(), RoutineAssignmentBench, samples),
        ),
        (
            "farm routines waiting",
            bench_schedule(app.world_mut(), PhysicalWorkBench, samples),
        ),
        (
            "economy idle tick",
            bench_schedule(app.world_mut(), EconomyIdleBench, samples),
        ),
        (
            "staged daily reviews",
            bench_daily_economy(app.world_mut(), samples),
        ),
        (
            "nutrition active cohort",
            bench_nutrition_health(app.world_mut(), samples),
        ),
        (
            "selected village systems",
            bench_schedule(app.world_mut(), VillageSteadyBench, samples),
        ),
        (
            "identity steady pass",
            bench_schedule(app.world_mut(), IdentitySteadyBench, samples),
        ),
    ] {
        print_timing(name, &timing);
    }

    let entities_after = app.world().entities().len();
    let counted: usize = app
        .world_mut()
        .query::<&Settlement>()
        .iter(app.world())
        .map(|settlement| settlement.residents as usize)
        .sum();
    // The daily probe advances finite public payroll by many calendar days.
    // Former civic staff may legitimately resign and acquire ambient life;
    // committed producer/service routines must never be interrupted by it.
    let (ambient, farmers) = assert_ambient_preserves_committed_work(app.world_mut());
    let person_ids: std::collections::HashSet<PersonId> = app
        .world_mut()
        .query::<&PersonId>()
        .iter(app.world())
        .copied()
        .collect();
    println!(
        "SCALE result residents={counted} entities={entities_after} entity_growth={} ambient_orders={ambient} committed_ambient_overlaps=0 retained_farmers={farmers} rss={:.1}MiB",
        entities_after as isize - entities_before as isize,
        resident_memory_mb().unwrap_or(f64::NAN),
    );

    assert_eq!(counted, npcs, "resident recount drifted at scale");
    assert_eq!(
        person_ids.len(),
        npcs,
        "durable person ids collided at scale"
    );
    assert_eq!(
        entities_after, entities_before,
        "steady ticks leaked entities"
    );
    if npcs > towns * 2 {
        assert!(
            farmers > 0,
            "the work-ownership probe lost its producer cohort"
        );
    }
    drop(app);
    active::bench_movement(npcs, moving_npcs, samples);
}
