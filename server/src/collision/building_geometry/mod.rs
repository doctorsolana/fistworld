//! Building collision geometry internals.

mod shapes;

use bevy::prelude::*;

use shared::building::{building_rotation_quat, BuildingPosition, PlacedBuilding};
use shared::physics::WALKABLE_THRESHOLD;
use shared::structures::StructureCollider;

use crate::collision::building_index::BuildingSpatialIndex;
use crate::collision::geometry::{sphere_vs_compound_hulls, SupportContact};
use crate::collision::library::DerivedBuildingColliderLibrary;
use shapes::{get_structure_bounding_radius, sphere_vs_structure};

/// Resolve capsule vs placed building colliders.
/// Uses baked mesh colliders when available, falls back to box colliders otherwise.
pub fn handle_capsule_vs_buildings(
    building_lib: Option<&DerivedBuildingColliderLibrary>,
    building_index: Option<&BuildingSpatialIndex>,
    buildings: &Query<(Entity, &PlacedBuilding, &BuildingPosition)>,
    candidate_entities: &mut Vec<Entity>,
    pos: &mut Vec3,
    mut velocity: Option<&mut Vec3>,
    radius: f32,
    height: f32,
    step_up_height: f32,
) -> SupportContact {
    let half_h = height * 0.5;
    let step_up_height = step_up_height.max(0.0);
    let sphere_offset = (half_h - radius).max(0.0);
    let mut support = SupportContact::default();

    for _ in 0..4 {
        let mut moved = false;

        if let Some(index) = building_index {
            index.collect_nearby_entities(
                *pos,
                radius + half_h + step_up_height + 8.0,
                candidate_entities,
            );
        } else {
            candidate_entities.clear();
            candidate_entities.extend(buildings.iter().map(|(entity, _, _)| entity));
        }

        for candidate_entity in candidate_entities.iter().copied() {
            let Ok((_entity, building, building_pos)) = buildings.get(candidate_entity) else {
                continue;
            };
            let building_rot = building_rotation_quat(building.rotation);
            let def = building.building_type.definition();
            let building_scale = 1.0;

            let baked_shape = building_lib.and_then(|lib| lib.by_type.get(&building.building_type));

            if let Some(shape) = baked_shape {
                let bounding_r = shape.bounding_radius * building_scale;
                let to_building = *pos - building_pos.0;
                let dist2 = to_building.length_squared();
                let max_dist = radius + bounding_r + half_h;
                if dist2 >= max_dist * max_dist {
                    continue;
                }

                let sphere_positions = [
                    *pos - Vec3::Y * sphere_offset,
                    *pos,
                    *pos + Vec3::Y * sphere_offset,
                ];

                let mut best_penetration = 0.0f32;
                let mut best_normal = Vec3::ZERO;
                let mut best_sphere_idx: usize = 1;
                let mut bottom_support_normal = Vec3::ZERO;
                let mut bottom_has_support = false;

                for (sphere_idx, sphere_pos) in sphere_positions.iter().copied().enumerate() {
                    if let Some((pen, normal)) = sphere_vs_compound_hulls(
                        sphere_pos,
                        radius,
                        &shape.hulls,
                        building_pos.0,
                        building_rot,
                        building_scale,
                    ) {
                        if sphere_idx == 0 && normal.y > WALKABLE_THRESHOLD {
                            bottom_has_support = true;
                            if normal.y > bottom_support_normal.y {
                                bottom_support_normal = normal;
                            }
                        }
                        if pen > best_penetration {
                            best_penetration = pen;
                            best_normal = normal;
                            best_sphere_idx = sphere_idx;
                        }
                    }
                }

                if best_penetration <= 0.0 {
                    continue;
                }

                let is_walkable_slope = best_normal.y > WALKABLE_THRESHOLD;

                if bottom_has_support {
                    support.has_support = true;
                    if bottom_support_normal.y > support.support_normal.y {
                        support.support_normal = bottom_support_normal;
                    }
                }

                let vel_y = velocity.as_deref().map(|v| v.y).unwrap_or(0.0);
                let can_step = step_up_height > 0.0
                    && best_sphere_idx == 0
                    && best_normal.y.abs() < 0.2
                    && vel_y <= 0.0;

                if can_step {
                    let mut test_pos = *pos;
                    test_pos.y += step_up_height;

                    let test_sphere_positions = [
                        test_pos - Vec3::Y * sphere_offset,
                        test_pos,
                        test_pos + Vec3::Y * sphere_offset,
                    ];

                    let mut still_colliding = false;
                    for test_sphere_pos in test_sphere_positions.iter().copied() {
                        if sphere_vs_compound_hulls(
                            test_sphere_pos,
                            radius,
                            &shape.hulls,
                            building_pos.0,
                            building_rot,
                            building_scale,
                        )
                        .is_some()
                        {
                            still_colliding = true;
                            break;
                        }
                    }

                    if !still_colliding {
                        pos.y = test_pos.y;
                        if let Some(v) = velocity.as_deref_mut() {
                            v.y = v.y.max(0.0);
                        }
                        support.has_support = true;
                        moved = true;
                        continue;
                    }
                }

                let push = best_normal * best_penetration;
                pos.x += push.x;
                pos.y += if is_walkable_slope {
                    push.y
                } else {
                    push.y.max(0.0)
                };
                pos.z += push.z;

                if let Some(v) = velocity.as_deref_mut() {
                    let vn = v.dot(best_normal);
                    if vn < 0.0 {
                        *v -= best_normal * vn;
                    }
                }

                moved = true;
            } else {
                // Fallback to box collider for buildings without baked colliders.
                let collider = StructureCollider::Box {
                    half_extents: Vec3::new(
                        def.footprint.x * 0.5,
                        def.height * 0.5,
                        def.footprint.y * 0.5,
                    ),
                };

                let bounding_r = get_structure_bounding_radius(&collider, building_scale);
                let to_building = *pos - building_pos.0;
                let dist2 = to_building.length_squared();
                let max_dist = radius + bounding_r + half_h;
                if dist2 >= max_dist * max_dist {
                    continue;
                }

                let sphere_positions = [
                    *pos - Vec3::Y * sphere_offset,
                    *pos,
                    *pos + Vec3::Y * sphere_offset,
                ];

                let mut best_penetration = 0.0f32;
                let mut best_normal = Vec3::ZERO;
                let mut best_sphere_idx: usize = 1;
                let mut bottom_support_normal = Vec3::ZERO;
                let mut bottom_has_support = false;

                for (sphere_idx, sphere_pos) in sphere_positions.iter().copied().enumerate() {
                    if let Some((pen, normal)) = sphere_vs_structure(
                        sphere_pos,
                        radius,
                        &collider,
                        building_pos.0,
                        building_rot,
                        building_scale,
                    ) {
                        if sphere_idx == 0 && normal.y > WALKABLE_THRESHOLD {
                            bottom_has_support = true;
                            if normal.y > bottom_support_normal.y {
                                bottom_support_normal = normal;
                            }
                        }
                        if pen > best_penetration {
                            best_penetration = pen;
                            best_normal = normal;
                            best_sphere_idx = sphere_idx;
                        }
                    }
                }

                if best_penetration <= 0.0 {
                    continue;
                }

                let is_walkable_slope = best_normal.y > WALKABLE_THRESHOLD;

                if bottom_has_support {
                    support.has_support = true;
                    if bottom_support_normal.y > support.support_normal.y {
                        support.support_normal = bottom_support_normal;
                    }
                }

                let vel_y = velocity.as_deref().map(|v| v.y).unwrap_or(0.0);
                let can_step = step_up_height > 0.0
                    && best_sphere_idx == 0
                    && best_normal.y.abs() < 0.2
                    && vel_y <= 0.0;

                if can_step {
                    let mut test_pos = *pos;
                    test_pos.y += step_up_height;

                    let test_sphere_positions = [
                        test_pos - Vec3::Y * sphere_offset,
                        test_pos,
                        test_pos + Vec3::Y * sphere_offset,
                    ];

                    let mut still_colliding = false;
                    for test_sphere_pos in test_sphere_positions.iter().copied() {
                        if sphere_vs_structure(
                            test_sphere_pos,
                            radius,
                            &collider,
                            building_pos.0,
                            building_rot,
                            building_scale,
                        )
                        .is_some()
                        {
                            still_colliding = true;
                            break;
                        }
                    }

                    if !still_colliding {
                        pos.y = test_pos.y;
                        if let Some(v) = velocity.as_deref_mut() {
                            v.y = v.y.max(0.0);
                        }
                        support.has_support = true;
                        moved = true;
                        continue;
                    }
                }

                let push = best_normal * best_penetration;
                pos.x += push.x;
                pos.y += if is_walkable_slope {
                    push.y
                } else {
                    push.y.max(0.0)
                };
                pos.z += push.z;

                if let Some(v) = velocity.as_deref_mut() {
                    let vn = v.dot(best_normal);
                    if vn < 0.0 {
                        *v -= best_normal * vn;
                    }
                }

                moved = true;
            }
        }

        if !moved {
            break;
        }
    }

    support
}
