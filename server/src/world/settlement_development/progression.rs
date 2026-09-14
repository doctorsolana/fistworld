//! Civic development uses dated structural evidence. Wellbeing remains a
//! separate economy reading; qualifying never grants money, materials or work.

use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use shared::components::*;
use shared::economy::{
    Good, GoodsInventory, DEVELOPMENT_REQUIRED_DAYS, TOWN_HALL_STONE_REQUIRED,
    TOWN_MIN_HOUSED_RESIDENTS, TOWN_MIN_OPERATING_BUSINESS_TYPES, TOWN_MIN_RESIDENTS,
    VILLAGE_HALL_WOOD_REQUIRED, VILLAGE_MIN_HOUSED_RESIDENTS, VILLAGE_MIN_OCCUPIED_HOMES,
    VILLAGE_MIN_RESIDENTS,
};

use crate::world::village::development_evidence::SettlementDevelopmentSamples;

#[derive(Default)]
struct RoadSummary {
    dirt: u16,
    stone: u16,
    committed: u32,
    needed: u32,
}

#[derive(Clone, Copy)]
struct ProjectSummary {
    raising: bool,
    staged: u32,
    required: u32,
}

#[derive(Default)]
pub(crate) struct ProgressionRuntime {
    road_stamp: Option<(u32, u32)>,
    roads: HashMap<SettlementId, RoadSummary>,
    projects: HashMap<SettlementId, ProjectSummary>,
}

fn gate(tier: SettlementTier, evidence: &SettlementDevelopmentEvidence) -> SettlementProgressGate {
    match tier {
        SettlementTier::Hamlet => {
            if evidence.residents < VILLAGE_MIN_RESIDENTS {
                SettlementProgressGate::Population
            } else if evidence.housed_residents < VILLAGE_MIN_HOUSED_RESIDENTS {
                SettlementProgressGate::Housing
            } else if evidence.occupied_homes < VILLAGE_MIN_OCCUPIED_HOMES {
                SettlementProgressGate::OccupiedHomes
            } else {
                SettlementProgressGate::Sustaining
            }
        }
        SettlementTier::Village => {
            if evidence.residents < TOWN_MIN_RESIDENTS {
                SettlementProgressGate::Population
            } else if evidence.housed_residents < TOWN_MIN_HOUSED_RESIDENTS {
                SettlementProgressGate::Housing
            } else if !evidence.market_accessible {
                SettlementProgressGate::Marketplace
            } else if evidence.operating_business_types < TOWN_MIN_OPERATING_BUSINESS_TYPES {
                SettlementProgressGate::BusinessActivity
            } else if evidence.paid_trade_pennies == 0 {
                SettlementProgressGate::Trade
            } else {
                SettlementProgressGate::Sustaining
            }
        }
        SettlementTier::Ruins => SettlementProgressGate::Population,
        SettlementTier::Town | SettlementTier::City => SettlementProgressGate::Complete,
    }
}

/// The daily evidence pass runs first. Ordinary ticks read settlement summaries
/// and active Hall projects only; no person or completed-building scans occur.
/// Road presentation refreshes once per simulation second in one global pass.
#[allow(clippy::type_complexity)]
pub fn update_settlement_developments(
    mut commands: Commands,
    clock: Query<&WorldTime>,
    samples: Option<Res<SettlementDevelopmentSamples>>,
    mut settlements: Query<(
        &SettlementId,
        &Settlement,
        &mut SettlementDevelopment,
        &PlayerPosition,
        Option<&PlayerRotation>,
    )>,
    roads: Query<(&VillageRoad, &RoadOf)>,
    hall_projects: Query<(
        &CivicHallUpgradeWorksite,
        &BuildingOf,
        &ConstructionSite,
        &GoodsInventory,
    )>,
    mut runtime: Local<ProgressionRuntime>,
) {
    let Some(clock) = clock.iter().next() else {
        return;
    };
    let day = clock.day;
    let runtime = &mut *runtime;
    let road_stamp = (day, clock.seconds_in_cycle.max(0.0) as u32);
    if runtime.road_stamp != Some(road_stamp) {
        runtime.road_stamp = Some(road_stamp);
        runtime.roads.clear();
        for (road, road_of) in &roads {
            if !road.is_complete() {
                continue;
            }
            let summary = runtime.roads.entry(road_of.0).or_default();
            summary.committed = summary.committed.saturating_add(road.stone_committed);
            match road.surface {
                RoadSurface::Dirt => {
                    summary.dirt = summary.dirt.saturating_add(1);
                    if road.class == RoadClass::Main && summary.needed == 0 {
                        summary.needed = road.stone_required().saturating_sub(road.stone_committed);
                    }
                }
                RoadSurface::Stone => summary.stone = summary.stone.saturating_add(1),
            }
        }
    }
    runtime.projects.clear();
    for (project, building_of, site, inventory) in &hall_projects {
        runtime.projects.insert(
            building_of.0,
            ProjectSummary {
                raising: site.raising,
                staged: inventory.amount(project.material),
                required: project.material_required,
            },
        );
    }

    for (id, settlement, mut development, position, rotation) in &mut settlements {
        let summary = runtime.roads.get(id);
        let (dirt, stone, committed, needed) = summary.map_or((0, 0, 0, 0), |value| {
            (value.dirt, value.stone, value.committed, value.needed)
        });
        let needed = if settlement.tier >= SettlementTier::Town {
            needed
        } else {
            0
        };
        if (
            development.dirt_roads,
            development.stone_roads,
            development.stone_committed,
            development.stone_needed,
        ) != (dirt, stone, committed, needed)
        {
            development.dirt_roads = dirt;
            development.stone_roads = stone;
            development.stone_committed = committed;
            development.stone_needed = needed;
        }

        let sample = samples
            .as_ref()
            .and_then(|samples| samples.by_settlement.get(id));
        let mut current = sample.map_or_else(SettlementDevelopmentEvidence::default, |sample| {
            sample.current
        });
        current.residents = current.residents.min(settlement.residents);
        if development.evidence != current {
            development.evidence = current;
        }

        // Commissioning is a durable commitment. Later hunger, migration or
        // inactivity does not revoke the existing paid physical project.
        if let Some(project) = runtime.projects.get(id) {
            let next_gate = if project.raising {
                SettlementProgressGate::CivicHallConstruction
            } else {
                SettlementProgressGate::CivicHallMaterials
            };
            if development.next_gate != next_gate {
                development.next_gate = next_gate;
            }
            if development.material_staged != project.staged {
                development.material_staged = project.staged;
            }
            if development.material_required != project.required {
                development.material_required = project.required;
            }
            continue;
        }

        let promotable = matches!(
            settlement.tier,
            SettlementTier::Hamlet | SettlementTier::Village
        );
        let next_gate = gate(settlement.tier, &current);
        if development.next_gate != next_gate {
            development.next_gate = next_gate;
        }
        let required = if promotable {
            DEVELOPMENT_REQUIRED_DAYS
        } else {
            0
        };
        if development.required_days != required {
            development.required_days = required;
        }
        if development.material_staged != 0 || development.material_required != 0 {
            development.material_staged = 0;
            development.material_required = 0;
        }
        if !promotable {
            if development.progress_days != 0 || development.qualification_bits != 0 {
                development.reset_qualification(day);
            }
            continue;
        }

        // A sample for some older date must not be relabelled as yesterday.
        // A missing date expires old bits but earns no credit, and evaluation
        // cannot run again midway through the same calendar day.
        if day <= development.last_progress_day {
            continue;
        }
        let completed_day = day - 1;
        let observed = sample
            .and_then(|sample| sample.completed)
            .filter(|(observed_day, _)| *observed_day == completed_day);
        let qualifies = observed.is_some_and(|(_, evidence)| {
            gate(settlement.tier, &evidence) == SettlementProgressGate::Sustaining
        });
        development.record_qualification_day(completed_day, qualifies);
        if observed.is_none()
            || next_gate != SettlementProgressGate::Sustaining
            || development.progress_days < DEVELOPMENT_REQUIRED_DAYS
        {
            continue;
        }

        let (target, material, amount) = if settlement.tier == SettlementTier::Hamlet {
            (
                CivicHallLevel::Village,
                Good::Wood,
                VILLAGE_HALL_WOOD_REQUIRED,
            )
        } else {
            (CivicHallLevel::Town, Good::Stone, TOWN_HALL_STONE_REQUIRED)
        };
        super::spawn_civic_hall_worksite(
            &mut commands,
            *id,
            &settlement.name,
            position.0,
            rotation.map_or(0.0, |rotation| rotation.0),
            target,
            material,
            amount,
            day,
        );
        development.material_required = amount;
        development.next_gate = SettlementProgressGate::CivicHallMaterials;
        info!(
            "Settlement '{}' qualified for {:?} and commissioned {} {:?} of Hall work",
            settlement.name, target, amount, material
        );
    }
}

#[cfg(test)]
#[path = "progression_tests.rs"]
mod tests;
