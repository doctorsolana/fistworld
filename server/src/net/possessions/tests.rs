//! Packet-level regressions use the same Replicon backend as Lightyear. The
//! assertions inspect receiving worlds, including component retractions.

use super::*;
use bevy::state::app::StatesPlugin;
use bevy_replicon::{
    prelude::{AppRuleExt, Replicated, RepliconPlugins, ServerPlugin},
    test_app::{ServerTestAppExt, TestClientEntity},
};
use shared::{
    components::CharacterName,
    economy::{CarriedLoad, Good},
};

fn app() -> App {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        StatesPlugin,
        RepliconPlugins.set(ServerPlugin::new(PostUpdate)),
    ));
    app.replicate::<CharacterName>()
        .replicate::<PersonId>()
        .replicate::<CharacterKind>()
        .replicate::<Wallet>()
        .replicate::<GoodsInventory>()
        .replicate::<CarriedLoad>();
    app.finish();
    app
}

fn server() -> App {
    let mut app = app();
    app.init_resource::<PlayerProfiles>();
    install(&mut app);
    app
}

fn authenticate(server: &mut App, client: &App, peer: PeerId, account: &str) {
    let link = **client.world().resource::<TestClientEntity>();
    server.world_mut().entity_mut(link).insert(RemoteId(peer));
    server
        .world_mut()
        .resource_mut::<PlayerProfiles>()
        .peer_to_name
        .insert(peer, account.to_owned());
}

fn exchange(server: &mut App, clients: &mut [&mut App]) {
    server.update();
    for client in clients {
        server.exchange_with_client(client);
        client.update();
    }
}

fn inventory() -> GoodsInventory {
    let mut inventory = GoodsInventory::new(100);
    inventory.add(Good::Wood, 12);
    inventory
}

fn person(server: &mut App, label: &str, account: Option<&str>) -> Entity {
    let inventory = inventory();
    let mut entity = server.world_mut().spawn((
        Replicated,
        CharacterName(label.into()),
        CharacterKind::Villager,
        PersonId(42),
        Wallet::new(987),
        CarriedLoad::from_inventory(&inventory),
        inventory,
    ));
    if let Some(account) = account {
        entity.insert(CommandedBy(account.into()));
    }
    entity.id()
}

/// Person existence, exact money, exact inventory, publicly visible carried prop.
fn received(client: &mut App, name: &str) -> (bool, Option<u64>, Option<u32>, Option<Good>) {
    client
        .world_mut()
        .query::<(
            &CharacterName,
            Option<&Wallet>,
            Option<&GoodsInventory>,
            Option<&CarriedLoad>,
        )>()
        .iter(client.world())
        .find(|(n, ..)| n.0 == name)
        .map_or((false, None, None, None), |(_, wallet, inventory, load)| {
            (
                true,
                wallet.map(|w| w.balance()),
                inventory.map(|i| i.amount(Good::Wood)),
                load.and_then(|l| l.good),
            )
        })
}

#[test]
fn first_spawn_is_private_but_public_body_and_carry_visual_still_replicate() {
    let mut server = server();
    let mut owner = app();
    let mut stranger = app();
    server.connect_client(&mut owner);
    server.connect_client(&mut stranger);
    authenticate(&mut server, &owner, PeerId::Netcode(1), "alice");
    authenticate(&mut server, &stranger, PeerId::Netcode(2), "bob");
    let entity = person(&mut server, "guard", Some("alice"));
    assert_eq!(
        server.world().get::<PossessionsAccess>(entity),
        Some(&PossessionsAccess::Denied)
    );
    exchange(&mut server, &mut [&mut owner, &mut stranger]);
    assert_eq!(
        received(&mut owner, "guard"),
        (true, Some(987), Some(12), Some(Good::Wood))
    );
    assert_eq!(
        received(&mut stranger, "guard"),
        (true, None, None, Some(Good::Wood))
    );
}

#[test]
fn command_transfer_and_removal_retract_exact_components_from_previous_owner() {
    let mut server = server();
    let mut alice = app();
    let mut bob = app();
    server.connect_client(&mut alice);
    server.connect_client(&mut bob);
    authenticate(&mut server, &alice, PeerId::Netcode(1), "alice");
    authenticate(&mut server, &bob, PeerId::Netcode(2), "bob");
    let entity = person(&mut server, "guard", Some("alice"));
    exchange(&mut server, &mut [&mut alice, &mut bob]);
    server.world_mut().get_mut::<CommandedBy>(entity).unwrap().0 = "bob".into();
    exchange(&mut server, &mut [&mut alice, &mut bob]);
    assert_eq!(
        received(&mut alice, "guard"),
        (true, None, None, Some(Good::Wood))
    );
    assert_eq!(
        received(&mut bob, "guard"),
        (true, Some(987), Some(12), Some(Good::Wood))
    );
    server
        .world_mut()
        .entity_mut(entity)
        .remove::<CommandedBy>();
    exchange(&mut server, &mut [&mut alice, &mut bob]);
    assert_eq!(
        received(&mut bob, "guard"),
        (true, None, None, Some(Good::Wood))
    );
}

#[test]
fn anonymous_connections_cannot_read_hero_possessions_or_stale_account_access() {
    let mut server = server();
    let mut client = app();
    server.connect_client(&mut client);
    let link = **client.world().resource::<TestClientEntity>();
    let peer = PeerId::Netcode(1);
    server.world_mut().entity_mut(link).insert(RemoteId(peer));
    let entity = person(&mut server, "hero", None);
    server
        .world_mut()
        .entity_mut(entity)
        .insert(Hero { owner: peer });
    exchange(&mut server, &mut [&mut client]);
    assert_eq!(received(&mut client, "hero").1, None);
    authenticate(&mut server, &client, peer, "alice");
    exchange(&mut server, &mut [&mut client]);
    assert_eq!(received(&mut client, "hero").1, Some(987));
    server
        .world_mut()
        .resource_mut::<PlayerProfiles>()
        .peer_to_name
        .remove(&peer);
    exchange(&mut server, &mut [&mut client]);
    assert_eq!(received(&mut client, "hero").1, None);
}

#[test]
fn public_building_storage_is_unchanged_and_unclassified_storage_stays_denied() {
    let mut server = server();
    let mut client = app();
    server.connect_client(&mut client);
    server.world_mut().spawn((
        Replicated,
        CharacterName("warehouse".into()),
        BuildingId(100),
        inventory(),
        Wallet::new(321),
    ));
    let pending = server
        .world_mut()
        .spawn((
            Replicated,
            CharacterName("incomplete".into()),
            inventory(),
            Wallet::new(123),
        ))
        .id();
    exchange(&mut server, &mut [&mut client]);
    assert_eq!(
        received(&mut client, "warehouse"),
        (true, Some(321), Some(12), None)
    );
    assert_eq!(
        received(&mut client, "incomplete"),
        (true, None, None, None)
    );
    server
        .world_mut()
        .entity_mut(pending)
        .insert((CharacterKind::Villager, PersonId(55)));
    exchange(&mut server, &mut [&mut client]);
    assert_eq!(
        received(&mut client, "incomplete"),
        (true, None, None, None)
    );
}

#[test]
fn late_joining_stranger_never_gets_preexisting_personal_inventory() {
    let mut server = server();
    let mut owner = app();
    server.connect_client(&mut owner);
    authenticate(&mut server, &owner, PeerId::Netcode(1), "alice");
    person(&mut server, "guard", Some("alice"));
    exchange(&mut server, &mut [&mut owner]);
    let mut newcomer = app();
    server.connect_client(&mut newcomer);
    authenticate(&mut server, &newcomer, PeerId::Netcode(2), "bob");
    exchange(&mut server, &mut [&mut owner, &mut newcomer]);
    assert_eq!(
        received(&mut newcomer, "guard"),
        (true, None, None, Some(Good::Wood))
    );
}
