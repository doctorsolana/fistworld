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
    app.add_systems(Update, tick);
    app
}
#[test]
fn wildlife_movement_and_decisions_are_identical_when_camera_moves_or_disconnects() {
    fn fixture() -> App {
        let mut app = app();
        app.world_mut().spawn(WorldTime::new_default());
        // More than the old 32-observed-horse ceiling, including distant herds.
        for i in 0..48 {
            let at = Vec3::new((i % 8) as f32 * 20., 0., (i / 8) as f32 * 20.);
            let horse = spawn_checked(app.world_mut(), at).unwrap();
            app.world_mut().get_mut::<WildHorse>(horse).unwrap().target = Some(at + Vec3::X * 5.);
        }
        app
    }
    let mut unseen = fixture();
    let mut seen = fixture();
    let camera = observer(seen.world_mut(), Vec3::ZERO);
    for frame in 0..900 {
        if frame == 150 {
            seen.world_mut()
                .get_mut::<PlayerPosition>(camera)
                .unwrap()
                .0 = Vec3::splat(5000.);
        }
        if frame == 300 {
            seen.world_mut().despawn(camera);
        }
        for app in [&mut unseen, &mut seen] {
            let world = app.world_mut();
            for mut time in world.query::<&mut WorldTime>().iter_mut(world) {
                time.seconds_in_cycle += 1. / 60.;
            }
            app.update();
        }
        fn snapshot(world: &mut World) -> Vec<(u64, Vec3, f32, HorseAnimation, u64, Option<Vec3>)> {
            let mut rows: Vec<_> = world
                .query::<(
                    &Horse,
                    &PlayerPosition,
                    &PlayerRotation,
                    &HorseAnimation,
                    &WildHorse,
                )>()
                .iter(world)
                .map(|(horse, position, rotation, animation, wild)| {
                    (
                        horse.id,
                        position.0,
                        rotation.0,
                        *animation,
                        wild.serial,
                        wild.target,
                    )
                })
                .collect();
            rows.sort_unstable_by_key(|row| row.0);
            rows
        }
        assert_eq!(
            snapshot(unseen.world_mut()),
            snapshot(seen.world_mut()),
            "camera changed wildlife at frame {frame}"
        );
    }
    let world = unseen.world_mut();
    assert_eq!(world.query::<&Horse>().iter(world).count(), 48);
    assert!(
        world
            .query::<(&PlayerPosition, &WildHorse)>()
            .iter(world)
            .all(|(p, wild)| p.0.distance_squared(wild.home) > 0.01),
        "an unobserved horse never moved"
    );
}
#[test]
fn observed_horse_moves_but_cannot_walk_through_another_horse() {
    let mut app = App::new();
    app.add_systems(Update, tick);
    let horse = spawn_checked(app.world_mut(), Vec3::ZERO).unwrap();
    let other = spawn_checked(app.world_mut(), Vec3::X * 4.).unwrap();
    app.world_mut().get_mut::<WildHorse>(horse).unwrap().target = Some(Vec3::X * 8.);
    for _ in 0..250 {
        app.update();
    }
    let at = app.world().get::<PlayerPosition>(horse).unwrap().0;
    assert!(at.x > 0.1, "horse never started walking");
    assert!(
        at.distance(app.world().get::<PlayerPosition>(other).unwrap().0) >= HORSE_CLEARANCE * 2.
    );
    assert!(
        !app.world()
            .get::<CharacterMotion>(horse)
            .unwrap()
            .is_moving()
    );
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
    assert!(
        population::candidates(bounds, 42)
            .iter()
            .all(|p| bounds.contains_xz(p.x, p.z))
    );
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
fn wildlife_colliders_load_without_observers_and_camera_does_not_change_the_footprint() {
    use crate::collision::{
        building_index::BuildingSpatialIndex,
        library::{DerivedColliderLibrary, StaticColliders},
        streaming::{ColliderStreamingState, update_static_collider_streaming},
    };
    let mut app = App::new();
    app.init_resource::<WorldTerrain>()
        .init_resource::<BuildingSpatialIndex>()
        .init_resource::<StaticColliders>()
        .init_resource::<ColliderStreamingState>()
        .insert_resource(DerivedColliderLibrary {
            by_kind: Default::default(),
        })
        .add_systems(Update, update_static_collider_streaming);
    let horse = app
        .world_mut()
        .spawn((Horse { id: 1, rider: None }, PlayerPosition(Vec3::ZERO)))
        .id();
    for _ in 0..3 {
        app.update();
    }
    let before = app
        .world()
        .resource::<StaticColliders>()
        .loaded_chunks
        .clone();
    assert_eq!(
        before.len(),
        9,
        "one neighboring chunk covers the local grazing range"
    );
    let camera = observer(app.world_mut(), Vec3::new(1500., 0., 1500.));
    for _ in 0..3 {
        app.update();
    }
    assert_eq!(
        app.world().resource::<StaticColliders>().loaded_chunks,
        before
    );
    app.world_mut().despawn(horse);
    app.update();
    assert!(
        app.world()
            .resource::<StaticColliders>()
            .loaded_chunks
            .is_empty(),
        "the remote camera must not keep authoritative blockers resident"
    );
    app.world_mut().despawn(camera);
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

#[test]
fn a_horse_retains_its_target_until_the_entire_swept_footprint_has_blockers() {
    use crate::collision::library::StaticColliders;
    use shared::terrain::{CHUNK_SIZE, ChunkCoord};
    let mut app = app();
    let start = Vec3::new(CHUNK_SIZE - 0.5, 0., 10.);
    let target = start + Vec3::X * 4.;
    let horse = spawn_checked(app.world_mut(), start).unwrap();
    app.world_mut().get_mut::<WildHorse>(horse).unwrap().target = Some(target);
    let mut colliders = StaticColliders::default();
    colliders.loaded_chunks.insert(ChunkCoord::new(0, 0));
    app.insert_resource(colliders);
    app.update();
    assert_eq!(app.world().get::<PlayerPosition>(horse).unwrap().0, start);
    assert_eq!(
        app.world().get::<WildHorse>(horse).unwrap().target,
        Some(target)
    );
    app.world_mut()
        .resource_mut::<StaticColliders>()
        .loaded_chunks
        .insert(ChunkCoord::new(1, 0));
    app.update();
    assert!(app.world().get::<PlayerPosition>(horse).unwrap().0.x > start.x);
}
