//! Authored carry/tool joints, work props and camera head anchors.

use bevy::platform::collections::{HashMap, HashSet};
use bevy::prelude::*;
use shared::components::{CharacterActivity, CharacterKind};
use shared::economy::{CarriedAppearance, CarriedLoad, PorterCartState};

/// Authored carried-goods scenes, loaded once and shared by every character.
#[derive(Resource, Default)]
pub(super) struct CarriedLoadAssets {
    pub(super) scenes: HashMap<CarriedAppearance, Handle<WorldAsset>>,
}

/// Authored hand-tool scenes, loaded once and shared by every character.
#[derive(Resource, Default)]
pub(super) struct ToolAssets {
    pub(super) scenes: HashMap<ToolKind, Handle<WorldAsset>>,
}

/// The authored resource scene parented to the rig's `attach.carry` joint.
#[derive(Component)]
pub(super) struct CarriedLoadVisual(pub(super) CarriedAppearance);

/// An authored rig node that accepts the current carried-goods visual.
#[derive(Component)]
pub(super) struct CarryAttachment;

/// Direct link from an authored attachment joint to its replicated character
/// root. Resolving this once avoids walking the glTF hierarchy for every tool
/// and carried-load check on every frame.
#[derive(Component)]
pub(super) struct CharacterAttachmentOwner(pub(super) Entity);

/// Exact authored head joint and the character root that owns it.
///
/// Camera presentation must follow the dressed/animated rig rather than guess
/// a face height from the replicated character root. The latter is especially
/// visible in seated poses, where a standing-height estimate can put a close
/// camera inside hair, clothing, or the boat.
#[derive(Component, Debug, Clone, Copy)]
pub(crate) struct CharacterHead {
    pub(crate) owner: Entity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum ToolKind {
    Axe,
    Sword,
    Hammer,
    Scythe,
}

impl ToolKind {
    pub(super) const fn label(self) -> &'static str {
        match self {
            Self::Axe => "Felling axe",
            Self::Sword => "Soldier sidearm",
            Self::Hammer => "Framing hammer",
            Self::Scythe => "Mowing scythe",
        }
    }

    pub(super) const fn scene_path(self) -> &'static str {
        match self {
            Self::Axe => "game_assets/tools/AxeFelling.glb#Scene0",
            Self::Sword => "game_assets/tools/SoldierSidearm.glb#Scene0",
            Self::Hammer => "game_assets/tools/HammerFraming.glb#Scene0",
            Self::Scythe => "game_assets/tools/ScytheMowing.glb#Scene0",
        }
    }
}

/// The authored tool scene parented to the rig's `attach.tool.R` joint.
#[derive(Component)]
pub(super) struct ToolVisual(pub(super) ToolKind);

/// The authored right-hand joint that accepts the active work tool.
#[derive(Component)]
pub(super) struct ToolAttachment;

/// Parent the matching authored resource bundle to the rig's carry attachment.
pub(super) fn tag_carry_attachments(
    mut commands: Commands,
    named: Query<(Entity, &Name), (Added<Name>, Without<CarryAttachment>)>,
    parents: Query<&ChildOf>,
    characters: Query<(), With<CharacterKind>>,
) {
    for (entity, name) in named.iter() {
        if name.as_str() == "attach.carry" {
            let mut ancestor = entity;
            while let Ok(parent) = parents.get(ancestor) {
                ancestor = parent.parent();
                if characters.get(ancestor).is_ok() {
                    commands
                        .entity(entity)
                        .insert((CarryAttachment, CharacterAttachmentOwner(ancestor)));
                    break;
                }
            }
        }
    }
}

/// Tag the authored right-hand tool joint once its glTF node is instantiated.
pub(super) fn tag_tool_attachments(
    mut commands: Commands,
    named: Query<(Entity, &Name), (Added<Name>, Without<ToolAttachment>)>,
    parents: Query<&ChildOf>,
    characters: Query<(), With<CharacterKind>>,
) {
    for (entity, name) in named.iter() {
        if name.as_str() == "attach.tool.R" {
            let mut ancestor = entity;
            while let Ok(parent) = parents.get(ancestor) {
                ancestor = parent.parent();
                if characters.get(ancestor).is_ok() {
                    commands
                        .entity(entity)
                        .insert((ToolAttachment, CharacterAttachmentOwner(ancestor)));
                    break;
                }
            }
        }
    }
}

/// Resolve the canonical `head` bone after the asynchronous glTF hierarchy is
/// instantiated. Like tools and carried loads, this pays the ancestry walk
/// once and leaves a direct owner link for cheap per-frame camera queries.
pub(super) fn tag_character_heads(
    mut commands: Commands,
    named: Query<(Entity, &Name), (Added<Name>, Without<CharacterHead>)>,
    parents: Query<&ChildOf>,
    characters: Query<(), With<CharacterKind>>,
) {
    for (entity, name) in named.iter() {
        if name.as_str() != "head" {
            continue;
        }
        let mut ancestor = entity;
        while let Ok(parent) = parents.get(ancestor) {
            ancestor = parent.parent();
            if characters.get(ancestor).is_ok() {
                commands
                    .entity(entity)
                    .insert(CharacterHead { owner: ancestor });
                break;
            }
        }
    }
}

pub(super) fn sync_carried_load_visuals(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut assets: ResMut<CarriedLoadAssets>,
    attachments: Query<(Entity, Ref<CarryAttachment>, &CharacterAttachmentOwner)>,
    children: Query<&Children>,
    loads: Query<(Ref<CarriedLoad>, Option<Ref<PorterCartState>>), With<CharacterKind>>,
    mut removed_carts: RemovedComponents<PorterCartState>,
    existing_visuals: Query<&CarriedLoadVisual>,
) {
    let removed_carts: HashSet<_> = removed_carts.read().collect();
    for (attachment, marker, owner) in attachments.iter() {
        let Ok((load, cart)) = loads.get(owner.0) else {
            continue;
        };
        if !marker.is_added()
            && !load.is_changed()
            && !cart.as_ref().is_some_and(|cart| cart.is_changed())
            && !removed_carts.contains(&owner.0)
        {
            continue;
        }
        // A cart load belongs on its authored bed anchors, never duplicated in
        // the porter's arms. Removing the cart re-evaluates this attachment so
        // an abnormal in-flight personal load remains visible.
        let desired = cart.is_none().then(|| load.visible_appearance()).flatten();
        let existing = children.get(attachment).ok().and_then(|children| {
            children.iter().find_map(|child| {
                existing_visuals
                    .get(child)
                    .ok()
                    .map(|visual| (child, visual.0))
            })
        });

        if existing.is_some_and(|(_, appearance)| Some(appearance) == desired) {
            continue;
        }
        if let Some((entity, _)) = existing {
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

        commands.entity(attachment).with_children(|bone| {
            bone.spawn((
                Name::new(format!("Carried {}", appearance.label())),
                CarriedLoadVisual(appearance),
                WorldAssetRoot(scene),
                // Every authored bundle has its origin on its base, and the
                // joint marks that same base. No height correction belongs here.
                carried_bundle_transform(),
            ));
        });
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct CarriedAssetSpec {
    pub(super) scene_path: &'static str,
}

/// The authored measurements were intentionally conservative. In the actual
/// isometric game view they read as parcels rather than work loads, so carried
/// presentation is enlarged without changing inventory bulk or the source GLBs.
pub(super) const CARRIED_BUNDLE_SCALE: f32 = 1.35;

/// Bevy/character forward is -Z. Pull the load slightly away from the torso so
/// its centre sits in the cupped hands instead of clipping into the chest.
pub(super) const CARRIED_BUNDLE_FORWARD_OFFSET: f32 = -0.08;

pub(super) fn carried_bundle_transform() -> Transform {
    Transform::from_xyz(0.0, 0.0, CARRIED_BUNDLE_FORWARD_OFFSET)
        .with_scale(Vec3::splat(CARRIED_BUNDLE_SCALE))
}

pub(super) fn carried_asset_spec(appearance: CarriedAppearance) -> CarriedAssetSpec {
    match appearance {
        CarriedAppearance::WoodBundle => CarriedAssetSpec {
            scene_path: "game_assets/resources/carried/WoodBundle.glb#Scene0",
        },
        CarriedAppearance::WheatSheaf => CarriedAssetSpec {
            scene_path: "game_assets/resources/carried/WheatSheaf.glb#Scene0",
        },
        CarriedAppearance::FishBasket => CarriedAssetSpec {
            scene_path: "game_assets/resources/carried/FishBasket.glb#Scene0",
        },
        CarriedAppearance::StoneBundle => CarriedAssetSpec {
            scene_path: "game_assets/resources/carried/StoneBundle.glb#Scene0",
        },
        CarriedAppearance::IronBundle => CarriedAssetSpec {
            scene_path: "game_assets/resources/carried/IronBundle.glb#Scene0",
        },
        CarriedAppearance::FlourSack => CarriedAssetSpec {
            scene_path: "game_assets/resources/carried/FlourSack.glb#Scene0",
        },
        CarriedAppearance::BreadBasket => CarriedAssetSpec {
            scene_path: "game_assets/resources/carried/BreadBasket.glb#Scene0",
        },
        CarriedAppearance::WoolFleece => CarriedAssetSpec {
            scene_path: "game_assets/resources/carried/WoolFleece.glb#Scene0",
        },
        CarriedAppearance::MeatHaunch => CarriedAssetSpec {
            scene_path: "game_assets/resources/carried/MeatHaunch.glb#Scene0",
        },
    }
}

pub(super) fn desired_tool(
    activity: Option<CharacterActivity>,
    carrying: bool,
) -> Option<ToolKind> {
    if carrying {
        return None;
    }
    match activity {
        Some(CharacterActivity::Chopping) => Some(ToolKind::Axe),
        Some(CharacterActivity::Fighting) => Some(ToolKind::Sword),
        Some(CharacterActivity::Farming) => Some(ToolKind::Scythe),
        Some(CharacterActivity::Building | CharacterActivity::Mining) => Some(ToolKind::Hammer),
        _ => None,
    }
}

/// Attach only the tool required by the character's current visible work.
///
/// A physical load always wins: a villager carrying wood cannot also hold a
/// tool. `CharacterActivity` is authoritative, avoiding an N characters × M
/// construction-sites proximity join on the client.
pub(super) fn sync_tool_visuals(
    mut commands: Commands,
    asset_server: Res<AssetServer>,
    mut assets: ResMut<ToolAssets>,
    attachments: Query<(Entity, Ref<ToolAttachment>, &CharacterAttachmentOwner)>,
    children: Query<&Children>,
    characters: Query<
        (
            Option<Ref<CharacterActivity>>,
            Option<Ref<CarriedLoad>>,
            Option<Ref<PorterCartState>>,
        ),
        With<CharacterKind>,
    >,
    mut removed_carts: RemovedComponents<PorterCartState>,
    readiness: Query<Ref<shared::components::CombatReady>>,
    mut removed_ready: RemovedComponents<shared::components::CombatReady>,
    existing_visuals: Query<&ToolVisual>,
) {
    let removed_carts: HashSet<_> = removed_carts.read().collect();
    let removed_ready: HashSet<_> = removed_ready.read().collect();
    for (attachment, marker, owner) in attachments.iter() {
        let ready = readiness.get(owner.0).ok();
        let Ok((activity, carried, cart)) = characters.get(owner.0) else {
            continue;
        };
        if !marker.is_added()
            && !ready.as_ref().is_some_and(|r| r.is_added())
            && !removed_ready.contains(&owner.0)
            && !activity
                .as_ref()
                .is_some_and(|activity| activity.is_changed())
            && !carried.as_ref().is_some_and(|carried| carried.is_changed())
            && !cart.as_ref().is_some_and(|cart| cart.is_changed())
            && !removed_carts.contains(&owner.0)
        {
            continue;
        }
        let desired = desired_tool(
            if ready.is_some() {
                Some(CharacterActivity::Fighting)
            } else {
                activity.as_deref().copied()
            },
            cart.is_some() || carried.is_some_and(|load| !load.is_empty()),
        );
        let existing = children.get(attachment).ok().and_then(|children| {
            children.iter().find_map(|child| {
                existing_visuals
                    .get(child)
                    .ok()
                    .map(|visual| (child, visual.0))
            })
        });

        if existing.is_some_and(|(_, tool)| Some(tool) == desired) {
            continue;
        }
        if let Some((entity, _)) = existing {
            commands.entity(entity).despawn();
        }
        let Some(tool) = desired else {
            continue;
        };

        let scene = assets
            .scenes
            .entry(tool)
            .or_insert_with(|| asset_server.load(tool.scene_path()))
            .clone();
        commands.entity(attachment).with_children(|joint| {
            joint.spawn((
                Name::new(tool.label()),
                ToolVisual(tool),
                WorldAssetRoot(scene),
                // Tools are authored grip-at-origin in the attachment joint's
                // basis. Any correction here would conceal an asset contract bug.
                Transform::IDENTITY,
            ));
        });
    }
}
