use crate::camera_rts::CommanderCamera;
use bevy::{
    camera::primitives::{Frustum, Sphere},
    gltf::{Gltf, GltfMesh, GltfNode},
    prelude::*,
};
use shared::components::{
    CharacterMotion, Horse, HorseActivity, HorseAnimation, HorseGait, PlayerPosition,
    PlayerRotation, TimeWarp, WorldTime, HORSE_SCENE,
};

const MAX_WILD_RIGS: usize = 32;
const MAX_MOUNTED_RIGS: usize = 160;
const RIG_DISTANCE: f32 = 220.0;
const SCENES_PER_FRAME: usize = 2;
const ACTIVITIES: [HorseActivity; 7] = [
    HorseActivity::Idle,
    HorseActivity::Graze,
    HorseActivity::Alert,
    HorseActivity::Moving(HorseGait::Walk),
    HorseActivity::Moving(HorseGait::Trot),
    HorseActivity::Moving(HorseGait::Canter),
    HorseActivity::Moving(HorseGait::Gallop),
];

#[derive(Resource, Default)]
pub(super) struct HorseAssets {
    scene: Option<Handle<WorldAsset>>,
    gltf: Option<Handle<Gltf>>,
    graph: Option<(Handle<AnimationGraph>, [AnimationNodeIndex; 7])>,
    proxy: Option<(Handle<Mesh>, Handle<StandardMaterial>)>,
    rest_seat: Option<Transform>,
}
#[derive(Component)]
pub(crate) struct HorseVisual {
    position: Vec3,
    received_at: f64,
    ground_normal: Option<Vec3>,
    scene: Option<Entity>,
    proxy: Option<Entity>,
}
#[derive(Component)]
pub(crate) struct HorseRig {
    pub(crate) socket_path: Vec<Entity>,
    pub(crate) bind_inverse: Quat,
    player: Entity,
    current: Option<usize>,
    previous: Option<usize>,
    fade_seconds: f32,
}

pub(super) fn attach(
    mut commands: Commands,
    time: Res<Time>,
    horses: Query<(Entity, &PlayerPosition, &PlayerRotation), (With<Horse>, Without<HorseVisual>)>,
) {
    for (entity, p, r) in &horses {
        commands.entity(entity).insert((
            Transform::from_translation(p.0).with_rotation(Quat::from_rotation_y(r.0)),
            Visibility::Hidden,
            HorseVisual {
                position: p.0,
                received_at: time.elapsed_secs_f64(),
                ground_normal: None,
                scene: None,
                proxy: None,
            },
        ));
    }
}

// Deterministic ties and a hard budget across the whole client. Population size
// and camera zoom cannot silently multiply skeletons. Spawn at most two per frame.
fn within_budget(candidates: &mut Vec<(Entity, f32, u64)>, limit: usize) {
    candidates.sort_unstable_by(|a, b| a.1.total_cmp(&b.1).then(a.2.cmp(&b.2)));
    candidates.truncate(limit);
}

pub(super) fn select_rigs(
    mut commands: Commands,
    time: Res<Time>,
    cameras: Query<&CommanderCamera>,
    asset_server: Res<AssetServer>,
    mut assets: ResMut<HorseAssets>,
    mut horses: Query<(
        Entity,
        &Horse,
        &PlayerPosition,
        &mut HorseVisual,
        &mut Visibility,
    )>,
    mut candidates: Local<Vec<(Entity, f32, u64)>>,
    mut mounted_candidates: Local<Vec<(Entity, f32, u64)>>,
    mut proxies: Query<&mut Visibility, (Without<Horse>, With<HorseProxy>)>,
    mut scenes: Query<&mut Visibility, (Without<Horse>, Without<HorseProxy>)>,
    mut selected: Local<bevy::platform::collections::HashSet<Entity>>,
) {
    let Ok(camera) = cameras.single() else {
        return;
    };
    candidates.clear();
    mounted_candidates.clear();
    for (e, h, p, visual, _) in &horses {
        let d = p.0.distance_squared(camera.focus);
        let retained = visual.scene.is_some();
        let radius = RIG_DISTANCE + if retained { 25. } else { 0. };
        let zoom_limit = if retained { 800. } else { 700. };
        if d < radius * radius && camera.zoom < zoom_limit {
            let list = if h.rider.is_some() {
                &mut *mounted_candidates
            } else {
                &mut *candidates
            };
            list.push((e, d * if retained { 0.85 } else { 1.0 }, h.id));
        }
    }
    within_budget(&mut candidates, MAX_WILD_RIGS);
    within_budget(&mut mounted_candidates, MAX_MOUNTED_RIGS);
    candidates.extend(mounted_candidates.iter().copied());
    selected.clear();
    selected.extend(candidates.iter().map(|c| c.0));
    let mut spawned = 0;
    for (e, horse, p, mut visual, mut visibility) in &mut horses {
        if horse.rider.is_some() && visual.proxy.is_none() {
            if let (Some((mesh, material)), Some(seat)) = (&assets.proxy, assets.rest_seat) {
                visual.proxy = Some(
                    commands
                        .spawn((
                            HorseProxy,
                            Mesh3d(mesh.clone()),
                            MeshMaterial3d(material.clone()),
                            Transform::default(),
                            Visibility::Hidden,
                            ChildOf(e),
                        ))
                        .id(),
                );
                commands.entity(e).insert(HorseRestSeat(seat));
            }
        }
        let wanted = selected.contains(&e);
        if !wanted {
            visibility.set_if_neq(if horse.rider.is_some() && visual.proxy.is_some() {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            });
            if let Some(proxy) = visual.proxy {
                if let Ok(mut v) = proxies.get_mut(proxy) {
                    v.set_if_neq(if horse.rider.is_some() {
                        Visibility::Inherited
                    } else {
                        Visibility::Hidden
                    });
                }
            }
            if let Some(scene) = visual.scene.take() {
                commands.entity(scene).try_despawn();
                commands.entity(e).remove::<HorseRig>();
            }
        } else if visual.scene.is_none() && spawned < SCENES_PER_FRAME {
            let scene = assets
                .scene
                .get_or_insert_with(|| asset_server.load(HORSE_SCENE))
                .clone();
            assets.gltf.get_or_insert_with(|| {
                asset_server.load(HORSE_SCENE.split('#').next().unwrap().to_owned())
            });
            visual.scene = Some(
                commands
                    .spawn((
                        WorldAssetRoot(scene),
                        Transform::default(),
                        Visibility::Hidden,
                        ChildOf(e),
                    ))
                    .id(),
            );
            visual.position = p.0;
            visual.received_at = time.elapsed_secs_f64();
            spawned += 1;
            // Reveal after the player is configured; never flash a rest-pose rig.
            visibility.set_if_neq(if visual.proxy.is_some() {
                Visibility::Inherited
            } else {
                Visibility::Hidden
            });
        } else if wanted {
            if let Some(scene) = visual.scene {
                let ready = scenes
                    .get_mut(scene)
                    .is_ok_and(|v| *v != Visibility::Hidden);
                if let Some(proxy) = visual.proxy {
                    if let Ok(mut v) = proxies.get_mut(proxy) {
                        v.set_if_neq(if ready {
                            Visibility::Hidden
                        } else {
                            Visibility::Inherited
                        });
                    }
                }
            }
        }
    }
}

pub(super) fn setup_animation(
    mut commands: Commands,
    mut assets: ResMut<HorseAssets>,
    gltfs: Res<Assets<Gltf>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    children: Query<&Children>,
    names: Query<&Name>,
    transforms: Query<&Transform>,
    parents: Query<&ChildOf>,
    mut scene_visibility: Query<&mut Visibility, Without<HorseVisual>>,
    players: Query<(), With<AnimationPlayer>>,
    mut horses: Query<(Entity, &HorseVisual, &mut Visibility), Without<HorseRig>>,
) {
    if horses.is_empty() {
        return;
    }
    if assets.graph.is_none() {
        let Some(gltf) = assets.gltf.as_ref().and_then(|h| gltfs.get(h)) else {
            return;
        };
        let mut graph = AnimationGraph::new();
        let nodes = ACTIVITIES.map(|activity| {
            let clip = gltf
                .named_animations
                .get(activity.clip())
                .unwrap_or_else(|| panic!("Horse asset is missing {}", activity.clip()));
            graph.add_clip(clip.clone(), 1., graph.root)
        });
        assets.graph = Some((graphs.add(graph), nodes));
    }
    let (graph, _) = assets.graph.as_ref().unwrap();
    for (entity, visual, mut visibility) in &mut horses {
        let Some(scene) = visual.scene else {
            continue;
        };
        let Some(player) = children
            .iter_descendants(scene)
            .find(|e| players.contains(*e))
        else {
            continue;
        };
        let Some(anchor) = children.iter_descendants(scene).find(|e| {
            names
                .get(*e)
                .is_ok_and(|name| name.as_str() == "Anchor_Rider")
        }) else {
            continue;
        };
        let mut socket_path = Vec::new();
        let mut current = anchor;
        let mut bind = Quat::IDENTITY;
        while current != entity {
            let Ok(t) = transforms.get(current) else {
                break;
            };
            bind = t.rotation * bind;
            socket_path.push(current);
            let Ok(parent) = parents.get(current) else {
                break;
            };
            current = parent.parent();
        }
        socket_path.reverse();
        if let Ok(mut visible) = scene_visibility.get_mut(scene) {
            *visible = Visibility::Inherited;
        }
        if let Some(proxy) = visual.proxy {
            if let Ok(mut visible) = scene_visibility.get_mut(proxy) {
                *visible = Visibility::Hidden;
            }
        }
        commands
            .entity(player)
            .insert(AnimationGraphHandle(graph.clone()));
        commands.entity(entity).insert(HorseRig {
            socket_path,
            bind_inverse: bind.inverse(),
            player,
            current: None,
            previous: None,
            fade_seconds: 0.,
        });
        visibility.set_if_neq(Visibility::Inherited);
    }
}

pub(super) fn animate(
    time: Res<Time>,
    clocks: Query<&WorldTime>,
    presentation: Option<Res<crate::animation_clock::AnimationClock>>,
    terrain: Option<Res<shared::terrain::WorldTerrain>>,
    warp: Query<&TimeWarp>,
    frusta: Query<&Frustum, With<Camera3d>>,
    assets: Res<HorseAssets>,
    mut horses: Query<(
        Ref<PlayerPosition>,
        &PlayerRotation,
        &CharacterMotion,
        &HorseAnimation,
        &mut HorseVisual,
        &mut Transform,
        Option<&mut HorseRig>,
        &Visibility,
    )>,
    mut players: Query<&mut AnimationPlayer>,
) {
    let nodes = assets.graph.as_ref().map(|(_, nodes)| nodes);
    let now = clocks.iter().next().map_or(0., |clock| {
        presentation.as_ref().map_or_else(
            || crate::animation_clock::seconds(clock),
            |presentation| presentation.sample(clock),
        )
    });
    let factor = crate::hero::motion::visual_time_factor(warp.iter().next().map_or(1., |w| w.0));
    let blend = crate::hero::motion::visual_position_blend(time.delta_secs(), factor);
    for (p, r, motion, animation, mut visual, mut transform, rig, visibility) in &mut horses {
        if p.is_changed() {
            visual.position = p.0;
            visual.received_at = time.elapsed_secs_f64();
        }
        if p.is_changed() || visual.ground_normal.is_none() {
            visual.ground_normal = Some(
                terrain
                    .as_ref()
                    .map_or(Vec3::Y, |t| t.get_normal(p.0.x, p.0.z)),
            );
        }
        let target = crate::hero::motion::extrapolated_motion_target(
            visual.position,
            motion.velocity,
            time.elapsed_secs_f64() - visual.received_at,
            factor,
        );
        transform.translation = if transform.translation.distance_squared(target) > 400. {
            target
        } else {
            transform.translation.lerp(target, blend)
        };
        transform.rotation = transform.rotation.slerp(
            Quat::from_rotation_arc(Vec3::Y, visual.ground_normal.unwrap_or(Vec3::Y))
                * Quat::from_rotation_y(r.0),
            blend,
        );
        let Some(nodes) = nodes else {
            continue;
        };
        let Some(mut rig) = rig else {
            continue;
        };
        let Ok(mut player) = players.get_mut(rig.player) else {
            continue;
        };
        let in_view = *visibility != Visibility::Hidden
            && frusta.iter().any(|f| {
                f.intersects_sphere(
                    &Sphere {
                        center: (transform.translation + Vec3::Y).into(),
                        radius: 4.0,
                    },
                    false,
                )
            });
        if !in_view {
            // Pause alone still evaluates bones. Zero weight removes that work.
            for (_, active) in player.playing_animations_mut() {
                active.pause().set_weight(0.);
            }
            continue;
        }
        let index = ACTIVITIES
            .iter()
            .position(|a| *a == animation.activity)
            .unwrap();
        if rig.current != Some(index) {
            if let Some(previous) = rig.previous.take() {
                player.stop(nodes[previous]);
            }
            rig.previous = rig.current;
            rig.fade_seconds = 0.;
            player.start(nodes[index]);
            rig.current = Some(index);
        }
        rig.fade_seconds += time.delta_secs();
        let weight = (rig.fade_seconds / 0.18).clamp(0., 1.);
        if let Some(previous) = rig.previous {
            if weight >= 1. {
                player.stop(nodes[previous]);
                rig.previous = None;
            } else if let Some(active) = player.animation_mut(nodes[previous]) {
                active.pause().set_weight(1. - weight);
            }
        }
        if let Some(active) = player.animation_mut(nodes[index]) {
            active
                .pause()
                .set_weight(if rig.previous.is_some() { weight } else { 1. })
                .set_seek_time(animation.sample(now));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn mounted_and_wild_rig_budgets_are_separate_and_bounded() {
        let all: Vec<_> = (0..500)
            .map(|id| (Entity::PLACEHOLDER, id as f32, id))
            .collect();
        let mut wild = all.clone();
        let mut mounted = all;
        within_budget(&mut wild, MAX_WILD_RIGS);
        within_budget(&mut mounted, MAX_MOUNTED_RIGS);
        assert_eq!(wild.len(), 32);
        assert_eq!(mounted.len(), 160);
    }
    #[test]
    fn horse_rig_budget_is_bounded_and_ties_follow_identity() {
        let mut horses: Vec<_> = (0..100)
            .rev()
            .map(|id| (Entity::PLACEHOLDER, 1., id))
            .collect();
        within_budget(&mut horses, MAX_WILD_RIGS);
        assert_eq!(horses.len(), MAX_WILD_RIGS);
        assert_eq!(
            horses.iter().map(|c| c.2).collect::<Vec<_>>(),
            (0..32).collect::<Vec<_>>()
        );
    }
}

/// A static copy of the authored horse mesh, sharing its material. Mounted
/// units keep this silhouette outside the skeleton budget and at wide zoom.
#[derive(Component)]
pub(super) struct HorseProxy;
#[derive(Component, Clone, Copy)]
pub(crate) struct HorseRestSeat(pub(crate) Transform);

pub(super) fn setup_proxy_assets(
    mut assets: ResMut<HorseAssets>,
    server: Res<AssetServer>,
    gltfs: Res<Assets<Gltf>>,
    nodes: Res<Assets<GltfNode>>,
    gltf_meshes: Res<Assets<GltfMesh>>,
    materials: Res<Assets<StandardMaterial>>,
    mut meshes: ResMut<Assets<Mesh>>,
) {
    if assets.proxy.is_some() {
        return;
    }
    let handle = assets
        .gltf
        .get_or_insert_with(|| server.load(HORSE_SCENE.split('#').next().unwrap().to_owned()));
    let Some(gltf) = gltfs.get(&*handle) else {
        return;
    };
    let Some(primitive) = gltf
        .meshes
        .first()
        .and_then(|h| gltf_meshes.get(h))
        .and_then(|m| m.primitives.first())
    else {
        return;
    };
    let Some(mesh) = meshes.get(&primitive.mesh) else {
        return;
    };
    let Some(path) = primitive.material.as_ref().and_then(|h| h.path()) else {
        return;
    };
    let Some(label) = path.label() else {
        return;
    };
    // Bevy 0.19's PBR glTF extension stores the converted, shared material
    // alongside the source descriptor at `<material label>/std`.
    let material: Handle<StandardMaterial> =
        server.load(path.clone().with_label(format!("{label}/std")));
    if materials.get(&material).is_none() {
        return;
    }
    let Some(mut node) = gltf
        .named_nodes
        .get("Anchor_Rider")
        .and_then(|h| nodes.get(h))
    else {
        return;
    };
    let mut seat = node.transform;
    // Setup-only traversal of the tiny asset graph. The proxy seat comes from
    // the asset itself, so future changes in horse size cannot strand a rider.
    for _ in 0..32 {
        let Some(parent) = gltf
            .nodes
            .iter()
            .filter_map(|h| nodes.get(h))
            .find(|candidate| {
                candidate
                    .children
                    .iter()
                    .any(|h| nodes.get(h).is_some_and(|child| child.index == node.index))
            })
        else {
            break;
        };
        seat = parent.transform * seat;
        node = parent;
    }
    seat.rotation = Quat::IDENTITY;
    let mut proxy = mesh.clone();
    proxy.remove_attribute(Mesh::ATTRIBUTE_JOINT_INDEX);
    proxy.remove_attribute(Mesh::ATTRIBUTE_JOINT_WEIGHT);
    assets.proxy = Some((meshes.add(proxy), material));
    assets.rest_seat = Some(seat);
}
