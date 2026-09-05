//! Porter cart scenes, load slots, wheel travel and animation lifetime.

use super::animation::HeroAnim;
use super::attachments::{carried_asset_spec, CarriedLoadAssets};
use bevy::animation::AnimatedBy;
use bevy::gltf::Gltf;
use bevy::prelude::*;
use shared::components::CharacterKind;
use shared::economy::{CarriedAppearance, CarriedLoad, PorterCartState};

pub(super) const PORTER_CART_GLTF_PATH: &str = "game_assets/props/HandCart.glb";

pub(super) const PORTER_CART_SCENE_PATH: &str = "game_assets/props/HandCart.glb#Scene0";

pub(super) const PORTER_CART_WHEEL_RADIUS: f32 = 0.35;

pub(super) fn advanced_cart_wheel_angle(current: f32, ground_distance: f32) -> f32 {
    (current - ground_distance / PORTER_CART_WHEEL_RADIUS).rem_euclid(std::f32::consts::TAU)
}

/// Shared scene, source glTF and graph for every active porter cart.
#[derive(Resource, Default)]
pub(super) struct PorterCartAssets {
    pub(super) scene: Option<Handle<WorldAsset>>,
    pub(super) gltf: Option<Handle<Gltf>>,
    pub(super) graph: Option<PorterCartGraph>,
}

#[derive(Clone)]
pub(super) struct PorterCartGraph {
    pub(super) handle: Handle<AnimationGraph>,
    pub(super) pull: AnimationNodeIndex,
}

/// Link from a replicated character to its world-space handcart child.
#[derive(Component)]
pub(super) struct PorterCartLink(pub(super) Entity);

/// Root of the instantiated handcart scene. Parenting this directly beneath
/// the character root copies locomotion without inheriting skeleton bob.
#[derive(Component)]
pub(super) struct PorterCartVisual {
    pub(super) owner: Entity,
}

#[derive(Component)]
pub(super) struct PorterCartLoadAttachment {
    pub(super) owner: Entity,
    pub(super) slot: u8,
}

#[derive(Component)]
pub(super) struct PorterCartLoadVisual {
    pub(super) appearance: CarriedAppearance,
    pub(super) slot: u8,
}

#[derive(Component)]
pub(super) struct PorterCartAnimationTarget;

#[derive(Component)]
pub(super) struct PorterCartAnimation {
    pub(super) player: Entity,
    pub(super) pull: AnimationNodeIndex,
    pub(super) wheels: [Entity; 2],
    pub(super) wheel_rest: [Quat; 2],
    pub(super) wheel_angle: f32,
    pub(super) last_position: Vec3,
}

/// Add and remove the cart as a direct child of the replicated character.
/// Its authored root is already in the character's final game-space frame, so
/// no corrective offset, rotation or scale belongs here.
pub(super) fn sync_porter_cart_visuals(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut assets: ResMut<PorterCartAssets>,
    active: Query<(Entity, Option<&PorterCartLink>), (With<CharacterKind>, With<PorterCartState>)>,
    inactive: Query<(Entity, &PorterCartLink), (With<CharacterKind>, Without<PorterCartState>)>,
    cart_visuals: Query<(), With<PorterCartVisual>>,
) {
    for (owner, link) in active.iter() {
        if link.is_some_and(|link| cart_visuals.get(link.0).is_ok()) {
            continue;
        }

        let scene = assets
            .scene
            .get_or_insert_with(|| asset_server.load(PORTER_CART_SCENE_PATH))
            .clone();
        assets
            .gltf
            .get_or_insert_with(|| asset_server.load(PORTER_CART_GLTF_PATH));
        let cart = commands
            .spawn((
                Name::new("Porter Handcart"),
                PorterCartVisual { owner },
                Transform::IDENTITY,
                Visibility::default(),
                WorldAssetRoot(scene),
            ))
            .id();
        commands
            .entity(owner)
            .add_child(cart)
            .insert(PorterCartLink(cart));
    }

    for (owner, link) in inactive.iter() {
        if cart_visuals.get(link.0).is_ok() {
            commands.entity(link.0).despawn();
        }
        commands.entity(owner).remove::<PorterCartLink>();
    }
}

/// Resolve the two authored bed anchors to their replicated character owner.
/// This is a one-time hierarchy walk per instantiated cart, not a per-frame
/// scene-name search.
pub(super) fn tag_porter_cart_load_attachments(
    mut commands: Commands,
    named: Query<(Entity, &Name), (Added<Name>, Without<PorterCartLoadAttachment>)>,
    parents: Query<&ChildOf>,
    carts: Query<&PorterCartVisual>,
) {
    for (entity, name) in named.iter() {
        let slot = match name.as_str() {
            "Anchor_Load.1" => 1,
            "Anchor_Load.2" => 2,
            _ => continue,
        };
        let mut ancestor = entity;
        while let Ok(parent) = parents.get(ancestor) {
            ancestor = parent.parent();
            if let Ok(cart) = carts.get(ancestor) {
                commands.entity(entity).insert(PorterCartLoadAttachment {
                    owner: cart.owner,
                    slot,
                });
                break;
            }
        }
    }
}

/// Rebind if scene streaming ever replaces the animated descendants while
/// retaining the cart root.
pub(super) fn recover_stale_porter_cart_animation(
    mut commands: Commands,
    carts: Query<(Entity, &PorterCartAnimation)>,
    players: Query<(), With<AnimationPlayer>>,
    transforms: Query<(), With<Transform>>,
    children: Query<&Children>,
    wired: Query<(), With<PorterCartAnimationTarget>>,
) {
    for (root, animation) in carts.iter() {
        let stale = players.get(animation.player).is_err()
            || animation
                .wheels
                .iter()
                .any(|wheel| transforms.get(*wheel).is_err());
        if !stale {
            continue;
        }
        let mut stack = vec![root];
        while let Some(entity) = stack.pop() {
            if wired.get(entity).is_ok() {
                commands
                    .entity(entity)
                    .remove::<PorterCartAnimationTarget>();
            }
            if let Ok(entity_children) = children.get(entity) {
                stack.extend(entity_children.iter());
            }
        }
        commands.entity(root).remove::<PorterCartAnimation>();
    }
}

/// Bind the authored cart-body nod. Wheel rotation is intentionally excluded
/// from the graph and driven from measured world distance below.
#[allow(clippy::too_many_arguments)]
pub(super) fn setup_porter_cart_animation(
    mut commands: Commands,
    gltfs: Res<Assets<Gltf>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    mut assets: ResMut<PorterCartAssets>,
    targets: Query<(Entity, &Name, &AnimatedBy), Without<PorterCartAnimationTarget>>,
    mut players: Query<&mut AnimationPlayer>,
    parents: Query<&ChildOf>,
    children: Query<&Children>,
    names: Query<&Name>,
    transforms: Query<&Transform>,
    carts: Query<&PorterCartVisual, Without<PorterCartAnimation>>,
    owners: Query<&Transform, With<CharacterKind>>,
) {
    for (target, name, animated_by) in targets.iter() {
        if name.as_str() != "HandCartBody" {
            continue;
        }

        let mut ancestor = target;
        let mut cart_root = None;
        loop {
            if let Ok(cart) = carts.get(ancestor) {
                cart_root = Some((ancestor, cart));
                break;
            }
            let Ok(parent) = parents.get(ancestor) else {
                break;
            };
            ancestor = parent.parent();
        }
        let Some((root, cart)) = cart_root else {
            continue;
        };

        let mut wheels = [None, None];
        let mut stack = vec![root];
        while let Some(entity) = stack.pop() {
            if let Ok(node_name) = names.get(entity) {
                match node_name.as_str() {
                    "HandCartWheelL" => wheels[0] = Some(entity),
                    "HandCartWheelR" => wheels[1] = Some(entity),
                    _ => {}
                }
            }
            if let Ok(entity_children) = children.get(entity) {
                stack.extend(entity_children.iter());
            }
        }
        let [Some(wheel_l), Some(wheel_r)] = wheels else {
            continue;
        };
        let Ok([wheel_l_transform, wheel_r_transform]) = transforms.get_many([wheel_l, wheel_r])
        else {
            continue;
        };
        let Ok(owner_transform) = owners.get(cart.owner) else {
            continue;
        };
        let Ok(mut player) = players.get_mut(animated_by.0) else {
            continue;
        };

        let cart_graph = if let Some(graph) = &assets.graph {
            graph.clone()
        } else {
            let Some(gltf) = assets.gltf.as_ref().and_then(|handle| gltfs.get(handle)) else {
                continue;
            };
            let Some(clip) = gltf.named_animations.get("cart_pull") else {
                warn!("HandCart.glb has no cart_pull clip");
                continue;
            };
            let mut graph = AnimationGraph::new();
            let pull = graph.add_clip(clip.clone(), 1.0, graph.root);
            let graph = PorterCartGraph {
                handle: graphs.add(graph),
                pull,
            };
            assets.graph = Some(graph.clone());
            graph
        };

        player.play(cart_graph.pull).repeat().set_speed(0.0);
        commands
            .entity(animated_by.0)
            .insert(AnimationGraphHandle(cart_graph.handle.clone()));
        commands.entity(target).insert(PorterCartAnimationTarget);
        commands.entity(root).insert(PorterCartAnimation {
            player: animated_by.0,
            pull: cart_graph.pull,
            wheels: [wheel_l, wheel_r],
            wheel_rest: [wheel_l_transform.rotation, wheel_r_transform.rotation],
            wheel_angle: 0.0,
            last_position: owner_transform.translation,
        });
    }
}

/// Parent one or two copies of the authoritative carried-good appearance to
/// the cart bed. Cart slots use the resource GLBs at authored scale; the 1.35
/// hand-held compensation belongs only to the character's carry joint.
pub(super) fn sync_porter_cart_load_visuals(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut assets: ResMut<CarriedLoadAssets>,
    attachments: Query<(Entity, Ref<PorterCartLoadAttachment>)>,
    children: Query<&Children>,
    loads: Query<(Ref<CarriedLoad>, Ref<PorterCartState>), With<CharacterKind>>,
    existing_visuals: Query<&PorterCartLoadVisual>,
) {
    for (attachment, marker) in attachments.iter() {
        let Ok((load, cart)) = loads.get(marker.owner) else {
            continue;
        };
        if !marker.is_added() && !load.is_changed() && !cart.is_changed() {
            continue;
        }
        let desired = (marker.slot <= cart.load_slots)
            .then(|| load.visible_appearance())
            .flatten();
        let existing = children.get(attachment).ok().and_then(|children| {
            children.iter().find_map(|child| {
                existing_visuals
                    .get(child)
                    .ok()
                    .map(|visual| (child, visual.appearance, visual.slot))
            })
        });
        if existing
            .is_some_and(|(_, appearance, slot)| Some(appearance) == desired && slot == marker.slot)
        {
            continue;
        }
        if let Some((entity, _, _)) = existing {
            commands.entity(entity).despawn();
        }
        let Some(appearance) = desired else {
            continue;
        };
        let spec = carried_asset_spec(appearance);
        let scene = assets
            .scenes
            .entry(appearance)
            .or_insert_with(|| asset_server.load(spec.scene_path))
            .clone();
        commands.entity(attachment).with_children(|anchor| {
            anchor.spawn((
                Name::new(format!("Cart load {}", marker.slot)),
                PorterCartLoadVisual {
                    appearance,
                    slot: marker.slot,
                },
                WorldAssetRoot(scene),
                Transform::IDENTITY,
            ));
        });
    }
}

/// Keep the cart-body nod phase-identical to the character's pull clip and
/// rotate both wheels by actual ground distance (`theta = -distance / r`).
pub(super) fn drive_porter_cart_motion(
    mut carts: Query<(&PorterCartVisual, &mut PorterCartAnimation)>,
    heroes: Query<(&Transform, &HeroAnim), With<CharacterKind>>,
    mut animation_players: ParamSet<(Query<&AnimationPlayer>, Query<&mut AnimationPlayer>)>,
    mut wheel_transforms: Query<&mut Transform, Without<CharacterKind>>,
) {
    for (cart, mut animation) in carts.iter_mut() {
        let Ok((owner_transform, hero_anim)) = heroes.get(cart.owner) else {
            continue;
        };

        let hero_pose = {
            let players = animation_players.p0();
            let Some(pull) = hero_anim.pull else {
                continue;
            };
            let Ok(player) = players.get(hero_anim.player) else {
                continue;
            };
            let Some(active) = player.animation(pull) else {
                continue;
            };
            (active.seek_time(), active.speed(), active.weight())
        };
        {
            let mut players = animation_players.p1();
            let Ok(mut player) = players.get_mut(animation.player) else {
                continue;
            };
            if !player.is_playing_animation(animation.pull) {
                player.play(animation.pull).repeat();
            }
            if let Some(active) = player.animation_mut(animation.pull) {
                active.set_seek_time(hero_pose.0);
                active.set_speed(hero_pose.1);
                active.set_weight(hero_pose.2);
            }
        }

        let position = owner_transform.translation;
        let delta = Vec2::new(
            position.x - animation.last_position.x,
            position.z - animation.last_position.z,
        );
        let travelled = delta.length();
        animation.last_position = position;
        // Network correction/teleport: relocate the cart without presenting a
        // wildly spinning wheel for one frame. Ordinary 100x lab movement is
        // far below this threshold and remains distance-exact.
        if travelled <= 32.0 {
            animation.wheel_angle = advanced_cart_wheel_angle(animation.wheel_angle, travelled);
        }
        let wheel_rotation = Quat::from_rotation_x(animation.wheel_angle);
        for (index, wheel) in animation.wheels.into_iter().enumerate() {
            if let Ok(mut transform) = wheel_transforms.get_mut(wheel) {
                transform.rotation = animation.wheel_rest[index] * wheel_rotation;
            }
        }
    }
}
