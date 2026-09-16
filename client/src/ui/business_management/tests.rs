use super::*;

#[test]
fn coverage_choices_use_human_days_instead_of_raw_unit_thresholds() {
    assert_eq!(coverage_label(0), "OFF");
    assert_eq!(coverage_label(1), "1 DAY");
    assert_eq!(coverage_label(5), "5 DAYS");
}

#[test]
fn sourcing_modes_have_plain_language_labels() {
    assert_eq!(
        sourcing_short_label(BusinessSourcingMode::PreferOwned),
        "COMPANY FIRST"
    );
    assert_eq!(
        sourcing_short_label(BusinessSourcingMode::CheapestAvailable),
        "BEST VALUE"
    );
    assert_eq!(
        sourcing_short_label(BusinessSourcingMode::OwnedOnly),
        "COMPANY ONLY"
    );
}

fn sample_model(wage: u64, selected_positions: u8) -> ControlsModel {
    sample_model_with_owner(
        wage,
        selected_positions,
        None,
        Some(PersonId(1)),
        Some(PersonId(1)),
    )
}

fn sample_model_with_owner(
    wage: u64,
    selected_positions: u8,
    company: Option<CompanyId>,
    owner: Option<PersonId>,
    local_person: Option<PersonId>,
) -> ControlsModel {
    let building = SettlementBuilding {
        kind: SettlementBuildingKind::Bakery,
        settlement: "Brackwater".into(),
        owner: None,
        quality: 1.0,
        workers: vec![],
    };
    let staffing = BusinessStaffingPolicy::new(selected_positions);
    let feedback = BusinessFeedback::default();
    let draft = ShareOrderDraft::default();
    let name_of = |person: PersonId| format!("Person #{}", person.0);
    controls_model(&ModelInputs {
        workers: &[],
        site: Some(SiteView {
            building: &building,
            building_id: BuildingId(7),
            company,
            owner,
            account: &BusinessAccount::default(),
            management: &BusinessManagementPolicy::default(),
            wage: &BusinessWagePolicy {
                daily_wage: wage,
                ..default()
            },
            sale: &BusinessSalePolicy::default(),
            staffing: Some(&staffing),
            procurement: &BusinessProcurementPolicy::default(),
            supply: &BusinessSupplyPolicy::default(),
            inventory: &GoodsInventory::default(),
            tavern_service: None,
        }),
        company: None,
        local_person,
        share_draft: &draft,
        dividend_draft: &DividendDraft::default(),
        page: BusinessManagementPage::Site,
        feedback: &feedback,
        name_of: &name_of,
    })
}

#[test]
fn value_changes_keep_the_structure_key_so_the_panel_binds_in_place() {
    let before = sample_model(500, 1);
    let after = sample_model(525, 2);
    assert_eq!(before.structure_key(), after.structure_key());
    let wage_value = |model: &ControlsModel| {
        model
            .blocks
            .iter()
            .find_map(|block| match block {
                Block::Row(row) if row.id == "wage" => Some(row.value.clone()),
                _ => None,
            })
            .unwrap()
    };
    assert_ne!(wage_value(&before), wage_value(&after));
}

fn bound_ids(model: &ControlsModel) -> Vec<String> {
    let mut ids = vec!["title".to_string(), "subtitle".into(), "feedback".into()];
    for block in &model.blocks {
        match block {
            Block::Row(row) => {
                ids.push(row.id.clone());
                ids.extend(row.controls.iter().map(|control| control.id.clone()));
            }
            Block::Meter(meter) => ids.push(meter.id.clone()),
            Block::Section(_) | Block::Scope(_) => {}
        }
    }
    ids
}

#[test]
fn control_ids_are_unique_within_a_model() {
    // Vacant worker slots are ids too: the sample site has no observed
    // employees, so both Bakery slots are present and must stay distinct.
    for model in [
        sample_model(500, 1),
        ScopedFixture::new(BusinessManagementPage::Site, true, true).model(),
        ScopedFixture::new(BusinessManagementPage::Site, true, true)
            .offers(&[(PersonId(2), 50)])
            .model(),
    ] {
        let ids = bound_ids(&model);
        assert!(
            ids.iter().filter(|id| id.starts_with("worker.")).count() >= 2,
            "fixed worker slots missing: {ids:?}"
        );
        let mut deduped = ids.clone();
        deduped.sort();
        deduped.dedup();
        assert_eq!(ids.len(), deduped.len(), "duplicate bound ids: {ids:?}");
        let hashed: std::collections::HashSet<_> = ids.iter().map(|id| BoundId::of(id)).collect();
        assert_eq!(ids.len(), hashed.len(), "bound id hash collision: {ids:?}");
    }
}

/// A company with a Bakery site; `workers`, the cap table and the public
/// share board are the volatile inputs the structure key must ignore.
struct ScopedFixture {
    page: BusinessManagementPage,
    with_site: bool,
    manager: bool,
    workers: Vec<PersonId>,
    ownership: CompanyOwnership,
    share_market: CompanyShareMarket,
    capacity: Option<CompanyDividendCapacity>,
    dividend_draft: DividendDraft,
}

/// A published headroom snapshot: `distributable` pennies on day 12, 18.50
/// coin of reserves, 200.00 coin last paid on day 11.
fn capacity(distributable: u64) -> CompanyDividendCapacity {
    CompanyDividendCapacity {
        day: 12,
        distributable,
        protected_reserves: 1_850,
        retained_profit: distributable.saturating_add(400),
        last_paid_day: 11,
        last_paid: 20_000,
    }
}

impl ScopedFixture {
    fn new(page: BusinessManagementPage, with_site: bool, manager: bool) -> Self {
        Self {
            page,
            with_site,
            manager,
            workers: vec![PersonId(9), PersonId(10)],
            ownership: CompanyOwnership::sole(PersonId(1)),
            share_market: CompanyShareMarket::default(),
            capacity: None,
            dividend_draft: DividendDraft::default(),
        }
    }

    fn workers(mut self, workers: &[PersonId]) -> Self {
        self.workers = workers.to_vec();
        self
    }

    fn capacity(mut self, capacity: Option<CompanyDividendCapacity>) -> Self {
        self.capacity = capacity;
        self
    }

    fn dividend_draft(mut self, pennies: u64) -> Self {
        self.dividend_draft = DividendDraft {
            company: Some(CompanyId(4)),
            pennies,
            edited: true,
        };
        self
    }

    /// PersonId(2) holds 100 shares and lists `(seller, shares)` offers.
    fn offers(mut self, offers: &[(PersonId, u16)]) -> Self {
        self.ownership = CompanyOwnership::from_shares(vec![
            shared::components::CompanyShare {
                shareholder: PersonId(1),
                shares: 900,
            },
            shared::components::CompanyShare {
                shareholder: PersonId(2),
                shares: 100,
            },
        ])
        .unwrap();
        self.share_market = CompanyShareMarket::default();
        for (seller, shares) in offers {
            assert!(
                self.share_market
                    .list(&self.ownership, *seller, *shares, 100, 3),
                "fixture offer {seller:?} x {shares} must be listable"
            );
        }
        self
    }

    fn model(&self) -> ControlsModel {
        scoped_model_from(self)
    }
}

fn scoped_model(page: BusinessManagementPage, with_site: bool, manager: bool) -> ControlsModel {
    ScopedFixture::new(page, with_site, manager).model()
}

/// Two observed workers share the name "Ada" so identity must come from ids.
fn fixture_name(person: PersonId) -> String {
    match person {
        PersonId(9) | PersonId(10) => "Ada".to_string(),
        person => format!("Person #{}", person.0),
    }
}

fn scoped_model_from(fixture: &ScopedFixture) -> ControlsModel {
    let ScopedFixture {
        page,
        with_site,
        manager,
        workers,
        ownership,
        share_market,
        capacity,
        dividend_draft,
    } = fixture;
    let (page, with_site, manager) = (*page, *with_site, *manager);
    let person = PersonId(1);
    let company = Company {
        name: "Brackwater Trading Company".into(),
        founded_day: 2,
    };
    let leadership = CompanyLeadership {
        master: if manager { person } else { PersonId(2) },
    };
    let company_account = CompanyAccount {
        cash: 7_500,
        ..default()
    };
    let company_policy = CompanyManagementPolicy {
        strategy: BusinessStrategy::Conservative,
        ..default()
    };
    let decisions = CompanyDecisionHistory::default();
    let building = SettlementBuilding {
        kind: SettlementBuildingKind::Bakery,
        settlement: "Brackwater".into(),
        owner: None,
        quality: 1.0,
        workers: vec![],
    };
    let account = BusinessAccount::default();
    let management = BusinessManagementPolicy {
        strategy: BusinessStrategy::Aggressive,
        autopilot: false,
        ..default()
    };
    let wage = BusinessWagePolicy::default();
    let sale = BusinessSalePolicy::default();
    let staffing = BusinessStaffingPolicy::new(1);
    let procurement = BusinessProcurementPolicy::default();
    let supply = BusinessSupplyPolicy::default();
    let inventory = GoodsInventory::default();
    controls_model(&ModelInputs {
        site: with_site.then_some(SiteView {
            building: &building,
            building_id: BuildingId(7),
            company: Some(CompanyId(4)),
            owner: Some(person),
            account: &account,
            management: &management,
            wage: &wage,
            sale: &sale,
            staffing: Some(&staffing),
            procurement: &procurement,
            supply: &supply,
            inventory: &inventory,
            tavern_service: None,
        }),
        workers,
        company: Some(CompanyView {
            id: CompanyId(4),
            company: &company,
            ownership,
            leadership: &leadership,
            account: &company_account,
            policy: &company_policy,
            decisions: &decisions,
            share_market,
            capacity: *capacity,
        }),
        local_person: Some(person),
        share_draft: &ShareOrderDraft::default(),
        dividend_draft,
        page,
        feedback: &BusinessFeedback::default(),
        name_of: &fixture_name,
    })
}

fn scoped_controls(model: &ControlsModel) -> Vec<(BusinessManagementPage, &ControlModel)> {
    let mut page = BusinessManagementPage::Site;
    let mut result = Vec::new();
    for block in &model.blocks {
        match block {
            Block::Scope(scope) => page = *scope,
            Block::Row(row) => result.extend(row.controls.iter().map(|control| (page, control))),
            _ => {}
        }
    }
    result
}

#[test]
fn company_management_needs_no_site_and_addresses_the_company_directly() {
    let model = scoped_model(BusinessManagementPage::Company, false, true);
    assert!(!model.site_available);
    assert_eq!(model.title, "Brackwater Trading Company");
    let controls = scoped_controls(&model);
    assert!(controls.iter().any(|(_, control)| matches!(
        control.press,
        ControlPress::Company(CompanyId(4), HeroCompanyAction::DistributeDividend { .. })
    )));
    assert!(controls
        .iter()
        .all(|(page, control)| *page == BusinessManagementPage::Company
            && !matches!(
                control.press,
                ControlPress::Order(_) | ControlPress::Person(_)
            )));
}

#[test]
fn company_and_site_policies_keep_distinct_scopes_and_manual_selections() {
    let model = scoped_model(BusinessManagementPage::Site, true, true);
    let controls = scoped_controls(&model);
    assert!(controls
        .iter()
        .any(|(page, control)| *page == BusinessManagementPage::Site
            && control.selected
            && matches!(
                control.press,
                ControlPress::Order(HeroBusinessAction::SetStrategy(
                    BusinessStrategy::Aggressive
                ))
            )));
    assert!(controls
        .iter()
        .any(|(page, control)| *page == BusinessManagementPage::Company
            && control.selected
            && matches!(
                control.press,
                ControlPress::Company(
                    CompanyId(4),
                    HeroCompanyAction::SetStrategy(BusinessStrategy::Conservative)
                )
            )));
    for (page, control) in &controls {
        match control.press {
            ControlPress::Order(_) | ControlPress::Person(_) => {
                assert_eq!(*page, BusinessManagementPage::Site)
            }
            ControlPress::Company(id, _) => {
                assert_eq!(id, CompanyId(4));
                assert_eq!(*page, BusinessManagementPage::Company);
            }
            ControlPress::Draft(_) | ControlPress::DividendDraft(_) => {
                assert_eq!(*page, BusinessManagementPage::Company)
            }
            ControlPress::Vacant(slot) => panic!("unexpected vacant {slot:?} slot {}", control.id),
        }
    }
    let ids: Vec<_> = controls
        .iter()
        .map(|(_, control)| control.id.as_str())
        .collect();
    let unique: std::collections::HashSet<_> = ids.iter().copied().collect();
    assert_eq!(ids.len(), unique.len());
    let linked: Vec<_> = controls
        .iter()
        .filter_map(|(_, control)| {
            if let ControlPress::Person(person) = control.press {
                Some((person.0, control.label.as_str()))
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        linked,
        vec![(9, "Ada"), (10, "Ada")],
        "identical names must retain different person identities"
    );
    // A Bakery has two positions and both are observed: no vacant slot.
    assert!(controls.iter().all(|(_, control)| control.visible()));
}

#[test]
fn shareholders_without_executive_authority_do_not_get_operating_controls() {
    let model = scoped_model(BusinessManagementPage::Company, true, false);
    let controls = scoped_controls(&model);
    assert!(!controls.iter().any(|(_, control)| matches!(
        control.press,
        ControlPress::Order(_)
            | ControlPress::Company(
                _,
                HeroCompanyAction::SetStrategy(_)
                    | HeroCompanyAction::SetAutopilot(_)
                    | HeroCompanyAction::SetAutomaticDividends(_)
                    | HeroCompanyAction::DistributeDividend { .. }
                    | HeroCompanyAction::ContributeCapital { .. }
            )
    )));
    assert!(controls.iter().any(|(_, control)| matches!(
        control.press,
        ControlPress::Company(_, HeroCompanyAction::ListCompanyShares { .. })
    )));
}

#[test]
fn switching_scopes_retains_control_entities_and_each_scroll_position() {
    use bevy::ecs::{system::SystemState, world::CommandQueue};
    let before = scoped_model(BusinessManagementPage::Site, true, true);
    let after = scoped_model(BusinessManagementPage::Company, true, true);
    assert_eq!(before.structure_key(), after.structure_key());
    let mut world = World::new();
    let host = world.spawn(Node::default()).id();
    let mut queue = CommandQueue::default();
    spawn_panel(
        &mut Commands::new(&mut queue, &world),
        host,
        BusinessManagementSelection::Site(Entity::PLACEHOLDER),
        before.structure_key(),
        &before,
        [Vec2::new(0.0, 120.0), Vec2::new(0.0, 240.0)],
    );
    queue.apply(&mut world);
    let mut buttons = world.query::<(Entity, &BoundButton)>();
    let ids: Vec<_> = buttons
        .iter(&world)
        .map(|(entity, marker)| (entity, marker.0))
        .collect();
    assert!(!ids.is_empty());
    let mut state = SystemState::<BoundControls>::new(&mut world);
    let mut slots = HashMap::default();
    let fixed = PanelScratch::default().fixed;
    bind_panel(
        &after,
        &mut state.get_mut(&mut world).unwrap(),
        &mut slots,
        fixed,
    );
    assert_eq!(
        ids,
        buttons
            .iter(&world)
            .map(|(entity, marker)| (entity, marker.0))
            .collect::<Vec<_>>()
    );
    let mut pages = world.query::<(&PageBody, &Node, &ScrollPosition)>();
    assert_eq!(pages.iter(&world).count(), 2);
    for (page, node, scroll) in pages.iter(&world) {
        assert_eq!(
            node.display,
            page_display(page.0, BusinessManagementPage::Company)
        );
        assert_eq!(
            scroll.0.y,
            if page.0 == BusinessManagementPage::Site {
                120.0
            } else {
                240.0
            }
        );
    }
}

#[test]
fn missing_company_record_does_not_turn_its_site_into_independently_owned_controls() {
    let model = sample_model_with_owner(
        100,
        1,
        Some(CompanyId(4)),
        Some(PersonId(1)),
        Some(PersonId(1)),
    );
    assert!(model.subtitle.contains("COMPANY RECORD UNAVAILABLE"));
    assert!(!model.subtitle.contains("INDEPENDENT"));
    assert!(!scoped_controls(&model)
        .iter()
        .any(|(_, control)| matches!(control.press, ControlPress::Order(_))));
    assert!(
        model
            .blocks
            .iter()
            .any(|block| matches!(block, Block::Row(row) if row.id == "site.today")),
        "missing authority must leave the site inspectable"
    );
}

#[test]
fn independent_site_controls_require_the_assigned_owner_identity() {
    for (owner, local, allowed) in [
        (Some(PersonId(1)), Some(PersonId(1)), true),
        (Some(PersonId(2)), Some(PersonId(1)), false),
        (None, Some(PersonId(1)), false),
        (Some(PersonId(1)), None, false),
        (
            Some(PersonId::UNASSIGNED),
            Some(PersonId::UNASSIGNED),
            false,
        ),
    ] {
        let model = sample_model_with_owner(100, 1, None, owner, local);
        let has_controls = scoped_controls(&model)
            .iter()
            .any(|(_, control)| matches!(control.press, ControlPress::Order(_)));
        assert_eq!(has_controls, allowed, "owner {owner:?}, local {local:?}");
    }
}

// --- fixed slots and bind-in-place ------------------------------------------

/// Spawn `model` into a fresh world and return it with the host.
fn spawned_world(model: &ControlsModel) -> World {
    use bevy::ecs::world::CommandQueue;
    let mut world = World::new();
    let host = world.spawn(Node::default()).id();
    let mut queue = CommandQueue::default();
    spawn_panel(
        &mut Commands::new(&mut queue, &world),
        host,
        BusinessManagementSelection::Site(Entity::PLACEHOLDER),
        model.structure_key(),
        model,
        [Vec2::ZERO; 2],
    );
    queue.apply(&mut world);
    world
}

fn bind(world: &mut World, model: &ControlsModel) {
    use bevy::ecs::system::SystemState;
    let mut state = SystemState::<BoundControls>::new(world);
    let mut slots = HashMap::default();
    let fixed = PanelScratch::default().fixed;
    bind_panel(model, &mut state.get_mut(world).unwrap(), &mut slots, fixed);
}

/// `(entity, PersonLink, display)` of every button bound to `id`.
fn slot_state(world: &mut World, id: &str) -> Vec<(Entity, Option<PersonId>, Display)> {
    let key = BoundId::of(id);
    world
        .query::<(Entity, &BoundButton, Option<&PersonLink>, &Node)>()
        .iter(world)
        .filter(|(_, marker, ..)| marker.0 == key)
        .map(|(entity, _, link, node)| (entity, link.map(|link| link.0), node.display))
        .collect()
}

fn control<'a>(model: &'a ControlsModel, id: &str) -> &'a ControlModel {
    scoped_controls(model)
        .into_iter()
        .map(|(_, control)| control)
        .find(|control| control.id == id)
        .unwrap_or_else(|| panic!("control {id} missing"))
}

fn row_value(model: &ControlsModel, id: &str) -> String {
    model
        .blocks
        .iter()
        .find_map(|block| match block {
            Block::Row(row) if row.id == id => Some(row.value.clone()),
            _ => None,
        })
        .unwrap_or_else(|| panic!("row {id} missing"))
}

#[test]
fn observed_worker_churn_keeps_the_structure_key() {
    let fixture = ScopedFixture::new(BusinessManagementPage::Site, true, true);
    let two = fixture.model();
    let one = ScopedFixture::new(BusinessManagementPage::Site, true, true)
        .workers(&[PersonId(9)])
        .model();
    let none = ScopedFixture::new(BusinessManagementPage::Site, true, true)
        .workers(&[])
        .model();
    assert_eq!(two.structure_key(), one.structure_key());
    assert_eq!(one.structure_key(), none.structure_key());
    // The chips are still there as values: slot 1 empties, slot 0 keeps Ada.
    assert_eq!(
        control(&two, "worker.1").press,
        ControlPress::Person(PersonId(10))
    );
    assert_eq!(
        control(&one, "worker.1").press,
        ControlPress::Vacant(VacantSlot::Person)
    );
    assert!(!control(&one, "worker.1").visible());
    assert_eq!(control(&one, "worker.0").label, "Ada");
    assert!(row_value(&none, "workers").starts_with("No observed employees"));
    assert!(row_value(&two, "workers").starts_with("2 observed employees"));
}

#[test]
fn duplicate_bodies_for_one_person_yield_one_chip() {
    let site = BuildingId(7);
    let mut scratch = vec![PersonId(99)];
    observed_workers(
        &mut scratch,
        site,
        [
            (PersonId(10), Some(site)),
            (PersonId(9), Some(site)),
            // A durable person in replication handover: two bodies, one id.
            (PersonId(9), Some(site)),
            (PersonId(11), Some(BuildingId(8))),
            (PersonId(12), None),
        ]
        .into_iter(),
    );
    assert_eq!(scratch, vec![PersonId(9), PersonId(10)]);
    let model = ScopedFixture::new(BusinessManagementPage::Site, true, true)
        .workers(&scratch)
        .model();
    let chips: Vec<_> = scoped_controls(&model)
        .into_iter()
        .filter_map(|(_, control)| match control.press {
            ControlPress::Person(person) => Some(person),
            _ => None,
        })
        .collect();
    assert_eq!(chips, vec![PersonId(9), PersonId(10)]);
    // More observed bodies than positions never grows the slot set.
    let crowded = ScopedFixture::new(BusinessManagementPage::Site, true, true)
        .workers(&[PersonId(9), PersonId(10), PersonId(11)])
        .model();
    assert_eq!(crowded.structure_key(), model.structure_key());
    assert!(
        bound_ids(&crowded)
            .iter()
            .filter(|id| id.starts_with("worker."))
            .count()
            == 2
    );
}

#[test]
fn partial_share_purchase_keeps_public_offer_structure() {
    let fifty = ScopedFixture::new(BusinessManagementPage::Company, true, true)
        .offers(&[(PersonId(2), 50)])
        .model();
    let forty = ScopedFixture::new(BusinessManagementPage::Company, true, true)
        .offers(&[(PersonId(2), 40)])
        .model();
    let five = ScopedFixture::new(BusinessManagementPage::Company, true, true)
        .offers(&[(PersonId(2), 5)])
        .model();
    assert_eq!(fifty.structure_key(), forty.structure_key());
    assert_eq!(forty.structure_key(), five.structure_key());
    let buy = |model: &ControlsModel, slot: usize| -> ControlPress {
        control(model, &format!("buy.2.{slot}")).press
    };
    let order = |shares: u16| {
        ControlPress::Company(
            CompanyId(4),
            HeroCompanyAction::BuyCompanyShares {
                seller: PersonId(2),
                shares,
            },
        )
    };
    assert_eq!(buy(&fifty, 2), order(50));
    assert_eq!(buy(&forty, 2), order(40));
    assert_eq!(control(&forty, "buy.2.2").label, "BUY 40 FROM PERSON #2");
    // BUY 1 / BUY 5: the BUY 10 slot collapses onto BUY ALL and stays vacant.
    assert_eq!(buy(&five, 0), order(1));
    assert_eq!(buy(&five, 1), order(5));
    assert_eq!(buy(&five, 2), ControlPress::Vacant(VacantSlot::Action));
    // A new seller is genuinely structural.
    let two_sellers = ScopedFixture::new(BusinessManagementPage::Company, true, true)
        .offers(&[(PersonId(2), 50), (PersonId(1), 20)])
        .model();
    assert_ne!(two_sellers.structure_key(), fifty.structure_key());
}

#[test]
fn person_links_rebind_by_id() {
    let before = ScopedFixture::new(BusinessManagementPage::Site, true, true).model();
    let mut world = spawned_world(&before);
    let slot0 = slot_state(&mut world, "worker.0");
    let slot1 = slot_state(&mut world, "worker.1");
    assert_eq!(slot0.len(), 1);
    assert_eq!(slot1.len(), 1);
    assert_eq!(slot0[0].1, Some(PersonId(9)));
    assert_eq!(slot1[0].1, Some(PersonId(10)));
    assert_eq!(slot1[0].2, Display::Flex);
    let drafts_before: Vec<_> = world
        .query::<(Entity, &ShareDraftAction)>()
        .iter(&world)
        .map(|(entity, step)| (entity, *step))
        .collect();
    assert_eq!(drafts_before.len(), 4);

    // Ada(9) leaves interest; Bea(11) is observed: slot 0 now links 11 and
    // slot 1 empties, on the same entities.
    let after = ScopedFixture::new(BusinessManagementPage::Site, true, true)
        .workers(&[PersonId(11)])
        .model();
    assert_eq!(after.structure_key(), before.structure_key());
    bind(&mut world, &after);
    let rebound0 = slot_state(&mut world, "worker.0");
    let rebound1 = slot_state(&mut world, "worker.1");
    assert_eq!(
        rebound0,
        vec![(slot0[0].0, Some(PersonId(11)), Display::Flex)]
    );
    assert_eq!(
        rebound1,
        vec![(slot1[0].0, Some(PersonId::UNASSIGNED), Display::None)]
    );
    let label = |world: &mut World, id: &str| -> String {
        let key = BoundId::of(id);
        world
            .query::<(&BoundText, &Text)>()
            .iter(world)
            .find(|(marker, _)| marker.0 == key)
            .map(|(_, text)| text.0.clone())
            .unwrap()
    };
    assert_eq!(label(&mut world, "worker.0"), "Person #11");
    assert_eq!(label(&mut world, "worker.1"), "");
    let drafts_after: Vec<_> = world
        .query::<(Entity, &ShareDraftAction)>()
        .iter(&world)
        .map(|(entity, step)| (entity, *step))
        .collect();
    assert_eq!(drafts_after, drafts_before);

    // And back: a returning worker fills the vacant slot in place.
    bind(&mut world, &before);
    assert_eq!(slot_state(&mut world, "worker.1"), slot1);
    assert_eq!(label(&mut world, "worker.1"), "Ada");
}

/// App-level twin: the real `ensure_panel` against replicated components.
fn management_app() -> (App, Entity) {
    use crate::ui::encyclopedia::{
        EncyclopediaOpen, EncyclopediaPageHost, EncyclopediaTab, KnownPeople,
    };
    let mut app = App::new();
    let person = PersonId(1);
    app.init_resource::<BusinessManagementTarget>()
        .init_resource::<BusinessManagementReturn>()
        .init_resource::<BusinessManagementPage>()
        .init_resource::<BusinessFeedback>()
        .init_resource::<ShareOrderDraft>()
        .init_resource::<DividendDraft>()
        .init_resource::<EncyclopediaOpen>()
        .init_resource::<EncyclopediaTab>()
        .init_resource::<KnownPeople>()
        .init_resource::<crate::ui::perf::UiPerf>()
        .insert_resource(crate::camera_rts::LocalPeerId(42))
        .add_systems(Update, ensure_panel);
    app.world_mut()
        .spawn((EncyclopediaPageHost, Node::default()));
    app.world_mut().spawn((
        Hero {
            owner: lightyear::prelude::PeerId::Netcode(42),
        },
        person,
    ));
    app.world_mut()
        .spawn((person, CharacterName("Aldric".into())));
    app.world_mut()
        .spawn((PersonId(2), CharacterName("Bea".into())));
    let mut ownership = CompanyOwnership::sole(person);
    assert!(ownership.transfer(person, PersonId(2), 100));
    let mut share_market = CompanyShareMarket::default();
    assert!(share_market.list(&ownership, PersonId(2), 50, 100, 3));
    let company = app
        .world_mut()
        .spawn((
            CompanyId(4),
            Company {
                name: "Brackwater Trading Company".into(),
                founded_day: 2,
            },
            ownership,
            CompanyLeadership { master: person },
            CompanyAccount {
                cash: 7_500,
                ..default()
            },
            CompanyManagementPolicy::default(),
            CompanyDecisionHistory::default(),
            share_market,
            capacity(30_000),
        ))
        .id();
    app.world_mut().resource_mut::<BusinessManagementTarget>().0 =
        Some(BusinessManagementSelection::Company(CompanyId(4)));
    app.update();
    app.update();
    (app, company)
}

fn panel_entities(world: &mut World) -> (Entity, u64, Vec<(Entity, BoundId)>) {
    let (root, state) = world.query::<(Entity, &Root)>().single(world).unwrap();
    let structure = state.structure;
    let mut buttons: Vec<_> = world
        .query::<(Entity, &BoundButton)>()
        .iter(world)
        .map(|(entity, marker)| (entity, marker.0))
        .collect();
    buttons.sort_by_key(|(entity, _)| *entity);
    (root, structure, buttons)
}

fn bound_text(world: &mut World, id: &str) -> String {
    let key = BoundId::of(id);
    world
        .query::<(&BoundText, &Text)>()
        .iter(world)
        .find(|(marker, _)| marker.0 == key)
        .map(|(_, text)| text.0.clone())
        .unwrap_or_else(|| panic!("bound text {id} missing"))
}

#[test]
fn company_cash_tick_keeps_root_and_control_entities() {
    let (mut app, company) = management_app();
    let (root, structure, buttons) = panel_entities(app.world_mut());
    assert!(!buttons.is_empty());
    assert!(bound_text(app.world_mut(), "company.treasury").starts_with("75.00 coin"));
    let buy_all = bound_text(app.world_mut(), "buy.2.2");
    assert_eq!(buy_all, "BUY 50 FROM BEA");

    // An economic tick and a partial purchase of the public offer.
    {
        let mut world = app.world_mut().entity_mut(company);
        world.get_mut::<CompanyAccount>().unwrap().cash = 9_925;
        let mut ownership = world.get::<CompanyOwnership>().unwrap().clone();
        let mut market = world.get::<CompanyShareMarket>().unwrap().clone();
        assert!(market.fill(&mut ownership, PersonId(2), PersonId(1), 10));
        *world.get_mut::<CompanyOwnership>().unwrap() = ownership;
        *world.get_mut::<CompanyShareMarket>().unwrap() = market;
    }
    app.update();
    let (root_after, structure_after, buttons_after) = panel_entities(app.world_mut());
    assert_eq!(root_after, root, "the page respawned on a value change");
    assert_eq!(structure_after, structure);
    assert_eq!(buttons_after, buttons);
    assert!(bound_text(app.world_mut(), "company.treasury").starts_with("99.25 coin"));
    assert_eq!(bound_text(app.world_mut(), "buy.2.2"), "BUY 40 FROM BEA");
    let buy_all_action = {
        let key = BoundId::of("buy.2.2");
        app.world_mut()
            .query::<(&BoundButton, &Action)>()
            .iter(app.world())
            .find(|(marker, _)| marker.0 == key)
            .map(|(_, action)| action.0)
            .unwrap()
    };
    assert_eq!(
        buy_all_action,
        ControlPress::Company(
            CompanyId(4),
            HeroCompanyAction::BuyCompanyShares {
                seller: PersonId(2),
                shares: 40,
            }
        )
    );

    // Buying out the offer removes the seller: that IS structural.
    {
        let mut world = app.world_mut().entity_mut(company);
        let mut ownership = world.get::<CompanyOwnership>().unwrap().clone();
        let mut market = world.get::<CompanyShareMarket>().unwrap().clone();
        assert!(market.fill(&mut ownership, PersonId(2), PersonId(1), 40));
        *world.get_mut::<CompanyOwnership>().unwrap() = ownership;
        *world.get_mut::<CompanyShareMarket>().unwrap() = market;
    }
    app.update();
    app.update();
    let (root_final, structure_final, _) = panel_entities(app.world_mut());
    assert_ne!(structure_final, structure);
    assert_ne!(root_final, root);
}

// --- dividend amount picker ---------------------------------------------------

fn confirm_payload(model: &ControlsModel) -> ControlPress {
    control(model, "company.dividends.distribute").press
}

#[test]
fn dividend_controls_step_within_capacity_and_send_the_drafted_amount() {
    let distributable = 30_000;
    let mut draft = DividendDraft::all_of(CompanyId(4), 0);
    draft.step(DividendDraftAction::All, distributable);
    assert_eq!(draft.pennies, 30_000);
    draft.step(DividendDraftAction::Up, distributable);
    assert_eq!(draft.pennies, 30_000, "+1 coin never exceeds the headroom");
    draft.step(DividendDraftAction::Down, distributable);
    assert_eq!(draft.pennies, 29_900);
    draft.step(DividendDraftAction::Quarter, distributable);
    assert_eq!(draft.pennies, 7_500);
    draft.step(DividendDraftAction::Half, distributable);
    assert_eq!(draft.pennies, 15_000);
    draft.step(DividendDraftAction::Up, distributable);
    assert_eq!(draft.pennies, 15_100);
    for _ in 0..200 {
        draft.step(DividendDraftAction::Down, distributable);
    }
    assert_eq!(draft.pennies, 0, "-1 coin stops at zero");
    // A snapshot that shrank below the draft pulls the draft down first.
    draft.pennies = 50_000;
    draft.step(DividendDraftAction::Up, distributable);
    assert_eq!(draft.pennies, 30_000);

    let fixture = |pennies: u64| {
        ScopedFixture::new(BusinessManagementPage::Company, false, true)
            .capacity(Some(capacity(distributable)))
            .dividend_draft(pennies)
            .model()
    };
    let half = fixture(15_000);
    assert_eq!(
        confirm_payload(&half),
        ControlPress::Company(
            CompanyId(4),
            HeroCompanyAction::DistributeDividend { pennies: 15_000 }
        )
    );
    assert_eq!(
        control(&half, "company.dividends.distribute").label,
        "DISTRIBUTE 150.00 COIN"
    );
    for (id, step) in [
        ("company.dividends.down", DividendDraftAction::Down),
        ("company.dividends.up", DividendDraftAction::Up),
        ("company.dividends.quarter", DividendDraftAction::Quarter),
        ("company.dividends.half", DividendDraftAction::Half),
        ("company.dividends.all", DividendDraftAction::All),
    ] {
        assert_eq!(control(&half, id).press, ControlPress::DividendDraft(step));
    }
    let value = row_value(&half, "company.dividends");
    assert!(
        value.contains("Available now 300.00 coin (day 12)")
            && value.contains("reserves 18.50 coin")
            && value.contains("last paid 200.00 coin on day 11"),
        "{value}"
    );

    // An oversized draft is clamped to the published headroom before it is
    // sent; the server clamps again against its live figure.
    let oversized = fixture(999_999);
    assert_eq!(
        confirm_payload(&oversized),
        ControlPress::Company(
            CompanyId(4),
            HeroCompanyAction::DistributeDividend {
                pennies: distributable
            }
        )
    );
    assert_eq!(half.structure_key(), oversized.structure_key());

    // Non-managers see the headroom but no picker or confirm control.
    let holder = ScopedFixture::new(BusinessManagementPage::Company, false, false)
        .capacity(Some(capacity(distributable)))
        .model();
    assert!(row_value(&holder, "company.dividends").contains("Available now 300.00 coin"));
    assert!(
        !scoped_controls(&holder).iter().any(|(_, control)| matches!(
            control.press,
            ControlPress::DividendDraft(_)
                | ControlPress::Company(_, HeroCompanyAction::DistributeDividend { .. })
        ))
    );
}

#[test]
fn confirm_control_stays_present_when_nothing_is_distributable() {
    let funded = ScopedFixture::new(BusinessManagementPage::Company, false, true)
        .capacity(Some(capacity(30_000)))
        .dividend_draft(30_000)
        .model();
    let empty = ScopedFixture::new(BusinessManagementPage::Company, false, true)
        .capacity(Some(CompanyDividendCapacity {
            distributable: 0,
            last_paid_day: u32::MAX,
            last_paid: 0,
            ..capacity(0)
        }))
        .dividend_draft(30_000)
        .model();
    let unpublished = ScopedFixture::new(BusinessManagementPage::Company, false, true)
        .dividend_draft(30_000)
        .model();
    // Capacity crossing zero (or not having arrived yet) is a value change:
    // the confirm control keeps its slot with a clamped payload.
    assert_eq!(funded.structure_key(), empty.structure_key());
    assert_eq!(empty.structure_key(), unpublished.structure_key());
    for model in [&empty, &unpublished] {
        assert_eq!(
            confirm_payload(model),
            ControlPress::Company(
                CompanyId(4),
                HeroCompanyAction::DistributeDividend { pennies: 0 }
            )
        );
        assert_eq!(
            control(model, "company.dividends.distribute").label,
            "DISTRIBUTE 0.00 COIN"
        );
    }
    let value = row_value(&empty, "company.dividends");
    assert!(
        value.contains("Available now 0.00 coin (day 12)") && value.contains("never paid"),
        "{value}"
    );
    assert!(
        row_value(&unpublished, "company.dividends").contains("awaiting the first finance review"),
        "{}",
        row_value(&unpublished, "company.dividends")
    );
}

#[test]
fn dividend_preview_matches_shared_split_for_the_local_holder() {
    // 900 / 100 cap table; 1,001 pennies leaves a one-penny remainder that
    // the shared split gives to the first cap-table entry (PersonId 1).
    let fixture = ScopedFixture::new(BusinessManagementPage::Company, false, true)
        .offers(&[])
        .capacity(Some(capacity(30_000)))
        .dividend_draft(1_001);
    let model = fixture.model();
    let mut split = Vec::new();
    pro_rata_split(1_001, &fixture.ownership, &mut split);
    assert_eq!(split, vec![(PersonId(1), 901), (PersonId(2), 100)]);
    assert_eq!(
        local_dividend_take(1_001, &fixture.ownership, PersonId(1)),
        901
    );
    assert_eq!(
        local_dividend_take(1_001, &fixture.ownership, PersonId(2)),
        100
    );
    assert_eq!(
        local_dividend_take(1_001, &fixture.ownership, PersonId(3)),
        0
    );
    let preview = row_value(&model, "company.dividends.preview");
    assert!(
        preview.contains("10.01 coin")
            && preview.contains("0.10 coin per 10 shares")
            && preview.contains("your 900 shares receive 9.01 coin"),
        "{preview}"
    );
    // The preview is a value: stepping the draft never respawns the page.
    let quarter = ScopedFixture::new(BusinessManagementPage::Company, false, true)
        .offers(&[])
        .capacity(Some(capacity(30_000)))
        .dividend_draft(7_500)
        .model();
    assert_eq!(quarter.structure_key(), model.structure_key());
    assert!(row_value(&quarter, "company.dividends.preview")
        .contains("your 900 shares receive 67.50 coin"));
    // A holder without executive authority previews a full distribution.
    let holder = ScopedFixture::new(BusinessManagementPage::Company, false, false)
        .offers(&[])
        .capacity(Some(capacity(30_000)))
        .model();
    assert!(row_value(&holder, "company.dividends.preview")
        .contains("your 900 shares receive 270.00 coin"));
}

#[test]
fn dividend_capacity_tick_rebinds_the_dividend_rows_in_place() {
    let (mut app, company) = management_app();
    let (root, structure, buttons) = panel_entities(app.world_mut());
    assert!(bound_text(app.world_mut(), "company.dividends").contains("Available now 300.00 coin"));
    assert_eq!(
        bound_text(app.world_mut(), "company.dividends.distribute"),
        "DISTRIBUTE 300.00 COIN",
        "the draft starts at everything distributable"
    );
    // The hero holds 900 of 1,000 shares after the fixture's transfer.
    assert!(bound_text(app.world_mut(), "company.dividends.preview")
        .contains("your 900 shares receive 270.00 coin"));

    // The finance pass paid 100.00 coin: the snapshot shrinks and records it.
    app.world_mut()
        .entity_mut(company)
        .insert(CompanyDividendCapacity {
            distributable: 20_000,
            last_paid_day: 12,
            last_paid: 10_000,
            ..capacity(20_000)
        });
    app.update();
    let (root_after, structure_after, buttons_after) = panel_entities(app.world_mut());
    assert_eq!(root_after, root, "a capacity change respawned the page");
    assert_eq!(structure_after, structure);
    assert_eq!(buttons_after, buttons);
    let value = bound_text(app.world_mut(), "company.dividends");
    assert!(
        value.contains("Available now 200.00 coin") && value.contains("100.00 coin on day 12"),
        "{value}"
    );
    assert_eq!(
        bound_text(app.world_mut(), "company.dividends.distribute"),
        "DISTRIBUTE 200.00 COIN",
        "the confirm payload is clamped to the new headroom"
    );
    let confirm = {
        let key = BoundId::of("company.dividends.distribute");
        app.world_mut()
            .query::<(&BoundButton, &Action)>()
            .iter(app.world())
            .find(|(marker, _)| marker.0 == key)
            .map(|(_, action)| action.0)
            .unwrap()
    };
    assert_eq!(
        confirm,
        ControlPress::Company(
            CompanyId(4),
            HeroCompanyAction::DistributeDividend { pennies: 20_000 }
        )
    );
    let steps: Vec<_> = app
        .world_mut()
        .query::<&DividendDraftAction>()
        .iter(app.world())
        .copied()
        .collect();
    assert_eq!(steps.len(), 5, "five picker steps carry their payload");
}

#[test]
fn an_unedited_draft_follows_the_headroom_when_the_snapshot_arrives() {
    let (mut app, company) = management_app();
    // A company opened before its first finance review has no snapshot, so
    // the draft seeds at zero.
    app.world_mut()
        .entity_mut(company)
        .remove::<CompanyDividendCapacity>();
    *app.world_mut().resource_mut::<DividendDraft>() = DividendDraft::default();
    app.update();
    assert_eq!(
        bound_text(app.world_mut(), "company.dividends.distribute"),
        "DISTRIBUTE 0.00 COIN"
    );
    assert_eq!(
        *app.world().resource::<DividendDraft>(),
        DividendDraft::all_of(CompanyId(4), 0)
    );

    // The finance pass publishes headroom: an unedited draft picks it up.
    app.world_mut().entity_mut(company).insert(capacity(31_500));
    app.update();
    assert_eq!(
        bound_text(app.world_mut(), "company.dividends.distribute"),
        "DISTRIBUTE 315.00 COIN",
        "the confirm control must not stay pinned at zero once headroom exists"
    );
    let confirm = |world: &mut World| {
        let key = BoundId::of("company.dividends.distribute");
        world
            .query::<(&BoundButton, &Action)>()
            .iter(world)
            .find(|(marker, _)| marker.0 == key)
            .map(|(_, action)| action.0)
            .unwrap()
    };
    assert_eq!(
        confirm(app.world_mut()),
        ControlPress::Company(
            CompanyId(4),
            HeroCompanyAction::DistributeDividend { pennies: 31_500 }
        )
    );

    // Once the player steps the amount, later snapshots leave it alone
    // (apart from the downward clamp the payload already applies).
    app.world_mut()
        .resource_mut::<DividendDraft>()
        .step(DividendDraftAction::Down, 31_500);
    app.update();
    assert_eq!(
        bound_text(app.world_mut(), "company.dividends.distribute"),
        "DISTRIBUTE 314.00 COIN"
    );
    app.world_mut().entity_mut(company).insert(capacity(40_000));
    app.update();
    assert_eq!(
        bound_text(app.world_mut(), "company.dividends.distribute"),
        "DISTRIBUTE 314.00 COIN",
        "an edited draft is the player's choice, not the headroom"
    );
    assert_eq!(
        confirm(app.world_mut()),
        ControlPress::Company(
            CompanyId(4),
            HeroCompanyAction::DistributeDividend { pennies: 31_400 }
        )
    );
}

/// The people index and worker list are refreshed from replication change
/// signals rather than rebuilt per frame; every kind of change (a body
/// arriving, a rename, a job change, a despawn) must still reach the chips.
#[test]
fn worker_churn_after_the_first_frame_still_rebinds_the_chips() {
    let (mut app, _) = management_app();
    let site = app
        .world_mut()
        .spawn((
            BuildingId(9),
            OperatedBy(CompanyId(4)),
            SettlementBuilding {
                kind: SettlementBuildingKind::Bakery,
                settlement: "Brackwater".into(),
                owner: Some("Aldric".into()),
                quality: 1.0,
                workers: Vec::new(),
            },
            GoodsInventory::new(SettlementBuildingKind::Bakery.storage_bulk_capacity()),
            BusinessAccount::default(),
            BusinessSalePolicy::default(),
            BusinessManagementPolicy::default(),
            BusinessWagePolicy::default(),
            BusinessProcurementPolicy::none(),
            BusinessSupplyPolicy::none(),
        ))
        .id();
    app.world_mut().resource_mut::<BusinessManagementTarget>().0 =
        Some(BusinessManagementSelection::Site(site));
    app.update();
    app.update();
    let vacant = slot_state(app.world_mut(), "worker.0");
    assert_eq!(vacant.len(), 1);
    assert_eq!(
        (vacant[0].1, vacant[0].2),
        (Some(PersonId::UNASSIGNED), Display::None),
        "no observed employee yet"
    );
    let (root, structure, buttons) = panel_entities(app.world_mut());

    // A worker walks into replication after the index was first built.
    let worker = app
        .world_mut()
        .spawn((
            PersonId(7),
            CharacterName("Cass".into()),
            EmployedAt(BuildingId(9)),
        ))
        .id();
    app.update();
    let filled = slot_state(app.world_mut(), "worker.0");
    assert_eq!(
        (filled[0].1, filled[0].2),
        (Some(PersonId(7)), Display::Flex)
    );
    assert_eq!(bound_text(app.world_mut(), "worker.0"), "Cass");

    // A rename reaches the chip label.
    app.world_mut().get_mut::<CharacterName>(worker).unwrap().0 = "Cassia".into();
    app.update();
    assert_eq!(bound_text(app.world_mut(), "worker.0"), "Cassia");

    // Leaving the job empties the slot; the body itself stays replicated.
    app.world_mut().entity_mut(worker).remove::<EmployedAt>();
    app.update();
    let left = slot_state(app.world_mut(), "worker.0");
    assert_eq!(
        (left[0].1, left[0].2),
        (Some(PersonId::UNASSIGNED), Display::None)
    );

    // Rehired, then despawned entirely.
    app.world_mut()
        .entity_mut(worker)
        .insert(EmployedAt(BuildingId(9)));
    app.update();
    assert_eq!(
        slot_state(app.world_mut(), "worker.0")[0].1,
        Some(PersonId(7))
    );
    app.world_mut().entity_mut(worker).despawn();
    app.update();
    let gone = slot_state(app.world_mut(), "worker.0");
    assert_eq!(
        (gone[0].1, gone[0].2),
        (Some(PersonId::UNASSIGNED), Display::None)
    );

    let (root_after, structure_after, buttons_after) = panel_entities(app.world_mut());
    assert_eq!(root_after, root, "worker churn never respawns the page");
    assert_eq!(structure_after, structure);
    assert_eq!(buttons_after, buttons);
}
