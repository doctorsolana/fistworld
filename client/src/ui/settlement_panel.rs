//! Compact world inspection for settlements and their buildings.
//!
//! A map click answers only the immediate questions: what is this, is it
//! occupied/working, and what is in its store? EXPAND opens the durable record
//! in the encyclopedia. Halls (and future market buildings carrying
//! [`MootMarket`]) additionally expose a dedicated, read-only trade board.

use bevy::ecs::system::SystemParam;
use bevy::input_focus::tab_navigation::TabGroup;
use bevy::prelude::*;
use bevy::ui::{FocusPolicy, InteractionDisabled};
use lightyear::prelude::{Connected, MessageReceiver, MessageSender};

use shared::components::{
    BuildingId, BuildingOf, CivicHallLevel, CivicHallUpgradeWorksite, CivicTradeContract,
    CompanyId, CompanyLeadership, ConstructionSite, Household, MootAdministration, OperatedBy,
    OwnedBy, PersonId, PlayerPosition, PlayerRotation, Settlement, SettlementBuilding,
    SettlementBuildingKind, SettlementDevelopment, SettlementId, SettlementOpportunityBoard,
    SettlementPolicies, TradeContractId,
};
use shared::economy::{
    format_money, BusinessAccount, BusinessCondition, BusinessForSale,
    BusinessManagementPolicy, BusinessProcurementPolicy, BusinessSalePolicy,
    BusinessStaffingPolicy, BusinessWagePolicy, CompanyAccount, Good, GoodsInventory, MootMarket,
    SettlementEconomy, Wallet,
};
use shared::protocol::{HeroMarketAction, HeroMarketOrder, HeroMarketResult, ReliableChannel};

use crate::selection::Selection;
use crate::states::GameState;
use crate::ui::foundation::{
    button_chrome, layer, retained_scroll, subtree_is_interacting, UiButtonLabel, UiButtonVariant,
    UiRefreshStamp,
};
use crate::ui::good_icon_path;
use crate::ui::modal::{
    handle_backdrop_pressed, spawn_modal, update_modal_click_guard, ModalLayout,
};
use crate::ui::styles::{
    plate_shadow, INK, INK_MUTED, LIMEWASH, LIMEWASH_LIT, LIMEWASH_WELL, PLATE_RULE,
    PLATE_RULE_SOFT, RADIUS,
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
                (
                    sync_compact_panel,
                    handle_compact_actions,
                    handle_manage_action,
                    open_nearby_market_on_interact,
                    ensure_trade_panel,
                    handle_market_trade_buttons,
                )
                    .chain(),
                (
                    receive_market_trade_results,
                    sync_trade_feedback,
                    update_trade_guard,
                    handle_trade_close,
                    sync_permit_tray_input_state,
                )
                    .chain(),
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

/// A text slot of the compact card whose value is bound in place.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
enum CompactBound {
    Title,
    Subtitle,
    Tile(usize),
    Row(usize),
}

#[derive(Component)]
struct InspectExpandButton;

#[derive(Component)]
struct InspectTradeButton;

#[derive(Component)]
struct InspectPropertyButton;

#[derive(Component)]
struct InspectManageButton;

#[derive(SystemParam)]
struct CompactPanelUi<'w, 's> {
    perf: Res<'w, crate::ui::perf::UiPerf>,
    panels: Query<
        'w,
        's,
        (Entity, &'static mut Node, &'static mut PanelSignature),
        With<SettlementPanel>,
    >,
    bodies: Query<'w, 's, Entity, With<SettlementPanelBody>>,
    texts: Query<'w, 's, (&'static CompactBound, &'static mut Text)>,
    children: Query<'w, 's, &'static Children>,
    interactions: Query<
        'w,
        's,
        (
            &'static Interaction,
            Has<crate::ui::foundation::UiRefreshExempt>,
        ),
    >,
}

#[derive(SystemParam)]
struct LocalBusinessOwnership<'w, 's> {
    owners: Query<'w, 's, &'static OwnedBy>,
    operations: Query<'w, 's, &'static OperatedBy>,
    companies: Query<
        'w,
        's,
        (
            &'static CompanyId,
            &'static CompanyLeadership,
            &'static CompanyAccount,
        ),
    >,
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

    fn can_open(&self, entity: Entity, person: Option<PersonId>) -> bool {
        if let Ok(operation) = self.operations.get(entity) {
            // Company books and public share offers are inspectable by every
            // player. The server still reserves operating controls for the
            // appointed Company Master.
            return self.companies.iter().any(|(id, ..)| *id == operation.0);
        }
        person.is_some_and(|person| self.owners.get(entity).is_ok_and(|owner| owner.0 == person))
    }
}

fn spawn_compact_panel(mut commands: Commands) {
    commands.spawn((
        SettlementPanel,
        TabGroup::new(0),
        PanelSignature::default(),
        Interaction::default(),
        FocusPolicy::Block,
        Pickable::default(),
        GlobalZIndex(layer::FLOATING_PANEL),
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(12.0),
            bottom: Val::Px(12.0),
            width: Val::Px(344.0),
            max_height: Val::Vh(82.0),
            display: Display::None,
            flex_direction: FlexDirection::Column,
            padding: UiRect::all(Val::Px(14.0)),
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
                row_gap: Val::Px(8.0),
                overflow: Overflow::scroll_y(),
                scrollbar_width: 8.0,
                ..default()
            },
        )],
    ));
}

fn spawn_header(commands: &mut Commands, model: &CompactModel) -> Entity {
    commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(2.0),
                padding: UiRect::bottom(Val::Px(8.0)),
                border: UiRect::bottom(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(PLATE_RULE_SOFT),
            children![
                (
                    CompactBound::Title,
                    Text::new(model.title.clone()),
                    TextFont {
                        font_size: FontSize::Px(20.0),
                        ..default()
                    },
                    TextColor(INK),
                ),
                (
                    CompactBound::Subtitle,
                    Text::new(model.subtitle.clone()),
                    TextFont {
                        font_size: FontSize::Px(12.0),
                        ..default()
                    },
                    TextColor(INK_MUTED),
                ),
            ],
        ))
        .id()
}

/// Key figures as a two-column grid of tiles.
fn spawn_tiles(commands: &mut Commands, tiles: &[(String, String)]) -> Entity {
    let grid = commands
        .spawn(Node {
            width: Val::Percent(100.0),
            flex_wrap: FlexWrap::Wrap,
            column_gap: Val::Px(6.0),
            row_gap: Val::Px(6.0),
            ..default()
        })
        .id();
    commands.entity(grid).with_children(|grid| {
        for (index, (label, value)) in tiles.iter().enumerate() {
            grid.spawn((
                Node {
                    flex_basis: Val::Percent(47.0),
                    flex_grow: 1.0,
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(2.0),
                    padding: UiRect::axes(Val::Px(10.0), Val::Px(8.0)),
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(RADIUS)),
                    ..default()
                },
                BackgroundColor(LIMEWASH_WELL),
                BorderColor::all(PLATE_RULE_SOFT),
                children![
                    (
                        Text::new(label.clone()),
                        TextFont {
                            font_size: FontSize::Px(11.0),
                            ..default()
                        },
                        TextColor(INK_MUTED),
                    ),
                    (
                        CompactBound::Tile(index),
                        Text::new(value.clone()),
                        TextFont {
                            font_size: FontSize::Px(16.0),
                            ..default()
                        },
                        TextColor(INK),
                    ),
                ],
            ));
        }
    });
    grid
}

fn spawn_line(commands: &mut Commands, index: usize, label: &str, value: &str) -> Entity {
    commands
        .spawn((
            Node {
                width: Val::Percent(100.0),
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::FlexStart,
                column_gap: Val::Px(12.0),
                ..default()
            },
            children![
                (
                    Text::new(label.to_string()),
                    TextFont {
                        font_size: FontSize::Px(12.0),
                        ..default()
                    },
                    TextColor(INK_MUTED),
                    Node {
                        flex_shrink: 0.0,
                        ..default()
                    },
                ),
                (
                    CompactBound::Row(index),
                    Text::new(value.to_string()),
                    TextFont {
                        font_size: FontSize::Px(14.0),
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
            spawn_card_button(row, InspectManageButton, "VIEW COMPANY");
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
                height: Val::Px(32.0),
                padding: UiRect::axes(Val::Px(12.0), Val::Px(6.0)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(RADIUS)),
                ..default()
            },
            button_chrome(UiButtonVariant::Secondary),
        ))
        .with_child((
            Text::new(label),
            UiButtonLabel,
            TextFont {
                font_size: FontSize::Px(13.0),
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
            (amount > 0).then_some(if inventory.partition_bulk_capacity().is_some() {
                format!(
                    "{} {amount}/{}",
                    good.label(),
                    inventory.bulk_capacity_for(good) / good.bulk_per_unit()
                )
            } else {
                format!("{} {amount}", good.label())
            })
        })
        .collect::<Vec<_>>();
    if goods.is_empty() {
        inventory.partition_bulk_capacity().map_or_else(
            || format!("Empty / {} bulk", inventory.bulk_capacity()),
            |capacity| format!("Empty / {capacity} bulk per resource"),
        )
    } else {
        let contents = goods.join(" / ");
        inventory
            .partition_bulk_capacity()
            .map_or(contents.clone(), |capacity| {
                format!("{contents} / {capacity} bulk per resource")
            })
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
    if matches!(
        development.next_gate,
        shared::components::SettlementProgressGate::CivicHallMaterials
            | shared::components::SettlementProgressGate::CivicHallConstruction
    ) {
        return format!(
            "{} — {} / {} units",
            development.next_gate.label(),
            development.progress_days,
            development.required_days
        );
    }
    format!(
        "{} / {} of {} days",
        development.next_gate.label(),
        development.progress_days,
        development.required_days
    )
}

#[allow(clippy::too_many_arguments, clippy::too_many_lines)]
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
        Option<&CivicHallUpgradeWorksite>,
    )>,
    positions: Query<&PlayerPosition>,
    inventories: Query<&GoodsInventory>,
    households: Query<&Household>,
    markets: Query<&MootMarket>,
    trade_contracts: Query<(&TradeContractId, &CivicTradeContract)>,
    business_economies: Query<
        (
            Option<&BuildingOf>,
            Option<&BusinessAccount>,
            Option<&BusinessSalePolicy>,
            Option<&BusinessWagePolicy>,
            Option<&BusinessStaffingPolicy>,
            Option<&BusinessManagementPolicy>,
            Option<&BusinessProcurementPolicy>,
            Option<&BusinessCondition>,
            Option<&BusinessForSale>,
            Option<&OperatedBy>,
        ),
        With<SettlementBuilding>,
    >,
    mut ui: CompactPanelUi,
) {
    let mut _ui_scope = ui.perf.scope("sync_compact_panel");
    let Ok((panel_entity, node, signature)) = ui.panels.single() else {
        return;
    };
    let selected = selection.primary();
    let local_person = ownership.local_person();
    let company_accounts: std::collections::HashMap<CompanyId, CompanyAccount> = ownership
        .companies
        .iter()
        .map(|(id, _, account)| (*id, *account))
        .collect();
    let model = selected.and_then(|entity| {
        if let Ok((_, settlement, hall_level, economy, _administration, development, _policy, opportunities)) =
            settlements.get(entity)
        {
            let inventory = inventories.get(entity).ok();
            let settlement_id = settlement_ids.get(entity).ok().map(|(_, id)| *id);
            let hall_level = hall_level
                .copied()
                .unwrap_or_else(|| CivicHallLevel::for_tier(settlement.tier));
            let residents = settlement.residents;
            let percent = |count: u32| {
                if residents == 0 {
                    0.0
                } else {
                    count as f32 / residents as f32 * 100.0
                }
            };
            let waiting = || "Pending".to_string();
            let tiles = vec![
                ("RESIDENTS".to_string(), residents.to_string()),
                (
                    "UNREST".to_string(),
                    economy.map_or_else(waiting, |economy| {
                        format!("{:.0} {}", economy.unrest, economy.unrest_label())
                    }),
                ),
                (
                    "FOOD".to_string(),
                    economy.map_or_else(waiting, |economy| {
                        format!(
                            "{} / {:.1} days",
                            food_security_state(economy),
                            economy.reserve_days
                        )
                    }),
                ),
                (
                    "UNEMPLOYMENT".to_string(),
                    economy.map_or_else(waiting, |economy| {
                        format!(
                            "{} seeking ({:.0}%)",
                            economy.job_seekers,
                            percent(u32::from(economy.job_seekers))
                        )
                    }),
                ),
                (
                    "HOMELESS".to_string(),
                    economy.map_or_else(waiting, |economy| {
                        format!(
                            "{} ({:.0}%)",
                            economy.homeless_residents,
                            percent(economy.homeless_residents)
                        )
                    }),
                ),
                (
                    "TREASURY".to_string(),
                    format!("{} coin", format_money(settlement.treasury)),
                ),
            ];
            let mut rows = vec![
                ("COMMON STORE".to_string(), inventory_summary(inventory)),
                ("PERMIT MARKET".to_string(), opportunity_summary(opportunities)),
                (
                    "TO ADVANCE".to_string(),
                    progression_summary(settlement, development),
                ),
            ];
            if let Some(economy) = economy.filter(|economy| economy.unmet_food > 0) {
                rows.push((
                    "UNFED".to_string(),
                    format!("{} of {residents} residents", economy.unmet_food),
                ));
            }
            if let Some(economy) = economy.filter(|economy| economy.unpaid_workers > 0) {
                rows.push((
                    "UNPAID WORKERS".to_string(),
                    economy.unpaid_workers.to_string(),
                ));
            }
            if let Some(settlement_id) = settlement_id {
                let mut imports: Vec<_> = trade_contracts
                    .iter()
                    .filter(|(_, contract)| {
                        contract.destination == settlement_id && contract.status.is_active()
                    })
                    .map(|(id, contract)| {
                        let status = if contract.origin.is_none()
                            && contract.status == shared::components::TradeContractStatus::Open
                        {
                            "awaiting supply"
                        } else {
                            contract.status.label()
                        };
                        format!(
                            "#{} {} {}/{} ({status})",
                            id.0,
                            contract.good.label(),
                            contract.delivered_units,
                            contract.requested_units,
                        )
                    })
                    .collect();
                imports.sort();
                if !imports.is_empty() {
                    rows.push(("IMPORTS".to_string(), imports.join(" / ")));
                }
            }
            return Some(CompactModel {
                kind: "hall",
                title: settlement.name.to_uppercase(),
                subtitle: format!(
                    "{} / {}",
                    settlement.tier.label().to_uppercase(),
                    hall_level.label().to_uppercase()
                ),
                tiles,
                rows,
                trade: markets.get(entity).is_ok(),
                property: true,
                manage: false,
                business_history: None,
            });
        }
        if let Ok(building) = buildings.get(entity) {
            let public_hall = (building.kind == SettlementBuildingKind::Market)
                .then(|| {
                    building_of.get(entity).ok().and_then(|owner| {
                        settlement_ids
                            .iter()
                            .find(|(_, id)| **id == owner.0)
                            .map(|(hall, _)| hall)
                    })
                })
                .flatten();
            let inventory = inventories.get(public_hall.unwrap_or(entity)).ok();
            let household = households.get(entity).ok();
            let (
                _,
                account,
                sale_policy,
                wage_policy,
                staffing,
                management,
                procurement,
                condition,
                for_sale,
                operated_by,
            ) = business_economies
                .get(entity)
                .unwrap_or((None, None, None, None, None, None, None, None, None, None));
            let company_account = operated_by
                .and_then(|operation| company_accounts.get(&operation.0))
                .copied();
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
            let mut tiles = Vec::new();
            if building.kind.housing_capacity() > 0 {
                tiles.push((
                    "BEDS".to_string(),
                    format!(
                        "{} / {}",
                        household.map_or(0, |home| home.residents.len()),
                        building.kind.housing_capacity()
                    ),
                ));
            } else {
                let target = staffing
                    .copied()
                    .unwrap_or_else(|| BusinessStaffingPolicy::new(building.kind.positions()))
                    .target_for(building.kind);
                tiles.push((
                    "STAFF".to_string(),
                    format!("{} of {} posts", building.workers.len(), target),
                ));
            }
            if let Some(condition) = condition {
                tiles.push(("BUSINESS".to_string(), condition.state.label().to_string()));
            }
            if let Some(label) = building.kind.site_quality_label() {
                tiles.push((
                    label.to_uppercase(),
                    format!("{:.0}%", building.quality * 100.0),
                ));
            }
            if let Some(wage) = wage_policy {
                tiles.push((
                    "DAILY WAGE".to_string(),
                    format!("{} coin", format_money(wage.daily_wage)),
                ));
            }
            if let Some(account) = account {
                let profit = account.previous_day.profit();
                tiles.push((
                    "YESTERDAY".to_string(),
                    format!(
                        "{}{} coin",
                        if profit < 0 { "-" } else { "+" },
                        format_money(profit.unsigned_abs())
                    ),
                ));
                tiles.push((
                    "COMPANY CASH".to_string(),
                    format!(
                        "{} coin",
                        format_money(company_account.map_or(0, |company| company.cash))
                    ),
                ));
            }
            let mut rows = vec![(
                "OWNER".to_string(),
                for_sale.map_or_else(
                    || {
                        building
                            .owner
                            .clone()
                            .unwrap_or_else(|| "The settlement".into())
                    },
                    |listing| {
                        format!(
                            "FOR SALE / {} coin / {}",
                            format_money(listing.asking_price),
                            listing.reason.label(),
                        )
                    },
                ),
            )];
            rows.push(("STORE".to_string(), inventory_summary(inventory)));
            if let Some(policy) = management {
                rows.push((
                    "STRATEGY".to_string(),
                    format!(
                        "{} / {}",
                        policy.strategy.label(),
                        if policy.autopilot { "autopilot" } else { "manual" }
                    ),
                ));
            }
            if let Some(account) = account.filter(|account| {
                account.wage_arrears > 0 || account.tax_arrears > 0
            }) {
                rows.push((
                    "DEBT".to_string(),
                    format!(
                        "{} wage / {} tax coin",
                        format_money(account.wage_arrears),
                        format_money(account.tax_arrears),
                    ),
                ));
            }
            if let Some(policy) = sale_policy {
                rows.push((
                    "SALE OFFER".to_string(),
                    format!(
                        "{} coin each / {} day reserve",
                        format_money(policy.asking_unit_price),
                        policy.company_reserve_days,
                    ),
                ));
            }
            if let Some(policy) = procurement {
                let orders = Good::ALL
                    .into_iter()
                    .filter_map(|good| {
                        let rule = policy.rule(good);
                        rule.enabled.then(|| {
                            format!(
                                "{} {} target @ max {}",
                                good.label(),
                                rule.target_units,
                                format_money(rule.maximum_unit_price)
                            )
                        })
                    })
                    .collect::<Vec<_>>();
                if !orders.is_empty() {
                    rows.push(("INPUT ORDERS".to_string(), orders.join(" / ")));
                }
            }
            return Some(CompactModel {
                kind: "building",
                title: building.kind.label().to_uppercase(),
                subtitle: building.settlement.to_uppercase(),
                tiles,
                rows,
                trade: public_hall.is_some() || markets.get(entity).is_ok(),
                property: false,
                manage: account.is_some() && ownership.can_open(entity, local_person),
                business_history,
            });
        }
        if let Ok((site, for_sale, site_owner, hall_upgrade)) = sites.get(entity) {
            let (good, required) = hall_upgrade.map_or(
                (Good::Wood, site.kind.construction_wood_required()),
                |upgrade| (upgrade.material, upgrade.material_required),
            );
            let delivered = inventories
                .get(entity)
                .map_or(0, |inventory| inventory.amount(good));
            let tiles = vec![
                (
                    "STATUS".to_string(),
                    if site.raising {
                        "Raising frame"
                    } else if delivered >= required {
                        "Ready to build"
                    } else {
                        "Awaiting materials"
                    }
                    .to_string(),
                ),
                (good.label().to_uppercase(), format!("{delivered} / {required}")),
            ];
            let mut rows = Vec::new();
            if let Some(settlement_id) = building_of.get(entity).ok().map(|owner| owner.0) {
                if let Some((id, contract)) = trade_contracts.iter().find(|(_, contract)| {
                    contract.destination == settlement_id
                        && contract.good == good
                        && contract.status.is_active()
                }) {
                    rows.push((
                        "INBOUND CONTRACT".to_string(),
                        format!(
                            "#{} / {} / {}/{} delivered",
                            id.0,
                            contract.status.label(),
                            contract.delivered_units,
                            contract.requested_units,
                        ),
                    ));
                }
            }
            if let Some(listing) = for_sale {
                rows.push((
                    "TAKEOVER".to_string(),
                    format!(
                        "FOR SALE / {} coin / {}",
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
                    "YOUR ORDER".to_string(),
                    "Select your hero, then right-click this worksite".to_string(),
                ));
            }
            return Some(CompactModel {
                kind: "site",
                title: format!("{} WORKSITE", site.kind.label().to_uppercase()),
                subtitle: site.settlement.to_uppercase(),
                tiles,
                rows,
                trade: false,
                property: false,
                manage: false,
                business_history: None,
            });
        }
        let _ = positions.get(entity).ok()?;
        None
    });
    let Some(model) = model else {
        if node.display != Display::None {
            ui.panels.single_mut().unwrap().1.display = Display::None;
        }
        return;
    };
    let structure = model.structure_key(selected);
    if signature.0 == structure && node.display == Display::Flex {
        // Same structure: bind the values into the existing slots.
        for (bound, mut text) in ui.texts.iter_mut() {
            let value = match bound {
                CompactBound::Title => &model.title,
                CompactBound::Subtitle => &model.subtitle,
                CompactBound::Tile(index) => &model.tiles[*index].1,
                CompactBound::Row(index) => &model.rows[*index].1,
            };
            if text.0 != *value {
                text.0 = value.clone();
            }
        }
        return;
    }
    if subtree_is_interacting(panel_entity, &ui.children, &ui.interactions) {
        return;
    }
    let (_, mut node, mut signature) = ui.panels.single_mut().unwrap();
    node.display = Display::Flex;
    signature.0 = structure;
    let Ok(body) = ui.bodies.single() else {
        return;
    };
    _ui_scope.rebuilt();
    commands.entity(body).despawn_related::<Children>();
    let header = spawn_header(&mut commands, &model);
    let tiles = spawn_tiles(&mut commands, &model.tiles);
    let mut children = vec![header, tiles];
    for (index, (label, value)) in model.rows.iter().enumerate() {
        children.push(spawn_line(&mut commands, index, label, value));
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

fn food_security_state(economy: &SettlementEconomy) -> &'static str {
    if economy.unmet_food > 0 {
        "CRISIS"
    } else if economy.reserve_days < 1.0 {
        "SHORT"
    } else if economy.reserve_days < shared::economy::FOOD_SECURITY_TARGET_DAYS {
        "FRAGILE"
    } else {
        "SECURE"
    }
}

/// The compact card as data: a few key tiles, a few vital rows, and the
/// actions that lead to the full record. Values bind in place; only the
/// [`CompactModel::structure_key`] decides a respawn.
struct CompactModel {
    kind: &'static str,
    title: String,
    subtitle: String,
    tiles: Vec<(String, String)>,
    rows: Vec<(String, String)>,
    trade: bool,
    property: bool,
    manage: bool,
    business_history: Option<crate::ui::history::BusinessHistoryButton>,
}

impl CompactModel {
    fn structure_key(&self, target: Option<Entity>) -> String {
        let mut key = format!("{}|{target:?}|", self.kind);
        for (label, _) in &self.tiles {
            key.push_str(label);
            key.push(',');
        }
        key.push('|');
        for (label, _) in &self.rows {
            key.push_str(label);
            key.push(',');
        }
        key.push_str(&format!(
            "|{}{}{}|{:?}",
            self.trade,
            self.property,
            self.manage,
            self.business_history.as_ref().map(|button| button.business)
        ));
        key
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_compact_actions(
    selection: Res<Selection>,
    settlements: Query<(Entity, &Settlement, &SettlementId)>,
    buildings: Query<(&SettlementBuilding, &PlayerPosition, Option<&BuildingOf>)>,
    sites: Query<(&ConstructionSite, Option<&OwnedBy>)>,
    markets: Query<&MootMarket>,
    places: Res<crate::ui::encyclopedia::places::KnownPlaces>,
    mut encyclopedia_open: ResMut<crate::ui::encyclopedia::EncyclopediaOpen>,
    mut tab: ResMut<crate::ui::encyclopedia::EncyclopediaTab>,
    mut selected_place: ResMut<crate::ui::encyclopedia::places::SelectedPlace>,
    mut selected_entry: ResMut<crate::ui::encyclopedia::places::SelectedPlaceEntry>,
    mut trade_target: ResMut<TradePanelTarget>,
    mut property_target: ResMut<crate::ui::property_market::PropertyMarketTarget>,
    expand_buttons: Query<
        &Interaction,
        (
            With<InspectExpandButton>,
            Changed<Interaction>,
            Without<InspectTradeButton>,
            Without<InspectPropertyButton>,
            Without<InspectManageButton>,
        ),
    >,
    trade_buttons: Query<
        &Interaction,
        (
            With<InspectTradeButton>,
            Changed<Interaction>,
            Without<InspectExpandButton>,
            Without<InspectPropertyButton>,
            Without<InspectManageButton>,
        ),
    >,
    property_buttons: Query<
        &Interaction,
        (
            With<InspectPropertyButton>,
            Changed<Interaction>,
            Without<InspectExpandButton>,
            Without<InspectTradeButton>,
            Without<InspectManageButton>,
        ),
    >,
) {
    for interaction in expand_buttons.iter() {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let Some(entity) = selection.primary() else {
            continue;
        };
        let (place_name, entry) = if let Ok((_, settlement, _)) = settlements.get(entity) {
            (
                settlement.name.clone(),
                crate::ui::encyclopedia::places::SelectedPlaceEntry::Hall,
            )
        } else if let Ok((building, position, _)) = buildings.get(entity) {
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

    for interaction in trade_buttons.iter() {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let Some(entity) = selection.primary() else {
            continue;
        };
        let market = markets.get(entity).is_ok().then_some(entity).or_else(|| {
            let (building, _, owner) = buildings.get(entity).ok()?;
            (building.kind == SettlementBuildingKind::Market).then_some(())?;
            let owner = owner?;
            settlements
                .iter()
                .find(|(_, _, id)| **id == owner.0)
                .map(|(hall, _, _)| hall)
        });
        if let Some(market) = market {
            encyclopedia_open.0 = false;
            property_target.0 = None;
            trade_target.0 = Some(market);
        }
    }

    for interaction in property_buttons.iter() {
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
    operations: Query<&OperatedBy>,
    mut encyclopedia_open: ResMut<crate::ui::encyclopedia::EncyclopediaOpen>,
    mut tab: ResMut<crate::ui::encyclopedia::EncyclopediaTab>,
    mut selected_company: ResMut<crate::ui::encyclopedia::companies::SelectedCompany>,
    mut return_to: ResMut<crate::ui::encyclopedia::companies::CompanyDrilldownReturn>,
    buttons: Query<&Interaction, (With<InspectManageButton>, Changed<Interaction>)>,
) {
    for interaction in buttons.iter() {
        if *interaction == Interaction::Pressed {
            let Some(entity) = selection.primary() else {
                continue;
            };
            let Ok(company) = operations.get(entity) else {
                continue;
            };
            selected_company.0 = Some(company.0);
            return_to.0 = None;
            *tab = crate::ui::encyclopedia::EncyclopediaTab::Companies;
            encyclopedia_open.0 = true;
        }
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
    target: Entity,
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

#[derive(Component)]
struct TradeListingViewport;

#[derive(Resource, Default)]
struct TradeFeedback {
    message: String,
    success: bool,
}

const HERO_MARKET_INTERACTION_RANGE: f32 = 12.0;

fn nearest_public_market_entrance(
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

fn ensure_trade_panel(
    mut commands: Commands,
    time: Res<Time<Real>>,
    target: Res<TradePanelTarget>,
    settlements: Query<(
        &Settlement,
        &SettlementId,
        Option<&CivicHallLevel>,
        &PlayerPosition,
        Option<&PlayerRotation>,
    )>,
    buildings: Query<(
        &SettlementBuilding,
        Option<&BuildingOf>,
        Option<&PlayerPosition>,
        Option<&PlayerRotation>,
    )>,
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
    roots: Query<(Entity, &TradePanelRoot, Option<&UiRefreshStamp>)>,
    asset_server: Res<AssetServer>,
    children: Query<&Children>,
    interactions: Query<(&Interaction, Has<crate::ui::foundation::UiRefreshExempt>)>,
    scrolls: Query<&ScrollPosition, With<TradeListingViewport>>,
) {
    let Some(entity) = target.0 else {
        for (root, ..) in roots.iter() {
            commands.entity(root).despawn();
        }
        return;
    };
    let Ok(market) = markets.get(entity) else {
        for (root, ..) in roots.iter() {
            commands.entity(root).despawn();
        }
        return;
    };
    let inventory = inventories.get(entity).ok();
    let (place, subtitle, treasury, hall_access) =
        if let Ok((settlement, settlement_id, hall_level, position, rotation)) =
            settlements.get(entity)
        {
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
                Some((
                    *settlement_id,
                    position.0,
                    rotation.map_or(0.0, |rotation| rotation.0),
                )),
            )
        } else if let Ok((building, ..)) = buildings.get(entity) {
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
    let market_position = local_hero.zip(hall_access).map(
        |((_, hero_position, ..), (settlement_id, hall_position, hall_rotation))| {
            let hall_entrance =
                SettlementBuildingKind::Hall.entrance_position(hall_position, hall_rotation);
            nearest_public_market_entrance(
                hero_position.0,
                hall_entrance,
                buildings
                    .iter()
                    .filter(|(building, building_of, ..)| {
                        building.kind == SettlementBuildingKind::Market
                            && building_of.is_some_and(|owner| owner.0 == settlement_id)
                    })
                    .filter_map(|(building, _, position, rotation)| {
                        Some(building.kind.entrance_position(position?.0, rotation?.0))
                    }),
            )
        },
    );
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
    if roots.iter().any(|(_, root, _)| root.signature == signature) {
        return;
    }
    if roots.iter().any(|(entity, _, stamp)| {
        subtree_is_interacting(entity, &children, &interactions)
            || stamp.is_some_and(|stamp| !stamp.is_ready(&time))
    }) {
        return;
    }
    let retained_scroll = retained_scroll(
        roots.iter().any(|(_, root, _)| root.target == entity),
        scrolls.iter().next().map(|position| position.0),
    );
    for (root, ..) in roots.iter() {
        commands.entity(root).despawn();
    }

    let nodes = spawn_modal(
        &mut commands,
        TradePanelRoot {
            signature: signature.clone(),
            target: entity,
        },
        TradeBackdrop,
        TradePanel,
        ModalLayout {
            panel_size: Vec2::new(900.0, 600.0),
            panel_padding: 0.0,
        },
    );
    commands
        .entity(nodes.root)
        .insert(UiRefreshStamp::now(&time));
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
                        button_chrome(UiButtonVariant::Ghost),
                    ))
                    .with_child((
                        Text::new("X"),
                        UiButtonLabel,
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
                        "{} / private consignment / {}% fee",
                        market.trade_tier().label(),
                        market.market_fee_bps() as f32 / 100.0,
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
                        |stock| {
                            stock.partition_bulk_capacity().map_or_else(
                                || {
                                    format!(
                                        "{} / {} bulk",
                                        stock.used_bulk(),
                                        stock.bulk_capacity()
                                    )
                                },
                                |capacity| format!("{capacity} bulk per resource"),
                            )
                        },
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
            .spawn((
                TradeListingViewport,
                ScrollPosition(retained_scroll),
                Node {
                    flex_grow: 1.0,
                    min_height: Val::Px(0.0),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::axes(Val::Px(22.0), Val::Px(12.0)),
                    overflow: Overflow::scroll_y(),
                    scrollbar_width: 8.0,
                    ..default()
                },
            ))
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
                "Select your hero and walk within 12m of the Hall or Marketplace, then press E or reopen this board to trade."
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
    let trade_unlocked = market.can_trade(good);
    let condition = if !trade_unlocked {
        format!("UNLOCKS AT {}", good.minimum_market_tier().label())
    } else if unmet > 0 {
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
            let offers = if trade_unlocked {
                market
                    .listings()
                    .iter()
                    .filter(|listing| listing.good == good)
                    .count()
            } else {
                0
            };
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
                    enabled: can_trade && trade_unlocked && market.listed_units(good) > 0,
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
                        && trade_unlocked
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
                button_chrome(UiButtonVariant::Secondary),
            ))
            .with_child((
                Text::new("HISTORY"),
                UiButtonLabel,
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
    let mut button = parent.spawn((
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
        button_chrome(UiButtonVariant::Secondary),
    ));
    if !enabled {
        button.insert(InteractionDisabled);
    }
    button.with_child((
        Text::new(label),
        UiButtonLabel,
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
/// opens a remote read-only board; E opens the settlement's shared exchange
/// when the embodied hero is at either its Hall or Marketplace counter.
fn open_nearby_market_on_interact(
    keyboard: Res<ButtonInput<KeyCode>>,
    input: Res<crate::input::InputState>,
    local: Option<Res<crate::camera_rts::LocalPeerId>>,
    heroes: Query<(&shared::components::Hero, &PlayerPosition)>,
    halls: Query<
        (
            Entity,
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
        .filter_map(|(entity, settlement_id, position, rotation)| {
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

fn sync_permit_tray_input_state(
    permits: Res<crate::ui::player_permits::PermitTrayOpen>,
    mut input: ResMut<crate::input::InputState>,
) {
    if input.permit_tray_open != permits.0 {
        input.permit_tray_open = permits.0;
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
