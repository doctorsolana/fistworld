//! The character roster: every person in the world.
//!
//! Built from LIVE ENTITIES, not from profiles on disk. That is the whole point
//! of the change from a player roster: a hero is a person standing somewhere,
//! and it keeps standing there when its owner logs off (heroes outlive
//! connections), so the entity is the truth about who is in the world. Profiles
//! are accounts, which is a different question.

use bevy::prelude::*;
use lightyear::prelude::server::ClientOf;
use lightyear::prelude::{MessageReceiver, MessageSender, RemoteId};

use shared::components::{
    CharacterAffiliation, CharacterAttributes, CharacterKind, CharacterName, Hero,
};
use shared::protocol::{
    CharacterRoster, CharacterRosterEntry, ReliableChannel, RequestCharacterRoster,
};

use crate::persistence::profiles::PlayerProfiles;

/// Answer roster requests with every named character in the world.
pub fn handle_character_roster_requests(
    profiles: Res<PlayerProfiles>,
    characters: Query<(
        &CharacterName,
        &CharacterKind,
        &CharacterAffiliation,
        Option<&CharacterAttributes>,
        Option<&Hero>,
    )>,
    mut client_links: Query<
        (
            &RemoteId,
            &mut MessageReceiver<RequestCharacterRoster>,
            &mut MessageSender<CharacterRoster>,
        ),
        With<ClientOf>,
    >,
) {
    for (remote_id, mut receiver, mut sender) in client_links.iter_mut() {
        if receiver.receive().next().is_none() {
            continue;
        }

        let mut entries: Vec<CharacterRosterEntry> = characters
            .iter()
            .map(|(name, kind, affiliation, attributes, hero)| {
                // A hero is "online" when its owner is connected. A villager is
                // never online -- it is simply present, which is a different
                // thing and must not render as an away marker.
                let online =
                    hero.is_some_and(|hero| profiles.peer_to_name.contains_key(&hero.owner));
                CharacterRosterEntry {
                    name: name.0.clone(),
                    kind: *kind,
                    affiliation: *affiliation,
                    attributes: attributes.copied().unwrap_or_default(),
                    online,
                    is_self: hero.is_some_and(|hero| hero.owner == remote_id.0),
                }
            })
            .collect();

        // Stable order so the client's list does not reshuffle between requests.
        entries.sort_by(|a, b| a.name.cmp(&b.name));

        sender.send::<ReliableChannel>(CharacterRoster { entries });
    }
}
