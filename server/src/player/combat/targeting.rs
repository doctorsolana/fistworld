//! A fresh post-movement spatial index of combat-capable bodies. Idle armies
//! participate, but an army on another side of the map is never scanned locally.

use super::{AttackOrder, WarParty, ACQUISITION_RANGE, MELEE_REACH};
use crate::player::hero::{MoveTarget, OfflineHero};
use crate::player::orders::CommandStance;
use bevy::prelude::*;
use shared::components::*;
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq)]
enum Side {
    Account(usize),
    Banner(u8),
}
struct Candidate {
    entity: Entity,
    point: Vec2,
    side: Side,
    range: f32,
    radius: f32,
}

#[derive(Default)]
pub struct AcquisitionScratch {
    accounts: HashMap<String, usize>,
    candidates: Vec<Candidate>,
    cells: HashMap<(i32, i32), Vec<usize>>,
}

fn cell(point: Vec2) -> (i32, i32) {
    (
        (point.x / ACQUISITION_RANGE).floor() as i32,
        (point.y / ACQUISITION_RANGE).floor() as i32,
    )
}

impl AcquisitionScratch {
    fn index(&mut self) {
        for members in self.cells.values_mut() {
            members.clear();
        }
        for (i, candidate) in self.candidates.iter().enumerate() {
            self.cells.entry(cell(candidate.point)).or_default().push(i);
        }
        self.cells.retain(|_, indices| !indices.is_empty());
    }

    fn nearest(&self, candidate: &Candidate) -> (Option<Entity>, usize) {
        let at = cell(candidate.point);
        let mut nearest: Option<(Entity, f32)> = None;
        let mut checked = 0;
        for x in -1..=1 {
            for z in -1..=1 {
                let Some(indices) = self.cells.get(&(at.0 + x, at.1 + z)) else {
                    continue;
                };
                for &i in indices {
                    checked += 1;
                    let other = &self.candidates[i];
                    if other.side == candidate.side {
                        continue;
                    }
                    let distance = candidate.point.distance_squared(other.point);
                    let range = if candidate.range > 0. && candidate.range < ACQUISITION_RANGE {
                        (candidate.radius + other.radius + 0.5).max(MELEE_REACH)
                    } else { candidate.range };
                    if distance > range * range {
                        continue;
                    }
                    if nearest.is_none_or(|(id, best)| {
                        distance < best || (distance == best && other.entity < id)
                    }) {
                        nearest = Some((other.entity, distance));
                    }
                }
            }
        }
        (nearest.map(|(e, _)| e), checked)
    }
}

#[allow(clippy::type_complexity)]
pub fn acquire_targets(
    mut commands: Commands,
    combatants: Query<
        (
            Entity,
            &PlayerPosition,
            Option<&WarParty>,
            Option<&CommandedBy>,
            Has<AttackOrder>,
            Has<MoveTarget>,
            Has<super::fronts::FormationMember>,
            Option<&CommandStance>,
            Option<&Health>,
        ),
        (
            With<CharacterKind>,
            Without<OfflineHero>,
            Without<AboardBoat>,
        ),
    >,
    identities: Query<&PersonId>,
    policies: Query<&BattalionStance>,
    bows: Query<(), With<BowEquipped>>,
    mounts: Query<(), With<Mounted>>,
    mut scratch: Local<AcquisitionScratch>,
) {
    scratch.candidates.clear();
    for (entity, position, party, owner, engaged, moving, formed, stance, health) in &combatants {
        if health.is_some_and(|h| h.is_dead()) {
            continue;
        }
        let side = if let Some(owner) = owner {
            let next = scratch.accounts.len();
            let key = if let Some(key) = scratch.accounts.get(&owner.0) {
                *key
            } else {
                scratch.accounts.insert(owner.0.clone(), next);
                next
            };
            Side::Account(key)
        } else if let Some(party) = party {
            Side::Banner(party.banner)
        } else {
            continue;
        };
        let hold_reach = if mounts.contains(entity) { HORSE_BODY_RADIUS * 2. + 0.5 } else { MELEE_REACH };
        let range = if engaged || formed || bows.contains(entity) {
            0.0
        } else {
            match stance {
                Some(CommandStance::Move | CommandStance::Retreat) => 0.0,
                Some(CommandStance::Hold | CommandStance::Guard) => hold_reach,
                Some(CommandStance::AttackMove) => ACQUISITION_RANGE,
                None if moving => 0.0,
                None if policies
                    .get(entity)
                    .is_ok_and(|s| *s == BattalionStance::HoldLine) =>
                {
                    hold_reach
                }
                None => ACQUISITION_RANGE,
            }
        };
        scratch.candidates.push(Candidate {
            entity,
            point: position.0.xz(),
            side,
            range,
            radius: if mounts.contains(entity) { HORSE_BODY_RADIUS } else { super::BODY_RADIUS },
        });
    }
    if scratch.candidates.len() < 2 {
        return;
    }
    scratch.index();
    for candidate in &scratch.candidates {
        if candidate.range == 0.0 {
            continue;
        }
        if let (Some(target), _) = scratch.nearest(candidate) {
            let mut entity = commands.entity(candidate.entity);
            entity.insert(AttackOrder { target });
            if let Ok(person) = identities.get(target) {
                entity.insert(EngagedWith(*person));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn distant_armies_do_not_scan_each_other() {
        let mut scratch = AcquisitionScratch::default();
        for i in 0..2000 {
            scratch.candidates.push(Candidate {
                entity: Entity::from_raw_u32(i + 1).unwrap(),
                point: Vec2::new(i as f32 * 40.0, 0.0),
                side: Side::Account(i as usize % 2),
                range: ACQUISITION_RANGE,
                radius: super::super::BODY_RADIUS,
            });
        }
        scratch.index();
        let visits: usize = scratch
            .candidates
            .iter()
            .map(|c| {
                let (target, visits) = scratch.nearest(c);
                assert!(target.is_none());
                visits
            })
            .sum();
        assert_eq!(visits, 2000); // one self entry each, not four million pairs
    }

    #[test]
    fn acquisition_covers_nine_metres_across_negative_cell_boundaries() {
        let mut scratch = AcquisitionScratch::default();
        for (i, x) in [-0.1, 8.8, 9.1].into_iter().enumerate() {
            scratch.candidates.push(Candidate {
                entity: Entity::from_raw_u32(i as u32 + 1).unwrap(),
                point: Vec2::new(x, 0.0),
                side: Side::Account(i),
                range: ACQUISITION_RANGE,
                radius: super::super::BODY_RADIUS,
            });
        }
        scratch.index();
        assert_eq!(
            scratch.nearest(&scratch.candidates[0]).0,
            Some(scratch.candidates[1].entity)
        );
    }
}
