//! Company and workplace policy pages with explicit scopes.
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
    CompanyShareMarket, EmployedAt, Hero, OperatedBy, OwnedBy, PersonId, SettlementBuilding,
    SettlementBuildingKind,
};
use shared::economy::{
    BusinessAccount, BusinessManagementPolicy, BusinessProcurementPolicy, BusinessSalePolicy,
    BusinessSourcingMode, BusinessStaffingPolicy, BusinessStrategy, BusinessSupplyPolicy,
    BusinessWagePolicy, CompanyAccount, CompanyDecisionHistory, CompanyManagementPolicy, Good,
    GoodsInventory, TavernService, format_money,
};
use shared::protocol::{
    HeroBusinessAction, HeroBusinessOrder, HeroBusinessResult, HeroCompanyAction, HeroCompanyOrder,
    ReliableChannel,
};

use crate::states::GameState;
use crate::ui::foundation::{
    UiButtonLabel, UiButtonStyle, UiButtonVariant, retained_scroll, selected_button_chrome,
    subtree_is_interacting,
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
        app.add_systems(
            Update,
            (
                receive_results,
                ensure_panel,
                update_guard,
                handle_action_buttons,
                handle_share_draft_buttons,
                handle_page_buttons,
                sync_input_state,
            )
                .chain()
                .run_if(in_state(GameState::Playing)),
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

#[derive(Component)]
struct Root {
    structure: String,
    target: BusinessManagementSelection,
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
use model::*;

// --- systems ----------------------------------------------------------------

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
fn ensure_panel(
    mut commands: Commands,
    mut target: ResMut<BusinessManagementTarget>,
    mut feedback: ResMut<BusinessFeedback>,
    mut page: ResMut<BusinessManagementPage>,
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
    )>,
    people: Query<(&PersonId, &CharacterName, Option<&EmployedAt>)>,
    heroes: Query<(&Hero, &PersonId)>,
    mut ui: ManagementPanelUi,
    mut bound: BoundControls,
) {
    let mut _ui_scope = ui.perf.scope("ensure_business_panel");
    let Some(entity) = target.0 else {
        feedback.message.clear();
        feedback.context = None;
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
        people.iter().find(|(id, ..)| **id == person).map_or_else(
            || {
                ui.known_people
                    .find_by_id(person)
                    .filter(|record| record.known || record.is_self)
                    .map_or_else(
                        || format!("Person #{}", person.0),
                        |record| record.name.clone(),
                    )
            },
            |(_, name, _)| name.0.clone(),
        )
    };
    if selected_site.is_none() {
        *page = BusinessManagementPage::Company;
    } else if company.is_none() {
        *page = BusinessManagementPage::Site;
    }
    let context = Some((entity, *page));
    if feedback.context != context {
        feedback.message.clear();
        feedback.context = context;
    }
    let mut workers: Vec<_> = selected_site
        .as_ref()
        .map(|site| {
            people
                .iter()
                .filter(|(_, _, employment)| {
                    employment.is_some_and(|employment| employment.0 == *site.1)
                })
                .map(|(person, name, _)| WorkerModel {
                    person: *person,
                    name: name.0.clone(),
                })
                .collect()
        })
        .unwrap_or_default();
    workers.sort_unstable_by_key(|worker| worker.person.0);
    let model = controls_model(&ModelInputs {
        workers: &workers,
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
        share_draft: &share_draft,
        page: *page,
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
fn bind_panel(model: &ControlsModel, bound: &mut BoundControls) {
    let mut texts: HashMap<&str, (&str, Option<Color>)> = HashMap::new();
    let mut buttons: HashMap<&str, (ControlPress, bool)> = HashMap::new();
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
            Block::Section(_) | Block::Scope(_) => {}
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
        if let Some(mut action) = action {
            if action.0 != *press {
                action.0 = *press;
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
        let Some(widths) = lanes.get(fill.id.as_str()) else {
            continue;
        };
        let width = Val::Percent(widths[fill.lane]);
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
            ControlPress::Draft(_) | ControlPress::Person(_) => {}
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
