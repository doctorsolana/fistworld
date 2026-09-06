//! Settlement presentation and animation lifecycle regressions.

use super::animation::{
    advance_door_state, recover_stale_building_animation_wiring, windmill_cap_yaw,
    BuildingDoorAnimation, DoorCommand, DoorState, DoorTargetWired, WindmillMotion,
    DOOR_CLOSE_SECONDS, DOOR_OPEN_SECONDS,
};
use super::lighting::{
    move_towards, sync_window_lighting, window_light_target, WindowLamp, WindowLighting,
    MAX_LIT_WINDOW_BUILDINGS,
};
use super::stock::bakery_bread_level;
use bevy::prelude::*;
use shared::components::{
    Household, PlayerPosition, SettlementBuilding, SettlementBuildingKind, WorldTime,
};
use shared::economy::{Good, GoodsInventory};

#[test]
fn aggregate_door_demand_opens_holds_and_closes_once() {
    let (opening, command) = advance_door_state(DoorState::Shut, true, 0.0);
    assert_eq!(command, Some(DoorCommand::Open { seek_seconds: 0.0 }));
    let (open, command) = advance_door_state(opening, true, DOOR_OPEN_SECONDS);
    assert!(matches!(open, DoorState::Open { .. }));
    assert_eq!(command, None, "continued demand must not replay door_open");

    let (held, command) = advance_door_state(open, false, 0.25);
    assert!(matches!(held, DoorState::Open { .. }));
    assert_eq!(command, None);
    let (closing, command) = advance_door_state(held, false, 0.26);
    assert!(matches!(closing, DoorState::Closing { .. }));
    assert_eq!(command, Some(DoorCommand::Close));
    let (shut, command) = advance_door_state(closing, false, DOOR_CLOSE_SECONDS);
    assert_eq!(shut, DoorState::Shut);
    assert_eq!(command, None);
}

#[test]
fn a_second_arrival_during_closing_reopens_the_door() {
    let (state, command) =
        advance_door_state(DoorState::Closing { elapsed: 0.2 }, true, 1.0 / 60.0);
    let DoorState::Opening { elapsed } = state else {
        panic!("the closing door should resume opening");
    };
    let expected = (1.0 - 0.2 / DOOR_CLOSE_SECONDS) * DOOR_OPEN_SECONDS;
    assert!((elapsed - expected).abs() < 1e-6);
    assert_eq!(
        command,
        Some(DoorCommand::Open {
            seek_seconds: expected
        })
    );
}

#[test]
fn stale_scene_players_release_the_root_for_rewiring() {
    let mut app = App::new();
    app.add_systems(Update, recover_stale_building_animation_wiring);
    let graph = AnimationGraph::new();
    let node = graph.root;
    let root = app
        .world_mut()
        .spawn(BuildingDoorAnimation {
            player: Entity::PLACEHOLDER,
            open: node,
            close: node,
            state: DoorState::Shut,
        })
        .id();
    let old_target = app.world_mut().spawn(DoorTargetWired).id();
    app.world_mut().entity_mut(root).add_child(old_target);

    app.update();

    assert!(
        app.world().get::<BuildingDoorAnimation>(root).is_none(),
        "a dead animation player must not leave the building permanently wired"
    );
    assert!(
        app.world().get::<DoorTargetWired>(old_target).is_none(),
        "the replacement scene's target must be eligible for discovery"
    );
}

#[test]
fn a_replaced_windmill_cap_releases_the_root_for_rewiring() {
    let mut app = App::new();
    app.add_systems(Update, recover_stale_building_animation_wiring);
    let graph = AnimationGraph::new();
    let node = graph.root;
    let player = app.world_mut().spawn(AnimationPlayer::default()).id();
    let root = app
        .world_mut()
        .spawn((
            BuildingDoorAnimation {
                player,
                open: node,
                close: node,
                state: DoorState::Shut,
            },
            WindmillMotion {
                player,
                sails: node,
                cap: Entity::PLACEHOLDER,
                cap_rest_rotation: Quat::IDENTITY,
            },
        ))
        .id();

    app.update();

    assert!(app.world().get::<BuildingDoorAnimation>(root).is_none());
    assert!(app.world().get::<WindmillMotion>(root).is_none());
}

#[test]
fn windmill_cap_faces_upwind_after_any_plot_rotation() {
    for direction in [
        Vec2::X,
        Vec2::NEG_X,
        Vec2::Y,
        Vec2::NEG_Y,
        crate::wind::WIND_DIRECTION,
    ] {
        for root_yaw in [0.0, 0.4, -1.2, 2.7] {
            let world_yaw = root_yaw + windmill_cap_yaw(root_yaw, direction);
            let front = Quat::from_rotation_y(world_yaw) * Vec3::NEG_Z;
            let upwind = -direction;
            assert!((front.x - upwind.x).abs() < 1e-5);
            assert!((front.z - upwind.y).abs() < 1e-5);
        }
    }
}

#[test]
fn bakery_loaves_present_empty_partial_and_full_real_stock() {
    let mut inventory = GoodsInventory::new(240);
    assert_eq!(bakery_bread_level(&inventory), 0);
    inventory.add(Good::Bread, 1);
    assert_eq!(bakery_bread_level(&inventory), 1);
    inventory.add(Good::Bread, 119);
    assert_eq!(bakery_bread_level(&inventory), 3);
    inventory.add(Good::Bread, 120);
    assert_eq!(bakery_bread_level(&inventory), 6);
}

#[test]
fn cabin_windows_require_both_darkness_and_an_occupied_household() {
    // Use the production daylight/night proportions. An arbitrary short
    // clock no longer maps the authored 05:00/23:00 normalized constants
    // to its day boundary now that the display clock and sun arc are
    // intentionally separate.
    let mut clock = WorldTime::new_default();

    clock.set_normalized_time(0.5);
    assert_eq!(window_light_target(&clock, true), 0.0);

    clock.set_normalized_time(0.0);
    assert_eq!(window_light_target(&clock, false), 0.0);
    assert_eq!(window_light_target(&clock, true), 1.0);

    clock.set_normalized_time(WorldTime::SUNSET_NORMALIZED + 0.001);
    assert!(window_light_target(&clock, true) < 0.1);
}

#[test]
fn cabin_window_fade_never_overshoots_its_target() {
    assert_eq!(move_towards(0.0, 1.0, 0.25), 0.25);
    assert_eq!(move_towards(0.9, 1.0, 0.25), 1.0);
    assert_eq!(move_towards(1.0, 0.0, 0.4), 0.6);
    assert_eq!(move_towards(0.1, 0.0, 0.4), 0.0);
}

#[test]
fn one_resident_lights_a_cabin_and_stale_scene_wiring_is_rebuilt() {
    let mut app = App::new();
    app.init_resource::<Time>();
    app.init_resource::<Assets<StandardMaterial>>();
    app.add_systems(Update, sync_window_lighting);
    let mut clock = WorldTime::new(600.0, 300.0, 0.0);
    clock.set_normalized_time(0.0);
    app.world_mut().spawn(clock);
    let glass = app
        .world_mut()
        .resource_mut::<Assets<StandardMaterial>>()
        .add(StandardMaterial::default());
    let lamp = app
        .world_mut()
        .spawn((WindowLamp, PointLight::default(), Visibility::Hidden))
        .id();
    let house = app
        .world_mut()
        .spawn((
            Household {
                residents: vec!["Alda".into()],
                ..default()
            },
            WindowLighting {
                glass: glass.clone(),
                lamps: vec![lamp],
                strength: 0.0,
                lamp_strength: 0.0,
            },
        ))
        .id();
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(std::time::Duration::from_secs(1));

    app.update();

    assert!(
        app.world().get::<PointLight>(lamp).unwrap().intensity > 0.0,
        "a single assigned resident should light the cabin at night"
    );
    assert_eq!(
        *app.world().get::<Visibility>(lamp).unwrap(),
        Visibility::Inherited
    );
    assert_ne!(
        app.world()
            .resource::<Assets<StandardMaterial>>()
            .get(&glass)
            .unwrap()
            .emissive,
        LinearRgba::BLACK
    );

    app.world_mut().despawn(lamp);
    app.update();
    assert!(
        app.world().get::<WindowLighting>(house).is_none(),
        "despawned scene descendants must release stale wiring so setup can discover the replacement scene"
    );
}

#[test]
fn dense_neighbourhood_keeps_all_windows_emissive_but_caps_real_lights() {
    let mut app = App::new();
    app.init_resource::<Time>();
    app.init_resource::<Assets<StandardMaterial>>();
    app.add_systems(Update, sync_window_lighting);
    let mut clock = WorldTime::new(600.0, 300.0, 0.0);
    clock.set_normalized_time(0.0);
    app.world_mut().spawn(clock);
    app.world_mut().spawn(crate::camera_rts::CommanderCamera {
        zoom: 190.0,
        zoom_target: 190.0,
        ..default()
    });

    let mut lamps = Vec::new();
    for index in 0..(MAX_LIT_WINDOW_BUILDINGS + 8) {
        let glass = app
            .world_mut()
            .resource_mut::<Assets<StandardMaterial>>()
            .add(StandardMaterial::default());
        let lamp = app
            .world_mut()
            .spawn((
                WindowLamp,
                PointLight {
                    intensity: 0.0,
                    ..default()
                },
                Visibility::Hidden,
            ))
            .id();
        lamps.push(lamp);
        let root = app
            .world_mut()
            .spawn((
                PlayerPosition(Vec3::new(index as f32, 0.0, 0.0)),
                WindowLighting {
                    glass,
                    lamps: vec![lamp],
                    strength: 0.0,
                    lamp_strength: 0.0,
                },
            ))
            .id();
        if index % 2 == 0 {
            app.world_mut().entity_mut(root).insert(Household {
                residents: vec![format!("Resident {index}")],
                ..default()
            });
        } else {
            app.world_mut()
                .entity_mut(root)
                .insert(staffed_lumberjack());
        }
    }
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(std::time::Duration::from_secs(1));

    app.update();

    let active = lamps
        .iter()
        .filter(|lamp| {
            app.world()
                .get::<PointLight>(**lamp)
                .is_some_and(|light| light.intensity > 0.0)
        })
        .count();
    assert_eq!(active, MAX_LIT_WINDOW_BUILDINGS);
    let world = app.world_mut();
    let mut windows = world.query::<&WindowLighting>();
    assert!(windows.iter(world).all(|window| window.strength > 0.0));
}

fn staffed_lumberjack() -> SettlementBuilding {
    SettlementBuilding {
        kind: SettlementBuildingKind::LumberjackHut,
        settlement: "Brackwater".into(),
        owner: Some("Alda".into()),
        quality: 0.8,
        workers: vec!["Alda".into()],
    }
}

#[test]
fn lumberjack_panes_bind_once_and_follow_staffing_and_daylight() {
    assert_workshop_window_lighting(
        shared::components::SettlementBuildingKind::LumberjackHut,
        "HutGlass",
    );
}

#[test]
fn windmill_panes_bind_once_and_follow_staffing_and_daylight() {
    assert_workshop_window_lighting(
        shared::components::SettlementBuildingKind::Windmill,
        "WindMillGlass",
    );
}

fn assert_workshop_window_lighting(
    kind: shared::components::SettlementBuildingKind,
    material_name: &str,
) {
    use super::buildings::BuildingVisual;
    use super::lighting::setup_window_lighting;
    use bevy::gltf::GltfMaterialName;

    let mut app = App::new();
    app.init_resource::<Time>();
    app.init_resource::<Assets<StandardMaterial>>();
    app.add_systems(
        Update,
        (setup_window_lighting, sync_window_lighting).chain(),
    );
    let mut clock = WorldTime::new(600.0, 300.0, 0.0);
    clock.set_normalized_time(0.0);
    let clock_entity = app.world_mut().spawn(clock).id();
    let source = app
        .world_mut()
        .resource_mut::<Assets<StandardMaterial>>()
        .add(StandardMaterial::default());
    let root = app
        .world_mut()
        .spawn((
            shared::components::SettlementBuilding {
                kind,
                ..staffed_lumberjack()
            },
            BuildingVisual {
                building_type: kind.art(),
            },
        ))
        .id();
    let pane = app
        .world_mut()
        .spawn((
            GltfMaterialName(material_name.into()),
            MeshMaterial3d(source.clone()),
        ))
        .id();
    app.world_mut().entity_mut(root).add_child(pane);
    for name in ["Light_Window.L", "Light_Window.R"] {
        let anchor = app.world_mut().spawn(Name::new(name)).id();
        app.world_mut().entity_mut(root).add_child(anchor);
    }
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(std::time::Duration::from_secs(1));
    app.update();
    let wiring = app.world().get::<WindowLighting>(root).unwrap();
    assert_eq!(wiring.lamps.len(), 2);
    assert_eq!(
        wiring.strength, 1.0,
        "a staffed workshop must glow after dark"
    );
    let clone = wiring.glass.clone();
    assert_ne!(
        clone, source,
        "night glow must not mutate the shared GLB material"
    );
    assert_eq!(
        app.world()
            .resource::<Assets<StandardMaterial>>()
            .get(&source)
            .unwrap()
            .emissive,
        LinearRgba::BLACK
    );
    app.update();
    assert_eq!(
        app.world().get::<WindowLighting>(root).unwrap().glass,
        clone
    );
    app.world_mut()
        .get_mut::<WorldTime>(clock_entity)
        .unwrap()
        .set_normalized_time(0.5);
    app.update();
    assert_eq!(
        app.world().get::<WindowLighting>(root).unwrap().strength,
        0.0
    );
    app.world_mut()
        .get_mut::<WorldTime>(clock_entity)
        .unwrap()
        .set_normalized_time(0.0);
    app.world_mut()
        .get_mut::<SettlementBuilding>(root)
        .unwrap()
        .workers
        .clear();
    app.update();
    let wiring = app.world().get::<WindowLighting>(root).unwrap();
    assert_eq!(
        wiring.strength, 0.0,
        "an unstaffed workshop should stay dark"
    );
    for lamp in &wiring.lamps {
        assert_eq!(app.world().get::<PointLight>(*lamp).unwrap().intensity, 0.0);
    }
}
