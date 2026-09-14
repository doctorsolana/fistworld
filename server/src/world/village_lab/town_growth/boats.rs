//! Physical arrival control and observation for the inland growth experiment.
//! This module is test-only. Production owns sailing, landing and admission.

use super::*;
use crate::world::immigration::{self, NaturalImmigrantVoyage, NaturalImmigrationDirector};
use shared::components::{AboardBoat, CharacterObjective, EmployedAt, Nutrition};
use std::collections::BTreeMap;

pub(super) fn configure(app: &mut App) {
    app.insert_resource(NaturalImmigrationDirector::manual_only());
    app.init_resource::<crate::world::dev::VillagerSeed>();
    app.add_systems(Startup, immigration::prepare_natural_immigration_coasts);
    app.add_systems(
        Update,
        (
            immigration::plan_natural_immigration,
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
    ) {
        let people: Vec<_> = world.query::<(Entity, &CharacterName, &CharacterKind, &PlayerPosition,
            Option<&VillagerIntent>, Option<&EmployedAt>, Option<&Nutrition>,
            Option<&CharacterObjective>, Option<&GoodsInventory>, Option<&Wallet>, Option<&MoveTarget>,
            Option<&NavigationRouteFailed>, &shared::components::PersonId)>().iter(world)
            .filter(|(_, _, kind, ..)| **kind == CharacterKind::Villager)
            .map(|(entity, name, _, position, intent, employed, nutrition, objective, inventory, wallet, target, failed, person)| serde_json::json!({
                "id": person, "entity": entity.to_bits().to_string(), "name": name.0, "position": position.0,
                "intent": format!("{intent:?}"), "resident": intent.is_some_and(VillagerIntent::counts_as_resident),
                "employed_at": employed.map(|e| e.0), "nutrition": nutrition,
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
            5 + self.spawned(),
            "the inland lab lost or duplicated a person outside the mortality ledger"
        );
        let businesses: Vec<_> = world.query::<(Entity, &BuildingId, &SettlementBuilding,
            &BusinessAccount, &BusinessCondition, &shared::economy::BusinessStaffingPolicy,
            &BusinessSalePolicy, &BusinessWagePolicy, Option<&village::BusinessOperatingPlan>)>()
            .iter(world).map(|(entity, id, building, account, condition, staffing, sale, wage, plan)| serde_json::json!({
                "id": id, "kind": building.kind, "quality": building.quality,
                "account": account, "condition": condition, "staffing": staffing,
                "sale": sale, "wage": wage,
                "plan": plan.map(|p| serde_json::json!({"day": p.day,
                    "target_output": p.target_output_units, "produced_output": p.produced_output_units,
                    "optimal_positions": p.optimal_positions, "marginal_profit": p.marginal_daily_profit})),
                "inventory": world.get::<GoodsInventory>(entity),
                "company": world.get::<OperatedBy>(entity),
            })).collect();
        let markets: Vec<_> = world
            .query::<(&SettlementId, &MootMarket)>()
            .iter(world)
            .map(|(id, market)| serde_json::json!({"settlement": id, "market": market}))
            .collect();
        let households: Vec<_> = world.query::<(&shared::components::HouseholdId,
            &shared::components::HouseholdMembers, &HouseholdEconomy)>().iter(world)
            .map(|(id, members, economy)| serde_json::json!({"id": id, "members": members, "economy": economy})).collect();
        let output = serde_json::json!({
            "elapsed_world_seconds": elapsed, "arrivals": self.arrivals.values().collect::<Vec<_>>(),
            "people": people, "total_deaths": total_deaths, "deaths": deaths,
            "businesses": businesses, "markets": markets, "households": households,
        });
        std::fs::write(
            directory.join(format!("people-{index:04}.json")),
            serde_json::to_vec(&output).unwrap(),
        )
        .expect("write growth population evidence");
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
