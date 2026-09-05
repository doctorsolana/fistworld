//! Character presentation and animation lifecycle regressions.

use super::animation::{
    desired_body_animation, drive_hero_locomotion, setup_hero_animation, HeroAnim, HeroGraph,
    RigMeshParts, BODY_ANIMATION_FADE_SECONDS,
};
use super::appearance::{
    apply_hero_skin, matte_character_materials, HeroAssets, HeroFullRig, HeroManifest,
    HeroSkinApplied,
};
use super::attachments::{carried_asset_spec, carried_bundle_transform, desired_tool, ToolKind};
use super::carts::{advanced_cart_wheel_angle, PORTER_CART_WHEEL_RADIUS};
use super::motion::{extrapolated_motion_target, visual_position_blend, HeroVisual};
use bevy::camera::visibility::ViewVisibility;
use bevy::gltf::{Gltf, GltfMaterialName};
use bevy::platform::collections::HashMap;
use bevy::prelude::*;
use shared::character::CharacterManifest;
use shared::components::{CharacterActivity, CharacterKind, CharacterMotion, HeroOutfit};
use shared::economy::{CarriedAppearance, CarriedLoad, PorterCartState};
use shared::player::HERO_MOVE_SPEED;

use bevy::ecs::system::RunSystemOnce;

fn queue_skin_primitive_despawn(
    mut commands: Commands,
    primitives: Query<Entity, With<GltfMaterialName>>,
) {
    for primitive in primitives.iter() {
        commands.entity(primitive).despawn();
    }
}

fn queue_animation_rig_despawn(
    mut commands: Commands,
    roots: Query<Entity, (With<CharacterKind>, With<HeroFullRig>)>,
) {
    for root in roots.iter() {
        commands.entity(root).despawn();
    }
}

fn animation_setup_test_app() -> App {
    let manifest = CharacterManifest::load().expect("shipped character manifest");
    let mut app = App::new();
    app.insert_resource(HeroManifest(manifest));
    app.init_resource::<Assets<Gltf>>();
    app.init_resource::<Assets<AnimationGraph>>();
    let handle = app
        .world_mut()
        .resource_mut::<Assets<AnimationGraph>>()
        .add(AnimationGraph::new());
    app.insert_resource(HeroAssets {
        graph: Some(HeroGraph {
            handle,
            body: HashMap::new(),
            face: HashMap::new(),
        }),
        ..default()
    });
    app
}

#[test]
fn skin_application_survives_a_same_frame_primitive_despawn() {
    let manifest = CharacterManifest::load().expect("shipped character manifest");
    let skin_material_name = manifest.skin.material.clone();
    let mut app = App::new();
    app.insert_resource(HeroManifest(manifest));
    app.init_resource::<HeroAssets>();
    app.init_resource::<Assets<StandardMaterial>>();

    let base = app
        .world_mut()
        .resource_mut::<Assets<StandardMaterial>>()
        .add(StandardMaterial::default());
    let primitive = app
        .world_mut()
        .spawn((GltfMaterialName(skin_material_name), MeshMaterial3d(base)))
        .id();
    let hero = app
        .world_mut()
        .spawn((
            HeroVisual { speed: 0.0 },
            HeroFullRig,
            HeroOutfit::default(),
        ))
        .add_child(primitive)
        .id();

    // A world teardown can queue the glTF child for despawn before deferred
    // commands are flushed. Material mutation must not leave a stale
    // entity command.
    app.add_systems(
        Update,
        (queue_skin_primitive_despawn, apply_hero_skin).chain_ignore_deferred(),
    );
    app.update();

    assert!(app.world().get_entity(primitive).is_err());
    assert!(app.world().entity(hero).contains::<HeroSkinApplied>());
}

#[test]
fn animation_setup_survives_a_same_frame_rig_despawn() {
    let mut app = animation_setup_test_app();
    let player = app.world_mut().spawn(AnimationPlayer::default()).id();
    let root = app
        .world_mut()
        .spawn((CharacterKind::Villager, HeroFullRig))
        .add_child(player)
        .id();

    // A disconnect/world teardown can invalidate both deferred insertion
    // targets after setup has already queried them. Fallible inserts must
    // make that cosmetic race a no-op rather than an EntityMut panic.
    app.add_systems(
        Update,
        (queue_animation_rig_despawn, setup_hero_animation).chain_ignore_deferred(),
    );
    app.update();

    assert!(app.world().get_entity(root).is_err());
    assert!(app.world().get_entity(player).is_err());
}

#[test]
fn motion_extrapolation_bridges_one_packet_but_never_runs_away() {
    let position = Vec3::new(5.0, 2.0, -3.0);
    let velocity = Vec3::new(10.0, 0.0, 0.0);
    assert_eq!(
        extrapolated_motion_target(position, velocity, 0.03, 1.0),
        Vec3::new(5.3, 2.0, -3.0)
    );
    assert_eq!(
        extrapolated_motion_target(position, velocity, 2.0, 1.0),
        Vec3::new(5.8, 2.0, -3.0),
        "a missing packet must not turn into long-running client authority"
    );
    assert_eq!(
        extrapolated_motion_target(position, Vec3::ZERO, 2.0, 1.0),
        position
    );
}

#[test]
fn accelerated_motion_keeps_the_same_visual_spatial_error() {
    let position = Vec3::new(5.0, 2.0, -3.0);
    let base_velocity = Vec3::new(HERO_MOVE_SPEED, 0.0, 0.0);
    let base_target = extrapolated_motion_target(position, base_velocity, 1.0, 1.0);
    let base_offset = base_target - position;

    for warp in [10.0_f32, 25.0, 100.0] {
        let target = extrapolated_motion_target(position, base_velocity * warp, 1.0, warp);
        assert!(
            target.distance(base_target) < 1.0e-5,
            "{warp}x extrapolated by {:?}, expected {:?}",
            target - position,
            base_offset,
        );

        let base_blend = visual_position_blend(1.0 / 60.0, 1.0);
        let warped_blend = visual_position_blend(1.0 / 60.0, warp);
        assert!(
            warped_blend > base_blend,
            "accelerated bodies must settle faster in real time"
        );
    }
}

#[test]
fn every_carried_appearance_has_an_authored_scene() {
    for appearance in CarriedAppearance::ALL {
        let spec = carried_asset_spec(appearance);
        assert!(spec.scene_path.ends_with(".glb#Scene0"));
        let asset_path = spec
            .scene_path
            .strip_suffix("#Scene0")
            .expect("carried asset scene suffix");
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("assets")
            .join(asset_path);
        assert!(path.is_file(), "missing carried asset: {}", path.display());
    }
    assert_eq!(
        carried_asset_spec(CarriedAppearance::FlourSack).scene_path,
        "game_assets/resources/carried/FlourSack.glb#Scene0"
    );
    assert_eq!(
        carried_asset_spec(CarriedAppearance::BreadBasket).scene_path,
        "game_assets/resources/carried/BreadBasket.glb#Scene0"
    );
    assert_eq!(
        carried_asset_spec(CarriedAppearance::WoolFleece).scene_path,
        "game_assets/resources/carried/WoolFleece.glb#Scene0"
    );
    assert_eq!(
        carried_asset_spec(CarriedAppearance::MeatHaunch).scene_path,
        "game_assets/resources/carried/MeatHaunch.glb#Scene0"
    );
    let wood = carried_bundle_transform();
    assert_eq!(wood.scale, Vec3::splat(1.35));
    assert_eq!(wood.translation.y, 0.0);
    assert_eq!(wood.translation.z, -0.08);
}

#[test]
fn porter_cart_asset_matches_the_runtime_node_and_clip_contract() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("assets/game_assets/props/HandCart.glb");
    let bytes = std::fs::read(&path).expect("shipped handcart GLB exists");
    assert_eq!(&bytes[0..4], b"glTF");
    let json_len = u32::from_le_bytes(bytes[12..16].try_into().unwrap()) as usize;
    let document: serde_json::Value =
        serde_json::from_slice(&bytes[20..20 + json_len]).expect("handcart GLB JSON parses");
    let nodes = document["nodes"].as_array().expect("handcart nodes");
    let node_index = |wanted: &str| {
        nodes
            .iter()
            .position(|node| node["name"].as_str() == Some(wanted))
            .unwrap_or_else(|| panic!("handcart node '{wanted}' is missing"))
    };
    let body = node_index("HandCartBody");
    let wheels = [node_index("HandCartWheelL"), node_index("HandCartWheelR")];
    for name in [
        "HandCart",
        "Anchor_Load.1",
        "Anchor_Load.2",
        "Anchor_GripL",
        "Anchor_GripR",
    ] {
        node_index(name);
    }

    let animations = document["animations"]
        .as_array()
        .expect("handcart animations");
    let animation = |wanted: &str| {
        animations
            .iter()
            .find(|animation| animation["name"].as_str() == Some(wanted))
            .unwrap_or_else(|| panic!("handcart clip '{wanted}' is missing"))
    };
    let pull_channels = animation("cart_pull")["channels"]
        .as_array()
        .expect("cart_pull channels");
    assert_eq!(pull_channels.len(), 1);
    assert_eq!(
        pull_channels[0]["target"]["node"].as_u64(),
        Some(body as u64)
    );
    assert_eq!(
        pull_channels[0]["target"]["path"].as_str(),
        Some("rotation")
    );

    let wheel_channels = animation("wheels_roll")["channels"]
        .as_array()
        .expect("wheels_roll channels");
    assert_eq!(wheel_channels.len(), 2);
    let mut wheel_targets: Vec<_> = wheel_channels
        .iter()
        .filter_map(|channel| channel["target"]["node"].as_u64())
        .map(|node| node as usize)
        .collect();
    wheel_targets.sort_unstable();
    let mut expected = wheels;
    expected.sort_unstable();
    assert_eq!(wheel_targets, expected);
}

#[test]
fn wheel_roll_uses_distance_and_the_authored_negative_direction() {
    let angle = advanced_cart_wheel_angle(0.0, PORTER_CART_WHEEL_RADIUS);
    assert!((angle - (std::f32::consts::TAU - 1.0)).abs() < 1e-5);
    let circumference = std::f32::consts::TAU * PORTER_CART_WHEEL_RADIUS;
    let revolution = advanced_cart_wheel_angle(0.0, circumference);
    assert!(revolution < 1e-5 || (std::f32::consts::TAU - revolution) < 1e-5);
}

#[test]
fn work_activity_selects_one_tool_and_carrying_selects_none() {
    assert_eq!(
        desired_tool(Some(CharacterActivity::Chopping), false),
        Some(ToolKind::Axe)
    );
    assert_eq!(
        desired_tool(Some(CharacterActivity::Farming), false),
        Some(ToolKind::Scythe)
    );
    assert_eq!(
        desired_tool(Some(CharacterActivity::Building), false),
        Some(ToolKind::Hammer)
    );
    assert_eq!(desired_tool(None, false), None);
    assert_eq!(desired_tool(Some(CharacterActivity::Chopping), true), None);
    assert_eq!(desired_tool(Some(CharacterActivity::Fishing), false), None);
}

#[test]
fn a_seated_passenger_does_not_walk_when_the_vessel_moves() {
    let idle = AnimationNodeIndex::new(0);
    let walk = AnimationNodeIndex::new(1);
    let sit = AnimationNodeIndex::new(2);
    let anim = HeroAnim {
        player: Entity::from_bits(1),
        idle: Some(idle),
        walk: Some(walk),
        build: None,
        chop: None,
        harvest: None,
        carry: None,
        pull: None,
        sit_idle: Some(sit),
        current_body: Some(walk),
        fading_body: None,
        body_fade_seconds: 0.0,
        paused: false,
        saved_weights: Vec::new(),
    };

    let (clip, speed, frozen) = desired_body_animation(
        &HeroVisual {
            speed: HERO_MOVE_SPEED,
        },
        &anim,
        Some(CharacterActivity::Sitting),
        false,
        false,
        None,
    );

    assert_eq!(clip, Some(sit));
    assert_eq!(speed, 1.0);
    assert!(!frozen);
}

#[test]
fn farming_uses_harvest_without_leaking_into_the_build_clip() {
    let mut world = World::new();
    world.insert_resource(Time::<()>::default());
    let idle = AnimationNodeIndex::new(0);
    let build = AnimationNodeIndex::new(1);
    let harvest = AnimationNodeIndex::new(2);
    let mut player = AnimationPlayer::default();
    player.play(idle).repeat().set_weight(1.0);
    let player_entity = world.spawn(player).id();
    world.spawn((
        HeroVisual { speed: 0.0 },
        HeroAnim {
            player: player_entity,
            idle: Some(idle),
            walk: None,
            build: Some(build),
            chop: None,
            harvest: Some(harvest),
            carry: None,
            pull: None,
            sit_idle: None,
            current_body: Some(idle),
            fading_body: None,
            body_fade_seconds: 0.0,
            paused: false,
            saved_weights: Vec::new(),
        },
        CharacterActivity::Farming,
    ));

    world.run_system_once(drive_hero_locomotion).unwrap();
    world
        .resource_mut::<Time<()>>()
        .advance_by(std::time::Duration::from_secs_f32(
            BODY_ANIMATION_FADE_SECONDS,
        ));
    world.run_system_once(drive_hero_locomotion).unwrap();

    let player = world.get::<AnimationPlayer>(player_entity).unwrap();
    assert_eq!(player.animation(harvest).unwrap().weight(), 1.0);
    assert!(player.animation(build).is_none());
    assert!(player.animation(idle).is_none());
    assert_eq!(player.playing_animations().count(), 1);
}

#[test]
fn a_loaded_stationary_villager_freezes_in_the_carry_pose() {
    let mut world = World::new();
    world.insert_resource(Time::<()>::default());
    let carry = AnimationNodeIndex::new(0);
    let idle = AnimationNodeIndex::new(1);
    let mut player = AnimationPlayer::default();
    player.play(idle).repeat().set_weight(1.0);
    let player_entity = world.spawn(player).id();
    world.spawn((
        HeroVisual { speed: 0.0 },
        HeroAnim {
            player: player_entity,
            idle: Some(idle),
            walk: None,
            build: None,
            chop: None,
            harvest: None,
            carry: Some(carry),
            pull: None,
            sit_idle: None,
            current_body: Some(idle),
            fading_body: None,
            body_fade_seconds: 0.0,
            paused: false,
            saved_weights: Vec::new(),
        },
        CarriedLoad {
            good: Some(shared::economy::Good::Wood),
            amount: 1,
            appearance: Some(CarriedAppearance::WoodBundle),
        },
    ));

    world.run_system_once(drive_hero_locomotion).unwrap();
    world
        .resource_mut::<Time<()>>()
        .advance_by(std::time::Duration::from_secs_f32(
            BODY_ANIMATION_FADE_SECONDS,
        ));
    world.run_system_once(drive_hero_locomotion).unwrap();

    let player = world.get::<AnimationPlayer>(player_entity).unwrap();
    let active_carry = player.animation(carry).unwrap();
    assert_eq!(active_carry.weight(), 1.0);
    assert_eq!(active_carry.speed(), 0.0);
    assert_eq!(active_carry.seek_time(), 0.0);
    assert!(player.animation(idle).is_none());
    assert_eq!(player.playing_animations().count(), 1);
}

/// A rig that no view sees (camera or shadow cascade, per its mesh
/// parts' ViewVisibility) must cost Bevy's animator nothing: only a clip
/// weight of exactly 0 makes animate_targets skip a rig, so pausing alone
/// still evaluated ~1000 unseen villagers every frame. On re-entry the
/// clip resumes at its saved weight and seek time.
#[test]
fn a_rig_no_view_can_see_stops_evaluating_and_resumes_where_it_left_off() {
    let mut world = World::new();
    world.insert_resource(Time::<()>::default());
    let idle = AnimationNodeIndex::new(0);
    let mut player = AnimationPlayer::default();
    player.play(idle).repeat().set_weight(1.0);
    let player_entity = world.spawn(player).id();
    // One mesh part, seen by no view. No camera exists, so the frustum
    // margin cannot rescue it either.
    let part = world.spawn(ViewVisibility::HIDDEN).id();
    world.spawn((
        HeroVisual { speed: 0.0 },
        HeroAnim {
            player: player_entity,
            idle: Some(idle),
            walk: None,
            build: None,
            chop: None,
            harvest: None,
            carry: None,
            pull: None,
            sit_idle: None,
            current_body: Some(idle),
            fading_body: None,
            body_fade_seconds: 0.0,
            paused: false,
            saved_weights: Vec::new(),
        },
        RigMeshParts(vec![part]),
        GlobalTransform::default(),
    ));

    world.run_system_once(drive_hero_locomotion).unwrap();
    {
        let player = world.get::<AnimationPlayer>(player_entity).unwrap();
        let active = player.animation(idle).unwrap();
        assert_eq!(active.weight(), 0.0, "an unseen rig must not be evaluated");
        assert!(active.is_paused());
    }

    // A view sees the part again (camera or a shadow cascade).
    *world.get_mut::<ViewVisibility>(part).unwrap() = ViewVisibility::VISIBLE;
    world.run_system_once(drive_hero_locomotion).unwrap();

    let player = world.get::<AnimationPlayer>(player_entity).unwrap();
    let active = player.animation(idle).unwrap();
    assert_eq!(active.weight(), 1.0, "the saved weight must come back");
    assert!(!active.is_paused());
    assert_eq!(player.playing_animations().count(), 1);
}

/// The visibility cull is only as good as its parts list: every mesh
/// primitive under a character root must be collected on that root, in
/// the frame it instantiates and for late arrivals alike.
#[test]
fn matte_pass_collects_every_rig_mesh_part_on_the_character_root() {
    let mut world = World::new();
    world.init_resource::<HeroAssets>();
    world.init_resource::<Assets<StandardMaterial>>();
    let material = world
        .resource_mut::<Assets<StandardMaterial>>()
        .add(StandardMaterial::default());
    let root = world.spawn(CharacterKind::Villager).id();
    let scene = world.spawn(ChildOf(root)).id();
    let part_a = world
        .spawn((MeshMaterial3d(material.clone()), ChildOf(scene)))
        .id();
    let part_b = world
        .spawn((MeshMaterial3d(material.clone()), ChildOf(scene)))
        .id();

    world.run_system_once(matte_character_materials).unwrap();
    let parts = world
        .get::<RigMeshParts>(root)
        .expect("primitives must be collected on the character root");
    assert_eq!(parts.0.len(), 2);
    assert!(parts.0.contains(&part_a) && parts.0.contains(&part_b));

    // A primitive that instantiates a frame later joins the same list.
    let part_c = world.spawn((MeshMaterial3d(material), ChildOf(scene))).id();
    world.run_system_once(matte_character_materials).unwrap();
    let parts = world.get::<RigMeshParts>(root).unwrap();
    assert_eq!(parts.0.len(), 3);
    assert!(parts.0.contains(&part_c));
}

#[test]
fn an_active_porter_cart_walks_even_before_visual_interpolation_reports_speed() {
    let mut world = World::new();
    world.insert_resource(Time::<()>::default());
    let idle = AnimationNodeIndex::new(0);
    let carry = AnimationNodeIndex::new(1);
    let pull = AnimationNodeIndex::new(2);
    let mut player = AnimationPlayer::default();
    player.play(idle).repeat().set_weight(1.0);
    let player_entity = world.spawn(player).id();
    world.spawn((
        HeroVisual { speed: 0.0 },
        HeroAnim {
            player: player_entity,
            idle: Some(idle),
            walk: None,
            build: None,
            chop: None,
            harvest: None,
            carry: Some(carry),
            pull: Some(pull),
            sit_idle: None,
            current_body: Some(idle),
            fading_body: None,
            body_fade_seconds: 0.0,
            paused: false,
            saved_weights: Vec::new(),
        },
        CarriedLoad {
            good: Some(shared::economy::Good::Wood),
            amount: 24,
            appearance: Some(CarriedAppearance::WoodBundle),
        },
        PorterCartState { load_slots: 2 },
        CharacterMotion::new(Vec3::new(0.0, 0.0, -HERO_MOVE_SPEED)),
    ));

    world.run_system_once(drive_hero_locomotion).unwrap();
    world
        .resource_mut::<Time<()>>()
        .advance_by(std::time::Duration::from_secs_f32(
            BODY_ANIMATION_FADE_SECONDS,
        ));
    world.run_system_once(drive_hero_locomotion).unwrap();

    let player = world.get::<AnimationPlayer>(player_entity).unwrap();
    let active_pull = player.animation(pull).unwrap();
    assert_eq!(active_pull.weight(), 1.0);
    assert_eq!(active_pull.speed(), 1.0);
    assert!(player.animation(carry).is_none());
    assert!(player.animation(idle).is_none());
    assert_eq!(player.playing_animations().count(), 1);
}
