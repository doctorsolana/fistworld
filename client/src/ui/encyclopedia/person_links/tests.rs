use super::*;
use bevy::ecs::system::RunSystemOnce;
use shared::components::{BuildingId, CharacterName, EmployedAt};

fn person(id: u64, name: &str, known: bool, own: bool) -> PersonRecord {
    PersonRecord {
        id: PersonId(id),
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
        is_self: own,
        commanded_by: None,
        residence: None,
        home: None,
        occupation: None,
        workplace: None,
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

fn world() -> World {
    let mut world = World::new();
    world.insert_resource(KnownPeople {
        records: vec![
            person(1, "Ada", true, false),
            person(2, "Ada", true, false),
            person(3, "Stranger", false, false),
            person(4, "Hero", false, true),
        ],
        requested: false,
    });
    world.insert_resource(ClickGuard(true));
    world.init_resource::<ButtonInput<MouseButton>>();
    world.init_resource::<crate::ui::hud::GodCapability>();
    world.init_resource::<EncyclopediaTab>();
    world.init_resource::<SelectedPerson>();
    world.init_resource::<PeopleFilter>();
    world.init_resource::<search::EncyclopediaSearch>();
    world.init_resource::<companies::SelectedCompany>();
    world.init_resource::<places::SelectedPlace>();
    world.init_resource::<places::SelectedPlaceEntry>();
    world.init_resource::<companies::CompanyDrilldownReturn>();
    world.init_resource::<BusinessManagementTarget>();
    world.init_resource::<BusinessManagementPage>();
    world.init_resource::<BusinessManagementReturn>();
    world.init_resource::<PersonLinkReturn>();
    world
}

fn open_link(world: &mut World, id: u64) -> Entity {
    world
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    let link = world
        .spawn((
            PersonLink(PersonId(id)),
            Interaction::Pressed,
            Node::default(),
        ))
        .id();
    world.run_system_once(handle_links).unwrap();
    link
}

fn go_back(world: &mut World, link: Entity) {
    world.entity_mut(link).insert(Interaction::None);
    world.spawn((ReturnLink, Interaction::Pressed));
    world.run_system_once(handle_return).unwrap();
}

#[test]
fn identity_permissions_keep_names_distinct_and_allow_own_record() {
    let world = world();
    let people = world.resource::<KnownPeople>();
    assert!(can_open(people, PersonId(1), false));
    assert!(can_open(people, PersonId(2), false));
    assert!(!can_open(people, PersonId(3), false));
    assert!(can_open(people, PersonId(3), true));
    assert!(can_open(people, PersonId(4), false));
    assert!(!can_open(people, PersonId(0), true));
    assert!(!can_open(people, PersonId(99), true));
    assert!(people
        .visible(PeopleFilter::All, false)
        .iter()
        .any(|person| person.id == PersonId(4)));
}

#[test]
fn company_link_clears_people_search_and_returns_to_same_company() {
    let mut world = world();
    *world.resource_mut::<EncyclopediaTab>() = EncyclopediaTab::Companies;
    world.resource_mut::<companies::SelectedCompany>().0 = Some(CompanyId(7));
    *world.resource_mut::<PeopleFilter>() = PeopleFilter::Unknown;
    world
        .resource_mut::<search::EncyclopediaSearch>()
        .set_query(EncyclopediaTab::People, "stranger");
    let link = open_link(&mut world, 2);
    assert_eq!(world.resource::<SelectedPerson>().0, Some(PersonId(2)));
    assert_eq!(
        *world.resource::<EncyclopediaTab>(),
        EncyclopediaTab::People
    );
    assert_eq!(*world.resource::<PeopleFilter>(), PeopleFilter::All);
    assert_eq!(
        world
            .resource::<search::EncyclopediaSearch>()
            .query(EncyclopediaTab::People),
        ""
    );
    go_back(&mut world, link);
    assert_eq!(
        *world.resource::<EncyclopediaTab>(),
        EncyclopediaTab::Companies
    );
    assert_eq!(
        world.resource::<companies::SelectedCompany>().0,
        Some(CompanyId(7))
    );
}

#[test]
fn place_and_management_returns_restore_their_actual_origin() {
    let mut world = world();
    *world.resource_mut::<EncyclopediaTab>() = EncyclopediaTab::Places;
    world.resource_mut::<places::SelectedPlace>().0 = Some(SettlementId(5));
    *world.resource_mut::<places::SelectedPlaceEntry>() = places::SelectedPlaceEntry::Building(2);
    world.resource_mut::<companies::CompanyDrilldownReturn>().0 = Some(CompanyId(7));
    let link = open_link(&mut world, 1);
    go_back(&mut world, link);
    assert_eq!(
        *world.resource::<EncyclopediaTab>(),
        EncyclopediaTab::Places
    );
    assert_eq!(
        world.resource::<places::SelectedPlace>().0,
        Some(SettlementId(5))
    );
    assert_eq!(
        *world.resource::<places::SelectedPlaceEntry>(),
        places::SelectedPlaceEntry::Building(2)
    );
    assert_eq!(
        world.resource::<companies::CompanyDrilldownReturn>().0,
        Some(CompanyId(7))
    );

    let site = world.spawn_empty().id();
    world.resource_mut::<BusinessManagementTarget>().0 =
        Some(BusinessManagementSelection::Site(site));
    *world.resource_mut::<BusinessManagementPage>() = BusinessManagementPage::Site;
    let link = open_link(&mut world, 2);
    assert_eq!(world.resource::<BusinessManagementTarget>().0, None);
    go_back(&mut world, link);
    assert_eq!(
        *world.resource::<EncyclopediaTab>(),
        EncyclopediaTab::Places
    );
    assert_eq!(
        world.resource::<BusinessManagementTarget>().0,
        Some(BusinessManagementSelection::Site(site))
    );
    assert_eq!(
        *world.resource::<BusinessManagementPage>(),
        BusinessManagementPage::Site
    );
}

#[test]
fn unknown_and_hidden_retained_links_cannot_navigate() {
    let mut world = world();
    *world.resource_mut::<EncyclopediaTab>() = EncyclopediaTab::Companies;
    let stranger = open_link(&mut world, 3);
    assert_eq!(world.resource::<SelectedPerson>().0, None);
    world.entity_mut(stranger).insert(Interaction::None);
    let hidden = world
        .spawn(Node {
            display: Display::None,
            ..default()
        })
        .id();
    world.spawn((
        PersonLink(PersonId(1)),
        Interaction::Pressed,
        Node::default(),
        ChildOf(hidden),
    ));
    world.run_system_once(handle_links).unwrap();
    assert_eq!(world.resource::<SelectedPerson>().0, None);
    assert_eq!(
        *world.resource::<EncyclopediaTab>(),
        EncyclopediaTab::Companies
    );
}

#[test]
fn workplace_roster_uses_employment_ids_deduplicates_replication_and_reuses_unchanged_rows() {
    let mut world = world();
    world.init_resource::<places::KnownPlaces>();
    world.init_resource::<Time>();
    world.init_resource::<crate::ui::perf::UiPerf>();
    world
        .run_system_once(|mut commands: Commands| {
            commands
                .spawn(Node::default())
                .with_children(|root| workplaces::spawn_site_people(root, BuildingId(11)));
        })
        .unwrap();
    world.spawn((
        PersonId(1),
        CharacterName("Ada".into()),
        EmployedAt(BuildingId(11)),
    ));
    world.spawn((
        PersonId(1),
        CharacterName("Zara".into()),
        EmployedAt(BuildingId(11)),
    ));
    world.spawn((
        PersonId(2),
        CharacterName("Ada".into()),
        EmployedAt(BuildingId(11)),
    ));
    world.spawn((
        PersonId(3),
        CharacterName("Stranger".into()),
        EmployedAt(BuildingId(11)),
    ));
    world.spawn((
        PersonId(4),
        CharacterName("Hero".into()),
        EmployedAt(BuildingId(12)),
    ));
    let mut schedule = Schedule::default();
    schedule.add_systems(workplaces::sync_rosters);
    schedule.run(&mut world);
    let mut rows: Vec<_> = world
        .query::<(Entity, &PersonLink)>()
        .iter(&world)
        .map(|(entity, link)| (link.0, entity))
        .collect();
    rows.sort_by_key(|row| row.0);
    assert_eq!(
        rows.iter().map(|row| row.0).collect::<Vec<_>>(),
        [PersonId(1), PersonId(2)]
    );
    world
        .resource_mut::<Time>()
        .advance_by(std::time::Duration::from_secs(1));
    schedule.run(&mut world);
    let mut after: Vec<_> = world
        .query::<(Entity, &PersonLink)>()
        .iter(&world)
        .map(|(entity, link)| (link.0, entity))
        .collect();
    after.sort_by_key(|row| row.0);
    assert_eq!(rows, after);
}

/// A company site card must carry its WORKERS rows on the frame it is
/// spawned. `roster_systems` is ordered after `rebuild_company_view` so the
/// roster fill sees the new host in the same frame; with the old ordering the
/// card appeared one frame short and grew a frame later, twice a second.
#[test]
fn site_card_roster_is_present_on_the_same_frame_as_the_card() {
    use companies::{
        CompanyDetailContent, CompanyDetailViewport, CompanyDirectory, CompanyFilter,
        CompanyHolderRecord, CompanyListContent, CompanyPolicyFeedback, CompanyPortfolioContent,
        CompanyRecord, CompanySiteRecord, SelectedCompany, TradeRouteEditorState,
    };
    use shared::components::SettlementBuildingKind;
    use shared::economy::{BusinessState, CompanyAccount, CompanyManagementPolicy, Good};

    let owner = PersonId(4);
    let site = CompanySiteRecord {
        entity: Entity::from_bits(111),
        id: BuildingId(11),
        settlement: "Oakfell".into(),
        settlement_id: SettlementId(1),
        kind: SettlementBuildingKind::Windmill,
        workers: 1,
        positions: 2,
        enabled_positions: 2,
        state: BusinessState::Operating,
        wage_arrears: 0,
        tax_arrears: 0,
        current_day: default(),
        previous_day: default(),
        output: Some(Good::Flour),
        output_stock: 0,
        asking_price: Some(100),
        input: Some(Good::Wheat),
        input_stock: 0,
        input_target: 0,
        input_coverage_days: 1,
        sourcing: None,
        preferred_supplier: None,
        goods: Vec::new(),
        used_bulk: 0,
        bulk_capacity: 0,
    };
    let company = CompanyRecord {
        id: CompanyId(7),
        name: "Mill & Co".into(),
        founded_day: 1,
        master: owner,
        master_name: "Hero".into(),
        account: CompanyAccount::default(),
        policy: CompanyManagementPolicy::default(),
        capacity: None,
        ownership: shared::components::CompanyOwnership::sole(owner),
        holders: vec![CompanyHolderRecord {
            person: owner,
            name: "Hero".into(),
            shares: 1_000,
        }],
        offers: Vec::new(),
        decisions: Vec::new(),
        sites: vec![site],
        branches: Vec::new(),
        routes: Vec::new(),
        fleet: default(),
    };

    let mut app = App::new();
    app.insert_resource(CompanyDirectory {
        records: vec![company],
        settlements: Vec::new(),
        local_person: Some(owner),
        local_wallet: Some(10),
    })
    .init_resource::<CompanyFilter>()
    .init_resource::<companies::CompanySort>()
    .init_resource::<search::EncyclopediaSearch>()
    .insert_resource(SelectedCompany(Some(CompanyId(7))))
    .init_resource::<CompanyPolicyFeedback>()
    .init_resource::<TradeRouteEditorState>()
    .init_resource::<crate::ui::perf::UiPerf>()
    .init_resource::<places::KnownPlaces>()
    .init_resource::<places::SelectedPlace>()
    .init_resource::<places::SelectedPlaceEntry>()
    .init_resource::<crate::ui::hud::GodCapability>()
    .init_resource::<PersonLinkReturn>()
    .init_resource::<Time>()
    .insert_resource(KnownPeople {
        records: vec![
            person(1, "Ada", true, false),
            person(4, "Hero", false, true),
        ],
        requested: false,
    });
    app.add_systems(Update, (companies::rebuild_company_view, roster_systems()));
    app.world_mut()
        .spawn((CompanyPortfolioContent, Node::default()));
    app.world_mut().spawn((CompanyListContent, Node::default()));
    let viewport = app
        .world_mut()
        .spawn((
            CompanyDetailViewport,
            Node::default(),
            ScrollPosition::default(),
        ))
        .id();
    let detail = app
        .world_mut()
        .spawn((CompanyDetailContent, Node::default(), ChildOf(viewport)))
        .id();
    app.world_mut().spawn((
        PersonId(1),
        CharacterName("Ada".into()),
        EmployedAt(BuildingId(11)),
    ));

    // ONE frame: the card and its roster must both exist afterwards.
    app.update();
    let world = app.world_mut();
    let (link, host_display) = {
        let mut links = world.query::<(Entity, &PersonLink, &ChildOf)>();
        let (link, _, parent) = links
            .iter(world)
            .find(|(_, link, _)| link.0 == PersonId(1))
            .expect("the worker row is spawned in the same frame as the site card");
        let host = parent.parent();
        (link, world.get::<Node>(host).unwrap().display)
    };
    assert_eq!(
        host_display,
        Display::Flex,
        "the roster host is shown, not hidden"
    );
    let mut cursor = link;
    let mut under_detail = false;
    while let Some(parent) = world.get::<ChildOf>(cursor) {
        cursor = parent.parent();
        if cursor == detail {
            under_detail = true;
            break;
        }
    }
    assert!(under_detail, "the roster row lives inside the detail pane");
}
