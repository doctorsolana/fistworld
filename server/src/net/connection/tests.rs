//! Account isolation and reconnect snapshots at the death-animation boundary.
use super::*;
use crate::player::hero::{MoveTarget, OfflineHero};
use shared::components::{Hero, HeroOutfit};

fn app() -> App {
    let mut app = App::new();
    app.init_resource::<PlayerProfiles>()
        .init_resource::<lightyear::connection::client::PeerMetadata>()
        .init_resource::<ClientInputs>()
        .init_resource::<crate::world::dev::GodAccessSessions>()
        .init_resource::<Time>()
        .add_observer(handle_disconnections);
    app
}

fn player(app: &mut App, account: &str, peer: PeerId, health: f32) -> (Entity, Entity) {
    let profile = PlayerProfile::new_player(account.to_string());
    let progression = PlayerProgression {
        level: profile.level,
        prestige: profile.prestige,
        reputation: profile.reputation,
        stamina: profile.stamina,
        intelligence: profile.intelligence,
        charm: profile.charm,
    };
    let mut registry = app.world_mut().resource_mut::<PlayerProfiles>();
    registry.profiles.insert(account.to_string(), profile);
    registry.peer_to_name.insert(peer, account.to_string());
    registry.name_to_peer.insert(account.to_string(), peer);
    app.world_mut().spawn((
        Player { client_id: peer },
        PlayerPosition(Vec3::ZERO),
        PlayerRotation(0.0),
        progression,
    ));
    let hero = app
        .world_mut()
        .spawn((
            Hero { owner: peer },
            HeroOutfit::default(),
            CharacterAttributes::default(),
            PlayerPosition(Vec3::new(10.0, 2.0, 4.0)),
            PlayerRotation(0.5),
            Health {
                current: health,
                max: 100.0,
            },
            MoveTarget(Vec3::new(20.0, 2.0, 4.0)),
        ))
        .id();
    let link = app.world_mut().spawn(RemoteId(peer)).id();
    (link, hero)
}

#[test]
fn disconnecting_one_account_retains_its_hero_without_touching_the_other() {
    let mut app = app();
    let a = PeerId::Netcode(101);
    let b = PeerId::Netcode(202);
    let (link, hero_a) = player(&mut app, "alice", a, 74.0);
    let (_, hero_b) = player(&mut app, "bruno", b, 91.0);
    app.world_mut()
        .entity_mut(link)
        .insert(Disconnected::default());
    app.world_mut().flush();

    let profiles = app.world().resource::<PlayerProfiles>();
    assert!(!profiles.is_name_online("alice"));
    assert_eq!(profiles.name_to_peer.get("bruno"), Some(&b));
    assert_eq!(
        profiles.peer_to_name.get(&b).map(String::as_str),
        Some("bruno")
    );
    assert!(profiles.profiles["alice"].hero.is_some());
    assert!(app.world().get::<OfflineHero>(hero_a).is_some());
    assert!(app.world().get::<MoveTarget>(hero_a).is_none());
    assert!(app.world().get::<OfflineHero>(hero_b).is_none());
    assert!(app.world().get::<MoveTarget>(hero_b).is_some());
    assert_eq!(app.world().get::<Health>(hero_b).unwrap().current, 91.0);
}

#[test]
fn disconnect_during_death_animation_does_not_restore_a_dead_hero_profile() {
    let mut app = app();
    let (link, corpse) = player(&mut app, "alice", PeerId::Netcode(101), 0.0);
    app.world_mut()
        .entity_mut(link)
        .insert(Disconnected::default());
    app.world_mut().flush();
    assert!(app.world().resource::<PlayerProfiles>().profiles["alice"]
        .hero
        .is_none());
    assert!(app.world().get::<OfflineHero>(corpse).is_none());
}

#[test]
fn periodic_save_during_death_animation_keeps_only_the_living_hero_snapshot() {
    let mut app = app();
    player(&mut app, "alice", PeerId::Netcode(101), 0.0);
    player(&mut app, "bruno", PeerId::Netcode(202), 91.0);
    app.add_systems(
        Update,
        crate::persistence::autosave::update_periodic_player_save,
    );
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(Duration::from_secs(31));
    app.update();
    let profiles = app.world().resource::<PlayerProfiles>();
    assert!(profiles.profiles["alice"].hero.is_none());
    assert!(profiles.profiles["bruno"].hero.is_some());
}
