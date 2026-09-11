//! The settlement market as an encyclopedia Places page.
//!
//! A market is knowledge about a place, not a second top-level modal. Every
//! entry point (the compact inspector, the town record, and nearby `E`
//! interaction) sets [`MarketPageTarget`], opens Places, and renders here.
//! Live prices bind into stable widgets so a fast simulation never despawns a
//! button from under the pointer.

mod model;
use model::{market_page_model, MarketPageModel, MarketRowModel};

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::ui::InteractionDisabled;
use lightyear::prelude::{Connected, MessageReceiver, MessageSender};

use shared::components::{
    BuildingOf, CivicHallLevel, PlayerPosition, PlayerRotation, Settlement, SettlementBuilding,
    SettlementBuildingKind, SettlementId,
};
use shared::economy::{format_money, Good, GoodsInventory, MootMarket, Wallet};
use shared::protocol::{HeroMarketAction, HeroMarketOrder, HeroMarketResult, ReliableChannel};

use crate::states::GameState;
use crate::ui::encyclopedia::{
    places::{SelectedPlace, SelectedPlaceEntry},
    EncyclopediaOpen, EncyclopediaPageHost, EncyclopediaTab,
};
use crate::ui::foundation::{button_chrome, UiButtonLabel, UiButtonStyle, UiButtonVariant};
use crate::ui::good_icon_path;
use crate::ui::styles::{
    INK, INK_MUTED, LIMEWASH, LIMEWASH_DETAIL, LIMEWASH_LIT, LIMEWASH_WELL, PLATE_RULE_SOFT,
    RADIUS, STATUS_GOOD,
};

const T_TITLE: f32 = 22.0;
const T_HEADING: f32 = 17.0;
const T_VALUE: f32 = 15.0;
const T_BUTTON: f32 = 13.5;
const T_BODY: f32 = 13.5;
const T_LABEL: f32 = 11.5;
pub(crate) const HERO_MARKET_INTERACTION_RANGE: f32 = 12.0;

pub struct MarketPlugin;

impl Plugin for MarketPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MarketPageTarget>();
        app.init_resource::<MarketFeedback>();
        app.add_systems(
            Update,
            (
                sync_place_market_action,
                handle_open_market_buttons,
                open_nearby_market_on_interact,
                handle_market_trade_buttons,
                receive_market_trade_results,
                sync_market_page,
            )
                .chain()
                .run_if(in_state(GameState::Playing)),
        );
        app.add_systems(OnExit(GameState::Playing), despawn_market_page);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MarketPage {
    pub settlement: Entity,
    /// Display only. Selection travels on `place_id` - names collide.
    pub place: String,
    pub place_id: shared::components::SettlementId,
}

#[derive(Resource, Default)]
pub(crate) struct MarketPageTarget(pub Option<MarketPage>);

/// Static action slot on the selected Places record. It is only shown and
/// armed when the selected place currently has a replicated public market.
#[derive(Component)]
pub(crate) struct PlaceMarketAction;

#[derive(Component, Clone, Debug, PartialEq, Eq)]
struct OpenMarketButton(MarketPage);

#[derive(Resource, Default)]
struct MarketFeedback {
    message: String,
    success: bool,
    market: Option<Entity>,
    // Orders and replies share an ordered reliable stream. Keep the owning
    // page even if the player changes markets before a reply arrives.
    pending: std::collections::VecDeque<Entity>,
}

#[derive(Component)]
struct MarketPageRoot {
    settlement: Entity,
}

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
enum MarketSummaryField {
    OnOffer,
    CommonStore,
    Today,
    Lifetime,
    HeroFunds,
}

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
enum MarketRowField {
    Condition,
    Store,
    Listed,
    LastSale,
    BestOffer,
    Today,
    HeroCargo,
}

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
enum MarketBoundText {
    Title,
    Subtitle,
    Access,
    Summary(MarketSummaryField),
    Row(Good, MarketRowField),
}

#[derive(Component)]
struct MarketAccessText;

#[derive(Component)]
struct MarketFeedbackText;

#[derive(Component)]
struct MarketViewport;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MarketButtonKind {
    Buy,
    Offer,
}

/// Client-side availability is presentation only. The server repeats every
/// distance, ownership, capacity and cash check before moving an item.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
struct MarketTradeButton {
    market: Entity,
    good: Good,
    kind: MarketButtonKind,
    action: HeroMarketAction,
    enabled: bool,
}

#[derive(Component, Clone, Copy)]
struct MarketActionLabel {
    good: Good,
    kind: MarketButtonKind,
}

#[derive(SystemParam)]
struct MarketWorld<'w, 's> {
    settlements: Query<
        'w,
        's,
        (
            &'static Settlement,
            &'static SettlementId,
            Option<&'static CivicHallLevel>,
            &'static PlayerPosition,
            Option<&'static PlayerRotation>,
        ),
    >,
    buildings: Query<
        'w,
        's,
        (
            &'static SettlementBuilding,
            Option<&'static BuildingOf>,
            Option<&'static PlayerPosition>,
            Option<&'static PlayerRotation>,
        ),
    >,
    inventories: Query<'w, 's, &'static GoodsInventory>,
    markets: Query<'w, 's, &'static MootMarket>,
    heroes: Query<
        'w,
        's,
        (
            &'static shared::components::Hero,
            &'static PlayerPosition,
            Option<&'static GoodsInventory>,
            Option<&'static Wallet>,
            Option<&'static shared::components::PersonId>,
        ),
    >,
    local: Option<Res<'w, crate::camera_rts::LocalPeerId>>,
}

#[derive(SystemParam)]
struct MarketPageUi<'w, 's> {
    roots: Query<'w, 's, (Entity, &'static MarketPageRoot, &'static mut Node)>,
    texts: Query<
        'w,
        's,
        (&'static MarketBoundText, &'static mut Text),
        (Without<MarketFeedbackText>, Without<MarketActionLabel>),
    >,
    access: Query<
        'w,
        's,
        &'static mut TextColor,
        (With<MarketAccessText>, Without<MarketFeedbackText>),
    >,
    feedback: Query<
        'w,
        's,
        (&'static mut Text, &'static mut TextColor),
        (
            With<MarketFeedbackText>,
            Without<MarketBoundText>,
            Without<MarketActionLabel>,
            Without<MarketAccessText>,
        ),
    >,
    actions: Query<
        'w,
        's,
        (
            Entity,
            &'static mut MarketTradeButton,
            Has<InteractionDisabled>,
            &'static mut UiButtonStyle,
        ),
    >,
    action_labels: Query<
        'w,
        's,
        (&'static MarketActionLabel, &'static mut Text),
        (Without<MarketBoundText>, Without<MarketFeedbackText>),
    >,
}

pub(crate) fn nearest_public_market_entrance(
    origin: Vec3,
    hall_entrance: Vec3,
    marketplace_entrances: impl IntoIterator<Item = Vec3>,
) -> Vec3 {
    let distance_squared =
        |point: Vec3| Vec2::new(origin.x, origin.z).distance_squared(Vec2::new(point.x, point.z));
    marketplace_entrances
        .into_iter()
        .fold(hall_entrance, |nearest, candidate| {
            if distance_squared(candidate) < distance_squared(nearest) {
                candidate
            } else {
                nearest
            }
        })
}

fn sync_place_market_action(
    mut commands: Commands,
    selected: Res<SelectedPlace>,
    settlements: Query<(Entity, &Settlement, &shared::components::SettlementId), With<MootMarket>>,
    mut buttons: Query<(Entity, &mut Node, Option<&OpenMarketButton>), With<PlaceMarketAction>>,
) {
    let target = selected.0.and_then(|place_id| {
        settlements
            .iter()
            .find(|(_, _, id)| **id == place_id)
            .map(|(entity, settlement, id)| MarketPage {
                settlement: entity,
                place: settlement.name.clone(),
                place_id: *id,
            })
    });
    for (entity, mut node, current) in buttons.iter_mut() {
        let display = if target.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
        match (&target, current) {
            (Some(target), Some(current)) if current.0 == *target => {}
            (Some(target), _) => {
                commands.entity(entity).insert((
                    OpenMarketButton(target.clone()),
                    Name::new("open-town-market"),
                ));
            }
            (None, Some(_)) => {
                commands.entity(entity).remove::<OpenMarketButton>();
            }
            (None, None) => {}
        }
    }
}

fn handle_open_market_buttons(
    mouse: Res<ButtonInput<MouseButton>>,
    buttons: Query<(&Interaction, &OpenMarketButton), Changed<Interaction>>,
    mut target: ResMut<MarketPageTarget>,
    mut open: ResMut<EncyclopediaOpen>,
    mut tab: ResMut<EncyclopediaTab>,
    mut selected: ResMut<SelectedPlace>,
    mut entry: ResMut<SelectedPlaceEntry>,
) {
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    for (interaction, button) in buttons.iter() {
        if *interaction != Interaction::Pressed {
            continue;
        }
        target.0 = Some(button.0.clone());
        selected.0 = Some(button.0.place_id);
        *entry = SelectedPlaceEntry::Overview;
        *tab = EncyclopediaTab::Places;
        open.0 = true;
    }
}

/// Conventional nearby-world interaction. `E` opens the same encyclopedia
/// page as every other entry point, already in trading range.
fn open_nearby_market_on_interact(
    keyboard: Res<ButtonInput<KeyCode>>,
    input: Res<crate::input::InputState>,
    local: Option<Res<crate::camera_rts::LocalPeerId>>,
    heroes: Query<(&shared::components::Hero, &PlayerPosition)>,
    halls: Query<
        (
            Entity,
            &Settlement,
            &SettlementId,
            &PlayerPosition,
            Option<&PlayerRotation>,
        ),
        With<MootMarket>,
    >,
    marketplaces: Query<(
        &SettlementBuilding,
        &BuildingOf,
        &PlayerPosition,
        &PlayerRotation,
    )>,
    mut target: ResMut<MarketPageTarget>,
    mut open: ResMut<EncyclopediaOpen>,
    mut tab: ResMut<EncyclopediaTab>,
    mut selected: ResMut<SelectedPlace>,
    mut entry: ResMut<SelectedPlaceEntry>,
) {
    if !keyboard.just_pressed(KeyCode::KeyE) || input.ui_blocking() {
        return;
    }
    let Some(local) = local else { return };
    let Some((_, hero_position)) = heroes
        .iter()
        .find(|(hero, _)| shared::player::peer_id_to_u64(hero.owner) == local.0)
    else {
        return;
    };
    let nearby = halls
        .iter()
        .filter_map(|(entity, settlement, settlement_id, position, rotation)| {
            let hall_entrance = SettlementBuildingKind::Hall
                .entrance_position(position.0, rotation.map_or(0.0, |rotation| rotation.0));
            let counter = nearest_public_market_entrance(
                hero_position.0,
                hall_entrance,
                marketplaces
                    .iter()
                    .filter(|(building, building_of, ..)| {
                        building.kind == SettlementBuildingKind::Market
                            && building_of.0 == *settlement_id
                    })
                    .map(|(building, _, position, rotation)| {
                        building.kind.entrance_position(position.0, rotation.0)
                    }),
            );
            let distance = Vec2::new(hero_position.0.x, hero_position.0.z)
                .distance(Vec2::new(counter.x, counter.z));
            (distance <= HERO_MARKET_INTERACTION_RANGE).then_some((
                MarketPage {
                    settlement: entity,
                    place: settlement.name.clone(),
                    place_id: *settlement_id,
                },
                distance,
            ))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1));
    let Some((page, _)) = nearby else { return };
    selected.0 = Some(page.place_id);
    *entry = SelectedPlaceEntry::Overview;
    *tab = EncyclopediaTab::Places;
    open.0 = true;
    target.0 = Some(page);
}

#[allow(clippy::too_many_arguments)]
fn sync_market_page(
    mut commands: Commands,
    mut target: ResMut<MarketPageTarget>,
    history: Res<crate::ui::history::HistoryPanelTarget>,
    feedback: Res<MarketFeedback>,
    mut encyclopedia_open: ResMut<EncyclopediaOpen>,
    mut tab: ResMut<EncyclopediaTab>,
    hosts: Query<Entity, With<EncyclopediaPageHost>>,
    asset_server: Res<AssetServer>,
    perf: Res<crate::ui::perf::UiPerf>,
    world: MarketWorld,
    mut ui: MarketPageUi,
) {
    let mut scope = perf.scope("sync_market_page");
    let Some(target_page) = target.0.clone() else {
        for (entity, ..) in ui.roots.iter_mut() {
            commands.entity(entity).despawn();
        }
        return;
    };
    let Ok(host) = hosts.single() else {
        if !encyclopedia_open.0 {
            encyclopedia_open.0 = true;
            *tab = EncyclopediaTab::Places;
        }
        return;
    };
    // An existing host with a closed window means X/backdrop dismissal is in
    // progress. Never fight that close by reopening from a stale page target;
    // `close_pages_with_encyclopedia` clears it in the same update.
    if !encyclopedia_open.0 {
        return;
    };
    let Some(model) = market_page_model(&target_page, &world, &feedback) else {
        for (entity, ..) in ui.roots.iter_mut() {
            commands.entity(entity).despawn();
        }
        target.0 = None;
        return;
    };
    let covered_by_history = history.0.is_some();
    let mut matching_root = false;
    for (entity, root, mut node) in ui.roots.iter_mut() {
        if root.settlement == model.settlement {
            matching_root = true;
            let display = if covered_by_history {
                Display::None
            } else {
                Display::Flex
            };
            if node.display != display {
                node.display = display;
            }
        } else {
            commands.entity(entity).despawn();
        }
    }
    if covered_by_history {
        return;
    }
    if !matching_root {
        scope.rebuilt();
        spawn_market_page(&mut commands, host, &asset_server, &model);
        return;
    }
    bind_market_page(&mut commands, &model, &mut ui);
}

fn bind_market_page(
    commands: &mut Commands,
    model: &MarketPageModel,
    ui: &mut MarketPageUi<'_, '_>,
) {
    for (field, mut text) in ui.texts.iter_mut() {
        let next = model.text(*field);
        if text.0 != next {
            text.0 = next.to_string();
        }
    }
    for mut color in ui.access.iter_mut() {
        let next = if model.access_enabled {
            STATUS_GOOD
        } else {
            INK_MUTED
        };
        if color.0 != next {
            color.0 = next;
        }
    }
    for (mut text, mut color) in ui.feedback.iter_mut() {
        if text.0 != model.feedback {
            text.0.clone_from(&model.feedback);
        }
        let next = match model.feedback_success {
            Some(true) => STATUS_GOOD,
            Some(false) => Color::srgb(0.62, 0.18, 0.14),
            None => INK_MUTED,
        };
        if color.0 != next {
            color.0 = next;
        }
    }
    for (entity, mut button, disabled, mut style) in ui.actions.iter_mut() {
        let row = model.row(button.good);
        let (enabled, action) = match button.kind {
            MarketButtonKind::Buy => (
                row.buy_enabled,
                HeroMarketAction::Buy {
                    maximum_unit_price: row.buy_price,
                },
            ),
            MarketButtonKind::Offer => (
                row.offer_enabled,
                HeroMarketAction::PostSellOrder {
                    unit_price: row.offer_price,
                },
            ),
        };
        button.market = model.settlement;
        button.enabled = enabled;
        button.action = action;
        style.variant = if button.kind == MarketButtonKind::Buy {
            UiButtonVariant::Primary
        } else {
            UiButtonVariant::Secondary
        };
        if enabled && disabled {
            commands.entity(entity).remove::<InteractionDisabled>();
        } else if !enabled && !disabled {
            commands.entity(entity).insert(InteractionDisabled);
        }
    }
    for (marker, mut text) in ui.action_labels.iter_mut() {
        let row = model.row(marker.good);
        let next = match marker.kind {
            MarketButtonKind::Buy => &row.buy_label,
            MarketButtonKind::Offer => &row.offer_label,
        };
        if text.0 != *next {
            text.0.clone_from(next);
        }
    }
}

fn spawn_market_page(
    commands: &mut Commands,
    host: Entity,
    asset_server: &AssetServer,
    model: &MarketPageModel,
) {
    let root = commands
        .spawn((
            MarketPageRoot {
                settlement: model.settlement,
            },
            Node {
                width: Val::Percent(100.0),
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Stretch,
                overflow: Overflow::clip(),
                ..default()
            },
            BackgroundColor(LIMEWASH_LIT),
        ))
        .id();
    commands.entity(host).add_child(root);
    commands.entity(root).with_children(|page| {
        page.spawn(Node {
            flex_shrink: 0.0,
            flex_direction: FlexDirection::Row,
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::Center,
            padding: UiRect::axes(Val::Px(22.0), Val::Px(14.0)),
            border: UiRect::bottom(Val::Px(1.0)),
            ..default()
        })
        .insert(BorderColor::all(PLATE_RULE_SOFT))
        .with_children(|header| {
            header
                .spawn(Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(3.0),
                    ..default()
                })
                .with_children(|copy| {
                    copy.spawn((
                        MarketBoundText::Title,
                        Text::new(model.title.clone()),
                        crate::ui::typography::text(T_TITLE),
                        TextColor(INK),
                    ));
                    copy.spawn((
                        MarketBoundText::Subtitle,
                        Text::new(model.subtitle.clone()),
                        crate::ui::typography::text(T_LABEL),
                        TextColor(INK_MUTED),
                    ));
                });
            header.spawn((
                MarketAccessText,
                MarketBoundText::Access,
                Text::new(model.access.clone()),
                crate::ui::typography::text(T_BODY),
                TextColor(if model.access_enabled { STATUS_GOOD } else { INK_MUTED }),
            ));
        });

        page.spawn(Node {
            flex_shrink: 0.0,
            flex_direction: FlexDirection::Row,
            align_items: AlignItems::Stretch,
            column_gap: Val::Px(9.0),
            padding: UiRect::axes(Val::Px(22.0), Val::Px(12.0)),
            border: UiRect::bottom(Val::Px(1.0)),
            ..default()
        })
        .insert(BorderColor::all(PLATE_RULE_SOFT))
        .with_children(|summary| {
            for (field, label) in [
                (MarketSummaryField::OnOffer, "ON OFFER"),
                (MarketSummaryField::CommonStore, "COMMON STORE"),
                (MarketSummaryField::Today, "TRADED TODAY"),
                (MarketSummaryField::Lifetime, "LIFETIME VOLUME"),
                (MarketSummaryField::HeroFunds, "YOUR FUNDS"),
            ] {
                spawn_summary_tile(summary, field, label, model.summary(field));
            }
        });

        page.spawn((
            MarketViewport,
            Node {
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Stretch,
                row_gap: Val::Px(9.0),
                padding: UiRect::all(Val::Px(20.0)),
                overflow: Overflow::scroll_y(),
                scrollbar_width: 8.0,
                ..default()
            },
        ))
        .with_children(|content| {
            content
                .spawn((
                    Node {
                        flex_shrink: 0.0,
                        flex_direction: FlexDirection::Row,
                        align_items: AlignItems::Center,
                        justify_content: JustifyContent::SpaceBetween,
                        column_gap: Val::Px(18.0),
                        padding: UiRect::axes(Val::Px(16.0), Val::Px(11.0)),
                        border: UiRect::all(Val::Px(1.0)),
                        border_radius: BorderRadius::all(Val::Px(RADIUS)),
                        ..default()
                    },
                    BackgroundColor(LIMEWASH_WELL),
                    BorderColor::all(PLATE_RULE_SOFT),
                ))
                .with_children(|intro| {
                    intro.spawn((
                        Text::new("HOW THIS EXCHANGE WORKS"),
                        crate::ui::typography::text(T_LABEL),
                        TextColor(INK),
                    ));
                    intro.spawn((
                        Text::new(
                            "Goods remain the seller's property until a real buyer clears the offer. Posting cargo is not an immediate sale.",
                        ),
                        crate::ui::typography::text(T_BODY),
                        TextColor(INK_MUTED),
                        TextLayout::justify(Justify::Right),
                        Node {
                            flex_grow: 1.0,
                            ..default()
                        },
                    ));
                });
            content.spawn((
                Text::new("GOODS LEDGER"),
                crate::ui::typography::text(T_HEADING),
                TextColor(INK),
                Node {
                    margin: UiRect::top(Val::Px(5.0)),
                    ..default()
                },
            ));
            for row in &model.rows {
                spawn_market_row(content, asset_server, model, row);
            }
        });

        page.spawn((
            MarketFeedbackText,
            Text::new(model.feedback.clone()),
            crate::ui::typography::text(T_BODY),
            TextColor(match model.feedback_success {
                Some(true) => STATUS_GOOD,
                Some(false) => Color::srgb(0.62, 0.18, 0.14),
                None => INK_MUTED,
            }),
            Node {
                flex_shrink: 0.0,
                padding: UiRect::axes(Val::Px(22.0), Val::Px(11.0)),
                border: UiRect::top(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(LIMEWASH),
            BorderColor::all(PLATE_RULE_SOFT),
        ));
    });
}

fn spawn_summary_tile(
    parent: &mut ChildSpawnerCommands<'_>,
    field: MarketSummaryField,
    label: &str,
    value: &str,
) {
    parent
        .spawn((
            Node {
                flex_grow: 1.0,
                flex_basis: Val::Percent(18.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(3.0),
                padding: UiRect::axes(Val::Px(12.0), Val::Px(9.0)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(RADIUS)),
                ..default()
            },
            BackgroundColor(LIMEWASH),
            BorderColor::all(PLATE_RULE_SOFT),
        ))
        .with_children(|tile| {
            tile.spawn((
                Text::new(label),
                crate::ui::typography::text(T_LABEL),
                TextColor(INK_MUTED),
            ));
            tile.spawn((
                MarketBoundText::Summary(field),
                Text::new(value),
                crate::ui::typography::text(T_VALUE),
                TextColor(INK),
            ));
        });
}

fn spawn_market_row(
    parent: &mut ChildSpawnerCommands<'_>,
    asset_server: &AssetServer,
    page: &MarketPageModel,
    row: &MarketRowModel,
) {
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                min_height: Val::Px(92.0),
                flex_shrink: 0.0,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: Val::Px(16.0),
                padding: UiRect::axes(Val::Px(14.0), Val::Px(10.0)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(RADIUS)),
                ..default()
            },
            BackgroundColor(LIMEWASH_DETAIL),
            BorderColor::all(PLATE_RULE_SOFT),
        ))
        .with_children(|record| {
            record
                .spawn(Node {
                    width: Val::Px(206.0),
                    flex_shrink: 0.0,
                    align_items: AlignItems::Center,
                    column_gap: Val::Px(12.0),
                    ..default()
                })
                .with_children(|identity| {
                    identity.spawn((
                        ImageNode::new(asset_server.load(good_icon_path(row.good))),
                        Node {
                            width: Val::Px(48.0),
                            height: Val::Px(48.0),
                            flex_shrink: 0.0,
                            ..default()
                        },
                        Pickable::IGNORE,
                    ));
                    identity
                        .spawn(Node {
                            flex_direction: FlexDirection::Column,
                            row_gap: Val::Px(3.0),
                            ..default()
                        })
                        .with_children(|copy| {
                            copy.spawn((
                                Text::new(row.good.label()),
                                crate::ui::typography::text(T_HEADING),
                                TextColor(INK),
                            ));
                            copy.spawn((
                                MarketBoundText::Row(row.good, MarketRowField::Condition),
                                Text::new(row.condition.clone()),
                                crate::ui::typography::text(T_LABEL),
                                TextColor(INK_MUTED),
                                Node {
                                    max_width: Val::Px(142.0),
                                    ..default()
                                },
                            ));
                        });
                });
            record
                .spawn(Node {
                    flex_grow: 1.0,
                    min_width: Val::Px(0.0),
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::FlexStart,
                    justify_content: JustifyContent::SpaceBetween,
                    column_gap: Val::Px(10.0),
                    ..default()
                })
                .with_children(|facts| {
                    spawn_market_fact(
                        facts,
                        MarketBoundText::Row(row.good, MarketRowField::Store),
                        "STORE / TARGET",
                        &row.store,
                    );
                    spawn_market_fact(
                        facts,
                        MarketBoundText::Row(row.good, MarketRowField::Listed),
                        "FOR SALE",
                        &row.listed,
                    );
                    spawn_market_fact(
                        facts,
                        MarketBoundText::Row(row.good, MarketRowField::LastSale),
                        "LAST SALE",
                        &row.last_sale,
                    );
                    spawn_market_fact(
                        facts,
                        MarketBoundText::Row(row.good, MarketRowField::BestOffer),
                        "BUY PRICE",
                        &row.best_offer,
                    );
                    spawn_market_fact(
                        facts,
                        MarketBoundText::Row(row.good, MarketRowField::Today),
                        "BOUGHT / UNMET",
                        &row.today,
                    );
                    spawn_market_fact(
                        facts,
                        MarketBoundText::Row(row.good, MarketRowField::HeroCargo),
                        "YOUR GOODS",
                        &row.hero_cargo,
                    );
                });
            record
                .spawn(Node {
                    width: Val::Px(330.0),
                    flex_shrink: 0.0,
                    flex_direction: FlexDirection::Row,
                    align_items: AlignItems::Center,
                    justify_content: JustifyContent::FlexEnd,
                    column_gap: Val::Px(7.0),
                    ..default()
                })
                .with_children(|actions| {
                    spawn_trade_button(actions, page.settlement, row, MarketButtonKind::Buy);
                    spawn_trade_button(actions, page.settlement, row, MarketButtonKind::Offer);
                    actions
                        .spawn((
                            crate::ui::history::MarketHistoryButton {
                                settlement: page.settlement,
                                place: page.place.clone(),
                                good: row.good,
                            },
                            Button,
                            Node {
                                width: Val::Px(82.0),
                                height: Val::Px(36.0),
                                flex_shrink: 0.0,
                                justify_content: JustifyContent::Center,
                                align_items: AlignItems::Center,
                                border: UiRect::all(Val::Px(1.0)),
                                border_radius: BorderRadius::all(Val::Px(RADIUS)),
                                ..default()
                            },
                            button_chrome(UiButtonVariant::Secondary),
                        ))
                        .with_child((
                            Text::new("HISTORY"),
                            UiButtonLabel,
                            crate::ui::typography::text(T_BUTTON),
                            TextColor(INK),
                            Pickable::IGNORE,
                        ));
                });
        });
}

fn spawn_market_fact(
    parent: &mut ChildSpawnerCommands<'_>,
    marker: MarketBoundText,
    label: &str,
    value: &str,
) {
    parent
        .spawn(Node {
            flex_grow: 1.0,
            flex_basis: Val::Px(72.0),
            min_width: Val::Px(64.0),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(3.0),
            ..default()
        })
        .with_children(|fact| {
            fact.spawn((
                Text::new(label),
                crate::ui::typography::text(T_LABEL),
                TextColor(INK_MUTED),
            ));
            fact.spawn((
                marker,
                Text::new(value),
                crate::ui::typography::text(T_VALUE),
                TextColor(INK),
            ));
        });
}

fn spawn_trade_button(
    parent: &mut ChildSpawnerCommands<'_>,
    settlement: Entity,
    row: &MarketRowModel,
    kind: MarketButtonKind,
) {
    let (enabled, action, label, variant, width) = match kind {
        MarketButtonKind::Buy => (
            row.buy_enabled,
            HeroMarketAction::Buy {
                maximum_unit_price: row.buy_price,
            },
            row.buy_label.as_str(),
            UiButtonVariant::Primary,
            112.0,
        ),
        MarketButtonKind::Offer => (
            row.offer_enabled,
            HeroMarketAction::PostSellOrder {
                unit_price: row.offer_price,
            },
            row.offer_label.as_str(),
            UiButtonVariant::Secondary,
            122.0,
        ),
    };
    let mut button = parent.spawn((
        Name::new(format!("market-{:?}-{:?}", kind, row.good)),
        MarketTradeButton {
            market: settlement,
            good: row.good,
            kind,
            action,
            enabled,
        },
        Button,
        Node {
            width: Val::Px(width),
            height: Val::Px(36.0),
            flex_shrink: 0.0,
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(RADIUS)),
            ..default()
        },
        button_chrome(variant),
    ));
    if !enabled {
        button.insert(InteractionDisabled);
    }
    button.with_child((
        MarketActionLabel {
            good: row.good,
            kind,
        },
        Text::new(label),
        UiButtonLabel,
        crate::ui::typography::text(T_BUTTON),
        TextColor(INK),
        Pickable::IGNORE,
    ));
}

fn handle_market_trade_buttons(
    mouse: Res<ButtonInput<MouseButton>>,
    mut feedback: ResMut<MarketFeedback>,
    buttons: Query<(&Interaction, &MarketTradeButton), Changed<Interaction>>,
    mut senders: Query<
        &mut MessageSender<HeroMarketOrder>,
        (With<crate::GameClient>, With<Connected>),
    >,
) {
    for (interaction, order) in buttons.iter() {
        if !order.enabled
            || *interaction != Interaction::Pressed
            || !mouse.just_pressed(MouseButton::Left)
        {
            continue;
        }
        if let Ok(mut sender) = senders.single_mut() {
            feedback.pending.push_back(order.market);
            sender.send::<ReliableChannel>(HeroMarketOrder {
                market: order.market,
                good: order.good,
                action: order.action,
                units: 1,
            });
        }
    }
}

fn receive_market_trade_results(
    mut receivers: Query<
        &mut MessageReceiver<HeroMarketResult>,
        (With<crate::GameClient>, With<Connected>),
    >,
    mut feedback: ResMut<MarketFeedback>,
) {
    for mut receiver in receivers.iter_mut() {
        for result in receiver.receive() {
            feedback.market = feedback.pending.pop_front();
            feedback.message = result.message;
            feedback.success = result.success;
        }
    }
}

fn despawn_market_page(
    mut commands: Commands,
    roots: Query<Entity, With<MarketPageRoot>>,
    mut target: ResMut<MarketPageTarget>,
    mut feedback: ResMut<MarketFeedback>,
) {
    for root in roots.iter() {
        commands.entity(root).despawn();
    }
    target.0 = None;
    *feedback = default();
}
