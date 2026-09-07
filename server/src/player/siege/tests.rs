use super::*;
use crate::player::{combat::world_clock_seconds, orders::apply_unit_order};
use shared::{
    protocol::{MovementMode, UnitOrder, UnitSelection},
    spatial::{ObstacleEntry, SpatialObstacleGrid},
};
fn fixture() -> (App, Entity, Entity) {
    let mut app = App::new();
    app.add_systems(
        Update,
        (advance_catapults, resolve_siege_projectiles).chain(),
    );
    let clock = app.world_mut().spawn(WorldTime::new_default()).id();
    let mut terrain = WorldTerrain::default();
    terrain.apply_flatten_rect(Vec3::new(0.0, 80.0, 0.0), Vec2::splat(150.0), 0.0, 4.0);
    app.insert_resource(terrain);
    let machine = spawn_catapult(
        &mut app.world_mut().commands(),
        "test",
        Vec3::new(0.0, 80.0, 0.0),
    );
    app.world_mut().flush();
    (app, clock, machine)
}
fn now(app: &App, clock: Entity) -> f64 {
    world_clock_seconds(app.world().get::<WorldTime>(clock).unwrap())
}
fn step(app: &mut App, clock: Entity, delta: f32) {
    let mut time = app.world_mut().get_mut::<WorldTime>(clock).unwrap();
    let total = time.seconds_in_cycle + delta;
    let cycle = time.cycle_duration();
    time.day += (total / cycle) as u32;
    time.seconds_in_cycle = total % cycle;
    drop(time);
    app.update();
}
fn order(app: &mut App, machine: Entity, command: UnitCommand) -> usize {
    apply_unit_order(
        app.world_mut(),
        "test",
        UnitOrder {
            selection: UnitSelection {
                units: vec![machine],
                battalions: vec![],
            },
            command,
        },
    )
    .0
}
fn aim() -> Vec3 {
    Vec3::new(0.0, 80.0, -60.0)
}
#[test]
fn ownership_bad_ranges_and_empty_ammo_do_not_interrupt_a_valid_order() {
    let (mut app, _, machine) = fixture();
    assert_eq!(
        order(
            &mut app,
            machine,
            UnitCommand::AttackGround { target: aim() }
        ),
        1
    );
    for point in [
        Vec3::NAN,
        Vec3::new(0.0, 80.0, -10.0),
        Vec3::new(0.0, 80.0, -200.0),
    ] {
        assert_eq!(
            order(
                &mut app,
                machine,
                UnitCommand::AttackGround { target: point }
            ),
            0
        );
        assert_eq!(
            app.world().get::<CatapultStatus>(machine).unwrap().aim,
            Some(aim())
        );
    }
    assert!(order_catapult(app.world_mut(), "other", machine, UnitCommand::Hold).is_err());
    app.world_mut()
        .get_mut::<Catapult>(machine)
        .unwrap()
        .ammunition = 0;
    assert_eq!(
        order(
            &mut app,
            machine,
            UnitCommand::AttackGround { target: aim() }
        ),
        0
    );
    assert!(app.world().get::<SiegeTarget>(machine).is_some());
}
#[test]
fn winding_can_be_cancelled_but_order_spam_cannot_skip_reload() {
    let (mut app, clock, machine) = fixture();
    order(
        &mut app,
        machine,
        UnitCommand::AttackGround { target: aim() },
    );
    step(&mut app, clock, 0.1);
    assert_eq!(
        app.world().get::<CatapultStatus>(machine).unwrap().phase,
        SiegePhase::Winding
    );
    order(&mut app, machine, UnitCommand::Hold);
    step(&mut app, clock, 2.0);
    assert_eq!(app.world().get::<Catapult>(machine).unwrap().ammunition, 20);
    order(
        &mut app,
        machine,
        UnitCommand::AttackGround { target: aim() },
    );
    step(&mut app, clock, 0.1);
    step(&mut app, clock, 1.7);
    assert_eq!(app.world().get::<Catapult>(machine).unwrap().ammunition, 19);
    let deadline = app.world().get::<CatapultStatus>(machine).unwrap().ready_at;
    for _ in 0..20 {
        order(&mut app, machine, UnitCommand::Hold);
        order(
            &mut app,
            machine,
            UnitCommand::AttackGround { target: aim() },
        );
        step(&mut app, clock, 0.1);
    }
    assert_eq!(app.world().get::<Catapult>(machine).unwrap().ammunition, 19);
    assert_eq!(
        app.world().get::<CatapultStatus>(machine).unwrap().ready_at,
        deadline
    );
    assert!(now(&app, clock) < deadline);
}
#[test]
fn impact_hits_multiple_friends_and_enemies_once_and_outlives_launcher() {
    let (mut app, clock, machine) = fixture();
    let mut victims = vec![];
    for (offset, owner) in [(0.0, "enemy"), (2.0, "test"), (7.0, "enemy")] {
        victims.push(
            app.world_mut()
                .spawn((
                    CharacterKind::Villager,
                    PlayerPosition(aim() + Vec3::X * offset),
                    Health::default(),
                    CommandedBy(owner.into()),
                ))
                .id(),
        );
    }
    order(
        &mut app,
        machine,
        UnitCommand::AttackGround { target: aim() },
    );
    step(&mut app, clock, 0.1);
    step(&mut app, clock, 1.7);
    assert_eq!(
        app.world_mut()
            .query::<&SiegeProjectile>()
            .iter(app.world())
            .count(),
        1
    );
    app.world_mut().despawn(machine);
    step(&mut app, clock, 4.0);
    let values: Vec<_> = victims
        .iter()
        .map(|e| app.world().get::<Health>(*e).unwrap().current)
        .collect();
    assert_eq!(values[0], 0.0);
    assert!(values[1] < 60.0 && values[1] > 40.0);
    assert_eq!(values[2], 100.0);
    assert!(matches!(
        app.world()
            .get::<crate::world::village::mortality::PendingDeathCause>(victims[0]),
        Some(crate::world::village::mortality::PendingDeathCause(
            DeathCause::Combat
        ))
    ));
    step(&mut app, clock, 1.0);
    assert_eq!(
        app.world().get::<Health>(victims[1]).unwrap().current,
        values[1]
    );
}
#[test]
fn production_mover_is_slow_and_wide_route_will_not_cut_a_corner() {
    let (mut app, clock, machine) = fixture();
    app.add_systems(
        Update,
        (
            crate::player::orders::advance_marches,
            crate::player::hero::step_units,
        )
            .chain()
            .before(advance_catapults),
    );
    let mut grid = SpatialObstacleGrid::new();
    grid.insert(ObstacleEntry {
        center: Vec2::new(1.3, -6.0),
        half_extents: Vec2::splat(0.2),
        rotation: 0.5,
        obstacle_type: 0,
    });
    assert!(!ground_clear(
        Vec2::ZERO,
        Vec2::new(0.0, -15.0),
        CATAPULT_CLEARANCE,
        Some(app.world().resource::<WorldTerrain>()),
        Some(&grid),
        None,
        None
    ));
    app.insert_resource(grid);
    assert_eq!(
        order(
            &mut app,
            machine,
            UnitCommand::Move {
                target: Vec3::new(0.0, 80.0, -18.0),
                frontage: None,
                mode: MovementMode::Move
            }
        ),
        1
    );
    let mut previous = Vec3::new(0.0, 80.0, 0.0);
    for _ in 0..1800 {
        step(&mut app, clock, 1.0 / 30.0);
        let p = app.world().get::<PlayerPosition>(machine).unwrap().0;
        assert!(p.xz().distance(previous.xz()) <= CATAPULT_SPEED / 30.0 + 0.005);
        assert!(!app
            .world()
            .resource::<SpatialObstacleGrid>()
            .segment_blocked_with_clearance(previous.xz(), p.xz(), CATAPULT_CLEARANCE));
        assert_eq!(app.world().get::<Catapult>(machine).unwrap().ammunition, 20);
        previous = p;
        if p.distance(Vec3::new(0.0, 80.0, -18.0)) < 0.25 {
            return;
        }
    }
    panic!("catapult failed to route around the obstacle, at {previous:?}");
}
#[test]
fn ridge_intercepts_the_same_ballistic_arc_rendered_by_the_client() {
    let (mut app, _, _) = fixture();
    let mut terrain = app.world_mut().remove_resource::<WorldTerrain>().unwrap();
    terrain.apply_flatten_rect(Vec3::new(0.0, 110.0, -35.0), Vec2::new(12.0, 4.0), 0.0, 1.0);
    let projectile = fire::trajectory(Vec3::new(0.0, 84.0, 0.0), aim(), 100.0, 1, Some(&terrain));
    assert!(
        projectile.impact.z > -45.0,
        "stone flew through ridge: {projectile:?}"
    );
    assert!(projectile.impact_at < projectile.launched_at + f64::from(projectile.flight_seconds));
    assert!(
        projectile
            .position(projectile.impact_at)
            .distance(projectile.impact)
            < 0.02
    );
}
#[test]
fn attack_ground_leaves_soldier_orders_alone_and_catapults_are_melee_targets() {
    let (mut app, _, machine) = fixture();
    let person = app
        .world_mut()
        .spawn((
            CharacterKind::Villager,
            PlayerPosition(aim()),
            PlayerRotation(0.0),
            Health::default(),
            CommandedBy("test".into()),
            crate::player::orders::CommandStance::Retreat,
        ))
        .id();
    apply_unit_order(
        app.world_mut(),
        "test",
        UnitOrder {
            selection: UnitSelection {
                units: vec![person, machine],
                battalions: vec![],
            },
            command: UnitCommand::AttackGround { target: aim() },
        },
    );
    assert_eq!(
        app.world()
            .get::<crate::player::orders::CommandStance>(person),
        Some(&crate::player::orders::CommandStance::Retreat)
    );
    app.world_mut().get_mut::<CommandedBy>(machine).unwrap().0 = "enemy".into();
    assert_eq!(
        apply_unit_order(
            app.world_mut(),
            "test",
            UnitOrder {
                selection: UnitSelection {
                    units: vec![person],
                    battalions: vec![]
                },
                command: UnitCommand::Attack {
                    target: machine,
                    mode: shared::protocol::AttackMode::Focus
                }
            }
        )
        .0,
        1
    );
    assert!(app
        .world()
        .get::<crate::player::combat::AttackOrder>(person)
        .is_some());
    assert!(app
        .world()
        .get::<crate::player::combat::SkirmishOrder>(person)
        .is_none());
}

#[test]
fn multiple_placements_in_one_frame_respect_the_cap_and_avoid_overlap() {
    let (mut app, _, _) = fixture();
    assert!(spawn_checked(app.world_mut(), "test", Vec3::new(1.0, 80.0, 0.0)).is_err());
    for i in 1..8 {
        assert!(spawn_checked(
            app.world_mut(),
            "test",
            Vec3::new(i as f32 * 7.0, 80.0, 0.0)
        )
        .is_ok());
    }
    assert!(spawn_checked(app.world_mut(), "test", Vec3::new(80.0, 80.0, 0.0)).is_err());
    assert_eq!(
        app.world_mut()
            .query::<&Catapult>()
            .iter(app.world())
            .count(),
        8
    );
}
#[test]
fn selected_catapults_arrive_in_a_spaced_line() {
    let (mut app, _, first) = fixture();
    let second = spawn_catapult(
        &mut app.world_mut().commands(),
        "test",
        Vec3::new(8.0, 80.0, 0.0),
    );
    app.world_mut().flush();
    let result = apply_unit_order(
        app.world_mut(),
        "test",
        UnitOrder {
            selection: UnitSelection {
                units: vec![first, second],
                battalions: vec![],
            },
            command: UnitCommand::Move {
                target: Vec3::new(0.0, 80.0, -30.0),
                frontage: None,
                mode: MovementMode::Move,
            },
        },
    );
    assert_eq!(result.0, 2);
    let a = app.world().get::<MarchOrder>(first).unwrap().destination;
    let b = app.world().get::<MarchOrder>(second).unwrap().destination;
    assert!(a.distance(b) > 6.0);
    assert!(((a + b) * 0.5).distance(Vec3::new(0.0, 80.0, -30.0)) < 0.001);
}
