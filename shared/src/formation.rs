//! Deterministic formation layout shared by the command preview and the server.
//! This computes destinations, never authority or movement.

use crate::protocol::FormationFrontage;
use bevy::prelude::*;

pub const DEFAULT_FILES: usize = 10;
pub const FILE_SPACING: f32 = 1.4;
pub const RANK_SPACING: f32 = 1.7;
pub const BATTALION_GAP: f32 = 5.0;
pub const MAX_FILES: usize = 20;

#[derive(Clone, Debug)]
pub struct FormationSoldier {
    pub entity: Entity,
    /// Durable PersonId; entity IDs differ between the server and its clients.
    pub identity: u64,
    pub position: Vec3,
    pub strength: u8,
}

#[derive(Clone, Debug)]
pub struct FormationGroup {
    pub key: u64,
    pub soldiers: Vec<FormationSoldier>,
}

#[derive(Debug)]
pub struct FormationBlock {
    pub key: u64,
    pub centre: Vec3,
    pub facing: Vec2,
    pub files: usize,
    pub slots: Vec<(Entity, Vec3)>,
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
    let files = frontage.map_or(DEFAULT_FILES, |f| {
        let available = (f.width - BATTALION_GAP * (groups.len() - 1) as f32).max(0.0);
        (1 + (available / groups.len() as f32 / FILE_SPACING).floor() as usize).clamp(2, MAX_FILES)
    });
    let width = |g: &FormationGroup| (g.soldiers.len().min(files) - 1) as f32 * FILE_SPACING;
    let total = groups.iter().map(width).sum::<f32>() + BATTALION_GAP * (groups.len() - 1) as f32;
    let mut offset = -total * 0.5;
    let mut blocks = Vec::with_capacity(groups.len());
    for mut group in groups {
        let block_width = width(&group);
        let at = target + Vec3::new(right.x, 0.0, right.y) * (offset + block_width * 0.5);
        offset += block_width + BATTALION_GAP;
        group.soldiers.sort_by(|a, b| {
            b.strength
                .cmp(&a.strength)
                .then(a.identity.cmp(&b.identity))
        });
        let mut slots = Vec::with_capacity(group.soldiers.len());
        for (rank, soldiers) in group.soldiers.chunks_mut(files).enumerate() {
            soldiers.sort_by(|a, b| {
                a.position
                    .xz()
                    .dot(right)
                    .total_cmp(&b.position.xz().dot(right))
                    .then(a.identity.cmp(&b.identity))
            });
            let count = soldiers.len();
            for (file, soldier) in soldiers.iter().enumerate() {
                let lateral = (file as f32 - (count - 1) as f32 * 0.5) * FILE_SPACING;
                let xz = at.xz() + right * lateral - facing * rank as f32 * RANK_SPACING;
                slots.push((soldier.entity, Vec3::new(xz.x, target.y, xz.y)));
            }
        }
        blocks.push(FormationBlock {
            key: group.key,
            centre: at,
            facing,
            files,
            slots,
        });
    }
    blocks
}

#[cfg(test)]
mod tests {
    use super::*;

    pub fn five_blocks() -> Vec<FormationGroup> {
        (0..5)
            .map(|g| FormationGroup {
                key: g,
                soldiers: (0..50)
                    .map(|s| FormationSoldier {
                        entity: Entity::from_raw_u32((g * 50 + s + 1) as u32).unwrap(),
                        identity: g * 50 + s + 1,
                        position: Vec3::new(
                            g as f32 * 20.0 + (s % 10) as f32,
                            0.0,
                            (s / 10) as f32,
                        ),
                        strength: (s % 20) as u8,
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
