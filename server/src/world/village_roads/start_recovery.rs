//! Repair an invalid embodied start after live prop collision becomes available.
//! This is bounded depenetration, never permission for a route to cross solids.

use super::*;

const MAX_CORRECTION: f32 = 4.0;
const STEP: f32 = 0.5;
const DIRECTIONS: usize = 16;
const MAX_NEARBY_PROPS: usize = 96;

/// Opt-in, bounded evidence that correction is followed by actual movement.
#[derive(Default)]
pub(super) struct RecoveryProgress {
    entries: HashMap<Entity, (Vec3, f64, f64)>,
}

impl RecoveryProgress {
    pub(super) fn note(&mut self, entity: Entity, from: Vec3, corrected: Vec3, now: f64) {
        if std::env::var_os("FISTWORLD_LAB_ROUTE_DIAGNOSTICS").is_none() {
            return;
        }
        eprintln!(
            "LAB corrected route start entity={entity:?} from={from:?} corrected={corrected:?}"
        );
        if self.entries.len() < 16 {
            self.entries
                .insert(entity, (corrected, now + 1.0, now + 8.0));
        }
    }

    pub(super) fn observe(&mut self, now: f64, position: impl Fn(Entity) -> Option<Vec3>) {
        self.entries.retain(|entity, (corrected, next, deadline)| {
            if now < *next {
                return true;
            }
            let Some(at) = position(*entity) else { return false };
            let distance = at.xz().distance(corrected.xz());
            eprintln!("LAB recovered route progress entity={entity:?} corrected={corrected:?} at={at:?} travelled={distance:.3}m");
            *next = now + 1.0;
            distance < 0.5 && now < *deadline
        });
    }
}

struct PropDisc {
    kind: shared::props::PropKind,
    center: Vec2,
    radius: f32,
}

pub(super) fn recover_prop_overlap(
    start: Vec3,
    terrain: &WorldTerrain,
    buildings: Option<&SpatialObstacleGrid>,
    colliders: &StaticColliders,
    derived: &DerivedColliderLibrary,
) -> Option<Vec3> {
    let point = start.xz();
    if crate::player::hero::navigation_segment_clear(
        point,
        point,
        None,
        Some(colliders),
        Some(derived),
    ) || !road_sample_is_dry(terrain, point)
        || buildings.is_some_and(|grid| grid.point_blocked(point))
    {
        return None;
    }

    // Match the movement query's cell halo for the largest authored prop.
    // Instance membership is unique to its centre cell; no dedup allocation.
    const CELL: f32 = 16.0;
    let low = ((point - Vec2::splat(MAX_CORRECTION + VILLAGER_PROP_RADIUS)) / CELL)
        .floor()
        .as_ivec2()
        - IVec2::ONE;
    let high = ((point + Vec2::splat(MAX_CORRECTION + VILLAGER_PROP_RADIUS)) / CELL)
        .floor()
        .as_ivec2()
        + IVec2::ONE;
    let mut props = Vec::new();
    for x in low.x..=high.x {
        for z in low.y..=high.y {
            let Some(ids) = colliders.cells.get(&(x, z)) else {
                continue;
            };
            for id in ids {
                let Some(instance) = colliders.instances.get(id) else {
                    continue;
                };
                let Some(shape) = derived.by_kind.get(&instance.kind) else {
                    continue;
                };
                let radius = shape.horizontal_radius * instance.scale + VILLAGER_PROP_RADIUS;
                let center = instance.position.xz();
                if center.distance_squared(point) > (MAX_CORRECTION + radius).powi(2) {
                    continue;
                }
                if props.len() == MAX_NEARBY_PROPS {
                    return None;
                }
                props.push(PropDisc {
                    kind: instance.kind,
                    center,
                    radius,
                });
            }
        }
    }
    let overlapping = |disc: &&PropDisc| point.distance_squared(disc.center) < disc.radius.powi(2);
    let deepest = props.iter().filter(overlapping).max_by(|a, b| {
        let depth = |disc: &PropDisc| disc.radius - disc.center.distance(point);
        depth(a)
            .total_cmp(&depth(b))
            .then_with(|| a.center.x.total_cmp(&b.center.x))
            .then_with(|| a.center.y.total_cmp(&b.center.y))
    })?;
    let away = point - deepest.center;
    let phase = if away.length_squared() > 1e-8 {
        away.y.atan2(away.x)
    } else {
        0.0
    };
    for ring in 1..=(MAX_CORRECTION / STEP) as usize {
        for direction in 0..DIRECTIONS {
            let angle = phase + direction as f32 * std::f32::consts::TAU / DIRECTIONS as f32;
            let candidate = point + Vec2::new(angle.cos(), angle.sin()) * (ring as f32 * STEP);
            if !terrain
                .generator
                .active_map_bounds()
                .contains_xz(candidate.x, candidate.y)
                || buildings.is_some_and(|grid| grid.segment_blocked(point, candidate))
                || !clear_escape(point, candidate, &props)
                || !embodied_segment_is_dry(terrain, point, candidate)
            {
                continue;
            }
            if std::env::var_os("FISTWORLD_LAB_ROUTE_DIAGNOSTICS").is_some() {
                for disc in props.iter().filter(overlapping) {
                    eprintln!(
                        "LAB recovered prop overlap from={point:?} to={candidate:?} kind={:?} center={:?} body_radius={:.3}",
                        disc.kind, disc.center, disc.radius,
                    );
                }
            }
            return Some(Vec3::new(
                candidate.x,
                terrain.get_height(candidate.x, candidate.y),
                candidate.y,
            ));
        }
    }
    None
}

fn clear_escape(start: Vec2, end: Vec2, props: &[PropDisc]) -> bool {
    let movement = end - start;
    props.iter().all(|disc| {
        let offset = start - disc.center;
        let radius_squared = disc.radius.powi(2);
        if offset.length_squared() < radius_squared {
            // Distance from every overlapping solid increases for the whole
            // segment. We may leave its initial penetration, never traverse
            // deeper into it or finish touching its body-expanded boundary.
            offset.dot(movement) >= -1e-5
                && end.distance_squared(disc.center) > (disc.radius + 0.03).powi(2)
        } else {
            point_segment_distance_squared(disc.center, start, end) >= radius_squared
        }
    })
}

#[cfg(test)]
mod tests;
