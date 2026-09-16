use super::super::{
    Affiliation, KnownPeople, PeopleFilter, PeopleListContent, PersonKind, PersonRow,
    SelectedPerson, actions, state_sync,
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

fn input_app() -> (App, Entity) {
    let mut app = App::new();
    app.init_resource::<EncyclopediaSearch>()
        .init_resource::<InputState>()
        .insert_resource(EncyclopediaOpen(true))
        .init_resource::<EncyclopediaTab>()
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
    app.add_systems(Startup, |mut commands: Commands| {
        commands
            .spawn((TabBody(EncyclopediaTab::People), Node::default()))
            .with_children(|directory| spawn(directory, EncyclopediaTab::People));
    });
    app.update();
    let field = app
        .world_mut()
        .query_filtered::<Entity, With<SearchField>>()
        .single(app.world())
        .unwrap();
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
    let (mut app, field) = input_app();
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
    let (mut app, field) = input_app();
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
