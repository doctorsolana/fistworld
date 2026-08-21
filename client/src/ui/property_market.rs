//! Settlement permit and private-property board.
//!
//! This is intentionally separate from the goods exchange: a sack of Flour is
//! a live offer, while a building takeover and a land-use permit are durable
//! development decisions. Both boards use the same visual ledger language.

use bevy::ecs::system::SystemParam;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use bevy::ui::InteractionDisabled;

use shared::components::{
    BuildingOf, CharacterName, CivicHallLevel, Company, CompanyId, CompanyLeadership, Hero,
    OperatedBy, OwnedBy, PermitMarketOpportunity, PersonId, PlayerPermitLedger, PlayerPosition,
    PropertyMarketListing, Settlement, SettlementBuilding, SettlementBuildingKind, SettlementId,
    SettlementOpportunityBoard, SettlementPolicies, SettlementPropertyBoard, WorldTime,
};
use shared::economy::{
    format_money, player_permit_price_with_subsidy, CompanyAccount, Wallet,
    PROPERTY_MARKET_EXPOSURE_DAYS,
};

use crate::camera_rts::LocalPeerId;
use crate::states::GameState;
use crate::ui::company_founding::{
    CompanyFoundingDraft, CompanyFoundingFeedback, OpenCompanyFounding,
};
use crate::ui::foundation::{
    button_chrome, selected_button_chrome, UiButtonLabel, UiButtonStyle, UiButtonVariant,
    UiRefreshExempt,
};
use crate::ui::modal::{
    handle_backdrop_pressed, spawn_modal, update_modal_click_guard, ModalLayout,
};
use crate::ui::styles::{
    plate_shadow, INK, INK_MUTED, LIMEWASH, LIMEWASH_LIT, LIMEWASH_WELL, PLATE_RULE,
    PLATE_RULE_SOFT, RADIUS,
};

use super::player_permits::{ActiveCompany, PendingPermitQuote, PurchasePermitButton};

/// Type scale for this board. The UI scale is derived from a 1600x900 design
/// frame, so the old 7-10 px captions rendered at roughly 6 pt on a Retina
/// laptop. Everything a player must read to make a decision sits at body
/// size or larger; captions label values, they never carry the decision.
const T_TITLE: f32 = 22.0;
const T_HEADING: f32 = 17.0;
const T_VALUE: f32 = 15.0;
const T_BUTTON: f32 = 14.0;
const T_BODY: f32 = 13.5;
const T_LABEL: f32 = 12.0;

pub struct PropertyMarketPlugin;

impl Plugin for PropertyMarketPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PropertyMarketTarget>();
        app.init_resource::<PropertyMarketTab>();
        app.init_resource::<PropertyClickGuard>();
        app.add_systems(
            Update,
            (
                handle_company_context_buttons,
                handle_property_tab_buttons,
                sync_property_panel,
                update_property_guard,
                handle_property_close,
            )
                .chain()
                .run_if(in_state(GameState::Playing)),
        );
        app.add_systems(OnExit(GameState::Playing), despawn_property_panel);
    }
}

#[derive(Resource, Default)]
pub(crate) struct PropertyMarketTarget(pub Option<Entity>);

/// The board shows one ledger at a time: land-use permits, or buildings
/// changing hands. Two half-width columns made both unreadable.
#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum PropertyMarketTab {
    #[default]
    Permits,
    ForSale,
}

#[derive(Component)]
struct PropertyTabButton(PropertyMarketTab);

/// Refresh-gated panel state that would otherwise push
/// [`ensure_property_panel`] past Bevy's system parameter limit.
/// The board's bound widgets. The tree is built once per page (see
/// [`PropertyPanelRoot`]) and these queries write the live values into it
/// every frame. No signature strings, no hover deferral, no scroll hacks:
/// the widgets under the pointer are the same entities from frame to frame.
#[derive(SystemParam)]
struct PropertyPanelUi<'w, 's> {
    roots: Query<'w, 's, (Entity, &'static PropertyPanelRoot)>,
    #[allow(clippy::type_complexity)]
    texts: Query<
        'w,
        's,
        (
            &'static mut Text,
            AnyOf<(
                &'static PermitPriceText,
                &'static PermitDemandText,
                &'static PermitActionLabel,
                &'static ListingFactText,
                &'static ActingCompanyText,
                &'static PropertyTabCount,
            )>,
        ),
    >,
    #[allow(clippy::type_complexity)]
    actions: Query<
        'w,
        's,
        (
            Entity,
            &'static PermitActionButton,
            &'static mut UiButtonStyle,
            Has<InteractionDisabled>,
            Option<&'static PurchasePermitButton>,
        ),
    >,
    tab: Res<'w, PropertyMarketTab>,
    perf: Res<'w, crate::ui::perf::UiPerf>,
}

// Bind markers: each names the one value its Text shows.
#[derive(Component)]
struct PermitPriceText(SettlementBuildingKind);
#[derive(Component)]
struct PermitDemandText(SettlementBuildingKind);
#[derive(Component)]
struct PermitActionButton(SettlementBuildingKind);
#[derive(Component)]
struct PermitActionLabel(SettlementBuildingKind);
#[derive(Component, Clone, Copy)]
struct ListingFactText {
    index: usize,
    field: ListingField,
}
#[derive(Clone, Copy, PartialEq, Eq)]
enum ListingField {
    Asking,
    Listed,
    Status,
}
#[derive(Component)]
struct ActingCompanyText;
#[derive(Component)]
struct PropertyTabCount(PropertyMarketTab);

/// Everything on a permit card that can change while the card stays.
#[derive(Clone, Debug, PartialEq, Eq)]
struct PermitCardValues {
    price_text: String,
    demand_text: String,
    action_label: String,
    enabled: bool,
    purchase: Option<PurchasePermitButton>,
}

fn tab_label(tab: PropertyMarketTab, permits: usize, listings: usize) -> String {
    match tab {
        PropertyMarketTab::Permits => format!("PERMITS  {permits}"),
        PropertyMarketTab::ForSale => format!("FOR SALE  {listings}"),
    }
}

fn acting_company_text(acting: &ActingCompanyView) -> String {
    format!("{}  /  {} coin", acting.name, format_money(acting.cash))
}

fn listing_fact(listing: &PropertyMarketListing, day: u32, field: ListingField) -> String {
    let age = day.saturating_sub(listing.listed_day);
    let exposure_left = PROPERTY_MARKET_EXPOSURE_DAYS.saturating_sub(age);
    match field {
        ListingField::Asking => format!("{} coin", format_money(listing.asking_price)),
        ListingField::Listed => format!("Day {}", listing.listed_day),
        ListingField::Status => {
            if exposure_left > 0 {
                format!(
                    "Public for {exposure_left} more day{}",
                    if exposure_left == 1 { "" } else { "s" }
                )
            } else {
                "Open to investors".into()
            }
        }
    }
}

/// The Hall prices the permit for THIS purchaser when the order lands; if the
/// fee moved since the card was drawn, the Hall answers with the exact figure,
/// the card's price and button rebind, and one more press confirms.
#[allow(clippy::too_many_arguments)]
fn permit_card_values(
    hall: Entity,
    settlement: SettlementId,
    opportunity: PermitMarketOpportunity,
    policy: Option<&SettlementPolicies>,
    quote: Option<&shared::protocol::HeroPermitQuote>,
    has_hero: bool,
    hero_nearby: bool,
    hero_balance: u64,
    acting_company: Option<&ActingCompanyView>,
    price_context: &PriceContext,
) -> PermitCardValues {
    let kind = opportunity.kind;
    let estimate = offer_price_for(opportunity, policy, price_context);
    let business_permit = kind != SettlementBuildingKind::House;
    let selected_company = business_permit
        .then(|| acting_company.map(|company| company.id))
        .flatten();
    // The Hall's own answer for this purchaser outranks the local estimate.
    let exact = quote.filter(|quote| {
        quote.settlement == settlement && quote.kind == kind && quote.company == selected_company
    });
    let fee = exact.map_or(estimate, |quote| quote.fee);
    let price_text = if fee == 0 {
        "Free".to_string()
    } else {
        format!("{} coin", format_money(fee))
    };
    let demand_text = format!(
        "{} / {}",
        signal_label(opportunity.score),
        opportunity.score
    );
    let privately_available = kind.minimum_player_permit_tier().is_some();
    let available = if business_permit {
        acting_company.map_or(0, |company| company.cash)
    } else {
        hero_balance
    };
    let (action_label, purchase): (String, Option<PurchasePermitButton>) = if !privately_available {
        ("NOT FOR SALE".into(), None)
    } else if !has_hero {
        ("CREATE A HERO FIRST".into(), None)
    } else if !hero_nearby {
        ("WALK TO THE HALL TO BUY".into(), None)
    } else if business_permit && acting_company.is_none() {
        ("FOUND A COMPANY FIRST".into(), None)
    } else if available < fee {
        (
            format!("NEED {} MORE COIN", format_money(fee - available)),
            None,
        )
    } else {
        let verb = if kind == SettlementBuildingKind::House {
            "CLAIM PLOT"
        } else {
            "BUY"
        };
        let price = if fee == 0 {
            "FREE".to_string()
        } else {
            format!("{} COIN", format_money(fee))
        };
        let label = if exact.is_some_and(|quote| quote.fee != estimate) {
            format!("PRICE CHANGED  /  {verb} FOR {price}")
        } else {
            format!("{verb}  /  {price}")
        };
        (
            label,
            Some(PurchasePermitButton {
                hall,
                kind,
                company: selected_company,
                fee,
            }),
        )
    };
    PermitCardValues {
        price_text,
        demand_text,
        action_label,
        enabled: purchase.is_some(),
        purchase,
    }
}

/// What the client can see of the Hall's person-specific pricing inputs, so
/// the price printed on a card is the price charged. Buildings still under
/// construction are not replicated; when one changes the fee, the Hall
/// answers a purchase with the exact figure and the card asks once more.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct PriceContext {
    personal_holdings: usize,
    company_holdings: usize,
    personal_kinds: Vec<SettlementBuildingKind>,
    company_kinds: Vec<SettlementBuildingKind>,
}

#[derive(SystemParam)]
struct PropertyHoldings<'w, 's> {
    buildings: Query<
        'w,
        's,
        (
            &'static SettlementBuilding,
            &'static BuildingOf,
            Option<&'static OperatedBy>,
            Option<&'static OwnedBy>,
        ),
    >,
}

impl PropertyHoldings<'_, '_> {
    /// Mirrors the server's `holding_count`: completed buildings plus
    /// stamped-but-unplaced permits, per settlement, per purchaser.
    fn context(
        &self,
        settlement: SettlementId,
        person: Option<PersonId>,
        company: Option<CompanyId>,
        ledger: Option<&PlayerPermitLedger>,
    ) -> PriceContext {
        let mut context = PriceContext::default();
        for (building, building_of, operated_by, owned_by) in self.buildings.iter() {
            if building_of.0 != settlement {
                continue;
            }
            if company.is_some() && operated_by.map(|operator| operator.0) == company {
                context.company_holdings += 1;
                context.company_kinds.push(building.kind);
            }
            if person.is_some() && owned_by.map(|owner| owner.0) == person {
                context.personal_holdings += 1;
                context.personal_kinds.push(building.kind);
            }
        }
        for permit in ledger.map_or(&[][..], |ledger| ledger.permits.as_slice()) {
            if permit.settlement != settlement {
                continue;
            }
            if permit.company.is_none() {
                context.personal_holdings += 1;
            } else if permit.company == company {
                context.company_holdings += 1;
            }
        }
        context
    }
}

#[derive(Resource, Default)]
pub(crate) struct PropertyClickGuard(pub bool);

/// The board is rebuilt only when its STRUCTURE changes (which settlement,
/// which tab, which cards exist, whether an acting company exists). Values
/// inside that structure are bound in place by [`sync_property_panel`].
#[derive(Component)]
struct PropertyPanelRoot {
    structure: String,
    target: Entity,
}

#[derive(Component)]
struct PropertyBackdrop;

#[derive(Component)]
struct PropertyPanel;

#[derive(Component)]
struct PropertyCloseButton;

#[derive(Component)]
struct PermitListingViewport;

#[derive(Component)]
struct PropertyListingViewport;

#[derive(Component)]
struct CycleActingCompany(i8);

/// The only founding-adjacent control this board still owns: the founding
/// form itself lives in `company_founding`.
#[cfg(test)]
type CompanyContextControl = With<CycleActingCompany>;

#[derive(Clone, Debug, PartialEq, Eq)]
struct ActingCompanyView {
    id: CompanyId,
    name: String,
    cash: u64,
}

#[cfg(test)]
fn offer_price(opportunity: PermitMarketOpportunity, policy: Option<&SettlementPolicies>) -> u64 {
    offer_price_for(opportunity, policy, &PriceContext::default())
}

/// The fee the Hall will charge THIS purchaser, computed with the same shared
/// formula and the purchaser's visible holdings.
fn offer_price_for(
    opportunity: PermitMarketOpportunity,
    policy: Option<&SettlementPolicies>,
    context: &PriceContext,
) -> u64 {
    let personal = opportunity.kind == SettlementBuildingKind::House;
    let (holdings, kinds) = if personal {
        (context.personal_holdings, &context.personal_kinds)
    } else {
        (context.company_holdings, &context.company_kinds)
    };
    // A new-entrant discount is for a NEW entrant: an owner of this kind in
    // this settlement pays full price (mirrors the server's rule).
    let subsidized = opportunity.subsidized
        && !(opportunity.requires_independent_owner && kinds.contains(&opportunity.kind));
    player_permit_price_with_subsidy(
        opportunity.kind,
        holdings,
        subsidized,
        policy.map_or(0, |policy| policy.business_permit_subsidy_bps),
    )
}

fn signal_label(score: u8) -> &'static str {
    match score {
        90.. => "URGENT",
        60..=89 => "REQUESTED",
        35..=59 => "PROMISING",
        _ => "SPECULATIVE",
    }
}

fn permit_description(kind: SettlementBuildingKind) -> &'static str {
    match kind {
        SettlementBuildingKind::House => "Shelter for one resident household.",
        SettlementBuildingKind::Farmstead => "Produces Wheat from two nearby crop fields.",
        SettlementBuildingKind::FishermansHut => "Produces fish from a reachable shoreline.",
        SettlementBuildingKind::LumberjackHut => "Harvests Wood efficiently from nearby forest.",
        SettlementBuildingKind::StoneQuarry => "Extracts Stone from rocky ground.",
        SettlementBuildingKind::LivestockFarm => {
            "Raises grazing animals for edible Meat and textile-ready Wool."
        }
        SettlementBuildingKind::Windmill => "Purchases Wheat and mills it into Flour.",
        SettlementBuildingKind::Bakery => "Purchases Flour and bakes filling Bread.",
        SettlementBuildingKind::StorageHall => {
            "Stores local company goods and employs private porters."
        }
        SettlementBuildingKind::Market => "A permanent local exchange and warehouse.",
        SettlementBuildingKind::Tavern => "Food, drink and social services for the settlement.",
        SettlementBuildingKind::Church => "A civic service building for a mature settlement.",
        SettlementBuildingKind::Hall => "The settlement's civic centre.",
    }
}

fn handle_company_context_buttons(
    mouse: Res<ButtonInput<MouseButton>>,
    guard: Res<PropertyClickGuard>,
    buttons: Query<(&Interaction, &CycleActingCompany), Changed<Interaction>>,
    local: Option<Res<LocalPeerId>>,
    heroes: Query<(&Hero, &PersonId)>,
    companies: Query<(&CompanyId, &CompanyLeadership)>,
    mut active: ResMut<ActiveCompany>,
) {
    if !guard.0 || !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    let Some(person) = local.as_ref().and_then(|local| {
        heroes
            .iter()
            .find(|(hero, _)| shared::player::peer_id_to_u64(hero.owner) == local.0)
            .map(|(_, person)| *person)
    }) else {
        return;
    };
    let mut mastered: Vec<_> = companies
        .iter()
        .filter_map(|(id, leadership)| (leadership.master == person).then_some(*id))
        .collect();
    mastered.sort_unstable();
    for (interaction, CycleActingCompany(step)) in buttons.iter() {
        if *interaction != Interaction::Pressed {
            continue;
        }
        if mastered.is_empty() {
            active.0 = None;
            continue;
        }
        let current = active
            .0
            .and_then(|selected| mastered.iter().position(|id| *id == selected))
            .unwrap_or(0);
        let next =
            (current as isize + isize::from(*step)).rem_euclid(mastered.len() as isize) as usize;
        active.0 = Some(mastered[next]);
    }
}

#[allow(clippy::too_many_arguments)]
fn sync_property_panel(
    mut commands: Commands,
    mut target: ResMut<PropertyMarketTarget>,
    quote: Res<PendingPermitQuote>,
    mut active_company: ResMut<ActiveCompany>,
    founding_feedback: Res<CompanyFoundingFeedback>,
    local: Option<Res<LocalPeerId>>,
    settlements: Query<(
        &SettlementId,
        &Settlement,
        &PlayerPosition,
        Option<&CivicHallLevel>,
        Option<&SettlementOpportunityBoard>,
        Option<&SettlementPropertyBoard>,
        Option<&SettlementPolicies>,
    )>,
    heroes: Query<(
        &Hero,
        &PersonId,
        &CharacterName,
        &PlayerPosition,
        Option<&Wallet>,
        Option<&PlayerPermitLedger>,
    )>,
    companies: Query<(&CompanyId, &Company, &CompanyLeadership, &CompanyAccount)>,
    world_time: Query<&WorldTime>,
    mut ui: PropertyPanelUi,
    holdings: PropertyHoldings,
) {
    let mut _ui_scope = ui.perf.scope("sync_property_panel");
    let Some(entity) = target.0 else {
        for (root, _) in ui.roots.iter() {
            commands.entity(root).despawn();
        }
        return;
    };
    let Ok((
        settlement_id,
        settlement,
        hall_position,
        hall_level,
        opportunities,
        properties,
        policy,
    )) = settlements.get(entity)
    else {
        for (root, _) in ui.roots.iter() {
            commands.entity(root).despawn();
        }
        target.0 = None;
        return;
    };
    let day = world_time.iter().next().map_or(0, |time| time.day);
    let permit_offers = opportunities.map_or(&[][..], |board| board.opportunities.as_slice());
    let property_listings = properties.map_or(&[][..], |board| board.listings.as_slice());
    let hall_level = hall_level
        .copied()
        .unwrap_or_else(|| CivicHallLevel::for_tier(settlement.tier));
    let local_hero = local.as_ref().and_then(|local| {
        heroes
            .iter()
            .find(|(hero, ..)| shared::player::peer_id_to_u64(hero.owner) == local.0)
    });
    let hero_balance = local_hero
        .and_then(|(_, _, _, _, wallet, _)| wallet)
        .map_or(0, |wallet| wallet.balance());
    let hero_nearby = local_hero.is_some_and(|(_, _, _, position, _, _)| {
        Vec2::new(position.0.x, position.0.z)
            .distance(Vec2::new(hall_position.0.x, hall_position.0.z))
            <= 12.0
    });
    let has_hero = local_hero.is_some();
    let local_person = local_hero.map(|(_, person, ..)| *person);
    let mut mastered_companies: Vec<_> = local_person.map_or_else(Vec::new, |person| {
        companies
            .iter()
            .filter(|(_, _, leadership, _)| leadership.master == person)
            .map(|(id, company, _, account)| ActingCompanyView {
                id: *id,
                name: company.name.clone(),
                cash: account.cash,
            })
            .collect()
    });
    mastered_companies.sort_by_key(|company| company.id);
    if !mastered_companies.is_empty()
        && active_company.0.is_none_or(|selected| {
            !mastered_companies
                .iter()
                .any(|company| company.id == selected)
        })
    {
        active_company.0 = mastered_companies.first().map(|company| company.id);
    } else if mastered_companies.is_empty() && !founding_feedback.success {
        active_company.0 = None;
    }
    let acting_company = active_company.0.and_then(|selected| {
        mastered_companies
            .iter()
            .find(|company| company.id == selected)
    });
    let ledger = local_hero.and_then(|(.., ledger)| ledger);
    let price_context = holdings.context(
        *settlement_id,
        local_person,
        acting_company.map(|company| company.id),
        ledger,
    );
    let tab = *ui.tab;

    // Structure: what exists. Values live in the bound widgets below.
    let structure = format!(
        "{entity:?}|{}|{}|{}|{tab:?}|{has_hero}|{:?}|{}|{:?}|{:?}|{}|{}",
        settlement.name,
        settlement.tier.label(),
        hall_level.label(),
        acting_company.map(|company| company.id),
        mastered_companies.len(),
        permit_offers
            .iter()
            .map(|offer| (
                offer.kind,
                offer.subsidized,
                offer.requires_independent_owner
            ))
            .collect::<Vec<_>>(),
        property_listings
            .iter()
            .map(|listing| (
                listing.kind,
                listing.listed_day,
                listing.stage,
                listing.reason
            ))
            .collect::<Vec<_>>(),
        founding_feedback.message,
        founding_feedback.success,
    );
    let quote = quote.0.as_ref();
    let mut card_values: HashMap<SettlementBuildingKind, PermitCardValues> = HashMap::default();
    for opportunity in permit_offers {
        card_values.insert(
            opportunity.kind,
            permit_card_values(
                entity,
                *settlement_id,
                *opportunity,
                policy,
                quote,
                has_hero,
                hero_nearby,
                hero_balance,
                acting_company,
                &price_context,
            ),
        );
    }

    let built = ui
        .roots
        .iter()
        .any(|(_, root)| root.target == entity && root.structure == structure);
    if !built {
        for (root, _) in ui.roots.iter() {
            commands.entity(root).despawn();
        }
        _ui_scope.rebuilt();
        let nodes = spawn_modal(
            &mut commands,
            PropertyPanelRoot {
                structure,
                target: entity,
            },
            PropertyBackdrop,
            PropertyPanel,
            ModalLayout {
                panel_size: Vec2::new(960.0, 700.0),
                panel_padding: 0.0,
            },
        );
        commands.entity(nodes.panel).insert((
            Node {
                width: Val::Vw(90.0),
                max_width: Val::Px(960.0),
                height: Val::Vh(86.0),
                max_height: Val::Px(700.0),
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
            spawn_header(
                panel,
                &settlement.name,
                settlement.tier.label(),
                hall_level.label(),
            );
            spawn_company_context(
                panel,
                &mastered_companies,
                acting_company,
                &founding_feedback,
                has_hero,
            );
            spawn_tab_bar(panel, tab, permit_offers.len(), property_listings.len());
            panel
                .spawn(Node {
                    flex_grow: 1.0,
                    min_height: Val::Px(0.0),
                    flex_direction: FlexDirection::Column,
                    padding: UiRect::axes(Val::Px(22.0), Val::Px(14.0)),
                    ..default()
                })
                .with_children(|body| match tab {
                    PropertyMarketTab::Permits => {
                        spawn_permit_list(body, permit_offers, &card_values)
                    }
                    PropertyMarketTab::ForSale => spawn_property_list(body, property_listings, day),
                });
        });
        return;
    }

    // Bind: write only what changed, into the widgets that already exist.
    let acting_text = acting_company.map(acting_company_text);
    for (mut text, (price, demand, label, listing, acting, tab_count)) in ui.texts.iter_mut() {
        let next = if let Some(PermitPriceText(kind)) = price {
            card_values
                .get(kind)
                .map(|values| values.price_text.clone())
        } else if let Some(PermitDemandText(kind)) = demand {
            card_values
                .get(kind)
                .map(|values| values.demand_text.clone())
        } else if let Some(PermitActionLabel(kind)) = label {
            card_values
                .get(kind)
                .map(|values| values.action_label.clone())
        } else if let Some(fact) = listing {
            property_listings
                .get(fact.index)
                .map(|listing| listing_fact(listing, day, fact.field))
        } else if acting.is_some() {
            acting_text.clone()
        } else if let Some(PropertyTabCount(which)) = tab_count {
            Some(tab_label(
                *which,
                permit_offers.len(),
                property_listings.len(),
            ))
        } else {
            None
        };
        if let Some(next) = next {
            if text.0 != next {
                text.0 = next;
            }
        }
    }
    for (button, PermitActionButton(kind), mut style, disabled, purchase) in ui.actions.iter_mut() {
        let Some(values) = card_values.get(kind) else {
            continue;
        };
        let variant = if values.enabled {
            UiButtonVariant::Primary
        } else {
            UiButtonVariant::Secondary
        };
        if style.variant != variant {
            style.variant = variant;
        }
        if values.enabled == disabled {
            if values.enabled {
                commands.entity(button).remove::<InteractionDisabled>();
            } else {
                commands.entity(button).insert(InteractionDisabled);
            }
        }
        if purchase.copied() != values.purchase {
            match values.purchase {
                Some(purchase) => {
                    commands.entity(button).insert(purchase);
                }
                None => {
                    commands.entity(button).remove::<PurchasePermitButton>();
                }
            }
        }
    }
}

fn spawn_header(panel: &mut ChildSpawnerCommands<'_>, place: &str, tier: &str, hall: &str) {
    panel
        .spawn((
            Node {
                width: Val::Percent(100.0),
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                padding: UiRect::axes(Val::Px(22.0), Val::Px(16.0)),
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
                    row_gap: Val::Px(3.0),
                    ..default()
                })
                .with_children(|copy| {
                    copy.spawn((
                        Text::new(format!("{} LAND & PROPERTY", place.to_uppercase())),
                        TextFont {
                            font_size: FontSize::Px(T_TITLE),
                            ..default()
                        },
                        TextColor(INK),
                    ));
                    copy.spawn((
                        Text::new(format!(
                            "{}  /  {}",
                            tier.to_uppercase(),
                            hall.to_uppercase()
                        )),
                        TextFont {
                            font_size: FontSize::Px(T_BODY),
                            ..default()
                        },
                        TextColor(INK_MUTED),
                    ));
                });
            header
                .spawn((
                    PropertyCloseButton,
                    Button,
                    Node {
                        width: Val::Px(38.0),
                        height: Val::Px(38.0),
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
                        font_size: FontSize::Px(T_BUTTON),
                        ..default()
                    },
                    TextColor(INK),
                    Pickable::IGNORE,
                ));
        });
}

#[allow(clippy::too_many_arguments)]
fn spawn_company_context(
    panel: &mut ChildSpawnerCommands<'_>,
    mastered: &[ActingCompanyView],
    acting: Option<&ActingCompanyView>,
    feedback: &CompanyFoundingFeedback,
    has_hero: bool,
) {
    panel
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_shrink: 0.0,
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(7.0),
                padding: UiRect::axes(Val::Px(22.0), Val::Px(10.0)),
                border: UiRect::bottom(Val::Px(1.0)),
                ..default()
            },
            BackgroundColor(LIMEWASH_WELL),
            BorderColor::all(PLATE_RULE_SOFT),
        ))
        .with_children(|context| {
            if !has_hero {
                context.spawn((
                    Text::new("Create a Hero to act for a company."),
                    TextFont {
                        font_size: FontSize::Px(T_BODY),
                        ..default()
                    },
                    TextColor(INK_MUTED),
                ));
                return;
            }
            if let Some(acting) = acting {
                context
                    .spawn(Node {
                        width: Val::Percent(100.0),
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(8.0),
                        ..default()
                    })
                    .with_children(|row| {
                        row.spawn((
                            Text::new("ACTING AS"),
                            TextFont {
                                font_size: FontSize::Px(T_LABEL),
                                ..default()
                            },
                            TextColor(INK_MUTED),
                        ));
                        if mastered.len() > 1 {
                            context_button(row, CycleActingCompany(-1), "<");
                        }
                        row.spawn((
                            ActingCompanyText,
                            Text::new(acting_company_text(acting)),
                            TextFont {
                                font_size: FontSize::Px(T_BUTTON),
                                ..default()
                            },
                            TextColor(INK),
                            Node {
                                flex_grow: 1.0,
                                ..default()
                            },
                        ));
                        if mastered.len() > 1 {
                            context_button(row, CycleActingCompany(1), ">");
                            row.spawn((
                                Text::new(format!("{} companies", mastered.len())),
                                TextFont {
                                    font_size: FontSize::Px(T_LABEL),
                                    ..default()
                                },
                                TextColor(INK_MUTED),
                            ));
                        }
                        context_button(row, OpenCompanyFounding, "NEW COMPANY");
                    });
            } else {
                // No company to act for: offer the one door to the founding
                // screen, nothing else.
                context
                    .spawn(Node {
                        width: Val::Percent(100.0),
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(12.0),
                        ..default()
                    })
                    .with_children(|row| {
                        row.spawn((
                            Text::new("ACTING AS  /  no company yet"),
                            TextFont {
                                font_size: FontSize::Px(T_BUTTON),
                                ..default()
                            },
                            TextColor(INK_MUTED),
                            Node {
                                flex_grow: 1.0,
                                ..default()
                            },
                        ));
                        context_button(row, OpenCompanyFounding, "CREATE A COMPANY");
                    });
            }
            if !feedback.message.is_empty() {
                context.spawn((
                    Text::new(feedback.message.clone()),
                    TextFont {
                        font_size: FontSize::Px(T_BODY),
                        ..default()
                    },
                    TextColor(if feedback.success {
                        Color::srgb(0.20, 0.48, 0.27)
                    } else {
                        Color::srgb(0.70, 0.20, 0.16)
                    }),
                ));
            }
        });
}

fn context_button(
    parent: &mut ChildSpawnerCommands<'_>,
    marker: impl Component,
    label: impl Into<String>,
) {
    parent
        .spawn((
            marker,
            Button,
            Node {
                min_height: Val::Px(38.0),
                padding: UiRect::axes(Val::Px(12.0), Val::Px(8.0)),
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
                font_size: FontSize::Px(T_BUTTON),
                ..default()
            },
            TextColor(INK),
            Pickable::IGNORE,
        ));
}

fn spawn_tab_bar(
    panel: &mut ChildSpawnerCommands<'_>,
    tab: PropertyMarketTab,
    permits: usize,
    listings: usize,
) {
    panel
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_shrink: 0.0,
                column_gap: Val::Px(4.0),
                padding: UiRect::horizontal(Val::Px(22.0)),
                border: UiRect::bottom(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(PLATE_RULE_SOFT),
        ))
        .with_children(|bar| {
            for target in [PropertyMarketTab::Permits, PropertyMarketTab::ForSale] {
                let label = tab_label(target, permits, listings);
                let selected = target == tab;
                bar.spawn((
                    PropertyTabButton(target),
                    Button,
                    // Hovering a tab must not freeze the board's refresh.
                    UiRefreshExempt,
                    Node {
                        min_height: Val::Px(44.0),
                        padding: UiRect::axes(Val::Px(20.0), Val::Px(10.0)),
                        justify_content: JustifyContent::Center,
                        align_items: AlignItems::Center,
                        border: UiRect::bottom(Val::Px(3.0)),
                        ..default()
                    },
                    selected_button_chrome(UiButtonVariant::Tab, selected),
                ))
                .with_child((
                    PropertyTabCount(target),
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
        });
}

fn spawn_permit_list(
    body: &mut ChildSpawnerCommands<'_>,
    opportunities: &[PermitMarketOpportunity],
    card_values: &HashMap<SettlementBuildingKind, PermitCardValues>,
) {
    body.spawn((
        PermitListingViewport,
        Node {
            flex_grow: 1.0,
            min_height: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(10.0),
            padding: UiRect::right(Val::Px(8.0)),
            overflow: Overflow::scroll_y(),
            scrollbar_width: 8.0,
            ..default()
        },
    ))
    .with_children(|list| {
        if opportunities.is_empty() {
            spawn_empty_state(list, "No permits for sale right now.");
        } else {
            for opportunity in opportunities {
                if let Some(values) = card_values.get(&opportunity.kind) {
                    spawn_permit_card(list, *opportunity, values);
                }
            }
        }
    });
}

fn spawn_property_list(
    body: &mut ChildSpawnerCommands<'_>,
    listings: &[PropertyMarketListing],
    day: u32,
) {
    body.spawn((
        PropertyListingViewport,
        Node {
            flex_grow: 1.0,
            min_height: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(10.0),
            padding: UiRect::right(Val::Px(8.0)),
            overflow: Overflow::scroll_y(),
            scrollbar_width: 8.0,
            ..default()
        },
    ))
    .with_children(|list| {
        if listings.is_empty() {
            spawn_empty_state(list, "No buildings for sale right now.");
        } else {
            for (index, listing) in listings.iter().enumerate() {
                spawn_property_card(list, index, listing, day);
            }
        }
    });
}

/// Spawn a card with its bound widgets; the values passed here are the same
/// ones [`sync_property_panel`] keeps writing afterwards.
fn spawn_permit_card(
    parent: &mut ChildSpawnerCommands<'_>,
    opportunity: PermitMarketOpportunity,
    values: &PermitCardValues,
) {
    let kind = opportunity.kind;
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_shrink: 0.0,
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(9.0),
                padding: UiRect::all(Val::Px(14.0)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(RADIUS)),
                ..default()
            },
            BackgroundColor(LIMEWASH),
            BorderColor::all(PLATE_RULE_SOFT),
        ))
        .with_children(|card| {
            card.spawn(Node {
                width: Val::Percent(100.0),
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                column_gap: Val::Px(10.0),
                ..default()
            })
            .with_children(|heading| {
                heading.spawn((
                    Text::new(kind.label().to_uppercase()),
                    TextFont {
                        font_size: FontSize::Px(T_HEADING),
                        ..default()
                    },
                    TextColor(INK),
                ));
                let badge = if kind == SettlementBuildingKind::House {
                    Some("HOME")
                } else if opportunity.requires_independent_owner {
                    Some("NEW OWNER DISCOUNT")
                } else if opportunity.subsidized {
                    Some("DISCOUNT")
                } else {
                    None
                };
                if let Some(badge) = badge {
                    spawn_badge(heading, badge);
                }
            });
            card.spawn((
                Text::new(permit_description(kind)),
                TextFont {
                    font_size: FontSize::Px(T_BODY),
                    ..default()
                },
                TextColor(INK_MUTED),
            ));
            card.spawn(Node {
                width: Val::Percent(100.0),
                column_gap: Val::Px(28.0),
                ..default()
            })
            .with_children(|facts| {
                spawn_bound_fact(
                    facts,
                    "PRICE",
                    values.price_text.clone(),
                    PermitPriceText(kind),
                );
                spawn_bound_fact(
                    facts,
                    "DEMAND",
                    values.demand_text.clone(),
                    PermitDemandText(kind),
                );
                spawn_inline_fact(facts, "WOOD", kind.construction_wood_required().to_string());
                let capacity = if kind.housing_capacity() > 0 {
                    format!("{} beds", kind.housing_capacity())
                } else {
                    format!("{} jobs", kind.positions())
                };
                spawn_inline_fact(facts, "CAPACITY", capacity);
            });
            spawn_permit_action(card, kind, values);
        });
}

/// One big button that says what happens when you press it. Its label,
/// enabled state and purchase order are bound: they follow the live price
/// without the card ever being rebuilt under the pointer.
fn spawn_permit_action(
    card: &mut ChildSpawnerCommands<'_>,
    kind: SettlementBuildingKind,
    values: &PermitCardValues,
) {
    let mut button = card.spawn((
        PermitActionButton(kind),
        Button,
        Node {
            width: Val::Percent(100.0),
            min_height: Val::Px(46.0),
            margin: UiRect::top(Val::Px(4.0)),
            padding: UiRect::axes(Val::Px(14.0), Val::Px(10.0)),
            justify_content: JustifyContent::Center,
            align_items: AlignItems::Center,
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(RADIUS)),
            ..default()
        },
        button_chrome(if values.enabled {
            UiButtonVariant::Primary
        } else {
            UiButtonVariant::Secondary
        }),
    ));
    if !values.enabled {
        button.insert(InteractionDisabled);
    }
    if let Some(purchase) = values.purchase {
        button.insert(purchase);
    }
    button.with_child((
        PermitActionLabel(kind),
        Text::new(values.action_label.clone()),
        UiButtonLabel,
        TextFont {
            font_size: FontSize::Px(T_BUTTON),
            ..default()
        },
        TextColor(if values.enabled { INK } else { INK_MUTED }),
        Pickable::IGNORE,
    ));
}

fn spawn_property_card(
    parent: &mut ChildSpawnerCommands<'_>,
    index: usize,
    listing: &PropertyMarketListing,
    day: u32,
) {
    parent
        .spawn((
            Node {
                width: Val::Percent(100.0),
                flex_shrink: 0.0,
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(9.0),
                padding: UiRect::all(Val::Px(14.0)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(RADIUS)),
                ..default()
            },
            BackgroundColor(LIMEWASH),
            BorderColor::all(PLATE_RULE_SOFT),
        ))
        .with_children(|card| {
            card.spawn(Node {
                width: Val::Percent(100.0),
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                column_gap: Val::Px(10.0),
                ..default()
            })
            .with_children(|heading| {
                heading.spawn((
                    Text::new(listing.kind.label().to_uppercase()),
                    TextFont {
                        font_size: FontSize::Px(T_HEADING),
                        ..default()
                    },
                    TextColor(INK),
                ));
                spawn_badge(heading, listing.stage.label().to_uppercase().as_str());
            });
            card.spawn((
                Text::new(format!("Listed: {}", listing.reason.label())),
                TextFont {
                    font_size: FontSize::Px(T_BODY),
                    ..default()
                },
                TextColor(INK_MUTED),
            ));
            card.spawn(Node {
                width: Val::Percent(100.0),
                column_gap: Val::Px(28.0),
                ..default()
            })
            .with_children(|facts| {
                for (label, field) in [
                    ("ASKING", ListingField::Asking),
                    ("LISTED", ListingField::Listed),
                    ("STATUS", ListingField::Status),
                ] {
                    spawn_bound_fact(
                        facts,
                        label,
                        listing_fact(listing, day, field),
                        ListingFactText { index, field },
                    );
                }
            });
        });
}

fn spawn_badge(parent: &mut ChildSpawnerCommands<'_>, label: &str) {
    parent
        .spawn((
            Node {
                padding: UiRect::axes(Val::Px(9.0), Val::Px(5.0)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(RADIUS)),
                ..default()
            },
            BackgroundColor(LIMEWASH_WELL),
            BorderColor::all(PLATE_RULE_SOFT),
        ))
        .with_child((
            Text::new(label),
            TextFont {
                font_size: FontSize::Px(T_LABEL),
                ..default()
            },
            TextColor(INK),
        ));
}

fn spawn_inline_fact(parent: &mut ChildSpawnerCommands<'_>, label: &str, value: String) {
    spawn_bound_fact(parent, label, value, ());
}

/// A label over a value; `marker` goes on the value's Text so a bind system
/// can keep it current.
fn spawn_bound_fact(
    parent: &mut ChildSpawnerCommands<'_>,
    label: &str,
    value: String,
    marker: impl Bundle,
) {
    parent
        .spawn(Node {
            flex_direction: FlexDirection::Column,
            row_gap: Val::Px(2.0),
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

fn spawn_empty_state(parent: &mut ChildSpawnerCommands<'_>, message: &str) {
    parent.spawn((
        Text::new(message),
        TextFont {
            font_size: FontSize::Px(T_VALUE),
            ..default()
        },
        TextColor(INK_MUTED),
        Node {
            width: Val::Percent(100.0),
            padding: UiRect::all(Val::Px(18.0)),
            border: UiRect::all(Val::Px(1.0)),
            border_radius: BorderRadius::all(Val::Px(RADIUS)),
            ..default()
        },
        BackgroundColor(LIMEWASH),
        BorderColor::all(PLATE_RULE_SOFT),
    ));
}

fn update_property_guard(
    target: Res<PropertyMarketTarget>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut guard: ResMut<PropertyClickGuard>,
) {
    update_modal_click_guard(target.0.is_some(), &mouse, &mut guard.0);
}

fn handle_property_close(
    keyboard: Res<ButtonInput<KeyCode>>,
    mouse: Res<ButtonInput<MouseButton>>,
    guard: Res<PropertyClickGuard>,
    backdrop: Query<&Interaction, (With<PropertyBackdrop>, Changed<Interaction>)>,
    close: Query<&Interaction, (With<PropertyCloseButton>, Changed<Interaction>)>,
    mut target: ResMut<PropertyMarketTarget>,
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

fn handle_property_tab_buttons(
    mouse: Res<ButtonInput<MouseButton>>,
    guard: Res<PropertyClickGuard>,
    mut tab: ResMut<PropertyMarketTab>,
    buttons: Query<(&Interaction, &PropertyTabButton), Changed<Interaction>>,
) {
    if !guard.0 || !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    for (interaction, PropertyTabButton(target)) in buttons.iter() {
        if *interaction == Interaction::Pressed && *tab != *target {
            *tab = *target;
        }
    }
}

fn despawn_property_panel(
    mut commands: Commands,
    roots: Query<Entity, With<PropertyPanelRoot>>,
    mut target: ResMut<PropertyMarketTarget>,
    mut active: ResMut<ActiveCompany>,
    mut founding: ResMut<CompanyFoundingDraft>,
    mut feedback: ResMut<CompanyFoundingFeedback>,
) {
    for root in roots.iter() {
        commands.entity(root).despawn();
    }
    target.0 = None;
    active.0 = None;
    *founding = default();
    *feedback = default();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn company_context_styling_cannot_capture_an_unrelated_backdrop() {
        let mut world = World::new();
        let backdrop = world
            .spawn((
                PropertyBackdrop,
                Interaction::Hovered,
                BackgroundColor::default(),
            ))
            .id();
        let company_control = world
            .spawn((
                CycleActingCompany(1),
                Interaction::Hovered,
                BackgroundColor::default(),
            ))
            .id();

        let mut controls = world.query_filtered::<Entity, CompanyContextControl>();
        let matches: Vec<_> = controls.iter(&world).collect();
        assert_eq!(matches, vec![company_control]);
        assert!(!matches.contains(&backdrop));
    }

    #[test]
    fn requested_business_uses_enacted_discount_but_speculation_does_not() {
        let mut policy = SettlementPolicies::from_foundation("Market Cross", Vec3::ZERO);
        policy.business_permit_subsidy_bps = 4_500;
        let requested = PermitMarketOpportunity {
            kind: SettlementBuildingKind::Farmstead,
            score: 80,
            subsidized: true,
            requires_independent_owner: false,
        };
        let speculative = PermitMarketOpportunity {
            subsidized: false,
            ..requested
        };
        assert!(offer_price(requested, Some(&policy)) < offer_price(speculative, Some(&policy)));
    }

    #[test]
    fn permit_demand_labels_have_stable_player_facing_bands() {
        assert_eq!(signal_label(95), "URGENT");
        assert_eq!(signal_label(60), "REQUESTED");
        assert_eq!(signal_label(35), "PROMISING");
        assert_eq!(signal_label(5), "SPECULATIVE");
    }

    #[test]
    fn permit_and_property_columns_scroll_independently() {
        let mut world = World::new();
        let root = world.spawn_empty().id();
        world.commands().entity(root).with_children(|body| {
            spawn_permit_list(body, &[], &HashMap::default());
            spawn_property_list(body, &[], 0);
        });
        world.flush();

        let mut permits = world.query_filtered::<&Node, With<PermitListingViewport>>();
        let permit = permits.single(&world).unwrap();
        assert_eq!(permit.min_height, Val::Px(0.0));
        assert_eq!(permit.overflow.y, OverflowAxis::Scroll);

        let mut properties = world.query_filtered::<&Node, With<PropertyListingViewport>>();
        let property = properties.single(&world).unwrap();
        assert_eq!(property.min_height, Val::Px(0.0));
        assert_eq!(property.overflow.y, OverflowAxis::Scroll);
    }

    #[test]
    fn nearby_exact_quote_creates_the_matching_purchase_action() {
        let mut world = World::new();
        let hall = world.spawn_empty().id();
        let root = world.spawn_empty().id();
        let quote = shared::protocol::HeroPermitQuote {
            settlement: SettlementId(4),
            settlement_name: "Oakfell".into(),
            kind: SettlementBuildingKind::Farmstead,
            fee: 300,
            recommended_working_capital: 500,
            wallet_balance: 1_000,
            company: Some(CompanyId(9)),
            company_cash: 1_000,
        };
        let acting = ActingCompanyView {
            id: CompanyId(9),
            name: "Oakfell Farms".into(),
            cash: 1_000,
        };
        let opportunity = PermitMarketOpportunity {
            kind: SettlementBuildingKind::Farmstead,
            score: 73,
            subsidized: true,
            requires_independent_owner: false,
        };
        let values = permit_card_values(
            hall,
            SettlementId(4),
            opportunity,
            None,
            Some(&quote),
            true,
            true,
            1_000,
            Some(&acting),
            &PriceContext::default(),
        );
        world.commands().entity(root).with_children(|body| {
            spawn_permit_card(body, opportunity, &values);
        });
        world.flush();

        let mut buttons = world.query::<&PurchasePermitButton>();
        let purchase = buttons.single(&world).unwrap();
        assert_eq!(purchase.hall, hall);
        assert_eq!(purchase.kind, SettlementBuildingKind::Farmstead);
        assert_eq!(purchase.company, Some(CompanyId(9)));
        assert_eq!(purchase.fee, 300);
    }

    /// The quote ceremony is gone: a card with no Hall answer yet buys
    /// directly at the price it prints, which is the purchaser-aware estimate.
    #[test]
    fn a_card_without_a_quote_buys_directly_at_its_printed_price() {
        let mut world = World::new();
        let hall = world.spawn_empty().id();
        let root = world.spawn_empty().id();
        let acting = ActingCompanyView {
            id: CompanyId(3),
            name: "Brackwater Mills".into(),
            cash: 10_000,
        };
        let opportunity = PermitMarketOpportunity {
            kind: SettlementBuildingKind::Windmill,
            score: 50,
            subsidized: true,
            requires_independent_owner: true,
        };
        // Already operating a windmill here: the new-owner discount must not
        // apply, and a second holding costs more than a first.
        let context = PriceContext {
            company_holdings: 1,
            company_kinds: vec![SettlementBuildingKind::Windmill],
            ..default()
        };
        let expected = offer_price_for(opportunity, None, &context);
        assert!(expected > offer_price_for(opportunity, None, &PriceContext::default()));
        let values = permit_card_values(
            hall,
            SettlementId(2),
            opportunity,
            None,
            None,
            true,
            true,
            0,
            Some(&acting),
            &context,
        );
        world.commands().entity(root).with_children(|body| {
            spawn_permit_card(body, opportunity, &values);
        });
        world.flush();

        let mut buttons = world.query::<&PurchasePermitButton>();
        let purchase = buttons.single(&world).unwrap();
        assert_eq!(purchase.company, Some(CompanyId(3)));
        assert_eq!(purchase.fee, expected);
    }
}
