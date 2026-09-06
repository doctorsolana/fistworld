//! Membership edits commit in receive order, including within a single tick.
//! Validation always sees the result of preceding assignments and disbands.

use super::{ordinal_name, BattalionLedger};
use crate::player::orders::{feedback, owned_character, take_orders};
use bevy::prelude::*;
use lightyear::prelude::{NetworkTarget, Replicate};
use shared::components::*;
use shared::protocol::ArmyOrder;
use shared::region::RegionCoord;
use std::collections::HashSet;

pub fn handle_army_orders(world: &mut World) {
    for (link, account, order) in take_orders::<ArmyOrder>(world) {
        let (count, message) = apply_army_order(world, &account, order);
        feedback(world, link, count, message);
    }
}

fn owned_battalion(world: &World, account: &str, entity: Entity) -> Option<BattalionId> {
    (entity != Entity::PLACEHOLDER
        && world
            .get::<CommandedBy>(entity)
            .is_some_and(|o| o.0 == account))
    .then(|| world.get::<Battalion>(entity).map(|b| b.id))
    .flatten()
}

fn recruits(
    world: &World,
    account: &str,
    requested: Vec<Entity>,
) -> Result<Vec<Entity>, &'static str> {
    if requested.len() > MAX_BATTALION_SIZE {
        return Err("A battalion holds at most 64 soldiers");
    }
    let mut seen = HashSet::new();
    Ok(requested
        .into_iter()
        .filter(|e| seen.insert(*e) && owned_character(world, *e, account))
        .collect())
}

fn release(world: &mut World, soldier: Entity) {
    world.entity_mut(soldier).remove::<(
        MemberOfBattalion,
        StandardBearer,
        crate::player::combat::fronts::FormationMember,
        crate::player::combat::fronts::PausedFormationMarch,
        CombatReady,
        BattalionStance,
    )>();
}

pub fn apply_army_order(world: &mut World, account: &str, order: ArmyOrder) -> (usize, String) {
    let unavailable = || (0, "Battalion or soldier unavailable".to_string());
    match order {
        ArmyOrder::Muster { members } => {
            let soldiers = match recruits(world, account, members) {
                Ok(s) => s,
                Err(e) => return (0, e.into()),
            };
            let standing = world
                .query::<(&Battalion, &CommandedBy)>()
                .iter(world)
                .filter(|(_, o)| o.0 == account)
                .count();
            if standing >= MAX_BATTALIONS_PER_ACCOUNT {
                return (0, "You already have 12 battalions".into());
            }
            let (id, ordinal) = world.resource_mut::<BattalionLedger>().mint(account);
            let position = shared::formation::centre(
                soldiers
                    .iter()
                    .filter_map(|e| world.get::<PlayerPosition>(*e).map(|p| p.0)),
            );
            world.spawn((
                Battalion {
                    id,
                    name: ordinal_name(ordinal),
                    ordinal,
                },
                CommandedBy(account.into()),
                BattalionStance::default(),
                PlayerPosition(position),
                RegionCoord::from_world_pos(position),
                Replicate::to_clients(NetworkTarget::All),
            ));
            for (i, soldier) in soldiers.iter().enumerate() {
                let mut entity = world.entity_mut(*soldier);
                entity.remove::<(
                    crate::player::combat::fronts::FormationMember,
                    crate::player::combat::fronts::PausedFormationMarch,
                    CombatReady,
                )>();
                entity.insert(MemberOfBattalion(id));
                if i == 0 {
                    entity.insert(StandardBearer);
                } else {
                    entity.remove::<StandardBearer>();
                }
                super::response::apply_policy(world, *soldier, BattalionStance::default());
            }
            (
                soldiers.len(),
                format!(
                    "Raised {} with {} soldiers",
                    ordinal_name(ordinal),
                    soldiers.len()
                ),
            )
        }
        ArmyOrder::Assign { battalion, members } => {
            let Some(id) = owned_battalion(world, account, battalion) else {
                return unavailable();
            };
            let soldiers = match recruits(world, account, members) {
                Ok(s) => s,
                Err(e) => return (0, e.into()),
            };
            let serving = world
                .query::<&MemberOfBattalion>()
                .iter(world)
                .filter(|m| m.0 == id)
                .count();
            let mut room = MAX_BATTALION_SIZE.saturating_sub(serving);
            let stance = world
                .get::<BattalionStance>(battalion)
                .copied()
                .unwrap_or_default();
            let mut accepted = 0;
            let requested = soldiers.len();
            for soldier in soldiers {
                if world
                    .get::<MemberOfBattalion>(soldier)
                    .is_some_and(|m| m.0 == id)
                {
                    accepted += 1;
                    continue;
                }
                if room == 0 {
                    continue;
                }
                world
                    .entity_mut(soldier)
                    .remove::<(
                        crate::player::combat::fronts::FormationMember,
                        crate::player::combat::fronts::PausedFormationMarch,
                        CombatReady,
                    )>()
                    .insert(MemberOfBattalion(id))
                    .remove::<StandardBearer>();
                super::response::apply_policy(world, soldier, stance);
                room -= 1;
                accepted += 1;
            }
            (
                accepted,
                if accepted < requested {
                    format!("Battalion full; assigned {accepted} of {requested}")
                } else {
                    format!("Assigned {accepted} soldiers")
                },
            )
        }
        ArmyOrder::Dismiss { members } => {
            let soldiers = match recruits(world, account, members) {
                Ok(s) => s,
                Err(e) => return (0, e.into()),
            };
            for soldier in &soldiers {
                release(world, *soldier);
            }
            (
                soldiers.len(),
                format!("Removed {} soldiers from their battalions", soldiers.len()),
            )
        }
        ArmyOrder::Disband { battalion } => {
            let Some(id) = owned_battalion(world, account, battalion) else {
                return unavailable();
            };
            let members: Vec<_> = world
                .query::<(Entity, &MemberOfBattalion)>()
                .iter(world)
                .filter(|(_, m)| m.0 == id)
                .map(|(e, _)| e)
                .collect();
            for soldier in &members {
                release(world, *soldier);
            }
            world.despawn(battalion);
            (
                members.len(),
                "Battalion disbanded; soldiers remain in your retinue".into(),
            )
        }
        ArmyOrder::SetStance { battalion, stance } => {
            let Some(id) = owned_battalion(world, account, battalion) else {
                return unavailable();
            };
            let count = super::set_stance(world, battalion, id, stance);
            (
                count.max(1),
                format!("Battalion stance: {}", stance.label()),
            )
        }
    }
}
