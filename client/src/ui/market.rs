//! The settlement market as an encyclopedia Places page.
//!
//! A market is knowledge about a place, not a second top-level modal. Every
//! entry point (the compact inspector, the town record, and nearby `E`
//! interaction) sets [`MarketPageTarget`], opens Places, and renders here.
//! Live prices bind into stable widgets so a fast simulation never despawns a
//! button from under the pointer.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::ui::InteractionDisabled;
use lightyear::prelude::{Connected, MessageReceiver, MessageSender};

use shared::components::{
    BuildingOf, CivicHallLevel, PlayerPosition, PlayerRotation, Settlement, SettlementBuilding,
    SettlementBuildingKind, SettlementId,
};
use shared::economy::{Good, GoodsInventory, MootMarket, Wallet, format_money};
use shared::protocol::{HeroMarketAction, HeroMarketOrder, HeroMarketResult, ReliableChannel};

use crate::states::GameState;
use crate::ui::encyclopedia::{
    EncyclopediaOpen, EncyclopediaPageHost, EncyclopediaTab,
    places::{SelectedPlace, SelectedPlaceEntry},
};
use crate::ui::foundation::{UiButtonLabel, UiButtonStyle, UiButtonVariant, button_chrome};
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
    pub place: String,
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

#[derive(Clone, Debug, PartialEq, Eq)]
struct MarketRowModel {
    good: Good,
    condition: String,
    store: String,
    listed: String,
    last_sale: String,
    best_offer: String,
    today: String,
    hero_cargo: String,
    buy_label: String,
    offer_label: String,
    buy_enabled: bool,
    offer_enabled: bool,
    offer_price: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct MarketPageModel {
    settlement: Entity,
    place: String,
    title: String,
    subtitle: String,
    access: String,
    access_enabled: bool,
    summaries: [String; 5],
    rows: Vec<MarketRowModel>,
    feedback: String,
    feedback_success: Option<bool>,
}

impl MarketPageModel {
    fn summary(&self, field: MarketSummaryField) -> &str {
        &self.summaries[match field {
            MarketSummaryField::OnOffer => 0,
            MarketSummaryField::CommonStore => 1,
            MarketSummaryField::Today => 2,
            MarketSummaryField::Lifetime => 3,
            MarketSummaryField::HeroFunds => 4,
        }]
    }

    fn row(&self, good: Good) -> &MarketRowModel {
        &self.rows[good.index()]
    }

    fn text(&self, field: MarketBoundText) -> &str {
        match field {
            MarketBoundText::Title => &self.title,
            MarketBoundText::Subtitle => &self.subtitle,
            MarketBoundText::Access => &self.access,
            MarketBoundText::Summary(field) => self.summary(field),
            MarketBoundText::Row(good, field) => {
                let row = self.row(good);
                match field {
                    MarketRowField::Condition => &row.condition,
                    MarketRowField::Store => &row.store,
                    MarketRowField::Listed => &row.listed,
                    MarketRowField::LastSale => &row.last_sale,
                    MarketRowField::BestOffer => &row.best_offer,
                    MarketRowField::Today => &row.today,
                    MarketRowField::HeroCargo => &row.hero_cargo,
                }
            }
        }
    }
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
    settlements: Query<(Entity, &Settlement), With<MootMarket>>,
    mut buttons: Query<(Entity, &mut Node, Option<&OpenMarketButton>), With<PlaceMarketAction>>,
) {
    let target = selected.0.as_deref().and_then(|place| {
        settlements
            .iter()
            .find(|(_, settlement)| settlement.name == place)
            .map(|(entity, settlement)| MarketPage {
                settlement: entity,
                place: settlement.name.clone(),
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
                commands
                    .entity(entity)
                    .insert(OpenMarketButton(target.clone()));
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
        selected.0 = Some(button.0.place.clone());
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
                },
                distance,
            ))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1));
    let Some((page, _)) = nearby else { return };
    selected.0 = Some(page.place.clone());
    *entry = SelectedPlaceEntry::Overview;
    *tab = EncyclopediaTab::Places;
    open.0 = true;
    target.0 = Some(page);
}

fn market_page_model(
    target: &MarketPage,
    world: &MarketWorld<'_, '_>,
    feedback: &MarketFeedback,
) -> Option<MarketPageModel> {
    let market = world.markets.get(target.settlement).ok()?;
    let (settlement, settlement_id, hall_level, hall_position, hall_rotation) =
        world.settlements.get(target.settlement).ok()?;
    let inventory = world.inventories.get(target.settlement).ok();
    let hall_level = hall_level
        .copied()
        .unwrap_or_else(|| CivicHallLevel::for_tier(settlement.tier));
    let local_hero = world.local.as_ref().and_then(|local| {
        world
            .heroes
            .iter()
            .find(|(hero, ..)| shared::player::peer_id_to_u64(hero.owner) == local.0)
    });
    let hero_inventory = local_hero.and_then(|(_, _, inventory, _)| inventory);
    let hero_wallet = local_hero.and_then(|(_, _, _, wallet)| wallet);
    let counter = local_hero.map(|(_, hero_position, ..)| {
        let hall_entrance = SettlementBuildingKind::Hall.entrance_position(
            hall_position.0,
            hall_rotation.map_or(0.0, |rotation| rotation.0),
        );
        nearest_public_market_entrance(
            hero_position.0,
            hall_entrance,
            world
                .buildings
                .iter()
                .filter(|(building, owner, ..)| {
                    building.kind == SettlementBuildingKind::Market
                        && owner.is_some_and(|owner| owner.0 == *settlement_id)
                })
                .filter_map(|(building, _, position, rotation)| {
                    Some(building.kind.entrance_position(position?.0, rotation?.0))
                }),
        )
    });
    let hero_distance = local_hero.zip(counter).map(|((_, position, ..), counter)| {
        Vec2::new(position.0.x, position.0.z).distance(Vec2::new(counter.x, counter.z))
    });
    let can_trade = hero_distance.is_some_and(|distance| distance <= HERO_MARKET_INTERACTION_RANGE);
    let access = if local_hero.is_none() {
        "VIEW ONLY / CREATE A HERO TO TRADE".to_string()
    } else if can_trade {
        "AT THE COUNTER / TRADING ENABLED".to_string()
    } else {
        hero_distance.map_or_else(
            || "VIEW ONLY / MARKET LOCATION UNKNOWN".to_string(),
            |distance| format!("VIEW ONLY / {distance:.0}M FROM THE COUNTER"),
        )
    };
    let rows: Vec<_> = Good::ALL
        .into_iter()
        .map(|good| {
            market_row_model(
                good,
                inventory,
                market,
                hero_inventory,
                hero_wallet,
                can_trade,
            )
        })
        .collect();
    let listed_units = Good::ALL
        .into_iter()
        .map(|good| market.listed_units(good))
        .fold(0u32, u32::saturating_add);
    let today_coin = Good::ALL
        .into_iter()
        .map(|good| market.pool(good).day.consumer_coin)
        .fold(0u64, u64::saturating_add);
    let today_units = Good::ALL
        .into_iter()
        .map(|good| market.pool(good).day.consumer_units)
        .fold(0u64, u64::saturating_add);
    let common_store = inventory.map_or_else(
        || "No common store".to_string(),
        |stock| format!("{} / {} bulk", stock.used_bulk(), stock.bulk_capacity()),
    );
    let hero_funds = hero_wallet.map_or_else(
        || "No wallet".to_string(),
        |wallet| format!("{} coin", format_money(wallet.balance())),
    );
    let feedback_present = !feedback.message.is_empty();
    let feedback_text = if feedback_present {
        feedback.message.clone()
    } else if can_trade {
        "You are at the exchange. BUY clears the cheapest physical offer; POST OFFER consigns one carried unit at the shown price.".to_string()
    } else {
        "Market records are readable from anywhere. To transact, take your hero within 12m of this town's Hall or Marketplace counter.".to_string()
    };
    Some(MarketPageModel {
        settlement: target.settlement,
        place: settlement.name.clone(),
        title: format!("{} MARKET", settlement.name.to_uppercase()),
        subtitle: format!(
            "{} / {} / {} / {:.2}% FEE",
            settlement.tier.label().to_uppercase(),
            hall_level.label(),
            market.trade_tier().label().to_uppercase(),
            market.market_fee_bps() as f32 / 100.0,
        ),
        access,
        access_enabled: can_trade,
        summaries: [
            format!("{listed_units} units"),
            common_store,
            format!("{} coin / {today_units} units", format_money(today_coin)),
            format!("{} coin", format_money(market.total_volume())),
            hero_funds,
        ],
        rows,
        feedback: feedback_text,
        feedback_success: feedback_present.then_some(feedback.success),
    })
}

fn market_row_model(
    good: Good,
    inventory: Option<&GoodsInventory>,
    market: &MootMarket,
    hero_inventory: Option<&GoodsInventory>,
    hero_wallet: Option<&Wallet>,
    can_trade: bool,
) -> MarketRowModel {
    let pool = market.pool(good);
    let stock = inventory.map_or(0, |inventory| inventory.amount(good));
    let listed = market.listed_units(good);
    let unlocked = market.can_trade(good);
    let unmet = pool.day.unmet_units();
    let condition = if !unlocked {
        format!(
            "UNLOCKS AT {}",
            good.minimum_market_tier().label().to_uppercase()
        )
    } else if unmet > 0 {
        format!("{unmet} REQUESTED UNITS UNFILLED TODAY")
    } else if listed == 0 {
        "NO LIVE OFFERS".to_string()
    } else if stock < pool.target_stock {
        "SHORT SUPPLY".to_string()
    } else if pool.target_stock > 0 && stock > pool.target_stock.saturating_mul(2) {
        "SURPLUS".to_string()
    } else {
        "BALANCED".to_string()
    };
    let hero_units = hero_inventory.map_or(0, |stock| stock.amount(good));
    let hero_balance = hero_wallet.map_or(0, |wallet| wallet.balance());
    let buy_enabled = can_trade && unlocked && listed > 0 && hero_balance >= pool.ask;
    let offer_enabled = can_trade && unlocked && hero_units > 0;
    let offer_price = market.suggested_price(good);
    let buy_label = if !unlocked {
        "BUY / LOCKED".to_string()
    } else if !can_trade {
        "BUY / VISIT".to_string()
    } else if listed == 0 {
        "BUY / NO OFFER".to_string()
    } else if hero_balance < pool.ask {
        "BUY / NEED COIN".to_string()
    } else {
        format!("BUY 1 / {}", format_money(pool.ask))
    };
    let offer_label = if !unlocked {
        "POST / LOCKED".to_string()
    } else if !can_trade {
        "POST / VISIT".to_string()
    } else if hero_units == 0 {
        "POST / EMPTY".to_string()
    } else {
        format!("POST 1 / {}", format_money(offer_price))
    };
    MarketRowModel {
        good,
        condition,
        store: format!("{stock} / {}", pool.target_stock),
        listed: format!("{listed}"),
        last_sale: if pool.bid == 0 {
            "No sale".to_string()
        } else {
            format!("{} coin", format_money(pool.bid))
        },
        best_offer: if listed == 0 {
            "None".to_string()
        } else {
            format!("{} coin", format_money(pool.ask))
        },
        today: format!("{} / {unmet}", pool.day.consumer_units),
        hero_cargo: format!("{hero_units}"),
        buy_label,
        offer_label,
        buy_enabled,
        offer_enabled,
        offer_price,
    }
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
            MarketButtonKind::Buy => (row.buy_enabled, HeroMarketAction::Buy),
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
                        TextFont {
                            font_size: FontSize::Px(T_TITLE),
                            ..default()
                        },
                        TextColor(INK),
                    ));
                    copy.spawn((
                        MarketBoundText::Subtitle,
                        Text::new(model.subtitle.clone()),
                        TextFont {
                            font_size: FontSize::Px(T_LABEL),
                            ..default()
                        },
                        TextColor(INK_MUTED),
                    ));
                });
            header.spawn((
                MarketAccessText,
                MarketBoundText::Access,
                Text::new(model.access.clone()),
                TextFont {
                    font_size: FontSize::Px(T_BODY),
                    ..default()
                },
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
                        TextFont {
                            font_size: FontSize::Px(T_LABEL),
                            ..default()
                        },
                        TextColor(INK),
                    ));
                    intro.spawn((
                        Text::new(
                            "Goods remain the seller's property until a real buyer clears the offer. Posting cargo is not an immediate sale.",
                        ),
                        TextFont {
                            font_size: FontSize::Px(T_BODY),
                            ..default()
                        },
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
                TextFont {
                    font_size: FontSize::Px(T_HEADING),
                    ..default()
                },
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
            TextFont {
                font_size: FontSize::Px(T_BODY),
                ..default()
            },
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
                TextFont {
                    font_size: FontSize::Px(T_LABEL),
                    ..default()
                },
                TextColor(INK_MUTED),
            ));
            tile.spawn((
                MarketBoundText::Summary(field),
                Text::new(value),
                TextFont {
                    font_size: FontSize::Px(T_VALUE),
                    ..default()
                },
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
                                TextFont {
                                    font_size: FontSize::Px(T_HEADING),
                                    ..default()
                                },
                                TextColor(INK),
                            ));
                            copy.spawn((
                                MarketBoundText::Row(row.good, MarketRowField::Condition),
                                Text::new(row.condition.clone()),
                                TextFont {
                                    font_size: FontSize::Px(T_LABEL),
                                    ..default()
                                },
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
                        "BEST OFFER",
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
                        "YOUR CARGO",
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
                            TextFont {
                                font_size: FontSize::Px(T_BUTTON),
                                ..default()
                            },
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
                TextFont {
                    font_size: FontSize::Px(T_LABEL),
                    ..default()
                },
                TextColor(INK_MUTED),
            ));
            fact.spawn((
                marker,
                Text::new(value),
                TextFont {
                    font_size: FontSize::Px(T_VALUE),
                    ..default()
                },
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
            HeroMarketAction::Buy,
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
        TextFont {
            font_size: FontSize::Px(T_BUTTON),
            ..default()
        },
        TextColor(INK),
        Pickable::IGNORE,
    ));
}

fn handle_market_trade_buttons(
    mouse: Res<ButtonInput<MouseButton>>,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_good_has_one_stable_row_slot() {
        let market = MootMarket::founding();
        let rows: Vec<_> = Good::ALL
            .into_iter()
            .map(|good| market_row_model(good, None, &market, None, None, false))
            .collect();
        assert_eq!(rows.len(), Good::COUNT);
        for good in Good::ALL {
            assert_eq!(rows[good.index()].good, good);
        }
    }

    #[test]
    fn remote_market_is_readable_but_not_actionable() {
        let market = MootMarket::founding();
        let row = market_row_model(
            Good::Wood,
            None,
            &market,
            None,
            Some(&Wallet::new(10_000)),
            false,
        );
        assert!(!row.buy_enabled);
        assert!(!row.offer_enabled);
        assert_eq!(row.buy_label, "BUY / VISIT");
        assert_eq!(row.offer_label, "POST / VISIT");
    }
}
