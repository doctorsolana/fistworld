//! Battalion intent and quiet deployment ranks. In combat each soldier chooses
//! a local approach; the formation is a preference, never an attack-side reservation.
use bevy::prelude::*;
#[cfg(test)]
use shared::formation::FILE_SPACING;
use shared::{
    components::*,
    formation::{FormationBlock, FormationSoldier, RANK_SPACING},
};
use std::collections::BTreeMap;

mod contacts;
mod geometry;
mod movement;
mod steering;
pub use contacts::{assign_formation_contacts, rebuild_combat_space, CombatSpace};
pub(crate) use geometry::Footprint;
pub use movement::advance_battle_fronts;

#[derive(Component, Clone, Copy, Debug)]
pub struct FormationMember {
    pub group: u64,
    pub battalion: Option<BattalionId>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Enemy {
    Battalion(BattalionId),
    Person(Entity),
}

#[derive(Component)]
pub struct PausedFormationMarch;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Intent {
    Hold,
    Attack(Enemy),
    March { engage: bool },
}

pub struct BattleFront {
    pub anchor: Vec2,
    pub facing: Vec2,
    pub columns: Vec<Vec<Entity>>,
    pub spacing: f32,
    /// Delay re-forming until local combat has stayed quiet.
    pub last_threat: f64,
    pub intent: Intent,
    pub active: bool,
    pub march_paused: bool,
    pub march_anchor: Vec2,
    /// Ranged deployment stays on occupied ground until the target leaves range.
    pub ranged_deployed: bool,
}
impl BattleFront {
    pub fn occupied_anchor(&self, space: &CombatSpace) -> Vec2 {
        let right = Vec2::new(self.facing.y, -self.facing.x);
        let mut sum = Vec2::ZERO;
        let mut count = 0;
        for (file, column) in self.columns.iter().enumerate() {
            let lateral = (file as f32 - (self.columns.len() - 1) as f32 * 0.5) * self.spacing;
            for (rank, e) in column.iter().enumerate() {
                if let Some(body) = space.body(*e) {
                    sum += body.point - right * lateral + self.facing * rank as f32 * RANK_SPACING;
                    count += 1;
                }
            }
        }
        if count > 0 {
            sum / count as f32
        } else {
            self.anchor
        }
    }
    pub fn posts(&self) -> impl Iterator<Item = (Entity, Vec2, Option<Entity>)> + '_ {
        let right = Vec2::new(self.facing.y, -self.facing.x);
        self.columns
            .iter()
            .enumerate()
            .flat_map(move |(file, column)| {
                let x = (file as f32 - (self.columns.len() - 1) as f32 * 0.5) * self.spacing;
                column.iter().enumerate().map(move |(rank, e)| {
                    (
                        *e,
                        self.anchor + right * x - self.facing * rank as f32 * RANK_SPACING,
                        rank.checked_sub(1).map(|i| column[i]),
                    )
                })
            })
    }
}

#[derive(Resource, Default)]
pub struct CombatFormations {
    next: u64,
    pub fronts: BTreeMap<u64, BattleFront>,
}

pub fn enemy_of(world: &World, entity: Entity) -> Enemy {
    world
        .get::<MemberOfBattalion>(entity)
        .map_or(Enemy::Person(entity), |m| Enemy::Battalion(m.0))
}

/// Called only at an accepted command/membership boundary, never every tick.
pub fn install(world: &mut World, block: FormationBlock, intent: Intent) {
    if block.slots.len() < 2
        && !block
            .slots
            .iter()
            .any(|(e, _)| world.get::<BowEquipped>(*e).is_some())
    {
        return;
    }
    world.init_resource::<CombatFormations>();
    world.init_resource::<crate::player::orders::FormationRoutes>();
    let mut columns = vec![Vec::new(); block.files.min(block.slots.len())];
    let count = columns.len();
    for ((entity, _), file) in block.slots.iter().zip(&block.file_indices) {
        columns[(*file).min(count - 1)].push(*entity);
    }
    let id = {
        let mut state = world.resource_mut::<CombatFormations>();
        state.next += 1;
        let id = state.next;
        state.fronts.insert(
            id,
            BattleFront {
                anchor: block.centre.xz(),
                facing: block.facing,
                spacing: block.spacing,
                last_threat: 0.0,
                columns,
                intent,
                active: matches!(intent, Intent::Attack(_)),
                march_paused: false,
                march_anchor: block.centre.xz(),
                ranged_deployed: false,
            },
        );
        id
    };
    for (entity, _) in block.slots {
        let battalion = world.get::<MemberOfBattalion>(entity).map(|m| m.0);
        world.entity_mut(entity).insert(FormationMember {
            group: id,
            battalion,
        });
    }
}

/// Preserve the actual file order on attack/hold; casual retargeting must not
/// reshuffle every soldier. New/unformed groups derive their first ranks once.
pub fn current_block(
    world: &World,
    key: u64,
    soldiers: &[FormationSoldier],
    preferred: BattalionFormation,
) -> FormationBlock {
    let previous = soldiers
        .first()
        .and_then(|s| world.get::<FormationMember>(s.entity))
        .and_then(|m| {
            world
                .get_resource::<CombatFormations>()?
                .fronts
                .get(&m.group)
        });
    if let Some(front) = previous {
        let mut slots = Vec::new();
        let mut file_indices = Vec::new();
        let mut positions = Vec::new();
        for rank in 0..front.columns.iter().map(Vec::len).max().unwrap_or(0) {
            for (file, column) in front.columns.iter().enumerate() {
                if let Some(entity) = column.get(rank) {
                    if let Some(s) = soldiers.iter().find(|s| s.entity == *entity) {
                        slots.push((*entity, s.position));
                        file_indices.push(file);
                        let right = Vec2::new(front.facing.y, -front.facing.x);
                        let lateral =
                            (file as f32 - (front.columns.len() - 1) as f32 * 0.5) * front.spacing;
                        positions.push(
                            s.position.xz() - right * lateral
                                + front.facing * rank as f32 * RANK_SPACING,
                        );
                    }
                }
            }
        }
        if slots.len() == soldiers.len() {
            let anchor = positions.iter().copied().sum::<Vec2>() / positions.len().max(1) as f32;
            return FormationBlock {
                key,
                centre: Vec3::new(anchor.x, soldiers[0].position.y, anchor.y),
                facing: front.facing,
                files: front.columns.len(),
                spacing: front.spacing,
                file_indices,
                slots,
            };
        }
    }
    let facing = soldiers
        .iter()
        .filter_map(|s| world.get::<PlayerRotation>(s.entity))
        .map(|r| Vec2::new(-r.0.sin(), -r.0.cos()))
        .sum::<Vec2>()
        .try_normalize()
        .unwrap_or(Vec2::Y);
    let shape = shared::formation::deployment_shape(soldiers.len(), preferred, None);
    let count = usize::from(shape.files);
    let depth = soldiers.len().div_ceil(count).saturating_sub(1) as f32 * RANK_SPACING;
    let centre = shared::formation::centre(soldiers.iter().map(|s| s.position))
        + Vec3::new(facing.x, 0.0, facing.y) * depth * 0.5;
    let right = Vec2::new(facing.y, -facing.x);
    let mut ordered = soldiers.to_vec();
    ordered.sort_by(|a, b| {
        b.position
            .xz()
            .dot(facing)
            .total_cmp(&a.position.xz().dot(facing))
            .then(a.identity.cmp(&b.identity))
    });
    for row in ordered.chunks_mut(count) {
        row.sort_by(|a, b| {
            a.position
                .xz()
                .dot(right)
                .total_cmp(&b.position.xz().dot(right))
                .then(a.identity.cmp(&b.identity))
        });
    }
    FormationBlock {
        key,
        centre,
        facing,
        files: count,
        spacing: shape.spacing,
        file_indices: (0..ordered.len()).map(|i| i % count).collect(),
        slots: ordered
            .into_iter()
            .map(|s| (s.entity, s.position))
            .collect(),
    }
}

#[cfg(test)]
mod tests;
