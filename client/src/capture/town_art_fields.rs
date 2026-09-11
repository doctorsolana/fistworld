//! Explicit offline field migration for the fixed town art comparison.
//! Shares the authoritative terrain, grade, road and building fitting contract.
//! New acreage is checked against the same immutable permanent-prop recipe and
//! baked horizontal radii used by authority, plus already accepted farm claims.

use bevy::prelude::*;
use shared::components::{fit_farm_field_shapes_on_terrain, SettlementBuildingKind};
use shared::settlement_snapshot::TownSnapshot;
use shared::terrain::WorldTerrain;

pub(super) fn fit_fields(snapshot: &mut TownSnapshot, terrain: &WorldTerrain) {
    let db = shared::colliders::load_baked_collider_db_from_file("client/assets/colliders.bin")
        .expect("explicit town-field fitting requires canonical baked colliders");
    let radius_for = |kind: shared::props::PropKind| {
        let radius = |points: &[[f32; 3]]| {
            points
                .iter()
                .map(|p| p[0] * p[0] + p[2] * p[2])
                .fold(0.0, f32::max)
                .sqrt()
        };
        match db.entries.get(kind.id())? {
            shared::colliders::BakedCollider::ConvexHull { points } => {
                (points.len() >= 4).then(|| radius(points))
            }
            shared::colliders::BakedCollider::CompoundConvex { hulls } => hulls
                .iter()
                .filter(|h| h.len() >= 4)
                .map(|h| radius(h))
                .reduce(f32::max),
        }
    };
    let farms: Vec<_> = snapshot
        .buildings
        .iter()
        .filter(|b| b.kind == SettlementBuildingKind::Farmstead && b.construction.is_none())
        .map(|b| (b.position, b.rotation))
        .collect();
    let roads: Vec<_> = snapshot.roads.iter().map(|r| &r.road).collect();
    for (farm, rotation) in farms {
        let buildings: Vec<_> = snapshot
            .buildings
            .iter()
            .filter(|b| {
                b.position.xz().distance(farm.xz()) > 0.1
                    && b.position.xz().distance(farm.xz()) < 60.0
            })
            .map(|b| (b.kind, b.position, b.rotation))
            .collect();
        let permanent =
            shared::components::farm_field_permanent_obstacles(terrain, farm, radius_for);
        let shapes = fit_farm_field_shapes_on_terrain(
            terrain,
            farm,
            rotation,
            &buildings,
            &roads,
            |point| {
                permanent.iter().all(|(center, radius)| {
                    ((point - *center).abs() - Vec2::splat(0.55))
                        .max(Vec2::ZERO)
                        .length_squared()
                        >= radius * radius
                }) && !snapshot.fields.iter().any(|other| {
                    other.component.farmstead.xz().distance(farm.xz()) > 0.1
                        && other.component.contains_world_point(
                            point,
                            other.position,
                            other.rotation,
                            2.0,
                        )
                }) && !snapshot.buildings.iter().any(|building| {
                    building.yard.as_ref().is_some_and(|yard| {
                        yard.contains_world_point(point, building.position, building.rotation, 1.0)
                    })
                })
            },
        );
        for field in snapshot
            .fields
            .iter_mut()
            .filter(|field| field.component.farmstead.xz().distance(farm.xz()) < 0.1)
        {
            let Some(mut shape) = shapes.get(field.component.plot_index as usize).cloned() else {
                continue;
            };
            let canonical = SettlementBuildingKind::Farmstead
                .field_position_at(farm, rotation, field.component.plot_index)
                .unwrap();
            let delta =
                shared::rotation::world_to_local_xz(canonical.xz() - field.position.xz(), rotation);
            for section in &mut shape.sections {
                section.left += delta.x;
                section.right += delta.x;
                section.z += delta.y;
            }
            field.component.shape = Some(shape);
            field.component.layout_version = 2;
            field.refresh_footprint();
        }
    }
}
