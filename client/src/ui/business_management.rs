//! Owner controls for one player-owned business.
//!
//! The panel edits replicated policies rather than maintaining client-only
//! settings. NPC autopilot and player management therefore remain one economy.

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
    CompanyManagementPolicy, Good, GoodsInventory, Wallet,
};
use shared::protocol::{
    HeroBusinessAction, HeroBusinessOrder, HeroBusinessResult, ReliableChannel,
};

use crate::states::GameState;
use crate::ui::foundation::{
    button_chrome, retained_scroll, subtree_is_interacting, UiButtonLabel, UiButtonVariant,
    UiRefreshStamp,
};
use crate::ui::modal::{
    handle_backdrop_pressed, spawn_modal, update_modal_click_guard, ModalLayout,
};
use crate::ui::styles::{
    plate_shadow, INK, INK_MUTED, LIMEWASH, LIMEWASH_LIT, LIMEWASH_WELL, PLATE_RULE,
    PLATE_RULE_SOFT, RADIUS,
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
                handle_back_to_company,
                handle_close,
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
    signature: String,
    target: Entity,
}

#[derive(Component)]
struct Backdrop;
#[derive(Component)]
struct Panel;
#[derive(Component)]
struct Close;
#[derive(Component)]
struct BackToCompany;
#[derive(Component)]
struct BodyScroll;

#[derive(Component, Clone, Copy)]
struct Action(HeroBusinessAction);

#[derive(Component, Clone, Copy)]
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

#[allow(clippy::type_complexity)]
fn ensure_panel(
    mut commands: Commands,
    time: Res<Time<Real>>,
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
        Option<&shared::economy::TavernService>,
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
    heroes: Query<(&Hero, &PersonId, Option<&Wallet>)>,
    roots: Query<(Entity, &Root, Option<&UiRefreshStamp>)>,
    body_scroll: Query<&ScrollPosition, With<BodyScroll>>,
    children: Query<&Children>,
    interactions: Query<(&Interaction, Has<crate::ui::foundation::UiRefreshExempt>)>,
) {
    let Some(entity) = target.0 else {
        for (root, ..) in roots.iter() {
            commands.entity(root).despawn();
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
        for (root, ..) in roots.iter() {
            commands.entity(root).despawn();
        }
        return;
    };
    let company =
        operated_by.and_then(|operation| companies.iter().find(|(id, ..)| **id == operation.0));
    let local_hero = local.as_ref().and_then(|local| {
        heroes
            .iter()
            .find(|(hero, ..)| shared::player::peer_id_to_u64(hero.owner) == local.0)
    });
    let local_person = local_hero.map(|(_, person, _)| *person);
    let local_balance = local_hero
        .and_then(|(_, _, wallet)| wallet)
        .map(|wallet| wallet.balance());
    if let Some((company_id, ..)) = company {
        if share_draft.company != Some(*company_id) {
            *share_draft = ShareOrderDraft {
                company: Some(*company_id),
                ..default()
            };
        }
    }
    let signature = format!(
        "{entity:?}|{building:?}|{building_id:?}|{operated_by:?}|{account:?}|{management:?}|{wage:?}|{sale:?}|{staffing:?}|{procurement:?}|{supply:?}|{inventory:?}|{tavern_service:?}|{company:?}|{local_person:?}|{local_balance:?}|{share_draft:?}|{}|{}|{:?}",
        feedback.success, feedback.message, return_to.0
    );
    if roots.iter().any(|(_, root, _)| root.signature == signature) {
        return;
    }
    let retained_scroll = retained_scroll(
        roots.iter().any(|(_, root, _)| root.target == entity),
        body_scroll.iter().next().map(|position| position.0),
    );
    if roots.iter().any(|(entity, _, stamp)| {
        subtree_is_interacting(entity, &children, &interactions)
            || stamp.is_some_and(|stamp| !stamp.is_ready(&time))
    }) {
        return;
    }
    for (root, ..) in roots.iter() {
        commands.entity(root).despawn();
    }

    let modal = spawn_modal(
        &mut commands,
        Root {
            signature,
            target: entity,
        },
        Backdrop,
        Panel,
        ModalLayout {
            panel_size: Vec2::new(760.0, 720.0),
            panel_padding: 0.0,
        },
    );
    commands
        .entity(modal.root)
        .insert(UiRefreshStamp::now(&time));
    commands.entity(modal.panel).insert((
        Node {
            width: Val::Vw(86.0),
            max_width: Val::Px(760.0),
            height: Val::Vh(84.0),
            max_height: Val::Px(720.0),
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
    commands.entity(modal.panel).with_children(|panel| {
        panel
            .spawn((
                Node {
                    width: Val::Percent(100.0),
                    padding: UiRect::axes(Val::Px(22.0), Val::Px(15.0)),
                    justify_content: JustifyContent::SpaceBetween,
                    align_items: AlignItems::Center,
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
                        let company_name = company
                            .map(|(_, company, ..)| company.name.as_str());
                        copy.spawn((
                            Text::new(company_name.map_or_else(
                                || format!("MANAGE {}", building.kind.label().to_uppercase()),
                                |name| format!("MANAGE {}", name.to_uppercase()),
                            )),
                            TextFont {
                                font_size: FontSize::Px(21.0),
                                ..default()
                            },
                            TextColor(INK),
                        ));
                        let company_cash = company.map_or(0, |(_, _, _, _, account, ..)| account.cash);
                        copy.spawn((
                            Text::new(format!(
                                "{} #{} / {} / COMPANY TREASURY {} COIN",
                                building.kind.label().to_uppercase(),
                                building_id.0,
                                building.settlement.to_uppercase(),
                                format_money(company_cash)
                            )),
                            TextFont {
                                font_size: FontSize::Px(9.0),
                                ..default()
                            },
                            TextColor(INK_MUTED),
                        ));
                    });
                header
                    .spawn(Node {
                        flex_direction: FlexDirection::Row,
                        column_gap: Val::Px(6.0),
                        ..default()
                    })
                    .with_children(|actions| {
                        if return_to.0.is_some() {
                            button(actions, BackToCompany, "BACK TO COMPANY");
                        }
                        button(actions, Close, "X");
                    });
            });

        panel
            .spawn((
                BodyScroll,
                ScrollPosition(retained_scroll),
                Node {
                    width: Val::Percent(100.0),
                    flex_grow: 1.0,
                    min_height: Val::Px(0.0),
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(12.0),
                    padding: UiRect::all(Val::Px(22.0)),
                    overflow: Overflow::scroll_y(),
                    scrollbar_width: 8.0,
                    ..default()
                },
            ))
            .with_children(|body| {
                // Company ownership and share trading live on the Company page.
                // Keep the legacy fallback only for a direct/open-without-return
                // path; the ordinary Back-to-Company flow starts with the site.
                if company.is_some() && return_to.0.is_none() {
                    section_caption(body, "COMPANY-WIDE FINANCE, OWNERSHIP & GOVERNANCE");
                }
                let can_manage = company.is_none_or(|(_, _, _, leadership, ..)| {
                    local_person.is_some_and(|person| leadership.can_manage(person))
                });
                if let Some((company_id, company, ownership, leadership, company_account, company_policy, decisions, share_market)) = company.filter(|_| return_to.0.is_none()) {
                    let master_name = people
                        .iter()
                        .find(|(id, _)| **id == leadership.master)
                        .map_or_else(
                            || format!("Person #{}", leadership.master.0),
                            |(_, name)| name.0.clone(),
                        );
                    let cap_table = ownership
                        .shares()
                        .iter()
                        .map(|holding| {
                            let name = people
                                .iter()
                                .find(|(id, _)| **id == holding.shareholder)
                                .map_or_else(
                                    || format!("Person #{}", holding.shareholder.0),
                                    |(_, name)| name.0.clone(),
                                );
                            format!("{name}: {} / 1,000 ({:.1}%)", holding.shares, f32::from(holding.shares) / 10.0)
                        })
                        .collect::<Vec<_>>()
                        .join("  /  ");
                    control_row(
                        body,
                        "COMPANY & 1,000-SHARE CAP TABLE",
                        &format!(
                            "{} (#{}), {}: {}; company treasury {} coin; capital assets {} coin; lifetime capital spending {} coin; wage/tax debt {} / {} coin\n{}",
                            company.name,
                            company_id.0,
                            CompanyLeadership::TITLE,
                            master_name,
                            format_money(company_account.cash),
                            format_money(company_account.book_value),
                            format_money(company_account.capital_expenditures),
                            format_money(company_account.wage_arrears),
                            format_money(company_account.tax_arrears),
                            cap_table,
                        ),
                        |controls| {
                            if local_person.is_some_and(|person| ownership.can_appoint_master(person)) {
                                for holding in ownership.shares() {
                                    let name = people
                                        .iter()
                                        .find(|(id, _)| **id == holding.shareholder)
                                        .map_or_else(
                                            || format!("Person #{}", holding.shareholder.0),
                                            |(_, name)| name.0.clone(),
                                        );
                                    action_button(
                                        controls,
                                        HeroBusinessAction::AppointCompanyMaster(
                                            holding.shareholder,
                                        ),
                                        if leadership.master == holding.shareholder {
                                            format!("SELECTED: {name} IS MASTER")
                                        } else {
                                            format!("APPOINT {name}")
                                        },
                                    );
                                }
                            }
                        },
                    );
                    let own_shares = local_person.map_or(0, |person| ownership.share_count(person));
                    let own_offer = local_person.and_then(|person| share_market.offer_from(person));
                    control_row(
                        body,
                        "YOUR SHARE OFFER",
                        &format!(
                            "You own {own_shares} / 1,000 shares. Draft: {} shares at {} coin each. {}",
                            share_draft.shares,
                            format_money(share_draft.unit_price),
                            own_offer.map_or_else(
                                || "No active offer.".to_string(),
                                |offer| format!(
                                    "Listed: {} at {} coin each since day {}.",
                                    offer.shares,
                                    format_money(offer.unit_price),
                                    offer.listed_day,
                                ),
                            ),
                        ),
                        |controls| {
                            if own_shares > 0 {
                                share_draft_button(controls, ShareDraftAction::SharesDown, "SHARES -10");
                                share_draft_button(controls, ShareDraftAction::SharesUp, "SHARES +10");
                                share_draft_button(controls, ShareDraftAction::PriceDown, "PRICE -0.25");
                                share_draft_button(controls, ShareDraftAction::PriceUp, "PRICE +0.25");
                                action_button(
                                    controls,
                                    HeroBusinessAction::ListCompanyShares {
                                        shares: share_draft.shares.min(own_shares),
                                        unit_price: share_draft.unit_price,
                                    },
                                    "POST / REPLACE OFFER",
                                );
                            }
                            if own_offer.is_some() {
                                action_button(
                                    controls,
                                    HeroBusinessAction::CancelCompanyShareListing,
                                    "CANCEL OFFER",
                                );
                            }
                        },
                    );
                    let offer_text = if share_market.offers().is_empty() {
                        "No shares are currently offered.".to_string()
                    } else {
                        share_market
                            .offers()
                            .iter()
                            .map(|offer| {
                                let seller = people
                                    .iter()
                                    .find(|(id, _)| **id == offer.seller)
                                    .map_or_else(
                                        || format!("Person #{}", offer.seller.0),
                                        |(_, name)| name.0.clone(),
                                    );
                                format!(
                                    "{seller}: {} shares at {} coin each",
                                    offer.shares,
                                    format_money(offer.unit_price),
                                )
                            })
                            .collect::<Vec<_>>()
                            .join("\n")
                    };
                    control_row(body, "PUBLIC SHARE OFFERS", &offer_text, |controls| {
                        if let Some(person) = local_person {
                            for offer in share_market.offers() {
                                if offer.seller == person {
                                    continue;
                                }
                                for quantity in [1, 10, offer.shares] {
                                    let quantity = quantity.min(offer.shares);
                                    if quantity == 0 {
                                        continue;
                                    }
                                    action_button(
                                        controls,
                                        HeroBusinessAction::BuyCompanyShares {
                                            seller: offer.seller,
                                            shares: quantity,
                                        },
                                        format!("BUY {quantity} FROM #{}", offer.seller.0),
                                    );
                                }
                            }
                        }
                    });
                    control_row(
                        body,
                        "COMPANY DIVIDENDS",
                        if company_policy.automatic_dividends {
                            "Automatic after company-wide payroll, tax, input and operating reserves"
                        } else {
                            "Retained until manually distributed"
                        },
                        |controls| {
                            if can_manage {
                                action_button(
                                    controls,
                                    HeroBusinessAction::SetAutomaticWithdrawals(
                                        !company_policy.automatic_dividends,
                                    ),
                                    if company_policy.automatic_dividends {
                                        "RETAIN PROFITS"
                                    } else {
                                        "AUTO DIVIDEND"
                                    },
                                );
                                action_button(
                                    controls,
                                    HeroBusinessAction::WithdrawAvailableProfit,
                                    "DISTRIBUTE AVAILABLE",
                                );
                            }
                        },
                    );
                    let decision_text = if decisions.entries().is_empty() {
                        "No strategy change recorded yet".to_string()
                    } else {
                        decisions
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
                    control_row(
                        body,
                        "COMPANY MASTER DECISIONS",
                        &decision_text,
                        |_| {},
                    );
                    control_row(
                        body,
                        "SITE / COMPANY RESULT TODAY",
                        &format!(
                            "Site {}{} coin (external {} + internal {}); consolidated company {}{} coin",
                            if account.current_day.profit() < 0 { "-" } else { "+" },
                            format_money(account.current_day.profit().unsigned_abs()),
                            format_money(account.current_day.gross_revenue),
                            format_money(account.current_day.internal_revenue),
                            if company_account.current_day.profit() < 0 { "-" } else { "+" },
                            format_money(company_account.current_day.profit().unsigned_abs()),
                        ),
                        |_| {},
                    );
                }
                section_caption(
                    body,
                    &format!(
                        "SITE OPERATING CONTROLS - {} #{}",
                        building.kind.label().to_uppercase(),
                        building_id.0
                    ),
                );
                if can_manage {
                control_row(
                    body,
                    "OWNER AUTOPILOT",
                    if management.autopilot { "ON" } else { "OFF" },
                    |controls| {
                        action_button(
                            controls,
                            HeroBusinessAction::SetAutopilot(!management.autopilot),
                            if management.autopilot {
                                "PAUSE"
                            } else {
                                "ENABLE"
                            },
                        );
                    },
                );
                body.spawn((
                    Text::new("STRATEGY"),
                    TextFont {
                        font_size: FontSize::Px(9.0),
                        ..default()
                    },
                    TextColor(INK_MUTED),
                ));
                body.spawn(Node {
                    width: Val::Percent(100.0),
                    flex_wrap: FlexWrap::Wrap,
                    column_gap: Val::Px(6.0),
                    row_gap: Val::Px(6.0),
                    ..default()
                })
                .with_children(|row| {
                    for strategy in [
                        BusinessStrategy::Balanced,
                        BusinessStrategy::Growth,
                        BusinessStrategy::HighMargin,
                        BusinessStrategy::Cautious,
                        BusinessStrategy::Opportunistic,
                    ] {
                        action_button(
                            row,
                            HeroBusinessAction::SetStrategy(strategy),
                            if strategy == management.strategy {
                                format!("SELECTED: {}", strategy.label())
                            } else {
                                strategy.label().into()
                            },
                        );
                    }
                });
                control_row(
                    body,
                    "DAILY WAGE",
                    &format!(
                        "{} coin / {}",
                        format_money(wage.daily_wage),
                        if wage.automatic {
                            "automatic"
                        } else {
                            "manual"
                        }
                    ),
                    |controls| {
                        action_button(
                            controls,
                            HeroBusinessAction::SetDailyWage(wage.daily_wage.saturating_sub(25)),
                            "- 0.25",
                        );
                        action_button(
                            controls,
                            HeroBusinessAction::SetDailyWage(wage.daily_wage.saturating_add(25)),
                            "+ 0.25",
                        );
                        action_button(
                            controls,
                            HeroBusinessAction::SetAutomaticWage(!wage.automatic),
                            if wage.automatic {
                                "SET MANUAL"
                            } else {
                                "USE AUTO"
                            },
                        );
                    },
                );
                let enabled_positions = staffing
                    .copied()
                    .unwrap_or_default()
                    .target_for(building.kind);
                control_row(
                    body,
                    "OPEN POSITIONS",
                    &format!(
                        "{} of {} positions advertised; closing a filled position releases the highest stable-id worker back to the local labour market",
                        enabled_positions,
                        building.kind.positions(),
                    ),
                    |controls| {
                        for positions in 0..=building.kind.positions() {
                            action_button(
                                controls,
                                HeroBusinessAction::SetEnabledPositions(positions),
                                if positions == enabled_positions {
                                    format!("SELECTED: {positions}")
                                } else {
                                    positions.to_string()
                                },
                            );
                        }
                    },
                );
                control_row(
                    body,
                    if building.kind == SettlementBuildingKind::Tavern {
                        "MEAL PRICE"
                    } else {
                        "ASKING PRICE"
                    },
                    &format!(
                        "{} coin / {}",
                        format_money(sale.asking_unit_price),
                        if sale.automatic_pricing {
                            "automatic"
                        } else {
                            "manual"
                        }
                    ),
                    |controls| {
                        action_button(
                            controls,
                            HeroBusinessAction::SetAskingPrice(
                                sale.asking_unit_price.saturating_sub(25),
                            ),
                            "- 0.25",
                        );
                        action_button(
                            controls,
                            HeroBusinessAction::SetAskingPrice(
                                sale.asking_unit_price.saturating_add(25),
                            ),
                            "+ 0.25",
                        );
                        action_button(
                            controls,
                            HeroBusinessAction::SetAutomaticPricing(!sale.automatic_pricing),
                            if sale.automatic_pricing {
                                "SET MANUAL"
                            } else {
                                "USE AUTO"
                            },
                        );
                    },
                );
                if let Some(service) = tavern_service {
                    section_caption(body, "TAVERN SERVICE");
                    let day = service.current_day;
                    spawn_meter(
                        body,
                        "GUEST SERVICE".to_string(),
                        format!(
                            "{} of {} planned visits served today · {} coin direct revenue · {} unaffordable · {} unavailable · {} route failures",
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
                    );
                    control_row(
                        body,
                        "OPEN FLOOR",
                        &format!(
                            "{} Innkeeper{} on duty · {} of {} guest places occupied. Customers pay this company at the Tavern; no Moot market fee is charged.",
                            service.innkeepers_on_duty,
                            if service.innkeepers_on_duty == 1 { "" } else { "s" },
                            service.current_guests,
                            service.guest_capacity,
                        ),
                        |_| {},
                    );
                }
                section_caption(body, "GOODS FLOW");
                if let Some(output) = output_good(building.kind) {
                    let held = inventory.amount(output);
                    spawn_meter(
                        body,
                        format!("{} SITE STOCK", output.label().to_uppercase()),
                        format!(
                            "{} units held here / {} bulk used of {}. Local retention and excess-sale controls live on the Company page.",
                            held,
                            inventory.used_bulk(),
                            inventory.bulk_capacity(),
                        ),
                        held,
                        0,
                        (inventory.bulk_capacity() / output.bulk_per_unit()).max(1),
                    );
                    control_row(
                        body,
                        "PUBLIC MARKET RELEASE",
                        "Set once per good and settlement under Company > Local Goods & Storage. This site contributes stock while it is operating; downstream company requests are protected first.",
                        |_| {},
                    );
                    if building.kind == SettlementBuildingKind::LivestockFarm {
                        let wool = inventory.amount(Good::Wool);
                        spawn_meter(
                            body,
                            "WOOL BY-PRODUCT STOCK".to_string(),
                            format!(
                                "{} units held here. Each completed livestock cycle creates one Meat and one Wool; Wool is non-food and reserved for the future textile chain.",
                                wool,
                            ),
                            wool,
                            0,
                            (inventory.bulk_capacity() / Good::Wool.bulk_per_unit()).max(1),
                        );
                        control_row(
                            body,
                            "BY-PRODUCT PRICING",
                            "Wool follows this site's chosen margin, scaled from the Meat ask by each good's reference value. It remains a free market offer, not a fixed civic price.",
                            |_| {},
                        );
                    }
                }
                if company.is_none() {
                    control_row(
                        body,
                        "PROFIT DRAWS",
                        if management.automatic_withdrawals { "Automatic after protected working capital" } else { "Retained in the business" },
                        |controls| {
                            action_button(controls, HeroBusinessAction::SetAutomaticWithdrawals(!management.automatic_withdrawals), if management.automatic_withdrawals { "RETAIN PROFITS" } else { "AUTO DRAW" });
                            action_button(controls, HeroBusinessAction::WithdrawAvailableProfit, "WITHDRAW AVAILABLE");
                        },
                    );
                }
                let inputs: Vec<_> = Good::ALL
                    .into_iter()
                    .filter(|good| procurement.rule(*good).enabled)
                    .collect();
                if inputs.is_empty() {
                    control_row(body, "INPUT PROCUREMENT", "No purchased inputs", |_| {});
                } else {
                    control_row(
                        body,
                        "INPUT SOURCING",
                        if procurement.automatic && supply.automatic {
                            "ON - company deliveries are considered first, then the selected market fallback"
                        } else {
                            "PAUSED - no new internal or public input trips will be requested"
                        },
                        |controls| {
                            action_button(
                                controls,
                                HeroBusinessAction::SetAutomaticProcurement(
                                    !(procurement.automatic && supply.automatic),
                                ),
                                if procurement.automatic && supply.automatic {
                                    "PAUSE SOURCING"
                                } else {
                                    "RESUME SOURCING"
                                },
                            );
                        },
                    );
                    for good in inputs {
                        let rule = procurement.rule(good);
                        let private = supply.rule(good);
                        spawn_input_coverage(
                            body,
                            good,
                            inventory.amount(good),
                            rule.target_units,
                            rule.reorder_below,
                            rule.coverage_days,
                        );
                        control_row(
                            body,
                            &format!("{} INPUT COVER", good.label().to_uppercase()),
                            &format!(
                                "Keep enough for {} day{} of work. Current staffing converts that to a {}-unit target and company logistics replenish it automatically.",
                                rule.coverage_days,
                                if rule.coverage_days == 1 { "" } else { "s" },
                                rule.target_units,
                            ),
                            |controls| {
                                for days in [0, 1, 2, 3, 5, 7] {
                                    action_button(
                                        controls,
                                        HeroBusinessAction::SetInputCoverageDays { good, days },
                                        coverage_choice(days, rule.coverage_days),
                                    );
                                }
                            },
                        );
                        control_row(
                            body,
                            &format!("{} SUPPLY PRIORITY", good.label().to_uppercase()),
                            sourcing_explanation(private.sourcing),
                            |controls| {
                                for mode in [BusinessSourcingMode::PreferOwned, BusinessSourcingMode::CheapestAvailable, BusinessSourcingMode::OwnedOnly] {
                                    action_button(controls, HeroBusinessAction::SetInputSourcingMode { good, mode }, if mode == private.sourcing { format!("SELECTED: {}", sourcing_short_label(mode)) } else { sourcing_short_label(mode).to_string() });
                                }
                            },
                        );
                        control_row(
                            body,
                            &format!("{} BID CEILING", good.label().to_uppercase()),
                            &format!("{} coin", format_money(rule.maximum_unit_price)),
                            |controls| {
                                action_button(
                                    controls,
                                    HeroBusinessAction::SetInputMaximumPrice {
                                        good,
                                        unit_price: rule.maximum_unit_price.saturating_sub(25),
                                    },
                                    "- 0.25",
                                );
                                action_button(
                                    controls,
                                    HeroBusinessAction::SetInputMaximumPrice {
                                        good,
                                        unit_price: rule.maximum_unit_price.saturating_add(25),
                                    },
                                    "+ 0.25",
                                );
                            },
                        );
                    }
                }
                if let Some((_, company, _, leadership, company_account, company_policy, _, _)) = company {
                    section_caption(body, "COMPANY FINANCE");
                    let master_name = people
                        .iter()
                        .find(|(id, _)| **id == leadership.master)
                        .map_or_else(
                            || format!("Person #{}", leadership.master.0),
                            |(_, name)| name.0.clone(),
                        );
                    control_row(
                        body,
                        "COMPANY CONTEXT",
                        &format!(
                            "{} - {}: {}; treasury {} coin; wage/tax debt {} / {} coin; today {}{} coin",
                            company.name,
                            CompanyLeadership::TITLE,
                            master_name,
                            format_money(company_account.cash),
                            format_money(company_account.wage_arrears),
                            format_money(company_account.tax_arrears),
                            if company_account.current_day.profit() < 0 { "-" } else { "+" },
                            format_money(company_account.current_day.profit().unsigned_abs()),
                        ),
                        |_| {},
                    );
                    control_row(
                        body,
                        "DIVIDENDS",
                        if company_policy.automatic_dividends {
                            "Automatic after company-wide payroll, tax, input and operating reserves"
                        } else {
                            "Retained until manually distributed"
                        },
                        |controls| {
                            action_button(
                                controls,
                                HeroBusinessAction::SetAutomaticWithdrawals(
                                    !company_policy.automatic_dividends,
                                ),
                                if company_policy.automatic_dividends {
                                    "RETAIN PROFITS"
                                } else {
                                    "AUTO DIVIDEND"
                                },
                            );
                            action_button(
                                controls,
                                HeroBusinessAction::WithdrawAvailableProfit,
                                "DISTRIBUTE AVAILABLE",
                            );
                        },
                    );
                }
                } else {
                    control_row(
                        body,
                        "OPERATING AUTHORITY",
                        "Only the appointed Company Master may set wages, prices, sourcing, strategy or dividends. Use Back to Company for ownership, shares and the consolidated ledger.",
                        |_| {},
                    );
                }
                if !feedback.message.is_empty() {
                    body.spawn((
                        Text::new(feedback.message.clone()),
                        TextFont {
                            font_size: FontSize::Px(11.0),
                            ..default()
                        },
                        TextColor(if feedback.success {
                            Color::srgb(0.13, 0.38, 0.19)
                        } else {
                            Color::srgb(0.58, 0.12, 0.10)
                        }),
                    ));
                }
            });
    });
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

const COMPANY_STOCK_FILL: Color = Color::srgba(0.34, 0.32, 0.28, 0.94);
const MARKET_STOCK_FILL: Color = Color::srgba(0.62, 0.57, 0.48, 0.94);

fn coverage_choice(days: u8, selected: u8) -> String {
    let label = if days == 0 {
        "OFF".to_string()
    } else if days == 1 {
        "1 DAY".to_string()
    } else {
        format!("{days} DAYS")
    };
    if days == selected {
        format!("SELECTED: {label}")
    } else {
        label
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
            "Company first - use reachable owned stock, then buy from the local market"
        }
        BusinessSourcingMode::CheapestAvailable => {
            "Best value - compare the landed company transfer with the local market"
        }
        BusinessSourcingMode::OwnedOnly => {
            "Company only - never buy this input from an outside seller"
        }
    }
}

fn spawn_meter(
    parent: &mut ChildSpawnerCommands<'_>,
    title: String,
    summary: String,
    first_units: u32,
    second_units: u32,
    scale_units: u32,
) {
    let scale = scale_units.max(1) as f32;
    let first_width = (first_units as f32 / scale * 100.0).clamp(0.0, 100.0);
    let second_width = (second_units as f32 / scale * 100.0).clamp(0.0, 100.0 - first_width);
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                padding: UiRect::all(Val::Px(12.0)),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(7.0),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(LIMEWASH),
            BorderColor::all(PLATE_RULE_SOFT),
        ))
        .with_children(|card| {
            card.spawn(Node {
                width: Val::Percent(100.0),
                justify_content: JustifyContent::SpaceBetween,
                ..default()
            })
            .with_children(|header| {
                header.spawn((
                    Text::new(title),
                    TextFont {
                        font_size: FontSize::Px(9.0),
                        ..default()
                    },
                    TextColor(INK_MUTED),
                ));
                header.spawn((
                    Text::new(summary),
                    TextFont {
                        font_size: FontSize::Px(10.0),
                        ..default()
                    },
                    TextColor(INK),
                ));
            });
            card.spawn((
                Node {
                    width: Val::Percent(100.0),
                    height: Val::Px(13.0),
                    flex_direction: FlexDirection::Row,
                    overflow: Overflow::clip(),
                    border: UiRect::all(Val::Px(1.0)),
                    border_radius: BorderRadius::all(Val::Px(2.0)),
                    ..default()
                },
                BackgroundColor(LIMEWASH_WELL),
                BorderColor::all(PLATE_RULE_SOFT),
            ))
            .with_children(|track| {
                if first_width > 0.0 {
                    track.spawn((
                        Node {
                            width: Val::Percent(first_width),
                            height: Val::Percent(100.0),
                            ..default()
                        },
                        BackgroundColor(COMPANY_STOCK_FILL),
                    ));
                }
                if second_width > 0.0 {
                    track.spawn((
                        Node {
                            width: Val::Percent(second_width),
                            height: Val::Percent(100.0),
                            ..default()
                        },
                        BackgroundColor(MARKET_STOCK_FILL),
                    ));
                }
            });
        });
}

fn spawn_input_coverage(
    parent: &mut ChildSpawnerCommands<'_>,
    good: Good,
    held: u32,
    target: u32,
    reorder_below: u32,
    days: u8,
) {
    let covered = held.min(target);
    let surplus = held.saturating_sub(target);
    let state = if days == 0 {
        "sourcing off".to_string()
    } else if held < reorder_below {
        "replenishment requested".to_string()
    } else if held < target {
        "using buffer".to_string()
    } else {
        "target covered".to_string()
    };
    spawn_meter(
        parent,
        format!("{} INPUT COVERAGE", good.label().to_uppercase()),
        format!("{held} held / {target} target - {state}"),
        covered,
        surplus,
        held.max(target),
    );
}

fn section_caption(parent: &mut ChildSpawnerCommands<'_>, label: &str) {
    parent.spawn((
        Text::new(label),
        TextFont {
            font_size: FontSize::Px(10.0),
            ..default()
        },
        TextColor(INK_MUTED),
        Node {
            margin: UiRect::top(Val::Px(4.0)),
            ..default()
        },
    ));
}

fn control_row(
    parent: &mut ChildSpawnerCommands<'_>,
    label: &str,
    value: &str,
    controls: impl FnOnce(&mut ChildSpawnerCommands<'_>),
) {
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                padding: UiRect::all(Val::Px(12.0)),
                border: UiRect::all(Val::Px(1.0)),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(8.0),
                ..default()
            },
            BackgroundColor(LIMEWASH),
            BorderColor::all(PLATE_RULE_SOFT),
        ))
        .with_children(|row| {
            row.spawn((
                Text::new(label),
                TextFont {
                    font_size: FontSize::Px(9.0),
                    ..default()
                },
                TextColor(INK_MUTED),
            ));
            row.spawn((
                Text::new(value),
                TextFont {
                    font_size: FontSize::Px(12.0),
                    ..default()
                },
                TextColor(INK),
            ));
            row.spawn(Node {
                width: Val::Percent(100.0),
                flex_wrap: FlexWrap::Wrap,
                column_gap: Val::Px(6.0),
                row_gap: Val::Px(6.0),
                ..default()
            })
            .with_children(controls);
        });
}

fn button<M: Component>(
    parent: &mut ChildSpawnerCommands<'_>,
    marker: M,
    label: impl Into<String>,
) {
    parent
        .spawn((
            marker,
            Button,
            Node {
                min_width: Val::Px(30.0),
                height: Val::Px(30.0),
                padding: UiRect::horizontal(Val::Px(9.0)),
                align_items: AlignItems::Center,
                justify_content: JustifyContent::Center,
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
                font_size: FontSize::Px(9.0),
                ..default()
            },
            TextColor(INK),
            Pickable::IGNORE,
        ));
}

fn action_button(
    parent: &mut ChildSpawnerCommands<'_>,
    action: HeroBusinessAction,
    label: impl Into<String>,
) {
    button(parent, Action(action), label);
}

fn share_draft_button(
    parent: &mut ChildSpawnerCommands<'_>,
    action: ShareDraftAction,
    label: impl Into<String>,
) {
    button(parent, action, label);
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

fn handle_back_to_company(
    guard: Res<BusinessClickGuard>,
    buttons: Query<&Interaction, (With<BackToCompany>, Changed<Interaction>)>,
    mut target: ResMut<BusinessManagementTarget>,
    mut return_to: ResMut<BusinessManagementReturn>,
    mut encyclopedia_open: ResMut<crate::ui::encyclopedia::EncyclopediaOpen>,
    mut tab: ResMut<crate::ui::encyclopedia::EncyclopediaTab>,
    mut selected: ResMut<crate::ui::encyclopedia::companies::SelectedCompany>,
) {
    if !guard.0
        || !buttons
            .iter()
            .any(|interaction| *interaction == Interaction::Pressed)
    {
        return;
    }
    let Some(company) = return_to.0.take() else {
        return;
    };
    target.0 = None;
    selected.0 = Some(company);
    *tab = crate::ui::encyclopedia::EncyclopediaTab::Companies;
    encyclopedia_open.0 = true;
}

fn handle_close(
    guard: Res<BusinessClickGuard>,
    close: Query<&Interaction, (With<Close>, Changed<Interaction>)>,
    backdrop: Query<&Interaction, (With<Backdrop>, Changed<Interaction>)>,
    mut target: ResMut<BusinessManagementTarget>,
    mut return_to: ResMut<BusinessManagementReturn>,
) {
    if !guard.0 {
        return;
    }
    if close
        .iter()
        .any(|interaction| *interaction == Interaction::Pressed)
        || handle_backdrop_pressed(&backdrop)
    {
        target.0 = None;
        return_to.0 = None;
    }
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
        assert_eq!(coverage_choice(0, 2), "OFF");
        assert_eq!(coverage_choice(1, 1), "SELECTED: 1 DAY");
        assert_eq!(coverage_choice(5, 2), "5 DAYS");
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
}
