use super::placement::MAX_HORSES;
use super::*;
use bevy::prelude::*;
use lightyear::prelude::PeerId;
use shared::{components::*, terrain::WorldTerrain};

fn observer(world: &mut World, at: Vec3) -> Entity {
    world
        .spawn((
            Player {
                client_id: PeerId::Netcode(1),
            },
            PlayerPosition(at),
        ))
        .id()
}
fn app() -> App {
    let mut app = App::new();
    app.add_systems(Update, (update_observation, tick).chain());
    app
}
#[test]
fn unobserved_horses_keep_identity_position_and_generate_no_snapshot_changes() {
    let mut app = app();
    let horse = spawn_checked(app.world_mut(), Vec3::ZERO).unwrap();
    app.world_mut().get_mut::<WildHorse>(horse).unwrap().target = Some(Vec3::X * 5.);
    let original = *app.world().get::<Horse>(horse).unwrap();
    app.update();
    app.world_mut().clear_trackers();
    for _ in 0..120 {
        app.update();
    }
    assert_eq!(app.world().get::<Horse>(horse), Some(&original));
    assert_eq!(
        app.world().get::<PlayerPosition>(horse).unwrap().0,
        Vec3::ZERO
    );
    assert!(!app
        .world()
        .entity(horse)
        .get_ref::<PlayerPosition>()
        .unwrap()
        .is_changed());
    assert!(!app
        .world()
        .entity(horse)
        .get_ref::<CharacterMotion>()
        .unwrap()
        .is_changed());
}
#[test]
fn observation_is_capped_and_demotion_preserves_the_same_horse() {
    let mut app = app();
    for i in 0..48 {
        spawn_checked(
            app.world_mut(),
            Vec3::new((i % 8) as f32 * 5., 0., (i / 8) as f32 * 5.),
        )
        .unwrap();
    }
    let player = observer(app.world_mut(), Vec3::ZERO);
    for _ in 0..32 {
        app.update();
    }
    assert_eq!(
        app.world_mut()
            .query_filtered::<Entity, With<ActiveWildHorse>>()
            .iter(app.world())
            .count(),
        MAX_ACTIVE_HORSES
    );
    let before: Vec<_> = app
        .world_mut()
        .query::<(Entity, &Horse, &PlayerPosition)>()
        .iter(app.world())
        .map(|(e, h, p)| (e, *h, p.0))
        .collect();
    app.world_mut().get_mut::<PlayerPosition>(player).unwrap().0 = Vec3::splat(5000.);
    for _ in 0..32 {
        app.update();
    }
    assert_eq!(
        app.world_mut()
            .query_filtered::<Entity, With<ActiveWildHorse>>()
            .iter(app.world())
            .count(),
        0
    );
    for (e, h, p) in before {
        assert_eq!(app.world().get::<Horse>(e), Some(&h));
        assert_eq!(app.world().get::<PlayerPosition>(e).unwrap().0, p);
    }
}
#[test]
fn observed_horse_moves_but_cannot_walk_through_another_horse() {
    let mut app = App::new();
    app.add_systems(Update, tick);
    let horse = spawn_checked(app.world_mut(), Vec3::ZERO).unwrap();
    let other = spawn_checked(app.world_mut(), Vec3::X * 4.).unwrap();
    app.world_mut().entity_mut(horse).insert(ActiveWildHorse);
    app.world_mut().get_mut::<WildHorse>(horse).unwrap().target = Some(Vec3::X * 8.);
    for _ in 0..250 {
        app.update();
    }
    let at = app.world().get::<PlayerPosition>(horse).unwrap().0;
    assert!(at.x > 0.1, "horse never started walking");
    assert!(
        at.distance(app.world().get::<PlayerPosition>(other).unwrap().0) >= HORSE_CLEARANCE * 2.
    );
    assert!(!app
        .world()
        .get::<CharacterMotion>(horse)
        .unwrap()
        .is_moving());
}
#[test]
fn initial_herd_candidates_are_deterministic_and_seeded() {
    let bounds = shared::map::MapBounds {
        min: [-512., -512.],
        max: [512., 512.],
    };
    assert_eq!(
        population::candidates(bounds, 42),
        population::candidates(bounds, 42)
    );
    assert_ne!(
        population::candidates(bounds, 42),
        population::candidates(bounds, 43)
    );
    assert!(population::candidates(bounds, 42)
        .iter()
        .all(|p| bounds.contains_xz(p.x, p.z)));
}
#[test]
fn spawning_rejects_non_finite_positions_overlap_and_population_overflow() {
    let mut world = World::new();
    assert!(spawn_checked(&mut world, Vec3::splat(f32::NAN)).is_err());
    spawn_checked(&mut world, Vec3::ZERO).unwrap();
    assert!(spawn_checked(&mut world, Vec3::X).is_err());
    for i in 1..MAX_HORSES {
        spawn_checked(&mut world, Vec3::X * i as f32 * 5.).unwrap();
    }
    assert!(spawn_checked(&mut world, Vec3::X * 5000.).is_err());
}

#[test]
#[ignore = "explicit real-map habitat audit; run once per map with CITYSIM_MAP_ID"]
fn meadow_population_uses_real_terrain_and_is_idempotent() {
    let mut world = World::new();
    world.init_resource::<WorldTerrain>();
    populate(&mut world);
    let mut first: Vec<_> = world
        .query::<(&Horse, &PlayerPosition)>()
        .iter(&world)
        .map(|(h, p)| (h.id, p.0))
        .collect();
    assert!(!first.is_empty(), "no meadow habitat populated on this map");
    assert!(first.len() <= 96);
    for (_, p) in &first {
        assert!(population::habitat(world.resource::<WorldTerrain>(), *p));
    }
    populate(&mut world);
    assert_eq!(world.query::<&Horse>().iter(&world).count(), first.len());
    first.sort_by(|a, b| a.1.length_squared().total_cmp(&b.1.length_squared()));
    println!(
        "{} horses; nearest to origin: {:?}",
        first.len(),
        &first[..first.len().min(8)]
    );
}

#[test]
fn distant_wildlife_does_not_expand_the_collider_streaming_footprint() {
    use crate::collision::{
        building_index::BuildingSpatialIndex,
        library::{DerivedColliderLibrary, StaticColliders},
        streaming::{update_static_collider_streaming, ColliderStreamingState},
    };
    let mut app = App::new();
    app.init_resource::<WorldTerrain>();
    app.init_resource::<BuildingSpatialIndex>();
    app.init_resource::<StaticColliders>();
    app.init_resource::<ColliderStreamingState>();
    app.insert_resource(DerivedColliderLibrary {
        by_kind: Default::default(),
    });
    app.add_systems(Update, update_static_collider_streaming);
    let horse = app
        .world_mut()
        .spawn((Horse { id: 1, rider: None }, PlayerPosition(Vec3::ZERO)))
        .id();
    app.update();
    assert!(app
        .world()
        .resource::<StaticColliders>()
        .loaded_chunks
        .is_empty());
    app.world_mut().entity_mut(horse).insert(ActiveWildHorse);
    app.update();
    assert!(!app
        .world()
        .resource::<StaticColliders>()
        .loaded_chunks
        .is_empty());
    app.world_mut()
        .entity_mut(horse)
        .remove::<ActiveWildHorse>();
    app.update();
    assert!(app
        .world()
        .resource::<StaticColliders>()
        .loaded_chunks
        .is_empty());
}

#[test]
fn provisioned_mounts_share_ids_without_consuming_wildlife_capacity() {
    let mut world = World::new();
    for i in 0..placement::MAX_HORSES {
        let id = allocate_id(&mut world);
        world.spawn((
            Horse {
                id,
                rider: Some(PersonId(i as u64)),
            },
            PlayerPosition(Vec3::new(1000. + i as f32 * 4., 0., 0.)),
        ));
    }
    let wild = spawn_checked(&mut world, Vec3::ZERO).unwrap();
    assert_eq!(
        world.get::<Horse>(wild).unwrap().id,
        placement::MAX_HORSES as u64 + 1
    );
    assert!(
        spawn_checked(&mut world, Vec3::new(1000., 0., 0.)).is_err(),
        "wildlife must still leave room beside an army mount"
    );
}
