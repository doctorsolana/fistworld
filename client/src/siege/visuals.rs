use super::*;
use crate::selection::{Selectable, SelectableShape};
#[derive(Component)]
pub(super) struct CatapultVisual {
    last: Vec3,
    wheel: f32,
}
#[derive(Component)]
pub struct CatapultSceneReady;
#[derive(Clone, Copy)]
enum Part {
    Arm,
    Wheel,
    Winch,
    Stone,
}
#[derive(Component)]
pub(super) struct Rig {
    owner: Entity,
    part: Part,
    rest: Transform,
}

pub(super) fn attach(
    mut commands: Commands,
    assets: Res<AssetServer>,
    effects: Res<super::effects::SiegeAssets>,
    units: Query<
        (Entity, &PlayerPosition, &PlayerRotation),
        (With<Catapult>, Without<CatapultVisual>),
    >,
) {
    for (entity, position, rotation) in &units {
        commands.entity(entity).insert((
            Name::new("Catapult"),
            CatapultVisual {
                last: position.0,
                wheel: 0.0,
            },
            Transform::from_translation(position.0)
                .with_rotation(Quat::from_rotation_y(rotation.0)),
            Visibility::default(),
            WorldAssetRoot(assets.load("game_assets/vehicles/Catapult.glb#Scene0")),
            selectable(rotation.0),
        ));
        commands.spawn((
            Name::new("Loaded stone"),
            Mesh3d(effects.stone.clone()),
            MeshMaterial3d(effects.rock.clone()),
            Transform::default(),
            Rig {
                owner: entity,
                part: Part::Stone,
                rest: Transform::IDENTITY,
            },
            ChildOf(entity),
        ));
    }
}
fn selectable(rotation: f32) -> Selectable {
    Selectable {
        radius: CATAPULT_CLEARANCE,
        height: 4.1,
        shape: SelectableShape::Footprint {
            half_extents: Vec2::new(1.55, 2.45),
            centre_offset: Vec2::ZERO,
            rotation,
        },
    }
}
pub(super) fn tag_rig(
    mut commands: Commands,
    named: Query<(Entity, &Name, &Transform), Added<Name>>,
    parents: Query<&ChildOf>,
    owners: Query<(), With<Catapult>>,
) {
    for (entity, name, transform) in &named {
        let part = if name.as_str() == "CatapultArm" {
            Part::Arm
        } else if name.starts_with("Wheel") {
            Part::Wheel
        } else if name.as_str() == "Winch" {
            Part::Winch
        } else {
            continue;
        };
        let mut cursor = entity;
        for _ in 0..12 {
            if owners.contains(cursor) {
                commands.entity(entity).insert(Rig {
                    owner: cursor,
                    part,
                    rest: *transform,
                });
                if matches!(part, Part::Arm) {
                    commands.entity(cursor).insert(CatapultSceneReady);
                }
                break;
            }
            let Ok(parent) = parents.get(cursor) else {
                break;
            };
            cursor = parent.parent();
        }
    }
}
pub(super) fn animate(
    time: Res<Time>,
    clock: Query<&WorldTime>,
    terrain: Res<shared::terrain::WorldTerrain>,
    mut machines: Query<
        (
            &PlayerPosition,
            &PlayerRotation,
            &CatapultStatus,
            &Catapult,
            &mut Transform,
            &mut CatapultVisual,
            &mut Selectable,
        ),
        Without<Rig>,
    >,
    mut rigs: Query<(&Rig, &mut Transform, Option<&mut Visibility>), Without<CatapultVisual>>,
    mut rope: Gizmos,
) {
    let now = clock.iter().next().map_or(0.0, super::seconds);
    let blend = 1.0 - (-time.delta_secs() / 0.06).exp();
    for (position, rotation, status, _, mut transform, mut visual, mut pick) in &mut machines {
        let next = transform.translation.lerp(position.0, blend);
        let forward = transform.rotation * Vec3::NEG_Z;
        visual.wheel -= (next - visual.last).dot(forward) / 0.61;
        visual.last = next;
        transform.translation = Vec3::new(next.x, terrain.get_height(next.x, next.z), next.z);
        let wreck = if status.phase == SiegePhase::Destroyed {
            Quat::from_rotation_z(-0.16)
        } else {
            Quat::IDENTITY
        };
        transform.rotation = transform
            .rotation
            .slerp(Quat::from_rotation_y(rotation.0) * wreck, blend);
        pick.set_if_neq(selectable(rotation.0));
        let angle = catapult_arm_angle(status, now);
        let arm =
            Vec3::new(0.0, 1.75, 0.0) + Quat::from_rotation_x(angle) * Vec3::new(0.0, 0.0, 1.75);
        // Tension cable follows the spoon during reset; it goes slack at release.
        if now - status.fire_at > 0.9
            || status.phase == SiegePhase::Winding
            || status.fire_at == 0.0
        {
            let winch = Vec3::new(0.0, 1.2, 1.73);
            rope.line(
                transform.transform_point(winch),
                transform.transform_point(arm),
                Color::srgb(0.58, 0.43, 0.23),
            );
        }
    }
    for (rig, mut transform, visibility) in &mut rigs {
        let Ok((_, _, status, catapult, _, visual, _)) = machines.get(rig.owner) else {
            continue;
        };
        let angle = catapult_arm_angle(status, now);
        *transform = rig.rest;
        match rig.part {
            Part::Arm => transform.rotation = Quat::from_rotation_x(angle) * rig.rest.rotation,
            Part::Wheel => {
                transform.rotation = Quat::from_rotation_x(visual.wheel) * rig.rest.rotation
            }
            Part::Winch => {
                transform.rotation = Quat::from_rotation_x(angle * 4.0) * rig.rest.rotation
            }
            Part::Stone => {
                transform.translation = catapult_stone_socket(status, now);
                let loaded = catapult.ammunition > 0
                    && (status.fire_at == 0.0
                        || now < status.fire_at + 0.14
                        || now >= status.ready_at - 0.7);
                if let Some(mut visible) = visibility {
                    *visible = if loaded {
                        Visibility::Inherited
                    } else {
                        Visibility::Hidden
                    };
                }
            }
        }
    }
}
