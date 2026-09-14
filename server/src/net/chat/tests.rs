//! Exercise the real handler through serialized Lightyear link buffers, without sockets.

use super::*;
use shared::player_profile::PlayerProfile;
use shared::protocol::{ProtocolPlugin, CHAT_MAX_CHARACTERS};

fn app() -> App {
    let mut app = App::new();
    app.add_plugins((MinimalPlugins, bevy::state::app::StatesPlugin));
    app.add_plugins(lightyear::prelude::server::ServerPlugins::default());
    app.insert_resource(ReplicationMetadata::new(Duration::from_millis(33)));
    app.add_plugins(ProtocolPlugin);
    app.init_resource::<PlayerProfiles>();
    install(&mut app);
    app.finish();
    app.cleanup();
    app.update();
    app
}

fn connect(app: &mut App, peer: PeerId, name: Option<&str>) -> Entity {
    let registry = app.world().resource::<ChannelRegistry>();
    let mut transport = Transport::default();
    transport.add_sender_from_registry::<ChatChannel>(registry);
    transport.add_receiver_from_registry::<ChatChannel>(registry);
    transport.add_sender_from_registry::<shared::protocol::InputChannel>(registry);
    transport.add_receiver_from_registry::<shared::protocol::InputChannel>(registry);
    let link = app
        .world_mut()
        .spawn((
            Link::default(),
            transport,
            ClientOf,
            RemoteId(peer),
            Linked,
            Connected,
            // Extra opposite-direction endpoints are only for this packet loopback.
            MessageSender::<ChatSend>::default(),
            MessageReceiver::<ChatEvent>::default(),
        ))
        .id();
    if let Some(name) = name {
        authenticate(app, peer, name);
    }
    app.world_mut().flush();
    link
}

fn authenticate(app: &mut App, peer: PeerId, name: &str) {
    let account = name.to_lowercase();
    let mut profiles = app.world_mut().resource_mut::<PlayerProfiles>();
    profiles
        .profiles
        .insert(account.clone(), PlayerProfile::new_player(name.to_owned()));
    profiles.peer_to_name.insert(peer, account.clone());
    profiles.name_to_peer.insert(account, peer);
}

fn send(app: &mut App, link: Entity, text: &str) {
    app.world_mut()
        .get_mut::<MessageSender<ChatSend>>(link)
        .unwrap()
        .send::<ChatChannel>(ChatSend {
            text: text.to_owned(),
        });
}

fn transmit(app: &mut App) {
    app.world_mut().run_schedule(PostUpdate);
    let mut links = app.world_mut().query::<&mut Link>();
    for mut link in links.iter_mut(app.world_mut()) {
        while let Some(payload) = link.send.pop() {
            link.recv.push_raw(payload);
        }
    }
    app.world_mut().run_schedule(PreUpdate);
}

fn deliver(app: &mut App) {
    transmit(app);
    app.world_mut().run_schedule(Update);
    transmit(app);
}

fn received(app: &mut App, link: Entity) -> Vec<ChatEvent> {
    app.world_mut()
        .get_mut::<MessageReceiver<ChatEvent>>(link)
        .unwrap()
        .receive()
        .collect()
}

fn accepted(events: &[ChatEvent]) -> Vec<(u64, &str, &str)> {
    events
        .iter()
        .filter_map(|event| match event {
            ChatEvent::Message {
                sequence,
                sender,
                text,
            } => Some((*sequence, sender.as_str(), text.as_str())),
            ChatEvent::Rejected { .. } => None,
        })
        .collect()
}

#[test]
fn chat_delivers_authenticated_display_names_in_identical_order_server_wide() {
    let mut app = app();
    let alice = connect(&mut app, PeerId::Netcode(1), Some("ALIce"));
    let bob = connect(&mut app, PeerId::Netcode(2), Some("Bob"));
    let anonymous = connect(&mut app, PeerId::Netcode(3), None);
    send(&mut app, alice, "  Olá 世界 👩‍🌾  ");
    send(&mut app, alice, "Second");
    deliver(&mut app);
    let alice_events = received(&mut app, alice);
    assert_eq!(received(&mut app, bob), alice_events);
    assert_eq!(
        accepted(&alice_events),
        [(1, "ALIce", "Olá 世界 👩‍🌾"), (2, "ALIce", "Second")]
    );
    assert!(received(&mut app, anonymous).is_empty());
    let late_join = connect(&mut app, PeerId::Netcode(4), Some("Later"));
    deliver(&mut app);
    assert!(
        received(&mut app, late_join).is_empty(),
        "chat has no history replay"
    );
}

#[test]
fn chat_unauthenticated_stale_and_wrong_channel_requests_never_publish() {
    let mut app = app();
    let alice = connect(&mut app, PeerId::Netcode(1), Some("Alice"));
    let anonymous = connect(&mut app, PeerId::Netcode(2), None);
    let stale = connect(&mut app, PeerId::Netcode(3), Some("Old"));
    app.world_mut()
        .resource_mut::<PlayerProfiles>()
        .name_to_peer
        .insert("old".into(), PeerId::Netcode(30));
    send(&mut app, anonymous, "Pretend to be Alice");
    send(&mut app, stale, "Old connection");
    app.world_mut()
        .get_mut::<MessageSender<ChatSend>>(alice)
        .unwrap()
        .send::<shared::protocol::InputChannel>(ChatSend {
            text: "Unordered bypass".into(),
        });
    deliver(&mut app);
    for link in [alice, anonymous, stale] {
        assert!(received(&mut app, link).is_empty());
        assert_eq!(
            app.world()
                .get::<MessageReceiver<ChatSend>>(link)
                .unwrap()
                .num_messages(),
            0
        );
    }
    assert_eq!(app.world().resource::<ChatState>().sequence, 0);
}

#[test]
fn chat_flood_is_bounded_per_connection_without_starving_another_sender() {
    let mut app = app();
    let alice = connect(&mut app, PeerId::Netcode(1), Some("Alice"));
    let bob = connect(&mut app, PeerId::Netcode(2), Some("Bob"));
    for _ in 0..80 {
        send(&mut app, alice, "flood");
    }
    send(&mut app, bob, "still heard");
    deliver(&mut app);
    let alice_events = received(&mut app, alice);
    let bob_events = received(&mut app, bob);
    assert_eq!(accepted(&alice_events), accepted(&bob_events));
    assert_eq!(
        accepted(&bob_events)
            .iter()
            .filter(|(_, name, _)| *name == "Alice")
            .count(),
        3
    );
    assert_eq!(
        accepted(&bob_events)
            .iter()
            .filter(|(_, name, _)| *name == "Bob")
            .count(),
        1
    );
    assert_eq!(
        alice_events
            .iter()
            .filter(|e| matches!(
                e,
                ChatEvent::Rejected {
                    reason: ChatRejectionReason::RateLimited
                }
            ))
            .count(),
        1
    );
    assert_eq!(
        app.world()
            .get::<MessageReceiver<ChatSend>>(alice)
            .unwrap()
            .num_messages(),
        0
    );
    deliver(&mut app);
    assert!(
        received(&mut app, bob).is_empty(),
        "discarded overflow cannot replay next frame"
    );
}

#[test]
fn chat_refill_uses_real_time_when_world_time_is_warped_or_paused() {
    let mut app = app();
    let alice = connect(&mut app, PeerId::Netcode(1), Some("Alice"));
    app.world_mut().spawn(shared::components::TimeWarp(1000.0));
    for _ in 0..3 {
        send(&mut app, alice, "burst");
    }
    deliver(&mut app);
    assert_eq!(accepted(&received(&mut app, alice)).len(), 3);
    // Virtual time and world warp cannot refill the real-time allowance.
    app.world_mut()
        .resource_mut::<Time<Virtual>>()
        .advance_by(Duration::from_secs(1000));
    send(&mut app, alice, "too soon");
    deliver(&mut app);
    assert!(accepted(&received(&mut app, alice)).is_empty());
    app.world_mut().resource_mut::<Time<Virtual>>().pause();
    app.world_mut()
        .resource_mut::<Time<Real>>()
        .advance_by(Duration::from_secs(2));
    send(&mut app, alice, "one refilled");
    deliver(&mut app);
    assert_eq!(accepted(&received(&mut app, alice)).len(), 1);
    send(&mut app, alice, "no second token");
    deliver(&mut app);
    assert!(accepted(&received(&mut app, alice)).is_empty());
}

#[test]
fn chat_each_rejected_retry_gets_feedback_before_refill() {
    let mut app = app();
    let alice = connect(&mut app, PeerId::Netcode(1), Some("Alice"));
    for _ in 0..3 {
        send(&mut app, alice, "burst");
    }
    deliver(&mut app);
    assert_eq!(accepted(&received(&mut app, alice)).len(), 3);
    for _ in 0..2 {
        send(&mut app, alice, "retry before refill");
        deliver(&mut app);
        assert_eq!(
            received(&mut app, alice),
            [ChatEvent::Rejected {
                reason: ChatRejectionReason::RateLimited,
            }]
        );
    }
}

#[test]
fn chat_rejects_unsafe_and_overlong_text_with_private_feedback() {
    for (text, reason) in [
        (" \u{2003} ".to_owned(), ChatRejectionReason::Empty),
        (
            "a".repeat(CHAT_MAX_CHARACTERS + 1),
            ChatRejectionReason::TooLong,
        ),
        (
            "line\nbreak".to_owned(),
            ChatRejectionReason::InvalidCharacters,
        ),
        (
            "spoof\u{202e}name".to_owned(),
            ChatRejectionReason::InvalidCharacters,
        ),
    ] {
        let mut app = app();
        let alice = connect(&mut app, PeerId::Netcode(1), Some("Alice"));
        let bob = connect(&mut app, PeerId::Netcode(2), Some("Bob"));
        send(&mut app, alice, &text);
        deliver(&mut app);
        assert_eq!(received(&mut app, alice), [ChatEvent::Rejected { reason }]);
        assert!(received(&mut app, bob).is_empty());
    }
}

#[test]
fn chat_disconnect_and_account_replacement_clear_state_and_queued_messages() {
    let mut app = app();
    let alice = connect(&mut app, PeerId::Netcode(1), Some("Alice"));
    let bob = connect(&mut app, PeerId::Netcode(2), Some("Bob"));
    send(&mut app, alice, "before");
    deliver(&mut app);
    received(&mut app, alice);
    received(&mut app, bob);
    assert!(app
        .world()
        .resource::<ChatState>()
        .sessions
        .contains_key(&alice));
    send(&mut app, alice, "queued under old account");
    transmit(&mut app);
    authenticate(&mut app, PeerId::Netcode(1), "Replacement");
    app.world_mut().run_schedule(Update);
    transmit(&mut app);
    assert!(received(&mut app, bob).is_empty());
    assert_eq!(
        app.world().resource::<ChatState>().sessions[&alice].account,
        "replacement"
    );
    send(&mut app, alice, "new account");
    deliver(&mut app);
    assert_eq!(
        accepted(&received(&mut app, bob)),
        [(2, "Replacement", "new account")]
    );
    app.world_mut()
        .entity_mut(alice)
        .insert(Disconnected::default());
    app.world_mut().flush();
    assert!(!app
        .world()
        .resource::<ChatState>()
        .sessions
        .contains_key(&alice));
    send(&mut app, bob, "after disconnect");
    deliver(&mut app);
    assert_eq!(
        accepted(&received(&mut app, bob)),
        [(3, "Bob", "after disconnect")]
    );
}

#[test]
fn chat_passive_account_replacement_discards_its_first_queued_message() {
    let mut app = app();
    let alice = connect(&mut app, PeerId::Netcode(1), Some("Alice"));
    let bob = connect(&mut app, PeerId::Netcode(2), Some("Bob"));
    deliver(&mut app); // records both accepted bindings before either speaks
    send(&mut app, alice, "old account's first message");
    transmit(&mut app);
    authenticate(&mut app, PeerId::Netcode(1), "Replacement");
    app.world_mut().run_schedule(Update);
    transmit(&mut app);
    assert!(received(&mut app, bob).is_empty());
    assert_eq!(app.world().resource::<ChatState>().sequence, 0);
    send(&mut app, alice, "new account's first message");
    deliver(&mut app);
    assert_eq!(
        accepted(&received(&mut app, bob)),
        [(1, "Replacement", "new account's first message")]
    );
}

#[test]
fn chat_limiter_does_not_bank_unbounded_credit_or_allow_invalid_floods() {
    let mut session = ChatSession::new("alice".into(), Duration::ZERO);
    for _ in 0..3 {
        assert!(session.take_token(Duration::ZERO));
    }
    assert!(!session.take_token(Duration::ZERO));
    assert!(!session.take_token(Duration::from_millis(1999)));
    assert!(session.take_token(Duration::from_secs(2)));
    assert!(!session.take_token(Duration::from_secs(2)));
    for _ in 0..3 {
        assert!(session.take_token(Duration::from_secs(1000)));
    }
    assert!(!session.take_token(Duration::from_secs(1000)));
}
