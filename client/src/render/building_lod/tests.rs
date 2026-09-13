use super::assets::Library;
use super::*;
use selection::*;
use std::sync::Arc;

#[test]
fn building_catalog_covers_every_authored_building_and_reduces_geometry() {
    let definitions = assets::definitions();
    assert_eq!(
        definitions.len(),
        shared::building::BuildingType::all().len()
    );
    for kind in shared::building::BuildingType::all() {
        let source = kind.scene_path().unwrap().split('#').next().unwrap();
        let asset = definitions
            .iter()
            .find(|d| d.source == source)
            .expect(source);
        assert!(asset.radius.is_finite() && asset.radius > 0.0);
        assert!(asset.triangles[1] < asset.triangles[0]);
        assert!(!asset.primitives.is_empty());
        assert!(std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("assets")
            .join(&asset.library)
            .is_file());
    }
}

#[test]
fn building_lod_hysteresis_prevents_boundary_chatter_and_handles_large_zoom_jumps() {
    for pixels in [DETAIL_PIXELS - 1.0, DETAIL_PIXELS, DETAIL_PIXELS + 1.0] {
        assert_eq!(select_level(pixels, 0), 0);
        assert_eq!(select_level(pixels, 1), 1);
    }
    for pixels in [HIDE_PIXELS - 0.1, HIDE_PIXELS + 0.1] {
        assert_eq!(select_level(pixels, 1), 1);
        assert_eq!(select_level(pixels, 2), 2);
    }
    assert_eq!(select_level(2.0, 0), HIDDEN);
    assert_eq!(select_level(1000.0, HIDDEN), 0);
}

#[test]
fn zooming_away_from_the_same_building_reduces_detail_and_resolution_matters() {
    let projection = Projection::Perspective(PerspectiveProjection::default());
    let near = GlobalTransform::from(Transform::from_xyz(0.0, 0.0, 20.0));
    let far = GlobalTransform::from(Transform::from_xyz(0.0, 0.0, 400.0));
    let close_pixels = projected_diameter(Vec3::ZERO, 6.0, &near, &projection, 1080.0);
    let far_pixels = projected_diameter(Vec3::ZERO, 6.0, &far, &projection, 1080.0);
    assert_eq!(select_level(close_pixels, 1), 0);
    assert_eq!(select_level(far_pixels, 0), 1);
    assert!(
        (projected_diameter(Vec3::ZERO, 6.0, &far, &projection, 2160.0) - far_pixels * 2.0).abs()
            < 0.001
    );
}

#[test]
fn mesh_swaps_preserve_animated_nodes_materials_and_return_to_the_exact_source() {
    use bevy::pbr::{check_entities_needing_specialization, EntitiesNeedingSpecialization};

    bevy::tasks::ComputeTaskPool::get_or_init(bevy::tasks::TaskPool::new);
    let mut app = App::new();
    app.init_resource::<Time>()
        .init_resource::<Assets<Mesh>>()
        .init_resource::<Assets<StandardMaterial>>()
        .init_resource::<EntitiesNeedingSpecialization<StandardMaterial>>()
        // Register the real renderer detector first: production ordering must
        // still put mesh swaps before its retained-bin invalidation snapshot.
        .add_systems(
            PostUpdate,
            check_entities_needing_specialization::<StandardMaterial>,
        );
    configure_selection(&mut app);
    let meshes = &mut app.world_mut().resource_mut::<Assets<Mesh>>();
    let handles = [
        meshes.add(Cuboid::new(1.0, 2.0, 0.1)),
        meshes.add(Cuboid::new(1.0, 2.0, 0.09)),
    ];
    let material = app
        .world_mut()
        .resource_mut::<Assets<StandardMaterial>>()
        .add(StandardMaterial::default());
    let door_pose = Transform::from_xyz(1.2, 0.15, -2.0).with_rotation(Quat::from_rotation_y(1.4));
    let door = app
        .world_mut()
        .spawn((
            Name::new("DoorPivot"),
            Mesh3d(handles[0].clone()),
            MeshMaterial3d(material.clone()),
            door_pose,
            Visibility::Inherited,
        ))
        .id();
    let root = app
        .world_mut()
        .spawn((
            GlobalTransform::IDENTITY,
            Visibility::Inherited,
            BuildingLod {
                library: Arc::new(Library {
                    gltf: Handle::default(),
                    meshes: vec![handles.clone()],
                    center: Vec3::ZERO,
                    radius: 5.0,
                    triangles: [12, 6],
                }),
                bindings: vec![(door, 0)],
                level: 0,
                ready: true,
                failed: false,
                needs_refresh: false,
                visibility_before_hide: None,
            },
        ))
        .id();
    app.world_mut().entity_mut(door).insert(ChildOf(root));
    let camera = app
        .world_mut()
        .spawn((
            crate::camera_rts::CommanderCamera::default(),
            Camera {
                viewport: Some(bevy::camera::Viewport {
                    physical_size: UVec2::new(1200, 1000),
                    ..default()
                }),
                ..default()
            },
            Projection::Perspective(PerspectiveProjection::default()),
            GlobalTransform::from(Transform::from_xyz(0.0, 0.0, 400.0)),
        ))
        .id();
    let entities = app.world().entities().len();
    for (distance, level) in [(400.0, 1), (8000.0, HIDDEN), (400.0, 1), (20.0, 0)] {
        app.world_mut()
            .entity_mut(camera)
            .insert(GlobalTransform::from(Transform::from_xyz(
                0.0, 0.0, distance,
            )));
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs_f32(0.11));
        app.update();
        assert_eq!(app.world().get::<BuildingLod>(root).unwrap().level, level);
        if level != HIDDEN {
            assert_eq!(app.world().get::<Mesh3d>(door).unwrap().0, handles[level]);
        }
        let changed = &app
            .world()
            .resource::<EntitiesNeedingSpecialization<StandardMaterial>>()
            .changed;
        assert_eq!(
            changed.contains(&door),
            level != HIDDEN,
            "the renderer must receive mesh swaps in the same frame, never one frame later"
        );
        assert_eq!(
            *app.world().get::<Visibility>(root).unwrap(),
            if level == HIDDEN {
                Visibility::Hidden
            } else {
                Visibility::Inherited
            }
        );
        assert_eq!(*app.world().get::<Transform>(door).unwrap(), door_pose);
        assert_eq!(
            app.world()
                .get::<MeshMaterial3d<StandardMaterial>>(door)
                .unwrap()
                .0,
            material
        );
        assert_eq!(
            *app.world().get::<Visibility>(door).unwrap(),
            Visibility::Inherited
        );
        assert_eq!(app.world().get::<ChildOf>(door).unwrap().parent(), root);
        assert_eq!(app.world().entities().len(), entities);
    }
    // Stationary selection must not keep invalidating the retained render bins.
    app.world_mut()
        .resource_mut::<Time>()
        .advance_by(std::time::Duration::from_secs_f32(0.11));
    app.update();
    assert!(app
        .world()
        .resource::<EntitiesNeedingSpecialization<StandardMaterial>>()
        .changed
        .is_empty());
    // Hiding/revealing must not reveal a root another feature already hid.
    app.world_mut().entity_mut(root).insert(Visibility::Hidden);
    for level in [HIDDEN, 0] {
        app.world_mut()
            .entity_mut(root)
            .insert(BuildingLodOverride(level));
        app.world_mut()
            .resource_mut::<Time>()
            .advance_by(std::time::Duration::from_secs_f32(0.11));
        app.update();
        assert_eq!(
            *app.world().get::<Visibility>(root).unwrap(),
            Visibility::Hidden
        );
    }
    // A library that is still loading, or failed to load, leaves the authored mesh alone.
    app.world_mut().get_mut::<BuildingLod>(root).unwrap().ready = false;
    app.world_mut()
        .entity_mut(root)
        .insert(BuildingLodOverride(2));
    app.update();
    assert_eq!(app.world().get::<Mesh3d>(door).unwrap().0, handles[0]);
}
