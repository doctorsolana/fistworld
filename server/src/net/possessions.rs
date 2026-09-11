//! Recipient-level access to personal money and inventory. Physical actors and
//! their visible carried prop remain public under the ordinary region filter.

#[cfg(test)]
mod tests;

use bevy::{ecs::system::SystemParam, platform::collections::HashSet, prelude::*};
use bevy_replicon::{
    prelude::{AppVisibilityExt, VisibilityFilter},
    server::{visibility::client_visibility::ClientVisibility, ServerSystems},
};
use lightyear::prelude::{PeerId, RemoteId, ReplicationSystems};
use shared::{
    components::{
        BuildingId, CharacterKind, CommandedBy, ConstructionSite, Hero, PersonId, Settlement,
        SettlementBuilding,
    },
    economy::{GoodsInventory, MootMarket, Wallet},
};

use crate::persistence::profiles::PlayerProfiles;

#[derive(Component, Default, Debug, PartialEq, Eq)]
#[component(immutable)]
enum PossessionsAccess {
    #[default]
    Denied,
    /// Explicitly identified market, building or construction storage retains
    /// its existing region-scoped public contract.
    Public,
    Person {
        hero: Option<PeerId>,
        commander: Option<String>,
    },
}

#[derive(Component, Debug, PartialEq, Eq)]
#[component(immutable)]
struct Recipient {
    peer: PeerId,
    account: String,
}

impl VisibilityFilter for PossessionsAccess {
    type ClientComponent = Recipient;
    type Scope = (Wallet, GoodsInventory);

    fn is_visible(&self, _client: Entity, recipient: Option<&Recipient>) -> bool {
        match self {
            Self::Public => true,
            Self::Denied => false,
            Self::Person { hero, commander } => recipient.is_some_and(|recipient| {
                *hero == Some(recipient.peer)
                    || commander
                        .as_deref()
                        .is_some_and(|name| !name.is_empty() && name == recipient.account)
            }),
        }
    }
}

/// Called after ProtocolPlugin has registered the shared component types, and
/// before Startup creates people. Required filters deny even on the first
/// spawn; classification and account changes settle before any packet is built.
pub(crate) fn install(app: &mut App) {
    app.add_visibility_filter::<PossessionsAccess>();
    app.world_mut()
        .register_required_components::<Wallet, PossessionsAccess>();
    app.world_mut()
        .register_required_components::<GoodsInventory, PossessionsAccess>();
    app.add_systems(
        PostUpdate,
        (sync_recipients, sync_access)
            .chain()
            .before(ReplicationSystems::Send)
            .before(ServerSystems::Send),
    );
}

fn sync_recipients(
    mut commands: Commands,
    profiles: Res<PlayerProfiles>,
    links: Query<(Entity, &RemoteId, Option<&Recipient>), With<ClientVisibility>>,
) {
    for (entity, peer, existing) in &links {
        let account = profiles
            .peer_to_name
            .get(&peer.0)
            .filter(|name| !name.is_empty());
        if let Some(account) = account {
            if existing.is_none_or(|r| r.peer != peer.0 || r.account != *account) {
                commands.entity(entity).insert(Recipient {
                    peer: peer.0,
                    account: account.clone(),
                });
            }
        } else if existing.is_some() {
            // Removing a recipient recomputes all its filters, retracting
            // previously visible possessions on logout/account replacement.
            commands.entity(entity).remove::<Recipient>();
        }
    }
}

type ChangedAccess = Or<(
    Added<PossessionsAccess>,
    Changed<Hero>,
    Changed<CommandedBy>,
    Added<CharacterKind>,
    Added<PersonId>,
    Added<BuildingId>,
    Added<Settlement>,
    Added<SettlementBuilding>,
    Added<ConstructionSite>,
    Added<MootMarket>,
)>;

type Subjects<'w, 's> = Query<
    'w,
    's,
    (
        Option<&'static Hero>,
        Option<&'static CommandedBy>,
        Has<CharacterKind>,
        Has<PersonId>,
        Has<BuildingId>,
        Has<Settlement>,
        Has<SettlementBuilding>,
        Has<ConstructionSite>,
        Has<MootMarket>,
        &'static PossessionsAccess,
    ),
>;

#[derive(SystemParam)]
struct RemovedAccess<'w, 's> {
    heroes: RemovedComponents<'w, 's, Hero>,
    commanders: RemovedComponents<'w, 's, CommandedBy>,
    kinds: RemovedComponents<'w, 's, CharacterKind>,
    people: RemovedComponents<'w, 's, PersonId>,
    buildings: RemovedComponents<'w, 's, BuildingId>,
    settlements: RemovedComponents<'w, 's, Settlement>,
    sites: RemovedComponents<'w, 's, SettlementBuilding>,
    construction: RemovedComponents<'w, 's, ConstructionSite>,
    markets: RemovedComponents<'w, 's, MootMarket>,
}

fn sync_access(
    mut commands: Commands,
    changed: Query<Entity, (With<PossessionsAccess>, ChangedAccess)>,
    mut removed: RemovedAccess,
    subjects: Subjects,
    mut dirty: Local<HashSet<Entity>>,
) {
    dirty.clear();
    dirty.extend(changed.iter());
    dirty.extend(removed.heroes.read());
    dirty.extend(removed.commanders.read());
    dirty.extend(removed.kinds.read());
    dirty.extend(removed.people.read());
    dirty.extend(removed.buildings.read());
    dirty.extend(removed.settlements.read());
    dirty.extend(removed.sites.read());
    dirty.extend(removed.construction.read());
    dirty.extend(removed.markets.read());
    for entity in dirty.iter().copied() {
        let Ok((
            hero,
            commander,
            character,
            person,
            building_id,
            settlement,
            building,
            construction,
            market,
            previous,
        )) = subjects.get(entity)
        else {
            continue;
        };
        let next = if character || person || hero.is_some() {
            PossessionsAccess::Person {
                hero: hero.map(|hero| hero.owner),
                commander: commander.map(|owner| owner.0.clone()),
            }
        } else if building_id || settlement || building || construction || market {
            PossessionsAccess::Public
        } else {
            // A partly constructed actor cannot become public just because its
            // CharacterKind/PersonId arrives after its money or inventory.
            PossessionsAccess::Denied
        };
        if *previous != next {
            commands.entity(entity).insert(next);
        }
    }
}
