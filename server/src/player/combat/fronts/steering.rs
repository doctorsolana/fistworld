//! Local approach choices for formed soldiers. Goals feed the existing mover;
//! this module never changes position or performs per-soldier pathfinding.
use super::{geometry::CONTACT_DISTANCE, CombatSpace};
use bevy::prelude::*;

#[derive(Default)]
pub(super) struct Steering {
    heading: Vec2,
    side: f32,
    turn_until: f64,
    target: Option<Entity>,
    approach_offset: Vec2,
    choose_after: f64,
    /// Once an opening is taken, do not tug the soldier back into their file
    /// whenever another body briefly crosses the approach.
    pub(super) released: bool,
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::components::*;

    #[test]
    fn a_passing_ally_does_not_flip_the_contact_goal_but_a_dead_enemy_invalidates_it() {
        let mut app = App::new();
        app.init_resource::<CombatSpace>();
        app.add_systems(Update, super::super::rebuild_combat_space);
        let mut spawn = |owner: &str, point: Vec2| {
            app.world_mut()
                .spawn((
                    CharacterKind::Villager,
                    CommandedBy(owner.into()),
                    PlayerPosition(Vec3::new(point.x, 0.0, point.y)),
                    PlayerRotation(0.0),
                    Health::new(100.0),
                ))
                .id()
        };
        let soldier = spawn("alice", Vec2::ZERO);
        let enemy = spawn("bob", Vec2::new(0.0, 4.0));
        let other = spawn("bob", Vec2::new(3.0, 4.0));
        app.update();
        let mut steering = Steering::default();
        let goal = steering
            .local_goal(app.world().resource::<CombatSpace>(), soldier, 0.0)
            .unwrap();
        app.world_mut().spawn((
            CharacterKind::Villager,
            CommandedBy("alice".into()),
            PlayerPosition(Vec3::new(goal.x, 0.0, goal.y)),
            PlayerRotation(0.0),
        ));
        app.update();
        let retained = steering
            .local_goal(app.world().resource::<CombatSpace>(), soldier, 0.1)
            .unwrap();
        assert!(
            retained.distance(goal) < 0.01,
            "a passing body must not reverse the approach"
        );
        app.world_mut().get_mut::<Health>(enemy).unwrap().current = 0.0;
        app.update();
        let replacement = steering
            .local_goal(app.world().resource::<CombatSpace>(), soldier, 0.2)
            .unwrap();
        let enemy_point = app.world().get::<PlayerPosition>(other).unwrap().0.xz();
        assert!((replacement.distance(enemy_point) - CONTACT_DISTANCE).abs() < 0.01);
        assert!(
            replacement.distance(goal) > 1.0,
            "a dead target must be replaced without waiting"
        );
    }
}

/// Prefer exposed nearby opponents and approach from the current side. Crowded
/// contact points cost more, so edge soldiers naturally spill around them.
impl Steering {
    pub(super) fn local_goal(
        &mut self,
        space: &CombatSpace,
        entity: Entity,
        now: f64,
    ) -> Option<Vec2> {
        let body = space.body(entity)?;
        let previous = self
            .target
            .and_then(|target| space.body(target))
            .filter(|enemy| {
                enemy.side != body.side && enemy.point.distance_squared(body.point) < 144.0
            });
        // Follow a chosen contact point for a short interval instead of picking
        // a different side every time a neighbouring body moves. A dead or
        // distant opponent invalidates the choice immediately. Staggering the
        // next decision also spreads the expensive candidate work across ticks.
        if now < self.choose_after {
            if let Some(enemy) = previous {
                return Some(enemy.point + self.approach_offset);
            }
        }
        let mut best: Option<(Vec2, f32, Entity)> = None;
        // Retain only eight nearest opponents before evaluating approach angles.
        // Dense battles cannot multiply the expensive clearance pass by roster size.
        let mut nearest = [None::<(&super::contacts::Body, f32)>; 8];
        for enemy in space.within(body.point, 12.0) {
            let distance = enemy.point.distance_squared(body.point);
            if enemy.side == body.side || distance >= 144.0 {
                continue;
            }
            if let Some(index) = nearest.iter().position(|entry| {
                entry
                    .is_none_or(|(b, d)| distance < d || (distance == d && enemy.entity < b.entity))
            }) {
                for i in (index + 1..8).rev() {
                    nearest[i] = nearest[i - 1];
                }
                nearest[index] = Some((enemy, distance));
            }
        }
        for (enemy, _) in nearest.into_iter().flatten() {
            let outward = (body.point - enemy.point)
                .try_normalize()
                .unwrap_or(Vec2::X);
            for angle in [0.0_f32, 0.55, -0.55, 1.05, -1.05] {
                let goal = enemy.point + Mat2::from_angle(angle) * outward * CONTACT_DISTANCE;
                let crowd = space
                    .nearby(goal)
                    .filter(|b| {
                        b.entity != entity
                            && b.entity != enemy.entity
                            && b.point.distance_squared(goal) < 0.95 * 0.95
                    })
                    .count();
                let change = previous.map_or(0.0, |old| {
                    goal.distance_squared(old.point + self.approach_offset)
                        .min(9.0)
                        * 2.0
                });
                let cost =
                    body.point.distance_squared(goal) + crowd as f32 * 12.0 + angle.abs() + change
                        - if self.target == Some(enemy.entity) {
                            2.0
                        } else {
                            0.0
                        };
                if best.is_none_or(|(_, old, id)| cost < old || (cost == old && enemy.entity < id))
                {
                    best = Some((goal, cost, enemy.entity));
                }
            }
        }
        self.target = best.map(|(_, _, enemy)| enemy);
        if let Some((goal, _, enemy)) = best {
            self.approach_offset = goal - space.body(enemy).unwrap().point;
            self.choose_after = now + 0.55 + (entity.to_bits() % 5) as f64 * 0.05;
        }
        best.map(|(p, _, _)| p)
    }
}

impl Steering {
    /// A short, swept and collision-certified step. Heading persistence avoids
    /// alternating left/right at a blockage. A packed rear rank may wait rather
    /// than repeatedly pressing into the fighter in front.
    pub(super) fn step(
        &mut self,
        space: &CombatSpace,
        entity: Entity,
        from: Vec2,
        goal: Vec2,
        now: f64,
        keep_file: bool,
        ground_clear: impl Fn(Vec2) -> bool,
    ) -> Option<Vec2> {
        let delta = goal - from;
        let distance = delta.length();
        if distance < 0.2 {
            return None;
        }
        let forward = delta / distance;
        let formation = space.body(entity).and_then(|body| body.formation);
        let length = distance.min(1.8);
        let mut best: Option<(Vec2, Vec2, f32, f32)> = None;
        for step_length in [length, length.min(0.6)] {
            for angle in [0.0_f32, 0.35, -0.35, 0.7, -0.7, 1.05, -1.05, 1.4, -1.4] {
                let direction = Mat2::from_angle(angle) * forward;
                let end = from + direction * step_length;
                if !space.approach_clear(entity, from, end) || !ground_clear(end) {
                    continue;
                }
                let look = end + forward * 1.0;
                let crowded = space
                    .nearby(look)
                    .filter(|b| {
                        b.entity != entity
                            && b.point.distance_squared(look) < 0.9 * 0.9
                            && !(keep_file && formation.is_some() && b.formation == formation)
                    })
                    .count() as f32;
                let switching = if now < self.turn_until && angle * self.side < -0.01 {
                    3.0
                } else {
                    0.0
                };
                let cost = (length - step_length) * 0.3
                    + angle.abs() * if keep_file { 1.8 } else { 0.6 }
                    + crowded * 0.7
                    + switching
                    + (1.0 - direction.dot(self.heading));
                if best.is_none_or(|(_, _, _, old)| cost < old) {
                    best = Some((end, direction, angle, cost));
                }
            }
        }
        let (end, direction, angle, _) = best?;
        self.heading = direction;
        if angle.abs() > 0.3 && (now >= self.turn_until || self.side == 0.0) {
            self.side = angle.signum();
            self.turn_until = now + 1.5;
        }
        Some(end)
    }
}
