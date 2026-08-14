//! Compact world inspection for settlements and their buildings.
//!
//! A map click answers only the immediate questions: what is this, is it
//! occupied/working, and what is in its store? EXPAND opens the durable record
//! in the encyclopedia. Halls (and future market buildings carrying
//! [`MootMarket`]) additionally expose a dedicated, read-only trade board.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use bevy::ui::FocusPolicy;
use lightyear::prelude::{Connected, MessageReceiver, MessageSender};

use shared::components::{
    BuildingId, BuildingOf, CivicHallLevel, ConstructionSite, Household, MootAdministration,
    OwnedBy, PersonId, PlayerPosition, Settlement, SettlementBuilding, SettlementDevelopment,
    SettlementId, SettlementOpportunityBoard, SettlementPolicies,
};
use shared::economy::{
    business_working_capital, format_money, BusinessAccount, BusinessCondition, BusinessForSale,
    BusinessManagementPolicy, BusinessProcurementPolicy, BusinessSalePolicy, BusinessWagePolicy,
    Good, GoodsInventory, MootMarket, SettlementEconomy, Wallet,
};
use shared::protocol::{HeroMarketAction, HeroMarketOrder, HeroMarketResult, ReliableChannel};

use crate::selection::Selection;
use crate::states::GameState;
use crate::ui::good_icon_path;
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
        app.init_resource::<TradeFeedback>();
        app.add_systems(OnEnter(GameState::Playing), spawn_compact_panel);
        app.add_systems(OnExit(GameState::Playing), despawn_all);
        app.add_systems(
            Update,
            (
                sync_compact_panel,
                handle_compact_actions,
                handle_manage_action,
                open_nearby_market_on_interact,
                ensure_trade_panel,
                handle_market_trade_buttons,
                receive_market_trade_results,
                sync_trade_feedback,
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

#[derive(Component)]
struct InspectPropertyButton;

#[derive(Component)]
struct InspectManageButton;

#[derive(SystemParam)]
struct LocalBusinessOwnership<'w, 's> {
    owners: Query<'w, 's, &'static OwnedBy>,
    local: Option<Res<'w, crate::camera_rts::LocalPeerId>>,
    heroes: Query<'w, 's, (&'static shared::components::Hero, &'static PersonId)>,
}

impl LocalBusinessOwnership<'_, '_> {
    fn local_person(&self) -> Option<PersonId> {
        let local = self.local.as_ref()?;
        self.heroes
            .iter()
            .find(|(hero, _)| shared::player::peer_id_to_u64(hero.owner) == local.0)
            .map(|(_, person)| *person)
    }

    fn owns(&self, entity: Entity, person: Option<PersonId>) -> bool {
        person.is_some_and(|person| self.owners.get(entity).is_ok_and(|owner| owner.0 == person))
    }
}

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
            max_height: Val::Vh(82.0),
            display: Display::None,
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(Val::Px(12.0)),
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(RADIUS)),
            overflow: Overflow::clip(),
            ..default()
        },
        BackgroundColor(LIMEWASH),
        BorderColor::all(PLATE_RULE),
        plate_shadow(),
        children![(
            SettlementPanelBody,
            Node {
                width: Val::Percent(100.0),
                min_height: Val::Px(0.0),
                flex_shrink: 1.0,
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(6.0),
                overflow: Overflow::scroll_y(),
                scrollbar_width: 8.0,
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
                    Node {
                        width: Val::Px(90.0),
                        flex_shrink: 0.0,
                        ..default()
                    },
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
                    Node {
                        flex_grow: 1.0,
                        flex_shrink: 1.0,
                        ..default()
                    },
                ),
            ],
        ))
        .id()
}

fn action_row(
    commands: &mut Commands,
    trade: bool,
    property: bool,
    manage: bool,
    business_history: Option<crate::ui::history::BusinessHistoryButton>,
) -> Entity {
    let row = commands
        .spawn(Node {
            width: Val::Percent(100.0),
            justify_content: JustifyContent::FlexEnd,
            flex_wrap: FlexWrap::Wrap,
            column_gap: Val::Px(6.0),
            row_gap: Val::Px(6.0),
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
        if property {
            spawn_card_button(row, InspectPropertyButton, "PERMITS & PROPERTY");
        }
        if manage {
            spawn_card_button(row, InspectManageButton, "MANAGE");
        }
        if let Some(button) = business_history {
            spawn_card_button(row, button, "VIEW HISTORY");
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

fn opportunity_summary(board: Option<&SettlementOpportunityBoard>) -> String {
    let Some(board) = board.filter(|board| !board.opportunities.is_empty()) else {
        return "No active opportunity signals".to_string();
    };
    board
        .opportunities
        .iter()
        .take(3)
        .map(|opportunity| {
            format!(
                "{}: {} signal{}",
                opportunity.kind.label(),
                opportunity.score,
                if opportunity.subsidized {
                    " / discounted"
                } else {
                    " / full price"
                }
            )
        })
        .collect::<Vec<_>>()
        .join(" / ")
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
    settlements: Query<(
        Entity,
        &Settlement,
        Option<&CivicHallLevel>,
        Option<&SettlementEconomy>,
        Option<&MootAdministration>,
        Option<&SettlementDevelopment>,
        Option<&SettlementPolicies>,
        Option<&SettlementOpportunityBoard>,
    )>,
    buildings: Query<&SettlementBuilding>,
    ownership: LocalBusinessOwnership,
    building_ids: Query<&BuildingId>,
    building_of: Query<&BuildingOf>,
    settlement_ids: Query<(Entity, &SettlementId), With<Settlement>>,
    sites: Query<(
        &ConstructionSite,
        Option<&BusinessForSale>,
        Option<&OwnedBy>,
    )>,
    positions: Query<&PlayerPosition>,
    inventories: Query<&GoodsInventory>,
    households: Query<&Household>,
    markets: Query<&MootMarket>,
    business_economies: Query<
        (
            Option<&BuildingOf>,
            Option<&BusinessAccount>,
            Option<&BusinessSalePolicy>,
            Option<&BusinessWagePolicy>,
            Option<&BusinessManagementPolicy>,
            Option<&BusinessProcurementPolicy>,
            Option<&BusinessCondition>,
            Option<&BusinessForSale>,
        ),
        With<SettlementBuilding>,
    >,
    mut panels: Query<(&mut Node, &mut PanelSignature), With<SettlementPanel>>,
    bodies: Query<Entity, With<SettlementPanelBody>>,
) {
    let Ok((node, signature)) = panels.single() else {
        return;
    };
    let selected = selection.primary();
    let local_person = ownership.local_person();
    let fact = selected.and_then(|entity| {
        if let Ok((
            _,
            settlement,
            hall_level,
            economy,
            administration,
            development,
            policy,
            opportunities,
        )) = settlements.get(entity)
        {
            let inventory = inventories.get(entity).ok();
            let next = opportunity_summary(opportunities);
            let settlement_id = settlement_ids.get(entity).ok().map(|(_, id)| *id);
            let (private_cash, private_wage_arrears, private_tax_arrears) =
                business_economies.iter().fold(
                    (0u64, 0u64, 0u64),
                    |(cash, wages, taxes), (owner, account, ..)| {
                        if settlement_id.is_none()
                            || !owner.is_some_and(|owner| Some(owner.0) == settlement_id)
                        {
                            return (cash, wages, taxes);
                        }
                        account.map_or((cash, wages, taxes), |account| {
                            (
                                cash.saturating_add(account.cash),
                                wages.saturating_add(account.wage_arrears),
                                taxes.saturating_add(account.tax_arrears),
                            )
                        })
                    },
                );
            let purchasable_food = markets.get(entity).map_or(0, MootMarket::listed_edible_units);
            let hall_level = hall_level
                .copied()
                .unwrap_or_else(|| CivicHallLevel::for_tier(settlement.tier));
            return Some((
                format!(
                    "hall|{:?}|{:?}|{}|{}|{}|{:?}|{:?}",
                    settlement,
                    hall_level,
                    inventory_summary(inventory),
                    next,
                    markets.get(entity).is_ok(),
                    (economy, administration, development),
                    policy,
                ),
                CompactModel {
                    title: settlement.name.to_uppercase(),
                    subtitle: format!(
                        "{} / {}",
                        settlement.tier.label().to_uppercase(),
                        hall_level.label()
                    ),
                    rows: vec![
                        ("RESIDENTS".into(), settlement.residents.to_string()),
                        (
                            "TREASURY".into(),
                            format!("{} coin", format_money(settlement.treasury)),
                        ),
                        ("COMMON STORE".into(), inventory_summary(inventory)),
                        ("PERMIT MARKET".into(), next),
                        (
                            "TO ADVANCE".into(),
                            progression_summary(settlement, development),
                        ),
                        (
                            "PUBLIC JOBS".into(),
                            administration.map_or_else(
                                || "Administration starting".to_string(),
                                |office| {
                                    let (worker_target, guard_target) = policy.map_or(
                                        (
                                            settlement.tier.public_worker_positions(),
                                            settlement.tier.public_guard_positions(),
                                        ),
                                        |policy| policy.staffing_posture.targets(settlement.tier),
                                    );
                                    let stewards = if office.city_workers.is_empty() {
                                        office
                                            .road_steward
                                            .as_deref()
                                            .unwrap_or("vacant")
                                            .to_string()
                                    } else {
                                        office.city_workers.join(", ")
                                    };
                                    format!(
                                        "Reeve {} / Moot Stewards {} ({}/{}) / guards {}/{}",
                                        office.reeve.as_deref().unwrap_or("vacant"),
                                        stewards,
                                        office.city_workers.len(),
                                        worker_target,
                                        office.guards.len(),
                                        guard_target,
                                    )
                                },
                            ),
                        ),
                        (
                            "CIVIC POLICY".into(),
                            policy.map_or_else(
                                || "Awaiting charter".to_string(),
                                |policy| {
                                    format!(
                                        "{} / {:.1}% market / {:.1}% profit levy / {}",
                                        policy.strategy.label(),
                                        policy.market_fee_bps as f32 / 100.0,
                                        policy.business_profit_tax_bps as f32 / 100.0,
                                        if policy.autopilot { "auto" } else { "manual" },
                                    )
                                },
                            ),
                        ),
                        (
                            "SOCIAL / GROWTH POLICY".into(),
                            policy.map_or_else(
                                || "Awaiting charter".to_string(),
                                |policy| {
                                    format!(
                                        "Relief {} / food {}d / payroll {}d / staffing {} / permit subsidy {:.1}%",
                                        policy.poor_relief.label(),
                                        policy.food_reserve_target_days,
                                        policy.civic_payroll_reserve_days,
                                        policy.staffing_posture.label(),
                                        policy.business_permit_subsidy_bps as f32 / 100.0,
                                    )
                                },
                            ),
                        ),
                        (
                            "CIVIC WAGE ARREARS".into(),
                            administration.map_or_else(
                                || "None recorded".to_string(),
                                |office| format!("{} coin", format_money(office.wage_arrears)),
                            ),
                        ),
                        (
                            "PRIVATE CASH / ARREARS".into(),
                            format!(
                                "{} / {} wage / {} tax coin",
                                format_money(private_cash),
                                format_money(private_wage_arrears),
                                format_money(private_tax_arrears),
                            ),
                        ),
                        (
                            "PURCHASABLE FOOD".into(),
                            format!("{purchasable_food} listed units"),
                        ),
                    ],
                    trade: markets.get(entity).is_ok(),
                    property: true,
                    manage: false,
                    business_history: None,
                },
            ));
        }
        if let Ok(building) = buildings.get(entity) {
            let inventory = inventories.get(entity).ok();
            let household = households.get(entity).ok();
            let (_, account, sale_policy, wage_policy, management, procurement, condition, for_sale) =
                business_economies
                    .get(entity)
                    .unwrap_or((None, None, None, None, None, None, None, None));
            let business_history = account.and_then(|_| {
                let business = *building_ids.get(entity).ok()?;
                let settlement = building_of
                    .get(entity)
                    .ok()
                    .and_then(|owner| {
                        settlement_ids
                            .iter()
                            .find(|(_, id)| **id == owner.0)
                            .map(|(entity, _)| entity)
                    })
                    .or_else(|| {
                        settlements
                            .iter()
                            .find(|(_, settlement, ..)| settlement.name == building.settlement)
                            .map(|(entity, ..)| entity)
                    })?;
                Some(crate::ui::history::BusinessHistoryButton {
                    settlement,
                    place: building.settlement.clone(),
                    business,
                })
            });
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
            let local_market = building_of.get(entity).ok().and_then(|owner| {
                settlement_ids
                    .iter()
                    .find(|(_, id)| **id == owner.0)
                    .and_then(|(hall, _)| markets.get(hall).ok())
            });
            let capital = match (wage_policy, management, procurement) {
                (Some(wage), Some(management), Some(procurement)) => business_working_capital(
                    building.kind.positions(),
                    wage,
                    management,
                    procurement,
                    local_market,
                ),
                _ => Default::default(),
            };
            let mut rows = vec![
                (
                    "OWNER".into(),
                    for_sale.map_or_else(
                        || {
                            building
                                .owner
                                .clone()
                                .unwrap_or_else(|| "The settlement".into())
                        },
                        |listing| {
                            format!(
                                "FOR SALE — {} coin / {}",
                                format_money(listing.asking_price),
                                listing.reason.label(),
                            )
                        },
                    ),
                ),
                occupancy,
            ];
            if let Some(label) = building.kind.site_quality_label() {
                rows.push((
                    label.into(),
                    format!("{:.0}%", building.quality * 100.0),
                ));
            }
            rows.extend([
                ("STORE".into(), inventory_summary(inventory)),
                (
                    "BUSINESS".into(),
                    condition.map_or_else(
                        || "Not a business".into(),
                        |condition| condition.state.label().into(),
                    ),
                ),
                (
                    "OWNER STRATEGY".into(),
                    management.map_or_else(
                        || "None".into(),
                        |policy| {
                            format!(
                                "{} / {}",
                                policy.strategy.label(),
                                if policy.autopilot {
                                    "autopilot"
                                } else {
                                    "manual"
                                }
                            )
                        },
                    ),
                ),
                (
                    "CASH / WAGE / TAX DEBT".into(),
                    account.map_or_else(
                        || "Not a business".into(),
                        |account| {
                            format!(
                                "{} / {} / {} coin",
                                format_money(account.cash),
                                format_money(account.wage_arrears),
                                format_money(account.tax_arrears),
                            )
                        },
                    ),
                ),
                (
                    "WAGE / TAX DEFAULTS".into(),
                    account.map_or_else(
                        || "Not a business".into(),
                        |account| {
                            format!(
                                "{} / {} coin",
                                format_money(account.defaulted_wages),
                                format_money(account.defaulted_taxes),
                            )
                        },
                    ),
                ),
                (
                    "PROTECTED / DRAWABLE".into(),
                    account.map_or_else(
                        || "Not a business".into(),
                        |account| {
                            format!(
                                "{} / {} coin",
                                format_money(capital.total_with_liabilities(account)),
                                format_money(account.withdrawable_profit(capital.total())),
                            )
                        },
                    ),
                ),
                (
                    "YESTERDAY P&L".into(),
                    account.map_or_else(
                        || "Not a business".into(),
                        |account| {
                            let profit = account.previous_day.profit();
                            format!(
                                "revenue {} / costs {} / {}{} coin",
                                format_money(account.previous_day.gross_revenue),
                                format_money(account.previous_day.operating_expenses()),
                                if profit < 0 { "-" } else { "+" },
                                format_money(profit.unsigned_abs())
                            )
                        },
                    ),
                ),
                (
                    "SALE OFFER".into(),
                    sale_policy.map_or_else(
                        || "None".into(),
                        |policy| {
                            format!(
                                "{} coin each / keep {} / collect up to {}",
                                format_money(policy.asking_unit_price),
                                policy.keep_units,
                                policy.max_units_per_collection,
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
                    "INPUT ORDERS".into(),
                    procurement.map_or_else(
                        || "None".into(),
                        |policy| {
                            let orders = Good::ALL
                                .into_iter()
                                .filter_map(|good| {
                                    let rule = policy.rule(good);
                                    rule.enabled.then(|| {
                                        format!(
                                            "{} <{} -> {} @ max {}",
                                            good.label(),
                                            rule.reorder_below,
                                            rule.target_units,
                                            format_money(rule.maximum_unit_price)
                                        )
                                    })
                                })
                                .collect::<Vec<_>>();
                            if orders.is_empty() {
                                "No purchased inputs".into()
                            } else {
                                orders.join(" / ")
                            }
                        },
                    ),
                ),
            ]);
            return Some((
                format!(
                    "building|{:?}|{}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}|{:?}",
                    building,
                    inventory_summary(inventory),
                    household,
                    account,
                    sale_policy,
                    wage_policy,
                    management,
                    procurement,
                    condition,
                    for_sale,
                ),
                CompactModel {
                    title: building.kind.label().to_string(),
                    subtitle: building.settlement.to_uppercase(),
                    rows,
                    trade: markets.get(entity).is_ok(),
                    property: false,
                    manage: account.is_some() && ownership.owns(entity, local_person),
                    business_history,
                },
            ));
        }
        if let Ok((site, for_sale, site_owner)) = sites.get(entity) {
            let delivered = inventories
                .get(entity)
                .map_or(0, |inventory| inventory.amount(Good::Wood));
            let required = site.kind.construction_wood_required();
            let mut rows = vec![
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
            ];
            if let Some(listing) = for_sale {
                rows.push((
                    "TAKEOVER".into(),
                    format!(
                        "FOR SALE — {} coin / {}",
                        format_money(listing.asking_price),
                        listing.reason.label(),
                    ),
                ));
            }
            if site_owner
                .zip(local_person)
                .is_some_and(|(owner, person)| owner.0 == person)
            {
                rows.push((
                    "YOUR ORDER".into(),
                    "Select your hero, then right-click this worksite".into(),
                ));
            }
            return Some((
                format!(
                    "site|{:?}|{delivered}|{:?}|{:?}",
                    site, for_sale, site_owner
                ),
                CompactModel {
                    title: format!("{} WORKSITE", site.kind.label()),
                    subtitle: site.settlement.to_uppercase(),
                    rows,
                    trade: false,
                    property: false,
                    manage: false,
                    business_history: None,
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
    children.push(action_row(
        &mut commands,
        model.trade,
        model.property,
        model.manage,
        model.business_history,
    ));
    commands.entity(body).add_children(&children);
}

struct CompactModel {
    title: String,
    subtitle: String,
    rows: Vec<(String, String)>,
    trade: bool,
    property: bool,
    manage: bool,
    business_history: Option<crate::ui::history::BusinessHistoryButton>,
}

#[allow(clippy::too_many_arguments)]
fn handle_compact_actions(
    selection: Res<Selection>,
    settlements: Query<&Settlement>,
    buildings: Query<(&SettlementBuilding, &PlayerPosition)>,
    sites: Query<(&ConstructionSite, Option<&OwnedBy>)>,
    markets: Query<&MootMarket>,
    places: Res<crate::ui::encyclopedia::places::KnownPlaces>,
    mut encyclopedia_open: ResMut<crate::ui::encyclopedia::EncyclopediaOpen>,
    mut tab: ResMut<crate::ui::encyclopedia::EncyclopediaTab>,
    mut selected_place: ResMut<crate::ui::encyclopedia::places::SelectedPlace>,
    mut selected_entry: ResMut<crate::ui::encyclopedia::places::SelectedPlaceEntry>,
    mut trade_target: ResMut<TradePanelTarget>,
    mut property_target: ResMut<crate::ui::property_market::PropertyMarketTarget>,
    mut expand_buttons: Query<
        (&Interaction, &mut BackgroundColor),
        (
            With<InspectExpandButton>,
            Changed<Interaction>,
            Without<InspectTradeButton>,
            Without<InspectPropertyButton>,
            Without<InspectManageButton>,
        ),
    >,
    mut trade_buttons: Query<
        (&Interaction, &mut BackgroundColor),
        (
            With<InspectTradeButton>,
            Changed<Interaction>,
            Without<InspectExpandButton>,
            Without<InspectPropertyButton>,
            Without<InspectManageButton>,
        ),
    >,
    mut property_buttons: Query<
        (&Interaction, &mut BackgroundColor),
        (
            With<InspectPropertyButton>,
            Changed<Interaction>,
            Without<InspectExpandButton>,
            Without<InspectTradeButton>,
            Without<InspectManageButton>,
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
        } else if let Ok((site, _)) = sites.get(entity) {
            (
                site.settlement.clone(),
                crate::ui::encyclopedia::places::SelectedPlaceEntry::Overview,
            )
        } else {
            continue;
        };
        trade_target.0 = None;
        property_target.0 = None;
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
            property_target.0 = None;
            trade_target.0 = Some(entity);
        }
    }

    for (interaction, mut background) in property_buttons.iter_mut() {
        *background = button_background(*interaction);
        if *interaction != Interaction::Pressed {
            continue;
        }
        let Some(entity) = selection.primary() else {
            continue;
        };
        if settlements.get(entity).is_ok() {
            encyclopedia_open.0 = false;
            trade_target.0 = None;
            property_target.0 = Some(entity);
        }
    }
}

fn handle_manage_action(
    selection: Res<Selection>,
    mut target: ResMut<crate::ui::business_management::BusinessManagementTarget>,
    mut buttons: Query<
        (&Interaction, &mut BackgroundColor),
        (With<InspectManageButton>, Changed<Interaction>),
    >,
) {
    for (interaction, mut background) in buttons.iter_mut() {
        *background = button_background(*interaction);
        if *interaction == Interaction::Pressed {
            target.0 = selection.primary();
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

/// Client prediction is presentation only; the server repeats every ownership,
/// distance, capacity and cash check before moving a single item.
#[derive(Component, Clone, Copy)]
struct MarketTradeButton {
    market: Entity,
    good: Good,
    action: HeroMarketAction,
    enabled: bool,
}

#[derive(Component)]
struct TradeFeedbackText;

#[derive(Resource, Default)]
struct TradeFeedback {
    message: String,
    success: bool,
}

const HERO_MARKET_INTERACTION_RANGE: f32 = 12.0;

fn ensure_trade_panel(
    mut commands: Commands,
    target: Res<TradePanelTarget>,
    settlements: Query<(&Settlement, Option<&CivicHallLevel>, &PlayerPosition)>,
    buildings: Query<&SettlementBuilding>,
    inventories: Query<&GoodsInventory>,
    markets: Query<&MootMarket>,
    feedback: Res<TradeFeedback>,
    heroes: Query<(
        &shared::components::Hero,
        &PlayerPosition,
        Option<&GoodsInventory>,
        Option<&Wallet>,
    )>,
    local: Option<Res<crate::camera_rts::LocalPeerId>>,
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
    let (place, subtitle, treasury, market_position) =
        if let Ok((settlement, hall_level, position)) = settlements.get(entity) {
            let hall_level = hall_level
                .copied()
                .unwrap_or_else(|| CivicHallLevel::for_tier(settlement.tier));
            (
                settlement.name.clone(),
                format!(
                    "{} / {} / PUBLIC EXCHANGE",
                    settlement.tier.label().to_uppercase(),
                    hall_level.label()
                ),
                Some(settlement.treasury),
                Some(position.0),
            )
        } else if let Ok(building) = buildings.get(entity) {
            (
                building.settlement.clone(),
                format!("{} / LOCAL EXCHANGE", building.kind.label()),
                None,
                None,
            )
        } else {
            ("Local".into(), "PUBLIC EXCHANGE".into(), None, None)
        };
    let local_hero = local.as_ref().and_then(|local| {
        heroes
            .iter()
            .find(|(hero, ..)| shared::player::peer_id_to_u64(hero.owner) == local.0)
    });
    let hero_inventory = local_hero.and_then(|(_, _, inventory, _)| inventory);
    let hero_wallet = local_hero.and_then(|(_, _, _, wallet)| wallet);
    let hero_distance = local_hero
        .zip(market_position)
        .map(|((_, position, ..), market)| {
            Vec2::new(position.0.x, position.0.z).distance(Vec2::new(market.x, market.z))
        });
    let can_trade = hero_distance.is_some_and(|distance| distance <= HERO_MARKET_INTERACTION_RANGE);
    let signature = format!(
        "{entity:?}|{place}|{market:?}|{inventory:?}|{treasury:?}|{hero_inventory:?}|{hero_wallet:?}|{can_trade}|{}|{}",
        feedback.success,
        feedback.message,
    );
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
            panel_size: Vec2::new(900.0, 600.0),
            panel_padding: 0.0,
        },
    );
    commands.entity(nodes.panel).insert((
        Node {
            width: Val::Px(900.0),
            height: Val::Px(600.0),
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
                    "MARKET MODEL",
                    format!(
                        "Private consignment / {}% fee",
                        market.market_fee_bps() as f32 / 100.0
                    ),
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
                spawn_market_stat(
                    summary,
                    "YOUR HERO",
                    local_hero.map_or_else(
                        || "Not created".to_string(),
                        |(_, _, inventory, wallet)| {
                            format!(
                                "{} coin / {} bulk / {}",
                                format_money(wallet.map_or(0, |wallet| wallet.balance())),
                                inventory.map_or(0, |stock| stock.used_bulk()),
                                hero_distance.map_or_else(
                                    || "remote".to_string(),
                                    |distance| format!("{distance:.1}m away")
                                )
                            )
                        }
                    ),
                );
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
                        hero_inventory,
                        can_trade,
                    );
                }
            });

        panel.spawn((
            TradeFeedbackText,
            Text::new(if !feedback.message.is_empty() {
                feedback.message.as_str()
            } else if can_trade {
                "BUY clears real listed stock. POST OFFER consigns physical cargo at the shown ask; it pays nothing until a real buyer clears it."
            } else {
                "Select your hero and walk within 12m of the Hall, then press E or reopen this board to trade."
            }),
            TextFont {
                font_size: FontSize::Px(10.0),
                ..default()
            },
            TextColor(if feedback.message.is_empty() {
                INK_MUTED
            } else if feedback.success {
                Color::srgb(0.18, 0.42, 0.22)
            } else {
                Color::srgb(0.62, 0.18, 0.14)
            }),
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
            market_header_cell(row, "GOOD", 180.0, true);
            market_header_cell(row, "MARKET", 90.0, false);
            market_header_cell(row, "LAST", 66.0, false);
            market_header_cell(row, "ASK", 66.0, false);
            market_header_cell(row, "OFFERS", 82.0, false);
            market_header_cell(row, "HERO", 58.0, false);
            market_header_cell(row, "BUY", 68.0, false);
            market_header_cell(row, "OFFER", 68.0, false);
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
    hero_inventory: Option<&GoodsInventory>,
    can_trade: bool,
) {
    let pool = market.pool(good);
    let stock = inventory.map_or(0, |inventory| inventory.amount(good));
    let unmet = pool.day.unmet_units();
    let condition = if unmet > 0 {
        format!("{} UNMET TODAY", unmet)
    } else if stock < pool.target_stock {
        "SHORT SUPPLY".to_string()
    } else if pool.target_stock > 0 && stock > pool.target_stock.saturating_mul(2) {
        "SURPLUS".to_string()
    } else {
        "BALANCED".to_string()
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
                width: Val::Px(180.0),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                column_gap: Val::Px(12.0),
                ..default()
            })
            .with_children(|good_cell| {
                good_cell.spawn((
                    ImageNode::new(asset_server.load(good_icon_path(good))),
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
                            Text::new(condition.clone()),
                            TextFont {
                                font_size: FontSize::Px(8.0),
                                ..default()
                            },
                            TextColor(INK_MUTED),
                        ));
                    });
            });
            market_value_cell(row, format!("{stock} / {}", pool.target_stock), 90.0);
            market_value_cell(row, format_money(pool.bid), 66.0);
            market_value_cell(row, format_money(pool.ask), 66.0);
            let offers = market
                .listings()
                .iter()
                .filter(|listing| listing.good == good)
                .count();
            market_value_cell(
                row,
                format!("{} / {}u", offers, market.listed_units(good)),
                82.0,
            );
            market_value_cell(
                row,
                hero_inventory
                    .map_or(0, |stock| stock.amount(good))
                    .to_string(),
                58.0,
            );
            spawn_market_trade_button(
                row,
                "BUY 1",
                MarketTradeButton {
                    market: settlement,
                    good,
                    action: HeroMarketAction::Buy,
                    enabled: can_trade && market.listed_units(good) > 0,
                },
            );
            spawn_market_trade_button(
                row,
                "POST 1",
                MarketTradeButton {
                    market: settlement,
                    good,
                    action: HeroMarketAction::PostSellOrder {
                        unit_price: market.suggested_price(good),
                    },
                    enabled: can_trade
                        && hero_inventory.is_some_and(|stock| stock.amount(good) > 0),
                },
            );
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

fn spawn_market_trade_button(
    parent: &mut ChildSpawnerCommands<'_>,
    label: &str,
    marker: MarketTradeButton,
) {
    let enabled = marker.enabled;
    parent
        .spawn((
            marker,
            Button,
            Node {
                width: Val::Px(62.0),
                height: Val::Px(26.0),
                margin: UiRect::left(Val::Px(6.0)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(RADIUS)),
                ..default()
            },
            BackgroundColor(if enabled { BUTTON_NORMAL } else { LIMEWASH }),
            BorderColor::all(PLATE_RULE_SOFT),
        ))
        .with_child((
            Text::new(label),
            TextFont {
                font_size: FontSize::Px(8.0),
                ..default()
            },
            TextColor(if enabled { INK } else { INK_MUTED }),
            Pickable::IGNORE,
        ));
}

fn handle_market_trade_buttons(
    mouse: Res<ButtonInput<MouseButton>>,
    mut buttons: Query<
        (&Interaction, &MarketTradeButton, &mut BackgroundColor),
        Changed<Interaction>,
    >,
    mut senders: Query<
        &mut MessageSender<HeroMarketOrder>,
        (With<crate::GameClient>, With<Connected>),
    >,
) {
    for (interaction, order, mut background) in buttons.iter_mut() {
        *background = if !order.enabled {
            LIMEWASH.into()
        } else {
            button_background(*interaction)
        };
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
    mut feedback: ResMut<TradeFeedback>,
) {
    for mut receiver in receivers.iter_mut() {
        for result in receiver.receive() {
            feedback.message = result.message;
            feedback.success = result.success;
        }
    }
}

fn sync_trade_feedback(
    feedback: Res<TradeFeedback>,
    mut labels: Query<(&mut Text, &mut TextColor), With<TradeFeedbackText>>,
) {
    if !feedback.is_changed() || feedback.message.is_empty() {
        return;
    }
    for (mut text, mut colour) in labels.iter_mut() {
        text.0.clone_from(&feedback.message);
        colour.0 = if feedback.success {
            Color::srgb(0.18, 0.42, 0.22)
        } else {
            Color::srgb(0.62, 0.18, 0.14)
        };
    }
}

/// Conventional nearby-world interaction. The compact inspection button still
/// opens a remote read-only board; E opens the nearest Hall only when the
/// player's embodied hero is physically at it.
fn open_nearby_market_on_interact(
    keyboard: Res<ButtonInput<KeyCode>>,
    input: Res<crate::input::InputState>,
    local: Option<Res<crate::camera_rts::LocalPeerId>>,
    heroes: Query<(&shared::components::Hero, &PlayerPosition)>,
    halls: Query<(Entity, &PlayerPosition), With<MootMarket>>,
    mut target: ResMut<TradePanelTarget>,
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
    target.0 = halls
        .iter()
        .filter_map(|(entity, position)| {
            let distance = Vec2::new(hero_position.0.x, hero_position.0.z)
                .distance(Vec2::new(position.0.x, position.0.z));
            (distance <= HERO_MARKET_INTERACTION_RANGE).then_some((entity, distance))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(entity, _)| entity);
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
    property: Res<crate::ui::property_market::PropertyMarketTarget>,
    permits: Res<crate::ui::player_permits::PermitTrayOpen>,
    mut input: ResMut<crate::input::InputState>,
) {
    let open = target.0.is_some() || property.0.is_some() || permits.0;
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
        let row = action_row(
            &mut world.commands(),
            true,
            true,
            true,
            Some(crate::ui::history::BusinessHistoryButton {
                settlement: Entity::from_bits(1),
                place: "Brackwater".into(),
                business: BuildingId(10),
            }),
        );
        world.flush();
        assert!(world
            .get::<Children>(row)
            .is_some_and(|children| children.len() == 5));
        let mut expand = world.query_filtered::<Entity, With<InspectExpandButton>>();
        let mut trade = world.query_filtered::<Entity, With<InspectTradeButton>>();
        let mut property = world.query_filtered::<Entity, With<InspectPropertyButton>>();
        let mut manage = world.query_filtered::<Entity, With<InspectManageButton>>();
        let mut history =
            world.query_filtered::<Entity, With<crate::ui::history::BusinessHistoryButton>>();
        assert_eq!(expand.iter(&world).count(), 1);
        assert_eq!(trade.iter(&world).count(), 1);
        assert_eq!(property.iter(&world).count(), 1);
        assert_eq!(manage.iter(&world).count(), 1);
        assert_eq!(history.iter(&world).count(), 1);
    }

    #[test]
    fn compact_card_is_height_bounded_and_scrollable() {
        let mut world = World::new();
        world.run_system_once(spawn_compact_panel).unwrap();

        let mut panels = world.query_filtered::<&Node, With<SettlementPanel>>();
        let panel = panels.single(&world).unwrap();
        assert_eq!(panel.max_height, Val::Vh(82.0));
        assert_eq!(panel.overflow.y, OverflowAxis::Clip);

        let mut bodies = world.query_filtered::<&Node, With<SettlementPanelBody>>();
        let body = bodies.single(&world).unwrap();
        assert_eq!(body.min_height, Val::Px(0.0));
        assert_eq!(body.overflow.y, OverflowAxis::Scroll);
    }

    #[test]
    fn trade_board_lists_every_good() {
        assert_eq!(Good::ALL.len(), Good::COUNT);
        for good in Good::ALL {
            assert!(good_icon_path(good).ends_with(".png"));
        }
        assert_eq!(good_icon_path(Good::Flour), "ui/goods/flour.png");
        assert_eq!(good_icon_path(Good::Bread), "ui/goods/bread.png");
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
