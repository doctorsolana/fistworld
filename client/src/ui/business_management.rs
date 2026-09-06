//! Owner controls for one player-owned business.
//!
//! The panel edits replicated policies rather than maintaining client-only
//! settings. NPC autopilot and player management therefore remain one economy.
//!
//! Rendering follows the build-once/bind-in-place rule from
//! `docs/UI-ARCHITECTURE.md`: every frame the replicated state is folded into a
//! pure [`ControlsModel`] (sections, rows, controls and meters, each with a
//! stable id). The id sequence is the panel's *structure key*; only a change
//! in that sequence respawns nodes. Everything else - values, button labels,
//! selected chrome, the order a button will send, meter fills - is written
//! into the existing entities by id, so stepping a wage or posting a share
//! offer never rebuilds the scroll body or steals the cursor's hover.

use std::collections::HashMap;

use bevy::prelude::*;
use lightyear::prelude::{Connected, MessageReceiver, MessageSender};
use shared::components::{
    BuildingId, CharacterName, Company, CompanyId, CompanyLeadership, CompanyOwnership,
    CompanyShareMarket, Hero, OperatedBy, PersonId, SettlementBuilding, SettlementBuildingKind,
};
use shared::economy::{
    format_money, BusinessAccount, BusinessManagementPolicy, BusinessProcurementPolicy,
    BusinessSalePolicy, BusinessSourcingMode, BusinessStaffingPolicy, BusinessStrategy,
    BusinessSupplyPolicy, BusinessWagePolicy, CompanyAccount, CompanyDecisionHistory,
    CompanyManagementPolicy, Good, GoodsInventory, TavernService,
};
use shared::protocol::{
    HeroBusinessAction, HeroBusinessOrder, HeroBusinessResult, ReliableChannel,
};

use crate::states::GameState;
use crate::ui::foundation::{
    retained_scroll, selected_button_chrome, subtree_is_interacting, UiButtonLabel, UiButtonStyle,
    UiButtonVariant,
};
use crate::ui::modal::update_modal_click_guard;
use crate::ui::styles::{
    INK, INK_MUTED, LIMEWASH, LIMEWASH_LIT, LIMEWASH_WELL, PLATE_RULE_SOFT, RADIUS,
};

pub struct BusinessManagementPlugin;

impl Plugin for BusinessManagementPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BusinessManagementTarget>();
        app.init_resource::<BusinessManagementReturn>();
        app.init_resource::<BusinessClickGuard>();
        app.init_resource::<BusinessFeedback>();
        app.init_resource::<ShareOrderDraft>();
        app.add_systems(
            Update,
            (
                receive_results,
                ensure_panel,
                update_guard,
                handle_action_buttons,
                handle_share_draft_buttons,
                sync_input_state,
            )
                .chain()
                .run_if(in_state(GameState::Playing)),
        );
        app.add_systems(OnExit(GameState::Playing), cleanup);
    }
}

#[derive(Resource, Default)]
pub(crate) struct BusinessManagementTarget(pub Option<Entity>);

/// Optional encyclopedia destination used when a site was opened from its
/// company record. The X still dismisses the whole flow; BACK restores it.
#[derive(Resource, Default, Clone, Copy)]
pub(crate) struct BusinessManagementReturn(pub Option<CompanyId>);

#[derive(Resource, Default)]
struct BusinessClickGuard(bool);

#[derive(Resource, Default)]
struct BusinessFeedback {
    message: String,
    success: bool,
}

#[derive(Component)]
struct Root {
    structure: String,
    target: Entity,
}

/// The page's scrolling body; `pub(crate)` so the capture harness can scroll it.
#[derive(Component)]
pub(crate) struct BodyScroll;

/// A text node whose content is bound by model id every frame.
#[derive(Component)]
struct BoundText(String);

/// A button whose label, selected chrome and (for orders) payload are bound
/// by model id every frame.
#[derive(Component)]
struct BoundButton(String);

/// One lane of a meter track; its width is bound by model id.
#[derive(Component)]
struct MeterFill {
    id: String,
    lane: usize,
}

/// Panel state plus the encyclopedia host this page renders into; bundled so
/// `ensure_panel` stays under Bevy's parameter limit.
#[derive(bevy::ecs::system::SystemParam)]
struct ManagementPanelUi<'w, 's> {
    roots: Query<'w, 's, (Entity, &'static Root)>,
    body_scroll: Query<'w, 's, &'static ScrollPosition, With<BodyScroll>>,
    children: Query<'w, 's, &'static Children>,
    interactions: Query<
        'w,
        's,
        (
            &'static Interaction,
            Has<crate::ui::foundation::UiRefreshExempt>,
        ),
    >,
    hosts: Query<'w, 's, Entity, With<crate::ui::encyclopedia::EncyclopediaPageHost>>,
    encyclopedia_open: ResMut<'w, crate::ui::encyclopedia::EncyclopediaOpen>,
    tab: ResMut<'w, crate::ui::encyclopedia::EncyclopediaTab>,
    perf: Res<'w, crate::ui::perf::UiPerf>,
}

/// The entities the bind pass writes into.
#[derive(bevy::ecs::system::SystemParam)]
struct BoundControls<'w, 's> {
    texts: Query<
        'w,
        's,
        (
            &'static BoundText,
            &'static mut Text,
            &'static mut TextColor,
            &'static mut Node,
        ),
    >,
    buttons: Query<
        'w,
        's,
        (
            &'static BoundButton,
            &'static mut UiButtonStyle,
            Option<&'static mut Action>,
        ),
    >,
    fills: Query<'w, 's, (&'static MeterFill, &'static mut Node), Without<BoundText>>,
}

#[derive(Component, Clone, Copy)]
struct Action(HeroBusinessAction);

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
enum ShareDraftAction {
    SharesDown,
    SharesUp,
    PriceDown,
    PriceUp,
}

#[derive(Resource, Debug, Clone, Copy)]
struct ShareOrderDraft {
    company: Option<CompanyId>,
    shares: u16,
    unit_price: u64,
}

impl Default for ShareOrderDraft {
    fn default() -> Self {
        Self {
            company: None,
            shares: 10,
            unit_price: 100,
        }
    }
}

// --- type scale -------------------------------------------------------------

const T_TITLE: f32 = 22.0;
const T_SECTION: f32 = 12.0;
const T_VALUE: f32 = 15.0;
const T_BUTTON: f32 = 14.0;
const T_BODY: f32 = 13.5;
const T_LABEL: f32 = 12.0;

const FEEDBACK_OK: Color = Color::srgb(0.13, 0.38, 0.19);
const FEEDBACK_FAIL: Color = Color::srgb(0.58, 0.12, 0.10);
const COMPANY_STOCK_FILL: Color = Color::srgba(0.34, 0.32, 0.28, 0.94);
const MARKET_STOCK_FILL: Color = Color::srgba(0.62, 0.57, 0.48, 0.94);

// --- model ------------------------------------------------------------------

/// What pressing a control does. Orders go to the server; draft steps edit the
/// local share-offer draft.
#[derive(Clone, Copy, Debug)]
enum Press {
    Order(HeroBusinessAction),
    Draft(ShareDraftAction),
}

struct ControlModel {
    id: String,
    label: String,
    press: Press,
    selected: bool,
}

struct RowModel {
    id: String,
    label: String,
    value: String,
    controls: Vec<ControlModel>,
}

struct MeterModel {
    id: String,
    title: String,
    summary: String,
    /// Lane widths in percent of the track.
    lanes: [f32; 2],
}

enum Block {
    Section(String),
    Row(RowModel),
    Meter(MeterModel),
}

/// The whole panel as data. See the module docs for how it is rendered.
struct ControlsModel {
    title: String,
    subtitle: String,
    blocks: Vec<Block>,
    feedback: Option<(String, bool)>,
}

impl ControlsModel {
    /// The id sequence; equal keys mean the spawned tree can be reused.
    fn structure_key(&self) -> String {
        let mut key = String::new();
        for block in &self.blocks {
            match block {
                Block::Section(label) => {
                    key.push_str("s:");
                    key.push_str(label);
                }
                Block::Row(row) => {
                    key.push_str("|r:");
                    key.push_str(&row.id);
                    for control in &row.controls {
                        key.push_str(",c:");
                        key.push_str(&control.id);
                    }
                }
                Block::Meter(meter) => {
                    key.push_str("|m:");
                    key.push_str(&meter.id);
                }
            }
            key.push('|');
        }
        key
    }
}

fn order(
    id: impl Into<String>,
    label: impl Into<String>,
    action: HeroBusinessAction,
) -> ControlModel {
    ControlModel {
        id: id.into(),
        label: label.into(),
        press: Press::Order(action),
        selected: false,
    }
}

fn choice(
    id: impl Into<String>,
    label: impl Into<String>,
    action: HeroBusinessAction,
    selected: bool,
) -> ControlModel {
    ControlModel {
        selected,
        ..order(id, label, action)
    }
}

fn row(
    id: impl Into<String>,
    label: impl Into<String>,
    value: impl Into<String>,
    controls: Vec<ControlModel>,
) -> Block {
    Block::Row(RowModel {
        id: id.into(),
        label: label.into(),
        value: value.into(),
        controls,
    })
}

fn meter(
    id: impl Into<String>,
    title: impl Into<String>,
    summary: impl Into<String>,
    first_units: u32,
    second_units: u32,
    scale_units: u32,
) -> Block {
    let scale = scale_units.max(1) as f32;
    let first = (first_units as f32 / scale * 100.0).clamp(0.0, 100.0);
    let second = (second_units as f32 / scale * 100.0).clamp(0.0, 100.0 - first);
    Block::Meter(MeterModel {
        id: id.into(),
        title: title.into(),
        summary: summary.into(),
        lanes: [first, second],
    })
}

fn signed_coin(pennies: i64) -> String {
    format!(
        "{}{} coin",
        if pennies < 0 { "-" } else { "+" },
        format_money(pennies.unsigned_abs())
    )
}

struct CompanyView<'a> {
    id: CompanyId,
    company: &'a Company,
    ownership: &'a CompanyOwnership,
    leadership: &'a CompanyLeadership,
    account: &'a CompanyAccount,
    policy: &'a CompanyManagementPolicy,
    decisions: &'a CompanyDecisionHistory,
    share_market: &'a CompanyShareMarket,
}

struct SiteView<'a> {
    building: &'a SettlementBuilding,
    building_id: BuildingId,
    account: &'a BusinessAccount,
    management: &'a BusinessManagementPolicy,
    wage: &'a BusinessWagePolicy,
    sale: &'a BusinessSalePolicy,
    staffing: Option<&'a BusinessStaffingPolicy>,
    procurement: &'a BusinessProcurementPolicy,
    supply: &'a BusinessSupplyPolicy,
    inventory: &'a GoodsInventory,
    tavern_service: Option<&'a TavernService>,
}

struct ModelInputs<'a> {
    site: SiteView<'a>,
    company: Option<CompanyView<'a>>,
    local_person: Option<PersonId>,
    share_draft: &'a ShareOrderDraft,
    /// True when the page was opened directly rather than from its company
    /// record; the company-wide block is shown here only in that case.
    direct_open: bool,
    feedback: &'a BusinessFeedback,
    name_of: &'a dyn Fn(PersonId) -> String,
}

#[allow(clippy::too_many_lines)]
fn controls_model(inputs: &ModelInputs<'_>) -> ControlsModel {
    let ModelInputs {
        site,
        company,
        local_person,
        share_draft,
        direct_open,
        feedback,
        name_of,
    } = inputs;
    let building = site.building;
    let kind_label = building.kind.label().to_uppercase();
    let site_label = format!("{kind_label} #{}", site.building_id.0);
    let title = format!("MANAGE {site_label}");
    let subtitle = company.as_ref().map_or_else(
        || {
            format!(
                "INDEPENDENT SITE  /  IN {}",
                building.settlement.to_uppercase()
            )
        },
        |company| {
            format!(
                "ONE SITE OF {}  /  IN {}  /  COMPANY TREASURY {} COIN",
                company.company.name.to_uppercase(),
                building.settlement.to_uppercase(),
                format_money(company.account.cash),
            )
        },
    );
    let can_manage = company.as_ref().is_none_or(|company| {
        local_person.is_some_and(|person| company.leadership.can_manage(person))
    });
    let mut blocks = Vec::new();

    // Company ownership and share trading live on the Company page. Keep the
    // fallback only for a direct open; the Back-to-Company flow starts with
    // the site.
    if let Some(company) = company.as_ref().filter(|_| *direct_open) {
        blocks.push(Block::Section(
            "COMPANY FINANCE, OWNERSHIP & GOVERNANCE".into(),
        ));
        let cap_table = company
            .ownership
            .shares()
            .iter()
            .map(|holding| {
                format!(
                    "{}: {} / 1,000 ({:.1}%)",
                    name_of(holding.shareholder),
                    holding.shares,
                    f32::from(holding.shares) / 10.0
                )
            })
            .collect::<Vec<_>>()
            .join("  /  ");
        let appoint = local_person
            .is_some_and(|person| company.ownership.can_appoint_master(person))
            .then(|| {
                company
                    .ownership
                    .shares()
                    .iter()
                    .map(|holding| {
                        let name = name_of(holding.shareholder);
                        choice(
                            format!("appoint.{}", holding.shareholder.0),
                            if company.leadership.master == holding.shareholder {
                                format!("{name} IS MASTER")
                            } else {
                                format!("APPOINT {name}")
                            },
                            HeroBusinessAction::AppointCompanyMaster(holding.shareholder),
                            company.leadership.master == holding.shareholder,
                        )
                    })
                    .collect()
            })
            .unwrap_or_default();
        blocks.push(row(
            "company.captable",
            "COMPANY & 1,000-SHARE CAP TABLE",
            format!(
                "{} (#{})  /  {}: {}\nTreasury {} coin  /  assets {} coin  /  debt {} wage + {} tax\n{cap_table}",
                company.company.name,
                company.id.0,
                CompanyLeadership::TITLE,
                name_of(company.leadership.master),
                format_money(company.account.cash),
                format_money(company.account.book_value),
                format_money(company.account.wage_arrears),
                format_money(company.account.tax_arrears),
            ),
            appoint,
        ));

        let own_shares = local_person.map_or(0, |person| company.ownership.share_count(person));
        let own_offer = local_person.and_then(|person| company.share_market.offer_from(person));
        let listed = own_offer.map_or_else(
            || "No active offer.".to_string(),
            |offer| {
                format!(
                    "Listed: {} at {} coin each since day {}.",
                    offer.shares,
                    format_money(offer.unit_price),
                    offer.listed_day,
                )
            },
        );
        let mut controls = Vec::new();
        if own_shares > 0 {
            controls.push(ControlModel {
                id: "draft.shares.down".into(),
                label: "SHARES -10".into(),
                press: Press::Draft(ShareDraftAction::SharesDown),
                selected: false,
            });
            controls.push(ControlModel {
                id: "draft.shares.up".into(),
                label: "SHARES +10".into(),
                press: Press::Draft(ShareDraftAction::SharesUp),
                selected: false,
            });
            controls.push(ControlModel {
                id: "draft.price.down".into(),
                label: "PRICE -0.25".into(),
                press: Press::Draft(ShareDraftAction::PriceDown),
                selected: false,
            });
            controls.push(ControlModel {
                id: "draft.price.up".into(),
                label: "PRICE +0.25".into(),
                press: Press::Draft(ShareDraftAction::PriceUp),
                selected: false,
            });
            controls.push(order(
                "share.post",
                "POST / REPLACE OFFER",
                HeroBusinessAction::ListCompanyShares {
                    shares: share_draft.shares.min(own_shares),
                    unit_price: share_draft.unit_price,
                },
            ));
        }
        if own_offer.is_some() {
            controls.push(order(
                "share.cancel",
                "CANCEL OFFER",
                HeroBusinessAction::CancelCompanyShareListing,
            ));
        }
        blocks.push(row(
            "share.offer",
            "YOUR SHARE OFFER",
            share_draft_summary(share_draft, own_shares, &listed),
            controls,
        ));

        let offers = company.share_market.offers();
        let offer_text = if offers.is_empty() {
            "No shares are currently offered.".to_string()
        } else {
            offers
                .iter()
                .map(|offer| {
                    format!(
                        "{}: {} shares at {} coin each",
                        name_of(offer.seller),
                        offer.shares,
                        format_money(offer.unit_price),
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        let mut buys = Vec::new();
        if let Some(person) = local_person {
            for offer in offers.iter().filter(|offer| offer.seller != *person) {
                let mut quantities = vec![1, 10, offer.shares];
                quantities
                    .iter_mut()
                    .for_each(|q| *q = (*q).min(offer.shares));
                quantities.retain(|q| *q > 0);
                quantities.dedup();
                for quantity in quantities {
                    buys.push(order(
                        format!("buy.{}.{quantity}", offer.seller.0),
                        format!(
                            "BUY {quantity} FROM {}",
                            name_of(offer.seller).to_uppercase()
                        ),
                        HeroBusinessAction::BuyCompanyShares {
                            seller: offer.seller,
                            shares: quantity,
                        },
                    ));
                }
            }
        }
        blocks.push(row("share.public", "PUBLIC SHARE OFFERS", offer_text, buys));
        blocks.push(dividend_row(
            "company.dividends",
            "COMPANY DIVIDENDS",
            company,
            can_manage,
        ));

        let decision_text = if company.decisions.entries().is_empty() {
            "No strategy change recorded yet".to_string()
        } else {
            company
                .decisions
                .entries()
                .iter()
                .rev()
                .take(5)
                .map(|decision| {
                    format!(
                        "Day {}: {} to {} - {}",
                        decision.day,
                        decision.from.label(),
                        decision.to.label(),
                        decision.reason.label(),
                    )
                })
                .collect::<Vec<_>>()
                .join("\n")
        };
        blocks.push(row(
            "company.decisions",
            "COMPANY MASTER DECISIONS",
            decision_text,
            vec![],
        ));
        blocks.push(row(
            "company.today",
            "RESULT TODAY",
            format!(
                "Site {} (external {} + internal {})  /  company {}",
                signed_coin(site.account.current_day.profit()),
                format_money(site.account.current_day.gross_revenue),
                format_money(site.account.current_day.internal_revenue),
                signed_coin(company.account.current_day.profit()),
            ),
            vec![],
        ));
    }

    blocks.push(Block::Section(format!(
        "SITE OPERATING CONTROLS  /  {site_label}"
    )));
    if can_manage {
        let management = site.management;
        blocks.push(row(
            "autopilot",
            "OWNER AUTOPILOT",
            if management.autopilot { "ON" } else { "OFF" },
            vec![order(
                "autopilot.toggle",
                if management.autopilot {
                    "PAUSE"
                } else {
                    "ENABLE"
                },
                HeroBusinessAction::SetAutopilot(!management.autopilot),
            )],
        ));
        blocks.push(row(
            "strategy",
            "STRATEGY",
            management.strategy.label(),
            [
                BusinessStrategy::Balanced,
                BusinessStrategy::Growth,
                BusinessStrategy::HighMargin,
                BusinessStrategy::Cautious,
                BusinessStrategy::Opportunistic,
            ]
            .into_iter()
            .enumerate()
            .map(|(index, strategy)| {
                choice(
                    format!("strategy.{index}"),
                    strategy.label().to_uppercase(),
                    HeroBusinessAction::SetStrategy(strategy),
                    strategy == management.strategy,
                )
            })
            .collect(),
        ));
        let wage = site.wage;
        blocks.push(row(
            "wage",
            "DAILY WAGE",
            format!(
                "{} coin  /  {}",
                format_money(wage.daily_wage),
                if wage.automatic {
                    "automatic"
                } else {
                    "manual"
                }
            ),
            vec![
                order(
                    "wage.down",
                    "-0.25",
                    HeroBusinessAction::SetDailyWage(wage.daily_wage.saturating_sub(25)),
                ),
                order(
                    "wage.up",
                    "+0.25",
                    HeroBusinessAction::SetDailyWage(wage.daily_wage.saturating_add(25)),
                ),
                choice(
                    "wage.auto",
                    "AUTO",
                    HeroBusinessAction::SetAutomaticWage(!wage.automatic),
                    wage.automatic,
                ),
            ],
        ));
        let enabled_positions = site
            .staffing
            .copied()
            .unwrap_or_default()
            .target_for(building.kind);
        blocks.push(row(
            "positions",
            "OPEN POSITIONS",
            format!(
                "{enabled_positions} of {} advertised",
                building.kind.positions()
            ),
            (0..=building.kind.positions())
                .map(|positions| {
                    choice(
                        format!("positions.{positions}"),
                        positions.to_string(),
                        HeroBusinessAction::SetEnabledPositions(positions),
                        positions == enabled_positions,
                    )
                })
                .collect(),
        ));
        let sale = site.sale;
        blocks.push(row(
            "price",
            if building.kind == SettlementBuildingKind::Tavern {
                "MEAL PRICE"
            } else {
                "ASKING PRICE"
            },
            format!(
                "{} coin  /  {}",
                format_money(sale.asking_unit_price),
                if sale.automatic_pricing {
                    "automatic"
                } else {
                    "manual"
                }
            ),
            vec![
                order(
                    "price.down",
                    "-0.25",
                    HeroBusinessAction::SetAskingPrice(sale.asking_unit_price.saturating_sub(25)),
                ),
                order(
                    "price.up",
                    "+0.25",
                    HeroBusinessAction::SetAskingPrice(sale.asking_unit_price.saturating_add(25)),
                ),
                choice(
                    "price.auto",
                    "AUTO",
                    HeroBusinessAction::SetAutomaticPricing(!sale.automatic_pricing),
                    sale.automatic_pricing,
                ),
            ],
        ));

        if let Some(service) = site.tavern_service {
            blocks.push(Block::Section("TAVERN SERVICE".into()));
            let day = service.current_day;
            blocks.push(meter(
                "tavern.service",
                "GUEST SERVICE TODAY",
                format!(
                    "{} of {} visits served  /  {} coin  /  {} unaffordable, {} unavailable, {} lost en route",
                    day.served_meals,
                    day.planned_visits,
                    format_money(day.revenue),
                    day.unaffordable_visits,
                    day.unavailable_visits,
                    day.route_failures,
                ),
                day.served_meals,
                0,
                service.daily_capacity().max(1),
            ));
            blocks.push(row(
                "tavern.floor",
                "OPEN FLOOR",
                format!(
                    "{} innkeeper{} on duty  /  {} of {} guest places taken",
                    service.innkeepers_on_duty,
                    if service.innkeepers_on_duty == 1 {
                        ""
                    } else {
                        "s"
                    },
                    service.current_guests,
                    service.guest_capacity,
                ),
                vec![],
            ));
        }

        blocks.push(Block::Section("GOODS FLOW".into()));
        let inventory = site.inventory;
        if let Some(output) = output_good(building.kind) {
            let held = inventory.amount(output);
            blocks.push(meter(
                format!("stock.{output:?}"),
                format!("{} SITE STOCK", output.label().to_uppercase()),
                format!(
                    "{held} units here  /  bulk {} of {}",
                    inventory.used_bulk(),
                    inventory.bulk_capacity(),
                ),
                held,
                0,
                (inventory.bulk_capacity() / output.bulk_per_unit()).max(1),
            ));
            if building.kind == SettlementBuildingKind::LivestockFarm {
                let wool = inventory.amount(Good::Wool);
                blocks.push(meter(
                    "stock.wool",
                    "WOOL BY-PRODUCT",
                    format!(
                        "{wool} units here  /  one per livestock cycle, priced from the Meat ask"
                    ),
                    wool,
                    0,
                    (inventory.bulk_capacity() / Good::Wool.bulk_per_unit()).max(1),
                ));
            }
        }
        if company.is_none() {
            blocks.push(row(
                "draws",
                "PROFIT DRAWS",
                if management.automatic_withdrawals {
                    "Automatic after protected working capital"
                } else {
                    "Retained in the business"
                },
                vec![
                    choice(
                        "draws.auto",
                        "AUTO DRAW",
                        HeroBusinessAction::SetAutomaticWithdrawals(
                            !management.automatic_withdrawals,
                        ),
                        management.automatic_withdrawals,
                    ),
                    order(
                        "draws.withdraw",
                        "WITHDRAW AVAILABLE",
                        HeroBusinessAction::WithdrawAvailableProfit,
                    ),
                ],
            ));
        }

        let procurement = site.procurement;
        let supply = site.supply;
        let inputs: Vec<_> = Good::ALL
            .into_iter()
            .filter(|good| procurement.rule(*good).enabled)
            .collect();
        if inputs.is_empty() {
            blocks.push(row(
                "inputs.none",
                "INPUT PROCUREMENT",
                "No purchased inputs",
                vec![],
            ));
        } else {
            let sourcing_on = procurement.automatic && supply.automatic;
            blocks.push(row(
                "inputs.sourcing",
                "INPUT SOURCING",
                if sourcing_on {
                    "ON  /  company deliveries first, then the selected market fallback"
                } else {
                    "PAUSED  /  no new input trips are requested"
                },
                vec![order(
                    "sourcing.toggle",
                    if sourcing_on {
                        "PAUSE SOURCING"
                    } else {
                        "RESUME SOURCING"
                    },
                    HeroBusinessAction::SetAutomaticProcurement(!sourcing_on),
                )],
            ));
            for good in inputs {
                let rule = procurement.rule(good);
                let private = supply.rule(good);
                let good_label = good.label().to_uppercase();
                blocks.push(input_coverage_meter(
                    good,
                    inventory.amount(good),
                    rule.target_units,
                    rule.reorder_below,
                    rule.coverage_days,
                ));
                blocks.push(row(
                    format!("cover.{good:?}"),
                    format!("{good_label} INPUT COVER"),
                    format!(
                        "{} day{} of work  /  {}-unit target at current staffing",
                        rule.coverage_days,
                        if rule.coverage_days == 1 { "" } else { "s" },
                        rule.target_units,
                    ),
                    [0u8, 1, 2, 3, 5, 7]
                        .into_iter()
                        .map(|days| {
                            choice(
                                format!("cover.{good:?}.{days}"),
                                coverage_label(days),
                                HeroBusinessAction::SetInputCoverageDays { good, days },
                                days == rule.coverage_days,
                            )
                        })
                        .collect(),
                ));
                blocks.push(row(
                    format!("priority.{good:?}"),
                    format!("{good_label} SUPPLY PRIORITY"),
                    sourcing_explanation(private.sourcing),
                    [
                        BusinessSourcingMode::PreferOwned,
                        BusinessSourcingMode::CheapestAvailable,
                        BusinessSourcingMode::OwnedOnly,
                    ]
                    .into_iter()
                    .enumerate()
                    .map(|(index, mode)| {
                        choice(
                            format!("priority.{good:?}.{index}"),
                            sourcing_short_label(mode),
                            HeroBusinessAction::SetInputSourcingMode { good, mode },
                            mode == private.sourcing,
                        )
                    })
                    .collect(),
                ));
                blocks.push(row(
                    format!("ceiling.{good:?}"),
                    format!("{good_label} BID CEILING"),
                    format!("{} coin", format_money(rule.maximum_unit_price)),
                    vec![
                        order(
                            format!("ceiling.{good:?}.down"),
                            "-0.25",
                            HeroBusinessAction::SetInputMaximumPrice {
                                good,
                                unit_price: rule.maximum_unit_price.saturating_sub(25),
                            },
                        ),
                        order(
                            format!("ceiling.{good:?}.up"),
                            "+0.25",
                            HeroBusinessAction::SetInputMaximumPrice {
                                good,
                                unit_price: rule.maximum_unit_price.saturating_add(25),
                            },
                        ),
                    ],
                ));
            }
        }

        if let Some(company) = company.as_ref() {
            blocks.push(Block::Section("COMPANY FINANCE".into()));
            blocks.push(row(
                "finance.context",
                "COMPANY CONTEXT",
                format!(
                    "{}  /  {}: {}\nTreasury {} coin  /  debt {} wage + {} tax  /  today {}",
                    company.company.name,
                    CompanyLeadership::TITLE,
                    name_of(company.leadership.master),
                    format_money(company.account.cash),
                    format_money(company.account.wage_arrears),
                    format_money(company.account.tax_arrears),
                    signed_coin(company.account.current_day.profit()),
                ),
                vec![],
            ));
            blocks.push(dividend_row(
                "finance.dividends",
                "DIVIDENDS",
                company,
                true,
            ));
        }
    } else {
        blocks.push(row(
            "authority",
            "OPERATING AUTHORITY",
            "Only the appointed Company Master can change wages, prices, sourcing, strategy or dividends. Ownership, shares and the ledger are on the company page.",
            vec![],
        ));
    }

    ControlsModel {
        title,
        subtitle,
        blocks,
        feedback: (!feedback.message.is_empty())
            .then(|| (feedback.message.clone(), feedback.success)),
    }
}

fn dividend_row(id: &str, label: &str, company: &CompanyView<'_>, can_manage: bool) -> Block {
    let automatic = company.policy.automatic_dividends;
    let controls = if can_manage {
        vec![
            choice(
                format!("{id}.auto"),
                "AUTO DIVIDEND",
                HeroBusinessAction::SetAutomaticWithdrawals(!automatic),
                automatic,
            ),
            order(
                format!("{id}.distribute"),
                "DISTRIBUTE AVAILABLE",
                HeroBusinessAction::WithdrawAvailableProfit,
            ),
        ]
    } else {
        vec![]
    };
    row(
        id,
        label,
        if automatic {
            "Automatic after company-wide payroll, tax, input and operating reserves"
        } else {
            "Retained until manually distributed"
        },
        controls,
    )
}

fn input_coverage_meter(good: Good, held: u32, target: u32, reorder_below: u32, days: u8) -> Block {
    let covered = held.min(target);
    let surplus = held.saturating_sub(target);
    let state = if days == 0 {
        "sourcing off"
    } else if held < reorder_below {
        "replenishment requested"
    } else if held < target {
        "using buffer"
    } else {
        "target covered"
    };
    meter(
        format!("input.{good:?}"),
        format!("{} INPUT COVERAGE", good.label().to_uppercase()),
        format!("{held} held  /  {target} target  /  {state}"),
        covered,
        surplus,
        held.max(target),
    )
}

fn share_draft_summary(draft: &ShareOrderDraft, own_shares: u16, listed: &str) -> String {
    format!(
        "You own {own_shares} / 1,000 shares.  Draft: {} shares at {} coin each.  {listed}",
        draft.shares,
        format_money(draft.unit_price),
    )
}

fn output_good(kind: SettlementBuildingKind) -> Option<Good> {
    match kind {
        SettlementBuildingKind::Farmstead => Some(Good::Wheat),
        SettlementBuildingKind::LumberjackHut => Some(Good::Wood),
        SettlementBuildingKind::StoneQuarry => Some(Good::Stone),
        SettlementBuildingKind::FishermansHut => Some(Good::Food),
        SettlementBuildingKind::Windmill => Some(Good::Flour),
        SettlementBuildingKind::Bakery => Some(Good::Bread),
        SettlementBuildingKind::LivestockFarm => Some(Good::Meat),
        _ => None,
    }
}

fn coverage_label(days: u8) -> String {
    match days {
        0 => "OFF".to_string(),
        1 => "1 DAY".to_string(),
        days => format!("{days} DAYS"),
    }
}

fn sourcing_short_label(mode: BusinessSourcingMode) -> &'static str {
    match mode {
        BusinessSourcingMode::PreferOwned => "COMPANY FIRST",
        BusinessSourcingMode::CheapestAvailable => "BEST VALUE",
        BusinessSourcingMode::OwnedOnly => "COMPANY ONLY",
    }
}

fn sourcing_explanation(mode: BusinessSourcingMode) -> &'static str {
    match mode {
        BusinessSourcingMode::PreferOwned => {
            "Company first  /  use reachable owned stock, then buy from the local market"
        }
        BusinessSourcingMode::CheapestAvailable => {
            "Best value  /  compare the landed company transfer with the local market"
        }
        BusinessSourcingMode::OwnedOnly => {
            "Company only  /  never buy this input from an outside seller"
        }
    }
}

// --- systems ----------------------------------------------------------------

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn ensure_panel(
    mut commands: Commands,
    mut target: ResMut<BusinessManagementTarget>,
    feedback: Res<BusinessFeedback>,
    return_to: Res<BusinessManagementReturn>,
    mut share_draft: ResMut<ShareOrderDraft>,
    local: Option<Res<crate::camera_rts::LocalPeerId>>,
    businesses: Query<(
        &SettlementBuilding,
        &BuildingId,
        Option<&OperatedBy>,
        &BusinessAccount,
        &BusinessManagementPolicy,
        &BusinessWagePolicy,
        &BusinessSalePolicy,
        Option<&BusinessStaffingPolicy>,
        &BusinessProcurementPolicy,
        &BusinessSupplyPolicy,
        &GoodsInventory,
        Option<&TavernService>,
    )>,
    companies: Query<(
        &CompanyId,
        &Company,
        &CompanyOwnership,
        &CompanyLeadership,
        &CompanyAccount,
        &CompanyManagementPolicy,
        &CompanyDecisionHistory,
        &CompanyShareMarket,
    )>,
    people: Query<(&PersonId, &CharacterName)>,
    heroes: Query<(&Hero, &PersonId)>,
    mut ui: ManagementPanelUi,
    mut bound: BoundControls,
) {
    let mut _ui_scope = ui.perf.scope("ensure_business_panel");
    let Some(entity) = target.0 else {
        for (root, ..) in ui.roots.iter() {
            commands.entity(root).despawn();
        }
        return;
    };
    // Company controls are an encyclopedia page: open the window on the
    // companies tab if needed and host the page next frame.
    let Ok(host) = ui.hosts.single() else {
        if !ui.encyclopedia_open.0 {
            ui.encyclopedia_open.0 = true;
            *ui.tab = crate::ui::encyclopedia::EncyclopediaTab::Companies;
        }
        return;
    };
    let Ok((
        building,
        building_id,
        operated_by,
        account,
        management,
        wage,
        sale,
        staffing,
        procurement,
        supply,
        inventory,
        tavern_service,
    )) = businesses.get(entity)
    else {
        target.0 = None;
        for (root, ..) in ui.roots.iter() {
            commands.entity(root).despawn();
        }
        return;
    };
    let company = operated_by
        .and_then(|operation| companies.iter().find(|(id, ..)| **id == operation.0))
        .map(
            |(id, company, ownership, leadership, account, policy, decisions, share_market)| {
                CompanyView {
                    id: *id,
                    company,
                    ownership,
                    leadership,
                    account,
                    policy,
                    decisions,
                    share_market,
                }
            },
        );
    let local_person = local.as_ref().and_then(|local| {
        heroes
            .iter()
            .find(|(hero, _)| shared::player::peer_id_to_u64(hero.owner) == local.0)
            .map(|(_, person)| *person)
    });
    if let Some(company) = company.as_ref() {
        if share_draft.company != Some(company.id) {
            *share_draft = ShareOrderDraft {
                company: Some(company.id),
                ..default()
            };
        }
    }
    let name_of = |person: PersonId| {
        people.iter().find(|(id, _)| **id == person).map_or_else(
            || format!("Person #{}", person.0),
            |(_, name)| name.0.clone(),
        )
    };
    let model = controls_model(&ModelInputs {
        site: SiteView {
            building,
            building_id: *building_id,
            account,
            management,
            wage,
            sale,
            staffing,
            procurement,
            supply,
            inventory,
            tavern_service,
        },
        company,
        local_person,
        share_draft: &share_draft,
        direct_open: return_to.0.is_none(),
        feedback: &feedback,
        name_of: &name_of,
    });
    let structure = model.structure_key();
    let existing = ui.roots.iter().next();
    if existing.is_some_and(|(_, root)| root.structure == structure && root.target == entity) {
        bind_panel(&model, &mut bound);
        return;
    }
    // A structural change while a control is pressed would pull the button
    // out from under the cursor; wait a frame.
    if existing
        .is_some_and(|(root, _)| subtree_is_interacting(root, &ui.children, &ui.interactions))
    {
        return;
    }
    let scroll = retained_scroll(
        existing.is_some_and(|(_, root)| root.target == entity),
        ui.body_scroll.iter().next().map(|position| position.0),
    );
    for (root, ..) in ui.roots.iter() {
        commands.entity(root).despawn();
    }
    _ui_scope.rebuilt();
    spawn_panel(&mut commands, host, entity, structure, &model, scroll);
}

/// Write the model's values into the spawned tree without touching structure.
fn bind_panel(model: &ControlsModel, bound: &mut BoundControls) {
    let mut texts: HashMap<&str, (&str, Option<Color>)> = HashMap::new();
    let mut buttons: HashMap<&str, (Press, bool)> = HashMap::new();
    let mut lanes: HashMap<&str, [f32; 2]> = HashMap::new();
    texts.insert("title", (&model.title, None));
    texts.insert("subtitle", (&model.subtitle, None));
    let (feedback_text, feedback_color) = model
        .feedback
        .as_ref()
        .map_or(("", FEEDBACK_OK), |(m, ok)| {
            (m.as_str(), if *ok { FEEDBACK_OK } else { FEEDBACK_FAIL })
        });
    texts.insert("feedback", (feedback_text, Some(feedback_color)));
    for block in &model.blocks {
        match block {
            Block::Section(_) => {}
            Block::Row(row) => {
                texts.insert(&row.id, (&row.value, None));
                for control in &row.controls {
                    texts.insert(&control.id, (&control.label, None));
                    buttons.insert(&control.id, (control.press, control.selected));
                }
            }
            Block::Meter(meter) => {
                texts.insert(&meter.id, (&meter.summary, None));
                lanes.insert(&meter.id, meter.lanes);
            }
        }
    }
    for (bound_text, mut text, mut color, mut node) in bound.texts.iter_mut() {
        let Some((value, tint)) = texts.get(bound_text.0.as_str()) else {
            continue;
        };
        if text.0 != *value {
            text.0 = (*value).to_string();
        }
        if let Some(tint) = tint {
            if color.0 != *tint {
                color.0 = *tint;
            }
        }
        let display = if value.is_empty() {
            Display::None
        } else {
            Display::Flex
        };
        if node.display != display {
            node.display = display;
        }
    }
    for (bound_button, mut style, action) in bound.buttons.iter_mut() {
        let Some((press, selected)) = buttons.get(bound_button.0.as_str()) else {
            continue;
        };
        if style.selected != *selected {
            style.selected = *selected;
        }
        if let (Some(mut action), Press::Order(next)) = (action, press) {
            if action.0 != *next {
                action.0 = *next;
            }
        }
    }
    for (fill, mut node) in bound.fills.iter_mut() {
        let Some(widths) = lanes.get(fill.id.as_str()) else {
            continue;
        };
        let width = Val::Percent(widths[fill.lane]);
        if node.width != width {
            node.width = width;
        }
    }
}

fn spawn_panel(
    commands: &mut Commands,
    host: Entity,
    target: Entity,
    structure: String,
    model: &ControlsModel,
    scroll: Vec2,
) {
    let panel_entity = commands
        .spawn((
            Root { structure, target },
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
    commands.entity(host).add_child(panel_entity);
    commands.entity(panel_entity).with_children(|panel| {
        panel
            .spawn((
                Node {
                    width: Val::Percent(100.0),
                    padding: UiRect::axes(Val::Px(22.0), Val::Px(15.0)),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(3.0),
                    border: UiRect::bottom(Val::Px(1.0)),
                    ..default()
                },
                BackgroundColor(LIMEWASH),
                BorderColor::all(PLATE_RULE_SOFT),
            ))
            .with_children(|header| {
                header.spawn((
                    BoundText("title".into()),
                    Text::new(model.title.clone()),
                    crate::ui::typography::text(T_TITLE),
                    TextColor(INK),
                ));
                header.spawn((
                    BoundText("subtitle".into()),
                    Text::new(model.subtitle.clone()),
                    crate::ui::typography::text(T_LABEL),
                    TextColor(INK_MUTED),
                ));
            });
        panel
            .spawn((
                BodyScroll,
                ScrollPosition(scroll),
                Node {
                    width: Val::Percent(100.0),
                    flex_grow: 1.0,
                    min_height: Val::Px(0.0),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(10.0),
                    padding: UiRect::all(Val::Px(22.0)),
                    overflow: Overflow::scroll_y(),
                    scrollbar_width: 8.0,
                    ..default()
                },
            ))
            .with_children(|body| {
                for block in &model.blocks {
                    match block {
                        Block::Section(label) => spawn_section(body, label),
                        Block::Row(row) => spawn_row(body, row),
                        Block::Meter(meter) => spawn_meter(body, meter),
                    }
                }
                let (message, ok) = model
                    .feedback
                    .as_ref()
                    .map_or((String::new(), true), |(m, ok)| (m.clone(), *ok));
                body.spawn((
                    BoundText("feedback".into()),
                    Text::new(message.clone()),
                    crate::ui::typography::text(T_BODY),
                    TextColor(if ok { FEEDBACK_OK } else { FEEDBACK_FAIL }),
                    Node {
                        display: if message.is_empty() {
                            Display::None
                        } else {
                            Display::Flex
                        },
                        ..default()
                    },
                ));
            });
    });
}

fn spawn_section(parent: &mut ChildSpawnerCommands<'_>, label: &str) {
    parent.spawn((
        Text::new(label),
        crate::ui::typography::text(T_SECTION),
        TextColor(INK_MUTED),
        Node {
            margin: UiRect::top(Val::Px(6.0)),
            ..default()
        },
    ));
}

fn spawn_row(parent: &mut ChildSpawnerCommands<'_>, row: &RowModel) {
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                padding: UiRect::axes(Val::Px(14.0), Val::Px(12.0)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(RADIUS)),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(8.0),
                ..default()
            },
            BackgroundColor(LIMEWASH),
            BorderColor::all(PLATE_RULE_SOFT),
        ))
        .with_children(|card| {
            card.spawn((
                Text::new(row.label.clone()),
                crate::ui::typography::text(T_LABEL),
                TextColor(INK_MUTED),
            ));
            card.spawn((
                BoundText(row.id.clone()),
                Text::new(row.value.clone()),
                crate::ui::typography::text(T_VALUE),
                TextColor(INK),
            ));
            if row.controls.is_empty() {
                return;
            }
            card.spawn(Node {
                width: Val::Percent(100.0),
                flex_wrap: FlexWrap::Wrap,
                column_gap: Val::Px(6.0),
                row_gap: Val::Px(6.0),
                margin: UiRect::top(Val::Px(2.0)),
                ..default()
            })
            .with_children(|controls| {
                for control in &row.controls {
                    spawn_control(controls, control);
                }
            });
        });
}

fn spawn_control(parent: &mut ChildSpawnerCommands<'_>, control: &ControlModel) {
    let mut button = parent.spawn((
        BoundButton(control.id.clone()),
        Button,
        Node {
            min_width: Val::Px(44.0),
            height: Val::Px(34.0),
            padding: UiRect::horizontal(Val::Px(12.0)),
            align_items: AlignItems::Center,
            justify_content: JustifyContent::Center,
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(RADIUS)),
            ..default()
        },
        selected_button_chrome(UiButtonVariant::Secondary, control.selected),
    ));
    match control.press {
        Press::Order(action) => {
            button.insert(Action(action));
        }
        Press::Draft(step) => {
            button.insert(step);
        }
    }
    button.with_child((
        BoundText(control.id.clone()),
        Text::new(control.label.clone()),
        UiButtonLabel,
        crate::ui::typography::text(T_BUTTON),
        TextColor(INK),
        Pickable::IGNORE,
    ));
}

fn spawn_meter(parent: &mut ChildSpawnerCommands<'_>, meter: &MeterModel) {
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                padding: UiRect::axes(Val::Px(14.0), Val::Px(12.0)),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(8.0),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(RADIUS)),
                ..default()
            },
            BackgroundColor(LIMEWASH),
            BorderColor::all(PLATE_RULE_SOFT),
        ))
        .with_children(|card| {
            card.spawn((
                Text::new(meter.title.clone()),
                crate::ui::typography::text(T_LABEL),
                TextColor(INK_MUTED),
            ));
            card.spawn((
                BoundText(meter.id.clone()),
                Text::new(meter.summary.clone()),
                crate::ui::typography::text(T_BODY),
                TextColor(INK),
            ));
            card.spawn((
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Px(14.0),
                    flex_direction: FlexDirection::Row,
                    overflow: Overflow::clip(),
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(3.0)),
                    ..default()
                },
                BackgroundColor(LIMEWASH_WELL),
                BorderColor::all(PLATE_RULE_SOFT),
            ))
            .with_children(|track| {
                for (lane, fill) in [COMPANY_STOCK_FILL, MARKET_STOCK_FILL]
                    .into_iter()
                    .enumerate()
                {
                    track.spawn((
                        MeterFill {
                            id: meter.id.clone(),
                            lane,
                        },
                        Node {
                            width: Val::Percent(meter.lanes[lane]),
                            height: Val::Percent(100.0),
                            ..default()
                        },
                        BackgroundColor(fill),
                    ));
                }
            });
        });
}

fn handle_share_draft_buttons(
    guard: Res<BusinessClickGuard>,
    mut draft: ResMut<ShareOrderDraft>,
    buttons: Query<(&Interaction, &ShareDraftAction), Changed<Interaction>>,
) {
    for (interaction, action) in buttons.iter() {
        if *interaction != Interaction::Pressed || !guard.0 {
            continue;
        }
        match action {
            ShareDraftAction::SharesDown => draft.shares = draft.shares.saturating_sub(10).max(1),
            ShareDraftAction::SharesUp => draft.shares = draft.shares.saturating_add(10).min(1_000),
            ShareDraftAction::PriceDown => {
                draft.unit_price = draft.unit_price.saturating_sub(25).max(1)
            }
            ShareDraftAction::PriceUp => {
                draft.unit_price = draft.unit_price.saturating_add(25).min(100_000)
            }
        }
    }
}

fn handle_action_buttons(
    guard: Res<BusinessClickGuard>,
    target: Res<BusinessManagementTarget>,
    buttons: Query<(&Interaction, &Action), Changed<Interaction>>,
    mut clients: Query<
        &mut MessageSender<HeroBusinessOrder>,
        (With<crate::GameClient>, With<Connected>),
    >,
) {
    for (interaction, action) in buttons.iter() {
        if *interaction != Interaction::Pressed || !guard.0 {
            continue;
        }
        let (Some(business), Ok(mut sender)) = (target.0, clients.single_mut()) else {
            continue;
        };
        sender.send::<ReliableChannel>(HeroBusinessOrder {
            business,
            action: action.0,
        });
    }
}

fn receive_results(
    mut receivers: Query<&mut MessageReceiver<HeroBusinessResult>, With<crate::GameClient>>,
    mut feedback: ResMut<BusinessFeedback>,
) {
    for mut receiver in receivers.iter_mut() {
        for result in receiver.receive() {
            feedback.message = result.message;
            feedback.success = result.success;
        }
    }
}

fn update_guard(
    mouse: Res<ButtonInput<MouseButton>>,
    target: Res<BusinessManagementTarget>,
    mut guard: ResMut<BusinessClickGuard>,
) {
    update_modal_click_guard(target.0.is_some(), &mouse, &mut guard.0);
}

fn sync_input_state(
    target: Res<BusinessManagementTarget>,
    mut input: ResMut<crate::input::InputState>,
) {
    input.business_management_open = target.0.is_some();
}

fn cleanup(
    mut commands: Commands,
    roots: Query<Entity, With<Root>>,
    mut target: ResMut<BusinessManagementTarget>,
    mut return_to: ResMut<BusinessManagementReturn>,
    mut input: ResMut<crate::input::InputState>,
) {
    for root in roots.iter() {
        commands.entity(root).despawn();
    }
    target.0 = None;
    return_to.0 = None;
    input.business_management_open = false;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quarter_coin_steps_are_exact_pennies() {
        assert_eq!(shared::economy::PENNIES_PER_COIN / 4, 25);
    }

    #[test]
    fn coverage_choices_use_human_days_instead_of_raw_unit_thresholds() {
        assert_eq!(coverage_label(0), "OFF");
        assert_eq!(coverage_label(1), "1 DAY");
        assert_eq!(coverage_label(5), "5 DAYS");
    }

    #[test]
    fn sourcing_modes_have_plain_language_labels() {
        assert_eq!(
            sourcing_short_label(BusinessSourcingMode::PreferOwned),
            "COMPANY FIRST"
        );
        assert_eq!(
            sourcing_short_label(BusinessSourcingMode::CheapestAvailable),
            "BEST VALUE"
        );
        assert_eq!(
            sourcing_short_label(BusinessSourcingMode::OwnedOnly),
            "COMPANY ONLY"
        );
    }

    fn sample_model(wage: u64, selected_positions: u8) -> ControlsModel {
        let building = SettlementBuilding {
            kind: SettlementBuildingKind::Bakery,
            settlement: "Brackwater".into(),
            owner: None,
            quality: 1.0,
            workers: vec![],
        };
        let staffing = BusinessStaffingPolicy::new(selected_positions);
        let feedback = BusinessFeedback::default();
        let draft = ShareOrderDraft::default();
        let name_of = |person: PersonId| format!("Person #{}", person.0);
        controls_model(&ModelInputs {
            site: SiteView {
                building: &building,
                building_id: BuildingId(7),
                account: &BusinessAccount::default(),
                management: &BusinessManagementPolicy::default(),
                wage: &BusinessWagePolicy {
                    daily_wage: wage,
                    ..default()
                },
                sale: &BusinessSalePolicy::default(),
                staffing: Some(&staffing),
                procurement: &BusinessProcurementPolicy::default(),
                supply: &BusinessSupplyPolicy::default(),
                inventory: &GoodsInventory::default(),
                tavern_service: None,
            },
            company: None,
            local_person: None,
            share_draft: &draft,
            direct_open: true,
            feedback: &feedback,
            name_of: &name_of,
        })
    }

    #[test]
    fn value_changes_keep_the_structure_key_so_the_panel_binds_in_place() {
        let before = sample_model(500, 1);
        let after = sample_model(525, 2);
        assert_eq!(before.structure_key(), after.structure_key());
        let wage_value = |model: &ControlsModel| {
            model
                .blocks
                .iter()
                .find_map(|block| match block {
                    Block::Row(row) if row.id == "wage" => Some(row.value.clone()),
                    _ => None,
                })
                .unwrap()
        };
        assert_ne!(wage_value(&before), wage_value(&after));
    }

    #[test]
    fn control_ids_are_unique_within_a_model() {
        let model = sample_model(500, 1);
        let mut ids = Vec::new();
        for block in &model.blocks {
            match block {
                Block::Row(row) => {
                    ids.push(row.id.clone());
                    ids.extend(row.controls.iter().map(|control| control.id.clone()));
                }
                Block::Meter(meter) => ids.push(meter.id.clone()),
                Block::Section(_) => {}
            }
        }
        let mut deduped = ids.clone();
        deduped.sort();
        deduped.dedup();
        assert_eq!(ids.len(), deduped.len(), "duplicate bound ids: {ids:?}");
    }
}
