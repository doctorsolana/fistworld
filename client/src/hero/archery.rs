//! Bow presentation for authoritative soldier equipment. This module owns no orders,
//! damage, ammunition or replication. Shot timestamps use the existing world clock.
use super::animation::HeroAnim;
use super::attachments::CharacterAttachmentOwner;
use bevy::animation::RepeatAnimation;
use bevy::gltf::Gltf;
use bevy::prelude::*;
use shared::components::{BowEquipped, BowShot, CharacterActivity, CharacterKind, WorldTime};
use shared::economy::{CarriedLoad, PorterCartState};

#[derive(Default)]
pub(super) struct ArcheryClips {
    pub ready: Option<AnimationNodeIndex>,
    pub shoot: Option<AnimationNodeIndex>,
}
impl ArcheryClips {
    pub fn sample(
        &self,
        now: f64,
        shot: Option<&BowShot>,
    ) -> Option<(AnimationNodeIndex, Option<f32>)> {
        if let Some(time) = shot.and_then(|s| s.sample(now)) {
            self.shoot.map(|clip| (clip, Some(time)))
        } else {
            self.ready.map(|clip| (clip, None))
        }
    }
}
#[derive(Resource, Default)]
pub(super) struct BowAssets {
    gltf: Option<Handle<Gltf>>,
    scene: Option<Handle<WorldAsset>>,
    graph: Option<(
        Handle<AnimationGraph>,
        AnimationNodeIndex,
        AnimationNodeIndex,
    )>,
}
#[derive(Component)]
pub(super) struct BowAttachment;
#[derive(Component)]
pub(crate) struct BowDressed;
#[derive(Component)]
pub(super) struct BowVisual {
    owner: Entity,
}
#[derive(Component)]
pub(super) struct BowAnimator {
    owner: Entity,
    ready: AnimationNodeIndex,
    shoot: AnimationNodeIndex,
}

pub(super) fn tag_bow_attachments(
    mut commands: Commands,
    named: Query<(Entity, &Name), Added<Name>>,
    parents: Query<&ChildOf>,
    characters: Query<(), With<CharacterKind>>,
) {
    for (entity, name) in &named {
        if name.as_str() != "attach.bow.L" {
            continue;
        }
        let mut ancestor = entity;
        while let Ok(parent) = parents.get(ancestor) {
            ancestor = parent.parent();
            if characters.contains(ancestor) {
                commands
                    .entity(entity)
                    .insert((BowAttachment, CharacterAttachmentOwner(ancestor)));
                break;
            }
        }
    }
}
pub(super) fn sync_bows(
    mut commands: Commands,
    server: Res<AssetServer>,
    mut assets: ResMut<BowAssets>,
    attachments: Query<(Entity, &CharacterAttachmentOwner), With<BowAttachment>>,
    equipped: Query<
        (
            Option<&CharacterActivity>,
            Option<&CarriedLoad>,
            Has<PorterCartState>,
        ),
        With<BowEquipped>,
    >,
    children: Query<&Children>,
    existing: Query<(), With<BowVisual>>,
) {
    for (attachment, owner) in &attachments {
        let want = equipped.get(owner.0).is_ok_and(|(activity, load, cart)| {
            !cart && !load.is_some_and(|l| !l.is_empty()) && available(activity.copied())
        });
        let current = children
            .get(attachment)
            .ok()
            .and_then(|kids| kids.iter().find(|e| existing.contains(*e)));
        if !want {
            if let Some(e) = current {
                commands.entity(e).despawn();
                commands.entity(owner.0).remove::<BowDressed>();
            }
            continue;
        }
        if current.is_some() {
            continue;
        }
        assets
            .gltf
            .get_or_insert_with(|| server.load("game_assets/tools/Bow.glb"));
        let scene = assets
            .scene
            .get_or_insert_with(|| server.load("game_assets/tools/Bow.glb#Scene0"))
            .clone();
        commands.entity(attachment).with_children(|joint| {
            joint.spawn((
                Name::new("Equipped bow"),
                BowVisual { owner: owner.0 },
                WorldAssetRoot(scene),
                Transform::IDENTITY,
            ));
        });
    }
}
pub(super) fn available(activity: Option<CharacterActivity>) -> bool {
    matches!(
        activity,
        None | Some(CharacterActivity::Idle | CharacterActivity::Fighting)
    )
}
pub(super) fn setup_bows(
    mut commands: Commands,
    mut assets: ResMut<BowAssets>,
    gltfs: Res<Assets<Gltf>>,
    mut graphs: ResMut<Assets<AnimationGraph>>,
    mut players: Query<(Entity, &mut AnimationPlayer), Without<AnimationGraphHandle>>,
    parents: Query<&ChildOf>,
    bows: Query<&BowVisual>,
) {
    let Some(gltf) = assets.gltf.as_ref().and_then(|h| gltfs.get(h)) else {
        return;
    };
    if assets.graph.is_none() {
        let (Some(ready), Some(shoot)) = (
            gltf.named_animations.get("bow_ready"),
            gltf.named_animations.get("bow_shoot"),
        ) else {
            return;
        };
        let mut graph = AnimationGraph::new();
        let ready = graph.add_clip(ready.clone(), 1., graph.root);
        let shoot = graph.add_clip(shoot.clone(), 1., graph.root);
        assets.graph = Some((graphs.add(graph), ready, shoot));
    }
    let (graph, ready, shoot) = assets.graph.as_ref().unwrap();
    for (entity, mut player) in &mut players {
        let mut ancestor = entity;
        while let Ok(parent) = parents.get(ancestor) {
            ancestor = parent.parent();
            if let Ok(bow) = bows.get(ancestor) {
                commands.entity(entity).insert((
                    AnimationGraphHandle(graph.clone()),
                    BowAnimator {
                        owner: bow.owner,
                        ready: *ready,
                        shoot: *shoot,
                    },
                ));
                commands.entity(bow.owner).insert(BowDressed);
                player.play(*ready).repeat();
                break;
            }
        }
    }
}
pub(super) fn drive_bows(
    clocks: Query<&WorldTime>,
    presentation: Option<Res<crate::animation_clock::AnimationClock>>,
    characters: Query<(&HeroAnim, Option<&BowShot>)>,
    mut bows: Query<(&BowAnimator, &mut AnimationPlayer)>,
) {
    let now = clocks.iter().next().map_or(0., |clock| {
        presentation.as_ref().map_or_else(
            || crate::animation_clock::seconds(clock),
            |presentation| presentation.sample(clock),
        )
    });
    for (bow, mut player) in &mut bows {
        let Ok((hero, shot)) = characters.get(bow.owner) else {
            continue;
        };
        let time = shot
            .and_then(|s| s.sample(now))
            .filter(|_| hero.current_body == hero.archery.shoot);
        let node = if time.is_some() { bow.shoot } else { bow.ready };
        if !player.is_playing_animation(node) {
            player.stop_all();
            player.start(node).set_repeat(RepeatAnimation::Never);
        }
        if let Some(active) = player.animation_mut(node) {
            // Both rigs use the exact same clock sample; release never drifts.
            active
                .pause()
                .set_seek_time(time.unwrap_or(0.))
                .set_weight(if hero.paused { 0. } else { 1. });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn shot_release_is_one_second_in_and_does_not_repeat() {
        let shot = BowShot { release_at: 10. };
        assert_eq!(shot.sample(8.9), None);
        assert_eq!(shot.sample(9.), Some(0.));
        assert_eq!(shot.sample(10.), Some(1.));
        assert_eq!(shot.sample(11.), None);
        assert_eq!(shot.sample(f64::NAN), None);
    }
    #[test]
    fn jobs_and_rest_do_not_display_a_second_weapon() {
        assert!(available(Some(CharacterActivity::Fighting)));
        assert!(!available(Some(CharacterActivity::Chopping)));
        assert!(!available(Some(CharacterActivity::LyingDown)));
    }
}

/// Read-only capture diagnostics: a server shot alone cannot prove the authored
/// bow/body clips are being displayed by the connected client.
#[derive(bevy::ecs::system::SystemParam)]
pub(crate) struct ArcheryInspection<'w, 's> {
    people: Query<
        'w,
        's,
        (
            &'static shared::components::PersonId,
            &'static HeroAnim,
            &'static super::HeroVisual,
            Option<&'static shared::components::CharacterMotion>,
            Option<&'static CharacterActivity>,
            Option<&'static BowShot>,
            Has<BowEquipped>,
            Has<BowDressed>,
        ),
        With<shared::components::SoldierRole>,
    >,
    players: Query<'w, 's, &'static AnimationPlayer>,
}
impl ArcheryInspection<'_, '_> {
    pub(crate) fn active_draws(&self) -> usize {
        self.people
            .iter()
            .filter(|(_, anim, _, _, _, _, bow, dressed)| {
                *bow && *dressed
                    && !anim.paused
                    && anim.archery.shoot.is_some()
                    && anim.current_body == anim.archery.shoot
                    && anim
                        .current_body
                        .and_then(|node| self.players.get(anim.player).ok()?.animation(node))
                        .is_some_and(|active| active.weight() > 0.8)
            })
            .count()
    }

    pub(crate) fn snapshot(&self) -> Vec<serde_json::Value> {
        self.people
            .iter()
            .filter(|(_, _, _, _, _, shot, bow, _)| *bow || shot.is_some())
            .map(|(id, anim, visual, motion, activity, shot, bow, dressed)| {
                let active = anim
                    .current_body
                    .and_then(|node| self.players.get(anim.player).ok()?.animation(node));
                serde_json::json!({
                    "person": id.0, "equipped": bow, "dressed": dressed,
                    "body": anim.current_body.map(|n| n.index()),
                    "shoot": anim.archery.shoot.map(|n| n.index()),
                    "ready": anim.archery.ready.map(|n| n.index()),
                    "paused": anim.paused, "speed": visual.speed,
                    "motion": motion.map(|m| m.velocity.to_array()),
                    "activity": activity, "release": shot.map(|s| s.release_at),
                    "seek": active.map(|a| a.seek_time()),
                    "weight": active.map(|a| a.weight()),
                })
            })
            .collect()
    }
}
