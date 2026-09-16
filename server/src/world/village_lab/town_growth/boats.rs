//! Growth population/economy evidence, plus physical arrival observation for
//! the inland experiment. Test-only; production owns sailing and admission.

use super::*;
use crate::world::immigration::{self, NaturalImmigrantVoyage, NaturalImmigrationDirector};
use shared::components::{AboardBoat, CharacterObjective, EmployedAt, Nutrition};
use std::collections::BTreeMap;

pub(super) fn configure(app: &mut App) {
    app.insert_resource(NaturalImmigrationDirector::manual_only());
    app.init_resource::<crate::world::dev::VillagerSeed>();
    app.init_resource::<crate::player::boat::VesselNavigationQueue>();
    app.init_resource::<crate::player::boat::clearance::WaterNavigationGeometry>();
    app.add_systems(Startup, immigration::prepare_natural_immigration_coasts);
    app.add_systems(
        Update,
        (
            immigration::plan_natural_immigration,
            crate::player::boat::clearance::rebuild_water_navigation_geometry,
            crate::player::boat::plan_vessel_routes,
            crate::player::boat::step_boats,
            immigration::sync_natural_immigrant_passengers,
            immigration::finish_natural_immigrant_voyages,
        )
            .chain()
            .after(village::schedule::VillageSimulationSet::Time)
            .before(village::schedule::VillageSimulationSet::Core),
    );
}

#[derive(Default)]
pub(super) struct Journal {
    arrivals: BTreeMap<Entity, serde_json::Value>,
}

impl Journal {
    pub(super) fn spawned(&self) -> usize {
        self.arrivals.len()
    }

    pub(super) fn observe(&mut self, world: &mut World, elapsed: f32) {
        for (entity, name, position) in world.query_filtered::<(Entity, &CharacterName, &PlayerPosition), With<NaturalImmigrantVoyage>>().iter(world) {
            self.arrivals.entry(entity).or_insert_with(|| serde_json::json!({
                "entity": entity.to_bits().to_string(), "name": name.0,
                "sailing_at": elapsed, "start": position.0,
                "landed_at": null, "admitted_at": null,
            }));
        }
        for (entity, record) in &mut self.arrivals {
            if world.get_entity(*entity).is_err() {
                continue;
            }
            if record["landed_at"].is_null() && world.get::<AboardBoat>(*entity).is_none() {
                record["landed_at"] = elapsed.into();
                record["landing"] =
                    serde_json::json!(world.get::<PlayerPosition>(*entity).map(|p| p.0));
            }
            if record["admitted_at"].is_null()
                && world
                    .get::<VillagerIntent>(*entity)
                    .is_some_and(VillagerIntent::counts_as_resident)
            {
                record["admitted_at"] = elapsed.into();
            }
        }
    }

    pub(super) fn write(
        &self,
        world: &mut World,
        elapsed: f32,
        directory: &std::path::Path,
        index: usize,
        expected_people: usize,
        include_boat_arrivals: bool,
    ) {
        let farm_admission = village::farmer_admission_diagnostics(world);
        let people: Vec<_> = world.query::<(Entity, &CharacterName, &CharacterKind, &PlayerPosition,
            Option<&VillagerIntent>, Option<&EmployedAt>, Option<&Nutrition>,
            Option<&CharacterObjective>, Option<&GoodsInventory>, Option<&Wallet>, Option<&MoveTarget>,
            Option<&NavigationRouteFailed>, &shared::components::PersonId)>().iter(world)
            .filter(|(_, _, kind, ..)| **kind == CharacterKind::Villager)
            .map(|(entity, name, _, position, intent, employed, nutrition, objective, inventory, wallet, target, failed, person)| serde_json::json!({
                "id": person, "entity": entity.to_bits().to_string(), "name": name.0, "position": position.0,
                "intent": format!("{intent:?}"), "resident": intent.is_some_and(VillagerIntent::counts_as_resident),
                "employed_at": employed.map(|e| e.0), "nutrition": nutrition,
                "work_status": world.get::<WorkStatus>(entity),
                "activity": world.get::<CharacterActivity>(entity),
                "simulation": "canonical",
                "employment_release_requested": world.get::<village::worker_activity::EmploymentReleaseRequested>(entity).is_some(),
                "production_routine": world.get::<village::FarmerRoutine>(entity).map(|r| format!("{r:?}"))
                    .or_else(|| world.get::<village::FishingRoutine>(entity).map(|r| format!("{r:?}")))
                    .or_else(|| world.get::<village::LumberjackRoutine>(entity).map(|r| format!("{r:?}")))
                    .or_else(|| world.get::<village::QuarryRoutine>(entity).map(|r| format!("{r:?}")))
                    .or_else(|| world.get::<village::ProcessingRoutine>(entity).map(|r| format!("{r:?}"))),
                "occupation": world.get::<Occupation>(entity),
                "civic_employment": world.get::<shared::components::CivicEmployment>(entity),
                "household": world.get::<shared::components::HouseholdMember>(entity),
                "health": world.get::<Health>(entity),
                "objective": objective, "inventory": inventory, "wallet": wallet,
                "move_target": target.map(|t| t.0), "route_failed": failed.is_some(),
            })).collect();
        let mortality = world.resource::<village::MortalityLedger>();
        let deaths: Vec<_> = mortality
            .iter()
            .map(|d| {
                serde_json::json!({
                    "id": d.id, "name": d.name, "day": d.day, "cause": d.cause,
                })
            })
            .collect();
        let total_deaths = mortality.total_deaths;
        assert_eq!(
            people.len() + total_deaths as usize,
            expected_people,
            "the growth lab lost or duplicated a person outside the mortality ledger"
        );
        let businesses: Vec<_> = world.query::<(Entity, &BuildingId, &SettlementBuilding,
            &BusinessAccount, &BusinessCondition, &shared::economy::BusinessStaffingPolicy,
            &BusinessSalePolicy, &BusinessWagePolicy, Option<&village::BusinessStaffingForecast>)>()
            .iter(world).map(|(entity, id, building, account, condition, staffing, sale, wage, plan)| serde_json::json!({
                "id": id, "kind": building.kind, "quality": building.quality,
                "account": account, "condition": condition, "staffing": staffing,
                "sale": sale, "wage": wage,
                "plan": plan.map(|p| serde_json::json!({"day": p.day,
                    "expected_sales": p.expected_sales_units, "produced_output": p.produced_output_units,
                    "optimal_positions": p.optimal_positions, "marginal_profit": p.marginal_daily_profit})),
                "inventory": world.get::<GoodsInventory>(entity),
                "company": world.get::<OperatedBy>(entity),
                "for_sale": world.get::<shared::economy::BusinessForSale>(entity),
            })).collect();
        let markets: Vec<_> = world
            .query::<(&SettlementId, &MootMarket)>()
            .iter(world)
            .map(|(id, market)| serde_json::json!({"settlement": id, "market": market}))
            .collect();
        let pantry_by_building: HashMap<_, _> = world
            .query::<(&BuildingId, &GoodsInventory)>()
            .iter(world)
            .map(|(id, inventory)| (*id, serde_json::json!(inventory)))
            .collect();
        let public_finance: Vec<_> = world.query::<(&SettlementId, &Settlement,
            Option<&MootAdministration>, Option<&SettlementPolicies>, Option<&CivicAccount>,
            Option<&GoodsInventory>)>().iter(world).map(|(id, settlement, administration, policies, account, stock)| serde_json::json!({
                "settlement": id, "treasury": settlement.treasury, "administration": administration,
                "policies": policies, "account": account, "inventory": stock,
            })).collect();
        let companies: Vec<_> = world
            .query::<(
                &CompanyId,
                &CompanyAccount,
                Option<&CompanyLeadership>,
                Option<&CompanyOwnership>,
            )>()
            .iter(world)
            .map(|(id, account, leadership, ownership)| {
                serde_json::json!({
                    "id": id, "account": account, "leadership": leadership, "ownership": ownership,
                })
            })
            .collect();
        let households: Vec<_> = world
            .query::<(
                &shared::components::HouseholdId,
                &shared::components::HouseholdMembers,
                &HouseholdEconomy,
            )>()
            .iter(world)
            .map(|(id, members, economy)| {
                serde_json::json!({
                    "id": id, "members": members, "economy": economy,
                    "pantry": members.dwelling.and_then(|home| pantry_by_building.get(&home)),
                })
            })
            .collect();
        let mut output = serde_json::json!({
            "elapsed_world_seconds": elapsed, "expected_people": expected_people,
            "people": people, "total_deaths": total_deaths, "deaths": deaths,
            "businesses": businesses, "markets": markets, "households": households,
            "companies": companies, "public_finance": public_finance,
            "farm_admission": farm_admission,
            "abandonments": world.get_resource::<abandonment::Audit>().map(abandonment::Audit::diagnostics),
        });
        if include_boat_arrivals {
            output["arrivals"] = serde_json::json!(self.arrivals.values().collect::<Vec<_>>());
        }
        std::fs::write(
            directory.join(format!("people-{index:04}.json")),
            serde_json::to_vec(&output).unwrap(),
        )
        .expect("write growth population evidence");
        if include_boat_arrivals {
            println!(
                "TOWN boats spawned={} landed={} admitted={} deaths={}",
                self.spawned(),
                self.arrivals
                    .values()
                    .filter(|r| !r["landed_at"].is_null())
                    .count(),
                self.arrivals
                    .values()
                    .filter(|r| !r["admitted_at"].is_null())
                    .count(),
                total_deaths
            );
        }
    }
}

#[test]
fn growth_journals_accept_profile_population_and_only_label_real_boat_arrivals() {
    let directory = std::env::temp_dir().join(format!(
        "fistworld-growth-journal-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    std::fs::create_dir_all(&directory).unwrap();
    for (index, (expected_people, include_boats)) in
        [(32, false), (5, true)].into_iter().enumerate()
    {
        let mut world = World::new();
        world.init_resource::<village::MortalityLedger>();
        for id in 0..expected_people {
            world.spawn((
                shared::components::PersonId(id as u64),
                CharacterName(format!("Founder {id}")),
                CharacterKind::Villager,
                PlayerPosition(Vec3::ZERO),
            ));
        }
        Journal::default().write(
            &mut world,
            0.0,
            &directory,
            index,
            expected_people,
            include_boats,
        );
        let output: serde_json::Value = serde_json::from_slice(
            &std::fs::read(directory.join(format!("people-{index:04}.json"))).unwrap(),
        )
        .unwrap();
        assert_eq!(output["expected_people"], expected_people);
        assert_eq!(output["people"].as_array().unwrap().len(), expected_people);
        assert!(output["businesses"].is_array());
        assert!(output["households"].is_array());
        assert!(output["markets"].is_array());
        assert!(output["companies"].is_array());
        assert!(output["public_finance"].is_array());
        assert!(output["people"][0].get("health").is_some());
        assert!(output["people"][0].get("work_status").is_some());
        assert_eq!(output.get("arrivals").is_some(), include_boats);
    }
    std::fs::remove_dir_all(directory).unwrap();
}
