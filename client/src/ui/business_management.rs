//! Company and workplace policy pages with explicit scopes.
//!
//! The panel edits replicated policies rather than maintaining client-only
//! settings. NPC autopilot and player management therefore remain one economy.
//!
//! Rendering follows the build-once/bind-in-place rule from
//! `docs/UI-ARCHITECTURE.md`: every frame the replicated state is folded into a
//! pure [`ControlsModel`] (sections, rows, controls and meters, each with a
//! stable id). A `u64` hash of that id sequence is the panel's *structure
//! key*; only a change in it respawns nodes. Everything else - values, button
//! labels, selected chrome, the payload a button will send (`Action`,
//! `PersonLink`, `ShareDraftAction`, `DividendDraftAction`), meter fills and
//! whether a fixed slot is occupied - is written into the existing entities by
//! id, so stepping a wage, posting a share offer, drafting a dividend amount,
//! a worker leaving replication interest, a partial share purchase or an
//! economic tick never rebuilds the scroll body or steals the cursor's hover.
//!
//! Dividends: the TREASURY & DIVIDENDS section binds the replicated
//! `CompanyDividendCapacity` (available now, reserves, last paid), an amount
//! picker (`DividendDraft` stepped by `DividendDraftAction` controls), one
//! always-present confirm control whose payload is
//! `DistributeDividend { pennies: min(draft, distributable) }` (`u64::MAX`,
//! everything, while the snapshot shows nothing, so the server answers from
//! its live figure) and a preview row computed with the shared
//! `pro_rata_split`, so the local holder's take matches the server's payout
//! penny for penny. The server answers one tick later with a
//! `HeroCompanyResult`, which lands in the feedback line through
//! `encyclopedia::companies::receive_company_policy_results`. The CONTRIBUTE
//! PERSONAL COIN row (any shareholder) mirrors the picker with a
//! `CapitalDraft` stepped by `CapitalDraftAction` controls and clamped to the
//! local hero's replicated `Wallet`.
//!
//! Ids that would otherwise leak volatile identity are fixed slots:
//! `worker.{i}` for `i in 0..kind.positions()` (observed employees, deduped
//! and sorted by `PersonId`; unused slots are vacant) and `buy.{seller}.{k}`
//! for `k in 0..3` (BUY 1 / BUY 10 / BUY ALL per public offer; duplicate
//! quantities are vacant). A vacant slot keeps its entity with
//! `Display::None`, an empty label and no person or order behind it.
//!
//! Diagnostics: `FISTFORCE_OPEN_BUSINESS=company[:<id>]` opens COMPANY
//! SETTINGS for the lowest-id (or the named) replicated company a few seconds
//! into gameplay, so `ClientPerfUi ensure_business_panel=Nc/1r` can be
//! measured without input automation.

use bevy::platform::collections::HashMap;

use bevy::prelude::*;
use lightyear::prelude::{Connected, MessageReceiver, MessageSender};
use shared::components::{
    BuildingId, CharacterName, Company, CompanyId, CompanyLeadership, CompanyOwnership,
    CompanyShareMarket, EmployedAt, Hero, OperatedBy, OwnedBy, PersonId, SettlementBuilding,
    SettlementBuildingKind,
};
use shared::economy::{
    format_money, pro_rata_split, BusinessAccount, BusinessManagementPolicy,
    BusinessProcurementPolicy, BusinessSalePolicy, BusinessSourcingMode, BusinessStaffingPolicy,
    BusinessStrategy, BusinessSupplyPolicy, BusinessWagePolicy, CompanyAccount,
    CompanyDecisionHistory, CompanyDividendCapacity, CompanyManagementPolicy, Good, GoodsInventory,
    TavernService, Wallet, COMPANY_DIVIDEND_FLOAT, PENNIES_PER_COIN,
};
use shared::protocol::{
    HeroBusinessAction, HeroBusinessOrder, HeroBusinessResult, HeroCompanyAction, HeroCompanyOrder,
    ReliableChannel,
};

use crate::states::GameState;
use crate::ui::encyclopedia::person_links::PersonLink;
use crate::ui::foundation::{
    retained_scroll, selected_button_chrome, subtree_is_interacting, UiButtonLabel, UiButtonStyle,
    UiButtonVariant,
};
use crate::ui::modal::update_modal_click_guard;
use crate::ui::styles::{INK, INK_MUTED, LIMEWASH_WELL, PLATE_RULE_SOFT, RADIUS};

pub struct BusinessManagementPlugin;

impl Plugin for BusinessManagementPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<BusinessManagementTarget>();
        app.init_resource::<BusinessManagementReturn>();
        app.init_resource::<BusinessManagementPage>();
        app.init_resource::<BusinessClickGuard>();
        app.init_resource::<BusinessFeedback>();
        app.init_resource::<ShareOrderDraft>();
        app.init_resource::<DividendDraft>();
        app.init_resource::<CapitalDraft>();
        app.add_systems(
            Update,
            (
                receive_results,
                ensure_panel.in_set(EnsureBusinessPanel),
                update_guard,
                handle_action_buttons,
                handle_share_draft_buttons,
                handle_dividend_draft_buttons,
                handle_capital_draft_buttons,
                handle_page_buttons,
                sync_input_state,
            )
                .chain()
                .run_if(in_state(GameState::Playing)),
        );
        app.add_systems(
            Update,
            auto_open_for_diagnostics
                .before(ensure_panel)
                .run_if(in_state(GameState::Playing))
                .run_if(auto_open_requested),
        );
        app.add_systems(OnExit(GameState::Playing), cleanup);
    }
}

#[derive(Resource, Default)]
pub(crate) struct BusinessManagementTarget(pub Option<BusinessManagementSelection>);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BusinessManagementSelection {
    Site(Entity),
    Company(CompanyId),
}

/// Optional encyclopedia destination used when a site was opened from its
/// company record. The X still dismisses the whole flow; BACK restores it.
#[derive(Resource, Default, Clone, Copy)]
pub(crate) struct BusinessManagementReturn(pub Option<CompanyId>);

/// The visible scope selects controls, each carrying its explicit site/company address.
/// Encyclopedia entry points choose Company for company management and Site for a workplace.
#[derive(Resource, Default, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BusinessManagementPage {
    #[default]
    Site,
    Company,
}

#[derive(Component)]
struct PageTab(BusinessManagementPage);

#[derive(Component)]
struct PageBody(BusinessManagementPage);

#[derive(Component)]
struct ControlPage(BusinessManagementPage);

#[derive(Resource, Default)]
struct BusinessClickGuard(bool);

#[derive(Resource, Default)]
pub(crate) struct BusinessFeedback {
    pub(crate) message: String,
    pub(crate) success: bool,
    context: Option<(BusinessManagementSelection, BusinessManagementPage)>,
}

impl BusinessFeedback {
    /// A reply already attributed to `target`'s `page`, so `ensure_panel`
    /// keeps it instead of clearing a message from another context. Capture
    /// fixtures use it to photograph the feedback line without a server.
    pub(crate) fn attributed(
        target: BusinessManagementSelection,
        page: BusinessManagementPage,
        message: impl Into<String>,
        success: bool,
    ) -> Self {
        Self {
            message: message.into(),
            success,
            context: Some((target, page)),
        }
    }
}

#[derive(Component)]
struct Root {
    structure: u64,
    target: BusinessManagementSelection,
}

/// The page's scrolling body; `pub(crate)` so the capture harness can scroll it.
#[derive(Component)]
pub(crate) struct BodyScroll;

/// A text node whose content is bound by model id every frame.
#[derive(Component)]
struct BoundText(BoundId);

/// A button whose label, selected chrome, visibility and payload (`Action`,
/// `PersonLink`, `ShareDraftAction`, `DividendDraftAction` or
/// `CapitalDraftAction`) are bound by model id every frame.
#[derive(Component)]
struct BoundButton(BoundId);

/// One lane of a meter track; its width is bound by model id.
#[derive(Component)]
struct MeterFill {
    id: BoundId,
    lane: usize,
}

/// Where a bound id's value lives in the current [`ControlsModel`]; the
/// per-frame scratch map holds these indices so it can be reused across
/// frames without borrowing the model.
#[derive(Clone, Copy)]
enum Slot {
    Title,
    Subtitle,
    Feedback,
    Row(usize),
    Control(usize, usize),
    Meter(usize),
}

/// Scratch collections reused by `ensure_panel` every frame, so steady state
/// allocates nothing per frame. The people index and the worker list are
/// refreshed only when replication reports a person, name or employment
/// change (or the selected site changes), not rebuilt per frame.
struct PanelScratch {
    /// Replicated people by durable id, for O(1) name lookups.
    people: HashMap<PersonId, Entity>,
    /// Observed employees of the selected site (see `observed_workers`).
    workers: Vec<PersonId>,
    /// The site `workers` was collected for.
    workers_site: Option<BuildingId>,
    /// Bound id -> model slot for the bind pass.
    slots: HashMap<BoundId, Slot>,
    /// Pre-hashed ids of the three fixed text slots.
    fixed: FixedSlots,
}

impl Default for PanelScratch {
    fn default() -> Self {
        Self {
            people: HashMap::default(),
            workers: Vec::new(),
            workers_site: None,
            slots: HashMap::default(),
            fixed: FixedSlots {
                title: BoundId::of("title"),
                subtitle: BoundId::of("subtitle"),
                feedback: BoundId::of("feedback"),
            },
        }
    }
}

#[derive(Clone, Copy)]
struct FixedSlots {
    title: BoundId,
    subtitle: BoundId,
    feedback: BoundId,
}

/// Replicated people plus the change signals that decide whether the panel's
/// `PersonId -> Entity` index and worker list must be refreshed this frame.
#[derive(bevy::ecs::system::SystemParam)]
struct ReplicatedPeople<'w, 's> {
    people: Query<
        'w,
        's,
        (
            Entity,
            &'static PersonId,
            &'static CharacterName,
            Option<&'static EmployedAt>,
        ),
    >,
    changed: Query<'w, 's, (), Or<(Added<PersonId>, Changed<CharacterName>, Changed<EmployedAt>)>>,
    removed_people: RemovedComponents<'w, 's, PersonId>,
    removed_jobs: RemovedComponents<'w, 's, EmployedAt>,
}

impl ReplicatedPeople<'_, '_> {
    /// Drain every change signal; true when the index built last frame no
    /// longer describes replication.
    fn changed_since_last_frame(&mut self) -> bool {
        let removed_people = self.removed_people.read().count() > 0;
        let removed_jobs = self.removed_jobs.read().count() > 0;
        removed_people || removed_jobs || !self.changed.is_empty()
    }
}

/// Panel state plus the encyclopedia host this page renders into; bundled so
/// `ensure_panel` stays under Bevy's parameter limit.
#[derive(bevy::ecs::system::SystemParam)]
struct ManagementPanelUi<'w, 's> {
    roots: Query<'w, 's, (Entity, &'static Root)>,
    body_scroll: Query<'w, 's, (&'static PageBody, &'static ScrollPosition), With<BodyScroll>>,
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
    known_people: Res<'w, crate::ui::encyclopedia::KnownPeople>,
}

/// The entities the bind pass writes into. Every query that touches `Node`
/// excludes the markers of the others so Bevy can prove them disjoint.
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
        Without<BoundButton>,
    >,
    buttons: Query<
        'w,
        's,
        (
            &'static BoundButton,
            &'static mut UiButtonStyle,
            &'static mut Node,
            Option<&'static mut Action>,
            Option<&'static mut PersonLink>,
            Option<&'static mut ShareDraftAction>,
            Option<&'static mut DividendDraftAction>,
            Option<&'static mut CapitalDraftAction>,
        ),
        (Without<BoundText>, Without<MeterFill>, Without<PageBody>),
    >,
    fills: Query<'w, 's, (&'static MeterFill, &'static mut Node), Without<BoundText>>,
    pages: Query<
        'w,
        's,
        (&'static PageBody, &'static mut Node),
        (Without<BoundText>, Without<MeterFill>),
    >,
    tabs: Query<'w, 's, (&'static PageTab, &'static mut UiButtonStyle), Without<BoundButton>>,
}

#[derive(Component, Clone, Copy)]
struct Action(ControlPress);

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

/// One step of the dividend amount picker. Steps are ±1 coin or a fraction of
/// the replicated `CompanyDividendCapacity::distributable`; every step clamps
/// the draft into `0..=distributable`.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum DividendDraftAction {
    Down,
    Up,
    Quarter,
    Half,
    All,
}

/// The amount the Company Master is about to distribute, in pennies. Until
/// the player steps it, the draft follows everything distributable (so a
/// company opened before its first finance review, or with no headroom yet,
/// picks up the published figure when it appears); the confirm control sends
/// `min(pennies, distributable)` and the server clamps again against its live
/// figure.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct DividendDraft {
    pub(crate) company: Option<CompanyId>,
    pub(crate) pennies: u64,
    /// True once the player has stepped the amount for this company.
    pub(crate) edited: bool,
}

impl DividendDraft {
    /// Everything distributable, not yet edited by the player.
    pub(crate) fn all_of(company: CompanyId, distributable: u64) -> Self {
        Self {
            company: Some(company),
            pennies: distributable,
            edited: false,
        }
    }

    /// Apply one picker step against the company's current headroom.
    pub(crate) fn step(&mut self, action: DividendDraftAction, distributable: u64) {
        self.edited = true;
        let current = self.pennies.min(distributable);
        self.pennies = match action {
            DividendDraftAction::Down => current.saturating_sub(PENNIES_PER_COIN),
            DividendDraftAction::Up => current.saturating_add(PENNIES_PER_COIN),
            DividendDraftAction::Quarter => distributable / 4,
            DividendDraftAction::Half => distributable / 2,
            DividendDraftAction::All => distributable,
        }
        .min(distributable);
    }
}

/// One step of the capital-contribution picker: ±1 coin, +10 coin or the
/// whole wallet; every step clamps the draft into `0..=wallet`.
#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CapitalDraftAction {
    Down,
    Up,
    TenUp,
    All,
}

/// The donation a shareholder is about to make, in pennies. Until the player
/// steps it, the draft follows one coin of the local hero's replicated
/// `Wallet` (so a panel opened before the wallet replicates does not stay
/// pinned at zero); every step and the confirm payload clamp to that wallet,
/// and the server checks it again when it debits.
#[derive(Resource, Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct CapitalDraft {
    pub(crate) company: Option<CompanyId>,
    pub(crate) pennies: u64,
    /// True once the player has stepped the amount for this company.
    pub(crate) edited: bool,
}

impl CapitalDraft {
    /// One coin (or the whole wallet below that), not yet edited by the player.
    pub(crate) fn for_company(company: CompanyId, wallet: u64) -> Self {
        Self {
            company: Some(company),
            pennies: PENNIES_PER_COIN.min(wallet),
            edited: false,
        }
    }

    /// Apply one picker step against the local wallet.
    pub(crate) fn step(&mut self, action: CapitalDraftAction, wallet: u64) {
        self.edited = true;
        let current = self.pennies.min(wallet);
        self.pennies = match action {
            CapitalDraftAction::Down => current.saturating_sub(PENNIES_PER_COIN),
            CapitalDraftAction::Up => current.saturating_add(PENNIES_PER_COIN),
            CapitalDraftAction::TenUp => current.saturating_add(10 * PENNIES_PER_COIN),
            CapitalDraftAction::All => wallet,
        }
        .min(wallet);
    }
}

/// The replicated wallet of the local hero, or zero while none is known.
fn local_hero_wallet<'a>(
    local: Option<&crate::camera_rts::LocalPeerId>,
    heroes: impl Iterator<Item = (&'a Hero, Option<&'a Wallet>)>,
) -> u64 {
    let Some(local) = local else {
        return 0;
    };
    heroes
        .into_iter()
        .find(|(hero, _)| shared::player::peer_id_to_u64(hero.owner) == local.0)
        .and_then(|(_, wallet)| wallet)
        .map_or(0, |wallet| wallet.balance())
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

mod model;
pub(crate) use model::local_dividend_take;
use model::*;

// --- systems ----------------------------------------------------------------

/// The set `ensure_panel` runs in, so other panels' systems (the encyclopedia
/// person-link sync) can order after it without seeing its private params.
#[derive(SystemSet, Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct EnsureBusinessPanel;

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn ensure_panel(
    mut commands: Commands,
    mut target: ResMut<BusinessManagementTarget>,
    mut feedback: ResMut<BusinessFeedback>,
    mut page: ResMut<BusinessManagementPage>,
    mut share_draft: ResMut<ShareOrderDraft>,
    mut dividend_draft: ResMut<DividendDraft>,
    mut capital_draft: ResMut<CapitalDraft>,
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
        Option<&OwnedBy>,
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
        Option<&CompanyDividendCapacity>,
    )>,
    mut replicated: ReplicatedPeople,
    heroes: Query<(&Hero, &PersonId, Option<&Wallet>)>,
    mut ui: ManagementPanelUi,
    mut bound: BoundControls,
    mut scratch: Local<PanelScratch>,
) {
    let mut _ui_scope = ui.perf.scope("ensure_business_panel");
    let Some(entity) = target.0 else {
        if !feedback.message.is_empty() || feedback.context.is_some() {
            feedback.message.clear();
            feedback.context = None;
        }
        for (root, ..) in ui.roots.iter() {
            commands.entity(root).despawn();
        }
        // Change signals are not read while closed; rebuild the index on open.
        scratch.people.clear();
        scratch.workers_site = None;
        return;
    };
    // Company controls are an encyclopedia page: open the window on the
    // companies tab if needed and host the page next frame.
    let Ok(host) = ui.hosts.single() else {
        if !ui.encyclopedia_open.0 {
            ui.encyclopedia_open.0 = true;
            *ui.tab = crate::ui::encyclopedia::EncyclopediaTab::Companies;
        }
        scratch.people.clear();
        scratch.workers_site = None;
        return;
    };
    let selected_site = match entity {
        BusinessManagementSelection::Site(site) => businesses.get(site).ok(),
        BusinessManagementSelection::Company(_) => None,
    };
    if matches!(entity, BusinessManagementSelection::Site(_)) && selected_site.is_none() {
        target.0 = None;
        for (root, ..) in ui.roots.iter() {
            commands.entity(root).despawn();
        }
        return;
    }
    let company_id = match entity {
        BusinessManagementSelection::Company(id) => Some(id),
        BusinessManagementSelection::Site(_) => selected_site
            .as_ref()
            .and_then(|site| site.2.map(|operation| operation.0)),
    };
    let company = company_id
        .and_then(|wanted| companies.iter().find(|(id, ..)| **id == wanted))
        .map(
            |(
                id,
                company,
                ownership,
                leadership,
                account,
                policy,
                decisions,
                share_market,
                capacity,
            )| {
                CompanyView {
                    id: *id,
                    company,
                    ownership,
                    leadership,
                    account,
                    policy,
                    decisions,
                    share_market,
                    capacity: capacity.copied(),
                }
            },
        );
    if matches!(entity, BusinessManagementSelection::Company(_)) && company.is_none() {
        target.0 = None;
        for (root, ..) in ui.roots.iter() {
            commands.entity(root).despawn();
        }
        return;
    }
    let local_person = local.as_ref().and_then(|local| {
        heroes
            .iter()
            .find(|(hero, ..)| shared::player::peer_id_to_u64(hero.owner) == local.0)
            .map(|(_, person, _)| *person)
    });
    let local_wallet = local_hero_wallet(
        local.as_deref(),
        heroes.iter().map(|(hero, _, wallet)| (hero, wallet)),
    );
    if let Some(company) = company.as_ref() {
        if share_draft.company != Some(company.id) {
            *share_draft = ShareOrderDraft {
                company: Some(company.id),
                ..default()
            };
        }
        if capital_draft.company != Some(company.id) {
            *capital_draft = CapitalDraft::for_company(company.id, local_wallet);
        } else if !capital_draft.edited {
            capital_draft.set_if_neq(CapitalDraft::for_company(company.id, local_wallet));
        }
        // Start from everything distributable, the same amount the old single
        // DISTRIBUTE AVAILABLE control sent, and keep following the published
        // headroom until the player steps the amount: a company opened before
        // its first finance review must not pin the draft at zero.
        if dividend_draft.company != Some(company.id) {
            *dividend_draft = DividendDraft::all_of(company.id, company.distributable());
        } else if !dividend_draft.edited {
            dividend_draft.set_if_neq(DividendDraft::all_of(company.id, company.distributable()));
        }
    }
    // One `PersonId -> Entity` index instead of a linear scan per looked-up
    // name, refreshed only when replication changed; the map keeps its
    // capacity across frames.
    let people_changed = replicated.changed_since_last_frame();
    let PanelScratch {
        people: person_index,
        workers,
        workers_site,
        slots,
        fixed,
    } = &mut *scratch;
    let people = &replicated.people;
    let refresh_index = people_changed || person_index.is_empty();
    if refresh_index {
        person_index.clear();
        person_index.extend(people.iter().map(|(body, person, ..)| (*person, body)));
    }
    let name_of = |person: PersonId| {
        person_index
            .get(&person)
            .and_then(|body| people.get(*body).ok())
            .map_or_else(
                || {
                    ui.known_people
                        .find_by_id(person)
                        .filter(|record| record.known || record.is_self)
                        .map_or_else(
                            || format!("Person #{}", person.0),
                            |record| record.name.clone(),
                        )
                },
                |(_, _, name, _)| name.0.clone(),
            )
    };
    if selected_site.is_none() {
        page.set_if_neq(BusinessManagementPage::Company);
    } else if company.is_none() {
        page.set_if_neq(BusinessManagementPage::Site);
    }
    let context = Some((entity, *page));
    if feedback.context != context {
        feedback.message.clear();
        feedback.context = context;
    }
    let site_id = selected_site.as_ref().map(|site| *site.1);
    if refresh_index || *workers_site != site_id {
        *workers_site = site_id;
        match site_id {
            Some(site) => observed_workers(
                workers,
                site,
                people
                    .iter()
                    .map(|(_, person, _, employment)| (*person, employment.map(|e| e.0))),
            ),
            None => workers.clear(),
        }
    }
    let model = controls_model(&ModelInputs {
        workers,
        site: selected_site.map(
            |(
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
                owner,
            )| SiteView {
                building,
                building_id: *building_id,
                company: operated_by.map(|operation| operation.0),
                owner: owner.map(|owner| owner.0),
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
        ),
        company,
        local_person,
        local_wallet,
        share_draft: &share_draft,
        dividend_draft: &dividend_draft,
        capital_draft: &capital_draft,
        page: *page,
        feedback: &feedback,
        name_of: &name_of,
    });
    let structure = model.structure_key();
    let existing = ui.roots.iter().next();
    if existing.is_some_and(|(_, root)| root.structure == structure && root.target == entity) {
        bind_panel(&model, &mut bound, slots, *fixed);
        return;
    }
    // A structural change while a control is pressed would pull the button
    // out from under the cursor; wait a frame.
    if existing.is_some_and(|(root, state)| {
        state.target == entity && subtree_is_interacting(root, &ui.children, &ui.interactions)
    }) {
        return;
    }
    let mut scroll = [Vec2::ZERO; 2];
    for (page, position) in ui.body_scroll.iter() {
        scroll[page.0.index()] = retained_scroll(
            existing.is_some_and(|(_, root)| root.target == entity),
            Some(position.0),
        );
    }
    for (root, ..) in ui.roots.iter() {
        commands.entity(root).despawn();
    }
    _ui_scope.rebuilt();
    spawn_panel(&mut commands, host, entity, structure, &model, scroll);
}

/// Write the model's values into the spawned tree without touching structure.
/// `slots` is reused scratch keyed by the ids the model pre-hashed when it was
/// built: after warm-up the pass allocates nothing and hashes no strings.
fn bind_panel(
    model: &ControlsModel,
    bound: &mut BoundControls,
    slots: &mut HashMap<BoundId, Slot>,
    fixed: FixedSlots,
) {
    slots.clear();
    slots.insert(fixed.title, Slot::Title);
    slots.insert(fixed.subtitle, Slot::Subtitle);
    slots.insert(fixed.feedback, Slot::Feedback);
    for (index, block) in model.blocks.iter().enumerate() {
        match block {
            Block::Section(_) | Block::Scope(_) => {}
            Block::Row(row) => {
                slots.insert(row.bound, Slot::Row(index));
                for (control_index, control) in row.controls.iter().enumerate() {
                    slots.insert(control.bound, Slot::Control(index, control_index));
                }
            }
            Block::Meter(meter) => {
                slots.insert(meter.bound, Slot::Meter(index));
            }
        }
    }
    let control_at = |slot: Slot| match slot {
        Slot::Control(block, control) => match &model.blocks[block] {
            Block::Row(row) => Some(&row.controls[control]),
            _ => None,
        },
        _ => None,
    };
    let (feedback_text, feedback_color) = model
        .feedback
        .as_ref()
        .map_or(("", FEEDBACK_OK), |(m, ok)| {
            (m.as_str(), if *ok { FEEDBACK_OK } else { FEEDBACK_FAIL })
        });
    for (bound_text, mut text, mut color, mut node) in bound.texts.iter_mut() {
        let Some(slot) = slots.get(&bound_text.0) else {
            continue;
        };
        let (value, tint): (&str, Option<Color>) = match *slot {
            Slot::Title => (&model.title, None),
            Slot::Subtitle => (&model.subtitle, None),
            Slot::Feedback => (feedback_text, Some(feedback_color)),
            Slot::Row(block) => match &model.blocks[block] {
                Block::Row(row) => (&row.value, None),
                _ => continue,
            },
            Slot::Control(..) => match control_at(*slot) {
                Some(control) => (&control.label, None),
                None => continue,
            },
            Slot::Meter(block) => match &model.blocks[block] {
                Block::Meter(meter) => (&meter.summary, None),
                _ => continue,
            },
        };
        if text.0 != value {
            text.0.clear();
            text.0.push_str(value);
        }
        if let Some(tint) = tint {
            if color.0 != tint {
                color.0 = tint;
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
    for (bound_button, mut style, mut node, action, person, draft, dividend, capital) in
        bound.buttons.iter_mut()
    {
        let Some(control) = slots
            .get(&bound_button.0)
            .and_then(|slot| control_at(*slot))
        else {
            continue;
        };
        if style.selected != control.selected {
            style.selected = control.selected;
        }
        let display = layout::control_display(control);
        if node.display != display {
            node.display = display;
        }
        if let Some(mut action) = action {
            if action.0 != control.press {
                action.0 = control.press;
            }
        }
        if let Some(mut person) = person {
            let wanted = layout::control_person(control);
            if person.0 != wanted {
                person.0 = wanted;
            }
        }
        if let (Some(mut draft), ControlPress::Draft(step)) = (draft, control.press) {
            if *draft != step {
                *draft = step;
            }
        }
        if let (Some(mut dividend), ControlPress::DividendDraft(step)) = (dividend, control.press) {
            if *dividend != step {
                *dividend = step;
            }
        }
        if let (Some(mut capital), ControlPress::CapitalDraft(step)) = (capital, control.press) {
            if *capital != step {
                *capital = step;
            }
        }
    }
    for (page, mut node) in bound.pages.iter_mut() {
        let display = page_display(page.0, model.page);
        if node.display != display {
            node.display = display;
        }
    }
    for (tab, mut style) in bound.tabs.iter_mut() {
        let selected = tab.0 == model.page;
        if style.selected != selected {
            style.selected = selected;
        }
    }
    for (fill, mut node) in bound.fills.iter_mut() {
        let Some(Slot::Meter(block)) = slots.get(&fill.id) else {
            continue;
        };
        let Block::Meter(meter) = &model.blocks[*block] else {
            continue;
        };
        let width = Val::Percent(meter.lanes[fill.lane]);
        if node.width != width {
            node.width = width;
        }
    }
}

mod layout;
use layout::{page_display, spawn_panel};

fn handle_share_draft_buttons(
    guard: Res<BusinessClickGuard>,
    mut draft: ResMut<ShareOrderDraft>,
    page: Res<BusinessManagementPage>,
    buttons: Query<(&Interaction, &ShareDraftAction, &ControlPage), Changed<Interaction>>,
) {
    for (interaction, action, control_page) in buttons.iter() {
        if *interaction != Interaction::Pressed || !guard.0 || control_page.0 != *page {
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

/// Step the dividend amount picker. The clamp uses the replicated capacity of
/// the drafted company; a company without a published snapshot yet has no
/// headroom to draft against.
fn handle_dividend_draft_buttons(
    guard: Res<BusinessClickGuard>,
    mut draft: ResMut<DividendDraft>,
    page: Res<BusinessManagementPage>,
    buttons: Query<(&Interaction, &DividendDraftAction, &ControlPage), Changed<Interaction>>,
    companies: Query<(&CompanyId, Option<&CompanyDividendCapacity>)>,
) {
    for (interaction, action, control_page) in buttons.iter() {
        if *interaction != Interaction::Pressed || !guard.0 || control_page.0 != *page {
            continue;
        }
        let distributable = draft
            .company
            .and_then(|wanted| companies.iter().find(|(id, _)| **id == wanted))
            .and_then(|(_, capacity)| capacity)
            .map_or(0, |capacity| capacity.distributable);
        draft.step(*action, distributable);
    }
}

/// Step the capital-contribution picker against the local hero's wallet.
fn handle_capital_draft_buttons(
    guard: Res<BusinessClickGuard>,
    mut draft: ResMut<CapitalDraft>,
    page: Res<BusinessManagementPage>,
    buttons: Query<(&Interaction, &CapitalDraftAction, &ControlPage), Changed<Interaction>>,
    local: Option<Res<crate::camera_rts::LocalPeerId>>,
    heroes: Query<(&Hero, Option<&Wallet>)>,
) {
    for (interaction, action, control_page) in buttons.iter() {
        if *interaction != Interaction::Pressed || !guard.0 || control_page.0 != *page {
            continue;
        }
        let wallet = local_hero_wallet(local.as_deref(), heroes.iter());
        draft.step(*action, wallet);
    }
}

fn handle_action_buttons(
    guard: Res<BusinessClickGuard>,
    target: Res<BusinessManagementTarget>,
    page: Res<BusinessManagementPage>,
    buttons: Query<(&Interaction, &Action, &ControlPage), Changed<Interaction>>,
    mut clients: Query<
        &mut MessageSender<HeroBusinessOrder>,
        (With<crate::GameClient>, With<Connected>),
    >,
    mut company_clients: Query<
        &mut MessageSender<HeroCompanyOrder>,
        (With<crate::GameClient>, With<Connected>),
    >,
) {
    for (interaction, action, control_page) in buttons.iter() {
        if *interaction != Interaction::Pressed || !guard.0 || control_page.0 != *page {
            continue;
        }
        match action.0 {
            ControlPress::Order(action) => {
                let (Some(BusinessManagementSelection::Site(business)), Ok(mut sender)) =
                    (target.0, clients.single_mut())
                else {
                    continue;
                };
                sender.send::<ReliableChannel>(HeroBusinessOrder { business, action });
            }
            ControlPress::Company(company, action) => {
                if let Ok(mut sender) = company_clients.single_mut() {
                    sender.send::<ReliableChannel>(HeroCompanyOrder { company, action });
                }
            }
            ControlPress::Draft(_)
            | ControlPress::DividendDraft(_)
            | ControlPress::CapitalDraft(_)
            | ControlPress::Person(_)
            | ControlPress::Vacant(_) => {}
        }
    }
}

fn handle_page_buttons(
    guard: Res<BusinessClickGuard>,
    mut page: ResMut<BusinessManagementPage>,
    buttons: Query<(&Interaction, &PageTab), Changed<Interaction>>,
) {
    for (interaction, tab) in buttons.iter() {
        if *interaction == Interaction::Pressed && guard.0 {
            *page = tab.0;
        }
    }
}

fn receive_results(
    mut receivers: Query<&mut MessageReceiver<HeroBusinessResult>, With<crate::GameClient>>,
    target: Res<BusinessManagementTarget>,
    page: Res<BusinessManagementPage>,
    mut feedback: ResMut<BusinessFeedback>,
    mut sounds: crate::ui::sound::UiActionSounds,
) {
    for mut receiver in receivers.iter_mut() {
        for result in receiver.receive() {
            sounds.result(result.success);
            if target.0 == Some(BusinessManagementSelection::Site(result.business))
                && *page == BusinessManagementPage::Site
            {
                feedback.message = result.message;
                feedback.success = result.success;
            }
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

/// Diagnostics: `FISTFORCE_OPEN_BUSINESS=company[:<id>]` opens COMPANY
/// SETTINGS for the lowest-id (or the named) replicated company a few seconds
/// into gameplay, for perf runs on a machine with no input automation.
fn auto_open_requested(mut cached: Local<Option<bool>>) -> bool {
    // Cached once: the flag is static for the process, and the opener must
    // not hold `ResMut` access every frame of every ordinary session.
    *cached.get_or_insert_with(|| std::env::var("FISTFORCE_OPEN_BUSINESS").is_ok())
}

fn auto_open_for_diagnostics(
    time: Res<Time>,
    capture: Option<Res<crate::capture::CaptureConfig>>,
    input_state: Res<crate::input::InputState>,
    companies: Query<&CompanyId>,
    mut target: ResMut<BusinessManagementTarget>,
    mut armed_at: Local<Option<f32>>,
    mut done: Local<bool>,
) {
    if *done {
        return;
    }
    // Capture runs stage their own fixtures (`FISTFORCE_CAPTURE_ENCYCLOPEDIA`).
    if capture.is_some() {
        *done = true;
        return;
    }
    let Ok(raw) = std::env::var("FISTFORCE_OPEN_BUSINESS") else {
        *done = true;
        return;
    };
    let raw = raw.trim().to_ascii_lowercase();
    let Some(wanted) = raw.strip_prefix("company") else {
        *done = true;
        return;
    };
    let wanted = match wanted.strip_prefix(':') {
        None if wanted.is_empty() => None,
        Some(id) => match id.parse::<u64>() {
            Ok(id) => Some(CompanyId(id)),
            Err(_) => {
                *done = true;
                return;
            }
        },
        None => {
            *done = true;
            return;
        }
    };
    let now = time.elapsed_secs();
    let armed = *armed_at.get_or_insert(now);
    // Same rule as the N key: never open over another modal. Companies
    // replicate after the hero spawns, so keep waiting until one is known.
    if now - armed < 5.0 || input_state.ui_blocking() {
        return;
    }
    let company = match wanted {
        Some(id) => companies
            .iter()
            .find(|candidate| **candidate == id)
            .copied(),
        None => companies.iter().min().copied(),
    };
    let Some(company) = company else {
        return;
    };
    target.0 = Some(BusinessManagementSelection::Company(company));
    *done = true;
    info!(
        "FISTFORCE_OPEN_BUSINESS: opened company settings for company #{}",
        company.0
    );
}

fn cleanup(
    mut commands: Commands,
    roots: Query<Entity, With<Root>>,
    mut target: ResMut<BusinessManagementTarget>,
    mut return_to: ResMut<BusinessManagementReturn>,
    mut page: ResMut<BusinessManagementPage>,
    mut input: ResMut<crate::input::InputState>,
) {
    for root in roots.iter() {
        commands.entity(root).despawn();
    }
    target.0 = None;
    return_to.0 = None;
    *page = BusinessManagementPage::Site;
    input.business_management_open = false;
}

#[cfg(test)]
mod tests;
