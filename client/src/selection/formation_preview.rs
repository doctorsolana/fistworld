//! Right-drag deployment uses the same rank/block layout as the server.

use bevy::prelude::*;
use shared::components::{PlayerPosition, MAX_BATTALION_SIZE};
use shared::formation::{FormationGroup, FormationSoldier};
use shared::protocol::FormationFrontage;
use std::collections::BTreeMap;

#[derive(Default, Reflect, GizmoConfigGroup)]
pub struct FormationGizmos;

#[derive(Resource, Default)]
pub struct PreviewReadiness {
    pub slots: usize,
    pub stable_frames: u32,
    target: Option<(Vec3, FormationFrontage)>,
}

pub fn frontage_from_drag(start: Vec3, end: Vec3) -> Option<(Vec3, FormationFrontage)> {
    let across = end.xz() - start.xz();
    let width = across.length();
    if !width.is_finite() || width < 2.0 {
        return None;
    }
    let right = across / width;
    Some((
        (start + end) * 0.5,
        FormationFrontage {
            facing: Vec2::new(-right.y, right.x),
            width: width.min(2048.0),
        },
    ))
}

pub fn draw_formation_preview(
    drag: Res<super::RightDrag>,
    selection: Res<super::Selection>,
    roster: Res<crate::army_roster::ArmyRoster>,
    hit: Res<crate::camera_rts::CursorTerrainHit>,
    positions: Query<&PlayerPosition>,
    terrain: Option<Res<shared::terrain::WorldTerrain>>,
    input: Res<crate::input::InputState>,
    mut readiness: ResMut<PreviewReadiness>,
    mut gizmos: Gizmos<FormationGizmos>,
) {
    if !drag.formation || input.ui_blocking() {
        if readiness.slots != 0 {
            *readiness = PreviewReadiness::default();
        }
        return;
    }
    let Some((start, end)) = drag.formation_start.zip(hit.0) else {
        return;
    };
    let Some((target, frontage)) = frontage_from_drag(start, end) else {
        return;
    };
    let placement = Some((target, frontage));
    if readiness.target == placement {
        readiness.stable_frames += 1;
    } else {
        readiness.target = placement;
        readiness.stable_frames = 0;
    }
    let mut groups = BTreeMap::<u64, Vec<FormationSoldier>>::new();
    let mut loose = 0;
    for entity in &selection.entities {
        let Some(soldier) = roster.soldiers.get(entity) else {
            continue;
        };
        let Ok(position) = positions.get(*entity) else {
            continue;
        };
        let key = soldier.battalion.map(|id| id.0).unwrap_or_else(|| {
            let key = u64::MAX - (loose / MAX_BATTALION_SIZE) as u64;
            loose += 1;
            key
        });
        groups.entry(key).or_default().push(FormationSoldier {
            entity: *entity,
            identity: soldier.identity,
            position: position.0,
            strength: soldier.strength,
        });
    }
    let blocks = shared::formation::layout(
        groups
            .into_iter()
            .map(|(key, soldiers)| FormationGroup { key, soldiers })
            .collect(),
        target,
        Some(frontage),
    );
    readiness.slots = blocks.iter().map(|b| b.slots.len()).sum();
    let ground = |mut p: Vec3| {
        if let Some(terrain) = terrain.as_deref() {
            p.y = terrain.get_height(p.x, p.z);
        }
        p + Vec3::Y * 0.15
    };
    let gold = Color::srgb(1.0, 0.82, 0.32);
    for block in blocks {
        for (_, position) in &block.slots {
            gizmos
                .circle(
                    Isometry3d::new(
                        ground(*position),
                        Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2),
                    ),
                    0.33,
                    gold,
                )
                .resolution(6);
        }
        let at = ground(block.centre);
        let front = ground(block.centre + Vec3::new(block.facing.x, 0.0, block.facing.y) * 4.0);
        gizmos.arrow(at, front, gold);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_drag_sets_frontage_centre_and_perpendicular_facing() {
        assert!(frontage_from_drag(Vec3::ZERO, Vec3::X).is_none());
        let (centre, f) = frontage_from_drag(Vec3::ZERO, Vec3::X * 80.0).unwrap();
        assert_eq!(centre, Vec3::X * 40.0);
        assert_eq!(f.facing, Vec2::Y);
        assert_eq!(f.width, 80.0);
    }
}
