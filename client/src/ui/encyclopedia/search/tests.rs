use super::super::{
    Affiliation, KnownPeople, PeopleFilter, PeopleListContent, PersonKind, PersonRow,
    SelectedPerson, actions, companies, state_sync,
};
use super::*;
use bevy::input::ButtonState;

fn person(id: u64, name: &str, known: bool) -> PersonRecord {
    PersonRecord {
        id: shared::components::PersonId(id),
        name: name.into(),
        kind: PersonKind::Villager,
        affiliation: Affiliation::default(),
        level: 0,
        prestige: 0,
        online: true,
        alive: true,
        health: None,
        death_day: None,
        death_cause: None,
        known,
        is_self: false,
        commanded_by: None,
        residence: Some("Broadholt".into()),
        home: None,
        occupation: None,
        workplace: Some("Oak Mill".into()),
        wallet: None,
        nutrition: None,
        activity: None,
        objective: None,
        day_plan: None,
        navigation: None,
        attributes: None,
        work_status: None,
        daily_wage: None,
        workforce_requirements: None,
        inventory: None,
        carried: None,
    }
}

#[test]
fn search_matches_unicode_case_and_all_terms_across_only_known_name_and_places() {
    let mut search = EncyclopediaSearch::default();
    search.set_query(EncyclopediaTab::People, "  ÉLo BROAD  ");
    let people = KnownPeople {
        records: vec![person(1, "Élodie", true), person(2, "Éloise", false)],
        requested: false,
    };
    let found: Vec<_> = people
        .visible(PeopleFilter::All, false)
        .into_iter()
        .filter(|person| search.matches_person(person))
        .map(|person| person.id.0)
        .collect();
    assert_eq!(found, [1]);
    assert!(!search.matches_person(&person(3, "Oliver", true)));
    search.set_query(EncyclopediaTab::People, "oak mill");
    assert!(search.matches_person(&people.records[0]));
    search.set_query(EncyclopediaTab::People, "wallet");
    assert!(!search.matches_person(&people.records[0]));
    search.set_query(EncyclopediaTab::Places, " BrOAD ");
    assert!(search.matches_place("Broadholt"));
    assert!(!search.matches_place("Oakbank"));
    search.clear_people();
    assert!(search.active(EncyclopediaTab::Places));
    assert!(!search.active(EncyclopediaTab::People));
}

#[test]
fn unicode_selection_edits_are_bounded_and_single_line() {
    let mut draft = SearchDraft::default();
    draft.insert("Élodie 世界");
    draft.navigate(&Key::Home, false);
    draft.navigate(&Key::ArrowRight, true);
    draft.insert("A");
    assert_eq!(draft.text, "Alodie 世界");
    draft.navigate(&Key::End, false);
    draft.erase(true);
    assert_eq!(draft.text, "Alodie 世");
    draft.anchor = 0;
    draft.insert("Oak\nMill\t ");
    assert_eq!(draft.text, "OakMill ");
    draft.insert(&"é".repeat(200));
    assert_eq!(draft.text.chars().count(), MAX_CHARACTERS);
    assert!(draft.text.is_char_boundary(draft.cursor));
}

/// A book open on `tab` whose search field already holds keyboard focus.
fn input_app(tab: EncyclopediaTab) -> (App, Entity) {
    let mut app = App::new();
    app.init_resource::<EncyclopediaSearch>()
        .init_resource::<InputState>()
        .insert_resource(EncyclopediaOpen(true))
        .insert_resource(tab)
        .insert_resource(ClickGuard(true))
        .init_resource::<ButtonInput<KeyCode>>()
        .init_resource::<ButtonInput<MouseButton>>()
        .init_resource::<InputFocus>()
        .add_message::<KeyboardInput>()
        .add_systems(PreUpdate, (reset_capture, handle_input).chain())
        .add_systems(
            Update,
            (
                actions::toggle_encyclopedia,
                actions::close_on_escape_or_backdrop,
            )
                .chain(),
        );
    app.add_systems(Startup, move |mut commands: Commands| {
        commands
            .spawn((TabBody(tab), Node::default()))
            .with_children(|directory| spawn(directory, tab, DIRECTORY_MARGIN));
    });
    app.update();
    let field = app
        .world_mut()
        .query_filtered::<Entity, With<SearchField>>()
        .single(app.world())
        .unwrap();
    // The foundation gives every enabled control its tab stop in production;
    // this fixture runs no foundation, so seed the one the field would have.
    app.world_mut().entity_mut(field).insert(TabIndex(0));
    app.world_mut()
        .resource_mut::<InputFocus>()
        .set(field, FocusCause::Pressed);
    (app, field)
}

// Chat is the first capture owner in production; mimic only its reset in this focused fixture.
fn reset_capture(mut input: ResMut<InputState>) {
    input.text_input_active = false;
    input.text_input_captured = false;
}

fn key(app: &mut App, code: KeyCode, logical: Key, text: Option<&str>) {
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .press(code);
    app.world_mut().write_message(KeyboardInput {
        key_code: code,
        logical_key: logical,
        text: text.map(Into::into),
        state: ButtonState::Pressed,
        repeat: false,
        window: Entity::PLACEHOLDER,
    });
}

#[test]
fn typing_and_escape_claim_the_frame_without_closing_or_rebuilding_the_field() {
    let (mut app, field) = input_app(EncyclopediaTab::People);
    for ch in ["n", "e", "t"] {
        let code = match ch {
            "n" => KeyCode::KeyN,
            "e" => KeyCode::KeyE,
            _ => KeyCode::KeyT,
        };
        key(&mut app, code, Key::Character(ch.into()), Some(ch));
        app.update();
        assert!(app.world().resource::<EncyclopediaOpen>().0);
        assert!(app.world().resource::<InputState>().text_input_blocking());
        assert_eq!(app.world().resource::<InputFocus>().get(), Some(field));
        app.world_mut()
            .resource_mut::<ButtonInput<KeyCode>>()
            .reset_all();
    }
    assert_eq!(
        app.world().resource::<EncyclopediaSearch>().people.text,
        "net"
    );
    key(&mut app, KeyCode::Escape, Key::Escape, None);
    app.update();
    assert!(app.world().resource::<EncyclopediaOpen>().0);
    assert!(app.world().resource::<InputState>().text_input_captured);
    assert_eq!(app.world().resource::<InputFocus>().get(), None);
    assert!(app.world().get::<SearchField>(field).is_some());
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .reset_all();
    key(&mut app, KeyCode::Escape, Key::Escape, None);
    app.update();
    assert!(!app.world().resource::<EncyclopediaOpen>().0);
}

#[test]
fn hidden_tab_cannot_keep_typing_and_visible_search_is_retained() {
    let (mut app, field) = input_app(EncyclopediaTab::People);
    key(
        &mut app,
        KeyCode::KeyA,
        Key::Character("A".into()),
        Some("A"),
    );
    app.update();
    *app.world_mut().resource_mut::<EncyclopediaTab>() = EncyclopediaTab::Places;
    key(
        &mut app,
        KeyCode::KeyB,
        Key::Character("B".into()),
        Some("B"),
    );
    app.update();
    assert_eq!(
        app.world().resource::<EncyclopediaSearch>().people.text,
        "A"
    );
    assert_eq!(app.world().resource::<InputFocus>().get(), None);
    assert!(app.world().get::<InteractionDisabled>(field).is_some());
    assert!(app.world().get::<TabIndex>(field).is_none());
}

#[test]
fn company_draft_is_independent_and_clear_companies_is_scoped() {
    let mut search = EncyclopediaSearch::default();
    search.set_query(EncyclopediaTab::People, "ada");
    search.set_query(EncyclopediaTab::Places, "brack");
    search.set_query(EncyclopediaTab::Companies, "Cassia");
    assert_eq!(search.query(EncyclopediaTab::Companies), "Cassia");
    assert_eq!(search.query(EncyclopediaTab::People), "ada");
    assert_eq!(search.query(EncyclopediaTab::Places), "brack");
    // Retinue and Army have no field of their own: they read the People draft.
    assert_eq!(search.query(EncyclopediaTab::Retinue), "ada");
    assert_eq!(search.query(EncyclopediaTab::Army), "ada");
    assert!(search.active(EncyclopediaTab::Companies));
    let revision = search.revision(EncyclopediaTab::Companies);
    search.clear_companies();
    assert!(!search.active(EncyclopediaTab::Companies));
    assert_eq!(search.query(EncyclopediaTab::Companies), "");
    assert_ne!(search.revision(EncyclopediaTab::Companies), revision);
    assert!(search.active(EncyclopediaTab::People));
    assert!(search.active(EncyclopediaTab::Places));
    // Clearing an empty draft is not an edit: list owners keep their rows.
    let cleared = search.revision(EncyclopediaTab::Companies);
    search.clear_companies();
    assert_eq!(search.revision(EncyclopediaTab::Companies), cleared);
    search.clear_people();
    assert!(search.active(EncyclopediaTab::Places));
    assert!(!search.active(EncyclopediaTab::People));
}

/// N toggles the book everywhere except inside a text field. The Companies
/// field must claim the frame like People's, or typing a company name that
/// contains "n" would close the window under the player.
#[test]
fn typing_n_in_the_companies_field_never_closes_the_book_and_escape_leaves_editing() {
    let (mut app, field) = input_app(EncyclopediaTab::Companies);
    key(
        &mut app,
        KeyCode::KeyN,
        Key::Character("n".into()),
        Some("n"),
    );
    app.update();
    assert!(
        app.world().resource::<EncyclopediaOpen>().0,
        "N must type into the field, not toggle the book"
    );
    assert!(app.world().resource::<InputState>().text_input_blocking());
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(field));
    assert_eq!(
        app.world().resource::<EncyclopediaSearch>().companies.text,
        "n"
    );
    assert!(
        app.world()
            .resource::<EncyclopediaSearch>()
            .people
            .text
            .is_empty()
    );
    assert!(app.world().get::<InteractionDisabled>(field).is_none());
    assert!(app.world().get::<TabIndex>(field).is_some());
    app.world_mut()
        .resource_mut::<ButtonInput<KeyCode>>()
        .reset_all();
    key(&mut app, KeyCode::Escape, Key::Escape, None);
    app.update();
    assert!(app.world().resource::<EncyclopediaOpen>().0);
    assert!(app.world().resource::<InputState>().text_input_captured);
    assert_eq!(app.world().resource::<InputFocus>().get(), None);
    assert_eq!(
        app.world().resource::<EncyclopediaSearch>().companies.text,
        "n",
        "Escape leaves editing and keeps the draft"
    );
    assert!(app.world().get::<SearchField>(field).is_some());
}

/// Company settings and ledgers cover the Companies body through the page
/// host. The covered field leaves tab order, drops focus and ignores keys.
#[test]
fn a_covering_page_disables_the_companies_search_field() {
    use crate::ui::business_management::{BusinessManagementSelection, BusinessManagementTarget};
    let (mut app, field) = input_app(EncyclopediaTab::Companies);
    app.init_resource::<crate::ui::history::HistoryPanelTarget>()
        .init_resource::<BusinessManagementTarget>()
        .init_resource::<crate::ui::business_management::BusinessManagementReturn>()
        .init_resource::<crate::ui::company_founding::FoundingPageOpen>()
        .init_resource::<crate::ui::market::MarketPageTarget>()
        .init_resource::<companies::CompanyDirectory>()
        .add_systems(Update, super::super::sync_page_host);
    app.update();
    assert!(app.world().get::<InteractionDisabled>(field).is_none());
    assert!(app.world().get::<TabIndex>(field).is_some());
    assert_eq!(app.world().resource::<InputFocus>().get(), Some(field));

    app.world_mut().resource_mut::<BusinessManagementTarget>().0 = Some(
        BusinessManagementSelection::Company(shared::components::CompanyId(1)),
    );
    // The page host hides the body in Update; the field reacts next PreUpdate.
    app.update();
    app.update();
    assert!(app.world().get::<InteractionDisabled>(field).is_some());
    assert!(app.world().get::<TabIndex>(field).is_none());
    assert_eq!(app.world().resource::<InputFocus>().get(), None);
    key(
        &mut app,
        KeyCode::KeyA,
        Key::Character("a".into()),
        Some("a"),
    );
    app.update();
    assert!(
        app.world()
            .resource::<EncyclopediaSearch>()
            .companies
            .text
            .is_empty(),
        "a covered field cannot take text"
    );
}

#[test]
fn filtering_keeps_person_selection_but_privacy_revocation_clears_it() {
    let mut app = App::new();
    app.init_resource::<EncyclopediaSearch>()
        .insert_resource(KnownPeople {
            records: vec![person(1, "Élodie", true)],
            requested: false,
        })
        .init_resource::<PeopleFilter>()
        .init_resource::<crate::ui::hud::GodCapability>()
        .insert_resource(SelectedPerson(Some(shared::components::PersonId(1))))
        .init_resource::<crate::ui::perf::UiPerf>()
        .add_systems(Update, state_sync::rebuild_people_list);
    let content = app
        .world_mut()
        .spawn((PeopleListContent, Node::default()))
        .id();
    app.world_mut()
        .resource_mut::<EncyclopediaSearch>()
        .set_query(EncyclopediaTab::People, "elsewhere");
    app.update();
    assert_eq!(
        app.world().resource::<SelectedPerson>().0,
        Some(shared::components::PersonId(1))
    );
    assert!(
        !app.world_mut()
            .query::<&PersonRow>()
            .iter(app.world())
            .any(|row| row.id.0 == 1)
    );
    app.world_mut()
        .resource_mut::<EncyclopediaSearch>()
        .clear_people();
    app.update();
    assert!(
        app.world_mut()
            .query::<&PersonRow>()
            .iter(app.world())
            .any(|row| row.id.0 == 1)
    );
    assert!(app.world().get::<PeopleListContent>(content).is_some());
    app.world_mut().resource_mut::<KnownPeople>().records[0].known = false;
    app.update();
    assert_eq!(app.world().resource::<SelectedPerson>().0, None);
    assert!(
        !app.world_mut()
            .query::<&PersonRow>()
            .iter(app.world())
            .any(|row| row.id.0 == 1)
    );
}
