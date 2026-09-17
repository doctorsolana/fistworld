//! Pure site/company presentation and authoritative action addressing.

use std::hash::{DefaultHasher, Hash, Hasher};

use super::*;

mod company;

// --- model ------------------------------------------------------------------

/// What pressing a control does. Orders go to the server; draft steps edit the
/// local share-offer, dividend-amount or capital-contribution draft.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum ControlPress {
    Order(HeroBusinessAction),
    Company(CompanyId, HeroCompanyAction),
    Draft(ShareDraftAction),
    DividendDraft(DividendDraftAction),
    CapitalDraft(CapitalDraftAction),
    Person(PersonId),
    /// A fixed slot with no occupant this frame. The button is spawned hidden
    /// and inert but keeps its entity and payload component, so a later
    /// occupant binds into it instead of respawning the page.
    Vacant(VacantSlot),
}

/// Which payload component a vacant slot's button carries, so the bind pass
/// can fill it later without `Commands`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum VacantSlot {
    /// A `PersonLink` slot (worker chips).
    Person,
    /// An `Action` slot (public share-offer buy buttons).
    Action,
}

pub(super) struct ControlModel {
    /// The readable id; production only needs its hash below, tests assert on it.
    #[cfg(test)]
    pub(super) id: String,
    /// `BoundId::of(&id)`, hashed once here so the structure key and the
    /// bind pass compare `u64`s instead of re-hashing the string per frame.
    pub(super) bound: BoundId,
    pub(super) label: String,
    pub(super) press: ControlPress,
    pub(super) selected: bool,
}

impl ControlModel {
    pub(super) fn new(
        id: impl Into<String>,
        label: impl Into<String>,
        press: ControlPress,
    ) -> Self {
        let id = id.into();
        Self {
            bound: BoundId::of(&id),
            #[cfg(test)]
            id,
            label: label.into(),
            press,
            selected: false,
        }
    }

    /// Vacant slots stay in the tree (`Display::None`) so the structure key
    /// and the entity set do not depend on who currently fills them.
    pub(super) fn visible(&self) -> bool {
        !matches!(self.press, ControlPress::Vacant(_))
    }

    pub(super) fn vacant(id: impl Into<String>, slot: VacantSlot) -> Self {
        Self::new(id, String::new(), ControlPress::Vacant(slot))
    }
}

/// A bound node's id, hashed once at spawn so the per-frame bind pass keys
/// its scratch map by `u64` instead of hashing strings or allocating.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(super) struct BoundId(u64);

impl BoundId {
    pub(super) fn of(id: &str) -> Self {
        let mut hasher = DefaultHasher::new();
        id.hash(&mut hasher);
        Self(hasher.finish())
    }
}

/// Fixed public-offer buy slots per seller: BUY 1, BUY 10 and BUY ALL, with
/// duplicates (an offer of 10 shares) left vacant rather than dropped.
pub(super) const BUY_SLOTS_PER_OFFER: usize = 3;

pub(super) struct RowModel {
    #[cfg(test)]
    pub(super) id: String,
    pub(super) bound: BoundId,
    pub(super) label: String,
    pub(super) value: String,
    pub(super) controls: Vec<ControlModel>,
}

pub(super) struct MeterModel {
    #[cfg(test)]
    pub(super) id: String,
    pub(super) bound: BoundId,
    pub(super) title: String,
    pub(super) summary: String,
    /// Lane widths in percent of the track.
    pub(super) lanes: [f32; 2],
}

/// Collect the observed employees of `site` into `scratch`: one entry per
/// durable person (a person in replication handover can briefly have two
/// bodies), sorted by id so a departing worker never shifts the chips that
/// remain. The slice is reused across frames by the caller.
pub(super) fn observed_workers(
    scratch: &mut Vec<PersonId>,
    site: BuildingId,
    people: impl Iterator<Item = (PersonId, Option<BuildingId>)>,
) {
    scratch.clear();
    scratch
        .extend(people.filter_map(|(person, employer)| (employer == Some(site)).then_some(person)));
    scratch.sort_unstable();
    scratch.dedup();
}

pub(super) enum Block {
    Scope(BusinessManagementPage),
    Section(&'static str),
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
    /// A hash of the id sequence (scopes, section labels, and the pre-hashed
    /// row, control and meter ids); equal keys mean the spawned tree can be
    /// reused. Values, labels, selection and whether a fixed slot is occupied
    /// are not part of it, so the key never allocates, hashes no id strings
    /// and never changes on an economic tick, a worker walking out of
    /// replication or a partial share sale.
    pub(super) fn structure_key(&self) -> u64 {
        let mut hasher = DefaultHasher::new();
        for block in &self.blocks {
            match block {
                Block::Scope(page) => {
                    0u8.hash(&mut hasher);
                    page.index().hash(&mut hasher);
                }
                Block::Section(label) => {
                    1u8.hash(&mut hasher);
                    label.hash(&mut hasher);
                }
                Block::Row(row) => {
                    2u8.hash(&mut hasher);
                    row.bound.hash(&mut hasher);
                    for control in &row.controls {
                        3u8.hash(&mut hasher);
                        control.bound.hash(&mut hasher);
                    }
                }
                Block::Meter(meter) => {
                    4u8.hash(&mut hasher);
                    meter.bound.hash(&mut hasher);
                }
            }
        }
        hasher.finish()
    }
}

pub(super) fn order(
    id: impl Into<String>,
    label: impl Into<String>,
    action: HeroBusinessAction,
) -> ControlModel {
    ControlModel::new(id, label, ControlPress::Order(action))
}

pub(super) fn company_order(
    company: CompanyId,
    id: impl Into<String>,
    label: impl Into<String>,
    action: HeroCompanyAction,
) -> ControlModel {
    ControlModel::new(id, label, ControlPress::Company(company, action))
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

/// The automatic dividend shares a Company Master can choose: `0` retains
/// profits, the rest pay that percentage of the retained profit above the
/// working-capital runway every day. The server accepts any share up to
/// `MAX_AUTOMATIC_PAYOUT_PERCENT`; an off-preset value (a future NPC default
/// viewed by its Master, say) binds into the fixed custom chip slot.
pub(super) const AUTOMATIC_DIVIDEND_PRESETS: [u8; 4] = [0, 10, 25, 50];

pub(super) fn automatic_dividend_label(percent: u8) -> String {
    if percent == 0 {
        "RETAIN".to_string()
    } else {
        format!("{percent}%")
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
    let id = id.into();
    Block::Row(RowModel {
        bound: BoundId::of(&id),
        #[cfg(test)]
        id,
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
    let id = id.into();
    Block::Meter(MeterModel {
        bound: BoundId::of(&id),
        #[cfg(test)]
        id,
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
    /// The server's latest dividend headroom snapshot; `None` until the
    /// finance pass has published one for this company.
    pub(super) capacity: Option<CompanyDividendCapacity>,
}

impl CompanyView<'_> {
    /// What a distribution could pay right now according to the last
    /// published snapshot (zero while none has arrived).
    pub(super) fn distributable(&self) -> u64 {
        self.capacity.map_or(0, |capacity| capacity.distributable)
    }
}

/// The exact pennies `person` receives from a `pennies` distribution over
/// `ownership`, from the same `pro_rata_split` the server pays with (the
/// whole-penny remainder goes to the first cap-table entry, which need not be
/// the local player). A thread-local scratch table keeps the preview free of
/// per-frame allocation.
pub(crate) fn local_dividend_take(
    pennies: u64,
    ownership: &CompanyOwnership,
    person: PersonId,
) -> u64 {
    thread_local! {
        static SPLIT: std::cell::RefCell<Vec<(PersonId, u64)>> = const { std::cell::RefCell::new(Vec::new()) };
    }
    SPLIT.with(|split| {
        let mut split = split.borrow_mut();
        pro_rata_split(pennies, ownership, &mut split);
        split
            .iter()
            .find(|(holder, _)| *holder == person)
            .map_or(0, |(_, take)| *take)
    })
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
    /// Observed employees of the selected site, deduplicated and sorted by
    /// id (see [`observed_workers`]); names come from `name_of`.
    pub(super) workers: &'a [PersonId],
    pub(super) company: Option<CompanyView<'a>>,
    pub(super) local_person: Option<PersonId>,
    /// The local hero's replicated wallet in pennies (zero while unknown);
    /// the capital-contribution draft and payload clamp to it.
    pub(super) local_wallet: u64,
    pub(super) share_draft: &'a ShareOrderDraft,
    pub(super) dividend_draft: &'a DividendDraft,
    pub(super) capital_draft: &'a CapitalDraft,
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
        local_wallet,
        share_draft,
        dividend_draft,
        capital_draft,
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
            *local_wallet,
            share_draft,
            dividend_draft,
            capital_draft,
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
    blocks.push(Block::Section("THIS WORKPLACE"));
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
    blocks.push(Block::Section("EMPLOYEES AT THIS SITE"));
    // One fixed chip slot per position: interest-scoped replication makes the
    // observed set churn, and the chips must not respawn the page when it does.
    let positions = usize::from(building.kind.positions());
    let observed = workers.len().min(positions);
    blocks.push(row(
        "workers",
        "EMPLOYEES",
        if observed == 0 {
            "No observed employees at this site".to_string()
        } else {
            format!("{observed} observed employees · select a name to view their character")
        },
        (0..positions)
            .map(|slot| match workers.get(slot) {
                Some(person) => ControlModel::new(
                    format!("worker.{slot}"),
                    name_of(*person),
                    ControlPress::Person(*person),
                ),
                None => ControlModel::vacant(format!("worker.{slot}"), VacantSlot::Person),
            })
            .collect(),
    ));
    if can_manage {
        let management = site.management;
        blocks.push(automation_row(
            "autopilot",
            "SITE AUTOPILOT",
            management.autopilot,
        ));
        blocks.push(strategy_row("strategy", management.strategy));
        blocks.push(Block::Section("STAFFING & PAY"));
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
        blocks.push(Block::Section("LOCAL SALES"));
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
            blocks.push(Block::Section("TAVERN SERVICE"));
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

        blocks.push(Block::Section("GOODS FLOW"));
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

/// The AUTOMATIC DIVIDEND choice row (the policy line and the RETAIN / share
/// chips), the SHAREHOLDER DIVIDENDS row (replicated headroom, amount picker
/// and confirm control) and the IF DISTRIBUTED NOW preview row.
///
/// Every control is present whenever the viewer can manage, whatever the
/// snapshot says: a headroom of zero, or none published yet, is a value
/// (label `DISTRIBUTE 0.00 COIN`, payload `pennies: u64::MAX`, so the
/// finance pass answers from its live figure and republishes the snapshot),
/// never a structural change. A draft at the whole headroom (ALL, or one the
/// player never stepped) also sends `u64::MAX`.
pub(super) fn push_dividend_rows(
    blocks: &mut Vec<Block>,
    company: &CompanyView<'_>,
    local_person: Option<PersonId>,
    draft: &DividendDraft,
    can_manage: bool,
) {
    let id = "company.dividends";
    let distributable = company.distributable();
    let headroom = match company.capacity {
        None => "Available now: awaiting the first finance review".to_string(),
        Some(capacity) => {
            let mut text = format!(
                "Available now {} coin ({})  ·  reserves {} coin  ·  ",
                format_money(capacity.distributable),
                if capacity.day == u32::MAX {
                    "not yet reviewed".to_string()
                } else {
                    format!("day {}", capacity.day)
                },
                format_money(capacity.protected_reserves),
            );
            if capacity.last_paid_day == u32::MAX {
                text.push_str("never paid");
            } else {
                text.push_str(&format!(
                    "last paid {} coin on day {}",
                    format_money(capacity.last_paid),
                    capacity.last_paid_day
                ));
            }
            text
        }
    };
    let float = format_money(COMPANY_DIVIDEND_FLOAT);
    let percent = company.policy.automatic_payout_percent;
    let policy = if percent > 0 {
        format!(
            "Pays {percent}% of retained profit above the working-capital runway (wage and tax debt, each site's payroll days and input coverage, {float} coin float) every day."
        )
    } else {
        format!(
            "Profits are retained until you distribute them. Anything above wage and tax debt, one day of every site's payroll and a {float} coin float may be paid, contributed capital included."
        )
    };
    // The share is a choice row: every preset is a fixed chip and the sixth
    // slot shows a share no preset can send as its own selected chip, so a
    // policy change (or an unusual value) binds in place and never respawns.
    let policy_controls = if can_manage {
        let mut chips: Vec<ControlModel> = AUTOMATIC_DIVIDEND_PRESETS
            .into_iter()
            .map(|preset| {
                company_choice(
                    company.id,
                    format!("{id}.auto.{preset}"),
                    automatic_dividend_label(preset),
                    HeroCompanyAction::SetAutomaticDividend {
                        payout_percent: preset,
                    },
                    preset == percent,
                )
            })
            .collect();
        chips.push(if AUTOMATIC_DIVIDEND_PRESETS.contains(&percent) {
            ControlModel::vacant(format!("{id}.auto.custom"), VacantSlot::Action)
        } else {
            company_choice(
                company.id,
                format!("{id}.auto.custom"),
                automatic_dividend_label(percent),
                HeroCompanyAction::SetAutomaticDividend {
                    payout_percent: percent,
                },
                true,
            )
        });
        chips
    } else {
        vec![]
    };
    blocks.push(row(
        format!("{id}.policy"),
        "AUTOMATIC DIVIDEND",
        policy,
        policy_controls,
    ));
    // Managers distribute the drafted amount; everyone else previews a full
    // distribution of the published headroom.
    let amount = if can_manage {
        draft.pennies.min(distributable)
    } else {
        distributable
    };
    let controls = if can_manage {
        let mut controls = Vec::with_capacity(6);
        controls.extend(
            [
                ("down", "-1 COIN", DividendDraftAction::Down),
                ("up", "+1 COIN", DividendDraftAction::Up),
                ("quarter", "25%", DividendDraftAction::Quarter),
                ("half", "50%", DividendDraftAction::Half),
                ("all", "ALL", DividendDraftAction::All),
            ]
            .into_iter()
            .map(|(suffix, label, step)| {
                ControlModel::new(
                    format!("{id}.{suffix}"),
                    label,
                    ControlPress::DividendDraft(step),
                )
            }),
        );
        // ALL (a draft at the whole published headroom) and a press while the
        // snapshot shows nothing ask for everything: the server clamps against
        // its live figure and republishes, so a stale snapshot is never a
        // dead end or a ceiling. A deliberately smaller draft is sent as is.
        controls.push(company_order(
            company.id,
            format!("{id}.distribute"),
            format!("DISTRIBUTE {} COIN", format_money(amount)),
            HeroCompanyAction::DistributeDividend {
                pennies: if distributable == 0 || amount >= distributable {
                    u64::MAX
                } else {
                    amount
                },
            },
        ));
        controls
    } else {
        vec![]
    };
    blocks.push(row(id, "SHAREHOLDER DIVIDENDS", headroom, controls));
    let own = local_person
        .filter(|person| person.is_assigned())
        .map(|person| {
            (
                company.ownership.share_count(person),
                local_dividend_take(amount, company.ownership, person),
            )
        });
    blocks.push(row(
        format!("{id}.preview"),
        "IF DISTRIBUTED NOW",
        format!(
            "{} coin  ·  {} coin per 10 shares  ·  {}",
            format_money(amount),
            // 1,000 shares: ten shares are 1% of the distribution. The same
            // wording as the server's reply; the exact take below carries the
            // penny remainder.
            format_money(amount / 100),
            match own {
                Some((shares, take)) if shares > 0 =>
                    format!("your {shares} shares receive {} coin", format_money(take)),
                _ => "you hold no shares".to_string(),
            }
        ),
        vec![],
    ));
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
