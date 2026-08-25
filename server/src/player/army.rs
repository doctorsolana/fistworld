//! Battalions: mustering, membership, and formation movement.
//!
//! A battalion is organization, not magic: soldiers in one are ordinary
//! characters who eat, sleep and die like everyone else, and every battalion
//! order decomposes into the same per-soldier primitives (`MoveTarget`,
//! `AttackOrder`) the rest of the game already polices. What the battalion
//! layer adds is identity (a named, owner-replicated entity), membership
//! (a durable-id tag per soldier), and FORMATION - the server computes
//! rank-and-file arrival slots so a moved battalion arrives as a line with
//! its strongest soldiers in front, Rome-style, instead of a loose crowd.
//!
//! Formation lives here and not on the client on purpose: the client asks
//! "walk these soldiers to that point in formation" and the server decides
//! where each body stands. If the client invented the slots, a refused or
//! dead soldier would desynchronise intent from authority.

use std::collections::{HashMap, HashSet};

use bevy::prelude::*;
use lightyear::prelude::server::ClientOf;
use lightyear::prelude::{MessageReceiver, NetworkTarget, RemoteId, Replicate};

use shared::components::{AboardBoat, CharacterActivity};
use shared::components::{
    Battalion, BattalionId, CharacterAttributes, CharacterKind, CommandedBy, MemberOfBattalion,
    PlayerPosition, MAX_BATTALION_SIZE,
};
use shared::protocol::{ArmyOrder, FormationMoveOrder, MAX_UNITS_PER_ORDER};
use shared::region::RegionCoord;

use crate::player::hero::{MoveTarget, OfflineHero};
use crate::world::village::PlayerConstructionAssignment;
use crate::world::village::{ConstructionMaterialRoutine, UnderConstruction};
use crate::world::village_roads::{NavigationRouteFailed, NavigationRoutePending, TravelRoute};

/// Conscription discharges a villager from village life entirely.
///
/// The village brain keys every decision off `VillagerIntent` (and the
/// routine components a decision leaves behind), and NONE of those systems
/// know about `CommandedBy` - `arrive_at_settlement` alone re-asserts its own
/// `MoveTarget` every tick, which is why a conscript used to obey a player
/// order for exactly one tick before snapping back toward the hall. Removing
/// the intent excludes the soldier from every decision system at once
/// (including strategic-LOD demotion, whose sweep requires the intent);
/// removing the in-flight routines stops the errand they were mid-way
/// through; removing employment stops payroll and staffing from re-hiring
/// them. `tag_villager_intent` is gated on `CommandedBy` so it cannot re-seed
/// what this strips, and dismissal reverses everything simply by removing
/// `CommandedBy` - the backfill then rebuilds an Idle villager on its own.
///
/// Conscripts also leave the road-route planner (`RouteMoverFilter` excludes
/// `CommandedBy`): `step_units` FREEZES a villager holding a pending or
/// failed route, and a soldier ordered across raw battlefield ground must
/// walk like a hero - directly, collision-gated - not stand paralyzed
/// because A* disliked the terrain.
pub fn discharge_from_village_life(entity: &mut bevy::ecs::system::EntityCommands) {
    entity
        .remove::<crate::world::village::VillagerIntent>()
        .remove::<crate::world::village::MigrationCooldown>()
        .remove::<(
            crate::world::village::ConstructionMaterialRoutine,
            crate::world::village::LumberjackRoutine,
            crate::world::village::FarmerRoutine,
            crate::world::village::FishingRoutine,
            crate::world::village::WorkerOffDuty,
            crate::world::village::MootSteward,
            crate::world::village::CompanyPorter,
            crate::world::village::MarketCollectionRoutine,
            crate::world::village::InternalDeliveryRoutine,
            crate::world::village::HouseholdShoppingRoutine,
            crate::world::village::WorkplaceDoorTransit,
            crate::world::village::HomeRoutine,
        )>()
        .remove::<(
            crate::world::village::ambient::AmbientRoutine,
            crate::world::village::moot_services::MootQueueTicket,
            crate::world::village::moot_services::MootQueueTransit,
            crate::world::village::population::ImmigrationDeparture,
            crate::world::village::strategic::StrategicPerson,
            crate::world::village::strategic::StrategicTravel,
            crate::world::village::strategic::PendingStrategicDemotion,
            PlayerConstructionAssignment,
        )>()
        .remove::<(
            MoveTarget,
            TravelRoute,
            NavigationRoutePending,
            NavigationRouteFailed,
            crate::world::village_roads::NavigationRouteBackoff,
        )>()
        .remove::<(
            shared::components::CharacterDayPlan,
            shared::components::WorkStatus,
            shared::components::EmployedAt,
            shared::components::CivicEmployment,
        )>()
        // A one-time write at the moment of conscription: whatever they were
        // doing mid-errand must not stay painted on the body.
        .insert(CharacterActivity::Idle)
        .insert(shared::components::Occupation(Some("Soldier".to_string())));
}

/// Soldiers per rank. Eight reads as a proper line at village scale and keeps
/// even a full 64-soldier battalion to eight ranks deep.
const FORMATION_FILE_WIDTH: usize = 8;
/// Shoulder-to-shoulder gap along a rank, metres.
const FORMATION_FILE_SPACING: f32 = 1.4;
/// Gap between ranks, metres. A touch deeper than the file gap so the block
/// reads as ranks from above.
const FORMATION_RANK_SPACING: f32 = 1.7;

/// How often the battalion entity's replicated centroid is refreshed, real
/// seconds. The centroid exists for the army roster's LOCATE and the map -
/// nothing simulates against it - so a slow cadence is honest and cheap.
const BATTALION_SYNC_SECONDS: f32 = 2.0;

/// Standing battalions one account may keep. Twelve full blocks is 768
/// soldiers - a real army - while still bounding what muster spam can spawn.
const MAX_BATTALIONS_PER_ACCOUNT: usize = 12;

/// Mints battalion ids and remembers how many battalions each account has
/// EVER raised, so ordinal names are never reused: disbanding the 2nd and
/// mustering again yields the 3rd, not a second 2nd.
#[derive(Resource, Default)]
pub struct BattalionLedger {
    next_id: u64,
    raised_by_account: HashMap<String, u64>,
}

impl BattalionLedger {
    fn mint(&mut self, account: &str) -> (BattalionId, u64) {
        self.next_id += 1;
        let raised = self
            .raised_by_account
            .entry(account.to_string())
            .or_insert(0);
        *raised += 1;
        (BattalionId(self.next_id), *raised)
    }
}

/// "1st Battalion", "2nd Battalion" ... with the 11th/12th/13th exceptions.
fn ordinal_name(ordinal: u64) -> String {
    let suffix = match (ordinal % 10, ordinal % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{ordinal}{suffix} Battalion")
}

pub fn handle_army_orders(
    mut commands: Commands,
    profiles: Res<crate::persistence::profiles::PlayerProfiles>,
    mut ledger: ResMut<BattalionLedger>,
    mut client_links: Query<(&RemoteId, &mut MessageReceiver<ArmyOrder>), With<ClientOf>>,
    soldiers: Query<
        (&CommandedBy, &PlayerPosition, Option<&MemberOfBattalion>),
        (
            With<CharacterKind>,
            Without<OfflineHero>,
            Without<AboardBoat>,
        ),
    >,
    battalions: Query<(Entity, &Battalion, &CommandedBy)>,
    members: Query<(Entity, &MemberOfBattalion), With<CharacterKind>>,
) {
    // Membership writes go through Commands and land after this system, so
    // every capacity decision inside one run must ALSO count what this run
    // has already admitted - otherwise several same-tick orders each see the
    // pre-order world and their admissions sum past every cap.
    let mut pending_admissions: HashMap<BattalionId, usize> = HashMap::new();
    let mut pending_musters: HashMap<String, usize> = HashMap::new();
    for (remote_id, mut receiver) in client_links.iter_mut() {
        let account = profiles.peer_to_name.get(&remote_id.0).cloned();
        for order in receiver.receive() {
            // Drain even for an unnamed peer, or the queue backs up forever.
            let Some(account) = account.as_deref() else {
                continue;
            };
            match order {
                ArmyOrder::Muster { members: recruits } => {
                    let recruits = owned_soldiers(&recruits, account, &soldiers);
                    if recruits.is_empty() {
                        continue;
                    }
                    // The ceiling every client message needs: without it a
                    // looping client mints an endless stream of replicated
                    // battalion entities from one reusable soldier.
                    let standing = battalions
                        .iter()
                        .filter(|(_, _, commanded)| commanded.0 == account)
                        .count()
                        + pending_musters.get(account).copied().unwrap_or(0);
                    if standing >= MAX_BATTALIONS_PER_ACCOUNT {
                        continue;
                    }
                    let (id, ordinal) = ledger.mint(account);
                    let centroid = centroid_of(recruits.iter().map(|(_, position, _)| *position));
                    commands.spawn((
                        Battalion {
                            id,
                            name: ordinal_name(ordinal),
                            ordinal,
                        },
                        CommandedBy(account.to_string()),
                        PlayerPosition(centroid),
                        RegionCoord::from_world_pos(centroid),
                        Replicate::to_clients(NetworkTarget::All),
                    ));
                    *pending_musters.entry(account.to_string()).or_insert(0) += 1;
                    pending_admissions.insert(id, recruits.len());
                    // The first recruit raises the standard: stable (the flag
                    // never hops between soldiers on a re-muster) and always
                    // present from the very first replicated frame.
                    for (index, (soldier, _, _)) in recruits.into_iter().enumerate() {
                        let mut soldier_commands = commands.entity(soldier);
                        soldier_commands.insert(MemberOfBattalion(id));
                        if index == 0 {
                            soldier_commands.insert(shared::components::StandardBearer);
                        } else {
                            soldier_commands.remove::<shared::components::StandardBearer>();
                        }
                    }
                }
                ArmyOrder::Assign {
                    battalion,
                    members: recruits,
                } => {
                    let Some(id) = owned_battalion(battalion, account, &battalions) else {
                        continue;
                    };
                    let serving = members.iter().filter(|(_, member)| member.0 == id).count()
                        + pending_admissions.get(&id).copied().unwrap_or(0);
                    let room = MAX_BATTALION_SIZE.saturating_sub(serving);
                    let admitted: Vec<Entity> = owned_soldiers(&recruits, account, &soldiers)
                        .into_iter()
                        // Already serving here: re-inserting an identical
                        // replicated tag would dirty it (a no-op on the wire
                        // costs real bandwidth) and steal room from genuine
                        // recruits in the same order.
                        .filter(|(_, _, member)| *member != Some(id))
                        .take(room)
                        .map(|(soldier, _, _)| soldier)
                        .collect();
                    *pending_admissions.entry(id).or_insert(0) += admitted.len();
                    for soldier in admitted {
                        // A transfer joins as a regular soldier; if they were
                        // their old battalion's bearer, that battalion raises
                        // a successor on the next maintenance pass.
                        commands
                            .entity(soldier)
                            .insert(MemberOfBattalion(id))
                            .remove::<shared::components::StandardBearer>();
                    }
                }
                ArmyOrder::Dismiss { members: released } => {
                    for (soldier, _, member) in owned_soldiers(&released, account, &soldiers) {
                        if member.is_some() {
                            commands
                                .entity(soldier)
                                .remove::<MemberOfBattalion>()
                                .remove::<shared::components::StandardBearer>();
                        }
                    }
                }
                ArmyOrder::Disband { battalion } => {
                    let Some(id) = owned_battalion(battalion, account, &battalions) else {
                        continue;
                    };
                    for (soldier, member) in members.iter() {
                        if member.0 == id {
                            commands
                                .entity(soldier)
                                .remove::<MemberOfBattalion>()
                                .remove::<shared::components::StandardBearer>();
                        }
                    }
                    commands.entity(battalion).despawn();
                }
            }
        }
    }
}

/// The referenced soldiers that actually belong to this account, deduplicated,
/// order preserved, capped. Everything else in the request silently drops -
/// the standard treatment for a stale or hostile order.
fn owned_soldiers(
    requested: &[Entity],
    account: &str,
    soldiers: &Query<
        (&CommandedBy, &PlayerPosition, Option<&MemberOfBattalion>),
        (
            With<CharacterKind>,
            Without<OfflineHero>,
            Without<AboardBoat>,
        ),
    >,
) -> Vec<(Entity, Vec3, Option<BattalionId>)> {
    let mut seen = HashSet::new();
    requested
        .iter()
        .take(MAX_BATTALION_SIZE)
        .filter(|entity| **entity != Entity::PLACEHOLDER && seen.insert(**entity))
        .filter_map(|entity| {
            let (commanded, position, member) = soldiers.get(*entity).ok()?;
            (commanded.0 == account).then_some((*entity, position.0, member.map(|m| m.0)))
        })
        .collect()
}

fn owned_battalion(
    battalion: Entity,
    account: &str,
    battalions: &Query<(Entity, &Battalion, &CommandedBy)>,
) -> Option<BattalionId> {
    if battalion == Entity::PLACEHOLDER {
        return None;
    }
    let (_, identity, commanded) = battalions.get(battalion).ok()?;
    (commanded.0 == account).then_some(identity.id)
}

fn centroid_of(positions: impl ExactSizeIterator<Item = Vec3>) -> Vec3 {
    let count = positions.len().max(1);
    let sum: Vec3 = positions.sum();
    sum / count as f32
}

/// Where each soldier of a `count`-strong block stands, front rank first,
/// each rank centered on the approach line through `target` facing `dir`.
fn formation_slots(count: usize, target: Vec3, dir: Vec2) -> Vec<Vec3> {
    // Perpendicular in the XZ plane; which "handedness" is irrelevant since
    // ranks are symmetric about the approach line.
    let right = Vec2::new(dir.y, -dir.x);
    let mut slots = Vec::with_capacity(count);
    let mut placed = 0;
    let mut rank = 0usize;
    while placed < count {
        let in_rank = (count - placed).min(FORMATION_FILE_WIDTH);
        let depth = rank as f32 * FORMATION_RANK_SPACING;
        for file in 0..in_rank {
            let lateral = (file as f32 - (in_rank as f32 - 1.0) * 0.5) * FORMATION_FILE_SPACING;
            let offset = right * lateral - dir * depth;
            slots.push(Vec3::new(
                target.x + offset.x,
                target.y,
                target.z + offset.y,
            ));
        }
        placed += in_rank;
        rank += 1;
    }
    slots
}

/// Pair soldiers with slots: strongest soldiers fill the front rank (reach is
/// short, so the front rank IS the battalion's fighting strength), and within
/// each rank soldiers keep their current left-to-right order so a formation
/// move never braids paths across the block.
fn assign_slots(
    mut soldiers: Vec<(Entity, Vec3, u8)>,
    target: Vec3,
    dir: Vec2,
) -> Vec<(Entity, Vec3)> {
    let right = Vec2::new(dir.y, -dir.x);
    // Strongest first; entity id breaks ties so the order is stable across
    // re-issues of the same order.
    soldiers.sort_by(|a, b| b.2.cmp(&a.2).then(a.0.cmp(&b.0)));
    let slots = formation_slots(soldiers.len(), target, dir);

    let mut assigned = Vec::with_capacity(soldiers.len());
    let mut cursor = 0;
    while cursor < soldiers.len() {
        let in_rank = (soldiers.len() - cursor).min(FORMATION_FILE_WIDTH);
        let mut rank: Vec<(Entity, Vec3, u8)> = soldiers[cursor..cursor + in_rank].to_vec();
        // Left-to-right by where each soldier stands NOW.
        rank.sort_by(|a, b| {
            let a_side = Vec2::new(a.1.x, a.1.z).dot(right);
            let b_side = Vec2::new(b.1.x, b.1.z).dot(right);
            a_side.total_cmp(&b_side)
        });
        // Slots within a rank are generated left-to-right already.
        for (index, (soldier, _, _)) in rank.into_iter().enumerate() {
            assigned.push((soldier, slots[cursor + index]));
        }
        cursor += in_rank;
    }
    assigned
}

#[allow(clippy::type_complexity)]
pub fn handle_formation_move_orders(
    mut commands: Commands,
    profiles: Res<crate::persistence::profiles::PlayerProfiles>,
    mut client_links: Query<(&RemoteId, &mut MessageReceiver<FormationMoveOrder>), With<ClientOf>>,
    units: Query<
        (
            &CommandedBy,
            &PlayerPosition,
            &CharacterAttributes,
            Option<&PlayerConstructionAssignment>,
        ),
        (
            With<CharacterKind>,
            Without<OfflineHero>,
            Without<AboardBoat>,
        ),
    >,
    mut sites: Query<&mut UnderConstruction>,
) {
    for (remote_id, mut receiver) in client_links.iter_mut() {
        let account = profiles.peer_to_name.get(&remote_id.0).cloned();
        for order in receiver.receive() {
            let Some(account) = account.as_deref() else {
                continue;
            };
            if !order.target.is_finite() {
                continue;
            }
            let mut seen = HashSet::new();
            let mut soldiers: Vec<(Entity, Vec3, u8)> = Vec::new();
            for unit in order.units.iter().take(MAX_UNITS_PER_ORDER) {
                if *unit == Entity::PLACEHOLDER || !seen.insert(*unit) {
                    continue;
                }
                let Ok((commanded, position, attributes, construction)) = units.get(*unit) else {
                    continue;
                };
                if commanded.0 != account {
                    continue;
                }
                // Same release as an ordinary move: a formation order pulls a
                // soldier off the worksite and stands them down from a fight.
                if let Some(construction) = construction {
                    if let Ok(mut site) = sites.get_mut(construction.site) {
                        if site.builder == Some(*unit) {
                            site.builder = None;
                        }
                    }
                    commands
                        .entity(*unit)
                        .remove::<PlayerConstructionAssignment>()
                        .remove::<ConstructionMaterialRoutine>()
                        .remove::<TravelRoute>()
                        .remove::<NavigationRoutePending>()
                        .remove::<NavigationRouteFailed>()
                        .insert(CharacterActivity::Idle);
                }
                soldiers.push((*unit, position.0, attributes.physique()));
            }
            if soldiers.is_empty() {
                continue;
            }
            let centroid = centroid_of(soldiers.iter().map(|(_, position, _)| *position));
            let approach = Vec2::new(order.target.x - centroid.x, order.target.z - centroid.z);
            // Ordered to the spot they already stand on: hold facing rather
            // than collapsing to a degenerate direction.
            let dir = if approach.length_squared() > 1e-4 {
                approach.normalize()
            } else {
                Vec2::new(0.0, 1.0)
            };
            for (soldier, slot) in assign_slots(soldiers, order.target, dir) {
                commands
                    .entity(soldier)
                    .insert(MoveTarget(slot))
                    .remove::<crate::player::combat::AttackOrder>()
                    .remove::<crate::player::combat::MeleeCooldown>();
            }
        }
    }
}

/// Housekeeping at a slow, fixed cadence: refresh each battalion's replicated
/// centroid (for LOCATE and the map), and dissolve battalions whose last
/// soldier died or was dismissed. Positions snap to a half-metre grid so a
/// standing battalion generates zero replication traffic.
pub fn maintain_battalions(
    mut commands: Commands,
    time: Res<Time>,
    mut elapsed: Local<f32>,
    mut condemned: Local<HashSet<BattalionId>>,
    mut battalions: Query<(Entity, &Battalion, &mut PlayerPosition, &mut RegionCoord)>,
    members: Query<
        (
            Entity,
            &MemberOfBattalion,
            &PlayerPosition,
            Has<shared::components::StandardBearer>,
            Option<&CharacterAttributes>,
        ),
        Without<Battalion>,
    >,
) {
    *elapsed += time.delta_secs();
    if *elapsed < BATTALION_SYNC_SECONDS {
        return;
    }
    *elapsed = 0.0;

    struct Muster {
        sum: Vec3,
        count: usize,
        bearers: usize,
        strongest: Option<(Entity, u8)>,
    }
    let mut sums: HashMap<BattalionId, Muster> = HashMap::new();
    for (soldier, member, position, is_bearer, attributes) in members.iter() {
        let entry = sums.entry(member.0).or_insert(Muster {
            sum: Vec3::ZERO,
            count: 0,
            bearers: 0,
            strongest: None,
        });
        entry.sum += position.0;
        entry.count += 1;
        entry.bearers += usize::from(is_bearer);
        let strength = attributes
            .map(|attributes| attributes.physique())
            .unwrap_or(0);
        if entry.strongest.is_none_or(|(_, best)| strength > best) {
            entry.strongest = Some((soldier, strength));
        }
    }
    // A battalion without a standard raises one: the bearer died, was
    // dismissed, or transferred. The strongest soldier picks it up.
    for muster in sums.values() {
        if muster.bearers == 0 {
            if let Some((successor, _)) = muster.strongest {
                commands
                    .entity(successor)
                    .insert(shared::components::StandardBearer);
            }
        }
    }
    for (entity, battalion, mut position, mut region) in battalions.iter_mut() {
        let Some(muster) = sums.get(&battalion.id) else {
            // Every soldier is gone; an empty battalion is a fiction. But it
            // dies only on the SECOND consecutive empty pass: this system has
            // no ordering edge to the order handlers, so a legal executor
            // interleaving can run it after a Muster spawned the battalion
            // and before the queued member tags applied - despawning on
            // first sight would orphan those tags on a battalion that no
            // longer exists.
            if condemned.contains(&battalion.id) {
                commands.entity(entity).despawn();
            } else {
                condemned.insert(battalion.id);
            }
            continue;
        };
        condemned.remove(&battalion.id);
        let centroid = muster.sum / muster.count as f32;
        let snapped = (centroid * 2.0).round() / 2.0;
        if position.0 != snapped {
            position.0 = snapped;
        }
        let next_region = RegionCoord::from_world_pos(snapped);
        if *region != next_region {
            *region = next_region;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ordinal_names_read_like_a_muster_roll() {
        let expect = [
            (1, "1st Battalion"),
            (2, "2nd Battalion"),
            (3, "3rd Battalion"),
            (4, "4th Battalion"),
            (11, "11th Battalion"),
            (12, "12th Battalion"),
            (13, "13th Battalion"),
            (21, "21st Battalion"),
            (22, "22nd Battalion"),
            (103, "103rd Battalion"),
        ];
        for (ordinal, name) in expect {
            assert_eq!(ordinal_name(ordinal), name);
        }
    }

    #[test]
    fn a_full_battalion_forms_ranks_of_eight_with_the_front_rank_on_the_target() {
        let target = Vec3::new(100.0, 3.0, 50.0);
        let dir = Vec2::new(0.0, 1.0); // marching toward +Z
        let slots = formation_slots(10, target, dir);
        assert_eq!(slots.len(), 10);
        // Front rank: eight soldiers AT the target depth, centered.
        for slot in &slots[0..8] {
            assert!(
                (slot.z - target.z).abs() < 1e-4,
                "front rank sits on the line"
            );
        }
        let front_x: Vec<f32> = slots[0..8].iter().map(|slot| slot.x).collect();
        let width = front_x.iter().fold(f32::MIN, |a, b| a.max(*b))
            - front_x.iter().fold(f32::MAX, |a, b| a.min(*b));
        assert!((width - 7.0 * FORMATION_FILE_SPACING).abs() < 1e-3);
        let mean: f32 = front_x.iter().sum::<f32>() / 8.0;
        assert!((mean - target.x).abs() < 1e-3, "the rank is centered");
        // Second rank: two soldiers one rank-space BEHIND the approach.
        for slot in &slots[8..10] {
            assert!((slot.z - (target.z - FORMATION_RANK_SPACING)).abs() < 1e-4);
        }
        // Every slot keeps the click's height so step_units owns the snap.
        assert!(slots.iter().all(|slot| (slot.y - 3.0).abs() < 1e-6));
    }

    #[test]
    fn the_strongest_soldiers_take_the_front_rank() {
        let target = Vec3::new(0.0, 0.0, 100.0);
        let dir = Vec2::new(0.0, 1.0);
        // Twelve soldiers, physique equal to their index: 0..=11.
        let soldiers: Vec<(Entity, Vec3, u8)> = (0..12u32)
            .map(|index| {
                (
                    Entity::from_raw_u32(index + 1).unwrap(),
                    Vec3::new(index as f32, 0.0, 0.0),
                    index as u8,
                )
            })
            .collect();
        let assigned = assign_slots(soldiers, target, dir);
        // The four weakest (physique 0..=3) must be the ones in the rear rank.
        let rear: HashSet<Entity> = assigned
            .iter()
            .filter(|(_, slot)| slot.z < target.z - 1.0)
            .map(|(soldier, _)| *soldier)
            .collect();
        let weakest: HashSet<Entity> = (0..4u32)
            .map(|index| Entity::from_raw_u32(index + 1).unwrap())
            .collect();
        assert_eq!(rear, weakest, "the rear rank shields the weakest");
    }

    #[test]
    fn soldiers_keep_their_left_to_right_order_within_a_rank() {
        let target = Vec3::new(0.0, 0.0, 100.0);
        let dir = Vec2::new(0.0, 1.0); // right = +X... (dir.y, -dir.x) = (1, 0)
                                       // Equal physique, standing in a line across X.
        let soldiers: Vec<(Entity, Vec3, u8)> = (0..4u32)
            .map(|index| {
                (
                    Entity::from_raw_u32(index + 1).unwrap(),
                    Vec3::new(index as f32 * 2.0, 0.0, 0.0),
                    10,
                )
            })
            .collect();
        let assigned = assign_slots(soldiers, target, dir);
        // Soldier order by slot X must match their order by current X:
        // no path crosses another inside the rank.
        let mut by_slot = assigned.clone();
        by_slot.sort_by(|a, b| a.1.x.total_cmp(&b.1.x));
        let expected: Vec<Entity> = (0..4u32)
            .map(|index| Entity::from_raw_u32(index + 1).unwrap())
            .collect();
        let actual: Vec<Entity> = by_slot.iter().map(|(soldier, _)| *soldier).collect();
        assert_eq!(actual, expected);
    }

    /// The conscription contract, end to end: discharge strips the villager
    /// brain, the backfill refuses to re-seed it while CommandedBy stands,
    /// and dismissal restores village life with no special code - the
    /// backfill simply resumes the moment CommandedBy is gone.
    #[test]
    fn a_conscript_leaves_village_life_and_a_dismissal_returns_them() {
        use crate::world::village::VillagerIntent;
        use shared::components::{CharacterName, Occupation, WorkStatus};

        let mut app = App::new();
        app.add_systems(
            Update,
            crate::world::village::population::tag_villager_intent,
        );
        let soldier = app
            .world_mut()
            .spawn((
                CharacterName("Odo".to_string()),
                CharacterKind::Villager,
                PlayerPosition(Vec3::ZERO),
                CommandedBy("wanderer".to_string()),
                VillagerIntent::Idle,
                WorkStatus::LookingForWork,
                MoveTarget(Vec3::new(5.0, 0.0, 5.0)),
            ))
            .id();

        let mut commands = app.world_mut().commands();
        discharge_from_village_life(&mut commands.entity(soldier));
        app.world_mut().flush();

        assert!(app.world().get::<VillagerIntent>(soldier).is_none());
        assert!(app.world().get::<WorkStatus>(soldier).is_none());
        assert!(app.world().get::<MoveTarget>(soldier).is_none());
        assert_eq!(
            app.world()
                .get::<Occupation>(soldier)
                .and_then(|occupation| occupation.0.as_deref()),
            Some("Soldier")
        );

        // The per-tick backfill must NOT hand the body back to the brain.
        app.update();
        assert!(
            app.world().get::<VillagerIntent>(soldier).is_none(),
            "a conscript never regains a village intent"
        );
        assert!(app.world().get::<WorkStatus>(soldier).is_none());

        // Dismissal: removing CommandedBy is the whole ceremony.
        app.world_mut().entity_mut(soldier).remove::<CommandedBy>();
        app.update();
        assert_eq!(
            app.world().get::<VillagerIntent>(soldier).cloned(),
            Some(VillagerIntent::Idle),
            "a dismissed villager rejoins village life on its own"
        );
        assert!(app.world().get::<WorkStatus>(soldier).is_some());
    }

    /// The standard never lies on the ground: when a battalion has soldiers
    /// but no bearer (he died, transferred, or was dismissed), maintenance
    /// hands the flag to the strongest survivor.
    #[test]
    fn a_fallen_standard_passes_to_the_strongest_survivor() {
        use shared::components::StandardBearer;

        let mut app = App::new();
        app.init_resource::<Time>();
        app.add_systems(Update, maintain_battalions);
        app.world_mut().spawn((
            Battalion {
                id: BattalionId(7),
                name: "1st Battalion".to_string(),
                ordinal: 1,
            },
            PlayerPosition(Vec3::ZERO),
            RegionCoord::from_world_pos(Vec3::ZERO),
        ));
        let weak = app
            .world_mut()
            .spawn((
                MemberOfBattalion(BattalionId(7)),
                PlayerPosition(Vec3::ZERO),
                CharacterAttributes::from_seed(1),
            ))
            .id();
        let strong = app
            .world_mut()
            .spawn((
                MemberOfBattalion(BattalionId(7)),
                PlayerPosition(Vec3::ZERO),
                CharacterAttributes::from_seed(2),
            ))
            .id();
        // Make relative strength deterministic regardless of seeds.
        let (weak, strong) = {
            let a = app
                .world()
                .get::<CharacterAttributes>(weak)
                .unwrap()
                .physique();
            let b = app
                .world()
                .get::<CharacterAttributes>(strong)
                .unwrap()
                .physique();
            if a > b {
                (strong, weak)
            } else {
                (weak, strong)
            }
        };

        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs(3));
        app.update();

        assert!(
            app.world().get::<StandardBearer>(strong).is_some(),
            "the strongest survivor raises the standard"
        );
        assert!(app.world().get::<StandardBearer>(weak).is_none());
    }

    #[test]
    fn an_empty_battalion_dissolves_and_a_manned_one_recenters() {
        let mut app = App::new();
        app.init_resource::<Time>();
        app.add_systems(Update, maintain_battalions);

        let manned = app
            .world_mut()
            .spawn((
                Battalion {
                    id: BattalionId(1),
                    name: "1st Battalion".to_string(),
                    ordinal: 1,
                },
                PlayerPosition(Vec3::ZERO),
                RegionCoord::from_world_pos(Vec3::ZERO),
            ))
            .id();
        let empty = app
            .world_mut()
            .spawn((
                Battalion {
                    id: BattalionId(2),
                    name: "2nd Battalion".to_string(),
                    ordinal: 2,
                },
                PlayerPosition(Vec3::ZERO),
                RegionCoord::from_world_pos(Vec3::ZERO),
            ))
            .id();
        app.world_mut().spawn((
            MemberOfBattalion(BattalionId(1)),
            PlayerPosition(Vec3::new(10.0, 0.0, 20.0)),
        ));

        // Force the cadence gate open: advance mocked time far enough.
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs(3));
        app.update();
        // One empty pass is only a condemnation, never a despawn: a battalion
        // mustered this very tick can look empty to an interleaved run of
        // this system while its member tags still sit in the command queue.
        assert!(
            app.world().get_entity(empty).is_ok(),
            "the first empty pass is a grace pass"
        );
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs(3));
        app.update();

        assert!(
            app.world().get_entity(empty).is_err(),
            "an empty battalion must dissolve on the second pass"
        );
        assert_eq!(
            app.world().get::<PlayerPosition>(manned).map(|p| p.0),
            Some(Vec3::new(10.0, 0.0, 20.0)),
            "the battalion centroid follows its soldiers"
        );
    }
}
