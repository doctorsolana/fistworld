//! Durable person links shared by workplace, company and management records.

use super::*;
use crate::ui::business_management::{
    BusinessManagementPage, BusinessManagementReturn, BusinessManagementSelection,
    BusinessManagementTarget,
};
use crate::ui::foundation::{button_chrome, UiButtonLabel, UiButtonVariant};
use crate::ui::ledger;
use crate::ui::styles::INK_MUTED;
use bevy::ecs::system::SystemParam;
use bevy::ui::InteractionDisabled;
use shared::components::{CompanyId, PersonId, SettlementId};

mod workplaces;
pub(super) use workplaces::{spawn_site_people, spawn_workplace_people};

#[derive(Component, Clone, Copy)]
pub(crate) struct PersonLink(pub PersonId);

#[derive(Clone, Copy)]
enum ReturnDestination {
    Company(CompanyId),
    Place(SettlementId, places::SelectedPlaceEntry, Option<CompanyId>),
    Management(
        BusinessManagementSelection,
        BusinessManagementPage,
        Option<CompanyId>,
        EncyclopediaTab,
    ),
}

#[derive(Resource, Default)]
struct PersonLinkReturn(Option<ReturnDestination>);

#[derive(Component)]
pub(crate) struct ReturnLink;
#[derive(Component)]
struct ReturnLabel;

pub(crate) struct PersonLinksPlugin;

impl Plugin for PersonLinksPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<PersonLinkReturn>().add_systems(
            Update,
            (
                (handle_links, handle_return)
                    .chain()
                    .after(shell::spawn_encyclopedia)
                    .before(state_sync::rebuild_people_list)
                    .before(companies::rebuild_company_view),
                roster_systems()
                    .after(handle_return)
                    .before(state_sync::rebuild_people_list),
            )
                .run_if(encyclopedia_open)
                .run_if(in_state(GameState::Playing)),
        );
        app.add_systems(Update, clear_return.run_if(encyclopedia_closed));
    }
}

/// Roster hosts are spawned hidden and unbound by the company page builder.
/// Running the fill AFTER `rebuild_company_view` (Bevy applies the builder's
/// commands between the two ordered systems) puts the WORKERS rows on the
/// card in the same frame it appears, so a structural rebuild never shows a
/// short card that grows a frame later. Tests register the same config.
pub(super) fn roster_systems(
) -> bevy::ecs::schedule::ScheduleConfigs<bevy::ecs::system::ScheduleSystem> {
    (workplaces::sync_rosters, sync_links)
        .chain()
        .after(companies::rebuild_company_view)
        // Management worker chips are `PersonLink` slots bound in place by
        // `ensure_panel`; running after it lets a chip that fills this frame
        // gain or lose `InteractionDisabled` in the same frame it appears.
        .after(crate::ui::business_management::EnsureBusinessPanel)
        .into_configs()
}

pub(crate) fn spawn_person_link(
    parent: &mut ChildSpawnerCommands<'_>,
    person: PersonId,
    name: &str,
    detail: &str,
) {
    let detail = if detail.is_empty() {
        "›".to_string()
    } else {
        format!("{detail}  ›")
    };
    spawn_person_link_with(parent, person, name, &detail, (), (), ());
}

/// A person link whose row, name and detail text carry bind markers, so a
/// retained page can rewrite who the row points at without respawning it.
/// `detail` is the finished trailing text, caret included.
pub(crate) fn spawn_person_link_with(
    parent: &mut ChildSpawnerCommands<'_>,
    person: PersonId,
    name: &str,
    detail: &str,
    link_marker: impl Bundle,
    name_marker: impl Bundle,
    detail_marker: impl Bundle,
) {
    parent
        .spawn((
            Name::new(format!("Person record: {}", person.0)),
            PersonLink(person),
            link_marker,
            Button,
            Node {
                min_height: Val::Px(34.0),
                flex_shrink: 0.0,
                align_items: AlignItems::Center,
                justify_content: JustifyContent::SpaceBetween,
                column_gap: Val::Px(12.0),
                padding: UiRect::axes(Val::Px(10.0), Val::Px(6.0)),
                border: UiRect::bottom(Val::Px(1.0)),
                ..default()
            },
            button_chrome(UiButtonVariant::Row),
        ))
        .with_children(|row| {
            row.spawn((UiButtonLabel, ledger::body_strong(name, 14.0), name_marker));
            row.spawn((
                Text::new(detail),
                ledger::reading(12.0),
                TextColor(INK_MUTED),
                Pickable::IGNORE,
                detail_marker,
            ));
        });
}

pub(super) fn spawn_return_link(parent: &mut ChildSpawnerCommands<'_>) {
    parent
        .spawn((
            Name::new("Back to workplace record"),
            ReturnLink,
            Button,
            Node {
                display: Display::None,
                min_height: Val::Px(34.0),
                flex_shrink: 0.0,
                margin: UiRect::axes(Val::Px(16.0), Val::Px(5.0)),
                padding: UiRect::all(Val::Px(8.0)),
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            button_chrome(UiButtonVariant::Secondary),
        ))
        .with_child((
            ReturnLabel,
            UiButtonLabel,
            ledger::body_strong("‹ Back to company", 13.0),
        ));
}

#[derive(SystemParam)]
struct Navigation<'w> {
    tab: ResMut<'w, EncyclopediaTab>,
    selected: ResMut<'w, SelectedPerson>,
    filter: ResMut<'w, PeopleFilter>,
    search: ResMut<'w, search::EncyclopediaSearch>,
    company: ResMut<'w, companies::SelectedCompany>,
    place: ResMut<'w, places::SelectedPlace>,
    place_entry: ResMut<'w, places::SelectedPlaceEntry>,
    place_return: ResMut<'w, companies::CompanyDrilldownReturn>,
    management: ResMut<'w, BusinessManagementTarget>,
    management_page: ResMut<'w, BusinessManagementPage>,
    management_return: ResMut<'w, BusinessManagementReturn>,
    return_to: ResMut<'w, PersonLinkReturn>,
}

fn handle_links(
    guard: Res<ClickGuard>,
    mouse: Res<ButtonInput<MouseButton>>,
    buttons: Query<
        (Entity, &Interaction, &PersonLink),
        (Changed<Interaction>, Without<InteractionDisabled>),
    >,
    people: Res<KnownPeople>,
    god: Res<crate::ui::hud::GodCapability>,
    nodes: Query<&Node>,
    parents: Query<&ChildOf>,
    mut nav: Navigation,
) {
    if !guard.0 || !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    for (entity, interaction, link) in &buttons {
        if *interaction != Interaction::Pressed
            || !can_open(&people, link.0, god.0)
            || !visible_control(entity, &nodes, &parents)
        {
            continue;
        }
        nav.return_to.0 = if let Some(target) = nav.management.0 {
            Some(ReturnDestination::Management(
                target,
                *nav.management_page,
                nav.management_return.0,
                *nav.tab,
            ))
        } else if *nav.tab == EncyclopediaTab::Companies {
            nav.company.0.map(ReturnDestination::Company)
        } else if *nav.tab == EncyclopediaTab::Places {
            nav.place
                .0
                .map(|place| ReturnDestination::Place(place, *nav.place_entry, nav.place_return.0))
        } else {
            None
        };
        nav.management.0 = None;
        nav.management_return.0 = None;
        nav.selected.0 = Some(link.0);
        *nav.filter = PeopleFilter::All;
        nav.search.clear_people();
        *nav.tab = EncyclopediaTab::People;
        break;
    }
}

/// Retained hidden page controls must not dispatch a stale press.
fn visible_control(mut entity: Entity, nodes: &Query<&Node>, parents: &Query<&ChildOf>) -> bool {
    loop {
        if nodes
            .get(entity)
            .is_ok_and(|node| node.display == Display::None)
        {
            return false;
        }
        let Ok(parent) = parents.get(entity) else {
            return true;
        };
        entity = parent.parent();
    }
}

fn can_open(people: &KnownPeople, person: PersonId, god: bool) -> bool {
    people
        .find_by_id(person)
        .is_some_and(|record| record.known || record.is_self || god)
}

fn handle_return(
    guard: Res<ClickGuard>,
    mouse: Res<ButtonInput<MouseButton>>,
    buttons: Query<&Interaction, (With<ReturnLink>, Changed<Interaction>)>,
    tabs: Query<&Interaction, (With<TabButton>, Changed<Interaction>)>,
    mut nav: Navigation,
) {
    if !guard.0 || !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    if tabs
        .iter()
        .any(|interaction| *interaction == Interaction::Pressed)
    {
        nav.return_to.0 = None;
        return;
    }
    if *nav.tab != EncyclopediaTab::People || !buttons.iter().any(|i| *i == Interaction::Pressed) {
        return;
    }
    match nav.return_to.0.take() {
        Some(ReturnDestination::Company(company)) => {
            nav.company.0 = Some(company);
            nav.search.clear_companies();
            *nav.tab = EncyclopediaTab::Companies;
        }
        Some(ReturnDestination::Place(place, entry, company)) => {
            nav.place.0 = Some(place);
            *nav.place_entry = entry;
            nav.place_return.0 = company;
            nav.search.clear_places();
            *nav.tab = EncyclopediaTab::Places;
        }
        Some(ReturnDestination::Management(target, page, company, origin_tab)) => {
            nav.management.0 = Some(target);
            *nav.management_page = page;
            nav.management_return.0 = company;
            *nav.tab = origin_tab;
        }
        None => {}
    }
}

fn sync_links(
    mut commands: Commands,
    people: Res<KnownPeople>,
    god: Res<crate::ui::hud::GodCapability>,
    return_to: Res<PersonLinkReturn>,
    directory: Res<companies::CompanyDirectory>,
    mut roots: Query<&mut Node, With<ReturnLink>>,
    mut labels: Query<&mut Text, With<ReturnLabel>>,
    links: Query<(Entity, &PersonLink, Has<InteractionDisabled>)>,
) {
    for (entity, link, disabled) in &links {
        let enabled = can_open(&people, link.0, god.0);
        if enabled && disabled {
            commands.entity(entity).remove::<InteractionDisabled>();
        }
        if !enabled && !disabled {
            commands.entity(entity).insert(InteractionDisabled);
        }
    }
    for mut node in &mut roots {
        let display = if return_to.0.is_some() {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
    let company_name = |id| {
        directory
            .records
            .iter()
            .find(|company| company.id == id)
            .map_or_else(|| "company".to_string(), |company| company.name.clone())
    };
    let text = match return_to.0 {
        Some(ReturnDestination::Company(id)) => format!("‹ Back to {}", company_name(id)),
        Some(ReturnDestination::Place(..)) => "‹ Back to workplace".into(),
        Some(ReturnDestination::Management(_, BusinessManagementPage::Site, _, _)) => {
            "‹ Back to site settings".into()
        }
        Some(ReturnDestination::Management(..)) => "‹ Back to company settings".into(),
        None => String::new(),
    };
    for mut label in &mut labels {
        if label.0 != text {
            label.0 = text.clone();
        }
    }
}

fn clear_return(mut return_to: ResMut<PersonLinkReturn>) {
    if return_to.0.is_some() {
        return_to.0 = None;
    }
}

#[cfg(test)]
mod tests;
