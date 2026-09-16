//! Pure site/company presentation and authoritative action addressing.

use super::*;

mod company;

// --- model ------------------------------------------------------------------

/// What pressing a control does. Orders go to the server; draft steps edit the
/// local share-offer draft.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ControlPress {
    Order(HeroBusinessAction),
    Company(CompanyId, HeroCompanyAction),
    Draft(ShareDraftAction),
    Person(PersonId),
}

pub(super) struct ControlModel {
    pub(super) id: String,
    pub(super) label: String,
    pub(super) press: ControlPress,
    pub(super) selected: bool,
}

pub(super) struct RowModel {
    pub(super) id: String,
    pub(super) label: String,
    pub(super) value: String,
    pub(super) controls: Vec<ControlModel>,
}

pub(super) struct MeterModel {
    pub(super) id: String,
    pub(super) title: String,
    pub(super) summary: String,
    /// Lane widths in percent of the track.
    pub(super) lanes: [f32; 2],
}

pub(super) struct WorkerModel {
    pub(super) person: PersonId,
    pub(super) name: String,
}

pub(super) enum Block {
    Scope(BusinessManagementPage),
    Section(String),
    Row(RowModel),
    Meter(MeterModel),
}

/// The whole panel as data. See the module docs for how it is rendered.
pub(super) struct ControlsModel {
    pub(super) page: BusinessManagementPage,
    pub(super) company_available: bool,
    pub(super) site_available: bool,
    pub(super) title: String,
    pub(super) subtitle: String,
    pub(super) blocks: Vec<Block>,
    pub(super) feedback: Option<(String, bool)>,
}

impl ControlsModel {
    /// The id sequence; equal keys mean the spawned tree can be reused.
    pub(super) fn structure_key(&self) -> String {
        let mut key = String::new();
        for block in &self.blocks {
            match block {
                Block::Scope(page) => key.push_str(match page {
                    BusinessManagementPage::Site => "scope.site",
                    BusinessManagementPage::Company => "scope.company",
                }),
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

pub(super) fn order(
    id: impl Into<String>,
    label: impl Into<String>,
    action: HeroBusinessAction,
) -> ControlModel {
    ControlModel {
        id: id.into(),
        label: label.into(),
        press: ControlPress::Order(action),
        selected: false,
    }
}

pub(super) fn company_order(
    company: CompanyId,
    id: impl Into<String>,
    label: impl Into<String>,
    action: HeroCompanyAction,
) -> ControlModel {
    ControlModel {
        id: id.into(),
        label: label.into(),
        press: ControlPress::Company(company, action),
        selected: false,
    }
}

pub(super) fn company_choice(
    company: CompanyId,
    id: impl Into<String>,
    label: impl Into<String>,
    action: HeroCompanyAction,
    selected: bool,
) -> ControlModel {
    ControlModel {
        selected,
        ..company_order(company, id, label, action)
    }
}

pub(super) fn choice(
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

pub(super) fn row(
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

pub(super) fn meter(
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

pub(super) fn signed_coin(pennies: i64) -> String {
    format!(
        "{}{} coin",
        if pennies < 0 { "-" } else { "+" },
        format_money(pennies.unsigned_abs())
    )
}

pub(super) struct CompanyView<'a> {
    pub(super) id: CompanyId,
    pub(super) company: &'a Company,
    pub(super) ownership: &'a CompanyOwnership,
    pub(super) leadership: &'a CompanyLeadership,
    pub(super) account: &'a CompanyAccount,
    pub(super) policy: &'a CompanyManagementPolicy,
    pub(super) decisions: &'a CompanyDecisionHistory,
    pub(super) share_market: &'a CompanyShareMarket,
}

pub(super) struct SiteView<'a> {
    pub(super) building: &'a SettlementBuilding,
    pub(super) building_id: BuildingId,
    pub(super) company: Option<CompanyId>,
    pub(super) owner: Option<PersonId>,
    pub(super) account: &'a BusinessAccount,
    pub(super) management: &'a BusinessManagementPolicy,
    pub(super) wage: &'a BusinessWagePolicy,
    pub(super) sale: &'a BusinessSalePolicy,
    pub(super) staffing: Option<&'a BusinessStaffingPolicy>,
    pub(super) procurement: &'a BusinessProcurementPolicy,
    pub(super) supply: &'a BusinessSupplyPolicy,
    pub(super) inventory: &'a GoodsInventory,
    pub(super) tavern_service: Option<&'a TavernService>,
}

pub(super) struct ModelInputs<'a> {
    pub(super) site: Option<SiteView<'a>>,
    pub(super) workers: &'a [WorkerModel],
    pub(super) company: Option<CompanyView<'a>>,
    pub(super) local_person: Option<PersonId>,
    pub(super) share_draft: &'a ShareOrderDraft,
    pub(super) page: BusinessManagementPage,
    pub(super) feedback: &'a BusinessFeedback,
    pub(super) name_of: &'a dyn Fn(PersonId) -> String,
}

#[allow(clippy::too_many_lines)]
pub(super) fn controls_model(inputs: &ModelInputs<'_>) -> ControlsModel {
    let ModelInputs {
        site,
        workers,
        company,
        local_person,
        share_draft,
        page,
        feedback,
        name_of,
    } = inputs;
    let site_label = site.as_ref().map(|site| {
        format!(
            "{} #{}",
            site.building.kind.label().to_uppercase(),
            site.building_id.0
        )
    });
    let page = match (site.is_some(), company.is_some()) {
        (false, true) => BusinessManagementPage::Company,
        (_, false) => BusinessManagementPage::Site,
        _ => *page,
    };
    let title = match (page, company.as_ref()) {
        (BusinessManagementPage::Company, Some(company)) => company.company.name.clone(),
        _ => site_label.clone().unwrap_or_else(|| "WORKPLACE".into()),
    };
    let subtitle = match (site.as_ref(), company.as_ref(), page) {
        (Some(site), Some(company), BusinessManagementPage::Site) => {
            format!("{}  ·  {}", company.company.name, site.building.settlement)
        }
        (Some(site), Some(_), BusinessManagementPage::Company) => format!(
            "COMPANY MANAGEMENT  ·  Selected site: {} in {}",
            site_label.as_deref().unwrap_or_default(),
            site.building.settlement
        ),
        (None, Some(_), _) => {
            "COMPANY MANAGEMENT  ·  Strategy, treasury and ownership across all sites".into()
        }
        (Some(site), None, _) if site.company.is_some() => {
            format!(
                "COMPANY RECORD UNAVAILABLE  ·  {}",
                site.building.settlement
            )
        }
        (Some(site), None, _) => format!("INDEPENDENT SITE  ·  {}", site.building.settlement),
        _ => String::new(),
    };
    let can_manage = local_person
        .filter(|person| person.is_assigned())
        .is_some_and(|person| {
            if let Some(company) = company.as_ref() {
                company.leadership.can_manage(person)
            } else {
                // Missing company replication is not independent ownership. Keep
                // the site inspectable while withholding unproven operating rights.
                site.as_ref()
                    .is_some_and(|site| site.company.is_none() && site.owner == Some(person))
            }
        });
    let mut blocks = Vec::new();

    if let Some(company) = company.as_ref() {
        company::append_company_controls(
            &mut blocks,
            company,
            local_person,
            share_draft,
            *name_of,
            can_manage,
        );
    }

    let Some(site) = site.as_ref() else {
        return ControlsModel {
            page,
            company_available: company.is_some(),
            site_available: false,
            title,
            subtitle,
            blocks,
            feedback: (!feedback.message.is_empty())
                .then(|| (feedback.message.clone(), feedback.success)),
        };
    };
    let building = site.building;
    let site_label = site_label.as_deref().unwrap_or_default();
    blocks.push(Block::Scope(BusinessManagementPage::Site));
    blocks.push(Block::Section("THIS WORKPLACE".into()));
    blocks.push(row(
        "site.scope", "SITE OPERATIONS",
        format!("{site_label} in {}. Staffing, wages, prices and input orders below affect this workplace only.", building.settlement), vec![],
    ));
    blocks.push(row(
        "site.today",
        "SITE RESULT TODAY",
        format!(
            "{}  ·  external sales {}  ·  internal transfers {}",
            signed_coin(site.account.current_day.profit()),
            format_money(site.account.current_day.gross_revenue),
            format_money(site.account.current_day.internal_revenue)
        ),
        vec![],
    ));
    blocks.push(Block::Section("EMPLOYEES AT THIS SITE".into()));
    if workers.is_empty() {
        blocks.push(row(
            "workers.empty",
            "EMPLOYEES",
            "No observed employees at this site",
            vec![],
        ));
    } else {
        blocks.push(row(
            "workers.list",
            "EMPLOYEES",
            format!(
                "{} observed employees · select a name to view their character",
                workers.len()
            ),
            workers
                .iter()
                .map(|worker| ControlModel {
                    id: format!("worker.{}", worker.person.0),
                    label: worker.name.clone(),
                    press: ControlPress::Person(worker.person),
                    selected: false,
                })
                .collect(),
        ));
    }
    if can_manage {
        let management = site.management;
        blocks.push(automation_row(
            "autopilot",
            "SITE AUTOPILOT",
            management.autopilot,
        ));
        blocks.push(strategy_row("strategy", management.strategy));
        blocks.push(Block::Section("STAFFING & PAY".into()));
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
            "STAFFING TARGET",
            format!(
                "{} employed  ·  {enabled_positions} positions enabled (maximum {})\nChoose 0 to pause hiring; existing workers finish committed deliveries before leaving.",
                building.workers.len(), building.kind.positions()
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
        blocks.push(Block::Section("LOCAL SALES".into()));
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
    } else {
        blocks.push(row(
            "authority",
            "OPERATING AUTHORITY",
            "Only the appointed Company Master can change wages, prices, sourcing, strategy or dividends. Ownership, shares and the ledger are on the company page.",
            vec![],
        ));
    }

    ControlsModel {
        page,
        company_available: company.is_some(),
        site_available: true,
        title,
        subtitle,
        blocks,
        feedback: (!feedback.message.is_empty())
            .then(|| (feedback.message.clone(), feedback.success)),
    }
}

pub(super) fn automation_row(id: &str, label: &str, automatic: bool) -> Block {
    row(
        id,
        label,
        if automatic {
            "Automatic management"
        } else {
            "Manual decisions retained"
        },
        vec![order(
            format!("{id}.toggle"),
            if automatic { "MANUAL" } else { "AUTOMATIC" },
            HeroBusinessAction::SetAutopilot(!automatic),
        )],
    )
}

pub(super) fn strategy_row(id: &str, strategy: BusinessStrategy) -> Block {
    row(
        id,
        "SITE STRATEGY",
        strategy.label(),
        BusinessStrategy::ALL
            .into_iter()
            .enumerate()
            .map(|(index, candidate)| {
                choice(
                    format!("{id}.{index}"),
                    candidate.label().to_uppercase(),
                    HeroBusinessAction::SetStrategy(candidate),
                    candidate == strategy,
                )
            })
            .collect(),
    )
}

pub(super) fn dividend_row(
    id: &str,
    label: &str,
    company: &CompanyView<'_>,
    can_manage: bool,
) -> Block {
    let automatic = company.policy.automatic_dividends;
    let controls = if can_manage {
        vec![
            company_choice(
                company.id,
                format!("{id}.auto"),
                "AUTO DIVIDEND",
                HeroCompanyAction::SetAutomaticDividends(!automatic),
                automatic,
            ),
            company_order(
                company.id,
                format!("{id}.distribute"),
                "DISTRIBUTE AVAILABLE",
                HeroCompanyAction::DistributeAvailableProfit,
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

pub(super) fn input_coverage_meter(
    good: Good,
    held: u32,
    target: u32,
    reorder_below: u32,
    days: u8,
) -> Block {
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

pub(super) fn share_draft_summary(
    draft: &ShareOrderDraft,
    own_shares: u16,
    listed: &str,
) -> String {
    format!(
        "You own {own_shares} / 1,000 shares.  Draft: {} shares at {} coin each.  {listed}",
        draft.shares,
        format_money(draft.unit_price),
    )
}

pub(super) fn output_good(kind: SettlementBuildingKind) -> Option<Good> {
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

pub(super) fn coverage_label(days: u8) -> String {
    match days {
        0 => "OFF".to_string(),
        1 => "1 DAY".to_string(),
        days => format!("{days} DAYS"),
    }
}

pub(super) fn sourcing_short_label(mode: BusinessSourcingMode) -> &'static str {
    match mode {
        BusinessSourcingMode::PreferOwned => "COMPANY FIRST",
        BusinessSourcingMode::CheapestAvailable => "BEST VALUE",
        BusinessSourcingMode::OwnedOnly => "COMPANY ONLY",
    }
}

pub(super) fn sourcing_explanation(mode: BusinessSourcingMode) -> &'static str {
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
