//! Player roster queries.

use bevy::prelude::*;
use lightyear::prelude::server::ClientOf;
use lightyear::prelude::{MessageReceiver, MessageSender, RemoteId};

use shared::protocol::{PlayerRoster, ReliableChannel, RequestPlayerRoster};

use crate::persistence::profiles::PlayerProfiles;
use crate::player::roster_cache::PlayerRosterCache;

/// Handle roster requests from clients.
pub fn handle_player_roster_requests(
    profiles: Res<PlayerProfiles>,
    roster_cache: Res<PlayerRosterCache>,
    mut client_links: Query<
        (
            &RemoteId,
            &mut MessageReceiver<RequestPlayerRoster>,
            &mut MessageSender<PlayerRoster>,
        ),
        With<ClientOf>,
    >,
) {
    for (_remote_id, mut receiver, mut sender) in client_links.iter_mut() {
        if receiver.receive().next().is_none() {
            continue;
        }

        let entries = roster_cache.build_roster(&profiles);
        sender.send::<ReliableChannel>(PlayerRoster { entries });
    }
}
