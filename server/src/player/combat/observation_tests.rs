//! The same authoritative fight must run regardless of replication interest.

use super::{fighter, *};
use crate::net::input::ClientInputs;
use crate::world::regions::{
    ClientInterest, RegionRegistry, update_client_interest, update_region_observers,
};
use crate::world::village::mortality::{MortalityLedger, process_character_deaths};
use crate::world::village::{BusinessEventQueue, CompanyEscrowRefundQueue};
use lightyear::prelude::{ControlledBy, Lifetime, PeerId};
use shared::components::{CharacterAffiliation, CharacterName, PersonId, Player};
use shared::region::{REGION_SIZE, RegionCoord};

#[derive(Debug, PartialEq)]
struct FightSample {
    attacker_health: f32,
    target_health: Option<f32>,
    bystander_health: f32,
    attacker_target: Option<PersonId>,
    attacker_activity: CharacterActivity,
    attacker_cooldown: Option<(f64, bool)>,
    deaths: Vec<(PersonId, DeathCause)>,
}

fn observed_fight(mode: usize) -> Vec<FightSample> {
    let mut app = App::new();
    app.init_resource::<ClientInputs>()
        .init_resource::<ClientInterest>()
        .init_resource::<RegionRegistry>()
        .init_resource::<MortalityLedger>()
        .init_resource::<BusinessEventQueue>()
        .init_resource::<CompanyEscrowRefundQueue>();
    app.world_mut()
        .resource_mut::<RegionRegistry>()
        .set_observers_for_test(RegionCoord::new(0, 0), 0);
    app.add_systems(
        Update,
        (
            update_client_interest,
            update_region_observers,
            acquire_targets,
            pursue_attack_orders,
            process_character_deaths,
            expire_combat_bodies,
        )
            .chain(),
    );
    let clock = app.world_mut().spawn(WorldTime::new_default()).id();
    let attacker = fighter(&mut app, Vec3::ZERO, "alice", 100.0);
    let civilian = fighter(&mut app, Vec3::X, "civilian", 100.0);
    let bystander = fighter(&mut app, Vec3::Z, "carol", 100.0);
    for (entity, id, name) in [
        (attacker, 1, "Attacker"),
        (civilian, 2, "Civilian"),
        (bystander, 3, "Bystander"),
    ] {
        app.world_mut().entity_mut(entity).insert((
            PersonId(id),
            CharacterName(name.into()),
            CharacterAffiliation::default(),
            RegionCoord::new(0, 0),
        ));
    }
    // An ordinary civilian must remain a real damageable body off camera.
    // No army marker or command ownership artificially keeps this person active.
    app.world_mut().entity_mut(civilian).remove::<CommandedBy>();
    let (accepted, _) = crate::player::orders::apply_unit_order(
        app.world_mut(),
        "alice",
        shared::protocol::UnitOrder {
            selection: shared::protocol::UnitSelection {
                units: vec![attacker],
                battalions: Vec::new(),
            },
            command: shared::protocol::UnitCommand::Attack {
                target: civilian,
                mode: shared::protocol::AttackMode::Focus,
            },
        },
    );
    assert_eq!(
        accepted, 1,
        "the same real command must pass authority checks"
    );
    // Allocate exactly the same non-economic camera shells in every run.
    let owner = app.world_mut().spawn_empty().id();
    let camera = app
        .world_mut()
        .spawn((
            PlayerPosition(Vec3::ZERO),
            ControlledBy {
                owner,
                lifetime: Lifetime::default(),
            },
        ))
        .id();
    let peer = PeerId::Local(77);
    let mut trace = Vec::new();
    let mut observed_damage = false;
    let mut observed_target = false;
    for tick in 0..900 {
        let interested = mode == 1 || (mode == 2 && (tick / 60) % 2 == 0);
        if mode != 0 {
            app.world_mut()
                .entity_mut(camera)
                .insert(Player { client_id: peer });
            app.world_mut().get_mut::<PlayerPosition>(camera).unwrap().0 = if interested {
                Vec3::ZERO
            } else {
                Vec3::X * REGION_SIZE * 100.0
            };
        }
        app.world_mut()
            .get_mut::<WorldTime>(clock)
            .unwrap()
            .advance(1.0 / 60.0, 0.0);
        app.update();
        assert_eq!(
            app.world()
                .resource::<RegionRegistry>()
                .get(RegionCoord::new(0, 0))
                .unwrap()
                .observers
                > 0,
            interested
        );
        let world = app.world();
        let target_health = world.get::<Health>(civilian).map(|health| health.current);
        observed_damage |= target_health.is_some_and(|health| health < 100.0);
        let attacker_target = world.get::<AttackOrder>(attacker).map(|order| {
            assert_eq!(order.target, civilian);
            PersonId(2)
        });
        observed_target |= attacker_target.is_some();
        assert!(world.get::<AttackOrder>(bystander).is_none());
        trace.push(FightSample {
            attacker_health: world.get::<Health>(attacker).unwrap().current,
            target_health,
            bystander_health: world.get::<Health>(bystander).unwrap().current,
            attacker_target,
            attacker_activity: *world.get::<CharacterActivity>(attacker).unwrap(),
            attacker_cooldown: world
                .get::<MeleeCooldown>(attacker)
                .map(|cooldown| (cooldown.ready_at, cooldown.engaged)),
            deaths: world
                .resource::<MortalityLedger>()
                .iter()
                .map(|death| (death.id, death.cause))
                .collect(),
        });
    }
    assert!(
        observed_damage && observed_target,
        "the fixture must witness an actual ongoing fight"
    );
    let last = trace.last().unwrap();
    assert_eq!(last.deaths, vec![(PersonId(2), DeathCause::Combat)]);
    assert!(
        last.target_health.is_none(),
        "the real mortality/corpse pipeline must finish"
    );
    assert!(last.attacker_target.is_none());
    assert_eq!(last.attacker_activity, CharacterActivity::Idle);
    assert_eq!(last.attacker_health, 100.0);
    assert_eq!(last.bystander_health, 100.0);
    trace
}

#[test]
fn civilian_damage_death_and_target_cleanup_are_identical_across_observer_changes() {
    let absent = observed_fight(0);
    let present = observed_fight(1);
    let moving = observed_fight(2);
    assert_eq!(absent, present, "a stationary camera changed the battle");
    assert_eq!(
        absent, moving,
        "moving attention changed damage, targeting or death timing"
    );
}
