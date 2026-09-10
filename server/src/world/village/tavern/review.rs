//! Opt-in connected tavern inspection: ordinary people, doors, payment and routes.
//! Start both processes with FISTWORLD_TAVERN_REVIEW=1 on village_lab.
use super::*;
use lightyear::prelude::{server::ClientOf, NetworkTarget, Replicate};
use shared::components::{BuildingOf, EmployedAt, Occupation, OwnedBy, PersonId, ResidentOf};

pub(crate) fn stage_tavern_review(
    mut commands: Commands,
    mut terrain: ResMut<WorldTerrain>,
    mut clocks: Query<&mut WorldTime>,
    clients: Query<Entity, With<ClientOf>>,
    mut ids: ResMut<crate::world::identity::WorldIdAllocator>,
    registry: Res<crate::world::regions::RegionRegistry>,
    mut staged: Local<bool>,
) {
    if *staged
        || std::env::var("FISTWORLD_TAVERN_REVIEW").as_deref() != Ok("1")
        || clients.is_empty()
    {
        return;
    }
    if registry
        .get(shared::region::RegionCoord::from_world_pos(Vec3::ZERO))
        .is_none_or(|region| region.sim_level != shared::region::SimLevel::Tactical)
    {
        return;
    }
    assert_eq!(
        terrain.generator.loaded_map().definition.map_id,
        "village_lab",
        "tavern review requires village_lab"
    );
    let Some(mut clock) = clocks.iter_mut().next() else {
        return;
    };
    clock.set_normalized_time(18.0 / 24.0);
    let day = clock.day;
    let origin = Vec3::new(0.0, terrain.get_height(0.0, 0.0), 0.0);
    let def = shared::building::BuildingType::Tavern.definition();
    terrain.apply_flatten_rect(
        origin,
        def.terrain_flat_half_extents(),
        0.0,
        def.terrain_blend_width(),
    );
    let settlement_id = ids.settlement();
    let building_id = ids.building();
    let owner = ids.person();
    let company = ids.company();
    let settlement = commands
        .spawn((
            Settlement {
                name: "Tankard Inn Review".into(),
                tier: shared::components::SettlementTier::Village,
                residents: 9,
                treasury: 100_000,
            },
            settlement_id,
            PlayerPosition(origin + Vec3::new(0.0, 0.0, 60.0)),
            PlayerRotation(0.0),
            Replicate::to_clients(NetworkTarget::All),
        ))
        .id();
    commands.spawn(new_company_bundle(
        company,
        "The Copper Tankard".into(),
        day,
        owner,
        100_000,
        100_000,
    ));
    let mut inventory = GoodsInventory::new(SettlementBuildingKind::Tavern.storage_bulk_capacity());
    inventory.add(Good::Bread, 24);
    let tavern = commands
        .spawn((
            SettlementBuilding {
                kind: SettlementBuildingKind::Tavern,
                settlement: "Tankard Inn Review".into(),
                owner: Some("Innkeeper".into()),
                quality: 1.0,
                workers: vec!["Innkeeper".into()],
            },
            building_id,
            BuildingOf(settlement_id),
            OwnedBy(owner),
            shared::components::OperatedBy(company),
            PlayerPosition(origin),
            PlayerRotation(0.0),
            inventory,
            BusinessSalePolicy {
                asking_unit_price: 10,
                ..Default::default()
            },
            BusinessAccount::default(),
            BusinessCondition {
                state: BusinessState::Operating,
                ..Default::default()
            },
            TavernService {
                innkeepers_on_duty: 1,
                ..Default::default()
            },
            Replicate::to_clients(NetworkTarget::All),
        ))
        .id();
    let worker = crate::player::hero::spawn_villager(&mut commands, &terrain, 8000, origin);
    commands.entity(worker).insert((
        owner,
        CharacterName("Innkeeper".into()),
        ResidentOf(settlement_id),
        Residence("Tankard Inn Review".into()),
        VillagerIntent::Resident { settlement },
        WorkStatus::Employed,
        Occupation(Some("Innkeeper".into())),
        EmployedAt(building_id),
        TavernWorkerRoutine {
            tavern,
            phase: TavernWorkerPhase::Serving,
        },
    ));
    for index in 0..8 {
        let position = origin
            + Vec3::new(
                (index % 4) as f32 * 2.0 - 3.0,
                0.0,
                -18.0 - (index / 4) as f32 * 2.0,
            );
        let visitor =
            crate::player::hero::spawn_villager(&mut commands, &terrain, 8100 + index, position);
        let person: PersonId = ids.person();
        commands.entity(visitor).insert((
            person,
            CharacterName(format!("Patron {}", index + 1)),
            ResidentOf(settlement_id),
            Residence("Tankard Inn Review".into()),
            VillagerIntent::Resident { settlement },
            WorkStatus::Chilling,
            Wallet::new(10_000),
            CharacterDayPlan {
                day,
                wake_minute: 360,
                work_minutes: None,
                meal_minute: 1080,
                leisure_minutes: (1080, 1290),
                sleep_minute: 1380,
                leisure: PlannedLeisure::TavernMeal,
                leisure_status: PlannedLeisureStatus::InProgress,
                planned_work_status: WorkStatus::Chilling,
            },
            TavernVisitRoutine {
                tavern,
                queue_order: u128::from(index),
                phase: TavernVisitPhase::Going,
                dining_seconds: 0.0,
                failed_routes: 0,
                served: false,
                outdoor_seat: None,
                progress_position: position,
                progress_world_seconds: f64::from(clock.seconds_in_cycle),
            },
        ));
    }
    *staged = true;
    info!("Tavern review: staged eight ordinary visits and one staffed company at {origin:?}");
}
