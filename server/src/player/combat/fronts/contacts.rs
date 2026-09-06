//! One reusable local body index. Contacts require space for the weapon: a
//! soldier cannot hit through the rank in front, or share an enemy with a pile.
use super::*;
use crate::player::{
    combat::{AttackOrder, WarParty, MELEE_REACH},
    hero::{MoveTarget, OfflineHero},
};
use std::collections::HashMap;
const CELL: f32 = 3.0;
#[derive(Clone, Copy)]
pub struct Body {
    pub entity: Entity,
    pub point: Vec2,
    pub side: usize,
    pub battalion: Option<BattalionId>,
    pub formation: Option<u64>,
    pub facing: Vec2,
}
#[derive(Resource, Default)]
pub struct CombatSpace {
    pub bodies: Vec<Body>,
    pub by_entity: HashMap<Entity, usize>,
    by_battalion: HashMap<BattalionId, Vec<usize>>,
    cells: HashMap<(i32, i32), Vec<usize>>,
    accounts: HashMap<String, usize>,
}
fn cell(p: Vec2) -> (i32, i32) {
    ((p.x / CELL).floor() as i32, (p.y / CELL).floor() as i32)
}
impl CombatSpace {
    pub fn body(&self, e: Entity) -> Option<&Body> {
        self.by_entity.get(&e).map(|i| &self.bodies[*i])
    }
    pub fn nearby(&self, p: Vec2) -> impl Iterator<Item = &Body> {
        let at = cell(p);
        (-1..=1)
            .flat_map(move |x| (-1..=1).filter_map(move |z| self.cells.get(&(at.0 + x, at.1 + z))))
            .flatten()
            .map(|i| &self.bodies[*i])
    }
    pub fn clear_strike(&self, a: Entity, b: Entity) -> bool {
        let (Some(a), Some(b)) = (self.body(a), self.body(b)) else {
            return false;
        };
        let delta = b.point - a.point;
        let d2 = delta.length_squared();
        if d2 > MELEE_REACH * MELEE_REACH || d2 < 0.01 {
            return false;
        }
        !self.nearby(a.point).any(|other| {
            if other.entity == a.entity || other.entity == b.entity {
                return false;
            }
            let t = (other.point - a.point).dot(delta) / d2;
            t > 0.05 && t < 0.95 && other.point.distance_squared(a.point + delta * t) < 0.42 * 0.42
        })
    }
    pub fn movement_clear(&self, entity: Entity, current: Vec2, next: Vec2) -> bool {
        let delta = next - current;
        let d2 = delta.length_squared().max(0.000001);
        !self.nearby(current).any(|other| {
            if other.entity == entity {
                return false;
            }
            let old = current.distance_squared(other.point);
            // Let existing penetration resolve outward, never deepen it.
            if old < 0.72 * 0.72 && next.distance_squared(other.point) > old {
                return false;
            }
            let t = ((other.point - current).dot(delta) / d2).clamp(0.0, 1.0);
            other.point.distance_squared(current + delta * t) < 0.72 * 0.72
        })
    }
    pub fn enemy_members(&self, enemy: Enemy) -> impl Iterator<Item = &Body> {
        let indices: &[usize] = match enemy {
            Enemy::Battalion(id) => self.by_battalion.get(&id).map_or(&[], Vec::as_slice),
            Enemy::Person(e) => self.by_entity.get(&e).map_or(&[], std::slice::from_ref),
        };
        indices.iter().map(|i| &self.bodies[*i])
    }
    pub fn approach_clear(&self, entity: Entity, from: Vec2, to: Vec2) -> bool {
        let distance = from.distance(to);
        let steps = (distance / CELL).ceil().max(1.0) as usize;
        // This is local melee manoeuvring, never a strategic route search.
        if steps > 32 {
            return false;
        }
        (0..steps).all(|i| {
            self.movement_clear(
                entity,
                from.lerp(to, i as f32 / steps as f32),
                from.lerp(to, (i + 1) as f32 / steps as f32),
            )
        })
    }
    pub fn footprint(&self, enemy: Enemy, fronts: &CombatFormations) -> Option<Footprint> {
        let indices: &[usize] = match enemy {
            Enemy::Battalion(id) => self.by_battalion.get(&id)?.as_slice(),
            Enemy::Person(e) => std::slice::from_ref(self.by_entity.get(&e)?),
        };
        let first = self.bodies[*indices.first()?];
        let facing = first
            .formation
            .and_then(|id| fronts.fronts.get(&id))
            .map_or(first.facing, |f| f.facing);
        let right = Vec2::new(facing.y, -facing.x);
        let mut min = Vec2::splat(f32::INFINITY);
        let mut max = Vec2::splat(f32::NEG_INFINITY);
        for &index in indices {
            let p = self.bodies[index].point;
            let projected = Vec2::new(p.dot(right), p.dot(facing));
            min = min.min(projected);
            max = max.max(projected);
        }
        let middle = (min + max) * 0.5;
        Some(Footprint {
            centre: right * middle.x + facing * middle.y,
            facing,
            half_width: (max.x - min.x) * 0.5,
            half_depth: (max.y - min.y) * 0.5,
        })
    }
}

#[allow(clippy::type_complexity)]
pub fn rebuild_combat_space(
    mut space: ResMut<CombatSpace>,
    people: Query<
        (
            Entity,
            &PlayerPosition,
            &PlayerRotation,
            Option<&CommandedBy>,
            Option<&WarParty>,
            Option<&MemberOfBattalion>,
            Option<&FormationMember>,
            Option<&Health>,
        ),
        (
            With<CharacterKind>,
            Or<(With<CommandedBy>, With<WarParty>)>,
            Without<OfflineHero>,
            Without<AboardBoat>,
            Without<crate::world::village::strategic::StrategicPerson>,
        ),
    >,
) {
    space.bodies.clear();
    space.by_entity.clear();
    for entries in space.cells.values_mut() {
        entries.clear();
    }
    for entries in space.by_battalion.values_mut() {
        entries.clear();
    }
    for (entity, position, rotation, owner, party, battalion, formation, health) in &people {
        if health.is_some_and(|h| h.is_dead()) {
            continue;
        }
        let side = if let Some(owner) = owner {
            let next = space.accounts.len() + 256;
            if let Some(side) = space.accounts.get(&owner.0) {
                *side
            } else {
                space.accounts.insert(owner.0.clone(), next);
                next
            }
        } else if let Some(party) = party {
            usize::from(party.banner)
        } else {
            continue;
        };
        let body = Body {
            entity,
            point: position.0.xz(),
            side,
            battalion: battalion.map(|m| m.0),
            formation: formation.map(|m| m.group),
            facing: Vec2::new(-rotation.0.sin(), -rotation.0.cos()),
        };
        let index = space.bodies.len();
        space.by_entity.insert(entity, index);
        space.cells.entry(cell(body.point)).or_default().push(index);
        if let Some(id) = body.battalion {
            space.by_battalion.entry(id).or_default().push(index);
        }
        space.bodies.push(body);
    }
    space.cells.retain(|_, v| !v.is_empty());
    space.by_battalion.retain(|_, v| !v.is_empty());
}

#[allow(clippy::type_complexity)]
pub fn assign_formation_contacts(
    mut commands: Commands,
    space: Res<CombatSpace>,
    fronts: Res<CombatFormations>,
    soldiers: Query<(Entity, &FormationMember, Option<&AttackOrder>)>,
    mut loads: Local<HashMap<Entity, usize>>,
) {
    loads.clear();
    // Existing valid contacts get first refusal. Target identity stays stable
    // when a neighbouring opponent becomes a few centimetres nearer.
    for (entity, member, order) in &soldiers {
        if fronts.fronts.get(&member.group).is_some_and(|f| f.active) {
            if let Some(order) = order.filter(|o| space.clear_strike(entity, o.target)) {
                *loads.entry(order.target).or_default() += 1;
            }
        }
    }
    for (entity, member, order) in &soldiers {
        let active = fronts.fronts.get(&member.group).is_some_and(|f| f.active);
        let Some(body) = space.body(entity) else {
            continue;
        };
        if !active {
            continue;
        }
        commands.entity(entity).insert_if_new(CombatReady);
        if order.is_some_and(|o| {
            space.body(o.target).is_some_and(|b| b.side != body.side)
                && space.clear_strike(entity, o.target)
        }) {
            continue;
        }
        let target = space
            .nearby(body.point)
            .filter(|other| {
                other.side != body.side && loads.get(&other.entity).copied().unwrap_or(0) < 2
            })
            .filter(|other| space.clear_strike(entity, other.entity))
            .min_by(|a, b| {
                a.point
                    .distance_squared(body.point)
                    .total_cmp(&b.point.distance_squared(body.point))
                    .then(a.entity.cmp(&b.entity))
            });
        if let Some(target) = target {
            *loads.entry(target.entity).or_default() += 1;
            commands
                .entity(entity)
                .insert(AttackOrder {
                    target: target.entity,
                })
                .remove::<MoveTarget>();
        } else if order.is_some() {
            commands
                .entity(entity)
                .remove::<(AttackOrder, EngagedWith)>();
        }
    }
}
