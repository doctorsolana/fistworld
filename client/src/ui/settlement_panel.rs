//! Compact world inspection for settlements and their buildings.
//!
//! A map click answers only the immediate questions: what is this, is it
//! occupied/working, and what is in its store? EXPAND opens the durable record
//! in the encyclopedia. Halls (and future market buildings carrying
//! [`MootMarket`]) additionally expose a dedicated, read-only trade board.

use bevy::prelude::*;
use bevy::ui::FocusPolicy;

use shared::components::{
    ConstructionSite, Household, MootAdministration, PlayerPosition, Settlement,
    SettlementBuilding, SettlementBuildingKind, SettlementDevelopment,
};
use shared::economy::{
    format_money, next_settlement_building, BusinessAccount, BusinessSalePolicy,
    BusinessWagePolicy, Good, GoodsInventory, MootMarket, SettlementEconomy,
};

use crate::selection::Selection;
use crate::states::GameState;
use crate::ui::modal::{
    handle_backdrop_pressed, spawn_modal, update_modal_click_guard, ModalLayout,
};
use crate::ui::styles::{
    plate_shadow, BUTTON_HOVERED, BUTTON_NORMAL, BUTTON_PRESSED, INK, INK_MUTED, LIMEWASH,
    LIMEWASH_LIT, PLATE_RULE, PLATE_RULE_SOFT, RADIUS,
};

pub struct SettlementPanelPlugin;

impl Plugin for SettlementPanelPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<TradePanelTarget>();
        app.init_resource::<TradeClickGuard>();
        app.add_systems(OnEnter(GameState::Playing), spawn_compact_panel);
        app.add_systems(OnExit(GameState::Playing), despawn_all);
        app.add_systems(
            Update,
            (
                sync_compact_panel,
                handle_compact_actions,
                ensure_trade_panel,
                update_trade_guard,
                handle_trade_close,
                sync_trade_input_state,
            )
                .chain()
                .run_if(in_state(GameState::Playing)),
        );
    }
}

// --- compact selection card ------------------------------------------------

#[derive(Component)]
struct SettlementPanel;

#[derive(Component)]
struct SettlementPanelBody;

#[derive(Component, Default)]
struct PanelSignature(String);

#[derive(Component)]
struct InspectExpandButton;

#[derive(Component)]
struct InspectTradeButton;

fn spawn_compact_panel(mut commands: Commands) {
    commands.spawn((
        SettlementPanel,
        PanelSignature::default(),
        Interaction::default(),
        FocusPolicy::Block,
        Pickable::default(),
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(12.0),
            bottom: Val::Px(12.0),
            width: Val::Px(286.0),
            display: Display::None,
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(Val::Px(12.0)),
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(RADIUS)),
            ..default()
        },
        BackgroundColor(LIMEWASH),
        BorderColor::all(PLATE_RULE),
        plate_shadow(),
        children![(
            SettlementPanelBody,
            Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(6.0),
                ..default()
            },
        )],
    ));
}

fn title(commands: &mut Commands, name: String, subtitle: String) -> Entity {
    commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(1.0),
                padding: UiRect::bottom(Val::Px(5.0)),
                border: UiRect::bottom(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(PLATE_RULE_SOFT),
            children![
                (
                    Text::new(name),
                    TextFont {
                        font_size: FontSize::Px(18.0),
                        ..default()
                    },
                    TextColor(INK),
                ),
                (
                    Text::new(subtitle),
                    TextFont {
                        font_size: FontSize::Px(9.0),
                        ..default()
                    },
                    TextColor(INK_MUTED),
                ),
            ],
        ))
        .id()
}

fn line(commands: &mut Commands, label: impl Into<String>, value: impl Into<String>) -> Entity {
    commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                column_gap: Val::Px(12.0),
                ..default()
            },
            children![
                (
                    Text::new(label.into()),
                    TextFont {
                        font_size: FontSize::Px(10.0),
                        ..default()
                    },
                    TextColor(INK_MUTED),
                ),
                (
                    Text::new(value.into()),
                    TextFont {
                        font_size: FontSize::Px(12.0),
                        ..default()
                    },
                    TextColor(INK),
                    TextLayout::justify(Justify::Right),
                ),
            ],
        ))
        .id()
}

fn action_row(commands: &mut Commands, trade: bool) -> Entity {
    let row = commands
        .spawn(Node {
            width: Val::Percent(100.0),
            justify_content: JustifyContent::FlexEnd,
            column_gap: Val::Px(6.0),
            margin: UiRect::top(Val::Px(5.0)),
            padding: UiRect::top(Val::Px(8.0)),
            border: UiRect::top(Val::Px(1.0)),
            ..default()
        })
        .id();
    commands.entity(row).with_children(|row| {
        if trade {
            spawn_card_button(row, InspectTradeButton, "TRADE");
        }
        spawn_card_button(row, InspectExpandButton, "EXPAND");
    });
    row
}

fn spawn_card_button<M: Component>(parent: &mut ChildSpawnerCommands<'_>, marker: M, label: &str) {
    parent
        .spawn((
            marker,
            Button,
            Node {
                height: Val::Px(29.0),
                padding: UiRect::axes(Val::Px(12.0), Val::Px(6.0)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(RADIUS)),
                ..default()
            },
            BackgroundColor(BUTTON_NORMAL),
            BorderColor::all(PLATE_RULE_SOFT),
        ))
        .with_child((
            Text::new(label),
            TextFont {
                font_size: FontSize::Px(10.0),
                ..default()
            },
            TextColor(INK),
            Pickable::IGNORE,
        ));
}

fn inventory_summary(inventory: Option<&GoodsInventory>) -> String {
    let Some(inventory) = inventory else {
        return "No storage".to_string();
    };
    let goods = Good::ALL
        .into_iter()
        .filter_map(|good| {
            let amount = inventory.amount(good);
            (amount > 0).then_some(format!("{} {amount}", good.label()))
        })
        .collect::<Vec<_>>();
    if goods.is_empty() {
        format!("Empty / {} bulk", inventory.bulk_capacity())
    } else {
        goods.join(" / ")
    }
}

fn selected_next_need(
    settlement: &Settlement,
    economy: Option<&SettlementEconomy>,
    buildings: &Query<&SettlementBuilding>,
    sites: &Query<&ConstructionSite>,
) -> String {
    let count = |kind| {
        buildings
            .iter()
            .filter(|building| building.settlement == settlement.name && building.kind == kind)
            .count()
            + sites
                .iter()
                .filter(|site| site.settlement == settlement.name && site.kind == kind)
                .count()
    };
    next_settlement_building(
        count(SettlementBuildingKind::Farmstead),
        count(SettlementBuildingKind::FishermansHut),
        count(SettlementBuildingKind::LumberjackHut),
        count(SettlementBuildingKind::House),
        settlement.residents,
        economy,
    )
    .map(|kind| kind.label().to_string())
    .unwrap_or_else(|| "No shortage".to_string())
}

fn progression_summary(
    settlement: &Settlement,
    development: Option<&SettlementDevelopment>,
) -> String {
    let Some(development) = development else {
        return settlement
            .tier
            .next_requirement()
            .unwrap_or("Highest tier reached")
            .to_string();
    };
    if development.required_days == 0 {
        return development.next_gate.label().to_string();
    }
    format!(
        "{} / {} of {} days",
        development.next_gate.label(),
        development.progress_days,
        development.required_days
    )
}

#[allow(clippy::too_many_arguments)]
fn sync_compact_panel(
    mut commands: Commands,
    selection: Res<Selection>,
    settlements: Query<&Settlement>,
    buildings: Query<&SettlementBuilding>,
    sites: Query<&ConstructionSite>,
    positions: Query<&PlayerPosition>,
    inventories: Query<&GoodsInventory>,
    households: Query<&Household>,
    markets: Query<&MootMarket>,
    economies: Query<&SettlementEconomy>,
    administrations: Query<&MootAdministration>,
    business_economies: Query<
        (
            Option<&BusinessAccount>,
            Option<&BusinessSalePolicy>,
            Option<&BusinessWagePolicy>,
        ),
        With<SettlementBuilding>,
    >,
    developments: Query<&SettlementDevelopment>,
    mut panels: Query<(&mut Node, &mut PanelSignature), With<SettlementPanel>>,
    bodies: Query<Entity, With<SettlementPanelBody>>,
) {
    let Ok((node, signature)) = panels.single() else {
        return;
    };
    let selected = selection.primary();
    let fact = selected.and_then(|entity| {
        if let Ok(settlement) = settlements.get(entity) {
            let inventory = inventories.get(entity).ok();
            let economy = economies.get(entity).ok();
            let administration = administrations.get(entity).ok();
            let development = developments.get(entity).ok();
            let next = selected_next_need(settlement, economy, &buildings, &sites);
            return Some((
                format!(
                    "hall|{:?}|{}|{}|{}|{:?}",
                    settlement,
                    inventory_summary(inventory),
                    next,
                    markets.get(entity).is_ok(),
                    (economy, administration, development)
                ),
                CompactModel {
                    title: settlement.name.to_uppercase(),
                    subtitle: format!("{} / MOOT HALL", settlement.tier.label().to_uppercase()),
                    rows: vec![
                        ("RESIDENTS".into(), settlement.residents.to_string()),
                        (
                            "TREASURY".into(),
                            format!("{} coin", format_money(settlement.treasury)),
                        ),
                        ("COMMON STORE".into(), inventory_summary(inventory)),
                        ("NEXT PERMIT".into(), next),
                        (
                            "TO ADVANCE".into(),
                            progression_summary(settlement, development),
                        ),
                        (
                            "PUBLIC JOBS".into(),
                            administration.map_or_else(
                                || "Administration starting".to_string(),
                                |office| {
                                    format!(
                                        "Reeve {} / porter {} / steward {} / guards {}/{}",
                                        office.reeve.as_deref().unwrap_or("vacant"),
                                        office.market_porter.as_deref().unwrap_or("vacant"),
                                        office.road_steward.as_deref().unwrap_or("vacant"),
                                        office.guards.len(),
                                        settlement.tier.public_guard_positions(),
                                    )
                                },
                            ),
                        ),
                    ],
                    trade: markets.get(entity).is_ok(),
                },
            ));
        }
        if let Ok(building) = buildings.get(entity) {
            let inventory = inventories.get(entity).ok();
            let household = households.get(entity).ok();
            let (account, sale_policy, wage_policy) =
                business_economies.get(entity).unwrap_or((None, None, None));
            let occupancy = if building.kind.housing_capacity() > 0 {
                (
                    "BEDS".to_string(),
                    format!(
                        "{} / {}",
                        household.map_or(0, |home| home.residents.len()),
                        building.kind.housing_capacity()
                    ),
                )
            } else {
                (
                    "STAFF".to_string(),
                    format!("{} / {}", building.workers.len(), building.kind.positions()),
                )
            };
            return Some((
                format!(
                    "building|{:?}|{}|{:?}|{:?}|{:?}|{:?}",
                    building,
                    inventory_summary(inventory),
                    household,
                    account,
                    sale_policy,
                    wage_policy,
                ),
                CompactModel {
                    title: building.kind.label().to_string(),
                    subtitle: building.settlement.to_uppercase(),
                    rows: vec![
                        (
                            "OWNER".into(),
                            building
                                .owner
                                .clone()
                                .unwrap_or_else(|| "The settlement".into()),
                        ),
                        occupancy,
                        (
                            "PLOT".into(),
                            format!("{:.0}% quality", building.quality * 100.0),
                        ),
                        ("STORE".into(), inventory_summary(inventory)),
                        (
                            "BUSINESS CASH".into(),
                            account.map_or_else(
                                || "Not a business".into(),
                                |account| format!("{} coin", format_money(account.cash)),
                            ),
                        ),
                        (
                            "SALE OFFER".into(),
                            sale_policy.map_or_else(
                                || "None".into(),
                                |policy| {
                                    format!(
                                        "keep {} / collect up to {} / min {} coin",
                                        policy.keep_units,
                                        policy.max_units_per_collection,
                                        format_money(policy.minimum_unit_price),
                                    )
                                },
                            ),
                        ),
                        (
                            "DAILY WAGE".into(),
                            wage_policy.map_or_else(
                                || "Not a business".into(),
                                |policy| {
                                    format!(
                                        "{} coin / {}{}",
                                        format_money(policy.daily_wage),
                                        if policy.automatic {
                                            "owner auto"
                                        } else {
                                            "owner set"
                                        },
                                        if policy.vacancy_days > 0 {
                                            format!(" / vacant {}d", policy.vacancy_days)
                                        } else {
                                            String::new()
                                        }
                                    )
                                },
                            ),
                        ),
                        (
                            "WAGE ARREARS".into(),
                            account.map_or_else(
                                || "None".into(),
                                |account| format!("{} coin", format_money(account.wage_arrears)),
                            ),
                        ),
                    ],
                    trade: markets.get(entity).is_ok(),
                },
            ));
        }
        if let Ok(site) = sites.get(entity) {
            let delivered = inventories
                .get(entity)
                .map_or(0, |inventory| inventory.amount(Good::Wood));
            let required = site.kind.construction_wood_required();
            return Some((
                format!("site|{:?}|{delivered}", site),
                CompactModel {
                    title: format!("{} WORKSITE", site.kind.label()),
                    subtitle: site.settlement.to_uppercase(),
                    rows: vec![
                        (
                            "STATUS".into(),
                            if site.raising {
                                "Raising frame"
                            } else if delivered >= required {
                                "Ready to build"
                            } else {
                                "Awaiting materials"
                            }
                            .into(),
                        ),
                        ("WOOD".into(), format!("{delivered} / {required}")),
                    ],
                    trade: false,
                },
            ));
        }
        let _ = positions.get(entity).ok()?;
        None
    });

    let Some((next_signature, model)) = fact else {
        if node.display != Display::None {
            panels.single_mut().unwrap().0.display = Display::None;
        }
        return;
    };
    if signature.0 == next_signature && node.display == Display::Flex {
        return;
    }

    let (mut node, mut signature) = panels.single_mut().unwrap();
    node.display = Display::Flex;
    signature.0 = next_signature;
    let Ok(body) = bodies.single() else {
        return;
    };
    commands.entity(body).despawn_related::<Children>();
    let mut children = vec![title(&mut commands, model.title, model.subtitle)];
    for (label, value) in model.rows {
        children.push(line(&mut commands, label, value));
    }
    children.push(action_row(&mut commands, model.trade));
    commands.entity(body).add_children(&children);
}

struct CompactModel {
    title: String,
    subtitle: String,
    rows: Vec<(String, String)>,
    trade: bool,
}

#[allow(clippy::too_many_arguments)]
fn handle_compact_actions(
    selection: Res<Selection>,
    settlements: Query<&Settlement>,
    buildings: Query<(&SettlementBuilding, &PlayerPosition)>,
    sites: Query<&ConstructionSite>,
    markets: Query<&MootMarket>,
    places: Res<crate::ui::encyclopedia::places::KnownPlaces>,
    mut encyclopedia_open: ResMut<crate::ui::encyclopedia::EncyclopediaOpen>,
    mut tab: ResMut<crate::ui::encyclopedia::EncyclopediaTab>,
    mut selected_place: ResMut<crate::ui::encyclopedia::places::SelectedPlace>,
    mut selected_entry: ResMut<crate::ui::encyclopedia::places::SelectedPlaceEntry>,
    mut trade_target: ResMut<TradePanelTarget>,
    mut expand_buttons: Query<
        (&Interaction, &mut BackgroundColor),
        (
            With<InspectExpandButton>,
            Changed<Interaction>,
            Without<InspectTradeButton>,
        ),
    >,
    mut trade_buttons: Query<
        (&Interaction, &mut BackgroundColor),
        (
            With<InspectTradeButton>,
            Changed<Interaction>,
            Without<InspectExpandButton>,
        ),
    >,
) {
    for (interaction, mut background) in expand_buttons.iter_mut() {
        *background = button_background(*interaction);
        if *interaction != Interaction::Pressed {
            continue;
        }
        let Some(entity) = selection.primary() else {
            continue;
        };
        let (place_name, entry) = if let Ok(settlement) = settlements.get(entity) {
            (
                settlement.name.clone(),
                crate::ui::encyclopedia::places::SelectedPlaceEntry::Hall,
            )
        } else if let Ok((building, position)) = buildings.get(entity) {
            let entry = places
                .find(&building.settlement)
                .and_then(|place| {
                    place
                        .buildings
                        .iter()
                        .position(|record| {
                            record.kind == building.kind
                                && record.position.distance_squared(position.0) < 0.01
                        })
                        .map(crate::ui::encyclopedia::places::SelectedPlaceEntry::Building)
                })
                .unwrap_or_default();
            (building.settlement.clone(), entry)
        } else if let Ok(site) = sites.get(entity) {
            (
                site.settlement.clone(),
                crate::ui::encyclopedia::places::SelectedPlaceEntry::Overview,
            )
        } else {
            continue;
        };
        trade_target.0 = None;
        selected_place.0 = Some(place_name);
        *selected_entry = entry;
        *tab = crate::ui::encyclopedia::EncyclopediaTab::Places;
        encyclopedia_open.0 = true;
    }

    for (interaction, mut background) in trade_buttons.iter_mut() {
        *background = button_background(*interaction);
        if *interaction != Interaction::Pressed {
            continue;
        }
        let Some(entity) = selection.primary() else {
            continue;
        };
        if markets.get(entity).is_ok() {
            encyclopedia_open.0 = false;
            trade_target.0 = Some(entity);
        }
    }
}

fn button_background(interaction: Interaction) -> BackgroundColor {
    match interaction {
        Interaction::Pressed => BUTTON_PRESSED.into(),
        Interaction::Hovered => BUTTON_HOVERED.into(),
        Interaction::None => BUTTON_NORMAL.into(),
    }
}

// --- market board ----------------------------------------------------------

#[derive(Resource, Default)]
pub(crate) struct TradePanelTarget(pub Option<Entity>);

#[derive(Resource, Default)]
struct TradeClickGuard(bool);

#[derive(Component)]
struct TradePanelRoot {
    signature: String,
}

#[derive(Component)]
struct TradeBackdrop;

#[derive(Component)]
struct TradePanel;

#[derive(Component)]
struct TradeCloseButton;

fn good_icon(good: Good) -> &'static str {
    match good {
        Good::Food => "ui/goods/fish.png",
        Good::Wheat => "ui/goods/wheat.png",
        Good::Wood => "ui/goods/wood.png",
        Good::Stone => "ui/goods/stone.png",
        Good::Iron => "ui/goods/iron.png",
    }
}

fn ensure_trade_panel(
    mut commands: Commands,
    target: Res<TradePanelTarget>,
    settlements: Query<&Settlement>,
    buildings: Query<&SettlementBuilding>,
    inventories: Query<&GoodsInventory>,
    markets: Query<&MootMarket>,
    roots: Query<(Entity, &TradePanelRoot)>,
    asset_server: Res<AssetServer>,
) {
    let Some(entity) = target.0 else {
        for (root, _) in roots.iter() {
            commands.entity(root).despawn();
        }
        return;
    };
    let Ok(market) = markets.get(entity) else {
        for (root, _) in roots.iter() {
            commands.entity(root).despawn();
        }
        return;
    };
    let inventory = inventories.get(entity).ok();
    let (place, subtitle, treasury) = if let Ok(settlement) = settlements.get(entity) {
        (
            settlement.name.clone(),
            format!(
                "{} MOOT HALL / PUBLIC EXCHANGE",
                settlement.tier.label().to_uppercase()
            ),
            Some(settlement.treasury),
        )
    } else if let Ok(building) = buildings.get(entity) {
        (
            building.settlement.clone(),
            format!("{} / LOCAL EXCHANGE", building.kind.label()),
            None,
        )
    } else {
        ("Local".into(), "PUBLIC EXCHANGE".into(), None)
    };
    let signature = format!("{entity:?}|{place}|{market:?}|{inventory:?}|{treasury:?}");
    if roots.iter().any(|(_, root)| root.signature == signature) {
        return;
    }
    for (root, _) in roots.iter() {
        commands.entity(root).despawn();
    }

    let nodes = spawn_modal(
        &mut commands,
        TradePanelRoot {
            signature: signature.clone(),
        },
        TradeBackdrop,
        TradePanel,
        ModalLayout {
            panel_size: Vec2::new(790.0, 560.0),
            panel_padding: 0.0,
        },
    );
    commands.entity(nodes.panel).insert((
        Node {
            width: Val::Px(790.0),
            height: Val::Px(560.0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(RADIUS)),
            overflow: Overflow::clip(),
            ..default()
        },
        BackgroundColor(LIMEWASH_LIT),
        BorderColor::all(PLATE_RULE),
        plate_shadow(),
    ));
    commands.entity(nodes.panel).with_children(|panel| {
        panel
            .spawn((
                Node {
                    flex_direction: FlexDirection::Row,
                    justify_content: JustifyContent::SpaceBetween,
                    align_items: AlignItems::Center,
                    padding: UiRect::axes(Val::Px(22.0), Val::Px(15.0)),
                    border: UiRect::bottom(Val::Px(1.0)),
                    ..default()
                },
                BackgroundColor(LIMEWASH),
                BorderColor::all(PLATE_RULE_SOFT),
            ))
            .with_children(|header| {
                header
                    .spawn(Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(2.0),
                        ..default()
                    })
                    .with_children(|copy| {
                        copy.spawn((
                            Text::new(format!("{} MARKET", place.to_uppercase())),
                            TextFont {
                                font_size: FontSize::Px(21.0),
                                ..default()
                            },
                            TextColor(INK),
                        ));
                        copy.spawn((
                            Text::new(subtitle),
                            TextFont {
                                font_size: FontSize::Px(9.0),
                                ..default()
                            },
                            TextColor(INK_MUTED),
                        ));
                    });
                header
                    .spawn((
                        TradeCloseButton,
                        Button,
                        Node {
                            width: Val::Px(30.0),
                            height: Val::Px(30.0),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            border: UiRect::all(Val::Px(1.0)),
                            border_radius: BorderRadius::all(Val::Px(RADIUS)),
                            ..default()
                        },
                        BackgroundColor(BUTTON_NORMAL),
                        BorderColor::all(PLATE_RULE_SOFT),
                    ))
                    .with_child((
                        Text::new("X"),
                        TextFont {
                            font_size: FontSize::Px(11.0),
                            ..default()
                        },
                        TextColor(INK),
                        Pickable::IGNORE,
                    ));
            });

        panel
            .spawn(Node {
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(28.0),
                padding: UiRect::axes(Val::Px(22.0), Val::Px(13.0)),
                border: UiRect::bottom(Val::Px(1.0)),
                ..default()
            })
            .with_children(|summary| {
                spawn_market_stat(
                    summary,
                    "BUYING LIQUIDITY",
                    format!("{} coin", format_money(market.total_liquidity())),
                );
                spawn_market_stat(
                    summary,
                    "LIFETIME VOLUME",
                    format!("{} coin", format_money(market.total_volume())),
                );
                spawn_market_stat(
                    summary,
                    "COMMON STORAGE",
                    inventory.map_or_else(
                        || "No store".to_string(),
                        |stock| format!("{} / {} bulk", stock.used_bulk(), stock.bulk_capacity()),
                    ),
                );
                if let Some(treasury) = treasury {
                    spawn_market_stat(
                        summary,
                        "CIVIC TREASURY",
                        format!("{} coin", format_money(treasury)),
                    );
                }
            });

        panel
            .spawn(Node {
                flex_grow: 1.0,
                min_height: Val::Px(0.0),
                flex_direction: FlexDirection::Column,
                padding: UiRect::axes(Val::Px(22.0), Val::Px(12.0)),
                overflow: Overflow::scroll_y(),
                scrollbar_width: 8.0,
                ..default()
            })
            .with_children(|table| {
                spawn_market_header(table);
                for good in Good::ALL {
                    spawn_market_row(
                        table,
                        &asset_server,
                        entity,
                        &place,
                        good,
                        inventory,
                        market,
                    );
                }
            });

        panel.spawn((
            Text::new(
                "BID is what the market pays a seller / ASK is what a buyer pays / prices react to real stock",
            ),
            TextFont {
                font_size: FontSize::Px(10.0),
                ..default()
            },
            TextColor(INK_MUTED),
            Node {
                padding: UiRect::axes(Val::Px(22.0), Val::Px(12.0)),
                border: UiRect::top(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(PLATE_RULE_SOFT),
        ));
    });
}

fn spawn_market_stat(parent: &mut ChildSpawnerCommands<'_>, label: &str, value: String) {
    parent
        .spawn(Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(2.0),
            ..default()
        })
        .with_children(|stat| {
            stat.spawn((
                Text::new(label),
                TextFont {
                    font_size: FontSize::Px(8.0),
                    ..default()
                },
                TextColor(INK_MUTED),
            ));
            stat.spawn((
                Text::new(value),
                TextFont {
                    font_size: FontSize::Px(13.0),
                    ..default()
                },
                TextColor(INK),
            ));
        });
}

fn spawn_market_header(parent: &mut ChildSpawnerCommands<'_>) {
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                height: Val::Px(28.0),
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(Val::Px(10.0)),
                border: UiRect::bottom(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(PLATE_RULE),
        ))
        .with_children(|row| {
            market_header_cell(row, "GOOD", 210.0, true);
            market_header_cell(row, "STOCK / TARGET", 105.0, false);
            market_header_cell(row, "BID", 75.0, false);
            market_header_cell(row, "ASK", 75.0, false);
            market_header_cell(row, "POOL FUNDS", 105.0, false);
            market_header_cell(row, "", 82.0, false);
        });
}

fn market_header_cell(parent: &mut ChildSpawnerCommands<'_>, text: &str, width: f32, left: bool) {
    parent.spawn((
        Text::new(text),
        TextFont {
            font_size: FontSize::Px(8.0),
            ..default()
        },
        TextColor(INK_MUTED),
        TextLayout::justify(if left { Justify::Left } else { Justify::Right }),
        Node {
            width: Val::Px(width),
            flex_shrink: 0.0,
            ..default()
        },
    ));
}

fn spawn_market_row(
    parent: &mut ChildSpawnerCommands<'_>,
    asset_server: &AssetServer,
    settlement: Entity,
    place: &str,
    good: Good,
    inventory: Option<&GoodsInventory>,
    market: &MootMarket,
) {
    let pool = market.pool(good);
    let stock = inventory.map_or(0, |inventory| inventory.amount(good));
    let condition = if stock < pool.target_stock {
        "SHORT SUPPLY"
    } else if pool.target_stock > 0 && stock > pool.target_stock.saturating_mul(2) {
        "SURPLUS"
    } else {
        "BALANCED"
    };
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                min_height: Val::Px(58.0),
                align_items: AlignItems::Center,
                padding: UiRect::horizontal(Val::Px(10.0)),
                border: UiRect::bottom(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(PLATE_RULE_SOFT),
        ))
        .with_children(|row| {
            row.spawn(Node {
                width: Val::Px(210.0),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                column_gap: Val::Px(12.0),
                ..default()
            })
            .with_children(|good_cell| {
                good_cell.spawn((
                    ImageNode::new(asset_server.load(good_icon(good))),
                    Node {
                        width: Val::Px(38.0),
                        height: Val::Px(38.0),
                        flex_shrink: 0.0,
                        ..default()
                    },
                    Pickable::IGNORE,
                ));
                good_cell
                    .spawn(Node {
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(1.0),
                        ..default()
                    })
                    .with_children(|copy| {
                        copy.spawn((
                            Text::new(good.label()),
                            TextFont {
                                font_size: FontSize::Px(13.0),
                                ..default()
                            },
                            TextColor(INK),
                        ));
                        copy.spawn((
                            Text::new(condition),
                            TextFont {
                                font_size: FontSize::Px(8.0),
                                ..default()
                            },
                            TextColor(INK_MUTED),
                        ));
                    });
            });
            market_value_cell(row, format!("{stock} / {}", pool.target_stock), 105.0);
            market_value_cell(row, format!("{}", format_money(pool.bid)), 75.0);
            market_value_cell(row, format!("{}", format_money(pool.ask)), 75.0);
            market_value_cell(row, format!("{}", format_money(pool.cash)), 105.0);
            row.spawn((
                crate::ui::history::MarketHistoryButton {
                    settlement,
                    place: place.to_string(),
                    good,
                },
                Button,
                Node {
                    width: Val::Px(76.0),
                    height: Val::Px(26.0),
                    margin: UiRect::left(Val::Px(6.0)),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(RADIUS)),
                    ..default()
                },
                BackgroundColor(BUTTON_NORMAL),
                BorderColor::all(PLATE_RULE_SOFT),
            ))
            .with_child((
                Text::new("HISTORY"),
                TextFont {
                    font_size: FontSize::Px(8.0),
                    ..default()
                },
                TextColor(INK),
                Pickable::IGNORE,
            ));
        });
}

fn market_value_cell(parent: &mut ChildSpawnerCommands<'_>, text: String, width: f32) {
    parent.spawn((
        Text::new(text),
        TextFont {
            font_size: FontSize::Px(12.0),
            ..default()
        },
        TextColor(INK),
        TextLayout::justify(Justify::Right),
        Node {
            width: Val::Px(width),
            flex_shrink: 0.0,
            ..default()
        },
    ));
}

fn update_trade_guard(
    target: Res<TradePanelTarget>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut guard: ResMut<TradeClickGuard>,
) {
    update_modal_click_guard(target.0.is_some(), &mouse, &mut guard.0);
}

fn handle_trade_close(
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    guard: Res<TradeClickGuard>,
    backdrop: Query<&Interaction, (With<TradeBackdrop>, Changed<Interaction>)>,
    close: Query<&Interaction, (With<TradeCloseButton>, Changed<Interaction>)>,
    mut target: ResMut<TradePanelTarget>,
) {
    let clicked = guard.0 && mouse.just_pressed(MouseButton::Left);
    let clicked_out = clicked && handle_backdrop_pressed(&backdrop);
    let clicked_close = clicked
        && close
            .iter()
            .any(|interaction| *interaction == Interaction::Pressed);
    if keyboard.just_pressed(KeyCode::Escape) || clicked_out || clicked_close {
        target.0 = None;
    }
}

fn sync_trade_input_state(
    target: Res<TradePanelTarget>,
    mut input: ResMut<crate::input::InputState>,
) {
    let open = target.0.is_some();
    if input.inventory_open != open {
        input.inventory_open = open;
    }
}

fn despawn_all(
    mut commands: Commands,
    panels: Query<Entity, With<SettlementPanel>>,
    trade: Query<Entity, With<TradePanelRoot>>,
    mut target: ResMut<TradePanelTarget>,
) {
    for entity in panels.iter().chain(trade.iter()) {
        commands.entity(entity).despawn();
    }
    target.0 = None;
}

#[cfg(test)]
mod tests {
    use super::*;
    use bevy::ecs::system::RunSystemOnce;

    #[test]
    fn compact_card_has_expand_and_optional_trade_actions() {
        let mut world = World::new();
        let row = action_row(&mut world.commands(), true);
        world.flush();
        assert!(world
            .get::<Children>(row)
            .is_some_and(|children| children.len() == 2));
        let mut expand = world.query_filtered::<Entity, With<InspectExpandButton>>();
        let mut trade = world.query_filtered::<Entity, With<InspectTradeButton>>();
        assert_eq!(expand.iter(&world).count(), 1);
        assert_eq!(trade.iter(&world).count(), 1);
    }

    #[test]
    fn trade_board_lists_every_good() {
        assert_eq!(Good::ALL.len(), 5);
        for good in Good::ALL {
            assert!(good_icon(good).ends_with(".png"));
        }
    }

    #[test]
    fn backdrop_is_the_only_blank_area_that_closes_trade() {
        let mut world = World::new();
        world.insert_resource(ButtonInput::<KeyCode>::default());
        let mut mouse = ButtonInput::<MouseButton>::default();
        mouse.press(MouseButton::Left);
        world.insert_resource(mouse);
        world.insert_resource(TradeClickGuard(true));
        world.insert_resource(TradePanelTarget(Some(Entity::from_bits(1))));
        world.spawn((TradeBackdrop, Interaction::None));
        world.spawn((TradePanel, Interaction::Pressed));
        world.run_system_once(handle_trade_close).unwrap();
        assert!(world.resource::<TradePanelTarget>().0.is_some());
    }
}
