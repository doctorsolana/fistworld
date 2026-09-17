//! Offline permit-placement preview rehearsal over replicated components.
//!
//! `FISTFORCE_CAPTURE_PERMIT_TOUR=1` with `FISTFORCE_CAPTURE_SETTLEMENT=1`
//! stages a small street beside the founded Hall (two cabins with a one-lot
//! gap, a completed farm with its fields, a pending cabin with its reserved
//! access lane) and arms a real HOUSE permit. Each shot parks the placement
//! cursor at a scripted spot and the sidecar records what the production
//! preview decided. This exercises the client's own land rules and their
//! wording; it does not prove a connected Hall would accept the plot.
use super::ledger_nested::shot;
use super::{CaptureConfig, CaptureState};
use bevy::{input::InputSystems, prelude::*};
use shared::components::{
    BuildingId, BuildingOf, ConstructionSite, FarmField, PlayerPosition, PlayerRotation,
    ReservedAccessLane, RoadClass, RoadOf, RoadSurface, SettlementBuilding,
    SettlementBuildingKind as Kind, SettlementId, VillageRoad, builder_stand_position,
};

#[derive(Resource, Default)]
pub(super) struct Rehearsal {
    staged: bool,
    hall: Vec3,
    cursor: Option<(usize, Vec2)>,
    ready_shot: Option<usize>,
    inspected: Option<usize>,
    error: String,
}

pub(super) fn install(app: &mut App) {
    if std::env::var("FISTFORCE_CAPTURE_PERMIT_TOUR").as_deref() != Ok("1") {
        return;
    }
    app.init_resource::<Rehearsal>();
    app.add_systems(Update, stage);
    app.add_systems(PreUpdate, drive.after(InputSystems));
    app.add_systems(Last, inspect);
}

/// Street setback the road snap uses for a cabin: corner radius, the reserved
/// half-width and the two verges, so the staged cabins stand exactly where a
/// snapped player cabin would.
fn cabin_setback() -> f32 {
    Kind::House.placement_definition().root_footprint_radius() + 0.45 + 2.0 + 0.25
}

/// Scripted cursor for a shot, relative to the Hall, and whether placement is
/// free (Shift held) or left to the road magnet.
fn script(name: &str) -> Option<(Vec2, bool)> {
    let gap_x = 12.0 + 12.2;
    let field_edge_x = -40.0
        + shared::components::FARM_FIELD_LATERAL_OFFSET
        + Kind::Farmstead.field_half_extents().unwrap().x
        + 0.7
        + Kind::House.placement_definition().footprint.x * 0.5;
    Some(match name.split('-').next()? {
        "00" | "01" => (Vec2::new(-40.0, -60.0), true),
        "02" => (Vec2::new(gap_x, -33.0), false),
        "03" => (Vec2::new(field_edge_x, 29.0), true),
        "04" => (Vec2::new(45.0, 0.0), true),
        _ => return None,
    })
}

fn stage(world: &mut World) {
    if world.resource::<Rehearsal>().staged {
        return;
    }
    let Some((settlement, hall)) = world
        .query::<(&SettlementId, &PlayerPosition)>()
        .iter(world)
        .find(|(id, _)| **id == SettlementId(1))
        .map(|(id, position)| (*id, position.0))
    else {
        return;
    };
    let ground = |world: &World, x: f32, z: f32| {
        world
            .resource::<shared::terrain::WorldTerrain>()
            .get_height(x, z)
    };
    let at = |world: &World, offset: Vec2| {
        let point = hall.xz() + offset;
        Vec3::new(point.x, ground(world, point.x, point.y), point.y)
    };
    let road = |points: Vec<Vec2>| VillageRoad {
        settlement: "Brackwater".into(),
        builder: "Capture Road Steward".into(),
        built_through: points.len() as u16,
        points,
        width: 3.0,
        reserved_width: 4.0,
        surface: RoadSurface::Dirt,
        class: RoadClass::Lane,
        stone_committed: 0,
    };
    let hall_door = Kind::Hall.entrance_position(hall, 0.0).xz();
    let street_z = hall.z - 30.0;
    // The street passes through the connector's junction so the road graph
    // the magnet walks from the Hall door reaches both of its halves.
    world.spawn((
        road(vec![
            Vec2::new(hall.x - 60.0, street_z),
            Vec2::new(hall_door.x, street_z),
            Vec2::new(hall.x + 60.0, street_z),
        ]),
        RoadOf(settlement),
    ));
    world.spawn((
        road(vec![hall_door, Vec2::new(hall_door.x, street_z)]),
        RoadOf(settlement),
    ));

    let building = |kind: Kind| SettlementBuilding {
        kind,
        settlement: "Brackwater".into(),
        owner: None,
        quality: 0.6,
        workers: Vec::new(),
    };
    // Two cabins on the north side of the street with exactly one frontage
    // lot free between them.
    for (id, x) in [(10, 12.0), (11, 12.0 + 2.0 * 12.2)] {
        let position = at(world, Vec2::new(x, -30.0 - cabin_setback()));
        world.spawn((
            building(Kind::House),
            BuildingOf(settlement),
            BuildingId(id),
            PlayerPosition(position),
            PlayerRotation(std::f32::consts::PI),
        ));
    }
    // A completed farm whose accepted fields face the open ground to the east.
    let farm = at(world, Vec2::new(-40.0, 20.0));
    world.spawn((
        building(Kind::Farmstead),
        BuildingOf(settlement),
        BuildingId(12),
        PlayerPosition(farm),
        PlayerRotation(0.0),
    ));
    for index in 0..2 {
        let field = Kind::Farmstead.field_position_at(farm, 0.0, index).unwrap();
        world.spawn((
            FarmField {
                settlement: "Brackwater".into(),
                farmstead: farm,
                plot_index: index,
                layout_version: 0,
                quality: 0.6,
                shape: None,
            },
            PlayerPosition(Vec3::new(field.x, ground(world, field.x, field.z), field.z)),
            PlayerRotation(0.0),
        ));
    }
    // A pending cabin whose surveyed lane still runs to the street.
    let site = at(world, Vec2::new(45.0, 30.0));
    let site_door = Kind::House.entrance_position(site, 0.0).xz();
    world.spawn((
        ConstructionSite {
            kind: Kind::House,
            settlement: "Brackwater".into(),
            raising: false,
            stand: builder_stand_position(site, 0.0, Kind::House.art().definition().footprint.y),
            rotation: 0.0,
        },
        BuildingOf(settlement),
        PlayerPosition(site),
        ReservedAccessLane {
            points: vec![
                site_door,
                Vec2::new(site_door.x, hall.z - 20.0),
                Vec2::new(site_door.x, street_z),
            ],
            half_width: 2.0,
        },
    ));

    *world.resource_mut::<crate::hero::control::WorldPlacementMode>() =
        crate::hero::control::WorldPlacementMode::Permit {
            permit: shared::components::PlayerPermit {
                id: shared::components::PermitId(900),
                settlement,
                kind: Kind::House,
                fee_escrow: 0,
                purchased_day: 1,
                company: None,
            },
            settlement_name: "Brackwater".into(),
            rotation: 0.0,
        };
    let mut rehearsal = world.resource_mut::<Rehearsal>();
    rehearsal.hall = hall;
    rehearsal.staged = true;
}

/// Park the placement cursor at the current shot's scripted spot and hold or
/// release Shift for free placement, through the same resources the real
/// pointer and keyboard feed.
fn drive(world: &mut World) {
    let Some((index, name)) = shot(world) else {
        return;
    };
    let staged = world.resource::<Rehearsal>().staged;
    if !staged {
        return;
    }
    let Some((offset, free)) = script(&name) else {
        return;
    };
    let hall = world.resource::<Rehearsal>().hall;
    let cursor = hall.xz() + offset;
    if world.resource::<Rehearsal>().cursor == Some((index, cursor)) {
        return;
    }
    world.insert_resource(crate::camera_rts::CursorTerrainOverride(cursor));
    let mut keys = world.resource_mut::<ButtonInput<KeyCode>>();
    if free {
        keys.press(KeyCode::ShiftLeft);
    } else {
        keys.release(KeyCode::ShiftLeft);
    }
    world.resource_mut::<Rehearsal>().cursor = Some((index, cursor));
}

fn check(world: &World, name: &str) -> Result<serde_json::Value, String> {
    let hall = world.resource::<Rehearsal>().hall;
    let (offset, _) = script(name).ok_or("shot has no script")?;
    let (position, rotation, valid, reason) =
        crate::ui::player_permits::inspect_placement(world).ok_or("no placement preview yet")?;
    let blocker = crate::ui::player_permits::inspect_placement_blocker(world);
    let cursor = hall.xz() + offset;
    let expect = |condition: bool, what: &str| {
        condition.then_some(()).ok_or_else(|| {
            format!(
                "{what}; preview valid={valid} reason={reason:?} blocker={blocker:?} at {position}"
            )
        })
    };
    match name.split('-').next().unwrap_or_default() {
        "00" | "01" => {
            expect(valid, "the clear spot must be accepted")?;
            expect(
                reason.starts_with("Legal expansion plot"),
                "the clear spot is an expansion plot",
            )?;
            expect(
                position.xz().distance(cursor) < 0.05,
                "free placement follows the cursor",
            )?;
        }
        "02" => {
            expect(valid, "the one-lot gap must be accepted")?;
            expect(
                reason.starts_with("Road frontage locked"),
                "the gap plot snaps to the street",
            )?;
            expect(
                (position.x - cursor.x).abs() < 0.05
                    && (position.z - (hall.z - 30.0 - cabin_setback())).abs() < 0.05,
                "the snapped cabin sits on the street setback between its neighbours",
            )?;
        }
        "03" => {
            expect(!valid, "a cabin 0.7 m off a fence must be refused")?;
            expect(
                reason.starts_with("Too close to FARMSTEAD #12's wheat field:")
                    && reason.ends_with("of 2.5 m."),
                "the field refusal names the farm and the yard",
            )?;
            expect(
                blocker
                    .as_ref()
                    .is_some_and(|(label, land)| label == "FARMSTEAD #12" && *land == "field"),
                "the keep-out is the farm's field",
            )?;
        }
        "04" => {
            expect(!valid, "a cabin across a reserved lane must be refused")?;
            expect(
                reason == "Overlaps the access lane reserved for HOUSE (under construction).",
                "the lane refusal names the pending cabin",
            )?;
            expect(
                blocker.as_ref().is_some_and(|(label, land)| {
                    label == "HOUSE (under construction)" && *land == "access lane"
                }),
                "the keep-out is the reserved lane",
            )?;
        }
        _ => return Err("unknown shot".into()),
    }
    Ok(serde_json::json!({
        "shot": name,
        "passed": true,
        "fixture": "offline replicated buildings, fields, roads and a reserved lane; the client's own preview rules, not a connected Hall verdict",
        "cursor": cursor.to_array(),
        "preview": {"position": position.to_array(), "rotation": rotation, "valid": valid, "reason": reason},
        "blocker": blocker.map(|(label, land)| serde_json::json!({"label": label, "land": land})),
    }))
}

fn inspect(world: &mut World) {
    let Some((index, name)) = shot(world) else {
        return;
    };
    if !world.resource::<Rehearsal>().staged {
        return;
    }
    let result = check(world, &name);
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
    let evidence =
        result.unwrap_or_else(|error| panic!("permit placement capture failed: {error}"));
    std::fs::write(
        world
            .resource::<CaptureConfig>()
            .out_dir
            .join(format!("{name}.placement.json")),
        serde_json::to_vec_pretty(&evidence).unwrap(),
    )
    .expect("permit placement evidence");
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
        "permit placement readiness timed out: {}",
        rehearsal.error
    );
    false
}
