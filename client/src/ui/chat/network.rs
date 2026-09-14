//! Chat uses its own reliable ordered stream; it never sends gameplay commands.

use bevy::prelude::*;
use lightyear::prelude::{Connected, MessageReceiver, MessageSender};
use shared::protocol::{ChatChannel, ChatEvent, ChatSend};

use super::ChatState;
use crate::ui::name_entry::PlayerNameInput;

pub(super) fn observe_connection(
    mut state: ResMut<ChatState>,
    connections: Query<Entity, (With<crate::GameClient>, With<Connected>)>,
) {
    state.set_connection(connections.iter().next());
}

pub(super) fn receive(
    mut state: ResMut<ChatState>,
    mut receivers: Query<
        &mut MessageReceiver<ChatEvent>,
        (With<crate::GameClient>, With<Connected>),
    >,
    account: Option<Res<PlayerNameInput>>,
    time: Res<Time<Real>>,
) {
    let local_name = account.as_ref().map_or("", |account| account.name.as_str());
    for mut receiver in &mut receivers {
        for event in receiver.receive() {
            state.receive(event, time.elapsed_secs_f64(), local_name);
        }
    }
}

pub(super) fn send(
    mut state: ResMut<ChatState>,
    mut senders: Query<&mut MessageSender<ChatSend>, (With<crate::GameClient>, With<Connected>)>,
) {
    if let Some(mut sender) = senders.iter_mut().next() {
        if let Some(message) = state.take_unsent() {
            sender.send::<ChatChannel>(message);
        }
    }
}
