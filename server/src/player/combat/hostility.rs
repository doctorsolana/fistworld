//! Ownership permits orders; it does not declare war. Reuse a compact snapshot
//! of authoritative combat intent in both local acquisition indices.
use super::{
    fronts::{CombatFormations, Enemy, FormationMember, Intent},
    AttackOrder, SkirmishOrder, WarParty,
};
use crate::player::orders::CommandStance;
use bevy::{ecs::system::SystemParam, prelude::*};
use shared::components::{BattalionId, CommandedBy, MemberOfBattalion};
use std::collections::{HashMap, HashSet};

#[derive(Clone, Copy)]
pub struct Allegiance {
    side: usize,
    aggressive: bool,
}
impl Allegiance {
    pub fn same_side(self, other: Self) -> bool {
        self.side == other.side
    }
}

/// Retained allocations, linear preparation, constant-time local comparisons.
/// Active engagements disappear when their authoritative orders disappear;
/// this is not a persistent diplomacy or account-wide declaration of war.
#[derive(Default)]
pub struct HostilityIndex {
    accounts: HashMap<String, usize>,
    people: HashMap<Entity, Allegiance>,
    battalions: HashMap<BattalionId, Allegiance>,
    engagements: HashSet<(usize, usize)>,
}
impl HostilityIndex {
    pub fn clear(&mut self) {
        self.people.clear();
        self.battalions.clear();
        self.engagements.clear();
    }

    pub fn insert(
        &mut self,
        entity: Entity,
        owner: Option<&CommandedBy>,
        party: Option<&WarParty>,
        battalion: Option<&MemberOfBattalion>,
        attack_move: bool,
    ) -> Option<Allegiance> {
        let side = if let Some(owner) = owner {
            let next = self.accounts.len() + 256;
            if let Some(side) = self.accounts.get(&owner.0) {
                *side
            } else {
                self.accounts.insert(owner.0.clone(), next);
                next
            }
        } else {
            usize::from(party?.banner)
        };
        let allegiance = Allegiance {
            side,
            aggressive: party.is_some() || attack_move,
        };
        self.people.insert(entity, allegiance);
        if let Some(battalion) = battalion {
            self.battalions.insert(battalion.0, allegiance);
        }
        Some(allegiance)
    }

    pub fn hostile(&self, a: Allegiance, b: Allegiance) -> bool {
        !a.same_side(b)
            && (a.aggressive || b.aggressive || self.engagements.contains(&pair(a.side, b.side)))
    }

    pub fn record_intents(&mut self, intents: &CombatIntents) {
        for (entity, attack, skirmish, member) in &intents.orders {
            let Some(actor) = self.people.get(&entity) else {
                continue;
            };
            let objective = intents
                .formation_intent(member)
                .and_then(|intent| match intent {
                    Intent::Attack(enemy) => Some(enemy),
                    _ => None,
                })
                .or(skirmish.map(|order| order.target))
                .or(attack.map(|order| Enemy::Person(order.target)));
            let other = match objective {
                Some(Enemy::Person(entity)) => self.people.get(&entity),
                Some(Enemy::Battalion(id)) => self.battalions.get(&id),
                None => None,
            };
            if let Some(other) = other {
                self.engagements.insert(pair(actor.side, other.side));
            }
        }
    }
}
fn pair(a: usize, b: usize) -> (usize, usize) {
    (a.min(b), a.max(b))
}

#[derive(SystemParam)]
pub struct CombatIntents<'w, 's> {
    #[allow(clippy::type_complexity)]
    orders: Query<
        'w,
        's,
        (
            Entity,
            Option<&'static AttackOrder>,
            Option<&'static SkirmishOrder>,
            Option<&'static FormationMember>,
        ),
        Or<(
            With<AttackOrder>,
            With<SkirmishOrder>,
            With<FormationMember>,
        )>,
    >,
    formations: Option<Res<'w, CombatFormations>>,
}
impl CombatIntents<'_, '_> {
    fn formation_intent(&self, member: Option<&FormationMember>) -> Option<Intent> {
        self.formations
            .as_ref()?
            .fronts
            .get(&member?.group)
            .map(|f| f.intent)
    }

    pub fn attack_move(&self, entity: Entity, stance: Option<&CommandStance>) -> bool {
        stance == Some(&CommandStance::AttackMove)
            || self.orders.get(entity).is_ok_and(|(_, _, _, member)| {
                self.formation_intent(member) == Some(Intent::March { engage: true })
            })
    }
}
