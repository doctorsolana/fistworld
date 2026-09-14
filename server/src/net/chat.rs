//! Ephemeral server-wide chat, independent of simulation time and region interest.

use std::{collections::HashMap, time::Duration};

use bevy::prelude::*;
use lightyear::prelude::{server::ClientOf, *};
use shared::protocol::{validate_chat_text, ChatChannel, ChatEvent, ChatRejectionReason, ChatSend};

use crate::persistence::profiles::PlayerProfiles;

const BURST_MESSAGES: f64 = 3.0;
const REFILL_INTERVAL: Duration = Duration::from_secs(2);
const MAX_REQUESTS_PER_PASS: usize = 4;

#[derive(Debug)]
struct ChatSession {
    account: String,
    tokens: f64,
    last_refill: Duration,
}

impl ChatSession {
    fn new(account: String, now: Duration) -> Self {
        Self {
            account,
            tokens: BURST_MESSAGES,
            last_refill: now,
        }
    }

    fn take_token(&mut self, now: Duration) -> bool {
        self.tokens = (self.tokens
            + now.saturating_sub(self.last_refill).as_secs_f64() / REFILL_INTERVAL.as_secs_f64())
        .min(BURST_MESSAGES);
        self.last_refill = now;
        if self.tokens < 1.0 {
            return false;
        }
        self.tokens -= 1.0;
        true
    }
}

#[derive(Resource, Default)]
struct ChatState {
    sessions: HashMap<Entity, ChatSession>,
    sequence: u64,
}

/// Chat has no fixed/world-time dependency: PreUpdate receives network messages,
/// Update processes them once, and PostUpdate sends the ordered result.
pub(crate) fn install(app: &mut App) {
    app.init_resource::<ChatState>()
        .add_systems(Update, handle_chat)
        .add_observer(clear_disconnected_chat);
}

fn current_account<'a>(profiles: &'a PlayerProfiles, peer: PeerId) -> Option<(&'a str, &'a str)> {
    let account = profiles.peer_to_name.get(&peer)?;
    if profiles.name_to_peer.get(account) != Some(&peer) {
        return None;
    }
    let profile = profiles.profiles.get(account)?;
    Some((account, profile.player_name.as_str()))
}

type CurrentLinks<'w, 's> = Query<
    'w,
    's,
    (Entity, &'static RemoteId),
    (With<ClientOf>, With<Connected>, Without<Disconnected>),
>;

fn handle_chat(
    time: Res<Time<Real>>,
    profiles: Res<PlayerProfiles>,
    mut state: ResMut<ChatState>,
    links: CurrentLinks,
    mut inboxes: Query<&mut MessageReceiver<ChatSend>>,
    mut outboxes: Query<&mut MessageSender<ChatEvent>>,
    mut broadcasts: Local<Vec<ChatEvent>>,
) {
    let now = time.elapsed();
    broadcasts.clear();
    // Despawn, logout and account replacement must not retain a limiter or an
    // old account's queued messages. These lookups touch only connection entities.
    state.sessions.retain(|entity, session| {
        let valid = links
            .get(*entity)
            .ok()
            .and_then(|(_, remote)| current_account(&profiles, remote.0))
            .is_some_and(|(account, _)| account == session.account);
        if !valid {
            if let Ok(mut inbox) = inboxes.get_mut(*entity) {
                drop(inbox.receive());
            }
            if let Ok(mut outbox) = outboxes.get_mut(*entity) {
                *outbox = MessageSender::default();
            }
        }
        valid
    });

    for (entity, remote) in &links {
        let Ok(mut inbox) = inboxes.get_mut(entity) else {
            continue;
        };
        let Some((account, display_name)) = current_account(&profiles, remote.0) else {
            if inbox.has_messages() {
                *inbox = MessageReceiver::default();
            }
            continue;
        };
        let session = state
            .sessions
            .entry(entity)
            .or_insert_with(|| ChatSession::new(account.to_owned(), now));
        if !inbox.has_messages() {
            continue;
        }
        let overflow = inbox.num_messages() > MAX_REQUESTS_PER_PASS;
        // receive_with_tick owns Vec::drain(..): dropping take() also drops every
        // excess request. Overflow cannot become a delayed flood on later ticks.
        for received in inbox.receive_with_tick().take(MAX_REQUESTS_PER_PASS) {
            if received.channel_kind.0 != std::any::TypeId::of::<ChatChannel>() {
                continue;
            }
            let validated = if session.take_token(now) {
                validate_chat_text(&received.data.text)
            } else {
                Err(ChatRejectionReason::RateLimited)
            };
            match validated {
                Ok(text) => broadcasts.push(ChatEvent::Message {
                    sequence: 0, // stamped once below, before fan-out
                    sender: display_name.to_owned(),
                    text: text.to_owned(),
                }),
                Err(reason) => {
                    // Every inspected request gets a result: the client may be
                    // waiting on this rejection before it permits another send.
                    // MAX_REQUESTS_PER_PASS also bounds these private replies.
                    if let Ok(mut sender) = outboxes.get_mut(entity) {
                        sender.send::<ChatChannel>(ChatEvent::Rejected { reason });
                    }
                }
            }
        }
        if overflow {
            // Release a flood's backing capacity instead of retaining its peak
            // allocation for the rest of this connection's lifetime.
            *inbox = MessageReceiver::default();
        }
    }
    if broadcasts.is_empty() {
        return;
    }
    for message in broadcasts.iter_mut() {
        state.sequence = state
            .sequence
            .checked_add(1)
            .expect("chat sequence exhausted");
        if let ChatEvent::Message { sequence, .. } = message {
            *sequence = state.sequence;
        }
    }
    // Everyone receives the same authoritative order, including the sender.
    // There is no region filter and no retained history for future connections.
    for (entity, remote) in &links {
        if current_account(&profiles, remote.0).is_none() {
            continue;
        }
        if let Ok(mut sender) = outboxes.get_mut(entity) {
            for message in broadcasts.iter() {
                sender.send::<ChatChannel>(message.clone());
            }
        }
    }
    broadcasts.clear();
}

fn clear_disconnected_chat(
    trigger: On<Add, Disconnected>,
    mut state: ResMut<ChatState>,
    mut inboxes: Query<&mut MessageReceiver<ChatSend>>,
    mut outboxes: Query<&mut MessageSender<ChatEvent>>,
) {
    state.sessions.remove(&trigger.entity);
    if let Ok(mut inbox) = inboxes.get_mut(trigger.entity) {
        drop(inbox.receive());
    }
    if let Ok(mut outbox) = outboxes.get_mut(trigger.entity) {
        *outbox = MessageSender::default();
    }
}

#[cfg(test)]
mod tests;
