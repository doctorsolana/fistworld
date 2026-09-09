//! Player-ordered melee: the first slice of the battle system
//! (docs/COMBAT-DESIGN.md, milestone 1).
//!
//! An attack order is a normal command-authority verb, exactly like a move
//! order: the server re-validates that every attacker is commanded by the
//! sender's account and that the target is a live character the sender does
//! not command. Pursuit and swings run in the shared Navigation chain,
//! warp-correct on the world clock, and damage lands only through
//! [`Health::take_damage`], so the existing mortality pipeline settles the
//! aftermath - estate, business listings, obituary ledger - unchanged.
//! Written fresh for the RTS: nothing here descends from the stripped
//! FPS-era combat.

use bevy::prelude::*;

use shared::components::{
    Catapult, CharacterActivity, CharacterAttributes, CharacterKind, CharacterMotion, CommandedBy,
    DeathCause, Health, PlayerPosition, PlayerRotation, WorldTime,
};

use shared::components::AboardBoat;

use crate::player::hero::{MoveTarget, OfflineHero};
use crate::world::village::mortality::PendingDeathCause;

/// Arm's length plus a sidearm. Real weapon kinds arrive in milestone 2.
const MELEE_REACH: f32 = 2.0;
const SWING_SECONDS: f64 = 0.8;
const BASE_SWING_DAMAGE: f32 = 14.0;
/// A high-warp tick can span many swing budgets; cap the catch-up so a 1000x
/// tick cannot resolve an entire duel invisibly between two frames.
const MAX_SWINGS_PER_TICK: usize = 4;
/// Re-aim the chase only when the target has drifted this far from the
/// current goal, so a fleeing target does not churn `MoveTarget` every step.
const CHASE_REAIM_DISTANCE_SQUARED: f32 = 0.75 * 0.75;

/// A standing order to close with and strike one character until they fall,
/// the order is replaced, or the attacker is given something else to do.
#[derive(Component, Debug, Clone, Copy)]
pub struct AttackOrder {
    pub target: Entity,
}

/// Server-only opt-in hostility (COMBAT-DESIGN.md). Nothing is hostile by
/// accident: a character fights only if it carries a war party banner or is
/// commanded by a player, and only against a DIFFERENT allegiance. Ordinary
/// villagers have neither and are never auto-targeted by anyone.
#[derive(Component, Debug, Clone, Copy, PartialEq, Eq)]
pub struct WarParty {
    pub banner: u8,
}

/// How close a hostile must come before a combatant engages on its own.
/// Short on purpose: soldiers defend themselves and hold a melee together,
/// they do not chase every enemy they can see - that is the player's call.
const ACQUISITION_RANGE: f32 = 9.0;

/// Two bodies in a melee may never share ground: each keeps this much
/// radius. Matches VILLAGER_PROP_RADIUS, the canonical embodied clearance.
const BODY_RADIUS: f32 = 0.35;
/// A push is positional, applied per tick; the cap keeps a deep pile from
/// teleporting its rim outward in one frame.
const MAX_PUSH_PER_TICK: f32 = 0.22;
/// Below this penetration nobody moves, so a settled front line generates
/// zero replication traffic.
const SEPARATION_SLACK: f32 = 0.02;

/// Server-only swing clock in absolute world seconds; deliberately its own
/// component so future weapon swapping cannot reset a swing in progress.
#[derive(Component, Debug, Clone, Copy)]
pub struct MeleeCooldown {
    ready_at: f64,
    engaged: bool,
}

impl MeleeCooldown {
    pub(crate) fn disengage(&mut self) {
        self.engaged = false;
    }
}

/// A physique-8 recruit swings at ~0.75x, a physique-100 veteran at ~1.3x.
fn swing_damage(attributes: &CharacterAttributes) -> f32 {
    BASE_SWING_DAMAGE * (0.7 + f32::from(attributes.physique()) / 100.0 * 0.6)
}

pub(crate) fn world_clock_seconds(clock: &WorldTime) -> f64 {
    f64::from(clock.day) * f64::from(clock.cycle_duration()) + f64::from(clock.seconds_in_cycle)
}

fn facing_toward(from: Vec3, to: Vec3) -> f32 {
    let direction = Vec2::new(to.x - from.x, to.z - from.z);
    f32::atan2(-direction.x, -direction.y)
}

/// Close with the ordered target and trade blows once in reach.
///
/// Runs in the Navigation chain: chase goals written here are stepped by
/// `step_units` on the following tick. Every replicated write is guarded -
/// activity and rotation flip only on real transitions, and Health is written
/// only when a swing actually lands.
#[allow(clippy::type_complexity)]
pub fn pursue_attack_orders(
    mut commands: Commands,
    world_time: Query<&WorldTime>,
    identities: Query<&shared::components::PersonId>,
    stances: Query<&crate::player::orders::CommandStance>,
    formations: Query<(), With<fronts::FormationMember>>,
    space: Option<Res<fronts::CombatSpace>>,
    swing_visuals: Query<&shared::components::CombatSwing>,
    engagements: Query<&shared::components::EngagedWith>,
    skirmishers: Query<(), With<SkirmishOrder>>,
    policies: Query<&shared::components::BattalionStance>,
    directed: Query<(), With<crate::player::army::DirectedAttack>>,
    mounted: Query<(), With<shared::components::Mounted>>,
    mut attackers: Query<
        (
            Entity,
            &AttackOrder,
            &PlayerPosition,
            Option<&MoveTarget>,
            Option<&mut MeleeCooldown>,
            &mut PlayerRotation,
            &mut CharacterActivity,
            &mut CharacterAttributes,
            &mut CharacterMotion,
        ),
        (
            Without<OfflineHero>,
            Without<AboardBoat>,
            Without<shared::components::BowEquipped>,
        ),
    >,
    mut targets: Query<
        (&PlayerPosition, &mut Health),
        (
            Or<(With<CharacterKind>, With<Catapult>)>,
            Without<AboardBoat>,
            Without<OfflineHero>,
        ),
    >,
) {
    let Some(clock) = world_time.iter().next() else {
        return;
    };
    let now = world_clock_seconds(clock);
    for (
        attacker,
        order,
        position,
        move_target,
        mut cooldown,
        mut rotation,
        mut activity,
        mut attributes,
        mut motion,
    ) in attackers.iter_mut()
    {
        let stand_down = |commands: &mut Commands| {
            commands
                .entity(attacker)
                .remove::<AttackOrder>()
                .remove::<crate::player::army::DirectedAttack>()
                .remove::<shared::components::EngagedWith>()
                .remove::<MoveTarget>();
        };
        // Earlier attackers in this same pass may already have killed us.
        if targets
            .get(attacker)
            .is_ok_and(|(_, health)| health.is_dead())
        {
            stand_down(&mut commands);
            if let Some(clock) = cooldown.as_mut() {
                clock.disengage();
            }
            activity.set_if_neq(CharacterActivity::Idle);
            motion.set_if_neq(CharacterMotion::STATIONARY);
            continue;
        }
        let Ok((target_position, mut target_health)) = targets.get_mut(order.target) else {
            stand_down(&mut commands);
            if let Some(clock) = cooldown.as_mut() {
                clock.disengage();
            }
            activity.set_if_neq(CharacterActivity::Idle);
            // Same mid-stride problem as entering reach: a target that dies
            // or despawns during the chase leaves the walk velocity as the
            // last replicated motion unless it is parked here.
            if motion.is_moving() {
                *motion = CharacterMotion::STATIONARY;
            }
            continue;
        };
        if target_health.is_dead() {
            stand_down(&mut commands);
            if let Some(clock) = cooldown.as_mut() {
                clock.disengage();
            }
            activity.set_if_neq(CharacterActivity::Idle);
            if motion.is_moving() {
                *motion = CharacterMotion::STATIONARY;
            }
            continue;
        }
        let distance = Vec2::new(position.0.x, position.0.z)
            .distance(Vec2::new(target_position.0.x, target_position.0.z));
        let radius = |e| {
            if mounted.contains(e) {
                shared::components::HORSE_BODY_RADIUS
            } else {
                BODY_RADIUS
            }
        };
        let reach = (radius(attacker) + radius(order.target) + 0.5).max(MELEE_REACH);
        if distance > reach
            || space.as_ref().is_some_and(|s| {
                s.body(attacker).is_some()
                    && s.body(order.target).is_some()
                    && !s.clear_strike(attacker, order.target)
            })
        {
            if let Some(clock) = cooldown.as_mut() {
                clock.disengage();
            }
            if formations.contains(attacker)
                || (!directed.contains(attacker)
                    && policies
                        .get(attacker)
                        .is_ok_and(|s| *s == shared::components::BattalionStance::HoldLine))
                || stances
                    .get(attacker)
                    .is_ok_and(|s| *s == crate::player::orders::CommandStance::Hold)
            {
                stand_down(&mut commands);
                activity.set_if_neq(CharacterActivity::Idle);
                motion.set_if_neq(CharacterMotion::STATIONARY);
                continue;
            }
            if skirmishers.contains(attacker)
                && space
                    .as_ref()
                    .is_some_and(|s| s.body(order.target).is_some())
            {
                continue;
            }
            let goal = target_position.0;
            if move_target
                .is_none_or(|target| target.0.distance_squared(goal) > CHASE_REAIM_DISTANCE_SQUARED)
            {
                commands.entity(attacker).insert(MoveTarget(goal));
            }
            continue;
        }
        // In reach: stand and fight. Reach (2.0m) is hit MID-STRIDE - well
        // before step_units' 0.15m arrival write - so the motion must be
        // parked here too, or the last replicated velocity stays a full walk
        // and the client plays walk-in-place over the fight forever (nothing
        // else resets a unit whose MoveTarget was removed out from under it;
        // settle_villagers_without_targets deliberately skips heroes).
        if move_target.is_some() {
            commands.entity(attacker).remove::<MoveTarget>();
        }
        if motion.is_moving() {
            *motion = CharacterMotion::STATIONARY;
        }
        rotation.set_if_neq(PlayerRotation(facing_toward(position.0, target_position.0)));
        activity.set_if_neq(CharacterActivity::Fighting);
        if let Ok(person) = identities.get(order.target) {
            if !engagements.get(attacker).is_ok_and(|e| e.0 == *person) {
                commands
                    .entity(attacker)
                    .insert(shared::components::EngagedWith(*person));
            }
        }
        let Some(mut cooldown) = cooldown else {
            // First contact: arm the clock, swing from the next tick on.
            commands.entity(attacker).insert(MeleeCooldown {
                ready_at: now
                    + f64::from(shared::components::COMBAT_WINDUP_SECONDS)
                    + (attacker.to_bits() % 11) as f64 * 0.017,
                engaged: true,
            });
            continue;
        };
        if !cooldown.engaged {
            // A weapon can become ready while chasing, but missed contacts
            // are not attacks that can be stored and spent on arrival.
            cooldown.ready_at = cooldown.ready_at.max(now);
            cooldown.engaged = true;
        }
        let oldest = now - SWING_SECONDS * (MAX_SWINGS_PER_TICK - 1) as f64;
        cooldown.ready_at = cooldown.ready_at.max(oldest);
        // A swing is announced before its authoritative impact. It is not a
        // free-running animation loop; changing target cannot reset its clock.
        if now + f64::from(shared::components::COMBAT_WINDUP_SECONDS) >= cooldown.ready_at
            && !swing_visuals
                .get(attacker)
                .is_ok_and(|s| s.impact_at == cooldown.ready_at)
        {
            commands
                .entity(attacker)
                .insert(shared::components::CombatSwing {
                    impact_at: cooldown.ready_at,
                });
        }
        let mut swings = 0;
        while cooldown.ready_at <= now && swings < MAX_SWINGS_PER_TICK {
            cooldown.ready_at += SWING_SECONDS;
            swings += 1;
            let fatal = target_health.take_damage(swing_damage(&attributes));
            commands
                .entity(order.target)
                .insert(shared::components::CombatReaction { at: now, fatal });
            if fatal {
                // The honest cause for the obituary, and a lesson for the arm
                // that swung: kills train physique the way work trains it.
                commands
                    .entity(order.target)
                    .insert(PendingDeathCause(DeathCause::Combat));
                // Guarded read-first: at the cap train_physique is a no-op,
                // but reaching it through Mut::deref_mut would still dirty
                // the replicated component on every capped kill.
                if attributes.physique() < CharacterAttributes::MAX {
                    attributes.train_physique(1);
                }
                cooldown.disengage();
                stand_down(&mut commands);
                activity.set_if_neq(CharacterActivity::Idle);
                break;
            }
        }
    }
}

pub mod fronts;
mod skirmish;
pub use skirmish::{steer_skirmishers, DirectCombatApproach, SkirmishOrder};

mod targeting;
pub use targeting::acquire_targets;

mod separation;
pub use separation::separate_melee_bodies;

#[cfg(test)]
mod tests {
    use super::*;
    use shared::components::CharacterName;
    use shared::components::PersonId;

    fn advance_world_seconds(app: &mut App, seconds: f32) {
        let mut query = app.world_mut().query::<&mut WorldTime>();
        let mut clock = query.single_mut(app.world_mut()).unwrap();
        let total = clock.seconds_in_cycle + seconds;
        let cycle = clock.cycle_duration();
        clock.day += (total / cycle) as u32;
        clock.seconds_in_cycle = total % cycle;
    }

    fn spawn_duel(app: &mut App) -> (Entity, Entity) {
        app.world_mut().spawn(WorldTime::new_default());
        let target = app
            .world_mut()
            .spawn((
                CharacterName("Dummy".to_string()),
                PersonId(2),
                CharacterKind::Villager,
                PlayerPosition(Vec3::new(1.0, 0.0, 0.0)),
                Health::new(shared::components::CHARACTER_MAX_HEALTH),
            ))
            .id();
        let attacker = app
            .world_mut()
            .spawn((
                CharacterName("Hero".to_string()),
                PersonId(1),
                CharacterKind::Hero,
                PlayerPosition(Vec3::ZERO),
                PlayerRotation(0.0),
                CharacterActivity::Idle,
                CharacterAttributes::default(),
                // Arrives at full stride: the fight must park this itself.
                CharacterMotion {
                    velocity: Vec3::new(0.0, 0.0, shared::player::HERO_MOVE_SPEED),
                },
                AttackOrder { target },
            ))
            .id();
        (attacker, target)
    }

    #[test]
    fn mounted_opponents_trade_blows_without_pushing_their_horses_inside_each_other() {
        use shared::components::{Mounted, HorseGait, RidingPhase};
        let mut app = App::new();
        app.init_resource::<fronts::CombatSpace>();
        app.add_systems(Update, (fronts::rebuild_combat_space, pursue_attack_orders).chain());
        let (a, b) = spawn_duel(&mut app);
        for (entity, id, owner) in [(a, 1, "alice"), (b, 2, "bob")] {
            app.world_mut().entity_mut(entity).insert((
                Mounted { horse: id, gait: HorseGait::Gallop, phase: RidingPhase::Riding, since: 0. },
                PlayerRotation(0.), CommandedBy(owner.into()),
            ));
        }
        app.world_mut().get_mut::<PlayerPosition>(b).unwrap().0 = Vec3::X * 3.2;
        app.update();
        let space = app.world().resource::<fronts::CombatSpace>();
        assert!(space.clear_strike(a, b));
        assert!(!space.movement_clear(a, Vec2::ZERO, Vec2::X * 0.5));
        for _ in 0..8 {
            advance_world_seconds(&mut app, 0.25);
            app.update();
        }
        assert!(app.world().get::<Health>(b).unwrap().current < shared::components::CHARACTER_MAX_HEALTH);
        assert_eq!(app.world().get::<PlayerPosition>(b).unwrap().0.x, 3.2);
    }

    #[test]
    fn a_duel_in_reach_kills_the_target_and_stamps_the_combat_cause() {
        let mut app = App::new();
        app.add_systems(Update, pursue_attack_orders);
        let (attacker, target) = spawn_duel(&mut app);

        // ~14 damage per 0.8s swing against 100 health: eight world-seconds
        // of fighting is decisively lethal.
        for _ in 0..24 {
            advance_world_seconds(&mut app, 0.5);
            app.update();
        }

        let health = app.world().get::<Health>(target).unwrap();
        assert!(health.is_dead(), "the dummy must fall");
        assert!(matches!(
            app.world().get::<PendingDeathCause>(target),
            Some(PendingDeathCause(DeathCause::Combat))
        ));
        assert!(
            app.world().get::<AttackOrder>(attacker).is_none(),
            "the attacker stands down after the kill"
        );
        assert_eq!(
            app.world().get::<CharacterActivity>(attacker).copied(),
            Some(CharacterActivity::Idle)
        );
    }

    #[test]
    fn a_giant_warp_tick_is_capped_instead_of_resolving_a_whole_duel_unseen() {
        let mut app = App::new();
        app.add_systems(Update, pursue_attack_orders);
        let (_, target) = spawn_duel(&mut app);
        app.update(); // arm the cooldown at first contact

        // One enormous tick: sixty world-seconds at once.
        advance_world_seconds(&mut app, 60.0);
        app.update();

        let health = app.world().get::<Health>(target).unwrap();
        let expected = shared::components::CHARACTER_MAX_HEALTH
            - swing_damage(&CharacterAttributes::default()) * (MAX_SWINGS_PER_TICK as f32 + 1.0);
        assert!(
            health.current >= expected - 0.01,
            "catch-up must be capped: {} < {}",
            health.current,
            expected
        );
        assert!(!health.is_dead());
    }

    #[test]
    fn an_out_of_reach_attacker_chases_instead_of_swinging() {
        let mut app = App::new();
        app.add_systems(Update, pursue_attack_orders);
        app.world_mut().spawn(WorldTime::new_default());
        let target = app
            .world_mut()
            .spawn((
                CharacterKind::Villager,
                PlayerPosition(Vec3::new(30.0, 0.0, 0.0)),
                Health::new(shared::components::CHARACTER_MAX_HEALTH),
            ))
            .id();
        let attacker = app
            .world_mut()
            .spawn((
                CharacterKind::Hero,
                PlayerPosition(Vec3::ZERO),
                PlayerRotation(0.0),
                CharacterActivity::Idle,
                CharacterAttributes::default(),
                CharacterMotion::STATIONARY,
                AttackOrder { target },
            ))
            .id();

        app.update();

        assert_eq!(
            app.world()
                .get::<MoveTarget>(attacker)
                .map(|target| target.0),
            Some(Vec3::new(30.0, 0.0, 0.0)),
            "beyond reach the order becomes a chase"
        );
        assert_eq!(
            app.world().get::<Health>(target).unwrap().current,
            shared::components::CHARACTER_MAX_HEALTH
        );
    }

    /// The design pin from COMBAT-DESIGN.md: bystanders are outside every
    /// war. A villager with neither banner nor commander is never targeted,
    /// and never targets.
    #[test]
    fn a_villager_without_a_war_party_is_never_targeted() {
        let mut app = App::new();
        app.add_systems(Update, acquire_targets);
        let bystander = app
            .world_mut()
            .spawn((
                CharacterKind::Villager,
                PlayerPosition(Vec3::new(1.0, 0.0, 0.0)),
                Health::new(shared::components::CHARACTER_MAX_HEALTH),
            ))
            .id();
        let raider = app
            .world_mut()
            .spawn((
                CharacterKind::Villager,
                PlayerPosition(Vec3::ZERO),
                Health::new(shared::components::CHARACTER_MAX_HEALTH),
                WarParty { banner: 1 },
            ))
            .id();

        app.update();

        assert!(app.world().get::<AttackOrder>(raider).is_none());
        assert!(app.world().get::<AttackOrder>(bystander).is_none());
    }

    /// A raider band and a player's soldiers engage each other on their own
    /// once in range - this is what turns one ordered attack into a battle.
    #[test]
    fn hostile_sides_acquire_each_other_and_allies_never_do() {
        let mut app = App::new();
        app.add_systems(Update, acquire_targets);
        let soldier = app
            .world_mut()
            .spawn((
                CharacterKind::Villager,
                PlayerPosition(Vec3::ZERO),
                Health::new(shared::components::CHARACTER_MAX_HEALTH),
                CommandedBy("wanderer".to_string()),
            ))
            .id();
        let ally = app
            .world_mut()
            .spawn((
                CharacterKind::Villager,
                PlayerPosition(Vec3::new(1.0, 0.0, 0.0)),
                Health::new(shared::components::CHARACTER_MAX_HEALTH),
                CommandedBy("wanderer".to_string()),
            ))
            .id();
        let raider = app
            .world_mut()
            .spawn((
                CharacterKind::Villager,
                PlayerPosition(Vec3::new(0.0, 0.0, 5.0)),
                Health::new(shared::components::CHARACTER_MAX_HEALTH),
                WarParty { banner: 1 },
            ))
            .id();
        let distant_raider = app
            .world_mut()
            .spawn((
                CharacterKind::Villager,
                PlayerPosition(Vec3::new(0.0, 0.0, 300.0)),
                Health::new(shared::components::CHARACTER_MAX_HEALTH),
                WarParty { banner: 1 },
            ))
            .id();

        app.update();

        assert_eq!(
            app.world().get::<AttackOrder>(soldier).map(|o| o.target),
            Some(raider),
            "the soldier engages the raider in range"
        );
        assert_eq!(
            app.world()
                .get::<AttackOrder>(raider)
                .map(|o| o.target)
                .is_some(),
            true,
            "the raider fights back"
        );
        assert_ne!(
            app.world().get::<AttackOrder>(soldier).map(|o| o.target),
            Some(ally),
            "allies are never acquired"
        );
        assert!(
            app.world().get::<AttackOrder>(distant_raider).is_none(),
            "beyond acquisition range nothing engages"
        );
    }

    /// Melee bodies may not share ground: two overlapping combatants are
    /// pushed to body distance over a few ticks, and a settled pair stops
    /// generating corrections (and so replication traffic) entirely.
    #[test]
    fn overlapping_fighters_are_pushed_to_body_distance_and_settle() {
        let mut app = App::new();
        app.add_systems(Update, separate_melee_bodies);
        let a = app
            .world_mut()
            .spawn((
                CharacterKind::Villager,
                PlayerPosition(Vec3::new(0.05, 0.0, 0.0)),
                shared::region::RegionCoord::from_world_pos(Vec3::ZERO),
                CommandedBy("wanderer".to_string()),
            ))
            .id();
        let b = app
            .world_mut()
            .spawn((
                CharacterKind::Villager,
                PlayerPosition(Vec3::new(-0.05, 0.0, 0.0)),
                shared::region::RegionCoord::from_world_pos(Vec3::ZERO),
                WarParty { banner: 1 },
            ))
            .id();

        let distance = |app: &App| {
            let pa = app.world().get::<PlayerPosition>(a).unwrap().0;
            let pb = app.world().get::<PlayerPosition>(b).unwrap().0;
            Vec2::new(pa.x, pa.z).distance(Vec2::new(pb.x, pb.z))
        };

        app.update();
        let after_one = distance(&app);
        assert!(
            after_one > 0.1 + 2.0 * MAX_PUSH_PER_TICK - 0.01,
            "one tick pushes both bodies, capped: {after_one}"
        );
        for _ in 0..12 {
            app.update();
        }
        let settled = distance(&app);
        assert!(
            settled >= BODY_RADIUS * 2.0 - SEPARATION_SLACK - 0.001,
            "bodies end at body distance: {settled}"
        );
        let before = app.world().get::<PlayerPosition>(a).unwrap().0;
        app.update();
        assert_eq!(
            app.world().get::<PlayerPosition>(a).unwrap().0,
            before,
            "a settled pair holds still"
        );
    }

    /// The other mid-stride exit: a target that despawns during the chase.
    /// Stand-down must park the walk too, or the chaser marches in place at
    /// the spot where its quarry vanished.
    #[test]
    fn a_chaser_whose_target_vanishes_stands_still() {
        let mut app = App::new();
        app.add_systems(Update, pursue_attack_orders);
        let (attacker, target) = spawn_duel(&mut app);
        app.world_mut().entity_mut(target).despawn();

        app.update();

        assert_eq!(
            app.world().get::<CharacterMotion>(attacker).copied(),
            Some(CharacterMotion::STATIONARY)
        );
        assert!(app.world().get::<AttackOrder>(attacker).is_none());
    }

    /// Reach (2.0m) is hit mid-stride, long before step_units' 0.15m arrival
    /// write. If the fight does not park the motion itself, the last
    /// replicated velocity stays a full walk and every client plays
    /// walk-in-place over the duel instead of the swing.
    #[test]
    fn entering_reach_parks_the_walk_so_the_swing_can_play() {
        let mut app = App::new();
        app.add_systems(Update, pursue_attack_orders);
        let (attacker, _) = spawn_duel(&mut app);

        app.update();

        assert_eq!(
            app.world().get::<CharacterMotion>(attacker).copied(),
            Some(CharacterMotion::STATIONARY),
            "in reach, the attacker must stand to fight"
        );
        assert_eq!(
            app.world().get::<CharacterActivity>(attacker).copied(),
            Some(CharacterActivity::Fighting)
        );
    }
}

#[cfg(test)]
mod regression_tests;

/// Corpses have already settled their estate; this only bounds their visual
/// lifetime. Dead bodies never re-enter targeting or formation movement.
#[derive(Component)]
pub struct SettledCombatDeath;
pub fn expire_combat_bodies(
    mut commands: Commands,
    clock: Query<&WorldTime>,
    bodies: Query<(Entity, &shared::components::CombatReaction), With<SettledCombatDeath>>,
) {
    let Some(clock) = clock.iter().next() else {
        return;
    };
    let now = world_clock_seconds(clock);
    for (entity, reaction) in &bodies {
        if now - reaction.at >= 1.4 {
            commands.entity(entity).despawn();
        }
    }
}
