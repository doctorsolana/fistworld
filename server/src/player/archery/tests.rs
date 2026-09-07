use super::*;
use crate::player::{
    army::{apply_army_order, BattalionLedger},
    combat::fronts::rebuild_combat_space,
};
use shared::protocol::ArmyOrder;
fn lab() -> App {
    let mut app = App::new();
    app.init_resource::<CombatSpace>()
        .init_resource::<CombatFormations>()
        .init_resource::<ArrowObstacles>()
        .init_resource::<BattalionLedger>();
    app.world_mut().spawn(WorldTime::new_default());
    app.add_systems(
        Update,
        (
            rebuild_combat_space,
            update_weapons,
            shoot_bows,
            advance_arrows,
        )
            .chain(),
    );
    app
}
fn person(app: &mut App, owner: &str, p: Vec3) -> Entity {
    let e = app
        .world_mut()
        .spawn((
            CharacterKind::Villager,
            CommandedBy(owner.into()),
            PlayerPosition(p),
            PlayerRotation(std::f32::consts::PI),
            CharacterMotion::STATIONARY,
            CharacterActivity::Idle,
            Health::new(100.),
            CharacterAttributes::default(),
        ))
        .id();
    app.world_mut().entity_mut(e).insert(PersonId(e.to_bits()));
    e
}
fn archer(app: &mut App) -> Entity {
    let e = person(app, "alice", Vec3::ZERO);
    inherit_equipment(
        app.world_mut(),
        e,
        SoldierRole::Archer,
        FirePolicy::FireAtWill,
    );
    e
}
fn step(app: &mut App, dt: f32) {
    let w = app.world_mut();
    w.query::<&mut WorldTime>()
        .single_mut(w)
        .unwrap()
        .seconds_in_cycle += dt;
    app.update();
}
#[test]
fn draw_releases_once_and_a_real_arrow_deals_damage() {
    let mut app = lab();
    let a = archer(&mut app);
    let b = person(&mut app, "bob", Vec3::Z * 30.);
    step(&mut app, 0.1);
    assert!(app.world().get::<BowShot>(a).is_some());
    for _ in 0..8 {
        step(&mut app, 0.1);
    }
    assert_eq!(app.world().get::<Quiver>(a).unwrap().arrows, 24);
    for _ in 0..5 {
        step(&mut app, 0.1);
    }
    assert_eq!(app.world().get::<Quiver>(a).unwrap().arrows, 23);
    assert_eq!(app.world().get::<Health>(b).unwrap().current, 100.);
    for _ in 0..10 {
        step(&mut app, 0.1);
    }
    assert_eq!(app.world().get::<Health>(b).unwrap().current, 68.);
}
#[test]
fn moving_during_draw_cancels_without_spending_arrows() {
    let mut app = lab();
    let a = archer(&mut app);
    person(&mut app, "bob", Vec3::Z * 30.);
    step(&mut app, 0.1);
    app.world_mut()
        .get_mut::<CharacterMotion>(a)
        .unwrap()
        .velocity = Vec3::X * 3.;
    for _ in 0..20 {
        step(&mut app, 0.1);
    }
    assert!(app.world().get::<BowShot>(a).is_none());
    assert_eq!(app.world().get::<Quiver>(a).unwrap().arrows, 24);
}
#[test]
fn hold_fire_and_friendly_screens_prevent_draws() {
    let mut app = lab();
    let a = archer(&mut app);
    let b = person(&mut app, "bob", Vec3::Z * 30.);
    app.world_mut().entity_mut(a).insert(FirePolicy::HoldFire);
    step(&mut app, 0.1);
    assert!(app.world().get::<BowShot>(a).is_none());
    app.world_mut().entity_mut(a).insert((
        SkirmishOrder {
            target: Enemy::Person(b),
        },
        FirePolicy::FireAtWill,
    ));
    let screen = person(&mut app, "alice", Vec3::new(0.40, 0., 1.7));
    for _ in 0..10 {
        step(&mut app, 0.1);
    }
    assert!(app.world().get::<BowShot>(a).is_none());
    app.world_mut().despawn(screen);
    for _ in 0..10 {
        step(&mut app, 0.1);
    }
    assert!(app.world().get::<BowShot>(a).is_some());
}
#[test]
fn sidearm_hysteresis_and_empty_quiver() {
    let mut app = lab();
    let a = archer(&mut app);
    let b = person(&mut app, "bob", Vec3::Z * 4.);
    step(&mut app, 0.1);
    assert!(app.world().get::<BowEquipped>(a).is_none());
    app.world_mut().get_mut::<PlayerPosition>(b).unwrap().0.z = 12.;
    step(&mut app, 1.);
    assert!(app.world().get::<BowEquipped>(a).is_none());
    step(&mut app, 1.1);
    assert!(app.world().get::<BowEquipped>(a).is_some());
    app.world_mut().get_mut::<Quiver>(a).unwrap().arrows = 0;
    step(&mut app, 0.1);
    assert!(app.world().get::<BowEquipped>(a).is_none());
}
#[test]
fn membership_and_role_toggles_preserve_ammunition_and_rearm_is_guarded() {
    let mut app = lab();
    let a = person(&mut app, "alice", Vec3::ZERO);
    apply_army_order(
        app.world_mut(),
        "alice",
        ArmyOrder::Muster { members: vec![a] },
    );
    let w = app.world_mut();
    let bat = w
        .query_filtered::<Entity, With<Battalion>>()
        .single(w)
        .unwrap();
    assert_eq!(
        apply_army_order(
            w,
            "bob",
            ArmyOrder::SetRole {
                battalion: bat,
                role: SoldierRole::Archer
            }
        )
        .0,
        0
    );
    assert_eq!(
        apply_army_order(
            w,
            "alice",
            ArmyOrder::SetRole {
                battalion: bat,
                role: SoldierRole::Archer
            }
        )
        .0,
        1
    );
    w.get_mut::<Quiver>(a).unwrap().arrows = 3;
    for role in [SoldierRole::Infantry, SoldierRole::Archer] {
        apply_army_order(
            w,
            "alice",
            ArmyOrder::SetRole {
                battalion: bat,
                role,
            },
        );
    }
    assert_eq!(w.get::<Quiver>(a).unwrap().arrows, 3);
    let enemy = person(&mut app, "bob", Vec3::Z * 50.);
    assert_eq!(
        apply_army_order(
            app.world_mut(),
            "alice",
            ArmyOrder::Rearm { battalion: bat }
        )
        .0,
        0
    );
    app.world_mut().despawn(enemy);
    assert_eq!(
        apply_army_order(
            app.world_mut(),
            "alice",
            ArmyOrder::Rearm { battalion: bat }
        )
        .0,
        1
    );
    assert_eq!(app.world().get::<Quiver>(a).unwrap().arrows, 24);
    apply_army_order(
        app.world_mut(),
        "alice",
        ArmyOrder::Dismiss { members: vec![a] },
    );
    assert_eq!(
        app.world().get::<SoldierRole>(a),
        Some(&SoldierRole::Archer)
    );
    app.world_mut().get_mut::<Quiver>(a).unwrap().arrows = 7;
    apply_army_order(
        app.world_mut(),
        "alice",
        ArmyOrder::Muster { members: vec![a] },
    );
    assert_eq!(
        app.world().get::<SoldierRole>(a),
        Some(&SoldierRole::Archer)
    );
    assert_eq!(app.world().get::<Quiver>(a).unwrap().arrows, 7);
    let id = app.world().get::<MemberOfBattalion>(a).unwrap().0;
    let world = app.world_mut();
    assert_eq!(
        world
            .query::<(&Battalion, &SoldierRole)>()
            .iter(world)
            .find(|(b, _)| b.id == id)
            .unwrap()
            .1,
        &SoldierRole::Archer
    );
}
#[test]
fn a_detached_archer_attack_stops_at_bow_range_after_a_move_order() {
    use crate::player::{
        combat::fronts::advance_battle_fronts,
        hero::step_units,
        orders::{advance_marches, apply_unit_order},
    };
    use shared::{
        protocol::{AttackMode, UnitCommand, UnitOrder, UnitSelection},
        region::RegionCoord,
    };
    let mut app = App::new();
    app.init_resource::<CombatSpace>()
        .init_resource::<CombatFormations>()
        .init_resource::<ArrowObstacles>();
    let mut terrain = shared::terrain::WorldTerrain::default();
    terrain.apply_flatten_rect(Vec3::new(0., 80., 0.), Vec2::splat(120.), 0., 4.);
    app.insert_resource(terrain);
    app.world_mut().spawn(WorldTime::new_default());
    app.add_systems(
        Update,
        (
            advance_marches,
            rebuild_combat_space,
            update_weapons,
            advance_battle_fronts,
            step_units,
            rebuild_combat_space,
            shoot_bows,
            advance_arrows,
        )
            .chain(),
    );
    let a = archer(&mut app);
    app.world_mut().entity_mut(a).insert((
        PlayerPosition(Vec3::new(0., 80., -90.)),
        RegionCoord::default(),
        CommandStance::Move,
        FirePolicy::HoldFire,
    ));
    let b = person(&mut app, "bob", Vec3::new(0., 80., 0.));
    app.world_mut().entity_mut(b).insert(Health::new(10000.));
    assert_eq!(
        apply_unit_order(
            app.world_mut(),
            "alice",
            UnitOrder {
                selection: UnitSelection {
                    units: vec![a],
                    battalions: vec![]
                },
                command: UnitCommand::Attack {
                    target: b,
                    mode: AttackMode::Focus
                }
            }
        )
        .0,
        1
    );
    for _ in 0..1800 {
        step(&mut app, 1. / 60.);
    }
    let position = app.world().get::<PlayerPosition>(a).unwrap().0;
    assert!(
        (-50. ..-35.).contains(&position.z),
        "archer stopped at {position:?}"
    );
    assert!(
        app.world().get::<Quiver>(a).unwrap().arrows < 24,
        "archer must fire after approaching"
    );
}
#[test]
fn a_friendly_crossing_after_release_intercepts_the_arrow() {
    let mut app = lab();
    let a = archer(&mut app);
    let b = person(&mut app, "bob", Vec3::Z * 30.);
    for _ in 0..13 {
        step(&mut app, 0.1);
    }
    assert_eq!(app.world().get::<Quiver>(a).unwrap().arrows, 23);
    let friend = person(&mut app, "alice", Vec3::Z * 15.);
    // At 15 m the arrow is above head height; put this friend on a small rise.
    app.world_mut()
        .get_mut::<PlayerPosition>(friend)
        .unwrap()
        .0
        .y = 1.;
    for _ in 0..9 {
        step(&mut app, 0.1);
    }
    assert_eq!(app.world().get::<Health>(friend).unwrap().current, 68.);
    assert_eq!(app.world().get::<Health>(b).unwrap().current, 100.);
}
#[test]
fn incomplete_ranks_settle_without_sideways_drift_or_cancelled_draws() {
    use crate::player::{
        combat::fronts::advance_battle_fronts, hero::step_units, orders::apply_unit_order,
    };
    use shared::{
        protocol::{AttackMode, UnitCommand, UnitOrder, UnitSelection},
        region::RegionCoord,
    };
    let mut app = App::new();
    app.init_resource::<CombatSpace>()
        .init_resource::<CombatFormations>()
        .init_resource::<ArrowObstacles>()
        .init_resource::<BattalionLedger>();
    let mut terrain = shared::terrain::WorldTerrain::default();
    terrain.apply_flatten_rect(Vec3::new(0., 80., 0.), Vec2::splat(120.), 0., 4.);
    app.insert_resource(terrain);
    app.world_mut().spawn(WorldTime::new_default());
    app.add_systems(
        Update,
        (
            rebuild_combat_space,
            update_weapons,
            advance_battle_fronts,
            step_units,
            rebuild_combat_space,
            shoot_bows,
            advance_arrows,
        )
            .chain(),
    );
    let mut members = Vec::new();
    for i in 0..16 {
        let e = person(
            &mut app,
            "alice",
            Vec3::new(
                (i % 10) as f32 * 1.4 - 6.3,
                80.,
                -50. - (i / 10) as f32 * 1.7,
            ),
        );
        app.world_mut().entity_mut(e).insert(RegionCoord::default());
        members.push(e);
    }
    apply_army_order(
        app.world_mut(),
        "alice",
        ArmyOrder::Muster {
            members: members.clone(),
        },
    );
    for &e in &members {
        inherit_equipment(
            app.world_mut(),
            e,
            SoldierRole::Archer,
            FirePolicy::FireAtWill,
        );
    }
    let b = person(&mut app, "bob", Vec3::new(0., 80., 0.));
    app.world_mut().entity_mut(b).insert(Health::new(10000.));
    apply_unit_order(
        app.world_mut(),
        "alice",
        UnitOrder {
            selection: UnitSelection {
                units: members.clone(),
                battalions: vec![],
            },
            command: UnitCommand::Attack {
                target: b,
                mode: AttackMode::Focus,
            },
        },
    );
    for _ in 0..900 {
        step(&mut app, 1. / 60.);
    }
    let positions: Vec<_> = members
        .iter()
        .map(|e| app.world().get::<PlayerPosition>(*e).unwrap().0)
        .collect();
    let centre = positions.iter().copied().sum::<Vec3>() / 16.;
    assert!(
        centre.x.abs() < 3.,
        "partial rank must not drift sideways: {centre:?}"
    );
    assert!(
        (-50. ..-35.).contains(&centre.z),
        "must hold useful range: {centre:?}"
    );
    let spent: u32 = members
        .iter()
        .map(|e| 24 - u32::from(app.world().get::<Quiver>(*e).unwrap().arrows))
        .sum();
    assert!(
        spent >= 16,
        "settled archers must finish their draws: {spent} released"
    );
}
#[test]
fn mixed_attack_on_a_neutral_keeps_infantry_pursuit() {
    use crate::player::orders::apply_unit_order;
    use shared::protocol::{AttackMode, UnitCommand, UnitOrder, UnitSelection};
    let mut app = lab();
    let mut all = Vec::new();
    for role in [SoldierRole::Infantry, SoldierRole::Archer] {
        let members: Vec<_> = (0..2)
            .map(|i| person(&mut app, "alice", Vec3::new(i as f32 * 2., 0., -30.)))
            .collect();
        apply_army_order(
            app.world_mut(),
            "alice",
            ArmyOrder::Muster {
                members: members.clone(),
            },
        );
        for &e in &members {
            inherit_equipment(app.world_mut(), e, role, FirePolicy::FireAtWill);
        }
        all.extend(members);
    }
    let target = person(&mut app, "neutral", Vec3::ZERO);
    app.world_mut().entity_mut(target).remove::<CommandedBy>();
    assert_eq!(
        apply_unit_order(
            app.world_mut(),
            "alice",
            UnitOrder {
                selection: UnitSelection {
                    units: all.clone(),
                    battalions: vec![]
                },
                command: UnitCommand::Attack {
                    target,
                    mode: AttackMode::Focus
                }
            }
        )
        .0,
        4
    );
    for &e in &all[..2] {
        assert!(app.world().get::<AttackOrder>(e).is_some());
        assert!(app.world().get::<FormationMember>(e).is_none());
    }
    for &e in &all[2..] {
        assert!(app.world().get::<AttackOrder>(e).is_none());
        assert!(app.world().get::<FormationMember>(e).is_some());
    }
}
#[test]
fn hold_fire_cancels_an_ordered_draw_without_cancelling_the_objective() {
    let mut app = lab();
    let a = archer(&mut app);
    let b = person(&mut app, "bob", Vec3::Z * 30.);
    app.world_mut().entity_mut(a).insert(SkirmishOrder {
        target: Enemy::Person(b),
    });
    step(&mut app, 0.1);
    assert!(app.world().get::<BowShot>(a).is_some());
    app.world_mut().entity_mut(a).insert(FirePolicy::HoldFire);
    for _ in 0..15 {
        step(&mut app, 0.1);
    }
    assert!(app.world().get::<BowShot>(a).is_none());
    assert!(app.world().get::<SkirmishOrder>(a).is_some());
    assert_eq!(app.world().get::<Quiver>(a).unwrap().arrows, 24);
}

#[test]
fn archer_attack_move_fires_at_range_then_resumes_its_original_destination() {
    use crate::player::{
        combat::fronts::{advance_battle_fronts, PausedFormationMarch},
        hero::step_units,
        orders::{advance_marches, apply_unit_order},
    };
    use shared::{
        protocol::{FormationFrontage, MovementMode, UnitCommand, UnitOrder, UnitSelection},
        region::RegionCoord,
    };
    let mut app = App::new();
    app.init_resource::<CombatSpace>()
        .init_resource::<CombatFormations>()
        .init_resource::<ArrowObstacles>()
        .init_resource::<BattalionLedger>();
    let mut terrain = shared::terrain::WorldTerrain::default();
    terrain.apply_flatten_rect(Vec3::new(0., 80., 0.), Vec2::splat(120.), 0., 4.);
    app.insert_resource(terrain);
    app.world_mut().spawn(WorldTime::new_default());
    app.add_systems(
        Update,
        (
            advance_marches,
            rebuild_combat_space,
            update_weapons,
            advance_battle_fronts,
            step_units,
            rebuild_combat_space,
            shoot_bows,
            advance_arrows,
        )
            .chain(),
    );
    let members: Vec<_> = (0..4)
        .map(|i| {
            let e = person(
                &mut app,
                "alice",
                Vec3::new(i as f32 * 1.4 - 2.1, 80., -65.),
            );
            app.world_mut().entity_mut(e).insert(RegionCoord::default());
            e
        })
        .collect();
    apply_army_order(
        app.world_mut(),
        "alice",
        ArmyOrder::Muster {
            members: members.clone(),
        },
    );
    for &e in &members {
        inherit_equipment(
            app.world_mut(),
            e,
            SoldierRole::Archer,
            FirePolicy::FireAtWill,
        );
    }
    let enemy = person(&mut app, "bob", Vec3::new(0., 80., 0.));
    app.world_mut()
        .entity_mut(enemy)
        .insert(Health::new(10000.));
    apply_unit_order(
        app.world_mut(),
        "alice",
        UnitOrder {
            selection: UnitSelection {
                units: members.clone(),
                battalions: vec![],
            },
            command: UnitCommand::Move {
                target: Vec3::new(0., 80., 30.),
                frontage: Some(FormationFrontage {
                    facing: Vec2::Y,
                    width: 4.21,
                }),
                mode: MovementMode::AttackMove,
            },
        },
    );
    for _ in 0..1200 {
        step(&mut app, 1. / 60.);
    }
    let stopped = app.world().get::<PlayerPosition>(members[0]).unwrap().0;
    assert!((-47. ..-30.).contains(&stopped.z), "stopped at {stopped:?}");
    assert!(members
        .iter()
        .all(|e| app.world().get::<PausedFormationMarch>(*e).is_some()));
    assert!(members
        .iter()
        .any(|e| app.world().get::<Quiver>(*e).unwrap().arrows < 24));
    app.world_mut().get_mut::<Health>(enemy).unwrap().current = 0.;
    for _ in 0..600 {
        step(&mut app, 1. / 60.);
    }
    assert!(members
        .iter()
        .all(|e| app.world().get::<PausedFormationMarch>(*e).is_none()));
    let resumed = app.world().get::<PlayerPosition>(members[0]).unwrap().0;
    assert!(
        resumed.z > stopped.z + 15.,
        "march did not resume: {stopped:?} -> {resumed:?}"
    );
}
