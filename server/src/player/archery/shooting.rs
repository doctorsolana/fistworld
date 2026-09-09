use super::*;
use crate::collision::{building_index::BuildingSpatialIndex, library::StaticColliders};
use lightyear::prelude::{NetworkTarget, Replicate};
use shared::{region::RegionCoord, terrain::WorldTerrain};
#[derive(bevy::ecs::system::SystemParam)]
pub struct FiringWorld<'w, 's> {
    space: Res<'w, CombatSpace>,
    fronts: Res<'w, CombatFormations>,
    obstacles: ResMut<'w, ArrowObstacles>,
    buildings: Option<Res<'w, BuildingSpatialIndex>>,
    props: Option<Res<'w, StaticColliders>>,
    terrain: Option<Res<'w, WorldTerrain>>,
    mounts: Query<'w, 's, (), With<Mounted>>,
    mounted_poses: Query<'w, 's, &'static PlayerRotation, With<Mounted>>,
    bodies: Query<
        'w,
        's,
        (
            &'static PlayerPosition,
            Option<&'static CharacterMotion>,
            &'static Health,
            Option<&'static PersonId>,
            Has<Mounted>,
        ),
    >,
}
impl FiringWorld<'_, '_> {
    fn aim(&self, from: Vec3, target: Entity) -> Option<Vec3> {
        let (position, motion, health, _, mounted) = self.bodies.get(target).ok()?;
        if health.is_dead() || from.xz().distance_squared(position.0.xz()) > BOW_RANGE * BOW_RANGE {
            return None;
        }
        arrow_velocity(
            from,
            position.0 + Vec3::Y * if mounted { 2.25 } else { 1.15 },
            motion.map_or(Vec3::ZERO, |m| m.velocity),
        )
    }
    fn clear(&self, e: Entity, target: Entity, from: Vec3, velocity: Vec3) -> bool {
        let Some(body) = self.space.body(e) else {
            return false;
        };
        let Ok((p, ..)) = self.bodies.get(target) else {
            return false;
        };
        let duration = from.xz().distance(p.0.xz()) / velocity.xz().length();
        let flight = ArrowProjectile {
            origin: from,
            velocity,
            launched_at: 0.,
            stopped_at: None,
        };
        let steps = (duration * ARROW_SPEED / 1.5).ceil() as usize;
        let body_search_radius = if self.mounts.is_empty() {
            3.
        } else {
            1.5 + collision::MOUNTED_HORIZONTAL_EXTENT
        };
        for i in 0..steps {
            let a = flight.position(f64::from(duration) * i as f64 / steps as f64);
            let b = flight.position(f64::from(duration) * (i + 1) as f64 / steps as f64);
            if self.obstacles.hit(a, b, self.props.as_deref()).is_some()
                || self
                    .terrain
                    .as_deref()
                    .is_some_and(|t| b.y <= t.get_height(b.x, b.z))
            {
                return false;
            }
            // Do not intentionally shoot through allies. Enemies intercept the
            // actual arrow normally, so dense enemy ranks do not block firing.
            if self.space.within(a.xz(), body_search_radius).any(|other| {
                other.entity != e
                    && other.side == body.side
                    && self
                        .bodies
                        .get(other.entity)
                        .is_ok_and(|(p, _, h, _, mounted)| {
                            let mounted_yaw = mounted.then_some(
                                self.mounted_poses.get(other.entity).map_or(0., |r| r.0),
                            );
                            !h.is_dead() && collision::body_hit(a, b, p.0, mounted_yaw).is_some()
                        })
            }) {
                return false;
            }
        }
        true
    }
}
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn shoot_bows(
    mut commands: Commands,
    clock: Query<&WorldTime>,
    mut field: FiringWorld,
    mut units: Query<
        (
            Entity,
            &PlayerPosition,
            &mut PlayerRotation,
            &CharacterMotion,
            &mut CharacterActivity,
            &mut Quiver,
            &mut ArcherState,
            Option<&FormationMember>,
            Option<&SkirmishOrder>,
            Option<&CommandStance>,
            Option<&FirePolicy>,
            Option<&CombatReaction>,
            Option<&BowShot>,
        ),
        (
            With<BowEquipped>,
            Without<OfflineHero>,
            Without<AboardBoat>,
            // Mounted bow combat is not implemented. This also keeps the
            // mounted collision poses disjoint from mutable archer facing.
            Without<Mounted>,
        ),
    >,
    mut candidates: Local<Vec<(Entity, f32)>>,
) {
    let Some(clock) = clock.iter().next() else {
        return;
    };
    let now = seconds(clock);
    if units.is_empty() {
        return;
    }
    field.obstacles.sync(field.buildings.as_deref());
    for (
        e,
        p,
        mut rotation,
        motion,
        mut activity,
        mut quiver,
        mut state,
        member,
        skirmish,
        stance,
        policy,
        reaction,
        shot,
    ) in &mut units
    {
        let explicit = objective(member, skirmish, &field.fronts);
        let interrupted = motion.is_moving()
            || matches!(stance, Some(CommandStance::Move | CommandStance::Retreat))
            || reaction.is_some_and(|r| r.fatal || now - r.at < 0.4)
            || policy == Some(&FirePolicy::HoldFire);
        if interrupted {
            state.cancel();
            if shot.is_some() {
                commands.entity(e).remove::<(BowShot, EngagedWith)>();
            }
            continue;
        }
        if shot.is_some_and(|s| now > s.release_at + 1.) {
            commands.entity(e).remove::<BowShot>();
        }
        if quiver.arrows == 0 {
            continue;
        }
        let origin = p.0 + Quat::from_rotation_y(rotation.0) * BOW_RELEASE_LOCAL;
        if let Some(release) = state.release_at {
            let aim = state
                .target
                .and_then(|target| field.aim(origin, target).map(|v| (target, v)));
            let Some((target, mut velocity)) = aim else {
                state.cancel();
                commands.entity(e).remove::<(BowShot, EngagedWith)>();
                continue;
            };
            rotation.set_if_neq(PlayerRotation(f32::atan2(-velocity.x, -velocity.z)));
            if now < release {
                continue;
            }
            state.release_at = None;
            if !field.clear(e, target, origin, velocity) {
                commands.entity(e).remove::<(BowShot, EngagedWith)>();
                continue;
            }
            // Reproducible small angular spread, sampled once. No homing or hit dice.
            let seed = shared::rng::XorShift64::new(e.to_bits() ^ release.to_bits()).next_u64();
            let jitter = |bits: u64| (bits as u16 as f32 / 65535. - 0.5) * 0.018;
            velocity =
                Quat::from_euler(EulerRot::YXZ, jitter(seed), jitter(seed >> 16), 0.) * velocity;
            quiver.arrows -= 1;
            commands.spawn((
                ArrowProjectile {
                    origin,
                    velocity,
                    launched_at: now,
                    stopped_at: None,
                },
                ArrowFlight {
                    shooter: e,
                    checked_at: now,
                },
                RegionCoord::from_world_pos(origin),
                Replicate::to_clients(NetworkTarget::All),
            ));
            continue;
        }
        if now < state.ready_at || now < state.next_decision {
            continue;
        }
        state.next_decision = now + 0.25 + (e.to_bits() % 7) as f64 * 0.013;
        let Some(body) = field.space.body(e) else {
            continue;
        };
        candidates.clear();
        for other in field.space.within(body.point, BOW_RANGE) {
            let distance = body.point.distance_squared(other.point);
            if other.side == body.side || !(36.0..=BOW_RANGE * BOW_RANGE).contains(&distance) {
                continue;
            }
            if explicit.is_some_and(|goal| match goal {
                Enemy::Battalion(id) => other.battalion != Some(id),
                Enemy::Person(target) => other.entity != target,
            }) {
                continue;
            }
            candidates.push((
                other.entity,
                distance
                    + if Some(other.entity) == state.target {
                        -64.
                    } else {
                        0.
                    },
            ));
        }
        if let Some(Enemy::Person(target)) = explicit {
            if field.space.body(target).is_none() && field.aim(origin, target).is_some() {
                candidates.push((target, 0.));
            }
        }
        candidates.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
        for &(target, _) in candidates.iter().take(8) {
            let Some(velocity) = field.aim(origin, target) else {
                continue;
            };
            if !field.clear(e, target, origin, velocity) {
                continue;
            }
            let release = now + f64::from(BOW_DRAW_SECONDS);
            state.target = Some(target);
            state.release_at = Some(release);
            state.ready_at = now + 2.6 + (e.to_bits() % 11) as f64 * 0.027;
            rotation.set_if_neq(PlayerRotation(f32::atan2(-velocity.x, -velocity.z)));
            activity.set_if_neq(CharacterActivity::Fighting);
            commands.entity(e).insert((
                BowShot {
                    release_at: release,
                },
                CombatReady,
            ));
            if let Ok((_, _, _, Some(id), ..)) = field.bodies.get(target) {
                commands.entity(e).insert(EngagedWith(*id));
            }
            break;
        }
    }
}
