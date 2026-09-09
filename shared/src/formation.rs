//! Deterministic formation layout shared by the command preview and the server.
//! This computes destinations, never authority or movement.

use crate::components::{BattalionFormation, SoldierRole};
use crate::protocol::FormationFrontage;
use bevy::prelude::*;

pub const DEFAULT_FILES: usize = 10;
pub const FILE_SPACING: f32 = 1.4;
pub const RANK_SPACING: f32 = 1.7;
pub const BATTALION_GAP: f32 = 5.0;
pub const MAX_FILES: usize = crate::components::MAX_BATTALION_SIZE;
pub const MIN_FILE_SPACING: f32 = 1.15;
pub const MAX_FILE_SPACING: f32 = 1.65;

#[derive(Clone, Debug)]
pub struct FormationSoldier {
    pub entity: Entity,
    /// Durable PersonId; entity IDs differ between the server and its clients.
    pub identity: u64,
    pub position: Vec3,
    pub seat: Option<Vec2>,
}

#[derive(Clone, Debug)]
pub struct FormationGroup {
    pub key: u64,
    pub shape: BattalionFormation,
    pub role: SoldierRole,
    pub soldiers: Vec<FormationSoldier>,
}

#[derive(Clone, Debug)]
pub struct FormationBlock {
    pub key: u64,
    pub centre: Vec3,
    pub facing: Vec2,
    pub files: usize,
    pub spacing: f32,
    pub rank_spacing: f32,
    pub clearance: f32,
    pub slots: Vec<(Entity, Vec3)>,
    /// File for each slot, including incomplete ranks and casualty gaps.
    pub file_indices: Vec<usize>,
}

pub fn centre(positions: impl Iterator<Item = Vec3>) -> Vec3 {
    let (sum, count) = positions.fold((Vec3::ZERO, 0), |(sum, n), p| (sum + p, n + 1));
    sum / count.max(1) as f32
}

pub fn layout(
    mut groups: Vec<FormationGroup>,
    target: Vec3,
    frontage: Option<FormationFrontage>,
) -> Vec<FormationBlock> {
    groups.retain(|g| !g.soldiers.is_empty());
    if groups.is_empty() {
        return Vec::new();
    }
    let approach = target.xz()
        - centre(
            groups
                .iter()
                .flat_map(|g| g.soldiers.iter().map(|s| s.position)),
        )
        .xz();
    let facing = frontage
        .map(|f| f.facing)
        .unwrap_or(approach)
        .try_normalize()
        .unwrap_or(Vec2::Y);
    let right = Vec2::new(facing.y, -facing.x);
    // Preserve block order from their current positions, not muster ordinals.
    groups.sort_by(|a, b| {
        let lateral = |g: &FormationGroup| {
            centre(g.soldiers.iter().map(|s| s.position))
                .xz()
                .dot(right)
        };
        lateral(a).total_cmp(&lateral(b)).then(a.key.cmp(&b.key))
    });
    // Nearly aligned blocks must not swap because a replicated final step is
    // a few centimetres older. Sort each lateral band by durable battalion ID.
    let mut begin = 0;
    while begin < groups.len() {
        let origin = centre(groups[begin].soldiers.iter().map(|s| s.position))
            .xz()
            .dot(right);
        let mut end = begin + 1;
        while end < groups.len()
            && centre(groups[end].soldiers.iter().map(|s| s.position))
                .xz()
                .dot(right)
                - origin
                < 1.0
        {
            end += 1;
        }
        groups[begin..end].sort_by_key(|group| group.key);
        begin = end;
    }
    let width_each = frontage.map(|f| {
        (f.width - BATTALION_GAP * (groups.len() - 1) as f32).max(0.0) / groups.len() as f32
    });
    for group in &mut groups {
        group.shape = deployment_shape(group.soldiers.len(), group.shape, width_each, group.role);
    }
    let width = |g: &FormationGroup| (usize::from(g.shape.files) - 1) as f32 * g.shape.spacing;
    let total = groups.iter().map(width).sum::<f32>() + BATTALION_GAP * (groups.len() - 1) as f32;
    let mut offset = -total * 0.5;
    let mut blocks = Vec::with_capacity(groups.len());
    for mut group in groups {
        let files = usize::from(group.shape.files);
        let spacing = group.shape.spacing;
        let rank_spacing = rank_spacing(group.role);
        let clearance = clearance(group.role);
        let block_width = width(&group);
        let at = target + Vec3::new(right.x, 0.0, right.y) * (offset + block_width * 0.5);
        offset += block_width + BATTALION_GAP;
        let seated = group
            .soldiers
            .iter()
            .filter_map(|s| s.seat)
            .collect::<Vec<_>>();
        let origin = centre(group.soldiers.iter().map(|s| s.position)).xz()
            - seated.iter().copied().sum::<Vec2>() / seated.len().max(1) as f32;
        let ordered_position = |s: &FormationSoldier| s.seat.unwrap_or(s.position.xz() - origin);
        // Keep nearby soldiers in nearby ranks. Re-sorting by strength on every
        // command made otherwise small redeployments send people across the block.
        group.soldiers.sort_by(|a, b| {
            ordered_position(b)
                .dot(facing)
                .total_cmp(&ordered_position(a).dot(facing))
                .then(a.identity.cmp(&b.identity))
        });
        let mut slots = Vec::with_capacity(group.soldiers.len());
        let mut file_indices = Vec::with_capacity(group.soldiers.len());
        for (rank, soldiers) in group.soldiers.chunks_mut(files).enumerate() {
            soldiers.sort_by(|a, b| {
                ordered_position(a)
                    .dot(right)
                    .total_cmp(&ordered_position(b).dot(right))
                    .then(a.identity.cmp(&b.identity))
            });
            let count = soldiers.len();
            for (file, soldier) in soldiers.iter().enumerate() {
                // The short last rank occupies real files, so its file queues
                // agree with combat replacement instead of sliding on arrival.
                let first_file = (files - count) / 2;
                let lateral = ((first_file + file) as f32 - (files - 1) as f32 * 0.5) * spacing;
                let xz = at.xz() + right * lateral - facing * rank as f32 * rank_spacing;
                slots.push((soldier.entity, Vec3::new(xz.x, target.y, xz.y)));
                file_indices.push(first_file + file);
            }
        }
        blocks.push(FormationBlock {
            key: group.key,
            centre: at,
            facing,
            files,
            spacing,
            rank_spacing,
            clearance,
            slots,
            file_indices,
        });
    }
    blocks
}

/// Physical dimensions are shared by previews, authoritative slots and combat files.
pub fn rank_spacing(role: SoldierRole) -> f32 {
    if role == SoldierRole::Cavalry {
        3.2
    } else {
        RANK_SPACING
    }
}
pub fn clearance(role: SoldierRole) -> f32 {
    if role == SoldierRole::Cavalry {
        crate::components::HORSE_CLEARANCE
    } else {
        0.0
    }
}

pub fn deployment_shape(
    count: usize,
    preferred: BattalionFormation,
    width: Option<f32>,
    role: SoldierRole,
) -> BattalionFormation {
    let mounted = role == SoldierRole::Cavalry;
    let base = if mounted { 3.2 } else { FILE_SPACING };
    let minimum = if mounted { 3.15 } else { MIN_FILE_SPACING };
    let maximum = if mounted { 3.6 } else { MAX_FILE_SPACING };
    let limit = count.clamp(1, MAX_FILES);
    let files = width
        .map_or(usize::from(preferred.files), |w| {
            1 + (w.max(0.0) / base).round() as usize
        })
        .clamp(1, limit);
    let spacing = width
        .filter(|_| files > 1)
        .map_or(if mounted && preferred.spacing < minimum { base } else { preferred.spacing }, |w| w / (files - 1) as f32);
    BattalionFormation {
        files: files as u8,
        spacing: if spacing.is_finite() {
            spacing.clamp(minimum, maximum)
        } else {
            base
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn five_blocks() -> Vec<FormationGroup> {
        (0..5)
            .map(|g| FormationGroup {
                role: SoldierRole::Infantry,
                key: g,
                shape: default(),
                soldiers: (0..50)
                    .map(|s| FormationSoldier {
                        entity: Entity::from_raw_u32((g * 50 + s + 1) as u32).unwrap(),
                        identity: g * 50 + s + 1,
                        seat: None,
                        position: Vec3::new(
                            g as f32 * 20.0 + (s % 10) as f32,
                            0.0,
                            (s / 10) as f32,
                        ),
                    })
                    .collect(),
            })
            .collect()
    }

    #[test]
    fn two_hundred_fifty_soldiers_form_five_separate_ten_by_five_blocks() {
        let blocks = layout(
            five_blocks(),
            Vec3::new(40.0, 0.0, 100.0),
            Some(FormationFrontage {
                facing: Vec2::Y,
                width: 83.0,
            }),
        );
        assert_eq!(blocks.len(), 5);
        for (i, block) in blocks.iter().enumerate() {
            assert_eq!(block.key, i as u64);
            assert_eq!(block.files, 10);
            assert_eq!(block.slots.len(), 50);
            for (n, (_, slot)) in block.slots.iter().enumerate() {
                assert!((slot.z - (100.0 - (n / 10) as f32 * RANK_SPACING)).abs() < 0.001);
            }
            if i > 0 {
                let previous = &blocks[i - 1];
                let left = block
                    .slots
                    .iter()
                    .map(|s| s.1.x)
                    .fold(f32::INFINITY, f32::min);
                let right = previous
                    .slots
                    .iter()
                    .map(|s| s.1.x)
                    .fold(f32::NEG_INFINITY, f32::max);
                assert!((left - right - BATTALION_GAP).abs() < 0.001);
            }
        }
    }

    #[test]
    fn aligned_blocks_do_not_swap_on_centimetre_replication_drift() {
        let a = five_blocks();
        let mut b = a.clone();
        for (i, group) in b.iter_mut().enumerate() {
            for soldier in &mut group.soldiers {
                soldier.position.z += (i as f32 - 2.0) * 0.05;
            }
        }
        let f = Some(FormationFrontage {
            facing: Vec2::X,
            width: 83.0,
        });
        let a = layout(a, Vec3::X * 100.0, f);
        let b = layout(b, Vec3::X * 100.0, f);
        assert_eq!(
            a.iter().map(|b| (b.key, b.centre)).collect::<Vec<_>>(),
            b.iter().map(|b| (b.key, b.centre)).collect::<Vec<_>>()
        );
    }

    #[test]
    fn client_entity_remapping_does_not_change_person_slot_assignment() {
        let original = five_blocks();
        let mut remapped = original.clone();
        for soldier in remapped.iter_mut().flat_map(|g| &mut g.soldiers) {
            soldier.entity = Entity::from_raw_u32(1000 - soldier.identity as u32).unwrap();
        }
        let a = layout(original.clone(), Vec3::Z * 40.0, None);
        let b = layout(remapped.clone(), Vec3::Z * 40.0, None);
        let ids = |groups: Vec<FormationGroup>, blocks: Vec<FormationBlock>| {
            let lookup: std::collections::HashMap<_, _> = groups
                .into_iter()
                .flat_map(|g| g.soldiers)
                .map(|s| (s.entity, s.identity))
                .collect();
            blocks
                .into_iter()
                .flat_map(|b| b.slots)
                .map(|(e, p)| (lookup[&e], p))
                .collect::<Vec<_>>()
        };
        assert_eq!(ids(original, a), ids(remapped, b));
    }

    #[test]
    fn layout_preserves_block_order_under_rotation_and_permutation() {
        let groups = five_blocks();
        let mut reversed = groups.clone();
        reversed.reverse();
        let f = Some(FormationFrontage {
            facing: Vec2::new(0.7, 0.7),
            width: 100.0,
        });
        let a = layout(groups, Vec3::new(100.0, 0.0, 80.0), f);
        let b = layout(reversed, Vec3::new(100.0, 0.0, 80.0), f);
        assert_eq!(
            a.iter().map(|b| &b.slots).collect::<Vec<_>>(),
            b.iter().map(|b| &b.slots).collect::<Vec<_>>()
        );
    }
}

#[cfg(test)]
mod fluid_tests {
    use super::*;
    fn group() -> FormationGroup {
        FormationGroup {
            role: SoldierRole::Infantry,
            key: 1,
            shape: default(),
            soldiers: (0..50)
                .map(|i| FormationSoldier {
                    entity: Entity::from_raw_u32(i + 1).unwrap(),
                    identity: i as u64,
                    position: Vec3::new(
                        (i % 10) as f32 * FILE_SPACING,
                        0.0,
                        -((i / 10) as f32) * RANK_SPACING,
                    ),
                    seat: None,
                })
                .collect(),
        }
    }
    #[test]
    fn a_fifty_person_battalion_can_form_two_ranks_and_keep_them_on_a_normal_move() {
        let mut group = group();
        let wide = layout(
            vec![group.clone()],
            Vec3::Z * 20.0,
            Some(FormationFrontage {
                facing: Vec2::Y,
                width: 33.6,
            }),
        )
        .pop()
        .unwrap();
        assert_eq!(wide.files, 25);
        assert_eq!(wide.slots.len().div_ceil(wide.files), 2);
        group.shape = BattalionFormation {
            files: wide.files as u8,
            spacing: wide.spacing,
        };
        let moved = layout(vec![group], Vec3::Z * 50.0, None).pop().unwrap();
        assert_eq!(moved.files, 25);
        assert_eq!(moved.spacing, wide.spacing);
    }
    #[test]
    fn small_drag_adjustments_change_spacing_without_jumping_a_whole_file() {
        let a = deployment_shape(50, default(), Some(33.2), SoldierRole::Infantry);
        let b = deployment_shape(50, default(), Some(33.3), SoldierRole::Infantry);
        assert_eq!(a.files, 25);
        assert_eq!(a.files, b.files);
        assert!((b.spacing - a.spacing - 0.1 / 24.0).abs() < 0.00001);
    }
    #[test]
    fn replicated_seats_make_redeployment_insensitive_to_walking_replication_delay() {
        let mut a = group();
        for s in &mut a.soldiers {
            s.seat = Some(s.position.xz());
        }
        let mut b = a.clone();
        for s in &mut b.soldiers {
            s.position += Vec3::new(
                (s.identity % 3) as f32 * 0.1,
                0.0,
                (s.identity % 7) as f32 * 0.05,
            );
        }
        for facing in [Vec2::Y, Vec2::X, Vec2::new(0.6, 0.8)] {
            let f = Some(FormationFrontage {
                facing,
                width: 33.6,
            });
            assert_eq!(
                layout(vec![a.clone()], Vec3::Z * 40.0, f)[0].slots,
                layout(vec![b.clone()], Vec3::Z * 40.0, f)[0].slots
            );
        }
    }
    #[test]
    fn partial_last_rank_occupies_real_files() {
        let mut g = group();
        g.soldiers.truncate(17);
        let b = layout(
            vec![g],
            Vec3::ZERO,
            Some(FormationFrontage {
                facing: Vec2::Y,
                width: 12.6,
            }),
        )
        .pop()
        .unwrap();
        for ((_, slot), file) in b.slots.iter().zip(&b.file_indices) {
            assert!((slot.x - (*file as f32 - 4.5) * b.spacing).abs() < 0.001);
        }
    }
}

#[cfg(test)]
mod cavalry_tests {
    use super::*;
    #[test]
    fn narrow_drag_preserves_the_horse_body_and_rank_clearance() {
        let group = FormationGroup {
            key: 1,
            role: SoldierRole::Cavalry,
            shape: BattalionFormation::default(),
            soldiers: (0..8).map(|i| FormationSoldier {
                entity: Entity::from_raw_u32(i + 1).unwrap(), identity: u64::from(i), position: Vec3::ZERO, seat: None,
            }).collect(),
        };
        let blocks = layout(vec![group], Vec3::Z * 20., Some(FormationFrontage { facing: Vec2::Y, width: 1. }));
        let block = &blocks[0];
        assert_eq!(block.files, 1);
        assert_eq!(block.clearance, crate::components::HORSE_CLEARANCE);
        assert!(block.rank_spacing >= crate::components::HORSE_CLEARANCE * 2.);
        for pair in block.slots.windows(2) {
            assert!(pair[0].1.distance(pair[1].1) >= 3.1);
        }
    }
}
