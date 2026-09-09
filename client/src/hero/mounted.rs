//! Pair one replicated character with its horse without reparenting either
//! authoritative entity. Only the cosmetic scene child follows the saddle socket.
use super::{
    animation::HeroAnim,
    appearance::{HeroDressed, HeroSceneRoot},
    motion::HeroVisual,
};
use bevy::{platform::collections::HashMap, prelude::*};
use shared::components::{Horse, HorseAnimation, Mounted, WorldTime};

#[derive(Component)]
pub(super) struct MountedScene;

#[derive(Component)]
pub(crate) struct MountedVisual {
    pub(crate) horse: Entity,
    pub(crate) horse_id: u64,
    pub(crate) ready: bool,
    pub(crate) lower_clip: &'static str,
    pub(crate) upper_clip: &'static str,
    pub(crate) phase: f32,
}

pub(super) fn bind_riders(
    mut commands: Commands,
    horses: Query<(Entity, &Horse)>,
    riders: Query<(Entity, &Mounted, Option<&MountedVisual>), With<HeroVisual>>,
    former: Query<Entity, (With<MountedVisual>, Without<Mounted>)>,
    mut index: Local<HashMap<u64, Entity>>,
) {
    index.clear();
    index.extend(horses.iter().map(|(e, horse)| (horse.id, e)));
    for (entity, mounted, visual) in &riders {
        let horse = index
            .get(&mounted.horse)
            .copied()
            .unwrap_or(Entity::PLACEHOLDER);
        if visual.is_some_and(|v| v.horse == horse && v.horse_id == mounted.horse) {
            continue;
        }
        commands.entity(entity).try_insert(MountedVisual {
            horse,
            horse_id: mounted.horse,
            ready: false,
            lower_clip: "ride_idle",
            upper_clip: "ride_idle",
            phase: 0.,
        });
    }
    for entity in &former {
        commands.entity(entity).remove::<MountedVisual>();
    }
}

#[allow(clippy::type_complexity)]
pub(super) fn follow_sockets(
    mut commands: Commands,
    clocks: Query<&WorldTime>,
    presentation: Option<Res<crate::animation_clock::AnimationClock>>,
    riders: Query<
        (
            &Transform,
            &Children,
            Option<&Mounted>,
            Option<&HeroAnim>,
            Has<HeroDressed>,
            Option<&shared::components::CharacterActivity>,
            Option<&mut MountedVisual>,
        ),
        (With<HeroVisual>, Without<HeroSceneRoot>),
    >,
    horses: Query<
        (
            &Transform,
            &Visibility,
            Option<&crate::animals::HorseRig>,
            Option<&crate::animals::HorseRestSeat>,
            &HorseAnimation,
        ),
        (With<Horse>, Without<HeroSceneRoot>),
    >,
    bones: Query<&Transform, (Without<HeroSceneRoot>, Without<HeroVisual>, Without<Horse>)>,
    mut scenes: Query<
        (&mut Transform, &mut Visibility, Option<&MountedScene>),
        With<HeroSceneRoot>,
    >,
) {
    let now = clocks.iter().next().map_or(0., |clock| {
        presentation.as_ref().map_or_else(
            || crate::animation_clock::seconds(clock),
            |presentation| presentation.sample(clock),
        )
    });
    for (rider, children, mounted, anim, dressed, activity, visual) in riders {
        let Some(mut visual) = visual else {
            // A dismounted/dead rider returns to the normal ground-based scene.
            for child in children.iter() {
                if let Ok((mut t, mut v, Some(_))) = scenes.get_mut(child) {
                    *t = Transform::IDENTITY;
                    *v = if dressed
                        && activity != Some(&shared::components::CharacterActivity::Indoors)
                    {
                        Visibility::Inherited
                    } else {
                        Visibility::Hidden
                    };
                    commands.entity(child).remove::<MountedScene>();
                }
            }
            continue;
        };
        let Some(mounted) = mounted else {
            continue;
        };
        let horse = horses.get(visual.horse).ok();
        visual.ready = false;
        let seat = horse.and_then(|(horse, visibility, rig, rest, animation)| {
            if *visibility == Visibility::Hidden || !dressed || anim.is_none() {
                return None;
            }
            let socket = if let Some(rig) = rig {
                let mut socket = Transform::IDENTITY;
                for node in &rig.socket_path {
                    socket = socket * *bones.get(*node).ok()?;
                }
                socket.rotation *= rig.bind_inverse;
                visual.ready = true;
                socket
            } else {
                rest?.0
            };
            if let Some(anim) = anim {
                let (lower, upper, phase) = anim.riding.labels(mounted, animation, now);
                visual.lower_clip = lower;
                visual.upper_clip = upper;
                visual.phase = phase;
            }
            // Root stays at the smoothed ground position for click bounds,
            // selection rings and command previews. The rendered rider shares
            // the horse's exact visual transform and animated socket.
            Some(relative_seat(rider, horse, &socket))
        });
        let indoors =
            activity.is_some_and(|a| *a == shared::components::CharacterActivity::Indoors);
        for child in children.iter() {
            if let Ok((mut transform, mut visibility, marker)) = scenes.get_mut(child) {
                if marker.is_none() {
                    commands.entity(child).insert(MountedScene);
                }
                if let Some(seat) = seat {
                    *transform = seat;
                }
                visibility.set_if_neq(if seat.is_some() && !indoors {
                    Visibility::Inherited
                } else {
                    Visibility::Hidden
                });
            }
        }
    }
}

fn relative_seat(rider: &Transform, horse: &Transform, socket: &Transform) -> Transform {
    Transform::from_matrix(rider.to_matrix().inverse() * horse.to_matrix() * socket.to_matrix())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn socket_queries_keep_horse_bones_and_scene_mutation_disjoint() {
        use bevy::ecs::system::RunSystemOnce;
        World::new().run_system_once(follow_sockets).unwrap();
    }
    #[test]
    fn animated_socket_stays_exact_when_network_roots_have_different_smoothing() {
        let person = Transform::from_xyz(10., 0., 20.).with_rotation(Quat::from_rotation_y(0.4));
        let horse = Transform::from_xyz(10.1, 0.3, 20.1).with_rotation(Quat::from_euler(
            EulerRot::YXZ,
            0.6,
            0.08,
            0.,
        ));
        let socket = Transform::from_xyz(0., 1.77, 0.12).with_rotation(Quat::from_rotation_x(0.02));
        let scene = relative_seat(&person, &horse, &socket);
        let actual = person.to_matrix() * scene.to_matrix();
        let expected = horse.to_matrix() * socket.to_matrix();
        assert!(actual.abs_diff_eq(expected, 0.00001));
        assert_eq!(person.translation, Vec3::new(10., 0., 20.));
    }
}
