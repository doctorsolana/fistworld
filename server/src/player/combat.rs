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
use lightyear::prelude::server::ClientOf;
use lightyear::prelude::{MessageReceiver, RemoteId};

use shared::components::{
    CharacterActivity, CharacterAttributes, CharacterKind, CharacterMotion, CommandedBy,
    DeathCause, Health, PlayerPosition, PlayerRotation, WorldTime,
};
use shared::protocol::{UnitAttackOrder, MAX_UNITS_PER_ORDER};

use shared::components::AboardBoat;

use crate::player::hero::{MoveTarget, OfflineHero};
use crate::world::village::mortality::PendingDeathCause;
use crate::world::village::PlayerConstructionAssignment;
use crate::world::village::{ConstructionMaterialRoutine, UnderConstruction};
use crate::world::village_roads::{NavigationRouteFailed, NavigationRoutePending, TravelRoute};

/// Arm's length plus a sidearm. Real weapon kinds arrive in milestone 2.
const MELEE_REACH: f32 = 2.0;
const SWING_SECONDS: f64 = 0.8;
const BASE_SWING_DAMAGE: f32 = 14.0;
/// A high-warp tick can span many swing budgets; cap the catch-up so a 1000x
/// tick cannot resolve an entire duel invisibly between two frames.
const MAX_SWINGS_PER_TICK: f64 = 4.0;
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
}

/// A physique-8 recruit swings at ~0.75x, a physique-100 veteran at ~1.3x.
fn swing_damage(attributes: &CharacterAttributes) -> f32 {
    BASE_SWING_DAMAGE * (0.7 + f32::from(attributes.physique()) / 100.0 * 0.6)
}

fn world_clock_seconds(clock: &WorldTime) -> f64 {
    f64::from(clock.day) * f64::from(clock.cycle_duration()) + f64::from(clock.seconds_in_cycle)
}

fn facing_toward(from: Vec3, to: Vec3) -> f32 {
    let direction = Vec2::new(to.x - from.x, to.z - from.z);
    f32::atan2(-direction.x, -direction.y)
}

/// Accept attack orders with exactly the movement-order authority rules.
///
/// No dev gate: striking with your own hero and retinue is a normal gameplay
/// verb; what it is *wise* to do in a town full of witnesses is a later
/// problem for law and reputation systems.
#[allow(clippy::type_complexity)]
pub fn handle_unit_attack_orders(
    mut commands: Commands,
    profiles: Res<crate::persistence::profiles::PlayerProfiles>,
    mut client_links: Query<(&RemoteId, &mut MessageReceiver<UnitAttackOrder>), With<ClientOf>>,
    attackers: Query<
        (&CommandedBy, Option<&PlayerConstructionAssignment>),
        (
            With<CharacterKind>,
            Without<OfflineHero>,
            Without<AboardBoat>,
        ),
    >,
    targets: Query<(&Health, Option<&CommandedBy>), With<CharacterKind>>,
    mut sites: Query<&mut UnderConstruction>,
) {
    for (remote_id, mut receiver) in client_links.iter_mut() {
        let account = profiles.peer_to_name.get(&remote_id.0).cloned();
        for order in receiver.receive() {
            // Drain even for an unnamed peer, or the queue backs up forever.
            let Some(account) = account.as_deref() else {
                continue;
            };
            if order.target == Entity::PLACEHOLDER {
                continue;
            }
            let Ok((target_health, target_owner)) = targets.get(order.target) else {
                continue;
            };
            if target_health.is_dead() {
                continue;
            }
            // No friendly fire: your own people can never be attack targets,
            // so a mis-click in a crowd cannot knife your own retinue.
            if target_owner.is_some_and(|owner| owner.0 == account) {
                continue;
            }
            for unit in order.units.iter().take(MAX_UNITS_PER_ORDER) {
                if *unit == Entity::PLACEHOLDER || *unit == order.target {
                    continue;
                }
                let Ok((commanded, construction)) = attackers.get(*unit) else {
                    continue;
                };
                if commanded.0 != account {
                    continue;
                }
                if let Some(construction) = construction {
                    if let Ok(mut site) = sites.get_mut(construction.site) {
                        if site.builder == Some(*unit) {
                            site.builder = None;
                        }
                    }
                    commands
                        .entity(*unit)
                        .remove::<PlayerConstructionAssignment>()
                        .remove::<ConstructionMaterialRoutine>();
                }
                commands
                    .entity(*unit)
                    .insert(AttackOrder {
                        target: order.target,
                    })
                    .remove::<MoveTarget>()
                    .remove::<TravelRoute>()
                    .remove::<NavigationRoutePending>()
                    .remove::<NavigationRouteFailed>();
            }
        }
    }
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
        Without<OfflineHero>,
    >,
    mut targets: Query<(&PlayerPosition, &mut Health), With<CharacterKind>>,
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
        cooldown,
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
                .remove::<MeleeCooldown>()
                .remove::<MoveTarget>();
        };
        let Ok((target_position, mut target_health)) = targets.get_mut(order.target) else {
            stand_down(&mut commands);
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
            activity.set_if_neq(CharacterActivity::Idle);
            if motion.is_moving() {
                *motion = CharacterMotion::STATIONARY;
            }
            continue;
        }
        let distance = Vec2::new(position.0.x, position.0.z)
            .distance(Vec2::new(target_position.0.x, target_position.0.z));
        if distance > MELEE_REACH {
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
        let Some(mut cooldown) = cooldown else {
            // First contact: arm the clock, swing from the next tick on.
            commands
                .entity(attacker)
                .insert(MeleeCooldown { ready_at: now });
            continue;
        };
        if cooldown.ready_at < now - SWING_SECONDS * MAX_SWINGS_PER_TICK {
            cooldown.ready_at = now - SWING_SECONDS * MAX_SWINGS_PER_TICK;
        }
        while cooldown.ready_at <= now {
            cooldown.ready_at += SWING_SECONDS;
            if target_health.take_damage(swing_damage(&attributes)) {
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
                stand_down(&mut commands);
                activity.set_if_neq(CharacterActivity::Idle);
                break;
            }
        }
    }
}

/// Which side a character fights for, if any. `None` never fights and is
/// never fought - the design pin that keeps bystanders out of every battle.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Allegiance<'a> {
    Banner(u8),
    Account(&'a str),
}

fn allegiance_of<'a>(
    war_party: Option<&WarParty>,
    commanded: Option<&'a CommandedBy>,
) -> Option<Allegiance<'a>> {
    match (commanded, war_party) {
        (Some(commanded), _) => Some(Allegiance::Account(commanded.0.as_str())),
        (None, Some(party)) => Some(Allegiance::Banner(party.banner)),
        (None, None) => None,
    }
}

/// Idle combatants engage the nearest hostile in reach on their own: this is
/// what turns "attack that one raider" into a battle line, because every
/// soldier who closes in picks their OWN opponent and every raider fights
/// back. A standing player order is never overridden - acquisition only
/// fills empty hands.
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
            Option<&Health>,
        ),
        (
            With<CharacterKind>,
            Without<OfflineHero>,
            Without<AboardBoat>,
        ),
    >,
) {
    // O(sides^2) over combat-capable characters only; a peaceful world exits
    // after one pass because it never sees two allegiances.
    let mut sides: Vec<(Entity, Vec2, Allegiance, bool)> = Vec::new();
    let mut factions: Vec<Allegiance> = Vec::new();
    for (entity, position, war_party, commanded, has_order, health) in combatants.iter() {
        if health.is_some_and(|health| health.is_dead()) {
            continue;
        }
        let Some(side) = allegiance_of(war_party, commanded) else {
            continue;
        };
        if !factions.contains(&side) {
            factions.push(side);
        }
        sides.push((
            entity,
            Vec2::new(position.0.x, position.0.z),
            side,
            has_order,
        ));
    }
    if factions.len() < 2 {
        return;
    }
    for (entity, point, side, has_order) in sides.iter() {
        if *has_order {
            continue;
        }
        let mut nearest: Option<(Entity, f32)> = None;
        for (other, other_point, other_side, _) in sides.iter() {
            if side == other_side {
                continue;
            }
            let distance = point.distance(*other_point);
            if distance > ACQUISITION_RANGE {
                continue;
            }
            if nearest.is_none_or(|(_, best)| distance < best) {
                nearest = Some((*other, distance));
            }
        }
        if let Some((target, _)) = nearest {
            commands.entity(*entity).insert(AttackOrder { target });
        }
    }
}

/// Melee bodies never overlap: any two combatants closer than two body radii
/// are pushed apart, half each, capped per tick. This is what makes a fight
/// a FRONT LINE - the second rank physically cannot occupy the first rank's
/// ground, so it holds behind or slides around the flanks - and it is scoped
/// to combatants so the tuned civilian flows (queues, doorways, markets) are
/// never disturbed.
#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub fn separate_melee_bodies(
    terrain: Option<Res<shared::terrain::WorldTerrain>>,
    obstacles: Option<Res<shared::spatial::SpatialObstacleGrid>>,
    colliders: Option<Res<crate::collision::library::StaticColliders>>,
    derived: Option<Res<crate::collision::library::DerivedColliderLibrary>>,
    mut bodies: Query<
        (
            Entity,
            &CharacterKind,
            &mut PlayerPosition,
            &mut shared::region::RegionCoord,
            Option<&WarParty>,
            Option<&CommandedBy>,
        ),
        (
            With<CharacterKind>,
            Without<OfflineHero>,
            Without<AboardBoat>,
            Without<crate::world::village::strategic::StrategicPerson>,
        ),
    >,
) {
    // Combatants only, hashed into coarse cells so the pair pass is local.
    const CELL: f32 = 2.0;
    let mut participants: Vec<(Entity, Vec2)> = Vec::new();
    for (entity, _, position, _, war_party, commanded) in bodies.iter() {
        if allegiance_of(war_party, commanded).is_none() {
            continue;
        }
        participants.push((entity, Vec2::new(position.0.x, position.0.z)));
    }
    if participants.len() < 2 {
        return;
    }
    let mut cells: std::collections::HashMap<(i32, i32), Vec<usize>> =
        std::collections::HashMap::new();
    for (index, (_, point)) in participants.iter().enumerate() {
        cells
            .entry((
                (point.x / CELL).floor() as i32,
                (point.y / CELL).floor() as i32,
            ))
            .or_default()
            .push(index);
    }

    let min_distance = BODY_RADIUS * 2.0;
    let mut pushes: std::collections::HashMap<Entity, Vec2> = std::collections::HashMap::new();
    for (index, (entity, point)) in participants.iter().enumerate() {
        let cell = (
            (point.x / CELL).floor() as i32,
            (point.y / CELL).floor() as i32,
        );
        for dx in -1..=1 {
            for dz in -1..=1 {
                let Some(neighbors) = cells.get(&(cell.0 + dx, cell.1 + dz)) else {
                    continue;
                };
                for other_index in neighbors {
                    // Each pair once.
                    if *other_index <= index {
                        continue;
                    }
                    let (other, other_point) = participants[*other_index];
                    let offset = *point - other_point;
                    let distance = offset.length();
                    if distance >= min_distance - SEPARATION_SLACK {
                        continue;
                    }
                    // Exactly coincident bodies fan out along a direction
                    // derived from the pair, deterministic across both ends.
                    let axis = if distance > 1.0e-4 {
                        offset / distance
                    } else {
                        let a = entity.to_bits().min(other.to_bits());
                        let b = entity.to_bits().max(other.to_bits());
                        let mixed = (a ^ (b.rotate_left(17))).wrapping_mul(0x9E37_79B9_7F4A_7C15);
                        let angle = (mixed as u32) as f32 / u32::MAX as f32 * std::f32::consts::TAU;
                        Vec2::new(angle.cos(), angle.sin())
                    };
                    let correction = ((min_distance - distance) * 0.5).min(MAX_PUSH_PER_TICK);
                    *pushes.entry(*entity).or_default() += axis * correction;
                    *pushes.entry(other).or_default() -= axis * correction;
                }
            }
        }
    }
    if pushes.is_empty() {
        return;
    }

    for (entity, kind, mut position, mut region, _, _) in bodies.iter_mut() {
        let Some(push) = pushes.get(&entity) else {
            continue;
        };
        let push = push.clamp_length_max(MAX_PUSH_PER_TICK);
        if push.length_squared() < 1.0e-8 {
            continue;
        }
        let current = Vec2::new(position.0.x, position.0.z);
        let next = current + push;
        // A shove must not put a villager inside a wall - that would hand
        // them to the route-failure machinery mid-fight. Heroes are as
        // collision-exempt here as they are in step_units.
        if *kind == CharacterKind::Villager
            && !crate::player::hero::navigation_segment_clear(
                current,
                next,
                obstacles.as_deref(),
                colliders.as_deref(),
                derived.as_deref(),
            )
        {
            continue;
        }
        let y = terrain
            .as_deref()
            .map(|terrain| terrain.get_height(next.x, next.y))
            .unwrap_or(position.0.y);
        let next_position = Vec3::new(next.x, y, next.y);
        if position.0 != next_position {
            position.0 = next_position;
        }
        let next_region = shared::region::RegionCoord::from_world_pos(next_position);
        if *region != next_region {
            *region = next_region;
        }
    }
}

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
