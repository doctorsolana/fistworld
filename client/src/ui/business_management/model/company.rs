//! Company-wide controls never require a site entity.

use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) fn append_company_controls(
    blocks: &mut Vec<Block>,
    company: &CompanyView<'_>,
    local_person: &Option<PersonId>,
    local_wallet: u64,
    share_draft: &ShareOrderDraft,
    dividend_draft: &DividendDraft,
    capital_draft: &CapitalDraft,
    name_of: &dyn Fn(PersonId) -> String,
    can_manage: bool,
) {
    let own_shares = local_person.map_or(0, |person| company.ownership.share_count(person));
    blocks.push(Block::Scope(BusinessManagementPage::Company));
    blocks.push(Block::Section("COMPANY DIRECTION"));
    blocks.push(row(
    "company.scope", "APPLIES ACROSS THE COMPANY",
    format!("{} sets the company strategy. Automatic sites follow it; manual site overrides remain in place.", CompanyLeadership::TITLE), vec![],
));
    if can_manage {
        blocks.push(row(
            "company.autopilot",
            "COMPANY MASTER AUTOPILOT",
            if company.policy.autopilot {
                "Automatically reviews company strategy"
            } else {
                "Your selected company strategy is retained"
            },
            vec![company_order(
                company.id,
                "company.autopilot.toggle",
                if company.policy.autopilot {
                    "MANUAL"
                } else {
                    "AUTOMATIC"
                },
                HeroCompanyAction::SetAutopilot(!company.policy.autopilot),
            )],
        ));
        blocks.push(row(
            "company.strategy",
            "COMPANY STRATEGY",
            company.policy.strategy.label(),
            BusinessStrategy::ALL
                .into_iter()
                .enumerate()
                .map(|(i, strategy)| {
                    company_choice(
                        company.id,
                        format!("company.strategy.{i}"),
                        strategy.label().to_uppercase(),
                        HeroCompanyAction::SetStrategy(strategy),
                        strategy == company.policy.strategy,
                    )
                })
                .collect(),
        ));
    }
    blocks.push(Block::Section("TREASURY & DIVIDENDS"));
    blocks.push(row(
        "company.today",
        "COMPANY RESULT TODAY",
        signed_coin(company.account.current_day.profit()),
        vec![],
    ));
    blocks.push(row(
        "company.treasury",
        "POOLED COMPANY TREASURY",
        format!(
            "{} coin  ·  assets {}  ·  wage debt {}  ·  tax debt {}",
            format_money(company.account.cash),
            format_money(company.account.book_value),
            format_money(company.account.wage_arrears),
            format_money(company.account.tax_arrears)
        ),
        vec![],
    ));
    // Any shareholder may donate personal coin; the presets and the stepped
    // draft both clamp to the replicated wallet, and the server checks it
    // again when it debits.
    if own_shares > 0 {
        let amount = capital_draft.pennies.min(local_wallet);
        let mut controls: Vec<ControlModel> = [100u64, 500]
            .into_iter()
            .map(|amount| {
                company_order(
                    company.id,
                    format!("company.capital.{amount}"),
                    format!("CONTRIBUTE {} COIN", format_money(amount)),
                    HeroCompanyAction::ContributeCapital { amount },
                )
            })
            .collect();
        controls.extend(
            [
                ("down", "-1 COIN", CapitalDraftAction::Down),
                ("up", "+1 COIN", CapitalDraftAction::Up),
                ("ten", "+10 COIN", CapitalDraftAction::TenUp),
                ("all", "ALL", CapitalDraftAction::All),
            ]
            .into_iter()
            .map(|(suffix, label, step)| {
                ControlModel::new(
                    format!("company.capital.{suffix}"),
                    label,
                    ControlPress::CapitalDraft(step),
                )
            }),
        );
        controls.push(company_order(
            company.id,
            "company.capital.contribute",
            format!("CONTRIBUTE {} COIN", format_money(amount)),
            HeroCompanyAction::ContributeCapital { amount },
        ));
        blocks.push(row(
            "company.capital",
            "CONTRIBUTE PERSONAL COIN",
            format!(
                "Draft {} coin of your {} coin wallet. A donation becomes company capital, not revenue: it raises what is distributable and is recoverable only pro rata, through dividends or a share sale.",
                format_money(amount),
                format_money(local_wallet),
            ),
            controls,
        ));
    }
    push_dividend_rows(blocks, company, *local_person, dividend_draft, can_manage);
    blocks.push(Block::Section("OWNERSHIP & GOVERNANCE"));
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
                    company_choice(
                        company.id,
                        format!("appoint.{}", holding.shareholder.0),
                        if company.leadership.master == holding.shareholder {
                            format!("{name} IS MASTER")
                        } else {
                            format!("APPOINT {name}")
                        },
                        HeroCompanyAction::AppointCompanyMaster(holding.shareholder),
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
        controls.push(ControlModel::new(
            "draft.shares.down",
            "SHARES -10",
            ControlPress::Draft(ShareDraftAction::SharesDown),
        ));
        controls.push(ControlModel::new(
            "draft.shares.up",
            "SHARES +10",
            ControlPress::Draft(ShareDraftAction::SharesUp),
        ));
        controls.push(ControlModel::new(
            "draft.price.down",
            "PRICE -0.25",
            ControlPress::Draft(ShareDraftAction::PriceDown),
        ));
        controls.push(ControlModel::new(
            "draft.price.up",
            "PRICE +0.25",
            ControlPress::Draft(ShareDraftAction::PriceUp),
        ));
        controls.push(company_order(
            company.id,
            "share.post",
            "POST / REPLACE OFFER",
            HeroCompanyAction::ListCompanyShares {
                shares: share_draft.shares.min(own_shares),
                unit_price: share_draft.unit_price,
            },
        ));
    }
    if own_offer.is_some() {
        controls.push(company_order(
            company.id,
            "share.cancel",
            "CANCEL OFFER",
            HeroCompanyAction::CancelCompanyShareListing,
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
            // Fixed slots per seller (`buy.{seller}.{k}`): the offered quantity
            // is a bound label and payload, so a partial purchase never
            // changes the structure key. Duplicate quantities leave a slot
            // vacant instead of shifting the buttons that follow.
            let mut quantities: [u16; BUY_SLOTS_PER_OFFER] = [1, 10, offer.shares];
            quantities
                .iter_mut()
                .for_each(|q| *q = (*q).min(offer.shares));
            let seller_name = name_of(offer.seller).to_uppercase();
            let mut previous = 0;
            for (slot, quantity) in quantities.into_iter().enumerate() {
                let id = format!("buy.{}.{slot}", offer.seller.0);
                if quantity == 0 || quantity == previous {
                    buys.push(ControlModel::vacant(id, VacantSlot::Action));
                    continue;
                }
                previous = quantity;
                buys.push(company_order(
                    company.id,
                    id,
                    format!("BUY {quantity} FROM {seller_name}"),
                    HeroCompanyAction::BuyCompanyShares {
                        seller: offer.seller,
                        shares: quantity,
                    },
                ));
            }
        }
    }
    blocks.push(row("share.public", "PUBLIC SHARE OFFERS", offer_text, buys));

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
}
