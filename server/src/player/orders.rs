//! Authoritative tactical command application. All verbs arrive in one ordered
//! receiver and each command commits before the next one is validated.

use bevy::prelude::*;
use lightyear::prelude::server::ClientOf;
use lightyear::prelude::{MessageReceiver, MessageSender, RemoteId};
use shared::components::*;
use shared::formation::{FormationGroup, FormationSoldier};
use shared::protocol::{
    ArmyOrderFeedback, MovementMode, ReliableChannel, UnitCommand, UnitOrder, UnitSelection,
    MAX_UNITS_PER_ORDER,
};
use std::collections::{BTreeMap, HashSet};

use super::combat::{AttackOrder, MeleeCooldown};
use super::hero::{MoveTarget, OfflineHero};
use crate::world::village::{
    ConstructionMaterialRoutine, PlayerConstructionAssignment, UnderConstruction,
};
use crate::world::village_roads::{
    NavigationRouteBackoff, NavigationRouteFailed, NavigationRoutePending, TravelRoute,
};

/// Distinct from MoveTarget: the latter can also be a chase or a route waypoint.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandStance {
    Move,
    AttackMove,
    Retreat,
    Hold,
}

#[derive(Component, Clone, Copy, Debug)]
pub struct MarchOrder {
    pub destination: Vec3,
    pub facing: Vec2,
    pub group: u64,
}

pub(crate) fn take_orders<T: Send + Sync + 'static>(world: &mut World) -> Vec<(Entity, String, T)> {
    let received: Vec<_> = world
        .query_filtered::<(Entity, &RemoteId, &mut MessageReceiver<T>), With<ClientOf>>()
        .iter_mut(world)
        .flat_map(|(link, peer, mut receiver)| {
            receiver
                .receive()
                .map(|order| (link, peer.0, order))
                .collect::<Vec<_>>()
        })
        .collect();
    let profiles = world.resource::<crate::persistence::profiles::PlayerProfiles>();
    received
        .into_iter()
        .filter_map(|(link, peer, order)| {
            profiles
                .peer_to_name
                .get(&peer)
                .map(|account| (link, account.clone(), order))
        })
        .collect()
}

pub(crate) fn feedback(world: &mut World, link: Entity, accepted: usize, message: String) {
    if let Some(mut sender) = world.get_mut::<MessageSender<ArmyOrderFeedback>>(link) {
        sender.send::<ReliableChannel>(ArmyOrderFeedback {
            accepted: accepted as u16,
            message,
        });
    }
}

pub fn handle_unit_orders(world: &mut World) {
    for (link, account, order) in take_orders::<UnitOrder>(world) {
        let (accepted, message) = apply_unit_order(world, &account, order);
        feedback(world, link, accepted, message);
    }
}

pub(crate) fn owned_character(world: &World, entity: Entity, account: &str) -> bool {
    world
        .get::<CommandedBy>(entity)
        .is_some_and(|owner| owner.0 == account)
        && world.get::<CharacterKind>(entity).is_some()
        && world.get::<PlayerPosition>(entity).is_some()
        && world.get::<OfflineHero>(entity).is_none()
        && world.get::<AboardBoat>(entity).is_none()
        && world
            .get::<Health>(entity)
            .is_none_or(|health| !health.is_dead())
}

fn resolve_selection(
    world: &mut World,
    account: &str,
    selection: UnitSelection,
) -> Result<Vec<Entity>, &'static str> {
    if selection.units.len() > MAX_UNITS_PER_ORDER
        || selection.battalions.len() > MAX_BATTALIONS_PER_ACCOUNT
    {
        return Err("Selection exceeds the command limit");
    }
    let requested: HashSet<_> = selection.battalions.into_iter().collect();
    let owned: HashSet<_> = world
        .query::<(&Battalion, &CommandedBy)>()
        .iter(world)
        .filter(|(b, owner)| owner.0 == account && requested.contains(&b.id))
        .map(|(b, _)| b.id)
        .collect();
    let mut units = selection.units;
    if !owned.is_empty() {
        units.extend(
            world
                .query::<(Entity, &MemberOfBattalion)>()
                .iter(world)
                .filter(|(_, member)| owned.contains(&member.0))
                .map(|(e, _)| e),
        );
    }
    let mut seen = HashSet::new();
    units.retain(|entity| *entity != Entity::PLACEHOLDER && seen.insert(*entity));
    if units.len() > MAX_UNITS_PER_ORDER {
        return Err("Selection exceeds the command limit");
    }
    Ok(units)
}

pub fn apply_unit_order(world: &mut World, account: &str, order: UnitOrder) -> (usize, String) {
    let units = match resolve_selection(world, account, order.selection) {
        Ok(units) => units,
        Err(reason) => return (0, reason.into()),
    };
    let requested = units.len();
    if let UnitCommand::Attack { target } = order.command {
        if target == Entity::PLACEHOLDER
            || world.get::<CharacterKind>(target).is_none()
            || world.get::<PlayerPosition>(target).is_none()
            || world.get::<OfflineHero>(target).is_some()
            || world.get::<AboardBoat>(target).is_some()
            || world.get::<Health>(target).is_none_or(|h| h.is_dead())
            || world
                .get::<CommandedBy>(target)
                .is_some_and(|o| o.0 == account)
        {
            return (0, "Attack target unavailable".into());
        }
    }
    if let UnitCommand::Move {
        target, frontage, ..
    } = order.command
    {
        if !target.is_finite()
            || target.abs().max_element() > 1_000_000.0
            || !target.length_squared().is_finite()
            || frontage.is_some_and(|f| {
                !f.facing.is_finite()
                    || !f.facing.length_squared().is_finite()
                    || f.facing.length_squared() < 0.001
                    || !f.width.is_finite()
                    || !(1.0..=2048.0).contains(&f.width)
            })
        {
            return (0, "Invalid destination or formation".into());
        }
    }
    let mut soldiers = Vec::new();
    let mut sailed = 0;
    for entity in units {
        if owned_character(world, entity, account) {
            soldiers.push(entity);
        } else if let UnitCommand::Move { target, .. } = order.command {
            if world.get::<Vessel>(entity).is_some()
                && world.get::<WreckedVessel>(entity).is_none()
                && world
                    .get::<CommandedBy>(entity)
                    .is_some_and(|o| o.0 == account)
            {
                world
                    .entity_mut(entity)
                    .remove::<(super::boat::VesselRoute, super::boat::PendingLanding)>();
                world
                    .resource_mut::<super::boat::VesselNavigationQueue>()
                    .request(entity, super::boat::VesselGoal::Sail(target.xz()));
                sailed += 1;
            }
        }
    }
    let accepted = soldiers.len() + sailed;
    if accepted == 0 {
        return (0, "No available units in that selection".into());
    }
    let mut grouped = BTreeMap::<u64, Vec<FormationSoldier>>::new();
    let mut loose = 0usize;
    for entity in &soldiers {
        let key = world
            .get::<MemberOfBattalion>(*entity)
            .map(|m| m.0 .0)
            .unwrap_or_else(|| {
                let key = u64::MAX - (loose / MAX_BATTALION_SIZE) as u64;
                loose += 1;
                key
            });
        grouped.entry(key).or_default().push(FormationSoldier {
            entity: *entity,
            identity: world
                .get::<PersonId>(*entity)
                .map_or(entity.to_bits(), |id| id.0),
            position: world.get::<PlayerPosition>(*entity).unwrap().0,
            strength: world
                .get::<CharacterAttributes>(*entity)
                .map_or(0, |a| a.physique()),
        });
    }
    let groups = grouped.len();
    let blocks = if let UnitCommand::Move {
        target, frontage, ..
    } = order.command
    {
        let blocks = shared::formation::layout(
            grouped
                .into_iter()
                .map(|(key, soldiers)| FormationGroup { key, soldiers })
                .collect(),
            target,
            frontage,
        );
        if blocks
            .iter()
            .flat_map(|b| &b.slots)
            .any(|(_, point)| !destination_allowed(world, *point))
        {
            return (0, "Formation overlaps water, an obstacle or the map edge; choose clear ground or a narrower frontage".into());
        }
        blocks
    } else {
        Vec::new()
    };
    for entity in &soldiers {
        interrupt_previous_order(world, *entity);
    }
    match order.command {
        UnitCommand::Move { mode, .. } => {
            world.init_resource::<navigation::FormationRoutes>();
            for block in blocks {
                let mut points: Vec<_> = block.slots.iter().map(|(_, p)| p.xz()).collect();
                points.extend(
                    block
                        .slots
                        .iter()
                        .filter_map(|(e, _)| world.get::<PlayerPosition>(*e).map(|p| p.0.xz())),
                );
                let goal = block
                    .slots
                    .iter()
                    .min_by(|(_, a), (_, b)| {
                        a.distance_squared(block.centre)
                            .total_cmp(&b.distance_squared(block.centre))
                    })
                    .unwrap()
                    .1
                    .xz();
                let group = world
                    .resource_mut::<navigation::FormationRoutes>()
                    .register(points, goal);
                for (entity, destination) in block.slots {
                    world.entity_mut(entity).insert((
                        MarchOrder {
                            destination,
                            facing: block.facing,
                            group,
                        },
                        MoveTarget(destination),
                        match mode {
                            MovementMode::Move => CommandStance::Move,
                            MovementMode::AttackMove => CommandStance::AttackMove,
                            MovementMode::Retreat => CommandStance::Retreat,
                        },
                    ));
                }
            }
        }
        UnitCommand::Attack { target } => {
            let person = world.get::<PersonId>(target).copied();
            for entity in soldiers {
                let mut soldier = world.entity_mut(entity);
                soldier
                    .remove::<CommandStance>()
                    .insert(AttackOrder { target });
                if let Some(person) = person {
                    soldier.insert(EngagedWith(person));
                }
            }
        }
        UnitCommand::Hold => {
            for entity in soldiers {
                world.entity_mut(entity).insert(CommandStance::Hold);
            }
        }
    }
    let verb = match order.command {
        UnitCommand::Move {
            mode: MovementMode::Retreat,
            ..
        } => "Retreating",
        UnitCommand::Move {
            mode: MovementMode::AttackMove,
            ..
        } => "Advancing",
        UnitCommand::Move { .. } => "Moving",
        UnitCommand::Attack { .. } => "Attacking with",
        UnitCommand::Hold => "Holding with",
    };
    let omitted = requested.saturating_sub(accepted);
    let suffix = if omitted > 0 {
        format!("; {omitted} unavailable")
    } else {
        String::new()
    };
    (
        accepted,
        format!("{verb} {accepted} units in {groups} groups{suffix}"),
    )
}

/// Used by every tactical verb so changing an order cannot reset a weapon
/// cooldown, leave a worksite claimed, or retain a stale navigation failure.
fn interrupt_previous_order(world: &mut World, entity: Entity) {
    if let Some(assignment) = world.get::<PlayerConstructionAssignment>(entity).copied() {
        if let Some(mut site) = world.get_mut::<UnderConstruction>(assignment.site) {
            if site.builder == Some(entity) {
                site.builder = None;
            }
        }
    }
    let mut unit = world.entity_mut(entity);
    if let Some(mut cooldown) = unit.get_mut::<MeleeCooldown>() {
        cooldown.disengage();
    }
    unit.remove::<(
        AttackOrder,
        EngagedWith,
        MarchOrder,
        MoveTarget,
        PlayerConstructionAssignment,
        ConstructionMaterialRoutine,
        TravelRoute,
        NavigationRoutePending,
        NavigationRouteFailed,
        NavigationRouteBackoff,
    )>();
    if let Some(mut motion) = unit.get_mut::<CharacterMotion>() {
        motion.set_if_neq(CharacterMotion::STATIONARY);
    }
    if let Some(mut activity) = unit.get_mut::<CharacterActivity>() {
        activity.set_if_neq(CharacterActivity::Idle);
    }
}

fn destination_allowed(world: &World, point: Vec3) -> bool {
    if let Some(terrain) = world.get_resource::<shared::terrain::WorldTerrain>() {
        let bounds = terrain.generator.active_map_bounds();
        let xz = point.xz();
        if xz.cmplt(Vec2::from_array(bounds.min)).any()
            || xz.cmpgt(Vec2::from_array(bounds.max)).any()
            || terrain.get_water_height(point.x, point.z).is_some()
        {
            return false;
        }
    }
    super::hero::navigation_segment_clear(
        point.xz(),
        point.xz(),
        world.get_resource::<shared::spatial::SpatialObstacleGrid>(),
        world.get_resource::<crate::collision::library::StaticColliders>(),
        world.get_resource::<crate::collision::library::DerivedColliderLibrary>(),
    )
}

mod flow;
mod navigation;
pub use navigation::advance_marches;

#[cfg(test)]
mod tests;
