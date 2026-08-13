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
    CharacterAffiliation, CharacterAttributes, CharacterKind, CharacterName, Health, Hero, PersonId,
};
use shared::protocol::{
    CharacterRoster, CharacterRosterEntry, ReliableChannel, RequestCharacterRoster,
};

use crate::persistence::profiles::PlayerProfiles;

/// Answer roster requests with every named character in the world.
pub fn handle_character_roster_requests(
    profiles: Res<PlayerProfiles>,
    mortality: Res<crate::world::village::MortalityLedger>,
    characters: Query<(
        &CharacterName,
        &CharacterKind,
        &CharacterAffiliation,
        Option<&CharacterAttributes>,
        Option<&Hero>,
        Option<&PersonId>,
        Option<&Health>,
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
            .map(
                |(name, kind, affiliation, attributes, hero, person_id, health)| {
                    // A hero is "online" when its owner is connected. A villager is
                    // never online -- it is simply present, which is a different
                    // thing and must not render as an away marker.
                    let online =
                        hero.is_some_and(|hero| profiles.peer_to_name.contains_key(&hero.owner));
                    CharacterRosterEntry {
                        id: person_id.copied().unwrap_or_default(),
                        name: name.0.clone(),
                        kind: *kind,
                        affiliation: *affiliation,
                        attributes: attributes.copied().unwrap_or_default(),
                        health: health.cloned().unwrap_or_default(),
                        alive: true,
                        death_day: None,
                        death_cause: None,
                        online,
                        is_self: hero.is_some_and(|hero| hero.owner == remote_id.0),
                    }
                },
            )
            .collect();

        let requesting_account = profiles.peer_to_name.get(&remote_id.0);
        entries.extend(mortality.iter().map(|record| {
            CharacterRosterEntry {
                id: record.id,
                name: record.name.clone(),
                kind: record.kind,
                affiliation: record.affiliation,
                attributes: record.attributes,
                health: Health {
                    current: 0.0,
                    max: shared::components::CHARACTER_MAX_HEALTH,
                },
                alive: false,
                death_day: Some(record.day),
                death_cause: Some(record.cause),
                online: false,
                is_self: record
                    .commanded_by
                    .as_ref()
                    .zip(requesting_account)
                    .is_some_and(|(dead, current)| dead == current),
            }
        }));

        // Stable order so the client's list does not reshuffle between requests.
        entries.sort_by(|a, b| a.name.cmp(&b.name));

        sender.send::<ReliableChannel>(CharacterRoster { entries });
    }
}
