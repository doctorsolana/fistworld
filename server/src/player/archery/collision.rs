//! Cached baked convex hulls, reused for firing lanes and swept arrow impacts.
//! Building broad phase rebuilds on edits; streamed props reuse their spatial hash.
use crate::collision::{
    building_index::{BuildingSpatialIndex, IndexedBuilding},
    library::StaticColliders,
};
use bevy::prelude::*;
use parry3d::{
    na::{Point3, Vector3},
    query::Ray,
    shape::{Capsule, SharedShape},
};
use std::collections::HashMap;

#[derive(Resource)]
pub struct ArrowObstacles {
    shapes: HashMap<String, Vec<SharedShape>>,
    version: Option<u64>,
    buildings: Vec<IndexedBuilding>,
    cells: HashMap<(i32, i32), Vec<usize>>,
    defenses: Vec<shared::components::FortificationSegment>,
    defense_cells: HashMap<(i32, i32), Vec<usize>>,
}
impl Default for ArrowObstacles {
    fn default() -> Self {
        static SHAPES: std::sync::OnceLock<HashMap<String, Vec<SharedShape>>> =
            std::sync::OnceLock::new();
        let shapes = SHAPES
            .get_or_init(|| {
                let mut shapes = HashMap::new();
                let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../client/assets/colliders.bin");
                let db = shared::colliders::load_baked_collider_db_from_file(path)
                    .expect("arrow collision database");
                for (key, baked) in db.entries {
                    let hulls = match baked {
                        shared::colliders::BakedCollider::ConvexHull { points } => vec![points],
                        shared::colliders::BakedCollider::CompoundConvex { hulls } => hulls,
                    };
                    shapes.insert(
                        key,
                        hulls
                            .into_iter()
                            .filter_map(|h| {
                                SharedShape::convex_hull(
                                    &h.into_iter().map(Point3::from).collect::<Vec<_>>(),
                                )
                            })
                            .collect(),
                    );
                }
                shapes
            })
            .clone();
        Self {
            shapes,
            version: None,
            buildings: Vec::new(),
            cells: HashMap::new(),
            defenses: Vec::new(),
            defense_cells: HashMap::new(),
        }
    }
}
fn cell(p: Vec2) -> (i32, i32) {
    ((p.x / 16.).floor() as i32, (p.y / 16.).floor() as i32)
}

/// Update once on construction, upgrade or removal, including when every archer
/// has died but their arrows are still in flight.
pub fn sync_defense_arrow_obstacles(
    mut obstacles: ResMut<ArrowObstacles>,
    walls: Query<&shared::components::FortificationSegment>,
    changed: Query<(), Changed<shared::components::FortificationSegment>>,
    mut removed: RemovedComponents<shared::components::FortificationSegment>,
) {
    let removed = removed.read().count() > 0;
    if changed.is_empty() && !removed {
        return;
    }
    obstacles.defenses.clear();
    obstacles.defense_cells.clear();
    for wall in walls.iter().filter(|wall| wall.complete) {
        let index = obstacles.defenses.len();
        // A turned gateway's jamb/lintel corners extend beyond its endpoints.
        // Sum the local extents for a conservative broad phase at every yaw.
        let margin = Vec2::splat(
            wall.material.gate_post_width() + 0.1 + wall.material.thickness().max(0.9) * 0.5,
        );
        let lo = cell(wall.start.xz().min(wall.end.xz()) - margin);
        let hi = cell(wall.start.xz().max(wall.end.xz()) + margin);
        for x in lo.0..=hi.0 {
            for z in lo.1..=hi.1 {
                obstacles
                    .defense_cells
                    .entry((x, z))
                    .or_default()
                    .push(index);
            }
        }
        obstacles.defenses.push(wall.clone());
    }
}
impl ArrowObstacles {
    pub fn sync(&mut self, index: Option<&BuildingSpatialIndex>) {
        let Some(index) = index else {
            return;
        };
        if self.version == Some(index.version) {
            return;
        }
        self.version = Some(index.version);
        self.buildings.clear();
        self.cells.clear();
        self.buildings.extend_from_slice(index.snapshot());
        for (i, b) in self.buildings.iter().enumerate() {
            let def = b.building_type.definition();
            let radius = def.footprint.length().max(def.height) + 2.;
            let lo = cell(b.position.xz() - Vec2::splat(radius));
            let hi = cell(b.position.xz() + Vec2::splat(radius));
            for x in lo.0..=hi.0 {
                for z in lo.1..=hi.1 {
                    self.cells.entry((x, z)).or_default().push(i);
                }
            }
        }
    }
    fn hull_hit(
        &self,
        id: &str,
        origin: Vec3,
        rotation: Quat,
        scale: f32,
        a: Vec3,
        b: Vec3,
    ) -> Option<f32> {
        let inverse = rotation.inverse();
        let a = inverse * (a - origin) / scale;
        let b = inverse * (b - origin) / scale;
        let ray = ray(a, b);
        self.shapes
            .get(id)?
            .iter()
            .filter_map(|s| s.cast_local_ray(&ray, 1., true))
            .min_by(f32::total_cmp)
    }
    pub fn hit(&self, a: Vec3, b: Vec3, props: Option<&StaticColliders>) -> Option<f32> {
        let mut result: Option<f32> = None;
        let mut accept = |t: Option<f32>| {
            if let Some(t) = t {
                if result.is_none_or(|old| t < old) {
                    result = Some(t);
                }
            }
        };
        let lo = cell(a.xz().min(b.xz()));
        let hi = cell(a.xz().max(b.xz()));
        for x in lo.0..=hi.0 {
            for z in lo.1..=hi.1 {
                if let Some(entries) = self.defense_cells.get(&(x, z)) {
                    for &index in entries {
                        accept(super::defenses::hit(&self.defenses[index], a, b));
                    }
                }
                if let Some(entries) = self.cells.get(&(x, z)) {
                    for &i in entries {
                        let building = &self.buildings[i];
                        accept(self.hull_hit(
                            building.building_type.id(),
                            building.position,
                            Quat::from_rotation_y(building.rotation),
                            1.,
                            a,
                            b,
                        ));
                    }
                }
            }
        }
        if let Some(props) = props {
            // Props are indexed by centre. Adjacent cells cover their baked radii.
            for x in lo.0 - 1..=hi.0 + 1 {
                for z in lo.1 - 1..=hi.1 + 1 {
                    if let Some(entries) = props.cells.get(&(x, z)) {
                        for id in entries {
                            if let Some(p) = props.instances.get(id) {
                                accept(self.hull_hit(
                                    p.kind.id(),
                                    p.position,
                                    p.rotation,
                                    p.scale,
                                    a,
                                    b,
                                ));
                            }
                        }
                    }
                }
            }
        }
        result
    }
}
fn ray(a: Vec3, b: Vec3) -> Ray {
    Ray::new(
        Point3::new(a.x, a.y, a.z),
        Vector3::new(b.x - a.x, b.y - a.y, b.z - a.z),
    )
}
/// A continuous body hit, not a distance check at the next projectile position.
pub fn body_hit(a: Vec3, b: Vec3, feet: Vec3) -> Option<f32> {
    use parry3d::query::RayCast;
    let shape = Capsule::new(Point3::new(0., 0.35, 0.), Point3::new(0., 1.55, 0.), 0.32);
    shape.cast_local_ray(&ray(a - feet, b - feet), 1., true)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn swept_arrow_hits_body_but_clears_overhead() {
        assert!(body_hit(Vec3::new(-5., 1., 0.), Vec3::new(5., 1., 0.), Vec3::ZERO).is_some());
        assert!(body_hit(Vec3::new(-5., 2.1, 0.), Vec3::new(5., 2.1, 0.), Vec3::ZERO).is_none());
    }
    #[test]
    fn rotated_convex_wall_stops_a_ray() {
        let mut obstacles = ArrowObstacles::default();
        obstacles
            .shapes
            .insert("wall".into(), vec![SharedShape::cuboid(2., 2., 0.1)]);
        assert!(obstacles
            .hull_hit(
                "wall",
                Vec3::ZERO,
                Quat::from_rotation_y(0.7),
                1.,
                Vec3::new(0., 1., -4.),
                Vec3::new(0., 1., 4.)
            )
            .is_some());
        assert!(obstacles
            .hull_hit(
                "wall",
                Vec3::ZERO,
                Quat::from_rotation_y(0.7),
                1.,
                Vec3::new(0., 3., -4.),
                Vec3::new(0., 3., 4.)
            )
            .is_none());
    }
}
