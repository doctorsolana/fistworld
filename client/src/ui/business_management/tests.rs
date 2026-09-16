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

#[test]
fn control_ids_are_unique_within_a_model() {
    let model = sample_model(500, 1);
    let mut ids = Vec::new();
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
    let mut deduped = ids.clone();
    deduped.sort();
    deduped.dedup();
    assert_eq!(ids.len(), deduped.len(), "duplicate bound ids: {ids:?}");
}

fn scoped_model(page: BusinessManagementPage, with_site: bool, manager: bool) -> ControlsModel {
    let person = PersonId(1);
    let company = Company {
        name: "Brackwater Trading Company".into(),
        founded_day: 2,
    };
    let ownership = CompanyOwnership::sole(person);
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
    let share_market = CompanyShareMarket::default();
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
    let workers = [
        WorkerModel {
            person: PersonId(9),
            name: "Ada".into(),
        },
        WorkerModel {
            person: PersonId(10),
            name: "Ada".into(),
        },
    ];
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
        workers: &workers,
        company: Some(CompanyView {
            id: CompanyId(4),
            company: &company,
            ownership: &ownership,
            leadership: &leadership,
            account: &company_account,
            policy: &company_policy,
            decisions: &decisions,
            share_market: &share_market,
        }),
        local_person: Some(person),
        share_draft: &ShareOrderDraft::default(),
        page,
        feedback: &BusinessFeedback::default(),
        name_of: &|person| format!("Person #{}", person.0),
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
        ControlPress::Company(CompanyId(4), HeroCompanyAction::DistributeAvailableProfit)
    )));
    assert!(
        controls
            .iter()
            .all(|(page, control)| *page == BusinessManagementPage::Company
                && !matches!(
                    control.press,
                    ControlPress::Order(_) | ControlPress::Person(_)
                ))
    );
}

#[test]
fn company_and_site_policies_keep_distinct_scopes_and_manual_selections() {
    let model = scoped_model(BusinessManagementPage::Site, true, true);
    let controls = scoped_controls(&model);
    assert!(
        controls
            .iter()
            .any(|(page, control)| *page == BusinessManagementPage::Site
                && control.selected
                && matches!(
                    control.press,
                    ControlPress::Order(HeroBusinessAction::SetStrategy(
                        BusinessStrategy::Aggressive
                    ))
                ))
    );
    assert!(
        controls
            .iter()
            .any(|(page, control)| *page == BusinessManagementPage::Company
                && control.selected
                && matches!(
                    control.press,
                    ControlPress::Company(
                        CompanyId(4),
                        HeroCompanyAction::SetStrategy(BusinessStrategy::Conservative)
                    )
                ))
    );
    for (page, control) in &controls {
        match control.press {
            ControlPress::Order(_) | ControlPress::Person(_) => {
                assert_eq!(*page, BusinessManagementPage::Site)
            }
            ControlPress::Company(id, _) => {
                assert_eq!(id, CompanyId(4));
                assert_eq!(*page, BusinessManagementPage::Company);
            }
            ControlPress::Draft(_) => assert_eq!(*page, BusinessManagementPage::Company),
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
                Some(person.0)
            } else {
                None
            }
        })
        .collect();
    assert_eq!(
        linked,
        vec![9, 10],
        "identical names must retain different person identities"
    );
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
                    | HeroCompanyAction::DistributeAvailableProfit
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
        .map(|(entity, marker)| (entity, marker.0.clone()))
        .collect();
    assert!(!ids.is_empty());
    let mut state = SystemState::<BoundControls>::new(&mut world);
    bind_panel(&after, &mut state.get_mut(&mut world).unwrap());
    assert_eq!(
        ids,
        buttons
            .iter(&world)
            .map(|(entity, marker)| (entity, marker.0.clone()))
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
    assert!(
        !scoped_controls(&model)
            .iter()
            .any(|(_, control)| matches!(control.press, ControlPress::Order(_)))
    );
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
