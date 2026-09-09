use super::*;
use crate::army_roster::SoldierFacts;
use shared::protocol::ArmyOrder;
fn e(n: u32) -> Entity {
    Entity::from_raw_u32(n).unwrap()
}
fn fixture() -> (ArmyRoster, ArmyManagement) {
    let mut roster = ArmyRoster::default();
    roster.account = "alice".into();
    for n in 1..=6 {
        roster.soldiers.insert(
            e(n),
            SoldierFacts {
                entity: e(n),
                identity: n as u64,
                name: format!("Soldier {n}"),
                battalion: match n {
                    1 | 2 => Some(BattalionId(1)),
                    3 | 4 => Some(BattalionId(2)),
                    _ => None,
                },
                strength: 20 - n as u8,
                current_health: 100.0,
                max_health: 100.0,
                available: n != 6,
                role: SoldierRole::Infantry,
                arrows: 0,
            },
        );
    }
    for n in 1..=2 {
        roster.battalions.push(BattalionFacts {
            entity: e(100 + n),
            id: BattalionId(n as u64),
            name: format!("Battalion {n}"),
            ordinal: n as u64,
            members: vec![e(n * 2 - 1), e(n * 2)],
            count: 2,
            mean_strength: 15,
            health_fraction: 1.0,
            stance: BattalionStance::Defensive,
            formation: default(),
            role: shared::components::SoldierRole::Infantry,
            fire_policy: shared::components::FirePolicy::FireAtWill,
            arrows: 0,
        });
    }
    (
        roster,
        ArmyManagement {
            selected: Some(e(101)),
            ..default()
        },
    )
}
#[test]
fn transfer_and_remove_are_explicit_and_embarked_troops_are_disabled() {
    let (r, mut s) = fixture();
    let m = PanelModel::new(&r, &s);
    assert_eq!(m.members, vec![e(1), e(2)]);
    assert_eq!(m.available, vec![e(5), e(6)]);
    assert_eq!(
        m.command(ArmyAction::Add(e(5)), &s, &r),
        Some(ArmyOrder::Assign {
            battalion: e(101),
            members: vec![e(5)]
        })
    );
    assert!(m.command(ArmyAction::Add(e(6)), &s, &r).is_none());
    assert!(!m.button(ArmyAction::Toggle(e(6)), &s, &r, false).1);
    s.other_battalions = true;
    let m = PanelModel::new(&r, &s);
    assert_eq!(m.available, vec![e(3), e(4)]);
    assert!(matches!(
        m.command(ArmyAction::Add(e(3)), &s, &r),
        Some(ArmyOrder::Assign { .. })
    ));
    assert_eq!(
        m.command(ArmyAction::Remove(e(1)), &s, &r),
        Some(ArmyOrder::Dismiss {
            members: vec![e(1)]
        })
    );
    assert!(m.command(ArmyAction::Remove(e(3)), &s, &r).is_none());
}
#[test]
fn bulk_actions_respect_capacity_and_new_battalion_never_steals_world_selection() {
    let (mut r, mut s) = fixture();
    r.battalions[0].count = 63;
    s.other_battalions = true;
    s.available.extend([e(3), e(4)]);
    let m = PanelModel::new(&r, &s);
    assert!(m.command(ArmyAction::AddChecked, &s, &r).is_none());
    assert_eq!(
        m.command(ArmyAction::Fill, &s, &r),
        Some(ArmyOrder::Assign {
            battalion: e(101),
            members: vec![e(5)]
        })
    );
    assert_eq!(
        m.command(ArmyAction::New, &s, &r),
        Some(ArmyOrder::Muster { members: vec![] })
    );
    assert!(m.command(ArmyAction::Disband, &s, &r).is_none());
    s.confirm_disband = true;
    assert!(matches!(
        m.command(ArmyAction::Disband, &s, &r),
        Some(ArmyOrder::Disband { .. })
    ));
}
#[test]
fn retained_controls_survive_health_policy_and_checkbox_changes() {
    let (r, s) = fixture();
    let mut app = App::new();
    app.insert_resource(r)
        .insert_resource(s)
        .init_resource::<Time>()
        .init_resource::<crate::ui::perf::UiPerf>();
    let root = app.world_mut().spawn(Node::default()).id();
    app.world_mut()
        .commands()
        .entity(root)
        .with_children(spawn_army_tab);
    app.world_mut().flush();
    app.add_systems(Update, sync_army_panel);
    app.update();
    app.update();
    let controls = |w: &mut World| {
        let mut ids: Vec<_> = w
            .query_filtered::<Entity, With<ArmyAction>>()
            .iter(w)
            .collect();
        ids.sort();
        ids
    };
    let before = controls(app.world_mut());
    app.world_mut()
        .resource_mut::<ArmyRoster>()
        .soldiers
        .get_mut(&e(1))
        .unwrap()
        .current_health = 57.0;
    app.world_mut().resource_mut::<ArmyRoster>().battalions[0].stance = BattalionStance::HoldLine;
    app.world_mut()
        .resource_mut::<ArmyManagement>()
        .members
        .insert(e(1));
    app.update();
    assert_eq!(before, controls(app.world_mut()));
    assert!(app
        .world_mut()
        .query::<&Text>()
        .iter(app.world())
        .any(|t| t.0.contains("57 HP")));
    assert!(app
        .world_mut()
        .query::<(&ArmyAction, &crate::ui::foundation::UiButtonStyle)>()
        .iter(app.world())
        .any(|(a, s)| *a == ArmyAction::Stance(BattalionStance::HoldLine) && s.selected));

    // A pending command must expire without another roster update. Otherwise
    // the next network action stays disabled forever after the first click.
    app.world_mut()
        .resource_mut::<ArmyManagement>()
        .pending_until = 1.0;
    app.update();
    let fill_disabled = |w: &mut World| {
        w.query::<(&ArmyAction, Has<InteractionDisabled>)>()
            .iter(w)
            .find(|(a, _)| **a == ArmyAction::Fill)
            .unwrap()
            .1
    };
    assert!(fill_disabled(app.world_mut()));
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(std::time::Duration::from_secs(2));
    app.update();
    assert!(!fill_disabled(app.world_mut()));
    assert_eq!(before, controls(app.world_mut()));
    // Switching the destination makes old Add controls invalid in the same
    // frame that their rows are replaced. Deferred disabled-state writes must
    // finish before the old rows are despawned.
    app.world_mut()
        .resource_mut::<ArmyManagement>()
        .other_battalions = true;
    app.update();
    app.update();
    app.world_mut().resource_mut::<ArmyManagement>().selected = Some(e(102));
    app.update();
    app.update();
    assert!(app
        .world_mut()
        .query::<&Text>()
        .iter(app.world())
        .any(|t| t.0 == "Battalion 2"));
}
#[test]
fn archery_controls_send_owned_battalion_commands_and_show_ammunition() {
    let (mut roster, state) = fixture();
    let unit = &mut roster.battalions[0];
    unit.role = SoldierRole::Archer;
    unit.arrows = 3;
    let model = PanelModel::new(&roster, &state);
    assert_eq!(
        model.command(ArmyAction::Fire(FirePolicy::HoldFire), &state, &roster),
        Some(ArmyOrder::SetFirePolicy {
            battalion: e(101),
            policy: FirePolicy::HoldFire
        })
    );
    assert_eq!(
        model.command(ArmyAction::Role(SoldierRole::Infantry), &state, &roster),
        Some(ArmyOrder::SetRole {
            battalion: e(101),
            role: SoldierRole::Infantry
        })
    );
    assert!(model.button(ArmyAction::Rearm, &state, &roster, false).1);
    assert!(!model.button(ArmyAction::Rearm, &state, &roster, true).1);
    assert!(
        model
            .button(
                ArmyAction::Role(SoldierRole::Archer),
                &state,
                &roster,
                false
            )
            .2
    );
}

#[test]
fn cavalry_keeps_its_mounts_when_managed_without_a_stable() {
    let (mut roster, state) = fixture();
    roster.battalions[0].role = SoldierRole::Cavalry;
    let model = PanelModel::new(&roster, &state);
    assert!(
        model.available.is_empty(),
        "foot troops need mounts before joining cavalry"
    );
    assert!(model.reserves.is_empty());
    assert_eq!(model.command(ArmyAction::Fill, &state, &roster), None);
    assert!(
        !model
            .button(
                ArmyAction::Role(SoldierRole::Infantry),
                &state,
                &roster,
                false
            )
            .1
    );
    assert_eq!(
        model.command(ArmyAction::Role(SoldierRole::Infantry), &state, &roster),
        None
    );
    assert!(
        model
            .button(ArmyAction::SelectMap, &state, &roster, false)
            .1
    );
}
