//! Company UI markers, navigation, command submission and button state.

use super::model::{
    CompanyDrilldownReturn, CompanyFilter, CompanyPolicyFeedback, SelectedCompany,
    TradeRouteEditorAction, TradeRouteQuickAction,
};
use crate::ui::encyclopedia::*;
use crate::ui::foundation::UiButtonStyle;
use bevy::prelude::*;
use lightyear::prelude::{Connected, MessageReceiver, MessageSender};
use shared::components::{BuildingId, CompanyId, PersonId, TradeRouteId};
use shared::protocol::{HeroCompanyAction, HeroCompanyOrder, HeroCompanyResult, ReliableChannel};

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

/// Opens the NEW COMPANY page.
#[derive(Component)]
pub struct NewCompanyPageButton;

pub(in crate::ui::encyclopedia) fn handle_new_company_button(
    guard: Res<ClickGuard>,
    mouse: Res<ButtonInput<MouseButton>>,
    buttons: Query<&Interaction, (With<NewCompanyPageButton>, Changed<Interaction>)>,
    mut page: ResMut<crate::ui::company_founding::FoundingPageOpen>,
) {
    if !guard.0 || !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    if buttons
        .iter()
        .any(|interaction| *interaction == Interaction::Pressed)
    {
        page.0 = true;
    }
}

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

#[derive(Component, Clone, Copy)]
pub struct NewTradeRouteButton(pub CompanyId);

#[derive(Component, Clone, Copy)]
pub struct EditTradeRouteButton {
    pub company: CompanyId,
    pub route: TradeRouteId,
}

#[derive(Component, Clone, Copy)]
pub struct TradeRouteQuickActionButton {
    pub company: CompanyId,
    pub route: TradeRouteId,
    pub action: TradeRouteQuickAction,
}

#[derive(Component, Clone, Copy)]
pub struct TradeRouteEditorButton(pub TradeRouteEditorAction);

pub(in crate::ui::encyclopedia) fn handle_company_filter_buttons(
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

pub(in crate::ui::encyclopedia) fn handle_company_rows(
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

pub(in crate::ui::encyclopedia) fn handle_company_site_buttons(
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
                .map(|index| (place.id, index))
        }) else {
            continue;
        };
        selected_place.0 = Some(place);
        *selected_entry = places::SelectedPlaceEntry::Building(index);
        return_to.0 = selected_company.0;
        *tab = EncyclopediaTab::Places;
    }
}

pub(in crate::ui::encyclopedia) fn handle_company_person_buttons(
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
        if people.records.iter().all(|record| record.id != *person) {
            continue;
        }
        selected.0 = Some(*person);
        *tab = EncyclopediaTab::People;
    }
}

pub(in crate::ui::encyclopedia) fn handle_company_management_buttons(
    guard: Res<ClickGuard>,
    mouse: Res<ButtonInput<MouseButton>>,
    buttons: Query<(&Interaction, &CompanyManagementButton), Changed<Interaction>>,
    mut target: ResMut<crate::ui::business_management::BusinessManagementTarget>,
    mut return_to: ResMut<crate::ui::business_management::BusinessManagementReturn>,
) {
    if !guard.0 || !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    for (interaction, button) in buttons.iter() {
        if *interaction == Interaction::Pressed {
            // Opens as a page inside this window; the window stays open.
            target.0 = Some(button.site);
            return_to.0 = Some(button.company);
        }
    }
}

pub(in crate::ui::encyclopedia) fn handle_company_branch_policy_buttons(
    guard: Res<ClickGuard>,
    mouse: Res<ButtonInput<MouseButton>>,
    buttons: Query<(&Interaction, &CompanyBranchPolicyButton), Changed<Interaction>>,
    mut clients: Query<
        &mut MessageSender<HeroCompanyOrder>,
        (With<crate::GameClient>, With<Connected>),
    >,
) {
    for (interaction, button) in buttons.iter() {
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

pub(in crate::ui::encyclopedia) fn receive_company_policy_results(
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

pub(in crate::ui::encyclopedia) fn style_company_controls(
    filter: Res<CompanyFilter>,
    selected: Res<SelectedCompany>,
    mut filters: Query<(&CompanyFilterButton, &mut UiButtonStyle), Without<CompanyRow>>,
    mut rows: Query<(&CompanyRow, &mut UiButtonStyle), Without<CompanyFilterButton>>,
) {
    for (CompanyFilterButton(button), mut style) in filters.iter_mut() {
        let selected = *button == *filter;
        if style.selected != selected {
            style.selected = selected;
        }
    }
    for (CompanyRow(company), mut style) in rows.iter_mut() {
        let selected = selected.0 == Some(*company);
        if style.selected != selected {
            style.selected = selected;
        }
    }
}
