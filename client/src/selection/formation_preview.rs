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
    roster_revision: u64,
    selection: Vec<Entity>,
    blocks: Vec<shared::formation::FormationBlock>,
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

#[derive(Component)]
pub struct FormationLabel;

pub fn spawn_preview_label(mut commands: Commands) {
    commands.spawn((
        FormationLabel,
        DespawnOnExit(crate::states::GameState::Playing),
        Pickable::IGNORE,
        GlobalZIndex(56),
        Node {
            position_type: PositionType::Absolute,
            bottom: Val::Px(248.0),
            left: Val::Percent(50.0),
            width: Val::Px(520.0),
            margin: UiRect::left(Val::Px(-260.0)),
            padding: UiRect::all(Val::Px(10.0)),
            border: UiRect::all(Val::Px(1.0)),
            display: Display::None,
            ..default()
        },
        BackgroundColor(crate::ui::styles::SIGN_WOOD),
        BorderColor::all(crate::ui::styles::BRASS),
        Text::new(""),
        crate::ui::typography::body(16.0),
        TextColor(crate::ui::styles::PARCHMENT),
        TextLayout::justify(Justify::Center),
    ));
}

pub fn draw_formation_preview(
    drag: Res<super::RightDrag>,
    selection: Res<super::Selection>,
    roster: Res<crate::army_roster::ArmyRoster>,
    hit: Res<crate::camera_rts::CursorTerrainHit>,
    positions: Query<(&PlayerPosition, Option<&shared::components::FormationSeat>)>,
    terrain: Option<Res<shared::terrain::WorldTerrain>>,
    input: Res<crate::input::InputState>,
    mut readiness: ResMut<PreviewReadiness>,
    mut gizmos: Gizmos<FormationGizmos>,
    mut label: Query<(&mut Text, &mut Node), With<FormationLabel>>,
) {
    let placement = drag
        .formation_start
        .zip(hit.0)
        .and_then(|(a, b)| frontage_from_drag(a, b));
    let visible = drag.formation
        && !input.ui_blocking()
        && placement.is_some()
        && selection
            .entities
            .iter()
            .any(|e| roster.soldiers.contains_key(e));
    for (_, mut node) in &mut label {
        let display = if visible {
            Display::Flex
        } else {
            Display::None
        };
        if node.display != display {
            node.display = display;
        }
    }
    if !visible {
        if readiness.slots != 0 {
            *readiness = PreviewReadiness::default();
        }
        return;
    }
    let (target, frontage) = placement.unwrap();
    let rebuild = readiness.target != placement
        || readiness.roster_revision != roster.revision
        || readiness.selection != selection.entities;
    if !rebuild {
        readiness.stable_frames += 1;
    } else {
        readiness.target = placement;
        readiness.stable_frames = 0;
        readiness.roster_revision = roster.revision;
        readiness.selection.clone_from(&selection.entities);
        let mut groups = BTreeMap::<u64, Vec<FormationSoldier>>::new();
        let mut loose = 0;
        for entity in &selection.entities {
            let Some(soldier) = roster.soldiers.get(entity) else {
                continue;
            };
            let Ok((position, seat)) = positions.get(*entity) else {
                continue;
            };
            let key = soldier.battalion.map(|id| id.0).unwrap_or_else(|| {
                let key = u64::MAX - (loose / MAX_BATTALION_SIZE) as u64;
                loose += 1;
                key
            });
            groups.entry(key).or_default().push(FormationSoldier {
                entity: *entity,
                seat: seat.map(|s| s.0),
                identity: soldier.identity,
                position: position.0,
            });
        }
        let blocks = shared::formation::layout(
            groups
                .into_iter()
                .map(|(key, soldiers)| FormationGroup {
                    key,
                    role: soldiers
                        .iter()
                        .filter_map(|s| roster.soldiers.get(&s.entity))
                        .map(|s| s.role)
                        .find(|role| *role == shared::components::SoldierRole::Cavalry)
                        .unwrap_or_default(),
                    shape: roster
                        .battalions
                        .iter()
                        .find(|b| b.id.0 == key)
                        .map_or(default(), |b| b.formation),
                    soldiers,
                })
                .collect(),
            target,
            Some(frontage),
        );
        readiness.slots = blocks.iter().map(|b| b.slots.len()).sum();
        let summary = if let Some(first) = blocks.first().filter(|first| {
            blocks.iter().all(|b| {
                b.files == first.files
                    && b.slots.len().div_ceil(b.files) == first.slots.len().div_ceil(first.files)
            })
        }) {
            format!(
                "{} battalion{} · {} across · {} ranks each",
                blocks.len(),
                if blocks.len() == 1 { "" } else { "s" },
                first.files,
                first.slots.len().div_ceil(first.files)
            )
        } else {
            blocks
                .iter()
                .map(|b| {
                    format!(
                        "{} across · {} ranks",
                        b.files,
                        b.slots.len().div_ceil(b.files)
                    )
                })
                .collect::<Vec<_>>()
                .join("   |   ")
        };
        let text = format!(
            "{} soldiers · {}\nRelease to deploy · Reverse the drag to reverse facing",
            readiness.slots, summary
        );
        for (mut label, _) in &mut label {
            if label.0 != text {
                label.0.clone_from(&text);
            }
        }
        readiness.blocks = blocks;
    }
    let ground = |mut p: Vec3| {
        if let Some(terrain) = terrain.as_deref() {
            p.y = terrain.get_height(p.x, p.z);
        }
        p + Vec3::Y * 0.15
    };
    let gold = Color::srgb(1.0, 0.82, 0.32);
    for block in &readiness.blocks {
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
        let right = Vec3::new(block.facing.y, 0.0, -block.facing.x);
        let forward = Vec3::new(block.facing.x, 0.0, block.facing.y);
        let half = (block.files - 1) as f32 * block.spacing * 0.5 + 0.65;
        let depth =
            (block.slots.len().div_ceil(block.files) - 1) as f32 * block.rank_spacing + 0.65;
        let corners = [
            block.centre - right * half + forward * 0.65,
            block.centre + right * half + forward * 0.65,
            block.centre + right * half - forward * depth,
            block.centre - right * half - forward * depth,
        ];
        for i in 0..4 {
            gizmos.line(
                ground(corners[i]),
                ground(corners[(i + 1) % 4]),
                gold.with_alpha(0.7),
            );
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
