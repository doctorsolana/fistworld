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

use super::combat::{fronts, AttackOrder, MeleeCooldown};
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
    /// Arrived / maintaining formation, without an explicit hold order.
    Guard,
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
        if std::env::var_os("FISTWORLD_ARMY_SCENARIO").is_some() {
            info!(
                "Army lab received {:?} for {} battalions and {} individuals",
                order.command,
                order.selection.battalions.len(),
                order.selection.units.len()
            );
        }
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
    let mut requested: HashSet<_> = selection.battalions.into_iter().collect();
    // Partial/old/malicious clients cannot peel members out with tactical orders.
    // Membership changes are a separate, explicitly authorized ArmyOrder.
    for &entity in &selection.units {
        if owned_character(world, entity, account) {
            if let Some(member) = world.get::<MemberOfBattalion>(entity) {
                requested.insert(member.0);
            }
        }
    }
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
    apply_local_unit_order(world, account, units, order.command)
}

/// Internal response to a local hazard may move only nearby members. Player
/// orders must enter apply_unit_order so selection expands before this boundary.
/// Ownership and target validation still apply here.
pub(super) fn apply_local_unit_order(
    world: &mut World,
    account: &str,
    units: Vec<Entity>,
    command: UnitCommand,
) -> (usize, String) {
    let order = UnitOrder {
        selection: UnitSelection::default(),
        command,
    };
    let requested = units.len();
    if let UnitCommand::Attack { target, .. } = order.command {
        if target == Entity::PLACEHOLDER
            || (world.get::<CharacterKind>(target).is_none()
                && world.get::<Catapult>(target).is_none())
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
    let mut siege_units = Vec::new();
    for entity in units {
        if world.get::<Catapult>(entity).is_some() {
            siege_units.push(entity);
        } else if matches!(order.command, UnitCommand::AttackGround { .. }) {
            continue;
        } else if owned_character(world, entity, account) {
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
    if soldiers.is_empty() && sailed == 0 && siege_units.is_empty() {
        return (0, "No available units in that selection".into());
    }
    if sailed == 0 && siege_units.is_empty() {
        if let UnitCommand::Move {
            target,
            frontage: None,
            mode: MovementMode::Move,
        } = order.command
        {
            if let Some(result) = super::swimming::try_order(world, &soldiers, target) {
                return result;
            }
        }
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
            seat: world.get::<FormationSeat>(*entity).map(|s| s.0),
            identity: world
                .get::<PersonId>(*entity)
                .map_or(entity.to_bits(), |id| id.0),
            position: world.get::<PlayerPosition>(*entity).unwrap().0,
        });
    }
    let mut full_sizes = BTreeMap::<u64, usize>::new();
    for (member, owner, health) in world
        .query::<(&MemberOfBattalion, &CommandedBy, Option<&Health>)>()
        .iter(world)
    {
        if owner.0 == account && health.is_none_or(|h| !h.is_dead()) {
            *full_sizes.entry(member.0 .0).or_default() += 1;
        }
    }
    let cohesive: HashSet<_> = grouped
        .iter()
        .filter(|(key, units)| full_sizes.get(key) == Some(&units.len()))
        .map(|(key, _)| *key)
        .collect();
    let shapes: BTreeMap<_, _> = world
        .query::<(Entity, &Battalion, Option<&BattalionFormation>)>()
        .iter(world)
        .map(|(e, b, shape)| (b.id.0, (e, shape.copied().unwrap_or_default())))
        .collect();
    let groups = grouped.len();
    let current_blocks: Vec<_> = if !matches!(order.command, UnitCommand::Move { .. }) {
        grouped
            .iter()
            .flat_map(|(key, soldiers)| {
                if soldiers.len() >= 2 && cohesive.contains(key) {
                    vec![fronts::current_block(
                        world,
                        *key,
                        soldiers,
                        shapes.get(key).map_or(default(), |(_, s)| *s),
                    )]
                } else {
                    // Detached archers keep individual objectives and reuse the
                    // same ranged formation approach with a one-person file.
                    soldiers
                        .iter()
                        .filter(|s| world.get::<BowEquipped>(s.entity).is_some())
                        .map(|s| {
                            fronts::current_block(
                                world,
                                s.entity.to_bits(),
                                std::slice::from_ref(s),
                                default(),
                            )
                        })
                        .collect()
                }
            })
            .collect()
    } else {
        Vec::new()
    };
    let blocks = if let UnitCommand::Move {
        target, frontage, ..
    } = order.command
    {
        let blocks = shared::formation::layout(
            grouped
                .into_iter()
                .map(|(key, soldiers)| FormationGroup {
                    key,
                    shape: shapes.get(&key).map_or(default(), |(_, s)| *s),
                    soldiers,
                })
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
    // Commit siege orders only after the mixed selection's formation has
    // passed validation. A refused frontage cannot move half the selection.
    let siege_destinations = if let UnitCommand::Move {
        target, frontage, ..
    } = order.command
    {
        let average = siege_units
            .iter()
            .filter_map(|e| world.get::<PlayerPosition>(*e))
            .map(|p| p.0.xz())
            .sum::<Vec2>()
            / siege_units.len().max(1) as f32;
        let forward = frontage
            .map(|f| f.facing)
            .unwrap_or(target.xz() - average)
            .normalize_or_zero();
        let forward = if forward == Vec2::ZERO {
            Vec2::Y
        } else {
            forward
        };
        let right = Vec2::new(forward.y, -forward.x);
        let behind = blocks
            .iter()
            .flat_map(|b| &b.slots)
            .map(|(_, p)| (target.xz() - p.xz()).dot(forward))
            .fold(0.0_f32, f32::max);
        (0..siege_units.len())
            .map(|index| {
                let lateral = (index as f32 - (siege_units.len().saturating_sub(1)) as f32 * 0.5)
                    * (CATAPULT_CLEARANCE * 2.0 + 0.5);
                let offset = right * lateral
                    - forward
                        * if soldiers.is_empty() {
                            0.0
                        } else {
                            behind + 6.0
                        };
                target + Vec3::new(offset.x, 0.0, offset.y)
            })
            .collect::<Vec<_>>()
    } else {
        Vec::new()
    };
    let mut siege = 0;
    let mut siege_error = None;
    for (index, entity) in siege_units.into_iter().enumerate() {
        let command = match order.command {
            UnitCommand::Move { frontage, mode, .. } => UnitCommand::Move {
                target: siege_destinations[index],
                frontage,
                mode,
            },
            other => other,
        };
        match super::siege::order_catapult(world, account, entity, command) {
            Ok(()) => siege += 1,
            Err(reason) => siege_error = Some(reason),
        }
    }
    let accepted = soldiers.len() + sailed + siege;
    if accepted == 0 {
        return (
            0,
            siege_error
                .unwrap_or("No available units in that selection")
                .into(),
        );
    }
    for entity in &soldiers {
        interrupt_previous_order(world, *entity);
    }
    match order.command {
        UnitCommand::Move { mode, .. } => {
            world.init_resource::<navigation::FormationRoutes>();
            for block in blocks {
                if let Some(&(entity, old)) = shapes
                    .get(&block.key)
                    .filter(|_| cohesive.contains(&block.key))
                {
                    let shape = if matches!(
                        order.command,
                        UnitCommand::Move {
                            frontage: Some(_),
                            ..
                        }
                    ) {
                        BattalionFormation {
                            files: block.files as u8,
                            spacing: block.spacing,
                        }
                    } else {
                        old
                    };
                    if old != shape || world.get::<BattalionFormation>(entity).is_none() {
                        world.entity_mut(entity).insert(shape);
                    }
                }
                if cohesive.contains(&block.key) {
                    fronts::install(
                        world,
                        block.clone(),
                        fronts::Intent::March {
                            engage: mode == MovementMode::AttackMove,
                        },
                    );
                }
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
                    let seat = FormationSeat(destination.xz() - block.centre.xz());
                    if world.get::<FormationSeat>(entity) != Some(&seat) {
                        world.entity_mut(entity).insert(seat);
                    }
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
        UnitCommand::Attack { target, mode } => {
            let enemy = fronts::enemy_of(world, target);
            // Uncommanded civilians are deliberately absent from the automatic
            // combat index. Explicit attacks on them keep ordinary pursuit.
            let indexed_target = world.get::<Catapult>(target).is_none()
                && (world.get::<CommandedBy>(target).is_some()
                    || world.get::<super::combat::WarParty>(target).is_some());
            let targets = attack::distribute_targets(world, &current_blocks, target, mode);
            for (block, objective) in current_blocks.into_iter().zip(targets) {
                if indexed_target
                    || block
                        .slots
                        .iter()
                        .any(|(e, _)| world.get::<BowEquipped>(*e).is_some())
                {
                    fronts::install(world, block, fronts::Intent::Attack(objective));
                }
            }
            // A fresh attack is an explicit fire command. A later Hold fire
            // policy can stop it immediately without erasing movement intent.
            let archer_battalions: HashSet<_> = soldiers
                .iter()
                .filter(|e| world.get::<SoldierRole>(**e) == Some(&SoldierRole::Archer))
                .filter_map(|e| world.get::<MemberOfBattalion>(*e).map(|m| m.0))
                .collect();
            let policies: Vec<_> = world
                .query::<(Entity, &Battalion)>()
                .iter(world)
                .filter(|(_, b)| archer_battalions.contains(&b.id))
                .map(|(e, _)| e)
                .collect();
            for entity in policies {
                world.entity_mut(entity).insert(FirePolicy::FireAtWill);
            }
            for entity in soldiers {
                if world.get::<SoldierRole>(entity) == Some(&SoldierRole::Archer) {
                    world.entity_mut(entity).insert(FirePolicy::FireAtWill);
                }
                world.entity_mut(entity).remove::<CommandStance>();
                if world.get::<fronts::FormationMember>(entity).is_some() {
                    continue;
                }
                let machine = world.get::<Catapult>(target).is_some();
                let mut soldier = world.entity_mut(entity);
                soldier
                    .remove::<CommandStance>()
                    .insert((AttackOrder { target }, super::army::DirectedAttack));
                if !machine {
                    soldier.insert(super::combat::SkirmishOrder { target: enemy });
                }
            }
        }
        UnitCommand::AttackGround { .. } => {}
        UnitCommand::Hold => {
            for block in current_blocks {
                fronts::install(world, block, fronts::Intent::Hold);
            }
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
        UnitCommand::AttackGround { .. } => "Bombarding with",
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
pub(super) fn interrupt_previous_order(world: &mut World, entity: Entity) {
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
    if let Some(mut state) = unit.get_mut::<super::archery::ArcherState>() {
        state.cancel();
    }
    unit.remove::<BowShot>();
    unit.remove::<(
        super::army::DirectedAttack,
        super::army::EvadingBombardment,
        fronts::FormationMember,
        fronts::PausedFormationMarch,
        super::combat::SkirmishOrder,
        super::combat::DirectCombatApproach,
        CombatReady,
        CombatSwing,
    )>();
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

mod attack;
mod flow;
mod navigation;
pub use navigation::{advance_marches, FormationRoutes};

#[cfg(test)]
mod tests;
