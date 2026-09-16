//! Temporary staff release while a newly built workplace awaits its road.
use super::*;

#[derive(Component)]
pub(super) struct AwaitingWorkplaceAccess {
    workplace: Entity,
}

pub(super) fn defer_shift(
    commands: &mut Commands,
    employees: &[Entity],
    workplace: Entity,
    day: u32,
    off_duty: &Query<&WorkerOffDuty>,
) {
    for employee in employees {
        if off_duty.get(*employee).is_ok_and(|off| off.day == day) {
            continue;
        }
        commands
            .entity(*employee)
            .insert((WorkerOffDuty { day }, AwaitingWorkplaceAccess { workplace }));
    }
}

pub(super) fn resume_staff_after_road_completion(
    mut commands: Commands,
    index: Res<crate::world::identity::WorldIdentityIndex>,
    employees: Query<(
        Entity,
        &AwaitingWorkplaceAccess,
        Option<&shared::components::EmployedAt>,
    )>,
    workplaces: Query<(
        &SettlementBuilding,
        &PlayerPosition,
        &PlayerRotation,
        &shared::components::BuildingId,
        &shared::components::BuildingOf,
    )>,
    settlements: Query<(&PlayerPosition, Option<&PlayerRotation>)>,
    roads: Query<(&VillageRoad, &shared::components::RoadOf)>,
    requests: Query<(), With<RoadRequest>>,
) {
    if employees.is_empty() {
        return;
    }
    // Several employees share one workplace. Resolve its access once, and
    // only inspect the small roster currently awaiting construction.
    let mut ready_by_workplace = HashMap::new();
    for (employee, awaiting, employment) in &employees {
        let Ok((building, position, rotation, id, building_of)) =
            workplaces.get(awaiting.workplace)
        else {
            commands
                .entity(employee)
                .remove::<AwaitingWorkplaceAccess>();
            continue;
        };
        if employment.is_none_or(|employment| employment.0 != *id) {
            commands
                .entity(employee)
                .remove::<AwaitingWorkplaceAccess>();
            continue;
        }
        let ready = *ready_by_workplace
            .entry(awaiting.workplace)
            .or_insert_with(|| {
                let Some((hall_position, hall_rotation)) = index
                    .settlements
                    .get(&building_of.0)
                    .and_then(|hall| settlements.get(*hall).ok())
                else {
                    return false;
                };
                trades::workplace_road_is_ready(
                    awaiting.workplace,
                    building.kind,
                    position.0,
                    rotation.0,
                    building_of.0,
                    hall_position.0,
                    hall_rotation.map_or(0.0, |r| r.0),
                    &roads,
                    &requests,
                )
            });
        if ready {
            // Ordinary assignment still enforces working hours and employment.
            // A road opening does not grant overtime or impose an output quota.
            commands
                .entity(employee)
                .remove::<AwaitingWorkplaceAccess>()
                .remove::<WorkerOffDuty>();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shared::components::{
        BuildingId, BuildingOf, EmployedAt, RoadClass, RoadOf, RoadSurface, SettlementId,
    };

    #[test]
    fn a_new_workplaces_staff_start_when_its_road_opens_that_same_day() {
        let mut app = App::new();
        app.init_resource::<crate::world::identity::WorldIdentityIndex>();
        app.add_systems(
            Update,
            (
                resume_staff_after_road_completion,
                trades::assign_lumberjack_routines,
            )
                .chain(),
        );
        let clock = WorldTime::new_default();
        let day = clock.day;
        app.world_mut().spawn(clock);
        let settlement_id = SettlementId(1);
        let hall_position = Vec3::new(1700.0, 0.0, 0.0);
        let hall = app
            .world_mut()
            .spawn((
                settlement_id,
                PlayerPosition(hall_position),
                PlayerRotation(0.0),
            ))
            .id();
        app.world_mut()
            .resource_mut::<crate::world::identity::WorldIdentityIndex>()
            .settlements
            .insert(settlement_id, hall);
        let hut_position = hall_position + Vec3::X * 30.0;
        let building_id = BuildingId(1);
        let hut = app
            .world_mut()
            .spawn((
                SettlementBuilding {
                    kind: SettlementBuildingKind::LumberjackHut,
                    settlement: "Roadstead".into(),
                    owner: None,
                    quality: 0.7,
                    workers: vec!["Mara".into()],
                },
                PlayerPosition(hut_position),
                PlayerRotation(0.0),
                building_id,
                BuildingOf(settlement_id),
            ))
            .id();
        let worker = app
            .world_mut()
            .spawn((
                CharacterName("Mara".into()),
                PlayerPosition(hall_position),
                VillagerIntent::Resident { settlement: hall },
                EmployedAt(building_id),
            ))
            .id();
        // A genuinely completed shift must stay finished when another
        // employee's temporary access wait ends.
        let finished = app
            .world_mut()
            .spawn((
                CharacterName("Finn".into()),
                PlayerPosition(hall_position),
                VillagerIntent::Resident { settlement: hall },
                EmployedAt(building_id),
                WorkerOffDuty { day },
            ))
            .id();
        let door = SettlementBuildingKind::LumberjackHut
            .entrance_position(hut_position, 0.0)
            .xz();
        let hall_door = SettlementBuildingKind::Hall
            .entrance_position(hall_position, 0.0)
            .xz();
        let road = app
            .world_mut()
            .spawn((
                VillageRoad {
                    settlement: "Roadstead".into(),
                    builder: "Road crew".into(),
                    points: vec![door, hall_door],
                    built_through: 1,
                    width: crate::world::village_roads::VILLAGE_ROAD_WIDTH,
                    reserved_width: RoadClass::Lane.initial_reserved_width(),
                    surface: RoadSurface::Dirt,
                    class: RoadClass::Lane,
                    stone_committed: 0,
                },
                RoadOf(settlement_id),
            ))
            .id();
        app.update();
        assert!(app.world().get::<LumberjackRoutine>(worker).is_none());
        assert_eq!(
            app.world()
                .get::<AwaitingWorkplaceAccess>(worker)
                .unwrap()
                .workplace,
            hut
        );
        assert!(app
            .world()
            .get::<AwaitingWorkplaceAccess>(finished)
            .is_none());
        app.world_mut()
            .get_mut::<VillageRoad>(road)
            .unwrap()
            .built_through = 2;
        app.update();
        assert!(app.world().get::<LumberjackRoutine>(worker).is_some());
        assert!(app.world().get::<WorkerOffDuty>(worker).is_none());
        assert!(app.world().get::<AwaitingWorkplaceAccess>(worker).is_none());
        assert!(app.world().get::<LumberjackRoutine>(finished).is_none());
        assert_eq!(app.world().get::<WorkerOffDuty>(finished).unwrap().day, day);
    }
}
