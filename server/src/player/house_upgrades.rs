//! Authenticated owner commands; AI and players share the same validation path.

use bevy::prelude::*;
use lightyear::prelude::server::*;
use lightyear::prelude::*;
use shared::components::{Hero, PersonId};
use shared::protocol::{HouseUpgradeRequest, HouseUpgradeResponse, ReliableChannel};

use crate::player::hero::OfflineHero;

/// At most one costly placement validation per connection per fixed tick.
/// Consume duplicate requests too, so an accidental double click cannot leave
/// a delayed series of commands in the reliable message queue.
pub fn handle_house_upgrade_requests(world: &mut World) {
    let requests: Vec<_> = world
        .query_filtered::<
            (Entity, &RemoteId, &mut MessageReceiver<HouseUpgradeRequest>),
            With<ClientOf>,
        >()
        .iter_mut(world)
        .filter_map(|(link, remote, mut receiver)| {
            let mut messages = receiver.receive();
            let first = messages.next();
            for _ in messages {}
            first.map(|request| (link, remote.0, request))
        })
        .collect();
    if requests.is_empty() {
        return;
    }
    let heroes: Vec<_> = world
        .query_filtered::<(&Hero, &PersonId), Without<OfflineHero>>()
        .iter(world)
        .map(|(hero, person)| (hero.owner, *person))
        .collect();
    for (link, remote, request) in requests {
        let result = heroes
            .iter()
            .find(|(owner, _)| *owner == remote)
            .ok_or_else(|| "Create your hero before upgrading a house.".to_owned())
            .and_then(|(_, person)| {
                crate::world::house_upgrades::request_upgrade(world, request.house, *person)
            });
        let response = HouseUpgradeResponse {
            house: request.house,
            accepted: result.is_ok(),
            message: result.err().unwrap_or_else(|| {
                "Upper-storey work commissioned. The house remains available during construction."
                    .to_owned()
            }),
        };
        if let Some(mut sender) = world.get_mut::<MessageSender<HouseUpgradeResponse>>(link) {
            sender.send::<ReliableChannel>(response);
        }
    }
}
