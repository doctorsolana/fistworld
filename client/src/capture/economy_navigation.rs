//! Native-input search and company/site navigation acceptance with offline facts.
//! No financial transaction or simulated worker production is fabricated here.
use super::ledger_nested::{bounds, entities, scroll_ancestor, scroll_limit, shot, visible};
use super::{CaptureConfig, CaptureState};
use crate::ui::{
    business_management::{
        BodyScroll, BusinessManagementPage as Scope, BusinessManagementSelection as Target,
        BusinessManagementTarget,
    },
    encyclopedia::{
        self, ClickGuard, EncyclopediaOpen, EncyclopediaPageBack, EncyclopediaPanel,
        EncyclopediaTab as Tab, PersonRow, SelectedPerson, TabButton, companies, places,
        search::{ClearSearch, EncyclopediaSearch, SearchField},
    },
    ledger::{IllustrationMaterial, LedgerArtwork, LedgerIllustration},
    portraits::{PersonPortrait, PortraitMetrics, PortraitStatus},
};
use bevy::{
    input::{
        ButtonState, InputSystems,
        keyboard::{Key, KeyboardInput},
    },
    input_focus::InputFocus,
    prelude::*,
    ui::{InteractionDisabled, UiSystems},
    window::PrimaryWindow,
};
use shared::{components::*, economy::*};

#[derive(Resource, Default)]
pub(super) struct Rehearsal {
    staged: bool,
    pressed: Option<Entity>,
    typed: Option<usize>,
    ready_shot: Option<usize>,
    inspected: Option<usize>,
    error: String,
    retained_buttons: Vec<Entity>,
    site_scroll: Option<f32>,
    company_scroll: Option<f32>,
}

pub(super) fn install(app: &mut App) {
    if std::env::var("FISTWORLD_CAPTURE_ECONOMY_NAVIGATION").as_deref() != Ok("1") {
        return;
    }
    app.init_resource::<Rehearsal>();
    app.add_systems(Update, stage);
    app.add_systems(
        PreUpdate,
        input
            .after(InputSystems)
            .after(UiSystems::Focus)
            .before(crate::ui::chat::ChatInput),
    );
    app.add_systems(Last, inspect);
}

fn stage(world: &mut World) {
    if world.resource::<Rehearsal>().staged {
        return;
    }
    let Some(hero) = entities::<Hero>(world).into_iter().next() else {
        return;
    };
    if !entities::<CompanyId>(world)
        .iter()
        .any(|e| world.get::<CompanyId>(*e) == Some(&CompanyId(501)))
    {
        return;
    }
    let owner = world.get::<Hero>(hero).unwrap().owner;
    world
        .entity_mut(hero)
        .insert((PersonId(1), CharacterName("Aldric".into())));
    world.insert_resource(crate::camera_rts::LocalPeerId(
        shared::player::peer_id_to_u64(owner),
    ));
    // The base company fixture contains directory records, not embodied
    // appearances. Record a bounded prior outfit observation for that cast,
    // as the encyclopedia tour does, so its visible portraits can really
    // finish rasterizing rather than waiting forever on unknown likenesses.
    let directory: Vec<_> = world
        .resource::<encyclopedia::KnownPeople>()
        .records
        .iter()
        .map(|record| record.id)
        .filter(|id| id.is_assigned())
        .collect();
    let observed: Vec<_> = world
        .query::<(&PersonId, &HeroOutfit)>()
        .iter(world)
        .map(|(id, _)| *id)
        .collect();
    for id in directory {
        if !observed.contains(&id) {
            world.spawn((id, HeroOutfit::varied(550 + id.0 * 19)));
        }
    }
    for (id, site, name) in [(9101, 602, "Ada"), (9102, 601, "Ada"), (9103, 602, "Edwin")] {
        world.spawn((
            PersonId(id),
            CharacterName(name.into()),
            CharacterKind::Villager,
            EmployedAt(BuildingId(site)),
            Residence("Brackwater".into()),
            Occupation(Some("Company worker".into())),
            CharacterAttributes::new(12, 11, 10),
            Health::new(100.0),
            HeroOutfit::varied(id),
            Wallet::new(500),
            CharacterActivity::Idle,
        ));
    }
    world.spawn((
        CompanyId(503),
        Company {
            name: "Aldric New Venture".into(),
            founded_day: 12,
        },
        CompanyOwnership::sole(PersonId(1)),
        CompanyLeadership {
            master: PersonId(1),
        },
        CompanyShareMarket::default(),
        CompanyAccount {
            cash: 900,
            ..default()
        },
        CompanyManagementPolicy::default(),
        CompanyBranchPolicies::default(),
        CompanyDecisionHistory::default(),
    ));
    world.resource_mut::<companies::SelectedCompany>().0 = Some(CompanyId(501));
    world.resource_mut::<EncyclopediaOpen>().0 = true;
    world.resource_mut::<crate::ui::hud::GodCapability>().0 = false;
    world.resource_mut::<Rehearsal>().staged = true;
}

fn find<T: Component>(world: &mut World, predicate: impl Fn(&T) -> bool) -> Option<Entity> {
    entities::<T>(world)
        .into_iter()
        .find(|e| world.get::<T>(*e).is_some_and(&predicate) && active_tree(world, *e))
}

/// Allow clipped rows to be revealed, but never choose a retained hidden page.
fn active_tree(world: &World, mut entity: Entity) -> bool {
    loop {
        if world
            .get::<Node>(entity)
            .is_some_and(|node| node.display == Display::None)
        {
            return false;
        }
        let Some(parent) = world.get::<ChildOf>(entity) else {
            return true;
        };
        entity = parent.parent();
    }
}

fn press(world: &mut World, entity: Option<Entity>) {
    let Some(entity) = entity else {
        return;
    };
    if !world.resource::<ClickGuard>().0 || world.get::<InteractionDisabled>(entity).is_some() {
        return;
    }
    if !visible(world, entity) {
        if let (Some(target), Some(ancestor)) =
            (bounds(world, entity), scroll_ancestor(world, entity))
        {
            if let Some(rect) = bounds(world, ancestor) {
                let inverse = world
                    .get::<ComputedNode>(ancestor)
                    .unwrap()
                    .inverse_scale_factor();
                let limit = scroll_limit(world, ancestor);
                if let Some(mut scroll) = world.get_mut::<ScrollPosition>(ancestor) {
                    scroll.y = (scroll.y + (target.center().y - rect.center().y) * inverse)
                        .clamp(0.0, limit);
                }
            }
        }
        return;
    }
    if let Some(mut interaction) = world.get_mut::<Interaction>(entity) {
        *interaction = Interaction::Pressed;
    }
    world
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    world.resource_mut::<Rehearsal>().pressed = Some(entity);
}

fn tab(world: &mut World, target: Tab) -> bool {
    if *world.resource::<Tab>() == target {
        return true;
    }
    let button = find::<TabButton>(world, |button| button.0 == target);
    press(world, button);
    false
}

fn text_button(world: &mut World, label: &str) -> Option<Entity> {
    let text = entities::<Text>(world)
        .into_iter()
        .find(|e| world.get::<Text>(*e).is_some_and(|t| t.0 == label) && visible(world, *e))?;
    let mut entity = text;
    loop {
        if world.get::<Button>(entity).is_some() {
            return Some(entity);
        }
        entity = world.get::<ChildOf>(entity)?.parent();
    }
}

fn type_query(world: &mut World, index: usize, target: Tab, value: &str) {
    if !tab(world, target) || world.resource::<Rehearsal>().typed == Some(index) {
        return;
    }
    let Some(field) = find::<SearchField>(world, |field| field.0 == target) else {
        return;
    };
    if world.resource::<InputFocus>().get() != Some(field) {
        press(world, Some(field));
        return;
    }
    let Some(window) = entities::<PrimaryWindow>(world).into_iter().next() else {
        return;
    };
    world.write_message(KeyboardInput {
        key_code: KeyCode::KeyA,
        logical_key: Key::Character(value.into()),
        state: ButtonState::Pressed,
        text: Some(value.into()),
        repeat: false,
        window,
    });
    world.resource_mut::<Rehearsal>().typed = Some(index);
}

fn site(world: &mut World) -> Option<Entity> {
    find::<BuildingId>(world, |id| *id == BuildingId(602))
}

fn manager_buttons(world: &mut World) -> Vec<Entity> {
    let mut result: Vec<_> = entities::<Button>(world)
        .into_iter()
        .filter(|e| {
            let mut cursor = *e;
            loop {
                if world.get::<BodyScroll>(cursor).is_some() {
                    return true;
                }
                let Some(parent) = world.get::<ChildOf>(cursor) else {
                    return false;
                };
                cursor = parent.parent();
            }
        })
        .collect();
    result.sort_unstable();
    result
}

fn viewport(world: &mut World) -> Option<Entity> {
    entities::<BodyScroll>(world)
        .into_iter()
        .find(|e| visible(world, *e))
}

fn input(world: &mut World) {
    // Hidden captures have no OS keyboard focus. This opt-in input rehearsal
    // represents a focused game window, just as its synthetic button edges
    // represent a pointer click; production search still owns every key event.
    for mut window in world
        .query_filtered::<&mut Window, With<PrimaryWindow>>()
        .iter_mut(world)
    {
        window.focused = true;
    }
    world
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    if let Some(previous) = world.resource_mut::<Rehearsal>().pressed.take() {
        if let Some(mut interaction) = world.get_mut::<Interaction>(previous) {
            *interaction = Interaction::None;
        }
    }
    if !world.resource::<Rehearsal>().staged
        || !matches!(
            *world.resource::<CaptureState>(),
            CaptureState::Warmup { .. } | CaptureState::Settling { .. }
        )
    {
        return;
    }
    let Some((index, name)) = shot(world) else {
        return;
    };
    let phase = name
        .split('-')
        .next()
        .and_then(|n| n.parse::<u8>().ok())
        .unwrap();
    match phase {
        1 => {
            tab(world, Tab::People);
        }
        2 => type_query(world, index, Tab::People, "Ada"),
        3 | 6 => {
            let which = if phase == 3 { Tab::People } else { Tab::Places };
            if tab(world, which)
                && !world
                    .resource::<EncyclopediaSearch>()
                    .query(which)
                    .is_empty()
            {
                let button = find::<ClearSearch>(world, |button| button.0 == which);
                press(world, button);
            }
        }
        4 => type_query(world, index, Tab::People, "zz-no-such-person"),
        5 => type_query(world, index, Tab::Places, "Brackwater"),
        7 => {
            tab(world, Tab::Companies);
        }
        8 | 10 | 18 => {
            if phase == 18
                && world.resource::<companies::SelectedCompany>().0 != Some(CompanyId(503))
            {
                let button =
                    find::<companies::CompanyRow>(world, |button| button.0 == CompanyId(503));
                press(world, button);
                return;
            }
            let target = if phase == 10 {
                site(world).map(Target::Site)
            } else {
                Some(Target::Company(CompanyId(if phase == 18 {
                    503
                } else {
                    501
                })))
            };
            if world.resource::<BusinessManagementTarget>().0 != target {
                let button = find::<companies::CompanyManagementButton>(world, |button| {
                    Some(button.target) == target
                });
                press(world, button);
            }
        }
        9 | 17 => {
            if world.resource::<BusinessManagementTarget>().0.is_some() {
                let button = find::<EncyclopediaPageBack>(world, |_| true);
                press(world, button);
            }
        }
        11 => {
            if world.resource::<BusinessManagementTarget>().0.is_some() {
                let button = find::<encyclopedia::person_links::PersonLink>(world, |button| {
                    button.0 == PersonId(9101)
                });
                press(world, button);
            }
        }
        12 => {
            if world.resource::<BusinessManagementTarget>().0.is_none() {
                let button =
                    find::<Name>(world, |name| name.as_str() == "Back to workplace record");
                press(world, button);
            }
        }
        13 | 15 => {
            if let Some(viewport) = viewport(world) {
                let wanted =
                    scroll_limit(world, viewport).min(if phase == 13 { 220.0 } else { 240.0 });
                world.get_mut::<ScrollPosition>(viewport).unwrap().y = wanted;
                if phase == 13 {
                    let ids = manager_buttons(world);
                    let mut state = world.resource_mut::<Rehearsal>();
                    state.site_scroll = Some(wanted);
                    state.retained_buttons = ids;
                } else {
                    world.resource_mut::<Rehearsal>().company_scroll = Some(wanted);
                }
            }
        }
        14 | 16 => {
            let wanted = if phase == 14 {
                Scope::Company
            } else {
                Scope::Site
            };
            if *world.resource::<Scope>() != wanted {
                let button = text_button(world, if phase == 14 { "COMPANY" } else { "SITE" });
                press(world, button);
            }
        }
        19 => {
            // Phase 18 left the empty company's settings page open.
            if world.resource::<BusinessManagementTarget>().0.is_some() {
                let button = find::<EncyclopediaPageBack>(world, |_| true);
                press(world, button);
                return;
            }
            type_query(world, index, Tab::Companies, "cassia");
        }
        20 => {
            if tab(world, Tab::Companies)
                && !world
                    .resource::<EncyclopediaSearch>()
                    .query(Tab::Companies)
                    .is_empty()
            {
                let button = find::<ClearSearch>(world, |button| button.0 == Tab::Companies);
                press(world, button);
            }
        }
        21 => {
            // One press per frame until the cycling key reads NAME.
            if world.resource::<companies::CompanySort>().key != companies::CompanySortKey::Name {
                let button = find::<companies::CompanySortKeyButton>(world, |_| true);
                press(world, button);
            }
        }
        22 => {
            if !world.resource::<companies::CompanySort>().descending {
                let button = find::<companies::CompanySortDirectionButton>(world, |_| true);
                press(world, button);
            }
        }
        _ => panic!("unknown economy navigation phase: {name}"),
    }
}

fn check(world: &mut World, index: usize, name: &str) -> Result<(), String> {
    macro_rules! require {
        ($condition:expr, $reason:expr) => {
            if !$condition {
                return Err($reason.into());
            }
        };
    }
    require!(world.resource::<Rehearsal>().staged, "fixture not staged");
    require!(
        world
            .resource::<LedgerArtwork>()
            .ready(world.resource::<AssetServer>()),
        "ledger artwork loading"
    );
    require!(
        world.resource::<EncyclopediaOpen>().0,
        "book closed unexpectedly"
    );
    let panels = entities::<EncyclopediaPanel>(world);
    require!(
        panels.len() == 1 && visible(world, panels[0]),
        "one visible laid-out book required"
    );
    let rect = bounds(world, panels[0]).unwrap();
    let size = world.resource::<CaptureConfig>().resolution;
    require!(
        rect.min.x >= -1.0
            && rect.min.y >= -1.0
            && rect.max.x <= size[0] as f32 + 1.0
            && rect.max.y <= size[1] as f32 + 1.0,
        "book exceeds viewport"
    );
    for entity in entities::<PortraitStatus>(world) {
        let status = *world.get::<PortraitStatus>(entity).unwrap();
        if visible(world, entity) && !status.ready {
            return Err(format!(
                "visible portrait work remains queued: {:?}, {:?}",
                world.get::<PersonPortrait>(entity),
                status,
            ));
        }
    }
    require!(
        world.resource::<PortraitMetrics>().bytes <= 32 * 1024 * 1024,
        "portrait texture budget exceeded"
    );
    for entity in entities::<LedgerIllustration>(world) {
        if visible(world, entity) {
            let kind = *world.get::<LedgerIllustration>(entity).unwrap();
            let image = world.get::<MaterialNode<IllustrationMaterial>>(entity);
            require!(
                image.is_some_and(|image| world
                    .resource::<Assets<IllustrationMaterial>>()
                    .get(&image.0)
                    .is_some_and(
                        |material| material.ready_for(kind, world.resource::<AssetServer>())
                    )),
                "visible illustration loading"
            );
        }
    }
    let phase = name.split('-').next().unwrap().parse::<u8>().unwrap();
    let people: Vec<_> = entities::<PersonRow>(world)
        .into_iter()
        .filter_map(|e| world.get::<PersonRow>(e))
        .filter(|p| p.id.is_assigned())
        .map(|p| p.id)
        .collect();
    let target = world.resource::<BusinessManagementTarget>().0;
    match phase {
        1 | 3 => {
            require!(
                *world.resource::<Tab>() == Tab::People,
                "People tab not open"
            );
            require!(
                world
                    .resource::<EncyclopediaSearch>()
                    .query(Tab::People)
                    .is_empty()
                    && people.contains(&PersonId(9101))
                    && people.contains(&PersonId(9102))
                    && people.contains(&PersonId(9103)),
                "clear did not restore known people"
            );
        }
        2 => {
            require!(
                world.resource::<Rehearsal>().typed == Some(index)
                    && world.resource::<EncyclopediaSearch>().query(Tab::People) == "Ada",
                format!(
                    "native people input not consumed: query={:?}, focus={:?}",
                    world.resource::<EncyclopediaSearch>().query(Tab::People),
                    world.resource::<InputFocus>().get()
                )
            );
            require!(
                people.len() == 2
                    && people.contains(&PersonId(9101))
                    && people.contains(&PersonId(9102)),
                "search did not retain both duplicate names"
            );
        }
        4 => {
            require!(
                people.is_empty()
                    && world.resource::<EncyclopediaSearch>().query(Tab::People)
                        == "zz-no-such-person",
                "no-match people query not applied"
            );
            require!(
                visible_text(world)
                    .iter()
                    .any(|t| t == "No people match this search"),
                "no-match explanation not visible"
            );
        }
        5 | 6 => {
            require!(
                *world.resource::<Tab>() == Tab::Places,
                "Places tab not open"
            );
            let rows = entities::<places::PlaceRow>(world)
                .into_iter()
                .filter(|e| {
                    world
                        .get::<places::PlaceRow>(*e)
                        .is_some_and(|row| row.0 != SettlementId::UNASSIGNED)
                })
                .count();
            if phase == 5 {
                require!(
                    world.resource::<EncyclopediaSearch>().query(Tab::Places) == "Brackwater"
                        && rows == 1,
                    "place search did not narrow to one known town"
                );
            } else {
                require!(
                    world
                        .resource::<EncyclopediaSearch>()
                        .query(Tab::Places)
                        .is_empty()
                        && rows > 1,
                    "place clear did not restore directory"
                );
            }
        }
        7 | 9 | 17 => {
            require!(
                *world.resource::<Tab>() == Tab::Companies && target.is_none(),
                "company directory/back action not restored"
            );
        }
        8 | 18 => {
            let id = CompanyId(if phase == 18 { 503 } else { 501 });
            require!(
                target == Some(Target::Company(id)) && *world.resource::<Scope>() == Scope::Company,
                "company control still targets a site"
            );
            require!(viewport(world).is_some(), "company page not laid out");
            if phase == 18 {
                require!(
                    !entities::<OperatedBy>(world).iter().any(|e| world
                        .get::<OperatedBy>(*e)
                        .is_some_and(|operation| operation.0 == id)),
                    "empty-company fixture unexpectedly owns a site"
                );
            }
        }
        10 | 12 | 13 | 16 => {
            require!(
                target == site(world).map(Target::Site)
                    && *world.resource::<Scope>() == Scope::Site,
                "site control/return opened the wrong context"
            );
            require!(viewport(world).is_some(), "site page not laid out");
        }
        11 => {
            require!(
                target.is_none()
                    && *world.resource::<Tab>() == Tab::People
                    && world.resource::<SelectedPerson>().0 == Some(PersonId(9101)),
                "worker link lost durable person identity"
            );
            require!(
                world
                    .resource::<EncyclopediaSearch>()
                    .query(Tab::People)
                    .is_empty()
                    && people.contains(&PersonId(9101)),
                "worker link did not clear conflicting search"
            );
            require!(
                entities::<encyclopedia::DetailName>(world)
                    .iter()
                    .any(|e| visible(world, *e)
                        && world.get::<Text>(*e).is_some_and(|t| t.0 == "Ada")),
                "worker detail did not render"
            );
        }
        14 | 15 => {
            require!(
                target == site(world).map(Target::Site)
                    && *world.resource::<Scope>() == Scope::Company,
                "company tab failed"
            );
        }
        19..=22 => {
            require!(
                *world.resource::<Tab>() == Tab::Companies && target.is_none(),
                "company directory not restored"
            );
            let query = world
                .resource::<EncyclopediaSearch>()
                .query(Tab::Companies)
                .to_owned();
            let sort = *world.resource::<companies::CompanySort>();
            let rows = company_rows(world);
            let names = company_names(world);
            let texts = visible_text(world);
            match phase {
                19 => {
                    require!(
                        world.resource::<Rehearsal>().typed == Some(index) && query == "cassia",
                        format!(
                            "native company input not consumed: query={query:?}, focus={:?}",
                            world.resource::<InputFocus>().get()
                        )
                    );
                    require!(
                        rows == [CompanyId(502)],
                        format!("company search did not narrow to Cassia River Fish: {names:?}")
                    );
                    require!(
                        world.resource::<companies::SelectedCompany>().0 == Some(CompanyId(503)),
                        "a company search must not revoke the selection"
                    );
                    require!(
                        texts.iter().any(|t| t == "1 of 3"),
                        "search count did not read 1 of 3"
                    );
                }
                20 => {
                    require!(
                        query.is_empty() && rows.len() == 3,
                        format!("company clear did not restore the directory: {names:?}")
                    );
                    require!(
                        texts.iter().any(|t| t == "3 companies"),
                        "directory count did not read 3 companies"
                    );
                }
                21 | 22 => {
                    let expected = [
                        "Aldric Grain & Bread",
                        "Aldric New Venture",
                        "Cassia River Fish",
                    ];
                    let descending = phase == 22;
                    require!(
                        sort == companies::CompanySort {
                            key: companies::CompanySortKey::Name,
                            descending,
                        },
                        format!("sort control did not reach NAME {descending}: {sort:?}")
                    );
                    let mut expected: Vec<&str> = expected.to_vec();
                    if descending {
                        expected.reverse();
                    }
                    require!(
                        names == expected,
                        format!("rows are not in {sort:?} order: {names:?}")
                    );
                    require!(
                        texts.iter().any(|t| t == "NAME")
                            && texts
                                .iter()
                                .any(|t| t == if descending { "v" } else { "^" }),
                        "sort labels did not show the retained choice"
                    );
                }
                _ => unreachable!(),
            }
        }
        _ => return Err("unknown phase".into()),
    }
    if (13..=16).contains(&phase) {
        let buttons = manager_buttons(world);
        require!(
            !buttons.is_empty() && buttons == world.resource::<Rehearsal>().retained_buttons,
            "tab switch rebuilt the retained controls"
        );
        let viewport = viewport(world).ok_or("active management scroll missing")?;
        let actual = world.get::<ScrollPosition>(viewport).unwrap().y;
        let expected = if phase == 13 || phase == 16 {
            world.resource::<Rehearsal>().site_scroll
        } else if phase == 15 {
            world.resource::<Rehearsal>().company_scroll
        } else {
            Some(0.0)
        };
        require!(
            expected.is_some_and(|expected| expected > 0.0 || phase == 14)
                && expected.is_some_and(|expected| (actual - expected).abs() < 1.0),
            "tab scroll was reset or did not move"
        );
    }
    Ok(())
}

/// The directory rows in display order.
fn company_rows(world: &mut World) -> Vec<CompanyId> {
    let Some(list) = entities::<companies::CompanyListContent>(world)
        .into_iter()
        .next()
    else {
        return Vec::new();
    };
    world
        .get::<Children>(list)
        .map(|children| {
            children
                .iter()
                .filter_map(|child| world.get::<companies::CompanyRow>(child).map(|row| row.0))
                .collect()
        })
        .unwrap_or_default()
}

fn company_names(world: &mut World) -> Vec<String> {
    let rows = company_rows(world);
    let directory = world.resource::<companies::CompanyDirectory>();
    rows.iter()
        .map(|id| {
            directory
                .records
                .iter()
                .find(|company| company.id == *id)
                .map_or_else(|| format!("{id:?}"), |company| company.name.clone())
        })
        .collect()
}

fn visible_text(world: &mut World) -> Vec<String> {
    entities::<Text>(world)
        .into_iter()
        .filter(|e| visible(world, *e))
        .filter_map(|e| world.get::<Text>(e).map(|text| text.0.clone()))
        .collect()
}

fn inspect(world: &mut World) {
    let Some((index, name)) = shot(world) else {
        return;
    };
    let result = check(world, index, &name);
    {
        let mut state = world.resource_mut::<Rehearsal>();
        state.ready_shot = result.is_ok().then_some(index);
        state.error = result.as_ref().err().cloned().unwrap_or_default();
    }
    if !matches!(
        *world.resource::<CaptureState>(),
        CaptureState::AwaitingCapture { .. }
    ) || world.resource::<Rehearsal>().inspected == Some(index)
    {
        return;
    }
    result.unwrap_or_else(|error| panic!("economy navigation capture failed: {error}"));
    let texts = visible_text(world);
    let company_rows: Vec<u64> = company_rows(world).into_iter().map(|id| id.0).collect();
    let sort = *world.resource::<companies::CompanySort>();
    let evidence = serde_json::json!({"shot":name,"passed":true,"input":"native KeyboardInput, production button handlers, real ScrollPosition", "fixture":"offline directory and employment facts; no financial or worker-simulation proof", "people_query":world.resource::<EncyclopediaSearch>().query(Tab::People), "places_query":world.resource::<EncyclopediaSearch>().query(Tab::Places), "companies_query":world.resource::<EncyclopediaSearch>().query(Tab::Companies), "company_sort":{"key":format!("{:?}",sort.key),"descending":sort.descending}, "company_rows":company_rows, "selected_company":world.resource::<companies::SelectedCompany>().0.map(|id|id.0), "selected_person":world.resource::<SelectedPerson>().0.map(|id|id.0), "management_target":format!("{:?}",world.resource::<BusinessManagementTarget>().0), "management_scope":format!("{:?}",world.resource::<Scope>()), "retained_controls":world.resource::<Rehearsal>().retained_buttons.len(), "site_scroll":world.resource::<Rehearsal>().site_scroll, "company_scroll":world.resource::<Rehearsal>().company_scroll,"visible_text":texts});
    std::fs::write(
        world
            .resource::<CaptureConfig>()
            .out_dir
            .join(format!("{name}.economy-ui.json")),
        serde_json::to_vec_pretty(&evidence).unwrap(),
    )
    .expect("economy navigation evidence");
    world.resource_mut::<Rehearsal>().inspected = Some(index);
}

pub(super) fn ready(
    rehearsal: Option<Res<Rehearsal>>,
    state: Res<CaptureState>,
    config: Res<CaptureConfig>,
    mut waiting: Local<u32>,
) -> bool {
    let Some(rehearsal) = rehearsal else {
        return true;
    };
    let index = match *state {
        CaptureState::Warmup { .. } => 0,
        CaptureState::Settling { shot, .. } => shot,
        _ => return true,
    };
    if rehearsal.ready_shot == Some(index) {
        *waiting = 0;
        return true;
    }
    *waiting += 1;
    assert!(
        *waiting
            < config.shots[index]
                .readiness
                .maximum_frames
                .max(config.warmup_frames),
        "economy navigation readiness timed out: {}",
        rehearsal.error
    );
    false
}
