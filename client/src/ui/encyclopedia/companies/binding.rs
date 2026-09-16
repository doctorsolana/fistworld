//! Typed bind-in-place markers for the company page.
//!
//! Every volatile value on the page -- a number, a status label, a colour, a
//! meter width, a note that may be hidden, or the absolute payload a button
//! sends to the authoritative server -- carries one [`CompanyBound`] key.
//! [`CompanyView::value`] is the single pure source of those values: the
//! spawn helpers call it once while building the tree, and the bind pass in
//! `view.rs` calls it again on every changed snapshot and rewrites only the
//! components whose value differs. Because both paths share one function the
//! spawned tree and the bound tree can never disagree.
//!
//! Structure -- which cards, rows and buttons exist -- is decided by
//! [`company_structure_key`], a hash of ids and existence/gating bits only.
//! It never hashes a value, so a cash, stock or cargo tick cannot respawn the
//! pane. Anything a `spawn_*` function branches on must be either in that key
//! or expressed as a bound `Display` toggle on an always-spawned node.
//!
//! Keys are `Copy + Eq + Hash` so the bind pass is a `match` over ids with
//! no per-frame `String` allocation or string hashing.

use super::controls::{CompanyBranchPolicyButton, CompanyManagementButton};
use super::model::{
    CompanyBranchRecord, CompanyDirectory, CompanyPolicyFeedback, CompanyRecord,
    CompanyRouteRecord, CompanySiteRecord, TradeRouteDraft, TradeRouteEditorState,
};
use super::widgets::signed_money;
use crate::ui::business_management::{local_dividend_take, BusinessManagementSelection};
use crate::ui::encyclopedia::person_links::PersonLink;
use crate::ui::history::CompanyHistoryButton;
use crate::ui::styles::{EMBER, INK_MUTED, PLATE_RULE_SOFT};
use bevy::ecs::system::EntityCommands;
use bevy::prelude::*;
use shared::components::{
    BuildingId, PersonId, SettlementBuildingKind, SettlementId, ShipId, ShipOrderId,
    ShipOrderStatus, TradeRouteId, TradeRouteMode, TradeRouteStatus, COMPANY_TOTAL_SHARES,
};
use shared::economy::{format_money, BusinessSourcingMode, CompanyDayLedger, Good};
use shared::protocol::HeroCompanyAction;
use std::hash::{Hash, Hasher};

/// Which live value a node shows. See the module docs for the convention.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum CompanyBound {
    Company(CompanyField),
    Site(BuildingId, SiteField),
    /// The branch card's summary line for one settlement.
    Branch(SettlementId),
    Resource(SettlementId, Good, ResourceField),
    Route(TradeRouteId, RouteField),
    RouteStop(TradeRouteId, u8, StopField),
    /// Trip row `n` (0 = most recent) of a route card.
    RouteTrip(TradeRouteId, u8),
    /// A fleet ship's status line.
    Ship(ShipId),
    Order(ShipOrderId, OrderField),
    /// Shareholder slot `n` in display order (largest holding first).
    Holder(u8, HolderField),
    /// Public offer slot `n` in display order (cheapest first).
    Offer(u8, PairField),
    /// Master decision slot `n` (0 = most recent).
    Decision(u8, PairField),
    Portfolio(PortfolioField),
    Editor(EditorField),
    EditorStop(u8, EditorStopField),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum CompanyField {
    Name,
    /// Payload: `CompanyHistoryButton.name`.
    HistoryName,
    Status,
    Cash,
    Revenue,
    Profit,
    Liabilities,
    Assets,
    Equity,
    TodayRevenue,
    TodayWages,
    TodayInputs,
    TodayFees,
    TodayTax,
    TodayProfit,
    DayLabel(DayLedger),
    DayLine(DayLedger),
    /// Display toggle on the INTERNAL FLOW MEMO row.
    DayMemoRow(DayLedger),
    DayMemo(DayLedger),
    LifetimeCapital,
    PositionShares,
    PositionInterest,
    PositionRole,
    PositionNote,
    RoutesHint,
    MasterName,
    OperatingPolicy,
    Dividends,
    /// Replicated `CompanyDividendCapacity`: available now, reserves, last paid.
    DividendCapacity,
    /// The local holder's exact take of a full distribution now.
    PositionDividend,
    NoteFeedback,
    NoteRoute,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum DayLedger {
    Today,
    Previous,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum SiteField {
    State,
    Staff,
    /// Payload: `CompanyManagementButton.target = Site(entity)`.
    Settings,
    LedgerTitle,
    LedgerState,
    LedgerSummary,
    LedgerStock,
    LedgerInput,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum ResourceField {
    Line,
    Policy,
    Meter,
    ToggleLabel,
    /// Payload: `SetSellExcess { enabled: !policy.sell_excess }`.
    Toggle,
    /// Payload: `SetRetainUnits { units }` for one stepper button.
    Step(RetainStep),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum RetainStep {
    Clear,
    Minus10,
    Minus1,
    Plus1,
    Plus10,
    Max,
}

impl RetainStep {
    pub(super) const ALL: [Self; 6] = [
        Self::Clear,
        Self::Minus10,
        Self::Minus1,
        Self::Plus1,
        Self::Plus10,
        Self::Max,
    ];

    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Clear => "CLEAR",
            Self::Minus10 => "-10",
            Self::Minus1 => "-1",
            Self::Plus1 => "+1",
            Self::Plus10 => "+10",
            Self::Max => "MAX",
        }
    }

    fn units(self, retain_units: u32, unit_capacity: u32) -> u32 {
        match self {
            Self::Clear => 0,
            Self::Minus10 => retain_units.saturating_sub(10),
            Self::Minus1 => retain_units.saturating_sub(1),
            Self::Plus1 => retain_units.saturating_add(1).min(unit_capacity),
            Self::Plus10 => retain_units.saturating_add(10).min(unit_capacity),
            Self::Max => unit_capacity,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum RouteField {
    Title,
    Subtitle,
    Status,
    Cargo,
    Caravaner,
    Trips,
    Units,
    Service,
    Summary,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum StopField {
    Name,
    Action,
    /// Highlight colours of the stop card itself.
    Card,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum OrderField {
    Title,
    Status,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum HolderField {
    /// Payload: the row's `PersonLink`.
    Link,
    Name,
    Detail,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum PairField {
    Label,
    Value,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum PortfolioField {
    Wallet,
    Holdings,
    Interest,
    Master,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum EditorField {
    Title,
    Subtitle,
    SaveLabel,
    Message,
    Home,
    Cargo,
    Capacity,
    Prices,
    ToggleLabel,
    Warning,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum EditorStopField {
    Name,
    Action,
    Warning,
}

/// Everything one key may write. Each `Some` field is applied to the matching
/// component when the entity has it; `None` leaves that component alone.
#[derive(Default, Debug, Clone, PartialEq)]
pub(super) struct BoundValue {
    pub text: Option<String>,
    pub color: Option<Color>,
    pub display: Option<Display>,
    /// `Node.width` as a percentage.
    pub fill: Option<f32>,
    /// `(BackgroundColor, BorderColor)` of a highlighted card.
    pub highlight: Option<(Color, Color)>,
    pub action: Option<HeroCompanyAction>,
    pub site: Option<Entity>,
    pub person: Option<PersonId>,
}

impl BoundValue {
    fn text(text: impl Into<String>) -> Self {
        Self {
            text: Some(text.into()),
            ..default()
        }
    }

    fn colored(text: impl Into<String>, color: Color) -> Self {
        Self {
            text: Some(text.into()),
            color: Some(color),
            ..default()
        }
    }

    /// A note that disappears entirely while its text is empty.
    fn note(text: String) -> Self {
        let display = if text.is_empty() {
            Display::None
        } else {
            Display::Flex
        };
        Self {
            text: Some(text),
            display: Some(display),
            ..default()
        }
    }

    fn shown(text: String, visible: bool) -> Self {
        Self {
            text: Some(text),
            display: Some(if visible {
                Display::Flex
            } else {
                Display::None
            }),
            ..default()
        }
    }

    fn visible(visible: bool) -> Self {
        Self {
            display: Some(if visible {
                Display::Flex
            } else {
                Display::None
            }),
            ..default()
        }
    }

    fn action(action: HeroCompanyAction) -> Self {
        Self {
            action: Some(action),
            ..default()
        }
    }
}

/// Who the local hero is to this company. Each class spawns a different set
/// of position nodes, so it is structural.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) enum OwnershipClass {
    NoHero,
    NoShares,
    Shareholder,
    Majority,
    Master,
    SoleMaster,
}

pub(super) fn ownership_class(
    company: &CompanyRecord,
    directory: &CompanyDirectory,
) -> OwnershipClass {
    let Some(person) = directory.local_person else {
        return OwnershipClass::NoHero;
    };
    let shares = company.shares_owned_by(person);
    if shares == 0 {
        OwnershipClass::NoShares
    } else if company.master == person && shares == COMPANY_TOTAL_SHARES {
        OwnershipClass::SoleMaster
    } else if company.master == person {
        OwnershipClass::Master
    } else if shares > COMPANY_TOTAL_SHARES / 2 {
        OwnershipClass::Majority
    } else {
        OwnershipClass::Shareholder
    }
}

/// The selected company's page inputs, borrowed for one spawn or bind pass.
#[derive(Clone, Copy)]
pub(super) struct CompanyView<'a> {
    pub company: &'a CompanyRecord,
    pub directory: &'a CompanyDirectory,
    pub feedback: &'a CompanyPolicyFeedback,
    pub editor: &'a TradeRouteEditorState,
}

impl<'a> CompanyView<'a> {
    pub(super) fn can_manage(&self) -> bool {
        self.directory.local_person == Some(self.company.master)
    }

    pub(super) fn ready_warehouse(&self) -> bool {
        self.company
            .sites
            .iter()
            .any(|site| site.kind == SettlementBuildingKind::StorageHall && site.workers > 0)
    }

    /// The route draft shown instead of the detail pane, if any.
    pub(super) fn draft(&self) -> Option<&'a TradeRouteDraft> {
        self.editor
            .draft
            .as_ref()
            .filter(|draft| draft.company == self.company.id)
    }

    /// Staffed Storage Halls the caravan editor can call home.
    pub(super) fn warehouses(&self) -> impl Iterator<Item = &'a CompanySiteRecord> + 'a {
        self.company
            .sites
            .iter()
            .filter(|site| site.kind == SettlementBuildingKind::StorageHall && site.workers > 0)
    }

    pub(super) fn settlement_name(&self, id: SettlementId) -> String {
        self.directory
            .settlements
            .iter()
            .find(|settlement| settlement.id == id)
            .map_or_else(
                || format!("Settlement #{}", id.0),
                |settlement| settlement.name.clone(),
            )
    }

    fn site(&self, id: BuildingId) -> Option<&'a CompanySiteRecord> {
        self.company.sites.iter().find(|site| site.id == id)
    }

    fn branch(&self, settlement: SettlementId) -> Option<&'a CompanyBranchRecord> {
        self.company
            .branches
            .iter()
            .find(|branch| branch.settlement_id == settlement)
    }

    fn route(&self, id: TradeRouteId) -> Option<&'a CompanyRouteRecord> {
        self.company.routes.iter().find(|route| route.id == id)
    }

    /// The current value for `key`, or `None` when the record it names is no
    /// longer in the snapshot (a structural rebuild is then already due).
    pub(super) fn value(&self, key: CompanyBound) -> Option<BoundValue> {
        let company = self.company;
        Some(match key {
            CompanyBound::Company(field) => return self.company_value(field),
            CompanyBound::Site(id, field) => return self.site_value(self.site(id)?, field),
            CompanyBound::Branch(settlement) => {
                let branch = self.branch(settlement)?;
                BoundValue::text(format!(
                    "{}  /  {} SITE{}  /  {} STORAGE HALL{}  /  {} OF {} BULK USED",
                    branch.settlement.to_uppercase(),
                    branch.sites,
                    if branch.sites == 1 { "" } else { "S" },
                    branch.storage_halls,
                    if branch.storage_halls == 1 { "" } else { "S" },
                    branch.used_bulk,
                    branch.bulk_capacity,
                ))
            }
            CompanyBound::Resource(settlement, good, field) => {
                let branch = self.branch(settlement)?;
                let (_, held, policy) = branch
                    .resources
                    .iter()
                    .find(|(candidate, ..)| *candidate == good)?;
                let unit_capacity = branch.bulk_capacity / good.bulk_per_unit().max(1);
                match field {
                    ResourceField::Line => BoundValue::text(format!(
                        "{}  /  {} HELD  /  RETAIN {}",
                        good.label().to_uppercase(),
                        held,
                        policy.retain_units,
                    )),
                    ResourceField::Policy => BoundValue::text(if policy.sell_excess {
                        "SELL EXCESS"
                    } else {
                        "HOLD ALL"
                    }),
                    ResourceField::Meter => BoundValue {
                        fill: Some(if unit_capacity == 0 {
                            0.0
                        } else {
                            policy.retain_units.min(unit_capacity) as f32 * 100.0
                                / unit_capacity as f32
                        }),
                        ..default()
                    },
                    ResourceField::ToggleLabel => BoundValue::text(if policy.sell_excess {
                        "HOLD ALL"
                    } else {
                        "SELL EXCESS"
                    }),
                    ResourceField::Toggle => BoundValue::action(HeroCompanyAction::SetSellExcess {
                        settlement,
                        good,
                        enabled: !policy.sell_excess,
                    }),
                    ResourceField::Step(step) => {
                        BoundValue::action(HeroCompanyAction::SetRetainUnits {
                            settlement,
                            good,
                            units: step.units(policy.retain_units, unit_capacity),
                        })
                    }
                }
            }
            CompanyBound::Route(id, field) => return route_value(self.route(id)?, field),
            CompanyBound::RouteStop(id, index, field) => {
                let route = self.route(id)?;
                let stop = route.stops.get(usize::from(index))?;
                match field {
                    StopField::Name => BoundValue::text(format!(
                        "STOP {}  /  {}",
                        u32::from(index) + 1,
                        stop.settlement_name.to_uppercase()
                    )),
                    StopField::Action => BoundValue::text(stop.action.label().to_uppercase()),
                    StopField::Card => {
                        let current =
                            route.current_stop == index && route.assigned_caravaner.is_some();
                        BoundValue {
                            highlight: Some(if current {
                                (Color::srgba(0.78, 0.42, 0.22, 0.14), EMBER)
                            } else {
                                (Color::srgba(0.96, 0.95, 0.91, 0.70), PLATE_RULE_SOFT)
                            }),
                            ..default()
                        }
                    }
                }
            }
            CompanyBound::RouteTrip(id, index) => {
                let route = self.route(id)?;
                let trip = route.trips.iter().rev().nth(usize::from(index))?;
                BoundValue::text(format!(
                    "DAY {}  /  {} stops  /  {} units  /  bought {}  /  freight {}  /  consigned {}  /  {:.1} world min",
                    trip.completed_day,
                    trip.stops_visited,
                    trip.units,
                    format_money(trip.source_purchase_cost),
                    format_money(trip.delivery_revenue),
                    format_money(trip.consigned_value),
                    trip.travel_world_seconds as f32 / 60.0,
                ))
            }
            CompanyBound::Ship(id) => {
                let (_, ship) = company
                    .fleet
                    .ships
                    .iter()
                    .find(|(candidate, _)| *candidate == id)?;
                BoundValue::text(format!(
                    "{}  ·  {} bulk  ·  {}",
                    ship.status.label(),
                    ship.kind.capacity(),
                    ship.assigned_route.map_or_else(
                        || "No route assigned".into(),
                        |route| format!("Route #{}", route.0)
                    )
                ))
            }
            CompanyBound::Order(id, field) => {
                let (_, order) = company
                    .fleet
                    .orders
                    .iter()
                    .find(|(candidate, _)| *candidate == id)?;
                match field {
                    OrderField::Title => {
                        let port_name = self
                            .directory
                            .settlements
                            .iter()
                            .find(|town| town.port.is_some_and(|port| port.port == order.port))
                            .map_or_else(
                                || format!("Port #{}", order.port.0),
                                |town| town.name.clone(),
                            );
                        BoundValue::text(format!(
                            "{} at {}  ·  {}%",
                            order.kind.label(),
                            port_name,
                            order.progress.min(1000) / 10
                        ))
                    }
                    OrderField::Status => {
                        let delivered = order
                            .kind
                            .materials()
                            .iter()
                            .enumerate()
                            .map(|(i, (good, n))| {
                                format!("{}/{} {}", order.delivered[i], n, good.label())
                            })
                            .collect::<Vec<_>>()
                            .join("  ·  ");
                        BoundValue::text(format!("{}\n{delivered}", order.status.label()))
                    }
                }
            }
            CompanyBound::Holder(index, field) => {
                let holder = company.holders.get(usize::from(index))?;
                match field {
                    HolderField::Link => BoundValue {
                        person: Some(holder.person),
                        ..default()
                    },
                    HolderField::Name => BoundValue::text(holder.name.clone()),
                    HolderField::Detail => BoundValue::text(format!(
                        "{} shares · {:.1}%{}  ›",
                        holder.shares,
                        f32::from(holder.shares) / 10.0,
                        if holder.person == company.master {
                            " · Company master"
                        } else {
                            ""
                        }
                    )),
                }
            }
            CompanyBound::Offer(index, field) => {
                let offer = company.offers.get(usize::from(index))?;
                match field {
                    PairField::Label => BoundValue::text(offer.seller_name.to_uppercase()),
                    PairField::Value => BoundValue::text(format!(
                        "{} shares at {} coin each  /  listed day {}  /  total {} coin",
                        offer.shares,
                        format_money(offer.unit_price),
                        offer.listed_day,
                        format_money(offer.unit_price.saturating_mul(u64::from(offer.shares))),
                    )),
                }
            }
            CompanyBound::Decision(index, field) => {
                let decision = company.decisions.iter().rev().nth(usize::from(index))?;
                match field {
                    PairField::Label => BoundValue::text(format!("DAY {}", decision.day)),
                    PairField::Value => BoundValue::text(format!(
                        "{} -> {}  /  {}",
                        decision.from.label(),
                        decision.to.label(),
                        decision.reason.label(),
                    )),
                }
            }
            CompanyBound::Portfolio(field) => portfolio_value(self.directory, field),
            CompanyBound::Editor(field) => return self.editor_value(self.draft()?, field),
            CompanyBound::EditorStop(index, field) => {
                let draft = self.draft()?;
                let stop = draft.stops.get(usize::from(index))?;
                match field {
                    EditorStopField::Name => {
                        BoundValue::text(self.settlement_name(stop.settlement).to_uppercase())
                    }
                    EditorStopField::Action => BoundValue::text(stop.action.label().to_uppercase()),
                    EditorStopField::Warning => {
                        let accepted = self.directory.settlements.iter().any(|settlement| {
                            settlement.id == stop.settlement && draft.accepts_settlement(settlement)
                        });
                        BoundValue::shown(
                            if draft.ship.is_some() {
                                "COMPLETED MARKET & SUITABLE PORT REQUIRED"
                            } else {
                                "LOCAL MOOT — BUILD A MARKETPLACE"
                            }
                            .into(),
                            !accepted,
                        )
                    }
                }
            }
        })
    }

    fn company_value(&self, field: CompanyField) -> Option<BoundValue> {
        let company = self.company;
        let account = &company.account;
        let coin = |value: u64| format!("{} coin", format_money(value));
        let expense = |value: u64| format!("−{} coin", format_money(value));
        Some(match field {
            CompanyField::Name | CompanyField::HistoryName => {
                BoundValue::text(company.name.clone())
            }
            CompanyField::Status => BoundValue::text(format!(
                "Company #{} · Founded Day {} · {}",
                company.id.0,
                company.founded_day,
                company.status(),
            )),
            CompanyField::Cash => BoundValue::text(coin(account.cash)),
            CompanyField::Revenue => BoundValue::text(coin(account.current_day.external_revenue)),
            CompanyField::Profit => BoundValue::text(signed_money(account.current_day.profit())),
            CompanyField::Liabilities => BoundValue::text(coin(
                account.wage_arrears.saturating_add(account.tax_arrears),
            )),
            CompanyField::Assets => BoundValue::text(coin(account.book_value)),
            CompanyField::Equity => BoundValue::text(coin(company.accounting_equity())),
            CompanyField::TodayRevenue => {
                BoundValue::text(coin(account.current_day.external_revenue))
            }
            CompanyField::TodayWages => BoundValue::text(expense(account.current_day.wage_expense)),
            CompanyField::TodayInputs => {
                BoundValue::text(expense(account.current_day.external_input_expense))
            }
            CompanyField::TodayFees => BoundValue::text(expense(
                account
                    .current_day
                    .market_fees
                    .saturating_add(account.current_day.delivery_fees),
            )),
            CompanyField::TodayTax => BoundValue::text(expense(account.current_day.profit_taxes)),
            CompanyField::TodayProfit => {
                let profit = account.current_day.profit();
                BoundValue::colored(
                    signed_money(profit),
                    if profit >= 0 {
                        Color::srgb(0.19, 0.36, 0.16)
                    } else {
                        EMBER
                    },
                )
            }
            CompanyField::DayLabel(which) => {
                let (label, day) = self.day_ledger(which);
                BoundValue::text(if day.day == u32::MAX {
                    label.to_string()
                } else {
                    format!("{label} / DAY {}", day.day)
                })
            }
            CompanyField::DayLine(which) => {
                let (_, day) = self.day_ledger(which);
                BoundValue::text(if day.day == u32::MAX {
                    "No completed trading record".to_string()
                } else {
                    format!(
                        "revenue {}  -  wages {}  -  outside inputs {}  -  market/delivery {}  -  tax {}  =  {}  /  dividends {}  /  capex {}",
                        format_money(day.external_revenue),
                        format_money(day.wage_expense),
                        format_money(day.external_input_expense),
                        format_money(day.market_fees.saturating_add(day.delivery_fees)),
                        format_money(day.profit_taxes),
                        signed_money(day.profit()),
                        format_money(day.owner_withdrawals),
                        format_money(day.capital_expenditures),
                    )
                })
            }
            CompanyField::DayMemoRow(which) => {
                let (_, day) = self.day_ledger(which);
                BoundValue::visible(day_has_memo(day))
            }
            CompanyField::DayMemo(which) => {
                let (_, day) = self.day_ledger(which);
                BoundValue::text(format!(
                    "{} supplier credits / {} buyer charges; eliminated from company profit",
                    format_money(day.internal_revenue),
                    format_money(day.internal_input_expense),
                ))
            }
            CompanyField::LifetimeCapital => BoundValue::text(format!(
                "{} contributed  /  {} capital spending  /  {} distributed",
                format_money(account.contributed_capital),
                format_money(account.capital_expenditures),
                format_money(account.owner_withdrawals),
            )),
            CompanyField::PositionShares => {
                let shares = self.local_shares();
                BoundValue::text(format!(
                    "{} / 1,000 shares · {:.1}%",
                    shares,
                    f32::from(shares) / 10.0
                ))
            }
            CompanyField::PositionInterest => BoundValue::text(format!(
                "Estimated book interest: {} coin",
                format_money(company.holding_book_interest(self.local_shares()))
            )),
            CompanyField::PositionRole => {
                BoundValue::text(match ownership_class(company, self.directory) {
                    OwnershipClass::Master | OwnershipClass::SoleMaster => {
                        "You are Company Master and control operating decisions."
                    }
                    OwnershipClass::Majority => {
                        "Majority holder; you may appoint the Company Master."
                    }
                    _ => "Shareholder; economic ownership without executive authority.",
                })
            }
            CompanyField::PositionNote => {
                BoundValue::text(match ownership_class(company, self.directory) {
                    OwnershipClass::NoHero => {
                        "Spawn or select your Hero to resolve personal holdings."
                    }
                    OwnershipClass::NoShares if company.offers.is_empty() => {
                        "You own no shares. No shareholder is currently offering stock."
                    }
                    OwnershipClass::NoShares => {
                        "You own no shares. Public offers are listed below; open COMPANY SETTINGS to trade."
                    }
                    _ => "Personal coin becomes company capital, not revenue or profit.",
                })
            }
            CompanyField::RoutesHint => BoundValue::text(if self.ready_warehouse() {
                "Choose up to eight towns and tell the caravan what to do at each stop."
            } else {
                "A staffed Storage Hall is required before this company can open a route."
            }),
            CompanyField::MasterName => BoundValue::text(company.master_name.clone()),
            CompanyField::OperatingPolicy => BoundValue::text(format!(
                "{}  /  {}  /  {} payroll reserve days",
                company.policy.strategy.label(),
                if company.policy.autopilot {
                    "autopilot"
                } else {
                    "manual"
                },
                company.policy.payroll_reserve_days,
            )),
            CompanyField::Dividends => BoundValue::text(if company.policy.automatic_dividends {
                format!(
                    "automatic after reserves  /  up to {} coin per day",
                    format_money(company.policy.max_daily_dividend)
                )
            } else {
                "retained until the Company Master distributes available profit".to_string()
            }),
            CompanyField::DividendCapacity => BoundValue::text(match company.capacity {
                None => "awaiting the first finance review".to_string(),
                Some(capacity) => format!(
                    "{} coin available now ({})  /  {} coin reserved  /  {}",
                    format_money(capacity.distributable),
                    if capacity.day == u32::MAX {
                        "not yet reviewed".to_string()
                    } else {
                        format!("day {}", capacity.day)
                    },
                    format_money(capacity.protected_reserves),
                    if capacity.last_paid_day == u32::MAX {
                        "never paid".to_string()
                    } else {
                        format!(
                            "last paid {} coin on day {}",
                            format_money(capacity.last_paid),
                            capacity.last_paid_day
                        )
                    },
                ),
            }),
            CompanyField::PositionDividend => {
                // Hidden until a snapshot exists; the amount picker itself
                // lives in COMPANY SETTINGS, this is the holder's exact take.
                let shares = self.local_shares();
                BoundValue::note(match (company.capacity, self.directory.local_person) {
                    (Some(capacity), Some(person)) if shares > 0 => {
                        if capacity.distributable == 0 {
                            format!(
                                "Nothing is distributable right now; {} coin is held as reserves.",
                                format_money(capacity.protected_reserves)
                            )
                        } else {
                            format!(
                                "A full distribution now ({} coin) would pay your {shares} shares {} coin.",
                                format_money(capacity.distributable),
                                format_money(local_dividend_take(
                                    capacity.distributable,
                                    &company.ownership,
                                    person,
                                )),
                            )
                        }
                    }
                    _ => String::new(),
                })
            }
            CompanyField::NoteFeedback => {
                let feedback = self.feedback;
                BoundValue::note(
                    if feedback.company == Some(company.id) && !feedback.message.is_empty() {
                        format!(
                            "{}: {}",
                            if feedback.success {
                                "UPDATED"
                            } else {
                                "NOT CHANGED"
                            },
                            feedback.message
                        )
                    } else {
                        String::new()
                    },
                )
            }
            CompanyField::NoteRoute => BoundValue::note(if self.editor.message.is_empty() {
                String::new()
            } else {
                format!(
                    "{}: {}",
                    if self.editor.success {
                        "ROUTE UPDATED"
                    } else {
                        "ROUTE NOT CHANGED"
                    },
                    self.editor.message
                )
            }),
        })
    }

    fn local_shares(&self) -> u16 {
        self.directory
            .local_person
            .map_or(0, |person| self.company.shares_owned_by(person))
    }

    fn day_ledger(&self, which: DayLedger) -> (&'static str, CompanyDayLedger) {
        match which {
            DayLedger::Today => ("TODAY", self.company.account.current_day),
            DayLedger::Previous => ("PREVIOUS DAY", self.company.account.previous_day),
        }
    }

    fn site_value(&self, site: &CompanySiteRecord, field: SiteField) -> Option<BoundValue> {
        Some(match field {
            SiteField::State => {
                BoundValue::text(format!("{} · {}", site.settlement, site.state.label()))
            }
            SiteField::Staff => BoundValue::text(format!(
                "Staff {} / {}",
                site.workers, site.enabled_positions
            )),
            SiteField::Settings => BoundValue {
                site: Some(site.entity),
                ..default()
            },
            SiteField::LedgerTitle => BoundValue::text(format!(
                "{} #{}  ·  {}",
                site.kind.label().to_uppercase(),
                site.id.0,
                site.settlement.to_uppercase()
            )),
            SiteField::LedgerState => BoundValue::text(site.state.label().to_uppercase()),
            SiteField::LedgerSummary => BoundValue::text(format!(
                "{} · {} · Staff {} / {} open / {} max · Today {}",
                site_flow(site),
                site_source(site),
                site.workers,
                site.enabled_positions,
                site.positions,
                signed_money(site.current_day.profit()),
            )),
            SiteField::LedgerStock => match (site.output, site.asking_price) {
                (Some(output), Some(price)) => BoundValue::shown(
                    format!(
                        "{} site stock {} / ask {} coin  /  wage-tax debt {} coin; public excess is set for the whole local branch above",
                        output.label(),
                        site.output_stock,
                        format_money(price),
                        format_money(site.wage_arrears.saturating_add(site.tax_arrears)),
                    ),
                    true,
                ),
                _ => BoundValue::shown(String::new(), false),
            },
            SiteField::LedgerInput => match site.input {
                Some(input) => BoundValue::shown(
                    format!(
                        "{} input / {} day{} cover / {} held / {} target",
                        input.label(),
                        site.input_coverage_days,
                        if site.input_coverage_days == 1 {
                            ""
                        } else {
                            "s"
                        },
                        site.input_stock,
                        site.input_target,
                    ),
                    true,
                ),
                None => BoundValue::shown(String::new(), false),
            },
        })
    }

    fn editor_value(&self, draft: &TradeRouteDraft, field: EditorField) -> Option<BoundValue> {
        let vessel = if draft.ship.is_some() {
            "SHIP"
        } else {
            "CARAVAN"
        };
        Some(match field {
            EditorField::Title => BoundValue::text(if let Some(route) = draft.route {
                format!("EDIT {vessel} ROUTE #{}", route.0)
            } else {
                format!("NEW {vessel} ROUTE")
            }),
            EditorField::Subtitle => BoundValue::text(format!(
                "{}  /  ORDERED MERCHANT TIMETABLE",
                self.company.name.to_uppercase()
            )),
            EditorField::SaveLabel => BoundValue::text(if draft.pending {
                "SAVING..."
            } else {
                "SAVE ROUTE"
            }),
            EditorField::Message => BoundValue::note(self.editor.message.clone()),
            EditorField::Home => BoundValue::text(if let Some((ship, kind)) = draft.ship {
                format!(
                    "{} #{} / {}",
                    kind.label(),
                    ship.0,
                    draft.stops.first().map_or_else(
                        || "Port unavailable".into(),
                        |stop| self.settlement_name(stop.settlement)
                    )
                )
            } else {
                self.warehouses()
                    .find(|site| site.id == draft.warehouse)
                    .map_or_else(
                        || format!("Storage Hall #{}", draft.warehouse.0),
                        |site| {
                            format!(
                                "Storage Hall #{} / {} / {} porter{}",
                                site.id.0,
                                site.settlement,
                                site.workers,
                                if site.workers == 1 { "" } else { "s" }
                            )
                        },
                    )
            }),
            EditorField::Cargo => BoundValue::text(format!(
                "CARGO  {}  /  TARGET {} UNIT{}",
                draft.good.label().to_uppercase(),
                draft.cargo_target,
                if draft.cargo_target == 1 { "" } else { "S" }
            )),
            EditorField::Capacity => BoundValue::text(format!(
                "Capacity: {} units of {} ({} bulk)",
                draft.cargo_capacity(),
                draft.good.label(),
                draft
                    .ship
                    .map_or(shared::economy::capacity::PORTER, |(_, kind)| kind
                        .capacity())
            )),
            EditorField::Prices => BoundValue::text(format!(
                "BUY CEILING  {} COIN  /  SALE FLOOR  {} COIN  /  {}",
                format_money(draft.maximum_purchase_price),
                format_money(draft.minimum_destination_price),
                if draft.automatic {
                    "REPEAT CONTINUOUSLY"
                } else {
                    "ONE CIRCUIT ON COMMAND"
                }
            )),
            EditorField::ToggleLabel => BoundValue::text(if draft.automatic {
                "MAKE ONE-CIRCUIT"
            } else {
                "REPEAT ROUTE"
            }),
            EditorField::Warning => {
                let storage_settlements: Vec<_> = self
                    .company
                    .sites
                    .iter()
                    .filter(|site| site.kind == SettlementBuildingKind::StorageHall)
                    .map(|site| site.settlement_id)
                    .collect();
                let has_invalid_private_stop = draft.stops.iter().any(|stop| {
                    matches!(
                        stop.action,
                        shared::components::TradeRouteStopAction::Load
                            | shared::components::TradeRouteStopAction::Unload
                    ) && !storage_settlements.contains(&stop.settlement)
                });
                let repeats_town = draft
                    .stops
                    .windows(2)
                    .any(|pair| pair[0].settlement == pair[1].settlement);
                BoundValue::note(if has_invalid_private_stop {
                    "Load and Unload require this company to own a Storage Hall in that town. Use Buy or Sell for a public market stop.".into()
                } else if repeats_town {
                    "The same town cannot appear in two consecutive stops. Returning to the home town as the final stop is allowed.".into()
                } else {
                    String::new()
                })
            }
        })
    }
}

fn day_has_memo(day: CompanyDayLedger) -> bool {
    day.internal_revenue > 0 || day.internal_input_expense > 0
}

pub(super) fn site_flow(site: &CompanySiteRecord) -> String {
    match (site.input, site.output) {
        (Some(input), Some(output)) => format!("{} -> {}", input.label(), output.label()),
        (None, Some(output)) => format!("produces {}", output.label()),
        _ => "service site".to_string(),
    }
}

fn site_source(site: &CompanySiteRecord) -> String {
    site.sourcing.map_or_else(
        || "public/local sourcing".to_string(),
        |sourcing| {
            let label = match sourcing {
                BusinessSourcingMode::PreferOwned => "company first",
                BusinessSourcingMode::CheapestAvailable => "best value",
                BusinessSourcingMode::OwnedOnly => "company only",
            };
            format!(
                "{}{}",
                label,
                site.preferred_supplier
                    .map_or_else(String::new, |supplier| format!(
                        " from site #{}",
                        supplier.0
                    ))
            )
        },
    )
}

fn route_value(route: &CompanyRouteRecord, field: RouteField) -> Option<BoundValue> {
    Some(match field {
        RouteField::Title => BoundValue::text(format!(
            "{} ROUTE #{}  /  {}",
            if route.ship.is_some() {
                "SHIP"
            } else {
                "CARAVAN"
            },
            route.id.0,
            route.good.label().to_uppercase()
        )),
        RouteField::Subtitle => BoundValue::text(format!(
            "{}  /  HOME {}",
            route.ship.map_or_else(
                || route.mode.label().to_uppercase(),
                |(id, kind)| format!("{} #{}", kind.label().to_uppercase(), id.0)
            ),
            route.warehouse_name.to_uppercase()
        )),
        RouteField::Status => BoundValue::colored(
            if route.ship.is_some() && route.status == TradeRouteStatus::WaitingForPorter {
                "WAITING FOR SAILOR".into()
            } else if route.ship.is_some() && route.status == TradeRouteStatus::Returning {
                "RETURNING TO HOME PORT".into()
            } else {
                route.status.label().to_uppercase()
            },
            if route.status == TradeRouteStatus::Mothballed {
                EMBER
            } else {
                INK_MUTED
            },
        ),
        RouteField::Cargo => BoundValue::text(format!(
            "CARGO  {} / {}",
            route.cargo_onboard, route.cargo_target
        )),
        RouteField::Caravaner => BoundValue::text(format!(
            "{}  {}",
            if route.ship.is_some() {
                "SAILOR"
            } else {
                "CARAVANER"
            },
            route
                .assigned_caravaner
                .as_deref()
                .unwrap_or("not assigned")
        )),
        RouteField::Trips => BoundValue::text(format!("TRIPS  {}", route.completed_trips)),
        RouteField::Units => BoundValue::text(format!("UNITS MOVED  {}", route.lifetime_units)),
        RouteField::Service => BoundValue::text(if route.autonomous_management {
            "SERVICE  MASTER-REVIEWED TRIAL"
        } else if route.automatic {
            "SERVICE  REPEAT"
        } else {
            "SERVICE  ONE CIRCUIT"
        }),
        RouteField::Summary => BoundValue::text(match route.mode {
            TradeRouteMode::ContractCarrier => format!(
                "Buyer-funded cargo  /  {} coin freight earned  /  purchase ceiling {} coin. Stops are fixed by the public contract.",
                format_money(route.lifetime_delivery_revenue),
                format_money(route.maximum_purchase_price),
            ),
            TradeRouteMode::Merchant => format!(
                "Buy at or below {} coin  /  list sales at or above {} coin  /  {} coin spent  /  {} coin consigned at asking value.{} Consignment becomes revenue only when a real buyer purchases it.",
                format_money(route.maximum_purchase_price),
                format_money(route.minimum_destination_price),
                format_money(route.lifetime_purchase_cost),
                format_money(route.lifetime_consigned_value),
                if route.autonomous_management {
                    format!(
                        " Company Master forecast: {} coin/trip at {}% confidence; the route pauses when cargo repeatedly remains unsold.",
                        if route.expected_trip_profit >= 0 {
                            format_money(route.expected_trip_profit as u64)
                        } else {
                            format!(
                                "-{}",
                                format_money(route.expected_trip_profit.unsigned_abs())
                            )
                        },
                        route.decision_confidence,
                    )
                } else {
                    String::new()
                },
            ),
        }),
    })
}

/// The portfolio strip depends on the whole directory, not on the selection.
pub(super) fn portfolio_value(directory: &CompanyDirectory, field: PortfolioField) -> BoundValue {
    let Some(person) = directory.local_person else {
        return BoundValue::text(String::new());
    };
    match field {
        PortfolioField::Wallet => BoundValue::text(directory.local_wallet.map_or_else(
            || "Not in range".to_string(),
            |wallet| format!("{} coin", format_money(wallet)),
        )),
        PortfolioField::Holdings => {
            let holdings = directory
                .records
                .iter()
                .filter(|company| company.shares_owned_by(person) > 0)
                .count();
            BoundValue::text(format!(
                "{} firm{}",
                holdings,
                if holdings == 1 { "" } else { "s" }
            ))
        }
        PortfolioField::Interest => {
            let interest = directory.records.iter().fold(0u64, |total, company| {
                let shares = company.shares_owned_by(person);
                if shares > 0 {
                    total.saturating_add(company.holding_book_interest(shares))
                } else {
                    total
                }
            });
            BoundValue::text(format!("{} coin", format_money(interest)))
        }
        PortfolioField::Master => {
            let mastered = directory
                .records
                .iter()
                .filter(|company| company.master == person)
                .count();
            BoundValue::text(format!(
                "{} firm{}",
                mastered,
                if mastered == 1 { "" } else { "s" }
            ))
        }
    }
}

/// Spawn a bound text node whose initial content comes from the same value
/// function the bind pass uses. `font` and `color` are the resting style; a
/// key with its own colour or display overrides them now and on every bind.
pub(super) fn bound_text<'a>(
    parent: &'a mut ChildSpawnerCommands<'_>,
    view: &CompanyView<'_>,
    key: CompanyBound,
    font: TextFont,
    color: Color,
) -> EntityCommands<'a> {
    let value = view.value(key).unwrap_or_default();
    let mut entity = parent.spawn((
        key,
        Text::new(value.text.unwrap_or_default()),
        font,
        TextColor(value.color.unwrap_or(color)),
    ));
    if let Some(display) = value.display {
        entity.insert(Node {
            display,
            ..default()
        });
    }
    entity
}

/// One mutable view of every component a key may drive. `Option` so a text
/// node, a meter lane, a stop card and a payload button share one query.
pub(super) type BoundTargets = (
    &'static CompanyBound,
    Option<&'static mut Text>,
    Option<&'static mut TextColor>,
    Option<&'static mut Node>,
    Option<&'static mut BackgroundColor>,
    Option<&'static mut BorderColor>,
    Option<&'static mut CompanyBranchPolicyButton>,
    Option<&'static mut CompanyManagementButton>,
    Option<&'static mut CompanyHistoryButton>,
    Option<&'static mut PersonLink>,
);

/// Write `value` into whichever of the entity's components it names, only
/// where the stored value differs, so unchanged nodes are never marked changed.
pub(super) fn apply_bound(
    value: BoundValue,
    (
        _,
        text,
        text_color,
        node,
        background,
        border,
        policy,
        management,
        history,
        link,
    ): <BoundTargets as bevy::ecs::query::QueryData>::Item<'_, '_>,
) {
    if let Some(next) = value.text {
        if let Some(mut history) = history {
            if history.name != next {
                history.name = next.clone();
            }
        }
        if let Some(mut text) = text {
            if text.0 != next {
                text.0 = next;
            }
        }
    }
    if let (Some(next), Some(mut color)) = (value.color, text_color) {
        if color.0 != next {
            color.0 = next;
        }
    }
    if let Some(mut node) = node {
        if let Some(display) = value.display {
            if node.display != display {
                node.display = display;
            }
        }
        if let Some(percent) = value.fill {
            let width = Val::Percent(percent);
            if node.width != width {
                node.width = width;
            }
        }
    }
    if let Some((next_background, next_border)) = value.highlight {
        if let Some(mut background) = background {
            if background.0 != next_background {
                background.0 = next_background;
            }
        }
        if let Some(mut border) = border {
            let next = BorderColor::all(next_border);
            if *border != next {
                *border = next;
            }
        }
    }
    if let (Some(action), Some(mut button)) = (value.action, policy) {
        if button.action != action {
            button.action = action;
        }
    }
    if let (Some(site), Some(mut button)) = (value.site, management) {
        let target = BusinessManagementSelection::Site(site);
        if button.target != target {
            button.target = target;
        }
    }
    if let (Some(person), Some(mut link)) = (value.person, link) {
        if link.0 != person {
            link.0 = person;
        }
    }
}

/// Hash of every id and gating bit the detail spawners branch on. Values
/// (cash, stock, cargo, names, prices) are deliberately absent: they bind.
pub(super) fn company_structure_key(
    company: Option<&CompanyRecord>,
    directory: &CompanyDirectory,
    editor: &TradeRouteEditorState,
) -> u64 {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let Some(company) = company else {
        0u8.hash(&mut hasher);
        return hasher.finish();
    };
    1u8.hash(&mut hasher);
    company.id.hash(&mut hasher);
    company.master.hash(&mut hasher);
    directory.local_person.is_some().hash(&mut hasher);
    let can_manage = directory.local_person == Some(company.master);
    can_manage.hash(&mut hasher);
    // Storage Halls decide both the route editor's home choices and the
    // detail pane's NEW CARAVAN ROUTE gate.
    let warehouses = company
        .sites
        .iter()
        .filter(|site| site.kind == SettlementBuildingKind::StorageHall && site.workers > 0)
        .count();
    warehouses.hash(&mut hasher);

    if let Some(draft) = editor
        .draft
        .as_ref()
        .filter(|draft| draft.company == company.id)
    {
        2u8.hash(&mut hasher);
        draft.route.hash(&mut hasher);
        draft.ship.hash(&mut hasher);
        draft.stops.len().hash(&mut hasher);
        return hasher.finish();
    }

    3u8.hash(&mut hasher);
    ownership_class(company, directory).hash(&mut hasher);
    (company.account.current_day.day != u32::MAX).hash(&mut hasher);
    (company.account.previous_day.day != u32::MAX).hash(&mut hasher);
    (directory.settlements.len() >= 2).hash(&mut hasher);

    company.sites.len().hash(&mut hasher);
    for site in &company.sites {
        site.id.hash(&mut hasher);
        site.kind.hash(&mut hasher);
        site.settlement_id.hash(&mut hasher);
    }
    company.branches.len().hash(&mut hasher);
    for branch in &company.branches {
        branch.settlement_id.hash(&mut hasher);
        branch.resources.len().hash(&mut hasher);
        for (good, ..) in &branch.resources {
            good.hash(&mut hasher);
        }
    }
    company.routes.len().hash(&mut hasher);
    for route in &company.routes {
        route.id.hash(&mut hasher);
        route.mode.hash(&mut hasher);
        route.ship.is_some().hash(&mut hasher);
        route.assigned_caravaner.is_none().hash(&mut hasher);
        (route.status == TradeRouteStatus::Idle).hash(&mut hasher);
        (route.status == TradeRouteStatus::Mothballed).hash(&mut hasher);
        route.automatic.hash(&mut hasher);
        route.stops.len().hash(&mut hasher);
        route.trips.len().min(3).hash(&mut hasher);
    }
    company.fleet.ships.len().hash(&mut hasher);
    for (id, ship) in &company.fleet.ships {
        id.hash(&mut hasher);
        ship.kind.hash(&mut hasher);
        ship.assigned_route.hash(&mut hasher);
        // EDIT TIMETABLE / ASSIGN ROUTE gates are route-side bits; hashing them
        // here as well keeps the fleet rows honest if the route list changes.
        for route in &company.routes {
            let at_home = route.assigned_caravaner.is_none()
                && matches!(
                    route.status,
                    TradeRouteStatus::Idle | TradeRouteStatus::Mothballed
                );
            (at_home && (ship.assigned_route == Some(route.id) || route.ship.is_some()))
                .hash(&mut hasher);
        }
    }
    for (id, order) in &company.fleet.orders {
        if matches!(
            order.status,
            ShipOrderStatus::Completed | ShipOrderStatus::Cancelled
        ) {
            continue;
        }
        id.hash(&mut hasher);
    }
    for town in &directory.settlements {
        if let Some(port) = town.port.filter(|port| port.built) {
            town.id.hash(&mut hasher);
            port.port.hash(&mut hasher);
            port.maximum_ship.hash(&mut hasher);
        }
    }
    company.holders.len().hash(&mut hasher);
    company.offers.len().hash(&mut hasher);
    company.decisions.len().min(8).hash(&mut hasher);
    hasher.finish()
}
