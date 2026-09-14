//! Owner controls for a funded extension of an existing, occupied house.

use bevy::prelude::*;
use lightyear::prelude::{Connected, MessageReceiver, MessageSender};
use shared::components::{
    BuildingId, BuildingOf, ConstructionSite, Hero, HouseAppearance, HouseLevel,
    HouseUpgradeWorksite, OwnedBy, PersonId, Settlement, SettlementBuilding,
    SettlementBuildingKind, SettlementId, SettlementSummary, SettlementTier,
    HOUSE_UPGRADE_ESCROW_PENNIES, HOUSE_UPGRADE_WOOD_REQUIRED,
};
use shared::economy::{format_money, Good, GoodsInventory, Wallet};
use shared::protocol::{HouseUpgradeRequest, HouseUpgradeResponse, ReliableChannel};

use super::foundation::{button_chrome, UiButtonLabel, UiButtonVariant};
use super::styles::{ACCENT_RED, INK, INK_MUTED, PLATE_RULE_SOFT};
use crate::camera_rts::LocalPeerId;
use crate::selection::Selection;
use crate::states::GameState;

pub struct HouseUpgradesPlugin;

impl Plugin for HouseUpgradesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<UpgradeRequests>();
        app.add_systems(
            Update,
            (
                receive_results,
                attach_controls,
                sync_controls,
                submit_upgrade,
            )
                .chain()
                .run_if(in_state(GameState::Playing)),
        );
        app.add_systems(OnExit(GameState::Playing), reset_requests);
    }
}

/// A retained extension of the ordinary compact building inspector.
#[derive(Component)]
pub(crate) struct HouseUpgradeMount;

#[derive(Component)]
struct Mounted;

#[derive(Component, Clone, Copy)]
enum Bound {
    Summary,
    Requirements,
    Status,
    Feedback,
    Button,
}

#[derive(Component, Default)]
struct UpgradeAction(Option<BuildingId>);

#[derive(Resource, Default)]
struct UpgradeRequests {
    pending: Option<BuildingId>,
    feedback: Option<HouseUpgradeResponse>,
}

/// Read-only evidence for the connected session capture driver.
pub(crate) fn inspect_requests(
    world: &World,
) -> (Option<BuildingId>, Option<HouseUpgradeResponse>) {
    world
        .get_resource::<UpgradeRequests>()
        .map(|requests| (requests.pending, requests.feedback.clone()))
        .unwrap_or_default()
}

fn reset_requests(mut requests: ResMut<UpgradeRequests>) {
    *requests = default();
}

fn receive_results(
    mut messages: Query<&mut MessageReceiver<HouseUpgradeResponse>, With<crate::GameClient>>,
    connected: Query<(), (With<crate::GameClient>, With<Connected>)>,
    mut requests: ResMut<UpgradeRequests>,
) {
    if connected.is_empty() {
        if let Some(house) = requests.pending.take() {
            requests.feedback = Some(HouseUpgradeResponse {
                house,
                accepted: false,
                message: "Connection lost. Reconnect to check your house upgrade.".into(),
            });
        }
    }
    for mut receiver in &mut messages {
        for response in receiver.receive() {
            if requests.pending == Some(response.house) {
                requests.pending = None;
            }
            requests.feedback = Some(response.clone());
        }
    }
}

fn attach_controls(
    mut commands: Commands,
    roots: Query<Entity, (With<HouseUpgradeMount>, Without<Mounted>)>,
) {
    for entity in &roots {
        commands
            .entity(entity)
            .insert(Mounted)
            .with_children(|root| {
                for (field, size) in [
                    (Bound::Summary, 14.0),
                    (Bound::Requirements, 12.0),
                    (Bound::Status, 12.0),
                ] {
                    root.spawn((
                        field,
                        Text::new(""),
                        super::typography::text(size),
                        TextColor(INK_MUTED),
                    ));
                }
                root.spawn((
                    Name::new("house-upgrade-upper-storey"),
                    UpgradeAction::default(),
                    Button,
                    bevy::ui::InteractionDisabled,
                    Node {
                        width: Val::Percent(100.0),
                        min_height: Val::Px(38.0),
                        padding: UiRect::axes(Val::Px(8.0), Val::Px(8.0)),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        ..default()
                    },
                    button_chrome(UiButtonVariant::Primary),
                    children![(
                        Bound::Button,
                        UiButtonLabel,
                        Text::new(""),
                        super::typography::text(12.0),
                        TextColor(INK),
                        TextLayout::justify(Justify::Center),
                        Pickable::IGNORE
                    )],
                ));
                root.spawn((
                    Bound::Feedback,
                    Text::new(""),
                    super::typography::text(12.0),
                    TextColor(INK_MUTED),
                ));
            });
    }
}

#[derive(Debug, PartialEq, Eq)]
struct UpgradeView {
    summary: String,
    requirements: String,
    status: String,
    button: String,
    enabled: bool,
}

fn visible_feedback(
    response: Option<&HouseUpgradeResponse>,
    house: BuildingId,
    level: HouseLevel,
) -> Option<&HouseUpgradeResponse> {
    response.filter(|response| {
        response.house == house && (level != HouseLevel::UpperStorey || !response.accepted)
    })
}

fn view(
    level: HouseLevel,
    tier: Option<SettlementTier>,
    cash: u64,
    work: Option<(u32, u32, bool)>,
    pending: bool,
) -> UpgradeView {
    let capacity = level.housing_capacity();
    let mut result = UpgradeView {
        summary: format!("Upper storey · {capacity} beds now"),
        requirements: format!(
            "{} Wood · reserve {} coin",
            HOUSE_UPGRADE_WOOD_REQUIRED,
            format_money(HOUSE_UPGRADE_ESCROW_PENNIES)
        ),
        status: format!(
            "Your purse: {} coin. Unused reserve is returned.",
            format_money(cash)
        ),
        button: "UPGRADE UPPER STOREY · 4 → 8 BEDS".into(),
        enabled: true,
    };
    if level == HouseLevel::UpperStorey {
        result.summary = "Upper storey complete · 8 beds".into();
        result.requirements.clear();
        result.status = "The completed house provides all eight beds.".into();
        result.button = "UPPER STOREY COMPLETE".into();
        result.enabled = false;
    } else if let Some((delivered, required, raising)) = work {
        result.requirements = format!("Wood delivered: {delivered} / {required}");
        result.status = if raising {
            "Building the upper storey. Four beds remain usable."
        } else {
            "Waiting for affordable Wood and an available builder. Four beds remain usable."
        }
        .into();
        result.button = "UPGRADE IN PROGRESS".into();
        result.enabled = false;
    } else if pending {
        result.status = "Waiting for the settlement to accept your order.".into();
        result.button = "REQUESTING UPGRADE…".into();
        result.enabled = false;
    } else if tier.is_none_or(|tier| tier < SettlementTier::Village) {
        result.status = if tier.is_some() {
            "Available when the settlement reaches Village."
        } else {
            "Waiting for this settlement's current tier."
        }
        .into();
        result.enabled = false;
    } else if cash < HOUSE_UPGRADE_ESCROW_PENNIES {
        result.status = format!(
            "Your purse: {} coin. Reserve {} coin to begin.",
            format_money(cash),
            format_money(HOUSE_UPGRADE_ESCROW_PENNIES)
        );
        result.enabled = false;
    }
    result
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn sync_controls(
    mut commands: Commands,
    selection: Res<Selection>,
    local: Option<Res<LocalPeerId>>,
    heroes: Query<(&Hero, &PersonId, &Wallet)>,
    houses: Query<(
        &BuildingId,
        &SettlementBuilding,
        Option<&HouseAppearance>,
        &OwnedBy,
        &BuildingOf,
    )>,
    settlements: Query<(&SettlementId, &Settlement)>,
    summaries: Query<&SettlementSummary>,
    worksites: Query<(&HouseUpgradeWorksite, &ConstructionSite, &GoodsInventory)>,
    requests: Res<UpgradeRequests>,
    mut mounts: Query<&mut Node, With<HouseUpgradeMount>>,
    mut texts: Query<(&Bound, &mut Text, &mut TextColor)>,
    mut buttons: Query<(
        Entity,
        &mut UpgradeAction,
        Has<bevy::ui::InteractionDisabled>,
    )>,
) {
    let selected = selection
        .primary()
        .and_then(|entity| houses.get(entity).ok());
    let local_hero = local.as_ref().and_then(|local| {
        heroes
            .iter()
            .find(|(hero, ..)| shared::player::peer_id_to_u64(hero.owner) == local.0)
    });
    let owned = selected
        .zip(local_hero)
        .filter(|((_, building, _, owner, _), (_, person, _))| {
            building.kind == SettlementBuildingKind::House && owner.0 == **person
        });
    for mut node in &mut mounts {
        let display = if owned.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
    let Some(((id, _, appearance, _, building_of), (_, _, wallet))) = owned else {
        return;
    };
    let level = appearance.copied().unwrap_or_default().level;
    let tier = settlements
        .iter()
        .find(|(id, _)| **id == building_of.0)
        .map(|(_, town)| town.tier)
        // An outer house can remain in view after its Hall detail leaves
        // regional interest. The global directory still supplies its tier.
        .or_else(|| {
            summaries
                .iter()
                .find(|summary| summary.id == building_of.0)
                .map(|summary| summary.tier)
        });
    let work = worksites
        .iter()
        .find(|(work, ..)| work.house == *id)
        .map(|(work, site, store)| (store.amount(Good::Wood), work.wood_required, site.raising));
    let model = view(
        level,
        tier,
        wallet.balance(),
        work,
        requests.pending.is_some(),
    );
    let feedback = visible_feedback(requests.feedback.as_ref(), *id, level);
    for (field, mut text, mut color) in &mut texts {
        let value: &str = match field {
            Bound::Summary => &model.summary,
            Bound::Requirements => &model.requirements,
            Bound::Status => &model.status,
            Bound::Button => &model.button,
            Bound::Feedback => feedback.map_or("", |response| response.message.as_str()),
        };
        if text.0 != value {
            text.0 = value.to_owned();
        }
        if matches!(field, Bound::Button) {
            // The shared button style owns enabled/disabled label colours.
            continue;
        }
        let next = if matches!(field, Bound::Feedback)
            && feedback.is_some_and(|response| !response.accepted)
        {
            ACCENT_RED
        } else if matches!(field, Bound::Summary | Bound::Button) {
            INK
        } else {
            INK_MUTED
        };
        if color.0 != next {
            color.0 = next;
        }
    }
    for (entity, mut action, disabled) in &mut buttons {
        let desired = model.enabled.then_some(*id);
        if action.0 != desired {
            action.0 = desired;
        }
        if model.enabled && disabled {
            commands
                .entity(entity)
                .remove::<bevy::ui::InteractionDisabled>();
        }
        if !model.enabled && !disabled {
            commands
                .entity(entity)
                .insert(bevy::ui::InteractionDisabled);
        }
    }
}

fn submit_upgrade(
    buttons: Query<
        (&Interaction, &UpgradeAction),
        (Changed<Interaction>, Without<bevy::ui::InteractionDisabled>),
    >,
    mut senders: Query<
        &mut MessageSender<HouseUpgradeRequest>,
        (With<crate::GameClient>, With<Connected>),
    >,
    mut requests: ResMut<UpgradeRequests>,
) {
    if requests.pending.is_some() {
        return;
    }
    for (interaction, action) in &buttons {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let Some(house) = action.0 else {
            continue;
        };
        if let Ok(mut sender) = senders.single_mut() {
            sender.send::<ReliableChannel>(HouseUpgradeRequest { house });
            requests.pending = Some(house);
            requests.feedback = None;
        } else {
            requests.feedback = Some(HouseUpgradeResponse {
                house,
                accepted: false,
                message: "Reconnect before requesting a house upgrade.".into(),
            });
        }
    }
}

pub(crate) fn mount() -> impl Bundle {
    (
        HouseUpgradeMount,
        Node {
            display: Display::None,
            width: Val::Percent(100.0),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(6.0),
            margin: UiRect::top(Val::Px(8.0)),
            padding: UiRect::top(Val::Px(10.0)),
            border: UiRect::top(Val::Px(1.0)),
            ..default()
        },
        BorderColor::all(PLATE_RULE_SOFT),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn completed_house_hides_old_commissioning_feedback_but_keeps_rejections() {
        let mut response = HouseUpgradeResponse {
            house: BuildingId(88),
            accepted: true,
            message: "Upper-storey work commissioned.".into(),
        };
        assert!(visible_feedback(Some(&response), BuildingId(88), HouseLevel::Ground).is_some());
        assert!(
            visible_feedback(Some(&response), BuildingId(88), HouseLevel::UpperStorey).is_none()
        );
        response.accepted = false;
        assert!(
            visible_feedback(Some(&response), BuildingId(88), HouseLevel::UpperStorey).is_some()
        );
        assert!(visible_feedback(Some(&response), BuildingId(99), HouseLevel::Ground).is_none());
    }

    #[test]
    fn pressed_owner_button_waits_for_one_request_and_exposes_connection_failure() {
        let mut app = App::new();
        app.init_resource::<UpgradeRequests>()
            .add_systems(Update, submit_upgrade);
        let button = app
            .world_mut()
            .spawn((
                Button,
                Interaction::Pressed,
                UpgradeAction(Some(BuildingId(88))),
                bevy::ui::InteractionDisabled,
            ))
            .id();
        app.update();
        assert_eq!(inspect_requests(app.world()).0, None);
        assert!(inspect_requests(app.world()).1.is_none());

        app.world_mut()
            .entity_mut(button)
            .remove::<bevy::ui::InteractionDisabled>();
        *app.world_mut().get_mut::<Interaction>(button).unwrap() = Interaction::Pressed;
        app.update();
        let (pending, feedback) = inspect_requests(app.world());
        assert!(pending.is_none());
        assert_eq!(feedback.as_ref().unwrap().house, BuildingId(88));
        assert!(feedback.unwrap().message.contains("Reconnect"));

        app.world_mut().spawn((
            crate::GameClient,
            lightyear::prelude::RemoteId(lightyear::prelude::PeerId::Server),
            Connected,
            MessageSender::<HouseUpgradeRequest>::default(),
        ));
        *app.world_mut().get_mut::<Interaction>(button).unwrap() = Interaction::Pressed;
        app.update();
        let (pending, feedback) = inspect_requests(app.world());
        assert_eq!(pending, Some(BuildingId(88)));
        assert!(feedback.is_none());

        app.world_mut().get_mut::<UpgradeAction>(button).unwrap().0 = Some(BuildingId(99));
        *app.world_mut().get_mut::<Interaction>(button).unwrap() = Interaction::Pressed;
        app.update();
        assert_eq!(inspect_requests(app.world()).0, Some(BuildingId(88)));
    }

    #[test]
    fn eligibility_uses_physical_level_tier_and_owner_reserve() {
        assert!(
            !view(
                HouseLevel::Ground,
                Some(SettlementTier::Hamlet),
                u64::MAX,
                None,
                false
            )
            .enabled
        );
        assert!(
            !view(
                HouseLevel::Ground,
                Some(SettlementTier::Village),
                HOUSE_UPGRADE_ESCROW_PENNIES - 1,
                None,
                false
            )
            .enabled
        );
        assert!(
            view(
                HouseLevel::Ground,
                Some(SettlementTier::Village),
                HOUSE_UPGRADE_ESCROW_PENNIES,
                None,
                false
            )
            .enabled
        );
        assert!(
            !view(
                HouseLevel::UpperStorey,
                Some(SettlementTier::Village),
                u64::MAX,
                None,
                false
            )
            .enabled
        );
    }
    #[test]
    fn global_tier_keeps_outer_house_action_available_and_live_hall_takes_precedence() {
        let mut app = App::new();
        app.init_resource::<UpgradeRequests>()
            .insert_resource(LocalPeerId(42))
            .add_systems(Update, sync_controls);
        app.world_mut().spawn((
            Hero {
                owner: lightyear::prelude::PeerId::Netcode(42),
            },
            PersonId(7),
            Wallet::new(HOUSE_UPGRADE_ESCROW_PENNIES),
        ));
        let home = app
            .world_mut()
            .spawn((
                BuildingId(88),
                HouseAppearance::default(),
                SettlementBuilding {
                    kind: SettlementBuildingKind::House,
                    settlement: "Outer home".into(),
                    owner: None,
                    quality: 1.0,
                    workers: Vec::new(),
                },
                OwnedBy(PersonId(7)),
                BuildingOf(SettlementId(22)),
            ))
            .id();
        app.insert_resource(Selection::from_entities(vec![home]));
        let button = app.world_mut().spawn(UpgradeAction::default()).id();
        let summary = app
            .world_mut()
            .spawn(SettlementSummary {
                id: SettlementId(999),
                name: "Directory".into(),
                tier: SettlementTier::Village,
                residents: 4,
                treasury: 0,
                prosperity: 0.0,
                reserve_days: 0.0,
                recent_food_production: 0.0,
                recent_food_consumption: 0.0,
                hungry: 0,
                housing_capacity: 4,
                homeless: 0,
                job_seekers: 0,
                unpaid_workers: 0,
                unrest: 0.0,
                unrest_change: 0.0,
                unrest_target: 0.0,
                unrest_hunger_pressure: 0.0,
                unrest_housing_pressure: 0.0,
                unrest_wage_pressure: 0.0,
                houses: 1,
                farmsteads: 0,
                fishing_huts: 0,
                lumber_huts: 0,
                windmills: 0,
                bakeries: 0,
                has_marketplace: false,
            })
            .id();
        app.update();
        assert_eq!(
            app.world().get::<UpgradeAction>(button).unwrap().0,
            None,
            "another settlement's tier does not qualify this house"
        );
        app.world_mut()
            .get_mut::<SettlementSummary>(summary)
            .unwrap()
            .id = SettlementId(22);
        app.update();
        assert_eq!(
            app.world().get::<UpgradeAction>(button).unwrap().0,
            Some(BuildingId(88)),
            "the global summary works without Hall detail"
        );
        let hall = app
            .world_mut()
            .spawn((
                SettlementId(22),
                Settlement {
                    name: "Live Hall".into(),
                    tier: SettlementTier::Hamlet,
                    residents: 4,
                    treasury: 0,
                },
            ))
            .id();
        app.update();
        assert_eq!(
            app.world().get::<UpgradeAction>(button).unwrap().0,
            None,
            "current regional detail takes precedence over the directory"
        );
        app.world_mut().despawn(hall);
        app.update();
        assert_eq!(
            app.world().get::<UpgradeAction>(button).unwrap().0,
            Some(BuildingId(88))
        );
    }
    #[test]
    fn lost_connection_clears_pending_request_without_leaving_playing() {
        let mut app = App::new();
        app.insert_resource(UpgradeRequests {
            pending: Some(BuildingId(44)),
            feedback: None,
        })
        .add_systems(Update, receive_results);
        app.world_mut().spawn(crate::GameClient);
        app.update();
        let requests = app.world().resource::<UpgradeRequests>();
        assert!(requests.pending.is_none());
        let feedback = requests.feedback.as_ref().unwrap();
        assert_eq!(feedback.house, BuildingId(44));
        assert!(!feedback.accepted);
        assert!(feedback.message.contains("Connection lost"));
    }
    #[test]
    fn active_construction_keeps_four_beds_until_completed() {
        let model = view(
            HouseLevel::Ground,
            Some(SettlementTier::Village),
            0,
            Some((8, 8, true)),
            false,
        );
        assert!(!model.enabled);
        assert!(model.summary.contains("4 beds now"));
        assert!(model.status.contains("Four beds remain usable"));
    }
}
