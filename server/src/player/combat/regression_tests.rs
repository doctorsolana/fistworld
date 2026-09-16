use super::*;
fn fighter(app: &mut App, point: Vec3, account: &str, health: f32) -> Entity {
    app.world_mut()
        .spawn((
            CharacterKind::Villager,
            PlayerPosition(point),
            PlayerRotation(0.0),
            CharacterActivity::Idle,
            CharacterAttributes::default(),
            CharacterMotion::STATIONARY,
            CommandedBy(account.to_string()),
            Health::new(health),
        ))
        .id()
}

fn tick_combat(app: &mut App, clock: Entity, ticks: usize) {
    for _ in 0..ticks {
        app.world_mut()
            .get_mut::<WorldTime>(clock)
            .unwrap()
            .seconds_in_cycle += 0.1;
        app.update();
    }
}

#[test]
fn different_player_heroes_stay_peaceful_after_meeting_and_stopping() {
    use crate::player::orders::CommandStance;
    let mut app = App::new();
    app.add_systems(Update, (acquire_targets, pursue_attack_orders).chain());
    let clock = app.world_mut().spawn(WorldTime::new_default()).id();
    let a = fighter(&mut app, Vec3::ZERO, "alice", 100.0);
    let b = fighter(&mut app, Vec3::X, "bob", 100.0);
    for e in [a, b] {
        app.world_mut().entity_mut(e).insert((
            CharacterKind::Hero,
            shared::components::PersonId(e.to_bits()),
            MoveTarget(Vec3::Z * 20.0),
            CommandStance::Move,
        ));
    }
    tick_combat(&mut app, clock, 20);
    for e in [a, b] {
        app.world_mut()
            .entity_mut(e)
            .remove::<MoveTarget>()
            .insert(CommandStance::Guard);
    }
    tick_combat(&mut app, clock, 30);
    for e in [a, b] {
        app.world_mut().entity_mut(e).remove::<CommandStance>();
    }
    tick_combat(&mut app, clock, 30);
    for e in [a, b] {
        assert!(app.world().get::<AttackOrder>(e).is_none());
        assert!(
            app.world()
                .get::<shared::components::EngagedWith>(e)
                .is_none()
        );
        assert_eq!(app.world().get::<Health>(e).unwrap().current, 100.0);
        assert_eq!(
            *app.world().get::<CharacterActivity>(e).unwrap(),
            CharacterActivity::Idle
        );
    }
}

#[test]
fn validated_attack_allows_retaliation_without_involving_a_third_player() {
    use crate::player::orders::apply_unit_order;
    use shared::protocol::{AttackMode, UnitCommand, UnitOrder, UnitSelection};
    let mut app = App::new();
    app.add_systems(Update, (acquire_targets, pursue_attack_orders).chain());
    let clock = app.world_mut().spawn(WorldTime::new_default()).id();
    let a = fighter(&mut app, Vec3::ZERO, "alice", 100.0);
    let b = fighter(&mut app, Vec3::X, "bob", 100.0);
    let bystander = fighter(&mut app, Vec3::Z * 0.5, "carol", 100.0);
    let attack = || UnitOrder {
        selection: UnitSelection {
            units: vec![a],
            battalions: vec![],
        },
        command: UnitCommand::Attack {
            target: b,
            mode: AttackMode::Focus,
        },
    };
    assert_eq!(apply_unit_order(app.world_mut(), "mallory", attack()).0, 0);
    tick_combat(&mut app, clock, 10);
    assert!(app.world().get::<AttackOrder>(b).is_none());
    assert_eq!(apply_unit_order(app.world_mut(), "alice", attack()).0, 1);
    tick_combat(&mut app, clock, 20);
    assert_eq!(
        app.world().get::<AttackOrder>(b).map(|order| order.target),
        Some(a)
    );
    assert!(app.world().get::<Health>(a).unwrap().current < 100.0);
    assert!(app.world().get::<Health>(b).unwrap().current < 100.0);
    assert!(app.world().get::<AttackOrder>(bystander).is_none());
    assert_eq!(app.world().get::<Health>(bystander).unwrap().current, 100.0);
    // Stopping both sides clears transient hostilities; proximity cannot restart it.
    for (entity, account) in [(a, "alice"), (b, "bob")] {
        assert_eq!(
            apply_unit_order(
                app.world_mut(),
                account,
                UnitOrder {
                    selection: UnitSelection {
                        units: vec![entity],
                        battalions: vec![]
                    },
                    command: UnitCommand::Hold,
                }
            )
            .0,
            1
        );
    }
    tick_combat(&mut app, clock, 20);
    assert!(app.world().get::<AttackOrder>(a).is_none());
    assert!(app.world().get::<AttackOrder>(b).is_none());
}

#[test]
fn explicit_attack_move_acquires_but_an_ordinary_move_still_takes_priority() {
    use crate::player::orders::CommandStance;
    let mut app = App::new();
    app.add_systems(Update, acquire_targets);
    let a = fighter(&mut app, Vec3::ZERO, "alice", 100.0);
    let b = fighter(&mut app, Vec3::X, "bob", 100.0);
    app.world_mut()
        .entity_mut(a)
        .insert(CommandStance::AttackMove);
    app.world_mut()
        .entity_mut(b)
        .insert((CommandStance::Move, MoveTarget(Vec3::Z * 20.0)));
    app.update();
    assert_eq!(
        app.world().get::<AttackOrder>(a).map(|order| order.target),
        Some(b)
    );
    assert!(app.world().get::<AttackOrder>(b).is_none());
    assert_eq!(app.world().get::<MoveTarget>(b).unwrap().0, Vec3::Z * 20.0);
}

#[test]
fn regression_retreat_destination_must_survive_nearby_enemy() {
    let mut app = App::new();
    app.add_systems(Update, (acquire_targets, pursue_attack_orders).chain());
    app.world_mut().spawn(WorldTime::new_default());
    let soldier = fighter(&mut app, Vec3::ZERO, "alice", 100.0);
    let enemy = fighter(&mut app, Vec3::X, "bob", 100.0);
    app.world_mut()
        .entity_mut(enemy)
        .insert(WarParty { banner: 1 });
    let retreat = Vec3::new(-30.0, 0.0, 0.0);
    // Exact postcondition of a valid move order: MoveTarget, no AttackOrder.
    app.world_mut()
        .entity_mut(soldier)
        .insert(MoveTarget(retreat));
    app.update();
    assert_eq!(
        app.world().get::<MoveTarget>(soldier).map(|m| m.0),
        Some(retreat)
    );
    assert!(app.world().get::<AttackOrder>(soldier).is_none());
}

#[test]
fn regression_a_fighter_killed_in_this_pass_must_not_strike_back() {
    let mut app = App::new();
    app.add_systems(Update, pursue_attack_orders);
    app.world_mut().spawn(WorldTime::new_default());
    let a = fighter(&mut app, Vec3::ZERO, "alice", 1.0);
    let b = fighter(&mut app, Vec3::X, "bob", 1.0);
    app.world_mut().entity_mut(a).insert((
        AttackOrder { target: b },
        MeleeCooldown {
            ready_at: 0.0,
            engaged: true,
        },
    ));
    app.world_mut().entity_mut(b).insert((
        AttackOrder { target: a },
        MeleeCooldown {
            ready_at: 0.0,
            engaged: true,
        },
    ));
    app.update();
    let survivors = [a, b]
        .into_iter()
        .filter(|e| !app.world().get::<Health>(*e).unwrap().is_dead())
        .count();
    assert_eq!(
        survivors, 1,
        "sequential killing blow should stop the victim's pending swing"
    );
}

#[test]
fn regression_normal_speed_pursuit_must_not_bank_missed_swings() {
    let mut app = App::new();
    app.add_systems(Update, pursue_attack_orders);
    let clock = app.world_mut().spawn(WorldTime::new_default()).id();
    let a = fighter(&mut app, Vec3::ZERO, "alice", 100.0);
    let b = fighter(&mut app, Vec3::new(30.0, 0.0, 0.0), "bob", 100.0);
    let now = world_clock_seconds(app.world().get::<WorldTime>(clock).unwrap());
    app.world_mut().entity_mut(a).insert((
        AttackOrder { target: b },
        MeleeCooldown {
            ready_at: now,
            engaged: true,
        },
    ));
    // Ordinary 60Hz frames spent outside reach, after a previous contact.
    for _ in 0..600 {
        app.world_mut()
            .get_mut::<WorldTime>(clock)
            .unwrap()
            .seconds_in_cycle += 1.0 / 60.0;
        app.update();
    }
    app.world_mut().get_mut::<PlayerPosition>(b).unwrap().0 = Vec3::X;
    app.update();
    let lost = 100.0 - app.world().get::<Health>(b).unwrap().current;
    assert!(
        lost <= swing_damage(&CharacterAttributes::default()) + 0.01,
        "banked damage on re-contact: {lost}"
    );
}

#[test]
fn replacing_an_attack_cannot_reset_a_ready_weapon_deadline() {
    use crate::player::orders::apply_unit_order;
    use shared::protocol::{UnitCommand, UnitOrder, UnitSelection};
    let mut app = App::new();
    app.add_systems(Update, pursue_attack_orders);
    let clock = app.world_mut().spawn(WorldTime::new_default()).id();
    let a = fighter(&mut app, Vec3::ZERO, "alice", 100.0);
    let b = fighter(&mut app, Vec3::X, "bob", 100.0);
    let now = world_clock_seconds(app.world().get::<WorldTime>(clock).unwrap());
    app.world_mut().entity_mut(a).insert(MeleeCooldown {
        ready_at: now + SWING_SECONDS,
        engaged: true,
    });
    for _ in 0..20 {
        apply_unit_order(
            app.world_mut(),
            "alice",
            UnitOrder {
                selection: UnitSelection {
                    units: vec![a],
                    battalions: vec![],
                },
                command: UnitCommand::Attack {
                    target: b,
                    mode: shared::protocol::AttackMode::Focus,
                },
            },
        );
        app.update();
    }
    assert_eq!(app.world().get::<Health>(b).unwrap().current, 100.0);
    assert_eq!(
        app.world().get::<MeleeCooldown>(a).unwrap().ready_at,
        now + SWING_SECONDS
    );
}

#[path = "observation_tests.rs"]
mod observation_tests;
