//! Stable navigation keys: data refreshes and rebuilt UI entities are silent.

use bevy::ecs::system::SystemParam;
use bevy::prelude::*;
use shared::components::{CompanyId, PersonId, SettlementId};

use crate::audio::sfx::SfxCue;
use crate::states::GameState;
use crate::ui::{business_management, company_founding, encyclopedia, history, market};

#[derive(SystemParam)]
pub(super) struct BookState<'w> {
    open: Option<Res<'w, encyclopedia::EncyclopediaOpen>>,
    tab: Option<Res<'w, encyclopedia::EncyclopediaTab>>,
    filter: Option<Res<'w, encyclopedia::PeopleFilter>>,
    person: Option<Res<'w, encyclopedia::SelectedPerson>>,
    place: Option<Res<'w, encyclopedia::places::SelectedPlace>>,
    entry: Option<Res<'w, encyclopedia::places::SelectedPlaceEntry>>,
    company: Option<Res<'w, encyclopedia::companies::SelectedCompany>>,
    company_filter: Option<Res<'w, encyclopedia::companies::CompanyFilter>>,
    history: Option<Res<'w, history::HistoryPanelTarget>>,
    history_range: Option<Res<'w, history::HistoryRange>>,
    business: Option<Res<'w, business_management::BusinessManagementTarget>>,
    founding: Option<Res<'w, company_founding::FoundingPageOpen>>,
    market: Option<Res<'w, market::MarketPageTarget>>,
}

#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub(super) struct BookSnapshot {
    state: Option<GameState>,
    open: bool,
    tab: encyclopedia::EncyclopediaTab,
    filter: encyclopedia::PeopleFilter,
    person: Option<PersonId>,
    place: Option<SettlementId>,
    entry: encyclopedia::places::SelectedPlaceEntry,
    company: Option<CompanyId>,
    company_filter: encyclopedia::companies::CompanyFilter,
    history: Option<(Option<Entity>, history::HistoryView)>,
    history_range: history::HistoryRange,
    business: Option<Entity>,
    founding: bool,
    market: Option<SettlementId>,
}

impl BookState<'_> {
    pub(super) fn snapshot(&self, state: Option<GameState>) -> BookSnapshot {
        BookSnapshot {
            state,
            open: self.open.as_ref().is_some_and(|value| value.0),
            tab: self.tab.as_ref().map(|value| **value).unwrap_or_default(),
            filter: self
                .filter
                .as_ref()
                .map(|value| **value)
                .unwrap_or_default(),
            person: self.person.as_ref().and_then(|value| value.0),
            place: self.place.as_ref().and_then(|value| value.0),
            entry: self.entry.as_ref().map(|value| **value).unwrap_or_default(),
            company: self.company.as_ref().and_then(|value| value.0),
            company_filter: self
                .company_filter
                .as_ref()
                .map(|value| **value)
                .unwrap_or_default(),
            history: self.history.as_ref().and_then(|value| {
                value
                    .0
                    .as_ref()
                    .map(|target| (target.settlement, target.view))
            }),
            history_range: self
                .history_range
                .as_ref()
                .map(|value| **value)
                .unwrap_or_default(),
            business: self.business.as_ref().and_then(|value| value.0),
            founding: self.founding.as_ref().is_some_and(|value| value.0),
            market: self
                .market
                .as_ref()
                .and_then(|value| value.0.as_ref().map(|target| target.place_id)),
        }
    }
}

impl BookSnapshot {
    pub(super) fn transition(&self, next: &Self, navigation_input: bool) -> Option<SfxCue> {
        // Enter/exit fixtures and session cleanup are not player navigation.
        if self.state != next.state || next.state != Some(GameState::Playing) {
            return None;
        }
        match (self.open, next.open) {
            (false, true) => Some(SfxCue::BookOpen),
            (true, false) => Some(SfxCue::BookClose),
            (true, true) if navigation_input && self != next => Some(SfxCue::PageTurn),
            _ => None,
        }
    }
}
