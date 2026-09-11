//! One-time authoritative agricultural fitting against existing land claims.

use crate::collision::library::DerivedColliderLibrary;
use bevy::prelude::*;
use shared::components::{FarmFieldShape, SettlementBuildingKind, VillageRoad};
use shared::terrain::WorldTerrain;

pub(super) fn fit(
    terrain: &WorldTerrain,
    farm: Vec3,
    rotation: f32,
    buildings: &[(SettlementBuildingKind, Vec3, f32)],
    roads: &[&VillageRoad],
    derived: Option<&DerivedColliderLibrary>,
    mut land_is_clear: impl FnMut(Vec2) -> bool,
) -> [FarmFieldShape; 2] {
    // The source recipe exists before any player observes a chunk. Streaming
    // determines which collision instances are embodied, never which land a
    // farm may claim. Trees/trunks are cleared by the existing farm reservation;
    // decorative nonblocking plants must not become oversized crop obstacles.
    let permanent = shared::components::farm_field_permanent_obstacles(terrain, farm, |kind| {
        derived
            .and_then(|library| library.by_kind.get(&kind))
            .map(|collider| collider.horizontal_radius)
    });
    shared::components::fit_farm_field_shapes_on_terrain(
        terrain,
        farm,
        rotation,
        buildings,
        roads,
        |point| {
            land_is_clear(point)
                && permanent.iter().all(|(center, radius)| {
                    let outside = ((point - *center).abs() - Vec2::splat(0.55)).max(Vec2::ZERO);
                    outside.length_squared() >= radius * radius
                })
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::collision::library::{DerivedCollider, StaticColliderInstance, StaticColliders};
    use shared::components::{
        BuildingId, FarmField, PlayerPosition, PlayerRotation, SettlementBuilding,
    };
    use shared::props::PropKind;

    fn accepted_fields(observed: bool) -> Vec<FarmFieldShape> {
        let farm = Vec3::new(1700., 80., 0.);
        let mut terrain = WorldTerrain::default();
        terrain.apply_flatten_rect(farm, Vec2::splat(25.), 0., 4.);
        let mut app = App::new();
        app.insert_resource(terrain);
        app.insert_resource(DerivedColliderLibrary {
            by_kind: std::collections::HashMap::from([(
                PropKind::SmallRockA,
                DerivedCollider {
                    horizontal_radius: 1.,
                },
            )]),
        });
        let mut streamed = StaticColliders::default();
        if observed {
            // An unrelated streamed instance must not alter the recipe-derived
            // parcel. The old observer-dependent callback rejected this land.
            for (i, x) in [-7., -4., -1., 2., 5., 8.].into_iter().enumerate() {
                let position = farm + Vec3::new(x, 0., 9.);
                let cell = (
                    (position.x / 16.).floor() as i32,
                    (position.z / 16.).floor() as i32,
                );
                streamed.instances.insert(
                    i as u32,
                    StaticColliderInstance {
                        kind: PropKind::SmallRockA,
                        position,
                        scale: 3.,
                        rotation: Quat::IDENTITY,
                        cell,
                    },
                );
                streamed.cells.entry(cell).or_default().push(i as u32);
            }
        }
        app.insert_resource(streamed);
        app.add_systems(Update, super::super::ensure_farm_fields);
        app.world_mut().spawn((
            BuildingId(1),
            PlayerPosition(farm),
            PlayerRotation(0.),
            SettlementBuilding {
                kind: SettlementBuildingKind::Farmstead,
                settlement: "Test".into(),
                owner: None,
                quality: 1.,
                workers: vec![],
            },
        ));
        app.update();
        let mut query = app.world_mut().query::<&FarmField>();
        let mut fields: Vec<_> = query
            .iter(app.world())
            .map(|f| (f.plot_index, f.shape.clone().unwrap()))
            .collect();
        fields.sort_by_key(|(plot, _)| *plot);
        fields.into_iter().map(|(_, shape)| shape).collect()
    }

    #[test]
    fn streaming_an_observer_cannot_change_accepted_crop_geometry() {
        let unobserved = accepted_fields(false);
        assert_eq!(unobserved.len(), 2);
        assert!(unobserved.iter().any(FarmFieldShape::is_valid));
        assert_eq!(accepted_fields(true), unobserved);
    }

    #[test]
    fn adjacent_farms_fit_in_stable_order_without_claiming_each_others_crops() {
        use shared::components::AttachedTo;
        let farm = Vec3::new(1700., 80., 0.);
        let mut terrain = WorldTerrain::default();
        terrain.apply_flatten_rect(farm, Vec2::splat(65.), 0., 4.);
        let mut app = App::new();
        app.insert_resource(terrain);
        app.add_systems(Update, super::super::ensure_farm_fields);
        for (id, offset) in [(2, 30.), (1, 0.)] {
            let p = farm + Vec3::X * offset;
            app.world_mut().spawn((
                BuildingId(id),
                PlayerPosition(p),
                PlayerRotation(0.),
                SettlementBuilding {
                    kind: SettlementBuildingKind::Farmstead,
                    settlement: "Test".into(),
                    owner: None,
                    quality: 1.,
                    workers: vec![],
                },
            ));
        }
        app.update();
        let mut query = app
            .world_mut()
            .query::<(&FarmField, &PlayerPosition, &PlayerRotation, &AttachedTo)>();
        let fields: Vec<_> = query
            .iter(app.world())
            .map(|(f, p, r, a)| (f.clone(), p.0, r.0, a.0))
            .collect();
        assert_eq!(fields.len(), 4);
        assert!(fields.iter().all(|(f, _, _, _)| f.layout_version == 1));
        for (field, p, r, id) in &fields {
            for local in field.accepted_shape().boundary_points() {
                let world = p.xz() + shared::rotation::local_to_world_xz(local, *r);
                assert!(!fields.iter().any(|(other, op, or, oid)| oid != id
                    && other.contains_world_point(world, *op, *or, 0.5)));
            }
        }
        app.update();
        let unchanged: Vec<_> = query
            .iter(app.world())
            .map(|(f, p, r, a)| (f.clone(), p.0, r.0, a.0))
            .collect();
        assert_eq!(
            fields
                .iter()
                .map(|(f, p, r, a)| (&f.shape, p, r, a))
                .collect::<Vec<_>>(),
            unchanged
                .iter()
                .map(|(f, p, r, a)| (&f.shape, p, r, a))
                .collect::<Vec<_>>(),
            "accepted crop geometry must not refit on ordinary ticks"
        );
    }

    #[test]
    fn new_fence_waits_for_a_passing_body_without_resurveying_crop_land() {
        use shared::components::{AttachedTo, CharacterKind};
        let farm = Vec3::new(1700., 80., 0.);
        let field_position = farm + Vec3::new(-4.45, 0., 9.);
        let mut app = App::new();
        app.insert_resource(WorldTerrain::default());
        app.add_systems(Update, super::super::ensure_farm_fields);
        let shape = FarmFieldShape::legacy_rectangle();
        let field = app
            .world_mut()
            .spawn((
                FarmField {
                    settlement: "Test".into(),
                    farmstead: farm,
                    plot_index: 0,
                    quality: 1.,
                    shape: Some(shape.clone()),
                    layout_version: 1,
                },
                PlayerPosition(field_position),
                PlayerRotation(0.),
                AttachedTo(BuildingId(1)),
            ))
            .id();
        let passer = app
            .world_mut()
            .spawn((
                CharacterKind::Villager,
                PlayerPosition(field_position + Vec3::X * -4.),
            ))
            .id();
        app.update();
        assert_eq!(
            app.world().get::<FarmField>(field).unwrap().layout_version,
            1
        );
        app.world_mut().get_mut::<PlayerPosition>(passer).unwrap().0 += Vec3::X * 3.;
        app.update();
        let accepted = app.world().get::<FarmField>(field).unwrap();
        assert_eq!(accepted.layout_version, 2);
        assert_eq!(accepted.shape, Some(shape));
        assert!(!accepted.ground_obstacles(field_position, 0.).is_empty());
    }
}
