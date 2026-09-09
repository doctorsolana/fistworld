//! Ranged combat authority. Orders remain in the shared command/formation path;
//! this domain owns weapon choice, draw cancellation, firing and swept impacts.
use super::{
    combat::{
        fronts::{CombatFormations, CombatSpace, Enemy, FormationMember, Intent},
        AttackOrder, SkirmishOrder,
    },
    hero::{MoveTarget, OfflineHero},
    orders::{CommandStance, MarchOrder},
};
use bevy::prelude::*;
use shared::components::*;
mod collision;
mod defenses;
mod equipment;
mod flight;
mod shooting;
pub use collision::sync_defense_arrow_obstacles;
pub use collision::ArrowObstacles;
pub(crate) use equipment::safe_to_equip;
pub use equipment::{apply_equipment_order, inherit_equipment};
pub use flight::advance_arrows;
pub use shooting::shoot_bows;

#[derive(Component, Default)]
pub struct ArcherState {
    target: Option<Entity>,
    release_at: Option<f64>,
    ready_at: f64,
    next_decision: f64,
    melee_until: f64,
}
impl ArcherState {
    pub fn cancel(&mut self) {
        self.target = None;
        self.release_at = None;
    }
}
#[derive(Component)]
pub struct ArrowFlight {
    shooter: Entity,
    checked_at: f64,
}
pub(crate) fn seconds(clock: &WorldTime) -> f64 {
    super::combat::world_clock_seconds(clock)
}
pub(crate) fn objective(
    member: Option<&FormationMember>,
    skirmish: Option<&SkirmishOrder>,
    fronts: &CombatFormations,
) -> Option<Enemy> {
    member
        .and_then(|m| fronts.fronts.get(&m.group))
        .and_then(|f| {
            if let Intent::Attack(e) = f.intent {
                Some(e)
            } else {
                None
            }
        })
        .or(skirmish.map(|s| s.target))
}
/// Bow/sidearm hysteresis prevents weapon flicker at the edge of a melee.
#[allow(clippy::type_complexity)]
pub fn update_weapons(
    mut commands: Commands,
    clock: Query<&WorldTime>,
    space: Res<CombatSpace>,
    mut soldiers: Query<
        (
            Entity,
            &SoldierRole,
            &Quiver,
            &Health,
            &mut ArcherState,
            Has<BowEquipped>,
            Option<&BowShot>,
        ),
        (Without<OfflineHero>, Without<AboardBoat>),
    >,
) {
    let Some(clock) = clock.iter().next() else {
        return;
    };
    let now = seconds(clock);
    for (e, role, quiver, health, mut state, bow, shot) in &mut soldiers {
        if *role != SoldierRole::Archer || health.is_dead() {
            state.cancel();
            if bow {
                commands.entity(e).remove::<(BowEquipped, BowShot)>();
            }
            continue;
        }
        let Some(body) = space.body(e) else {
            continue;
        };
        let close = space
            .within(body.point, if bow { 5. } else { 9. })
            .any(|b| {
                b.side != body.side
                    && b.point.distance_squared(body.point) < if bow { 25. } else { 81. }
            });
        if close {
            state.melee_until = now + 2.0;
        }
        let recovering = shot.is_some_and(|s| now >= s.release_at && now < s.release_at + 1.0)
            && state.release_at.is_none();
        let want = (quiver.arrows > 0 || recovering) && now >= state.melee_until;
        if want && !bow {
            state.cancel();
            commands
                .entity(e)
                .insert(BowEquipped)
                .remove::<(AttackOrder, CombatSwing, BowShot, EngagedWith)>();
        } else if !want && bow {
            state.cancel();
            commands
                .entity(e)
                .remove::<(BowEquipped, BowShot, EngagedWith)>();
        }
    }
}
#[cfg(test)]
mod tests;
