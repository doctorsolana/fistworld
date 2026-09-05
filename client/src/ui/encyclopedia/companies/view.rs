//! Company page shell and selective rebuilds of portfolio, rows and details.

use super::controls::{
    CompanyCountText, CompanyDetailContent, CompanyDetailViewport, CompanyFilterButton,
    CompanyListContent, CompanyListViewport, CompanyPortfolioContent, NewCompanyPageButton,
};
use super::details::spawn_company_detail;
use super::model::{
    CompanyDirectory, CompanyFilter, CompanyPolicyFeedback, CompanyRecord, SelectedCompany,
    TradeRouteEditorState,
};
use super::portfolio::{company_ledger_line, spawn_company_row, spawn_portfolio, CompanyRowLedger};
use super::route_editor::spawn_trade_route_editor;
use super::widgets::spawn_empty;
use crate::ui::encyclopedia::*;
use crate::ui::foundation::{button_chrome, UiButtonLabel, UiButtonVariant};
use crate::ui::styles::{INK, INK_MUTED, PLATE_RULE_SOFT, RADIUS};
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use shared::components::{CompanyId, PersonId};

pub(in crate::ui::encyclopedia) fn spawn_companies_tab(body: &mut ChildSpawnerCommands<'_>) {
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
            BorderColor::from(PLATE_RULE_SOFT),
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
            BorderColor::from(PLATE_RULE_SOFT),
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
                            button_chrome(UiButtonVariant::Tab),
                        ))
                        .with_child((
                            Text::new(filter.label()),
                            UiButtonLabel,
                            TextFont {
                                font_size: FontSize::Px(13.0),
                                ..default()
                            },
                            TextColor(INK_MUTED),
                            Pickable::IGNORE,
                        ));
                }
            });
            bar.spawn(Node {
                flex_direction: FlexDirection::Row,
                align_items: AlignItems::Center,
                column_gap: Val::Px(14.0),
                ..default()
            })
            .with_children(|right| {
                right.spawn((
                    CompanyCountText,
                    Text::new("0 companies"),
                    TextFont {
                        font_size: FontSize::Px(13.5),
                        ..default()
                    },
                    TextColor(INK_MUTED),
                ));
                right
                    .spawn((
                        NewCompanyPageButton,
                        Button,
                        Node {
                            min_height: Val::Px(36.0),
                            padding: UiRect::axes(Val::Px(14.0), Val::Px(8.0)),
                            justify_content: JustifyContent::Center,
                            align_items: AlignItems::Center,
                            border: UiRect::all(Val::Px(1.0)),
                            border_radius: BorderRadius::all(Val::Px(RADIUS)),
                            ..default()
                        },
                        button_chrome(UiButtonVariant::Primary),
                    ))
                    .with_child((
                        Text::new("NEW COMPANY"),
                        UiButtonLabel,
                        TextFont {
                            font_size: FontSize::Px(14.0),
                            ..default()
                        },
                        TextColor(INK),
                        Pickable::IGNORE,
                    ));
            });
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
                        width: Val::Px(340.0),
                        min_height: Val::Px(0.0),
                        flex_shrink: 0.0,
                        flex_direction: FlexDirection::Column,
                        overflow: Overflow::scroll_y(),
                        scrollbar_width: 8.0,
                        border: UiRect::right(Val::Px(1.0)),
                        ..default()
                    },
                    BorderColor::from(PLATE_RULE_SOFT),
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
                    BackgroundColor(LIMEWASH_DETAIL),
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

pub(in crate::ui::encyclopedia) fn rebuild_company_view(
    mut commands: Commands,
    directory: Res<CompanyDirectory>,
    filter: Res<CompanyFilter>,
    mut selected: ResMut<SelectedCompany>,
    portfolio: Query<(Entity, Option<&Children>), With<CompanyPortfolioContent>>,
    list: Query<(Entity, Option<&Children>), With<CompanyListContent>>,
    detail: Query<(Entity, Option<&Children>), With<CompanyDetailContent>>,
    mut count_text: Query<&mut Text, With<CompanyCountText>>,
    feedback: Res<CompanyPolicyFeedback>,
    editor: Res<TradeRouteEditorState>,
    ui_perf: Res<crate::ui::perf::UiPerf>,
    mut last_rows: Local<Option<u64>>,
    mut last_detail: Local<Option<u64>>,
    mut ledgers: Query<(&CompanyRowLedger, &mut Text), Without<CompanyCountText>>,
) {
    let mut _ui_scope = ui_perf.scope("rebuild_company_view");
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
    if !fresh
        && !directory.is_changed()
        && !filter.is_changed()
        && !selected.is_changed()
        && !feedback.is_changed()
        && !editor.is_changed()
        && last_rows.is_some()
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

    // Each region rebuilds only when ITS inputs move. The directory snapshot
    // changes whenever any company's books tick -- constantly, in a living
    // economy -- but a list row shows only name, status and your shares, and
    // must not be torn down because a stranger met payroll. A container with
    // no children is freshly respawned and always fills.
    if let Ok((entity, children)) = portfolio.single() {
        if directory.is_changed() || children.is_none() {
            _ui_scope.rebuilt();
            clear_children(&mut commands, children);
            commands
                .entity(entity)
                .with_children(|parent| spawn_portfolio(parent, &directory));
        }
    }
    let rows_signature = company_rows_signature(&visible, *filter, directory.local_person);
    if let Ok((entity, children)) = list.single() {
        if children.is_none() || *last_rows != Some(rows_signature) {
            *last_rows = Some(rows_signature);
            _ui_scope.rebuilt();
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
                    for company in &visible {
                        spawn_company_row(parent, company, directory.local_person);
                    }
                }
            });
        }
    }
    // The ledger line moves with every snapshot; rewrite it in place rather
    // than tearing down hundreds of rows twice a second.
    if directory.is_changed() {
        let by_id: HashMap<CompanyId, &CompanyRecord> = directory
            .records
            .iter()
            .map(|company| (company.id, company))
            .collect();
        for (CompanyRowLedger(id), mut text) in ledgers.iter_mut() {
            if let Some(company) = by_id.get(id) {
                let line = company_ledger_line(company);
                if text.0 != line {
                    text.0 = line;
                }
            }
        }
    }
    let company = selected
        .0
        .and_then(|id| directory.records.iter().find(|company| company.id == id));
    let detail_signature = company_detail_signature(company, &directory);
    if let Ok((entity, children)) = detail.single() {
        if children.is_none()
            || selected.is_changed()
            || feedback.is_changed()
            || editor.is_changed()
            || *last_detail != Some(detail_signature)
        {
            *last_detail = Some(detail_signature);
            _ui_scope.rebuilt();
            clear_children(&mut commands, children);
            commands.entity(entity).with_children(|parent| {
                if let Some(company) = company {
                    if let Some(draft) = editor
                        .draft
                        .as_ref()
                        .filter(|draft| draft.company == company.id)
                    {
                        spawn_trade_route_editor(parent, company, &directory, draft, &editor);
                    } else {
                        spawn_company_detail(parent, company, &directory, &feedback, &editor);
                    }
                } else {
                    spawn_empty(
                        parent,
                        "Select a company to inspect its ownership and books.",
                    );
                }
            });
        }
    }
}

/// Hash of exactly what [`spawn_company_row`] renders, in display order.
pub(super) fn company_rows_signature(
    visible: &[&CompanyRecord],
    filter: CompanyFilter,
    local_person: Option<PersonId>,
) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    (filter as u8).hash(&mut hasher);
    visible.len().hash(&mut hasher);
    for company in visible {
        company.id.hash(&mut hasher);
        company.name.hash(&mut hasher);
        company.status().hash(&mut hasher);
        company.master.hash(&mut hasher);
        local_person
            .map(|person| company.shares_owned_by(person))
            .hash(&mut hasher);
    }
    hasher.finish()
}

/// The detail pane shows the selected company's live books, so it legitimately
/// follows every snapshot of THAT record -- and nothing else's.
pub(super) fn company_detail_signature(
    company: Option<&CompanyRecord>,
    directory: &CompanyDirectory,
) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    format!("{company:?}").hash(&mut hasher);
    format!("{:?}", directory.settlements).hash(&mut hasher);
    directory.local_person.hash(&mut hasher);
    directory.local_wallet.hash(&mut hasher);
    hasher.finish()
}

pub(super) fn visible_companies(
    directory: &CompanyDirectory,
    filter: CompanyFilter,
) -> Vec<&CompanyRecord> {
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

pub(super) fn clear_children(commands: &mut Commands, children: Option<&Children>) {
    if let Some(children) = children {
        for child in children.iter() {
            commands.entity(child).despawn();
        }
    }
}
