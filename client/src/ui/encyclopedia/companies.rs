//! Company directory, player portfolio and consolidated current ledgers.
//!
//! A workplace panel answers "how do I run this site?". This page answers the
//! wider questions: what firms exist, which ones do I own, who controls them,
//! where they operate, and whether the whole company is actually healthy.

use bevy::platform::collections::HashMap;
use bevy::prelude::*;

use lightyear::prelude::{Connected, MessageReceiver, MessageSender};
use shared::components::{
    BuildingId, BuildingOf, CharacterName, Company, CompanyId, CompanyLeadership, CompanyOwnership,
    CompanyShareMarket, Hero, OperatedBy, PersonId, SettlementBuilding, SettlementBuildingKind,
    SettlementId,
};
use shared::economy::{
    format_money, BusinessAccount, BusinessCondition, BusinessProcurementPolicy,
    BusinessSourcingMode, BusinessStaffingPolicy, BusinessState, BusinessSupplyPolicy,
    CompanyAccount, CompanyBranchPolicies, CompanyDayLedger, CompanyDecisionHistory,
    CompanyDecisionRecord, CompanyManagementPolicy, CompanyResourcePolicy, Good, GoodsInventory,
    Wallet,
};
use shared::protocol::{HeroCompanyAction, HeroCompanyOrder, HeroCompanyResult, ReliableChannel};

use super::*;
use crate::ui::styles::{
    ACCENT_COLOR, BUTTON_BORDER, BUTTON_NORMAL, INK, PLATE_RULE_SOFT, RADIUS, TEXT_COLOR,
    TEXT_MUTED,
};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompanyHolderRecord {
    pub person: PersonId,
    pub name: String,
    pub shares: u16,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompanyOfferRecord {
    pub seller: PersonId,
    pub seller_name: String,
    pub shares: u16,
    pub unit_price: u64,
    pub listed_day: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompanySiteRecord {
    pub entity: Entity,
    pub id: BuildingId,
    pub settlement: String,
    pub settlement_id: SettlementId,
    pub kind: SettlementBuildingKind,
    pub workers: usize,
    pub positions: u8,
    pub enabled_positions: u8,
    pub state: BusinessState,
    pub wage_arrears: u64,
    pub tax_arrears: u64,
    pub current_day: shared::economy::BusinessDayLedger,
    pub previous_day: shared::economy::BusinessDayLedger,
    pub output: Option<Good>,
    pub output_stock: u32,
    pub asking_price: Option<u64>,
    pub input: Option<Good>,
    pub input_stock: u32,
    pub input_target: u32,
    pub input_coverage_days: u8,
    pub sourcing: Option<BusinessSourcingMode>,
    pub preferred_supplier: Option<BuildingId>,
    pub goods: Vec<(Good, u32)>,
    pub used_bulk: u32,
    pub bulk_capacity: u32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompanyBranchRecord {
    pub settlement: String,
    pub settlement_id: SettlementId,
    pub sites: usize,
    pub storage_halls: usize,
    pub used_bulk: u32,
    pub bulk_capacity: u32,
    pub resources: Vec<(Good, u32, CompanyResourcePolicy)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CompanyRecord {
    pub id: CompanyId,
    pub name: String,
    pub founded_day: u32,
    pub master: PersonId,
    pub master_name: String,
    pub account: CompanyAccount,
    pub policy: CompanyManagementPolicy,
    pub holders: Vec<CompanyHolderRecord>,
    pub offers: Vec<CompanyOfferRecord>,
    pub decisions: Vec<CompanyDecisionRecord>,
    pub sites: Vec<CompanySiteRecord>,
    pub branches: Vec<CompanyBranchRecord>,
}

impl CompanyRecord {
    pub fn shares_owned_by(&self, person: PersonId) -> u16 {
        self.holders
            .iter()
            .find(|holder| holder.person == person)
            .map_or(0, |holder| holder.shares)
    }

    pub fn accounting_equity(&self) -> u64 {
        self.account
            .cash
            .saturating_add(self.account.book_value)
            .saturating_sub(self.account.wage_arrears)
            .saturating_sub(self.account.tax_arrears)
    }

    pub fn holding_book_interest(&self, shares: u16) -> u64 {
        self.accounting_equity().saturating_mul(u64::from(shares))
            / u64::from(shared::components::COMPANY_TOTAL_SHARES)
    }

    fn status(&self) -> &'static str {
        if self.sites.is_empty() {
            "NO SITES"
        } else if self.sites.iter().any(|site| {
            matches!(
                site.state,
                BusinessState::Insolvent | BusinessState::Liquidating | BusinessState::Closed
            )
        }) {
            "AT RISK"
        } else if self.account.wage_arrears > 0 || self.account.tax_arrears > 0 {
            "IN ARREARS"
        } else if self.account.current_day.profit() > 0 {
            "PROFITABLE"
        } else if self
            .sites
            .iter()
            .all(|site| matches!(site.state, BusinessState::New))
        {
            "NEW"
        } else {
            "TRADING"
        }
    }
}

#[derive(Resource, Default, Debug, PartialEq, Eq)]
pub struct CompanyDirectory {
    pub records: Vec<CompanyRecord>,
    pub local_person: Option<PersonId>,
    pub local_wallet: Option<u64>,
}

#[derive(Resource, Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CompanyFilter {
    #[default]
    All,
    MyHoldings,
    SharesForSale,
}

impl CompanyFilter {
    const ALL: [Self; 3] = [Self::All, Self::MyHoldings, Self::SharesForSale];

    const fn label(self) -> &'static str {
        match self {
            Self::All => "ALL FIRMS",
            Self::MyHoldings => "MY HOLDINGS",
            Self::SharesForSale => "SHARES FOR SALE",
        }
    }
}

#[derive(Resource, Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct SelectedCompany(pub Option<CompanyId>);

#[derive(Resource, Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct CompanyDrilldownReturn(pub Option<CompanyId>);

#[derive(Component)]
pub struct CompanyPortfolioContent;
#[derive(Component)]
pub struct CompanyListContent;
#[derive(Component)]
pub struct CompanyListViewport;
#[derive(Component)]
pub struct CompanyDetailViewport;
#[derive(Component)]
pub struct CompanyDetailContent;
#[derive(Component)]
pub struct CompanyCountText;
#[derive(Component, Clone, Copy)]
pub struct CompanyFilterButton(pub CompanyFilter);
#[derive(Component, Clone, Copy)]
pub struct CompanyRow(pub CompanyId);
#[derive(Component, Clone, Copy)]
pub struct CompanySiteButton(pub BuildingId);
#[derive(Component, Clone, Copy)]
pub struct CompanyPersonButton(pub PersonId);
#[derive(Component, Clone, Copy)]
pub struct CompanyManagementButton {
    pub site: Entity,
    pub company: CompanyId,
}

#[derive(Component, Clone, Copy)]
pub struct CompanyBranchPolicyButton {
    pub company: CompanyId,
    pub action: HeroCompanyAction,
}

#[derive(Resource, Default, Debug, Clone, PartialEq, Eq)]
pub struct CompanyPolicyFeedback {
    pub message: String,
    pub success: bool,
}

pub(super) fn company_tab_active(tab: Res<EncyclopediaTab>) -> bool {
    *tab == EncyclopediaTab::Companies
}

#[allow(clippy::type_complexity)]
pub(super) fn refresh_company_directory(
    companies: Query<(
        &CompanyId,
        &Company,
        &CompanyOwnership,
        &CompanyLeadership,
        &CompanyAccount,
        &CompanyManagementPolicy,
        Option<&CompanyBranchPolicies>,
        &CompanyDecisionHistory,
        &CompanyShareMarket,
    )>,
    sites: Query<(
        Entity,
        &BuildingId,
        &OperatedBy,
        &BuildingOf,
        &SettlementBuilding,
        Option<&BusinessAccount>,
        Option<&BusinessCondition>,
        Option<&GoodsInventory>,
        Option<&shared::economy::BusinessSalePolicy>,
        Option<&BusinessProcurementPolicy>,
        Option<&BusinessSupplyPolicy>,
        Option<&BusinessStaffingPolicy>,
    )>,
    people: Query<(&PersonId, &CharacterName)>,
    heroes: Query<(&Hero, &PersonId, Option<&Wallet>)>,
    local: Option<Res<crate::camera_rts::LocalPeerId>>,
    known_people: Res<KnownPeople>,
    mut directory: ResMut<CompanyDirectory>,
) {
    let mut names: HashMap<PersonId, String> = known_people
        .records
        .iter()
        .filter(|record| record.id.is_assigned())
        .map(|record| (record.id, record.name.clone()))
        .collect();
    for (id, name) in people.iter() {
        names.insert(*id, name.0.clone());
    }
    let person_name = |person: PersonId| {
        names
            .get(&person)
            .cloned()
            .unwrap_or_else(|| format!("Person #{}", person.0))
    };
    let replicated_local = local.as_ref().and_then(|local| {
        heroes
            .iter()
            .find(|(hero, ..)| shared::player::peer_id_to_u64(hero.owner) == local.0)
            .map(|(_, person, wallet)| (*person, wallet.map(|wallet| wallet.balance())))
    });
    let roster_local = known_people
        .records
        .iter()
        .find(|record| record.is_self && record.id.is_assigned())
        .map(|record| (record.id, record.wallet));
    let (local_person, local_wallet) = replicated_local.or(roster_local).unzip();
    let local_wallet = local_wallet.flatten();

    let mut sites_by_company: HashMap<CompanyId, Vec<CompanySiteRecord>> = HashMap::new();
    for (
        entity,
        building_id,
        operated_by,
        building_of,
        building,
        site_account,
        condition,
        inventory,
        sale,
        procurement,
        supply,
        staffing,
    ) in sites.iter()
    {
        let output = output_good(building.kind);
        let input = input_good(building.kind);
        let sale = sale.copied();
        let output_stock = output.map_or(0, |good| {
            inventory.map_or(0, |inventory| inventory.amount(good))
        });
        let input_stock = input.map_or(0, |good| {
            inventory.map_or(0, |inventory| inventory.amount(good))
        });
        let input_rule = input.and_then(|good| procurement.map(|policy| policy.rule(good)));
        let private_rule = input.and_then(|good| supply.map(|policy| policy.rule(good)));
        let account = site_account.copied().unwrap_or_default();
        sites_by_company
            .entry(operated_by.0)
            .or_default()
            .push(CompanySiteRecord {
                entity,
                id: *building_id,
                settlement: building.settlement.clone(),
                settlement_id: building_of.0,
                kind: building.kind,
                workers: building.workers.len(),
                positions: building.kind.positions(),
                enabled_positions: staffing
                    .copied()
                    .unwrap_or_default()
                    .target_for(building.kind),
                state: condition.copied().unwrap_or_default().state,
                wage_arrears: account.wage_arrears,
                tax_arrears: account.tax_arrears,
                current_day: account.current_day,
                previous_day: account.previous_day,
                output,
                output_stock,
                asking_price: sale.map(|policy| policy.asking_unit_price),
                input,
                input_stock,
                input_target: input_rule.map_or(0, |rule| rule.target_units),
                input_coverage_days: input_rule.map_or(0, |rule| rule.coverage_days),
                sourcing: private_rule
                    .filter(|rule| rule.enabled)
                    .map(|rule| rule.sourcing),
                preferred_supplier: private_rule.and_then(|rule| rule.preferred_supplier),
                goods: inventory.map_or_else(Vec::new, |inventory| {
                    Good::ALL
                        .into_iter()
                        .filter_map(|good| {
                            let amount = inventory.amount(good);
                            (amount > 0).then_some((good, amount))
                        })
                        .collect()
                }),
                used_bulk: inventory.map_or(0, GoodsInventory::used_bulk),
                bulk_capacity: inventory.map_or(0, GoodsInventory::bulk_capacity),
            });
    }

    let mut records = Vec::new();
    for (
        id,
        company,
        ownership,
        leadership,
        account,
        policy,
        branch_policies,
        decisions,
        share_market,
    ) in companies.iter()
    {
        let mut holders: Vec<_> = ownership
            .shares()
            .iter()
            .map(|share| CompanyHolderRecord {
                person: share.shareholder,
                name: person_name(share.shareholder),
                shares: share.shares,
            })
            .collect();
        holders.sort_by(|a, b| b.shares.cmp(&a.shares).then_with(|| a.name.cmp(&b.name)));

        let mut offers: Vec<_> = share_market
            .offers()
            .iter()
            .map(|offer| CompanyOfferRecord {
                seller: offer.seller,
                seller_name: person_name(offer.seller),
                shares: offer.shares,
                unit_price: offer.unit_price,
                listed_day: offer.listed_day,
            })
            .collect();
        offers.sort_by_key(|offer| (offer.unit_price, offer.listed_day, offer.seller));

        let mut company_sites = sites_by_company.remove(id).unwrap_or_default();
        company_sites.sort_by_key(|site| (site.settlement.clone(), site.kind.label(), site.id));
        let mut branches_by_settlement: HashMap<SettlementId, CompanyBranchRecord> = HashMap::new();
        for site in &company_sites {
            let branch = branches_by_settlement
                .entry(site.settlement_id)
                .or_insert_with(|| CompanyBranchRecord {
                    settlement: site.settlement.clone(),
                    settlement_id: site.settlement_id,
                    sites: 0,
                    storage_halls: 0,
                    used_bulk: 0,
                    bulk_capacity: 0,
                    resources: Good::ALL
                        .into_iter()
                        .map(|good| {
                            (
                                good,
                                0,
                                branch_policies
                                    .map_or_else(CompanyResourcePolicy::default, |policies| {
                                        policies.resource(site.settlement_id, good)
                                    }),
                            )
                        })
                        .collect(),
                });
            branch.sites += 1;
            branch.storage_halls += usize::from(site.kind == SettlementBuildingKind::StorageHall);
            branch.used_bulk = branch.used_bulk.saturating_add(site.used_bulk);
            branch.bulk_capacity = branch.bulk_capacity.saturating_add(site.bulk_capacity);
            for (good, amount) in &site.goods {
                if let Some((_, total, _)) = branch
                    .resources
                    .iter_mut()
                    .find(|(candidate, ..)| candidate == good)
                {
                    *total = total.saturating_add(*amount);
                }
            }
        }
        let mut branches: Vec<_> = branches_by_settlement.into_values().collect();
        branches.sort_by(|a, b| a.settlement.cmp(&b.settlement));

        records.push(CompanyRecord {
            id: *id,
            name: company.name.clone(),
            founded_day: company.founded_day,
            master: leadership.master,
            master_name: person_name(leadership.master),
            account: *account,
            policy: *policy,
            holders,
            offers,
            decisions: decisions.entries().to_vec(),
            sites: company_sites,
            branches,
        });
    }
    records.sort_by(|a, b| {
        a.name
            .to_lowercase()
            .cmp(&b.name.to_lowercase())
            .then(a.id.cmp(&b.id))
    });

    let next = CompanyDirectory {
        records,
        local_person,
        local_wallet,
    };
    if *directory != next {
        *directory = next;
    }
}

pub(super) fn spawn_companies_tab(body: &mut ChildSpawnerCommands<'_>) {
    body.spawn((
        TabBody(EncyclopediaTab::Companies),
        Node {
            display: Display::None,
            flex_grow: 1.0,
            min_height: Val::Px(0.0),
            flex_direction: FlexDirection::Column,
            align_items: AlignItems::Stretch,
            overflow: Overflow::clip(),
            ..default()
        },
    ))
    .with_children(|tab| {
        tab.spawn((
            CompanyPortfolioContent,
            Node {
                flex_shrink: 0.0,
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Stretch,
                column_gap: Val::Px(8.0),
                padding: UiRect::all(Val::Px(12.0)),
                border: UiRect::bottom(Val::Px(1.0)),
                ..default()
            },
            BorderColor::from(DIVIDER),
        ));

        tab.spawn((
            Node {
                flex_shrink: 0.0,
                flex_direction: FlexDirection::Row,
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                padding: UiRect::axes(Val::Px(16.0), Val::Px(8.0)),
                border: UiRect::bottom(Val::Px(1.0)),
                ..default()
            },
            BorderColor::from(DIVIDER),
        ))
        .with_children(|bar| {
            bar.spawn(Node {
                flex_direction: FlexDirection::Row,
                column_gap: Val::Px(6.0),
                ..default()
            })
            .with_children(|filters| {
                for filter in CompanyFilter::ALL {
                    filters
                        .spawn((
                            Button,
                            CompanyFilterButton(filter),
                            Node {
                                padding: UiRect::axes(Val::Px(11.0), Val::Px(5.0)),
                                border: UiRect::all(Val::Px(1.0)),
                                border_radius: BorderRadius::all(Val::Px(11.0)),
                                ..default()
                            },
                            BackgroundColor(Color::NONE),
                            BorderColor::from(DIVIDER),
                        ))
                        .with_child((
                            Text::new(filter.label()),
                            TextFont {
                                font_size: FontSize::Px(9.5),
                                ..default()
                            },
                            TextColor(TEXT_MUTED),
                            Pickable::IGNORE,
                        ));
                }
            });
            bar.spawn((
                CompanyCountText,
                Text::new("0 companies"),
                TextFont {
                    font_size: FontSize::Px(10.0),
                    ..default()
                },
                TextColor(TEXT_MUTED),
            ));
        });

        tab.spawn(Node {
            flex_grow: 1.0,
            min_height: Val::Px(0.0),
            flex_direction: FlexDirection::Row,
            overflow: Overflow::clip(),
            ..default()
        })
        .with_children(|split| {
            split
                .spawn((
                    CompanyListViewport,
                    Node {
                        width: Val::Px(326.0),
                        min_height: Val::Px(0.0),
                        flex_shrink: 0.0,
                        flex_direction: FlexDirection::Column,
                        overflow: Overflow::scroll_y(),
                        scrollbar_width: 8.0,
                        border: UiRect::right(Val::Px(1.0)),
                        ..default()
                    },
                    BorderColor::from(DIVIDER),
                ))
                .with_child((
                    CompanyListContent,
                    Node {
                        flex_shrink: 0.0,
                        flex_direction: FlexDirection::Column,
                        row_gap: Val::Px(3.0),
                        padding: UiRect::all(Val::Px(8.0)),
                        ..default()
                    },
                ));
            split
                .spawn((
                    CompanyDetailViewport,
                    Node {
                        flex_grow: 1.0,
                        min_height: Val::Px(0.0),
                        flex_direction: FlexDirection::Column,
                        overflow: Overflow::scroll_y(),
                        scrollbar_width: 8.0,
                        ..default()
                    },
                    BackgroundColor(DETAIL_BG),
                ))
                .with_child((
                    CompanyDetailContent,
                    Node {
                        flex_shrink: 0.0,
                        flex_direction: FlexDirection::Column,
                        align_items: AlignItems::Stretch,
                        row_gap: Val::Px(12.0),
                        padding: UiRect::all(Val::Px(20.0)),
                        ..default()
                    },
                ));
        });
    });
}

pub(super) fn handle_company_filter_buttons(
    guard: Res<ClickGuard>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut filter: ResMut<CompanyFilter>,
    buttons: Query<(&Interaction, &CompanyFilterButton), Changed<Interaction>>,
) {
    if !guard.0 || !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    for (interaction, CompanyFilterButton(next)) in buttons.iter() {
        if *interaction == Interaction::Pressed {
            *filter = *next;
        }
    }
}

pub(super) fn handle_company_rows(
    guard: Res<ClickGuard>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut selected: ResMut<SelectedCompany>,
    rows: Query<(&Interaction, &CompanyRow), Changed<Interaction>>,
) {
    if !guard.0 || !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    for (interaction, CompanyRow(company)) in rows.iter() {
        if *interaction == Interaction::Pressed {
            selected.0 = Some(*company);
        }
    }
}

pub(super) fn handle_company_site_buttons(
    guard: Res<ClickGuard>,
    mouse: Res<ButtonInput<MouseButton>>,
    buttons: Query<(&Interaction, &CompanySiteButton), Changed<Interaction>>,
    places: Res<places::KnownPlaces>,
    selected_company: Res<SelectedCompany>,
    mut selected_place: ResMut<places::SelectedPlace>,
    mut selected_entry: ResMut<places::SelectedPlaceEntry>,
    mut return_to: ResMut<CompanyDrilldownReturn>,
    mut tab: ResMut<EncyclopediaTab>,
) {
    if !guard.0 || !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    for (interaction, CompanySiteButton(building)) in buttons.iter() {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let Some((place, index)) = places.records.iter().find_map(|place| {
            place
                .buildings
                .iter()
                .position(|candidate| candidate.id == Some(*building))
                .map(|index| (place.name.clone(), index))
        }) else {
            continue;
        };
        selected_place.0 = Some(place);
        *selected_entry = places::SelectedPlaceEntry::Building(index);
        return_to.0 = selected_company.0;
        *tab = EncyclopediaTab::Places;
    }
}

pub(super) fn handle_company_person_buttons(
    guard: Res<ClickGuard>,
    mouse: Res<ButtonInput<MouseButton>>,
    buttons: Query<(&Interaction, &CompanyPersonButton), Changed<Interaction>>,
    people: Res<KnownPeople>,
    mut selected: ResMut<SelectedPerson>,
    mut tab: ResMut<EncyclopediaTab>,
) {
    if !guard.0 || !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    for (interaction, CompanyPersonButton(person)) in buttons.iter() {
        if *interaction != Interaction::Pressed {
            continue;
        }
        let Some(record) = people.records.iter().find(|record| record.id == *person) else {
            continue;
        };
        selected.0 = Some(record.name.clone());
        *tab = EncyclopediaTab::People;
    }
}

pub(super) fn handle_company_management_buttons(
    guard: Res<ClickGuard>,
    mouse: Res<ButtonInput<MouseButton>>,
    buttons: Query<(&Interaction, &CompanyManagementButton), Changed<Interaction>>,
    mut target: ResMut<crate::ui::business_management::BusinessManagementTarget>,
    mut return_to: ResMut<crate::ui::business_management::BusinessManagementReturn>,
    mut open: ResMut<EncyclopediaOpen>,
) {
    if !guard.0 || !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    for (interaction, button) in buttons.iter() {
        if *interaction == Interaction::Pressed {
            target.0 = Some(button.site);
            return_to.0 = Some(button.company);
            open.0 = false;
        }
    }
}

pub(super) fn handle_company_branch_policy_buttons(
    guard: Res<ClickGuard>,
    mouse: Res<ButtonInput<MouseButton>>,
    mut buttons: Query<
        (
            &Interaction,
            &CompanyBranchPolicyButton,
            &mut BackgroundColor,
        ),
        Changed<Interaction>,
    >,
    mut clients: Query<
        &mut MessageSender<HeroCompanyOrder>,
        (With<crate::GameClient>, With<Connected>),
    >,
) {
    for (interaction, button, mut background) in buttons.iter_mut() {
        background.0 = if *interaction == Interaction::Pressed {
            Color::srgba(0.68, 0.64, 0.56, 1.0)
        } else if *interaction == Interaction::Hovered {
            ROW_HOVERED
        } else {
            BUTTON_NORMAL
        };
        if *interaction != Interaction::Pressed
            || !guard.0
            || !mouse.just_pressed(MouseButton::Left)
        {
            continue;
        }
        let Ok(mut sender) = clients.single_mut() else {
            continue;
        };
        sender.send::<ReliableChannel>(HeroCompanyOrder {
            company: button.company,
            action: button.action,
        });
    }
}

pub(super) fn receive_company_policy_results(
    mut receivers: Query<&mut MessageReceiver<HeroCompanyResult>, With<crate::GameClient>>,
    mut feedback: ResMut<CompanyPolicyFeedback>,
) {
    for mut receiver in receivers.iter_mut() {
        for result in receiver.receive() {
            feedback.message = result.message;
            feedback.success = result.success;
        }
    }
}

pub(super) fn rebuild_company_view(
    mut commands: Commands,
    directory: Res<CompanyDirectory>,
    filter: Res<CompanyFilter>,
    mut selected: ResMut<SelectedCompany>,
    portfolio: Query<(Entity, Option<&Children>), With<CompanyPortfolioContent>>,
    list: Query<(Entity, Option<&Children>), With<CompanyListContent>>,
    detail: Query<(Entity, Option<&Children>), With<CompanyDetailContent>>,
    mut count_text: Query<&mut Text, With<CompanyCountText>>,
    feedback: Res<CompanyPolicyFeedback>,
) {
    if !directory.is_changed()
        && !filter.is_changed()
        && !selected.is_changed()
        && !feedback.is_changed()
    {
        return;
    }
    let visible = visible_companies(&directory, *filter);
    if selected
        .0
        .is_some_and(|id| !directory.records.iter().any(|company| company.id == id))
    {
        selected.0 = None;
    }
    if selected.0.is_none() {
        selected.0 = visible.first().map(|company| company.id);
    }
    for mut text in count_text.iter_mut() {
        text.0 = match visible.len() {
            1 => "1 company".to_string(),
            count => format!("{count} companies"),
        };
    }

    if let Ok((entity, children)) = portfolio.single() {
        clear_children(&mut commands, children);
        commands
            .entity(entity)
            .with_children(|parent| spawn_portfolio(parent, &directory));
    }
    if let Ok((entity, children)) = list.single() {
        clear_children(&mut commands, children);
        commands.entity(entity).with_children(|parent| {
            if visible.is_empty() {
                spawn_empty(
                    parent,
                    match *filter {
                        CompanyFilter::All => "No companies exist yet.",
                        CompanyFilter::MyHoldings => {
                            "You do not own shares in a company yet. Switch to ALL FIRMS to browse."
                        }
                        CompanyFilter::SharesForSale => "No company shares are currently offered.",
                    },
                );
            } else {
                for company in visible {
                    spawn_company_row(parent, company, directory.local_person);
                }
            }
        });
    }
    if let Ok((entity, children)) = detail.single() {
        clear_children(&mut commands, children);
        commands.entity(entity).with_children(|parent| {
            let company = selected
                .0
                .and_then(|id| directory.records.iter().find(|company| company.id == id));
            if let Some(company) = company {
                spawn_company_detail(parent, company, &directory, &feedback);
            } else {
                spawn_empty(
                    parent,
                    "Select a company to inspect its ownership and books.",
                );
            }
        });
    }
}

pub(super) fn style_company_controls(
    filter: Res<CompanyFilter>,
    selected: Res<SelectedCompany>,
    mut filters: Query<
        (&CompanyFilterButton, &Interaction, &mut BackgroundColor),
        Without<CompanyRow>,
    >,
    mut rows: Query<
        (&CompanyRow, &Interaction, &mut BackgroundColor),
        Without<CompanyFilterButton>,
    >,
) {
    for (CompanyFilterButton(button), interaction, mut background) in filters.iter_mut() {
        background.0 = if *button == *filter {
            ROW_SELECTED
        } else if *interaction == Interaction::Hovered {
            ROW_HOVERED
        } else {
            Color::NONE
        };
    }
    for (CompanyRow(company), interaction, mut background) in rows.iter_mut() {
        background.0 = if selected.0 == Some(*company) {
            ROW_SELECTED
        } else if matches!(interaction, Interaction::Hovered | Interaction::Pressed) {
            ROW_HOVERED
        } else {
            ROW_NORMAL
        };
    }
}

fn visible_companies(directory: &CompanyDirectory, filter: CompanyFilter) -> Vec<&CompanyRecord> {
    let mut visible: Vec<_> = directory
        .records
        .iter()
        .filter(|company| match filter {
            CompanyFilter::All => true,
            CompanyFilter::MyHoldings => directory
                .local_person
                .is_some_and(|person| company.shares_owned_by(person) > 0),
            CompanyFilter::SharesForSale => !company.offers.is_empty(),
        })
        .collect();
    visible.sort_by(|a, b| {
        let a_owned = directory
            .local_person
            .map_or(0, |person| a.shares_owned_by(person));
        let b_owned = directory
            .local_person
            .map_or(0, |person| b.shares_owned_by(person));
        b_owned
            .cmp(&a_owned)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    visible
}

fn clear_children(commands: &mut Commands, children: Option<&Children>) {
    if let Some(children) = children {
        for child in children.iter() {
            commands.entity(child).despawn();
        }
    }
}

fn spawn_portfolio(parent: &mut ChildSpawnerCommands<'_>, directory: &CompanyDirectory) {
    let Some(person) = directory.local_person else {
        parent.spawn((
            Text::new(
                "PORTFOLIO UNAVAILABLE  /  Spawn or select your Hero to identify personal holdings. The company directory remains usable.",
            ),
            TextFont {
                font_size: FontSize::Px(10.0),
                ..default()
            },
            TextColor(TEXT_MUTED),
        ));
        return;
    };
    let holdings: Vec<_> = directory
        .records
        .iter()
        .filter_map(|company| {
            let shares = company.shares_owned_by(person);
            (shares > 0).then_some((company, shares))
        })
        .collect();
    let estimated_interest = holdings.iter().fold(0u64, |total, (company, shares)| {
        total.saturating_add(company.holding_book_interest(*shares))
    });
    let mastered = directory
        .records
        .iter()
        .filter(|company| company.master == person)
        .count();
    portfolio_card(
        parent,
        "HERO WALLET",
        directory.local_wallet.map_or_else(
            || "Not in range".to_string(),
            |wallet| format!("{} coin", format_money(wallet)),
        ),
        "Spendable by your Hero",
    );
    portfolio_card(
        parent,
        "COMPANY HOLDINGS",
        format!(
            "{} firm{}",
            holdings.len(),
            if holdings.len() == 1 { "" } else { "s" }
        ),
        "Direct share positions",
    );
    portfolio_card(
        parent,
        "BOOK INTEREST",
        format!("{} coin", format_money(estimated_interest)),
        "Accounting estimate, not cash",
    );
    portfolio_card(
        parent,
        "COMPANY MASTER",
        format!("{} firm{}", mastered, if mastered == 1 { "" } else { "s" }),
        "Executive authority",
    );
}

fn portfolio_card(parent: &mut ChildSpawnerCommands<'_>, label: &str, value: String, note: &str) {
    parent
        .spawn((
            Node {
                flex_grow: 1.0,
                min_width: Val::Px(0.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(2.0),
                padding: UiRect::axes(Val::Px(10.0), Val::Px(7.0)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(RADIUS)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.9, 0.88, 0.84, 0.58)),
            BorderColor::from(DIVIDER),
        ))
        .with_children(|card| {
            card.spawn((
                Text::new(label),
                TextFont {
                    font_size: FontSize::Px(8.5),
                    ..default()
                },
                TextColor(TEXT_MUTED),
            ));
            card.spawn((
                Text::new(value),
                TextFont {
                    font_size: FontSize::Px(14.0),
                    ..default()
                },
                TextColor(TEXT_COLOR),
            ));
            card.spawn((
                Text::new(note),
                TextFont {
                    font_size: FontSize::Px(7.5),
                    ..default()
                },
                TextColor(TEXT_MUTED),
            ));
        });
}

fn spawn_company_row(
    parent: &mut ChildSpawnerCommands<'_>,
    company: &CompanyRecord,
    local_person: Option<PersonId>,
) {
    let shares = local_person.map_or(0, |person| company.shares_owned_by(person));
    parent
        .spawn((
            Button,
            CompanyRow(company.id),
            Node {
                flex_shrink: 0.0,
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Stretch,
                row_gap: Val::Px(4.0),
                padding: UiRect::axes(Val::Px(10.0), Val::Px(8.0)),
                border_radius: BorderRadius::all(Val::Px(5.0)),
                ..default()
            },
            BackgroundColor(ROW_NORMAL),
        ))
        .with_children(|row| {
            row.spawn(Node {
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                ..default()
            })
            .with_children(|line| {
                line.spawn((
                    Text::new(company.name.clone()),
                    TextFont {
                        font_size: FontSize::Px(12.5),
                        ..default()
                    },
                    TextColor(TEXT_COLOR),
                ));
                line.spawn((
                    Text::new(company.status()),
                    TextFont {
                        font_size: FontSize::Px(8.0),
                        ..default()
                    },
                    TextColor(if company.status() == "AT RISK" {
                        ACCENT_COLOR
                    } else {
                        TEXT_MUTED
                    }),
                ));
            });
            row.spawn((
                Text::new(format!(
                    "{} site{}  /  {} cash  /  today {}{}",
                    company.sites.len(),
                    if company.sites.len() == 1 { "" } else { "s" },
                    format_money(company.account.cash),
                    if company.account.current_day.profit() < 0 {
                        "-"
                    } else {
                        "+"
                    },
                    format_money(company.account.current_day.profit().unsigned_abs()),
                )),
                TextFont {
                    font_size: FontSize::Px(8.5),
                    ..default()
                },
                TextColor(TEXT_MUTED),
            ));
            if shares > 0 {
                row.spawn((
                    Text::new(format!(
                        "YOUR HOLDING  {} / 1,000 ({:.1}%){}",
                        shares,
                        f32::from(shares) / 10.0,
                        if local_person == Some(company.master) {
                            "  /  COMPANY MASTER"
                        } else {
                            ""
                        }
                    )),
                    TextFont {
                        font_size: FontSize::Px(8.5),
                        ..default()
                    },
                    TextColor(ACCENT_COLOR),
                ));
            }
        });
}

fn spawn_company_detail(
    parent: &mut ChildSpawnerCommands<'_>,
    company: &CompanyRecord,
    directory: &CompanyDirectory,
    feedback: &CompanyPolicyFeedback,
) {
    parent
        .spawn(Node {
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::FlexStart,
            column_gap: Val::Px(12.0),
            ..default()
        })
        .with_children(|header| {
            header
                .spawn(Node {
                    min_width: Val::Px(0.0),
                    flex_grow: 1.0,
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(3.0),
                    ..default()
                })
                .with_children(|copy| {
                    copy.spawn((
                        Text::new(company.name.clone()),
                        TextFont {
                            font_size: FontSize::Px(22.0),
                            ..default()
                        },
                        TextColor(TEXT_COLOR),
                    ));
                    copy.spawn((
                        Text::new(format!(
                            "COMPANY #{}  /  FOUNDED DAY {}  /  {}",
                            company.id.0,
                            company.founded_day,
                            company.status(),
                        )),
                        TextFont {
                            font_size: FontSize::Px(9.0),
                            ..default()
                        },
                        TextColor(ACCENT_COLOR),
                    ));
                });
            header
                .spawn(Node {
                    flex_direction: FlexDirection::Row,
                    flex_wrap: FlexWrap::Wrap,
                    justify_content: JustifyContent::FlexEnd,
                    column_gap: Val::Px(6.0),
                    row_gap: Val::Px(6.0),
                    ..default()
                })
                .with_children(|actions| {
                    detail_button(
                        actions,
                        crate::ui::history::CompanyHistoryButton {
                            company: company.id,
                            name: company.name.clone(),
                        },
                        "FULL LEDGER",
                    );
                    if let Some(site) = company.sites.first() {
                        detail_button(
                            actions,
                            CompanyManagementButton {
                                site: site.entity,
                                company: company.id,
                            },
                            "COMPANY CONTROLS",
                        );
                    }
                });
        });

    if !feedback.message.is_empty() {
        spawn_note(
            parent,
            &format!(
                "{}: {}",
                if feedback.success {
                    "UPDATED"
                } else {
                    "NOT CHANGED"
                },
                feedback.message
            ),
        );
    }

    let liabilities = company
        .account
        .wage_arrears
        .saturating_add(company.account.tax_arrears);
    parent
        .spawn(Node {
            flex_wrap: FlexWrap::Wrap,
            column_gap: Val::Px(7.0),
            row_gap: Val::Px(7.0),
            ..default()
        })
        .with_children(|stats| {
            detail_stat(
                stats,
                "COMPANY CASH",
                format!("{} coin", format_money(company.account.cash)),
            );
            detail_stat(
                stats,
                "TODAY'S REVENUE",
                format!(
                    "{} coin",
                    format_money(company.account.current_day.external_revenue)
                ),
            );
            detail_stat(
                stats,
                "TODAY'S PROFIT",
                signed_money(company.account.current_day.profit()),
            );
            detail_stat(
                stats,
                "LIABILITIES",
                format!("{} coin", format_money(liabilities)),
            );
            detail_stat(
                stats,
                "CAPITAL ASSETS",
                format!("{} coin", format_money(company.account.book_value)),
            );
            detail_stat(
                stats,
                "BOOK EQUITY",
                format!("{} coin", format_money(company.accounting_equity())),
            );
        });

    spawn_section_title(
        parent,
        "YOUR POSITION",
        "wallet money and company money stay separate",
    );
    if let Some(person) = directory.local_person {
        let shares = company.shares_owned_by(person);
        if shares == 0 {
            spawn_note(
                parent,
                if company.offers.is_empty() {
                    "You own no shares. No shareholder is currently offering stock."
                } else {
                    "You own no shares. Public offers are listed below; open CONTROLS & SHARES to trade."
                },
            );
        } else {
            key_value(
                parent,
                "OWNERSHIP",
                format!(
                    "{} / 1,000 shares ({:.1}%)  /  estimated book interest {} coin",
                    shares,
                    f32::from(shares) / 10.0,
                    format_money(company.holding_book_interest(shares)),
                ),
            );
            key_value(
                parent,
                "AUTHORITY",
                if company.master == person {
                    "You are Company Master and control ordinary operating decisions".to_string()
                } else if shares > shared::components::COMPANY_TOTAL_SHARES / 2 {
                    "Majority holder; may appoint the Company Master".to_string()
                } else {
                    "Shareholder; economic ownership without executive authority".to_string()
                },
            );
            if company.master == person && shares == shared::components::COMPANY_TOTAL_SHARES {
                parent
                    .spawn(Node {
                        flex_direction: FlexDirection::Row,
                        flex_wrap: FlexWrap::Wrap,
                        align_items: AlignItems::Center,
                        column_gap: Val::Px(7.0),
                        row_gap: Val::Px(7.0),
                        ..default()
                    })
                    .with_children(|actions| {
                        actions.spawn((
                            Text::new(format!(
                                "HERO WALLET  {} coin",
                                format_money(directory.local_wallet.unwrap_or(0))
                            )),
                            TextFont {
                                font_size: FontSize::Px(9.0),
                                ..default()
                            },
                            TextColor(TEXT_MUTED),
                        ));
                        detail_button(
                            actions,
                            CompanyBranchPolicyButton {
                                company: company.id,
                                action: HeroCompanyAction::ContributeCapital {
                                    amount: shared::economy::PENNIES_PER_COIN,
                                },
                            },
                            "ADD 1 COIN",
                        );
                        detail_button(
                            actions,
                            CompanyBranchPolicyButton {
                                company: company.id,
                                action: HeroCompanyAction::ContributeCapital {
                                    amount: 5 * shared::economy::PENNIES_PER_COIN,
                                },
                            },
                            "ADD 5 COIN",
                        );
                    });
                spawn_note(
                    parent,
                    "Capital contributions move personal coin into the company treasury; they are not revenue or profit.",
                );
            }
        }
    } else {
        spawn_note(
            parent,
            "Spawn or select your Hero to resolve personal holdings.",
        );
    }

    spawn_section_title(
        parent,
        "CONSOLIDATED LEDGER",
        "wages post to the completed shift at dawn; internal transfers are memorandum only",
    );
    spawn_day_ledger(parent, "TODAY", company.account.current_day);
    if company.account.previous_day.day != u32::MAX {
        spawn_day_ledger(parent, "PREVIOUS DAY", company.account.previous_day);
    }
    key_value(
        parent,
        "LIFETIME CAPITAL",
        format!(
            "{} contributed  /  {} capital spending  /  {} distributed",
            format_money(company.account.contributed_capital),
            format_money(company.account.capital_expenditures),
            format_money(company.account.owner_withdrawals),
        ),
    );

    spawn_section_title(
        parent,
        "LOCAL GOODS & STORAGE",
        "stock is physical and never shared between settlements",
    );
    if company.branches.is_empty() {
        spawn_note(parent, "This company has no local operating branch.");
    } else {
        let can_manage = directory.local_person == Some(company.master);
        for branch in &company.branches {
            spawn_branch_card(parent, company.id, branch, can_manage);
        }
    }

    spawn_section_title(
        parent,
        "OPERATING SITES",
        "select a site to open its full place record",
    );
    if company.sites.is_empty() {
        spawn_note(parent, "This company has no operating site.");
    } else {
        for site in &company.sites {
            spawn_site_card(parent, company.id, site);
        }
    }

    spawn_section_title(parent, "OWNERSHIP", "1,000 ordinary shares in total");
    for holder in &company.holders {
        parent
            .spawn((
                Button,
                CompanyPersonButton(holder.person),
                Node {
                    justify_content: JustifyContent::SpaceBetween,
                    align_items: AlignItems::Center,
                    padding: UiRect::axes(Val::Px(10.0), Val::Px(7.0)),
                    border: UiRect::bottom(Val::Px(1.0)),
                    ..default()
                },
                BackgroundColor(Color::NONE),
                BorderColor::from(DIVIDER),
            ))
            .with_children(|row| {
                row.spawn((
                    Text::new(if holder.person == company.master {
                        format!("{}  /  COMPANY MASTER", holder.name)
                    } else {
                        holder.name.clone()
                    }),
                    TextFont {
                        font_size: FontSize::Px(10.5),
                        ..default()
                    },
                    TextColor(TEXT_COLOR),
                    Pickable::IGNORE,
                ));
                row.spawn((
                    Text::new(format!(
                        "{} shares  /  {:.1}%",
                        holder.shares,
                        f32::from(holder.shares) / 10.0
                    )),
                    TextFont {
                        font_size: FontSize::Px(10.0),
                        ..default()
                    },
                    TextColor(TEXT_MUTED),
                    Pickable::IGNORE,
                ));
            });
    }

    spawn_section_title(
        parent,
        "PUBLIC SHARE OFFERS",
        "seller chooses price; ownership moves only on purchase",
    );
    if company.offers.is_empty() {
        spawn_note(parent, "No shares are currently offered.");
    } else {
        for offer in &company.offers {
            key_value(
                parent,
                &offer.seller_name.to_uppercase(),
                format!(
                    "{} shares at {} coin each  /  listed day {}  /  total {} coin",
                    offer.shares,
                    format_money(offer.unit_price),
                    offer.listed_day,
                    format_money(offer.unit_price.saturating_mul(u64::from(offer.shares))),
                ),
            );
        }
    }

    spawn_section_title(
        parent,
        "GOVERNANCE",
        "company-wide policy set by the Company Master",
    );
    key_value(
        parent,
        "COMPANY MASTER",
        format!("{}  /  Person #{}", company.master_name, company.master.0),
    );
    key_value(
        parent,
        "OPERATING POLICY",
        format!(
            "{}  /  {}  /  {} payroll reserve days",
            company.policy.strategy.label(),
            if company.policy.autopilot {
                "autopilot"
            } else {
                "manual"
            },
            company.policy.payroll_reserve_days,
        ),
    );
    key_value(
        parent,
        "DIVIDENDS",
        if company.policy.automatic_dividends {
            format!(
                "automatic after reserves  /  up to {} coin per day",
                format_money(company.policy.max_daily_dividend)
            )
        } else {
            "retained until the Company Master distributes available profit".to_string()
        },
    );

    spawn_section_title(
        parent,
        "RECENT MASTER DECISIONS",
        "bounded executive audit trail",
    );
    if company.decisions.is_empty() {
        spawn_note(parent, "No strategy change has been recorded yet.");
    } else {
        for decision in company.decisions.iter().rev().take(8) {
            key_value(
                parent,
                &format!("DAY {}", decision.day),
                format!(
                    "{} -> {}  /  {}",
                    decision.from.label(),
                    decision.to.label(),
                    decision.reason.label(),
                ),
            );
        }
    }
}

fn spawn_branch_card(
    parent: &mut ChildSpawnerCommands<'_>,
    company: CompanyId,
    branch: &CompanyBranchRecord,
    can_manage: bool,
) {
    parent
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Stretch,
                row_gap: Val::Px(7.0),
                padding: UiRect::all(Val::Px(11.0)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(RADIUS)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.88, 0.86, 0.81, 0.52)),
            BorderColor::from(PLATE_RULE_SOFT),
        ))
        .with_children(|card| {
            card.spawn((
                Text::new(format!(
                    "{}  /  {} SITE{}  /  {} STORAGE HALL{}  /  {} OF {} BULK USED",
                    branch.settlement.to_uppercase(),
                    branch.sites,
                    if branch.sites == 1 { "" } else { "S" },
                    branch.storage_halls,
                    if branch.storage_halls == 1 { "" } else { "S" },
                    branch.used_bulk,
                    branch.bulk_capacity,
                )),
                TextFont {
                    font_size: FontSize::Px(9.5),
                    ..default()
                },
                TextColor(ACCENT_COLOR),
            ));
            for (good, held, policy) in &branch.resources {
                let unit_capacity = branch.bulk_capacity / good.bulk_per_unit().max(1);
                card.spawn(Node {
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(4.0),
                    padding: UiRect::vertical(Val::Px(5.0)),
                    border: UiRect::bottom(Val::Px(1.0)),
                    ..default()
                })
                .with_children(|row| {
                    row.spawn(Node {
                        justify_content: JustifyContent::SpaceBetween,
                        align_items: AlignItems::Center,
                        ..default()
                    })
                    .with_children(|line| {
                        line.spawn((
                            Text::new(format!(
                                "{}  /  {} HELD  /  RETAIN {}",
                                good.label().to_uppercase(),
                                held,
                                policy.retain_units,
                            )),
                            TextFont {
                                font_size: FontSize::Px(9.0),
                                ..default()
                            },
                            TextColor(TEXT_COLOR),
                        ));
                        line.spawn((
                            Text::new(if policy.sell_excess {
                                "SELL EXCESS"
                            } else {
                                "HOLD ALL"
                            }),
                            TextFont {
                                font_size: FontSize::Px(8.0),
                                ..default()
                            },
                            TextColor(TEXT_MUTED),
                        ));
                    });
                    if can_manage {
                        let retained_percent = if unit_capacity == 0 {
                            0.0
                        } else {
                            policy.retain_units.min(unit_capacity) as f32 * 100.0
                                / unit_capacity as f32
                        };
                        row.spawn((
                            Node {
                                width: Val::Percent(100.0),
                                height: Val::Px(5.0),
                                overflow: Overflow::clip_x(),
                                border_radius: BorderRadius::all(Val::Px(3.0)),
                                ..default()
                            },
                            BackgroundColor(Color::srgba(0.20, 0.18, 0.15, 0.12)),
                        ))
                        .with_child((
                            Node {
                                width: Val::Percent(retained_percent),
                                height: Val::Percent(100.0),
                                ..default()
                            },
                            BackgroundColor(ACCENT_COLOR),
                        ));
                        row.spawn(Node {
                            flex_direction: FlexDirection::Row,
                            flex_wrap: FlexWrap::Wrap,
                            column_gap: Val::Px(5.0),
                            row_gap: Val::Px(5.0),
                            ..default()
                        })
                        .with_children(|controls| {
                            for (label, units) in [
                                ("CLEAR", 0),
                                ("-10", policy.retain_units.saturating_sub(10)),
                                ("-1", policy.retain_units.saturating_sub(1)),
                                (
                                    "+1",
                                    policy.retain_units.saturating_add(1).min(unit_capacity),
                                ),
                                (
                                    "+10",
                                    policy.retain_units.saturating_add(10).min(unit_capacity),
                                ),
                                ("MAX", unit_capacity),
                            ] {
                                branch_policy_button(
                                    controls,
                                    CompanyBranchPolicyButton {
                                        company,
                                        action: HeroCompanyAction::SetRetainUnits {
                                            settlement: branch.settlement_id,
                                            good: *good,
                                            units,
                                        },
                                    },
                                    label.to_string(),
                                );
                            }
                            branch_policy_button(
                                controls,
                                CompanyBranchPolicyButton {
                                    company,
                                    action: HeroCompanyAction::SetSellExcess {
                                        settlement: branch.settlement_id,
                                        good: *good,
                                        enabled: !policy.sell_excess,
                                    },
                                },
                                if policy.sell_excess {
                                    "HOLD ALL".to_string()
                                } else {
                                    "SELL EXCESS".to_string()
                                },
                            );
                        });
                    }
                });
            }
        });
}

fn branch_policy_button(
    parent: &mut ChildSpawnerCommands<'_>,
    marker: CompanyBranchPolicyButton,
    label: String,
) {
    parent
        .spawn((
            Button,
            marker,
            Node {
                min_width: Val::Px(42.0),
                height: Val::Px(24.0),
                padding: UiRect::horizontal(Val::Px(7.0)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(RADIUS)),
                ..default()
            },
            BackgroundColor(BUTTON_NORMAL),
            BorderColor::from(BUTTON_BORDER),
        ))
        .with_child((
            Text::new(label),
            TextFont {
                font_size: FontSize::Px(8.0),
                ..default()
            },
            TextColor(INK),
            Pickable::IGNORE,
        ));
}

fn spawn_day_ledger(parent: &mut ChildSpawnerCommands<'_>, label: &str, day: CompanyDayLedger) {
    if day.day == u32::MAX {
        key_value(parent, label, "No completed trading record".to_string());
        return;
    }
    key_value(
        parent,
        &format!("{label} / DAY {}", day.day),
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
        ),
    );
    if day.internal_revenue > 0 || day.internal_input_expense > 0 {
        key_value(
            parent,
            "INTERNAL FLOW MEMO",
            format!(
                "{} supplier credits / {} buyer charges; eliminated from company profit",
                format_money(day.internal_revenue),
                format_money(day.internal_input_expense),
            ),
        );
    }
}

fn spawn_site_card(
    parent: &mut ChildSpawnerCommands<'_>,
    company: CompanyId,
    site: &CompanySiteRecord,
) {
    let flow = match (site.input, site.output) {
        (Some(input), Some(output)) => format!("{} -> {}", input.label(), output.label()),
        (None, Some(output)) => format!("produces {}", output.label()),
        _ => "service site".to_string(),
    };
    let source = site.sourcing.map_or_else(
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
                    .map_or_else(String::new, |supplier| {
                        format!(" from site #{}", supplier.0)
                    })
            )
        },
    );
    parent
        .spawn((
            Node {
                flex_direction: FlexDirection::Column,
                align_items: AlignItems::Stretch,
                row_gap: Val::Px(4.0),
                padding: UiRect::all(Val::Px(10.0)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(RADIUS)),
                ..default()
            },
            BackgroundColor(BUTTON_NORMAL),
            BorderColor::from(PLATE_RULE_SOFT),
        ))
        .with_children(|card| {
            card.spawn(Node {
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::Center,
                ..default()
            })
            .with_children(|line| {
                line.spawn((
                    Text::new(format!(
                        "{} #{}  /  {}",
                        site.kind.label().to_uppercase(),
                        site.id.0,
                        site.settlement.to_uppercase()
                    )),
                    TextFont {
                        font_size: FontSize::Px(10.5),
                        ..default()
                    },
                    TextColor(TEXT_COLOR),
                    Pickable::IGNORE,
                ));
                line.spawn((
                    Text::new(site.state.label().to_uppercase()),
                    TextFont {
                        font_size: FontSize::Px(8.0),
                        ..default()
                    },
                    TextColor(TEXT_MUTED),
                    Pickable::IGNORE,
                ));
            });
            card.spawn((
                Text::new(format!(
                    "{}  /  {}  /  staff {} / {} open / {} max  /  today {}",
                    flow,
                    source,
                    site.workers,
                    site.enabled_positions,
                    site.positions,
                    signed_money(site.current_day.profit()),
                )),
                TextFont {
                    font_size: FontSize::Px(8.5),
                    ..default()
                },
                TextColor(TEXT_MUTED),
                Pickable::IGNORE,
            ));
            if let (Some(output), Some(price)) = (site.output, site.asking_price) {
                card.spawn((
                    Text::new(format!(
                        "{} site stock {} / ask {} coin  /  wage-tax debt {} coin; public excess is set for the whole local branch above",
                        output.label(),
                        site.output_stock,
                        format_money(price),
                        format_money(site.wage_arrears.saturating_add(site.tax_arrears)),
                    )),
                    TextFont {
                        font_size: FontSize::Px(8.5),
                        ..default()
                    },
                    TextColor(TEXT_MUTED),
                    Pickable::IGNORE,
                ));
            }
            if let Some(input) = site.input {
                card.spawn((
                    Text::new(format!(
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
                    )),
                    TextFont {
                        font_size: FontSize::Px(8.5),
                        ..default()
                    },
                    TextColor(TEXT_MUTED),
                    Pickable::IGNORE,
                ));
            }
            card.spawn(Node {
                justify_content: JustifyContent::FlexEnd,
                column_gap: Val::Px(6.0),
                margin: UiRect::top(Val::Px(4.0)),
                ..default()
            })
            .with_children(|actions| {
                detail_button(actions, CompanySiteButton(site.id), "VIEW DETAILS");
                detail_button(
                    actions,
                    CompanyManagementButton {
                        site: site.entity,
                        company,
                    },
                    "MANAGE SITE",
                );
            });
        });
}

fn detail_button<M: Component>(parent: &mut ChildSpawnerCommands<'_>, marker: M, label: &str) {
    parent
        .spawn((
            Button,
            marker,
            Node {
                height: Val::Px(28.0),
                padding: UiRect::horizontal(Val::Px(10.0)),
                justify_content: JustifyContent::Center,
                align_items: AlignItems::Center,
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(RADIUS)),
                ..default()
            },
            BackgroundColor(BUTTON_NORMAL),
            BorderColor::from(BUTTON_BORDER),
        ))
        .with_child((
            Text::new(label),
            TextFont {
                font_size: FontSize::Px(8.0),
                ..default()
            },
            TextColor(INK),
            Pickable::IGNORE,
        ));
}

fn detail_stat(parent: &mut ChildSpawnerCommands<'_>, label: &str, value: String) {
    parent
        .spawn((
            Node {
                width: Val::Percent(31.8),
                min_width: Val::Px(150.0),
                flex_grow: 1.0,
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(3.0),
                padding: UiRect::all(Val::Px(9.0)),
                border: UiRect::all(Val::Px(1.0)),
                border_radius: BorderRadius::all(Val::Px(RADIUS)),
                ..default()
            },
            BackgroundColor(Color::srgba(0.84, 0.81, 0.76, 0.36)),
            BorderColor::from(DIVIDER),
        ))
        .with_children(|card| {
            card.spawn((
                Text::new(label),
                TextFont {
                    font_size: FontSize::Px(8.0),
                    ..default()
                },
                TextColor(TEXT_MUTED),
            ));
            card.spawn((
                Text::new(value),
                TextFont {
                    font_size: FontSize::Px(12.0),
                    ..default()
                },
                TextColor(TEXT_COLOR),
            ));
        });
}

fn spawn_section_title(parent: &mut ChildSpawnerCommands<'_>, title: &str, note: &str) {
    parent
        .spawn(Node {
            justify_content: JustifyContent::SpaceBetween,
            align_items: AlignItems::End,
            margin: UiRect::top(Val::Px(5.0)),
            padding: UiRect::bottom(Val::Px(5.0)),
            border: UiRect::bottom(Val::Px(1.0)),
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                Text::new(title),
                TextFont {
                    font_size: FontSize::Px(10.0),
                    ..default()
                },
                TextColor(ACCENT_COLOR),
            ));
            row.spawn((
                Text::new(note),
                TextFont {
                    font_size: FontSize::Px(7.5),
                    ..default()
                },
                TextColor(TEXT_MUTED),
            ));
        });
}

fn key_value(parent: &mut ChildSpawnerCommands<'_>, label: &str, value: String) {
    parent
        .spawn((
            Node {
                justify_content: JustifyContent::SpaceBetween,
                align_items: AlignItems::FlexStart,
                column_gap: Val::Px(14.0),
                padding: UiRect::vertical(Val::Px(6.0)),
                border: UiRect::bottom(Val::Px(1.0)),
                ..default()
            },
            BorderColor::from(DIVIDER),
        ))
        .with_children(|row| {
            row.spawn((
                Text::new(label),
                TextFont {
                    font_size: FontSize::Px(8.0),
                    ..default()
                },
                TextColor(TEXT_MUTED),
                Node {
                    width: Val::Px(112.0),
                    flex_shrink: 0.0,
                    ..default()
                },
            ));
            row.spawn((
                Text::new(value),
                TextFont {
                    font_size: FontSize::Px(9.5),
                    ..default()
                },
                TextColor(TEXT_COLOR),
                TextLayout::justify(Justify::Right),
                Node {
                    min_width: Val::Px(0.0),
                    flex_grow: 1.0,
                    ..default()
                },
            ));
        });
}

fn spawn_note(parent: &mut ChildSpawnerCommands<'_>, text: &str) {
    parent.spawn((
        Text::new(text),
        TextFont {
            font_size: FontSize::Px(9.5),
            ..default()
        },
        TextColor(TEXT_MUTED),
    ));
}

fn spawn_empty(parent: &mut ChildSpawnerCommands<'_>, text: &str) {
    parent.spawn((
        Text::new(text),
        TextFont {
            font_size: FontSize::Px(11.0),
            ..default()
        },
        TextColor(TEXT_MUTED),
        TextLayout::justify(Justify::Center),
        Node {
            align_self: AlignSelf::Center,
            max_width: Val::Px(360.0),
            margin: UiRect::all(Val::Px(28.0)),
            ..default()
        },
    ));
}

fn output_good(kind: SettlementBuildingKind) -> Option<Good> {
    match kind {
        SettlementBuildingKind::Farmstead => Some(Good::Wheat),
        SettlementBuildingKind::LumberjackHut => Some(Good::Wood),
        SettlementBuildingKind::FishermansHut => Some(Good::Food),
        SettlementBuildingKind::Windmill => Some(Good::Flour),
        SettlementBuildingKind::Bakery => Some(Good::Bread),
        _ => None,
    }
}

fn input_good(kind: SettlementBuildingKind) -> Option<Good> {
    match kind {
        SettlementBuildingKind::Windmill => Some(Good::Wheat),
        SettlementBuildingKind::Bakery => Some(Good::Flour),
        _ => None,
    }
}

fn signed_money(value: i64) -> String {
    format!(
        "{}{} coin",
        if value < 0 { "-" } else { "+" },
        format_money(value.unsigned_abs()),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn company(id: u64, owner: PersonId, cash: u64, assets: u64, debt: u64) -> CompanyRecord {
        CompanyRecord {
            id: CompanyId(id),
            name: format!("Company {id}"),
            founded_day: 1,
            master: owner,
            master_name: "Owner".to_string(),
            account: CompanyAccount {
                cash,
                wage_arrears: debt,
                book_value: assets,
                ..default()
            },
            policy: CompanyManagementPolicy::default(),
            holders: vec![CompanyHolderRecord {
                person: owner,
                name: "Owner".to_string(),
                shares: 1_000,
            }],
            offers: Vec::new(),
            decisions: Vec::new(),
            sites: Vec::new(),
            branches: Vec::new(),
        }
    }

    #[test]
    fn portfolio_interest_is_pro_rata_and_never_overdraws_debt() {
        let owner = PersonId(4);
        let healthy = company(1, owner, 1_000, 3_000, 500);
        assert_eq!(healthy.accounting_equity(), 3_500);
        assert_eq!(healthy.holding_book_interest(250), 875);

        let insolvent = company(2, owner, 100, 50, 1_000);
        assert_eq!(insolvent.accounting_equity(), 0);
    }

    #[test]
    fn holdings_filter_supports_more_than_one_company() {
        let owner = PersonId(8);
        let outsider = PersonId(9);
        let directory = CompanyDirectory {
            records: vec![
                company(1, owner, 0, 0, 0),
                company(2, owner, 0, 0, 0),
                company(3, outsider, 0, 0, 0),
            ],
            local_person: Some(owner),
            local_wallet: Some(2_000),
        };
        let visible = visible_companies(&directory, CompanyFilter::MyHoldings);
        assert_eq!(visible.len(), 2);
        assert_eq!(visible[0].id, CompanyId(1));
        assert_eq!(visible[1].id, CompanyId(2));
    }

    #[test]
    fn every_company_site_exposes_details_and_management_separately() {
        let mut world = World::new();
        let parent = world.spawn_empty().id();
        let site = CompanySiteRecord {
            entity: Entity::from_bits(41),
            id: BuildingId(42),
            settlement: "Oakfell".into(),
            settlement_id: SettlementId(1),
            kind: SettlementBuildingKind::Windmill,
            workers: 1,
            positions: 2,
            enabled_positions: 2,
            state: BusinessState::Operating,
            wage_arrears: 0,
            tax_arrears: 0,
            current_day: default(),
            previous_day: default(),
            output: Some(Good::Flour),
            output_stock: 4,
            asking_price: Some(100),
            input: Some(Good::Wheat),
            input_stock: 9,
            input_target: 18,
            input_coverage_days: 2,
            sourcing: Some(BusinessSourcingMode::PreferOwned),
            preferred_supplier: None,
            goods: vec![(Good::Flour, 4), (Good::Wheat, 9)],
            used_bulk: 13,
            bulk_capacity: 240,
        };
        world
            .commands()
            .entity(parent)
            .with_children(|children| spawn_site_card(children, CompanyId(43), &site));
        world.flush();

        let mut details = world.query_filtered::<&CompanySiteButton, With<Button>>();
        let mut management = world.query_filtered::<&CompanyManagementButton, With<Button>>();
        assert_eq!(details.single(&world).unwrap().0, site.id);
        assert_eq!(management.single(&world).unwrap().site, site.entity);
        assert_eq!(management.single(&world).unwrap().company, CompanyId(43));
    }
}
