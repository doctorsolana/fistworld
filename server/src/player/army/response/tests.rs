use super::*;
use crate::player::{
    army::{apply_army_order, BattalionLedger},
    orders::apply_unit_order,
};
use shared::region::RegionCoord;
fn lab() -> App {
    let mut app = App::new();
    app.init_resource::<BattalionLedger>();
    let mut terrain = shared::terrain::WorldTerrain::default();
    terrain.apply_flatten_rect(Vec3::new(0.0, 80.0, 0.0), Vec2::splat(150.0), 0.0, 4.0);
    app.insert_resource(terrain);
    app.world_mut().spawn(WorldTime::new_default());
    app
}
fn soldier(world: &mut World, owner: &str, p: Vec3) -> Entity {
    world
        .spawn((
            CharacterKind::Villager,
            CommandedBy(owner.into()),
            PlayerPosition(p),
            PlayerRotation(0.0),
            CharacterActivity::Idle,
            CharacterMotion::STATIONARY,
            CharacterAttributes::default(),
            Health::new(300.0),
            RegionCoord::default(),
        ))
        .id()
}
fn battalion(world: &mut World, owner: &str, members: &[Entity]) -> Entity {
    apply_army_order(
        world,
        owner,
        ArmyOrder::Muster {
            members: members.to_vec(),
        },
    );
    world
        .query::<(Entity, &Battalion, &CommandedBy)>()
        .iter(world)
        .filter(|(_, _, o)| o.0 == owner)
        .max_by_key(|(_, b, _)| b.ordinal)
        .unwrap()
        .0
}
fn impact(world: &mut World, at: Vec3, time: f64) {
    world.spawn((
        SiegeImpact {
            position: at,
            at: time,
            seed: 17,
        },
        UnansweredBombardment,
    ));
    react_to_bombardment(world);
}
#[test]
fn standing_policy_is_owned_inherited_and_removed_on_release() {
    let mut app = lab();
    let w = app.world_mut();
    let a = soldier(w, "alice", Vec3::new(0.0, 80.0, 0.0));
    let b = soldier(w, "alice", Vec3::new(2.0, 80.0, 0.0));
    let bat = battalion(w, "alice", &[a]);
    assert_eq!(
        apply_army_order(
            w,
            "bob",
            ArmyOrder::SetStance {
                battalion: bat,
                stance: BattalionStance::HoldLine
            }
        )
        .0,
        0
    );
    apply_army_order(
        w,
        "alice",
        ArmyOrder::SetStance {
            battalion: bat,
            stance: BattalionStance::HoldLine,
        },
    );
    apply_army_order(
        w,
        "alice",
        ArmyOrder::Assign {
            battalion: bat,
            members: vec![b],
        },
    );
    for e in [a, b, bat] {
        assert_eq!(
            w.get::<BattalionStance>(e),
            Some(&BattalionStance::HoldLine)
        );
    }
    apply_army_order(w, "alice", ArmyOrder::Dismiss { members: vec![b] });
    assert!(w.get::<BattalionStance>(b).is_none());
}
#[test]
fn impact_repositions_idle_battalion_and_independent_troop_but_not_hold_line() {
    let mut app = lab();
    let w = app.world_mut();
    let a = soldier(w, "alice", Vec3::new(-2.0, 80.0, 0.0));
    let b = soldier(w, "alice", Vec3::new(2.0, 80.0, 0.0));
    let lone = soldier(w, "alice", Vec3::new(0.0, 80.0, 4.0));
    let held = soldier(w, "alice", Vec3::new(0.0, 80.0, -4.0));
    battalion(w, "alice", &[a, b]);
    w.entity_mut(lone).insert(CommandStance::Hold);
    w.entity_mut(held).insert(BattalionStance::HoldLine);
    impact(w, Vec3::new(0.0, 80.0, 0.0), 10.0);
    for e in [a, b, lone] {
        assert!(w.get::<EvadingBombardment>(e).is_some());
        let p = w.get::<PlayerPosition>(e).unwrap().0;
        assert!(w.get::<MarchOrder>(e).unwrap().destination.distance(p) > 10.0);
    }
    assert!(w.get::<MoveTarget>(held).is_none());
    let group = w.get::<MarchOrder>(a).unwrap().group;
    assert_eq!(group, w.get::<MarchOrder>(b).unwrap().group);
    // Each impact is consumed once, and an ongoing escape is not replanned.
    react_to_bombardment(w);
    impact(w, Vec3::new(0.0, 80.0, 0.0), 11.0);
    assert_eq!(group, w.get::<MarchOrder>(a).unwrap().group);
    // A later impact may replan an escape that was blocked or is still exposed.
    impact(w, Vec3::new(0.0, 80.0, 0.0), 17.0);
    assert_ne!(group, w.get::<MarchOrder>(a).unwrap().group);
}
#[test]
fn direct_orders_win_and_switching_to_hold_line_stops_only_automatic_movement() {
    let mut app = lab();
    let w = app.world_mut();
    let a = soldier(w, "alice", Vec3::new(0.0, 80.0, 0.0));
    let b = soldier(w, "alice", Vec3::new(2.0, 80.0, 0.0));
    let bat = battalion(w, "alice", &[a, b]);
    let dest = Vec3::new(0.0, 80.0, 30.0);
    apply_unit_order(w, "alice", UnitOrder::move_to(vec![a, b], dest));
    let group = w.get::<MarchOrder>(a).unwrap().group;
    impact(w, Vec3::new(0.0, 80.0, 0.0), 10.0);
    apply_army_order(
        w,
        "alice",
        ArmyOrder::SetStance {
            battalion: bat,
            stance: BattalionStance::HoldLine,
        },
    );
    assert_eq!(group, w.get::<MarchOrder>(a).unwrap().group);
    assert!(w.get::<EvadingBombardment>(a).is_none());
    for e in [a, b] {
        w.entity_mut(e)
            .remove::<(MarchOrder, MoveTarget, fronts::FormationMember)>();
    }
    apply_army_order(
        w,
        "alice",
        ArmyOrder::SetStance {
            battalion: bat,
            stance: BattalionStance::Defensive,
        },
    );
    impact(w, Vec3::new(0.0, 80.0, 0.0), 20.0);
    assert!(w.get::<EvadingBombardment>(a).is_some());
    apply_army_order(
        w,
        "alice",
        ArmyOrder::SetStance {
            battalion: bat,
            stance: BattalionStance::HoldLine,
        },
    );
    for e in [a, b] {
        assert!(w.get::<MoveTarget>(e).is_none());
        assert!(w.get::<MarchOrder>(e).is_none());
    }
}
#[test]
fn explicit_attack_and_partial_battalion_march_are_not_overridden() {
    let mut app = lab();
    let w = app.world_mut();
    let a = soldier(w, "alice", Vec3::new(0.0, 80.0, 0.0));
    let b = soldier(w, "alice", Vec3::new(2.0, 80.0, 0.0));
    let enemy = soldier(w, "bob", Vec3::new(0.0, 80.0, 40.0));
    battalion(w, "alice", &[a, b]);
    apply_unit_order(
        w,
        "alice",
        UnitOrder {
            selection: UnitSelection {
                units: vec![a, b],
                battalions: vec![],
            },
            command: UnitCommand::Attack {
                target: enemy,
                mode: shared::protocol::AttackMode::Focus,
            },
        },
    );
    impact(w, Vec3::new(0.0, 80.0, 0.0), 10.0);
    assert!(w.get::<EvadingBombardment>(a).is_none());
    apply_unit_order(
        w,
        "alice",
        UnitOrder::move_to(vec![a], Vec3::new(0.0, 80.0, 30.0)),
    );
    w.entity_mut(b)
        .remove::<(fronts::FormationMember, AttackOrder)>();
    impact(w, Vec3::new(0.0, 80.0, 0.0), 20.0);
    assert!(w.get::<EvadingBombardment>(b).is_none());
}
#[test]
fn unreachable_escapes_do_not_replace_stationary_orders() {
    let mut app = lab();
    let w = app.world_mut();
    let a = soldier(w, "alice", Vec3::new(0.0, 80.0, 0.0));
    let mut obstacles = shared::spatial::SpatialObstacleGrid::default();
    obstacles.insert(shared::spatial::ObstacleEntry {
        obstacle_type: 0,
        center: Vec2::ZERO,
        half_extents: Vec2::splat(30.0),
        rotation: 0.0,
    });
    w.insert_resource(obstacles);
    impact(w, Vec3::new(0.0, 80.0, 0.0), 10.0);
    assert!(w.get::<MoveTarget>(a).is_none());
}

#[test]
fn a_distant_new_member_is_not_dragged_across_the_map_by_bombardment() {
    let mut app = lab();
    let w = app.world_mut();
    let near = soldier(w, "alice", Vec3::new(0.0, 80.0, 0.0));
    let far = soldier(w, "alice", Vec3::new(100.0, 80.0, 0.0));
    battalion(w, "alice", &[near, far]);
    impact(w, Vec3::new(0.0, 80.0, 0.0), 10.0);
    assert!(w.get::<EvadingBombardment>(near).is_some());
    assert!(w.get::<MoveTarget>(far).is_none());
}

fn movement_systems(app: &mut App) {
    app.init_resource::<fronts::CombatFormations>()
        .init_resource::<fronts::CombatSpace>();
    app.add_systems(
        Update,
        (
            crate::player::orders::advance_marches,
            fronts::rebuild_combat_space,
            fronts::advance_battle_fronts,
            crate::player::hero::step_units,
            fronts::rebuild_combat_space,
            fronts::assign_formation_contacts,
            crate::player::combat::acquire_targets,
            crate::player::combat::pursue_attack_orders,
            crate::player::combat::separate_melee_bodies,
        )
            .chain(),
    );
}
fn ticks(app: &mut App, count: usize) {
    for _ in 0..count {
        let w = app.world_mut();
        for mut time in w.query::<&mut WorldTime>().iter_mut(w) {
            time.seconds_in_cycle += 1.0 / 60.0;
        }
        app.update();
    }
}
#[test]
fn defensive_escape_uses_the_real_mover_and_settles_on_new_ground() {
    let mut app = lab();
    movement_systems(&mut app);
    let a = soldier(app.world_mut(), "alice", Vec3::new(-0.7, 80.0, 0.0));
    let b = soldier(app.world_mut(), "alice", Vec3::new(0.7, 80.0, 0.0));
    battalion(app.world_mut(), "alice", &[a, b]);
    impact(app.world_mut(), Vec3::new(0.0, 80.0, 0.0), 10.0);
    ticks(&mut app, 600);
    for e in [a, b] {
        assert!(
            app.world()
                .get::<PlayerPosition>(e)
                .unwrap()
                .0
                .xz()
                .length()
                > 10.0
        );
        assert!(app.world().get::<MarchOrder>(e).is_none());
        assert!(app.world().get::<EvadingBombardment>(e).is_none());
        assert_eq!(
            app.world().get::<CommandStance>(e),
            Some(&CommandStance::Guard)
        );
    }
}
#[test]
fn hold_line_resists_close_enemies_and_body_pushes_but_obeys_an_explicit_move() {
    let mut app = lab();
    movement_systems(&mut app);
    let start = Vec3::new(0.0, 80.0, 0.0);
    let a = soldier(app.world_mut(), "alice", start);
    let bat = battalion(app.world_mut(), "alice", &[a]);
    apply_army_order(
        app.world_mut(),
        "alice",
        ArmyOrder::SetStance {
            battalion: bat,
            stance: BattalionStance::HoldLine,
        },
    );
    let enemy = soldier(app.world_mut(), "bob", start + Vec3::X * 0.55);
    ticks(&mut app, 60);
    assert_eq!(app.world().get::<PlayerPosition>(a).unwrap().0, start);
    assert!(
        app.world().get::<Health>(enemy).unwrap().current < 300.0,
        "held troops still fight within reach"
    );
    apply_unit_order(
        app.world_mut(),
        "alice",
        UnitOrder::move_to(vec![a], start + Vec3::Z * 20.0),
    );
    ticks(&mut app, 120);
    assert!(
        app.world()
            .get::<PlayerPosition>(a)
            .unwrap()
            .0
            .distance(start)
            > 3.0
    );
}
