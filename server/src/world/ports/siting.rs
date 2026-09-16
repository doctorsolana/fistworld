//! Retained, bounded public harbour nomination and shore/access certification.

use super::{PORT_BUILD_SECONDS, PORT_MATERIALS, construction, funding};
use crate::player::boat::{berth, clearance::WaterNavigationGeometry};
use crate::world::village_roads::{
    IncrementalCorridorResult, IncrementalOverlandCorridorSearch, PlannedRoadAccess,
};
use bevy::prelude::*;
use lightyear::prelude::{NetworkTarget, Replicate};
use shared::{components::*, economy::*, region::RegionCoord, terrain::WorldTerrain};
use std::{collections::BTreeMap, time::Duration};

const BEARINGS: u32 = 48;
const RINGS: u32 = 32;
const MAX_PUBLIC_PROJECTS: usize = 2;

struct ShoreSurvey {
    hall: Entity,
    settlement: SettlementId,
    center: Vec3,
    candidate: u32,
    selected: Option<(PortGeometry, IncrementalOverlandCorridorSearch, bool)>,
}

#[derive(Resource, Default)]
pub(crate) struct PortDevelopment {
    last_review: Option<u32>,
    next_town: usize,
    pending: Option<ShoreSurvey>,
    ready: BTreeMap<SettlementId, PortGeometry>,
    next_funding: f64,
    next_funding_town: usize,
}

pub(super) fn port_still_valid(world: &World, entity: Entity) -> bool {
    let (Some(terrain), Some(geometry), Some(port)) = (
        world.get_resource::<WorldTerrain>(),
        world.get_resource::<WaterNavigationGeometry>(),
        world.get::<SettlementPort>(entity),
    ) else {
        return false;
    };
    berth::port_geometry_valid(terrain, geometry, &port.geometry)
}

/// Cover all three authored rectangles with <=6 m collision strips. Each
/// strip keeps the existing retained prop-cache warmup and exact wall test;
/// endpoints extend to the rectangle edges, so rounded caps cannot miss corners.
pub(super) fn port_footprint_clear(world: &mut World, port: PortGeometry) -> Option<bool> {
    for rect in port.footprints() {
        let rotation = Quat::from_rotation_y(rect.yaw);
        let across = (rotation * Vec3::X).xz();
        let forward = (rotation * Vec3::Z).xz();
        let (axis, normal, long, short) = if rect.half_extents.x >= rect.half_extents.y {
            (across, forward, rect.half_extents.x, rect.half_extents.y)
        } else {
            (forward, across, rect.half_extents.y, rect.half_extents.x)
        };
        let count = (short * 2. / 6.).ceil().max(1.) as usize;
        let width = short * 2. / count as f32;
        for i in 0..count {
            let center = rect.center + normal * (-short + width * (i as f32 + 0.5));
            let points = [center - axis * long, center + axis * long];
            match crate::world::village_roads::regional_bridge_footprint_clear(
                world, &points, width,
            ) {
                None => return None,
                Some(false) => return Some(false),
                Some(true) => {}
            }
        }
    }
    Some(true)
}

fn advance_survey(world: &mut World, survey: &mut ShoreSurvey) -> Option<Option<PortGeometry>> {
    if let Some((port, search, land_ready)) = &mut survey.selected {
        let terrain = world.get_resource::<WorldTerrain>()?;
        if !*land_ready {
            match search.advance(terrain, Duration::from_micros(500)) {
                IncrementalCorridorResult::Pending => return None,
                IncrementalCorridorResult::Unreachable => {
                    survey.selected = None;
                    return None;
                }
                IncrementalCorridorResult::Reachable => {
                    *land_ready = true;
                }
            }
        }
        // This independent retained cache warms at most one prop chunk. The
        // water survey cannot authorize a pier through surviving trees/walls.
        let port = *port;
        match port_footprint_clear(world, port) {
            None => return None,
            Some(false) => {
                survey.selected = None;
                return None;
            }
            Some(true) => return Some(Some(port)),
        }
    }
    if survey.candidate >= BEARINGS * RINGS {
        return Some(None);
    }
    let index = survey.candidate;
    survey.candidate += 1;
    let radius = 16.0 * (1 + index / BEARINGS) as f32;
    let angle = std::f32::consts::TAU * (index % BEARINGS) as f32 / BEARINGS as f32;
    let outward = Vec2::new(angle.cos(), angle.sin());
    let point = survey.center.xz() + outward * radius;
    let (Some(terrain), Some(geometry)) = (
        world.get_resource::<WorldTerrain>(),
        world.get_resource::<WaterNavigationGeometry>(),
    ) else {
        return None;
    };
    if !terrain
        .generator
        .active_map_bounds()
        .contains_xz(point.x, point.y)
    {
        return None;
    }
    let candidate = [ShipKind::Cog, ShipKind::Coaster]
        .into_iter()
        .find_map(|kind| berth::survey_port_berth(terrain, geometry, point, outward, kind));
    if let Some(port) = candidate {
        let start = funding::hall_pickup(world, survey.hall)?;
        survey.selected = Some((
            port,
            IncrementalOverlandCorridorSearch::new(start.xz(), port.shore.xz()),
            false,
        ));
    }
    None
}

fn approve(
    world: &mut World,
    hall: Entity,
    id: SettlementId,
    geometry: PortGeometry,
    clock: &WorldTime,
) -> bool {
    let (Some(town), Some(pickup)) = (
        world.get::<Settlement>(hall),
        funding::hall_pickup(world, hall),
    ) else {
        return false;
    };
    if !SettlementPort::tier_allowed(town.tier) {
        return false;
    }
    let available = crate::world::village::civic::civic_discretionary_budget(
        town,
        world.get::<MootAdministration>(hall),
        world.get::<SettlementPolicies>(hall),
    );
    let wage = funding::daily_wage(world, hall);
    let labour = funding::labour_quote(clock, wage, f64::from(PORT_BUILD_SECONDS));
    let haul_fees: Vec<_> = PORT_MATERIALS
        .iter()
        .map(|(good, units)| {
            crate::world::shipping::quote_haul_fee(
                clock,
                pickup,
                geometry.shore,
                *good,
                *units,
                wage,
            )
        })
        .collect();
    let Some(fees) = haul_fees
        .iter()
        .try_fold(labour, |sum, fee| sum.checked_add(*fee))
    else {
        return false;
    };
    let Some(budget) = available.checked_sub(fees) else {
        return false;
    };
    let Some(quote) = funding::quote_materials(
        world,
        hall,
        &PORT_MATERIALS,
        budget,
        Some(MarketSeller::Treasury(id)),
    ) else {
        return false;
    };
    let Some(total) = quote.cost.checked_add(fees) else {
        return false;
    };
    let material_cost = quote.cost;
    if port_footprint_clear(world, geometry) != Some(true) {
        return false;
    }
    let Some(terrain) = world.get_resource::<WorldTerrain>() else {
        return false;
    };
    let Some(water_geometry) = world.get_resource::<WaterNavigationGeometry>() else {
        return false;
    };
    if !berth::port_geometry_valid(terrain, water_geometry, &geometry) {
        return false;
    }
    world.get_mut::<Settlement>(hall).unwrap().treasury -= total;
    if let Some(mut account) = world.get_mut::<CivicAccount>(hall) {
        account.record_material_expense(clock.day, material_cost);
    }
    let building = world
        .resource_mut::<crate::world::identity::WorldIdAllocator>()
        .building();
    let port = world
        .spawn((
            building,
            BuildingOf(id),
            SettlementPort {
                settlement: id,
                geometry,
                built: false,
            },
            PlayerPosition(geometry.shore),
            PlayerRotation(geometry.pier_yaw()),
            RegionCoord::from_world_pos(geometry.shore),
            PlannedRoadAccess {
                settlement_id: id,
                points: vec![
                    geometry.shore.xz() - geometry.seaward() * 5.4,
                    geometry.pier_end.xz(),
                ],
                // A public harbour reserves its landward office/slab and wide
                // loading head. This is a plot claim, never a solid walking hull.
                half_width: 8.4,
            },
            Replicate::to_clients(NetworkTarget::All),
        ))
        .id();
    construction::start_work(
        world,
        port,
        hall,
        id,
        port,
        PortCargoOwner::Treasury(id),
        PORT_MATERIALS.to_vec(),
        PORT_BUILD_SECONDS,
        labour,
        haul_fees,
        quote,
        clock,
    );
    true
}

pub(crate) fn review_public_ports(world: &mut World) {
    let Some(clock) = funding::clock(world) else {
        return;
    };
    world.init_resource::<PortDevelopment>();
    let mut state = world.remove_resource::<PortDevelopment>().unwrap();
    if let Some(mut survey) = state.pending.take() {
        if world.get::<Settlement>(survey.hall).is_some() {
            match advance_survey(world, &mut survey) {
                None => state.pending = Some(survey),
                Some(Some(geometry)) => {
                    state.ready.insert(survey.settlement, geometry);
                }
                Some(None) => {}
            }
        }
    }
    let now = construction::time(&clock);
    if now >= state.next_funding && !state.ready.is_empty() {
        state.next_funding = now + 30.0;
        let existing: std::collections::HashSet<_> = world
            .query::<&SettlementPort>()
            .iter(world)
            .map(|port| port.settlement)
            .collect();
        state.ready.retain(|id, _| !existing.contains(id));
        let active = world
            .query::<&construction::PortWorkProject>()
            .iter(world)
            .filter(|project| {
                !project.finished && matches!(project.owner, PortCargoOwner::Treasury(_))
            })
            .count();
        if active < MAX_PUBLIC_PROJECTS {
            if let Some((id, geometry)) = state
                .ready
                .iter()
                .nth(state.next_funding_town % state.ready.len().max(1))
                .map(|(id, geometry)| (*id, *geometry))
            {
                state.next_funding_town = state.next_funding_town.wrapping_add(1);
                if let Some(hall) = funding::hall_for(world, id) {
                    if approve(world, hall, id, geometry, &clock) {
                        state.ready.remove(&id);
                    }
                } else {
                    state.ready.remove(&id);
                }
            }
        }
    }
    if state.pending.is_none() && state.last_review != Some(clock.day) && state.ready.len() < 64 {
        state.last_review = Some(clock.day);
        let existing: std::collections::HashSet<_> = world
            .query::<&SettlementPort>()
            .iter(world)
            .map(|port| port.settlement)
            .collect();
        let mut towns: Vec<_> = world
            .query::<(Entity, &SettlementId, &Settlement, &PlayerPosition)>()
            .iter(world)
            .filter(|(_, id, town, _)| {
                SettlementPort::tier_allowed(town.tier)
                    && !existing.contains(id)
                    && !state.ready.contains_key(id)
            })
            .map(|(entity, id, _, position)| (entity, *id, position.0))
            .collect();
        towns.sort_unstable_by_key(|(_, id, _)| *id);
        if !towns.is_empty() {
            let (hall, settlement, center) = towns[state.next_town % towns.len()];
            state.next_town = state.next_town.wrapping_add(1);
            state.pending = Some(ShoreSurvey {
                hall,
                settlement,
                center,
                candidate: 0,
                selected: None,
            });
        }
    }
    world.insert_resource(state);
}
