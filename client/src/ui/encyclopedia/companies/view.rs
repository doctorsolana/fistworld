//! Company page shell: build once, bind in place.
//!
//! Three retained regions live here. The portfolio strip is spawned once per
//! `local_person` presence and its values bind. The list rows respawn only
//! when the visible set (filter, search), order (sort) or a row's name/master
//! changes; their ledger and status lines rewrite in place. The detail pane respawns only when
//! [`company_structure_key`] (ids and gating bits, never values), the
//! selected company or the route editor's shape changes; every 0.5 s books
//! snapshot otherwise runs the bind pass, even while a control is hovered.
//! Structural rebuilds alone are deferred while a control is pressed/hovered,
//! and only they count as `r` in `ClientPerfUi rebuild_company_view`.

use super::binding::{
    apply_bound, company_structure_key, portfolio_value, BoundTargets, CompanyBound, CompanyView,
};
use super::controls::{
    CompanyCountText, CompanyDetailContent, CompanyDetailViewport, CompanyFilterButton,
    CompanyListContent, CompanyListViewport, CompanyPortfolioContent, CompanySortDirectionButton,
    CompanySortDirectionLabel, CompanySortKeyButton, CompanySortKeyLabel, NewCompanyPageButton,
};
use super::details::spawn_company_detail;
use super::model::{
    CompanyDirectory, CompanyFilter, CompanyPolicyFeedback, CompanyRecord, CompanySort,
    CompanySortKey, SelectedCompany, TradeRouteEditorState,
};
use super::portfolio::{
    company_ledger_line, company_status_line, spawn_company_row, spawn_portfolio, CompanyRowLedger,
    CompanyRowStatus,
};
use super::route_editor::spawn_trade_route_editor;
use super::widgets::spawn_empty;
use crate::ui::encyclopedia::*;
use crate::ui::foundation::{
    button_chrome, retained_scroll, subtree_is_interacting, UiArtworkFocus, UiButtonLabel,
    UiButtonVariant, UiRefreshExempt,
};
use crate::ui::styles::{INK_MUTED, PLATE_RULE_SOFT};
use bevy::ecs::system::SystemParam;
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use shared::components::{CompanyId, PersonId};
use std::cmp::Ordering;

pub(in crate::ui::encyclopedia) fn spawn_companies_tab(body: &mut ChildSpawnerCommands<'_>) {
    body.spawn((
        TabBody(EncyclopediaTab::Companies),
        Node {
            display: Display::None,
            flex_grow: 1.0,
            min_height: Val::Px(0.0),
            overflow: Overflow::clip(),
            ..default()
        },
    ))
    .with_children(|split| {
        split
            .spawn((
                Node {
                    width: Val::Percent(28.0),
                    min_width: Val::Px(300.0),
                    max_width: Val::Px(390.0),
                    min_height: Val::Px(0.0),
                    flex_shrink: 0.0,
                    flex_direction: FlexDirection::Column,
                    row_gap: Val::Px(12.0),
                    padding: UiRect::all(Val::Px(16.0)),
                    border: UiRect::right(Val::Px(1.0)),
                    ..default()
                },
                BorderColor::from(PLATE_RULE_SOFT),
                crate::ui::ledger::directory_paper(),
            ))
            .with_children(|sidebar| {
                sidebar.spawn(crate::ui::ledger::directory_gutter());
                sidebar
                    .spawn(Node {
                        flex_wrap: FlexWrap::Wrap,
                        column_gap: Val::Px(4.0),
                        row_gap: Val::Px(5.0),
                        flex_shrink: 0.0,
                        ..default()
                    })
                    .with_children(|filters| {
                        for filter in CompanyFilter::ALL {
                            filters
                                .spawn((
                                    Button,
                                    CompanyFilterButton(filter),
                                    Node {
                                        min_height: Val::Px(36.0),
                                        flex_grow: 1.0,
                                        padding: UiRect::axes(Val::Px(8.0), Val::Px(6.0)),
                                        border: UiRect::all(Val::Px(1.0)),
                                        justify_content: JustifyContent::Center,
                                        align_items: AlignItems::Center,
                                        ..default()
                                    },
                                    button_chrome(UiButtonVariant::Tab),
                                ))
                                .with_child((
                                    UiButtonLabel,
                                    crate::ui::ledger::body(filter.label(), 12.0),
                                ));
                        }
                    });
                sidebar
                    .spawn((
                        NewCompanyPageButton,
                        Button,
                        Node {
                            min_height: Val::Px(40.0),
                            flex_shrink: 0.0,
                            padding: UiRect::axes(Val::Px(14.0), Val::Px(8.0)),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            border: UiRect::all(Val::Px(1.0)),
                            ..default()
                        },
                        button_chrome(UiButtonVariant::Secondary),
                    ))
                    .with_child((
                        UiButtonLabel,
                        crate::ui::ledger::heading("+  New Company", 16.0),
                    ));
                // The sidebar already pads 16 px, so the search row carries
                // no margin of its own and the column gap spaces it.
                search::spawn(sidebar, EncyclopediaTab::Companies, UiRect::ZERO);
                spawn_count_and_sort_row(sidebar);
                sidebar
                    .spawn((
                        CompanyListViewport,
                        Node {
                            flex_grow: 1.0,
                            min_height: Val::Px(0.0),
                            flex_direction: FlexDirection::Column,
                            overflow: Overflow::scroll_y(),
                            scrollbar_width: 8.0,
                            ..default()
                        },
                    ))
                    .with_child((
                        CompanyListContent,
                        Node {
                            flex_shrink: 0.0,
                            flex_direction: FlexDirection::Column,
                            row_gap: Val::Px(8.0),
                            ..default()
                        },
                    ));
            });
        split
            .spawn(Node {
                flex_grow: 1.0,
                flex_basis: Val::Px(0.0),
                min_width: Val::Px(0.0),
                min_height: Val::Px(0.0),
                flex_direction: FlexDirection::Column,
                ..default()
            })
            .with_children(|right| {
                right.spawn((
                    CompanyPortfolioContent,
                    Node {
                        flex_shrink: 0.0,
                        align_items: AlignItems::Stretch,
                        column_gap: Val::Px(2.0),
                        margin: UiRect::all(Val::Px(14.0)),
                        padding: UiRect::vertical(Val::Px(5.0)),
                        border: UiRect::all(Val::Px(1.0)),
                        ..default()
                    },
                    BorderColor::from(PLATE_RULE_SOFT),
                ));
                right
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
                    ))
                    .with_child((
                        CompanyDetailContent,
                        Node {
                            flex_shrink: 0.0,
                            flex_direction: FlexDirection::Column,
                            align_items: AlignItems::Stretch,
                            row_gap: Val::Px(10.0),
                            padding: UiRect::axes(Val::Px(20.0), Val::Px(6.0)),
                            ..default()
                        },
                    ));
            });
    });
}

/// One 39 px row under the search field: the live count on the left, then
/// SORT, the cycling key button and the direction toggle. Its labels are
/// spawned with the default sort and rewritten in place by
/// `style_company_controls`, so a reopened window shows the retained choice.
fn spawn_count_and_sort_row(sidebar: &mut ChildSpawnerCommands<'_>) {
    sidebar
        .spawn(Node {
            height: Val::Px(39.0),
            flex_shrink: 0.0,
            align_items: AlignItems::Center,
            column_gap: Val::Px(8.0),
            ..default()
        })
        .with_children(|row| {
            row.spawn((
                CompanyCountText,
                crate::ui::ledger::body("0 companies", 13.0),
                TextLayout::no_wrap(),
                Node {
                    flex_grow: 1.0,
                    min_width: Val::Px(0.0),
                    ..default()
                },
            ));
            row.spawn(crate::ui::ledger::body("SORT", 12.0)).insert((
                TextColor(INK_MUTED),
                Node {
                    flex_shrink: 0.0,
                    ..default()
                },
            ));
            row.spawn((
                Button,
                CompanySortKeyButton,
                Name::new("Sort companies by"),
                Node {
                    min_height: Val::Px(35.0),
                    min_width: Val::Px(120.0),
                    flex_shrink: 0.0,
                    padding: UiRect::axes(Val::Px(11.0), Val::Px(5.0)),
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    border: UiRect::all(Val::Px(1.0)),
                    ..default()
                },
                button_chrome(UiButtonVariant::Secondary),
            ))
            .with_child((
                CompanySortKeyLabel,
                UiButtonLabel,
                crate::ui::ledger::body_strong(CompanySort::default().key.label(), 12.0),
            ));
            // The same square face as the search field's clear button.
            row.spawn((
                Button,
                CompanySortDirectionButton,
                Name::new("Reverse company sort"),
                UiArtworkFocus,
                button_chrome(UiButtonVariant::Secondary),
                Node {
                    width: Val::Px(35.0),
                    height: Val::Px(35.0),
                    flex_shrink: 0.0,
                    justify_content: JustifyContent::Center,
                    align_items: AlignItems::Center,
                    ..default()
                },
            ))
            .with_child((
                CompanySortDirectionLabel,
                UiButtonLabel,
                crate::ui::ledger::body_strong("v", 15.0),
            ));
        });
}

#[derive(Default)]
pub(in crate::ui::encyclopedia) struct CompanyViewState {
    rows: Option<u64>,
    /// `EncyclopediaSearch::revision(Companies)` the list was last built for.
    search: u64,
    /// [`company_structure_key`] of the current detail tree.
    detail: Option<u64>,
    detail_company: Option<CompanyId>,
    /// A structural change is waiting for the pointer to leave a control.
    pending_detail: bool,
    /// `local_person.is_some()` when the portfolio strip was spawned.
    portfolio: Option<bool>,
    /// The Companies body was hidden (a page was open) on the last run, so
    /// the next visible frame must bind whatever changed meanwhile.
    hidden: bool,
}

/// What shapes the list alone: the chip filter, the search draft and the
/// sort. Folded so `rebuild_company_view` stays within Bevy's parameter cap.
#[derive(SystemParam)]
pub(in crate::ui::encyclopedia) struct CompanyListInputs<'w> {
    filter: Res<'w, CompanyFilter>,
    search: Res<'w, search::EncyclopediaSearch>,
    sort: Res<'w, CompanySort>,
}

#[derive(SystemParam)]
pub(in crate::ui::encyclopedia) struct CompanyDetailInteraction<'w, 's> {
    children: Query<'w, 's, &'static Children>,
    interactions: Query<'w, 's, (&'static Interaction, Has<UiRefreshExempt>)>,
    scroll: Query<
        'w,
        's,
        &'static mut ScrollPosition,
        (With<CompanyDetailViewport>, Without<CompanyListViewport>),
    >,
    /// Reset by a search or sort change; a books tick never touches it.
    list_scroll: Query<
        'w,
        's,
        &'static mut ScrollPosition,
        (With<CompanyListViewport>, Without<CompanyDetailViewport>),
    >,
    history: Option<Res<'w, crate::ui::history::HistoryPanelTarget>>,
    business: Option<Res<'w, crate::ui::business_management::BusinessManagementTarget>>,
    founding: Option<Res<'w, crate::ui::company_founding::FoundingPageOpen>>,
    market: Option<Res<'w, crate::ui::market::MarketPageTarget>>,
}

impl CompanyDetailInteraction<'_, '_> {
    /// A page covers the Companies tab body this frame. Read from the page
    /// resources rather than the `TabBody` display, which `sync_tab_visuals`
    /// and `sync_page_host` rewrite later in the same frame.
    fn covered_by_page(&self) -> bool {
        crate::ui::encyclopedia::page_is_open(
            self.history.as_deref(),
            self.business.as_deref(),
            self.founding.as_deref(),
            self.market.as_deref(),
        )
    }
}

/// Every node the bind pass may write. The `Without` filters keep the four
/// `&mut Text` queries provably disjoint for Bevy's access checker.
#[derive(SystemParam)]
pub(in crate::ui::encyclopedia) struct CompanyBindTargets<'w, 's> {
    count_text: Query<
        'w,
        's,
        &'static mut Text,
        (
            With<CompanyCountText>,
            Without<CompanyRowLedger>,
            Without<CompanyRowStatus>,
            Without<CompanyBound>,
        ),
    >,
    ledgers: Query<
        'w,
        's,
        (&'static CompanyRowLedger, &'static mut Text),
        (
            Without<CompanyCountText>,
            Without<CompanyRowStatus>,
            Without<CompanyBound>,
        ),
    >,
    statuses: Query<
        'w,
        's,
        (&'static CompanyRowStatus, &'static mut Text),
        (
            Without<CompanyCountText>,
            Without<CompanyRowLedger>,
            Without<CompanyBound>,
        ),
    >,
    bound: Query<
        'w,
        's,
        BoundTargets,
        (
            Without<CompanyCountText>,
            Without<CompanyRowLedger>,
            Without<CompanyRowStatus>,
            Without<TabBody>,
        ),
    >,
}

pub(in crate::ui::encyclopedia) fn rebuild_company_view(
    mut commands: Commands,
    directory: Res<CompanyDirectory>,
    inputs: CompanyListInputs,
    mut selected: ResMut<SelectedCompany>,
    portfolio: Query<(Entity, Option<&Children>), With<CompanyPortfolioContent>>,
    list: Query<(Entity, Option<&Children>), With<CompanyListContent>>,
    detail: Query<(Entity, Option<&Children>), With<CompanyDetailContent>>,
    feedback: Res<CompanyPolicyFeedback>,
    editor: Res<TradeRouteEditorState>,
    ui_perf: Res<crate::ui::perf::UiPerf>,
    mut cache: Local<CompanyViewState>,
    mut interaction: CompanyDetailInteraction,
    mut targets: CompanyBindTargets,
) {
    let mut _ui_scope = ui_perf.scope("rebuild_company_view");
    // While a page (company settings, ledger) covers the tab its body is
    // hidden; binding hundreds of nodes nobody can see is wasted work. The
    // first visible frame afterwards binds everything that moved.
    if interaction.covered_by_page() {
        cache.hidden = true;
        return;
    }
    let revealed = std::mem::take(&mut cache.hidden);
    // Closing the window despawns these containers; reopening respawns them
    // childless while the Locals still remember the last build. A childless
    // container must fill regardless of what changed.
    let fresh = portfolio
        .single()
        .is_ok_and(|(_, children)| children.is_none())
        || list.single().is_ok_and(|(_, children)| children.is_none())
        || detail
            .single()
            .is_ok_and(|(_, children)| children.is_none());
    let filter = *inputs.filter;
    let sort = *inputs.sort;
    let search_revision = inputs.search.revision(EncyclopediaTab::Companies);
    // Search and sort shape only the list. The directory snapshot is written
    // only when the books moved, so they must open the gate on their own;
    // they never reach the detail pane's structure key or its bind pass.
    let list_changed = inputs.sort.is_changed() || cache.search != search_revision;
    let inputs_changed = directory.is_changed()
        || inputs.filter.is_changed()
        || selected.is_changed()
        || feedback.is_changed()
        || editor.is_changed();
    if !fresh && !revealed && !inputs_changed && !list_changed && cache.rows.is_some() {
        // A structural change may be waiting for the pointer to leave a
        // retained control. Check that cheaply each frame and flush it even
        // if there is no subsequent directory snapshot.
        if !cache.pending_detail
            || detail.single().is_ok_and(|(entity, _)| {
                subtree_is_interacting(entity, &interaction.children, &interaction.interactions)
            })
        {
            return;
        }
    }
    let visible = visible_companies(&directory, filter, &inputs.search, sort);
    // A company that left the directory cannot stay selected; one that a
    // search merely hides can, as on the People page.
    if selected
        .0
        .is_some_and(|id| !directory.records.iter().any(|company| company.id == id))
    {
        selected.0 = None;
    }
    if selected.0.is_none() {
        if let Some(company) = visible.first() {
            selected.0 = Some(company.id);
        }
    }
    let searching = inputs.search.active(EncyclopediaTab::Companies);
    if list_changed {
        cache.search = search_revision;
        // A new query or order starts at the top; the detail scroll is untouched.
        for mut position in &mut interaction.list_scroll {
            if position.y != 0.0 {
                position.y = 0.0;
            }
        }
    }
    for mut text in targets.count_text.iter_mut() {
        let count = if searching {
            let total = permitted_companies(&directory, filter).count();
            format!("{} of {total}", visible.len())
        } else {
            match visible.len() {
                1 => "1 company".to_string(),
                count => format!("{count} companies"),
            }
        };
        if text.0 != count {
            text.0 = count;
        }
    }

    // The portfolio strip's only structure is whether a local hero exists;
    // its four values bind below with everything else.
    if let Ok((entity, children)) = portfolio.single() {
        let structure = directory.local_person.is_some();
        if children.is_none() || cache.portfolio != Some(structure) {
            cache.portfolio = Some(structure);
            _ui_scope.rebuilt();
            clear_children(&mut commands, children);
            commands
                .entity(entity)
                .with_children(|parent| spawn_portfolio(parent, &directory));
        }
    }
    // A list row shows name, ledger line and status; only the visible set,
    // its order and the names are structure. The two live lines rewrite in
    // place, so a stranger meeting payroll never tears a row down.
    let rows_signature =
        company_rows_signature(&visible, filter, directory.local_person, searching, sort);
    if let Ok((entity, children)) = list.single() {
        if children.is_none() || cache.rows != Some(rows_signature) {
            cache.rows = Some(rows_signature);
            _ui_scope.rebuilt();
            clear_children(&mut commands, children);
            commands.entity(entity).with_children(|parent| {
                if visible.is_empty() {
                    spawn_empty(
                        parent,
                        if searching {
                            "No companies match this search"
                        } else {
                            match filter {
                                CompanyFilter::All => "No companies exist yet.",
                                CompanyFilter::MyHoldings => {
                                    "You do not own shares in a company yet. Switch to ALL FIRMS to browse."
                                }
                                CompanyFilter::SharesForSale => {
                                    "No company shares are currently offered."
                                }
                            }
                        },
                    );
                } else {
                    for company in &visible {
                        spawn_company_row(parent, company, directory.local_person);
                    }
                }
            });
        }
    }
    if directory.is_changed() || revealed {
        let by_id: HashMap<CompanyId, &CompanyRecord> = directory
            .records
            .iter()
            .map(|company| (company.id, company))
            .collect();
        for (CompanyRowLedger(id), mut text) in targets.ledgers.iter_mut() {
            if let Some(company) = by_id.get(id) {
                let line = company_ledger_line(company);
                if text.0 != line {
                    text.0 = line;
                }
            }
        }
        for (CompanyRowStatus(id), mut text) in targets.statuses.iter_mut() {
            if let Some(company) = by_id.get(id) {
                let line = company_status_line(company, directory.local_person);
                if text.0 != line {
                    text.0 = line;
                }
            }
        }
    }

    let company = selected
        .0
        .and_then(|id| directory.records.iter().find(|company| company.id == id));
    let view = company.map(|company| CompanyView {
        company,
        directory: &directory,
        feedback: &feedback,
        editor: &editor,
    });
    let structure = company_structure_key(company, &directory, &editor);
    let mut rebuilt_detail = false;
    if let Ok((entity, children)) = detail.single() {
        let same_target = cache.detail_company == selected.0;
        // Navigation must show its result immediately. Only a structural
        // change to the SAME record waits for the pointer to leave a control;
        // values under the pointer keep binding meanwhile.
        let force = children.is_none() || !same_target;
        if force || cache.detail != Some(structure) {
            if force
                || !subtree_is_interacting(entity, &interaction.children, &interaction.interactions)
            {
                cache.detail = Some(structure);
                cache.detail_company = selected.0;
                cache.pending_detail = false;
                rebuilt_detail = true;
                for mut scroll in &mut interaction.scroll {
                    let offset = retained_scroll(same_target, Some(scroll.0));
                    if scroll.0 != offset {
                        scroll.0 = offset;
                    }
                }
                _ui_scope.rebuilt();
                clear_children(&mut commands, children);
                commands.entity(entity).with_children(|parent| {
                    if let Some(view) = view.as_ref() {
                        if let Some(draft) = view.draft() {
                            spawn_trade_route_editor(parent, view, draft);
                        } else {
                            spawn_company_detail(parent, view);
                        }
                    } else {
                        spawn_empty(
                            parent,
                            "Select a company to inspect its ownership and books.",
                        );
                    }
                });
            } else {
                cache.pending_detail = true;
            }
        } else {
            cache.pending_detail = false;
        }
    }

    // The bind pass: every changed snapshot, receipt or draft step rewrites
    // the bound nodes whose value differs. A freshly spawned tree already
    // holds current values, so it needs no pass of its own.
    if inputs_changed || revealed {
        let _bind_scope = ui_perf.scope("bind_company_detail");
        for item in targets.bound.iter_mut() {
            let key = *item.0;
            let value = match key {
                CompanyBound::Portfolio(field) => Some(portfolio_value(&directory, field)),
                _ if rebuilt_detail => None,
                key => view.as_ref().and_then(|view| view.value(key)),
            };
            if let Some(value) = value {
                apply_bound(value, item);
            }
        }
    }
}

/// Hash of the row STRUCTURE [`spawn_company_row`] renders, in display
/// order: which companies, in what order, under which filter, search state
/// and sort, and the name / master / holding-or-not each row was built with.
/// Status, cash and share counts are values and rewrite in place, so a books
/// tick that leaves the order alone keeps every row entity even under a
/// CASH or PROFIT sort.
pub(super) fn company_rows_signature(
    visible: &[&CompanyRecord],
    filter: CompanyFilter,
    local_person: Option<PersonId>,
    searching: bool,
    sort: CompanySort,
) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (filter as u8).hash(&mut hasher);
    searching.hash(&mut hasher);
    (sort.key as u8).hash(&mut hasher);
    sort.descending.hash(&mut hasher);
    visible.len().hash(&mut hasher);
    for company in visible {
        company.id.hash(&mut hasher);
        company.name.hash(&mut hasher);
        company.master.hash(&mut hasher);
        local_person
            .map(|person| company.shares_owned_by(person) > 0)
            .hash(&mut hasher);
    }
    hasher.finish()
}

/// The records the chip filter permits, in directory order: the "M" of the
/// "N of M" count while a search narrows the list further.
fn permitted_companies<'a>(
    directory: &'a CompanyDirectory,
    filter: CompanyFilter,
) -> impl Iterator<Item = &'a CompanyRecord> + 'a {
    directory.records.iter().filter(move |company| match filter {
        CompanyFilter::All => true,
        CompanyFilter::MyHoldings => directory
            .local_person
            .is_some_and(|person| company.shares_owned_by(person) > 0),
        CompanyFilter::SharesForSale => !company.offers.is_empty(),
    })
}

/// Filter, then search, then sort: the rows the list shows, in display order.
pub(super) fn visible_companies<'a>(
    directory: &'a CompanyDirectory,
    filter: CompanyFilter,
    search: &search::EncyclopediaSearch,
    sort: CompanySort,
) -> Vec<&'a CompanyRecord> {
    let mut visible: Vec<_> = permitted_companies(directory, filter)
        .filter(|company| search.matches_company(company))
        .collect();
    visible.sort_by(|a, b| compare_companies(a, b, sort, directory.local_person));
    visible
}

/// One sort choice's order. `descending` reverses only the primary key;
/// equal rows always fall back to name (case-insensitive) and then id, so a
/// tick that leaves the key values alone cannot shuffle the list.
pub(super) fn compare_companies(
    a: &CompanyRecord,
    b: &CompanyRecord,
    sort: CompanySort,
    local_person: Option<PersonId>,
) -> Ordering {
    let owned = |company: &CompanyRecord| {
        local_person.map_or(0, |person| company.shares_owned_by(person))
    };
    let primary = match sort.key {
        CompanySortKey::Holdings => owned(a).cmp(&owned(b)),
        CompanySortKey::Name => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
        CompanySortKey::Cash => a.account.cash.cmp(&b.account.cash),
        CompanySortKey::Profit => a
            .account
            .current_day
            .profit()
            .cmp(&b.account.current_day.profit()),
        CompanySortKey::Sites => a.sites.len().cmp(&b.sites.len()),
    };
    let primary = if sort.descending {
        primary.reverse()
    } else {
        primary
    };
    primary
        .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        .then_with(|| a.id.cmp(&b.id))
}

pub(super) fn clear_children(commands: &mut Commands, children: Option<&Children>) {
    if let Some(children) = children {
        for child in children.iter() {
            commands.entity(child).despawn();
        }
    }
}
