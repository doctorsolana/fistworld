//! Observed appearance memory, clipped widget demand and the single worker.

use super::{
    cache::{Key, PortraitCache},
    raster,
    source::{self, PreparedSource, SourceAssets, SourceChanges},
    widgets::{OutfitPortrait, PersonPortrait, PortraitStatus},
};
use bevy::{
    gltf::Gltf,
    prelude::*,
    tasks::{block_on, poll_once, AsyncComputeTaskPool},
    ui::{CalculatedClip, UiGlobalTransform},
    window::PrimaryWindow,
};
use shared::components::{HeroOutfit, PersonId};
use std::sync::Arc;

/// Bounded-work diagnostics used by semantic captures, rather than frame sleeps.
#[derive(Resource, Default, Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PortraitMetrics {
    pub visible: usize,
    pub ready: usize,
    pub queued: usize,
    pub pending: bool,
    pub bytes: usize,
    pub completed: u64,
    pub discarded: u64,
    pub source_preparations: u64,
}

#[derive(Resource, Default)]
pub(super) struct SourceCache {
    gltf: Option<Handle<Gltf>>,
    background: Option<Handle<Image>>,
    fallback: Option<Handle<Image>>,
    prepared: Option<Arc<PreparedSource>>,
    preparations: u64,
}

pub(super) fn remember_existing(
    people: Query<(&PersonId, &HeroOutfit)>,
    mut cache: ResMut<PortraitCache>,
) {
    cache.known.extend(
        people
            .iter()
            .filter(|(id, _)| id.is_assigned())
            .map(|(id, outfit)| (*id, *outfit)),
    );
}

pub(super) fn remember_changed(
    people: Query<(&PersonId, &HeroOutfit), Or<(Changed<PersonId>, Changed<HeroOutfit>)>>,
    mut cache: ResMut<PortraitCache>,
) {
    // Either component may arrive first. Their Changed filters include Added,
    // so delayed appearance and a replaced replicated body both update memory.
    for (id, outfit) in &people {
        if id.is_assigned() {
            cache.known.insert(*id, *outfit);
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct Demand {
    entity: Entity,
    key: Option<Key>,
    visible: bool,
}

type WidgetQuery<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        Option<&'static PersonPortrait>,
        Option<&'static OutfitPortrait>,
        &'static Node,
        &'static ComputedNode,
        &'static UiGlobalTransform,
        &'static InheritedVisibility,
        Option<&'static CalculatedClip>,
    ),
    Or<(With<PersonPortrait>, With<OutfitPortrait>)>,
>;

#[allow(clippy::too_many_arguments)]
pub(super) fn sync_widgets(
    widgets: WidgetQuery,
    hierarchy: Query<(&Node, Option<&ChildOf>)>,
    windows: Query<&Window, With<PrimaryWindow>>,
    source: SourceAssets,
    mut source_cache: ResMut<SourceCache>,
    mut changes: SourceChanges,
    mut cache: ResMut<PortraitCache>,
    mut images: ResMut<Assets<Image>>,
    mut output: Query<(&mut ImageNode, &mut PortraitStatus)>,
    mut metrics: ResMut<PortraitMetrics>,
    mut demands: Local<Vec<Demand>>,
    perf: Option<Res<crate::ui::perf::UiPerf>>,
    perf_config: Option<Res<crate::perf_overlay::ClientPerfConfig>>,
) {
    let perf = perf
        .as_ref()
        .filter(|_| perf_config.as_ref().is_some_and(|config| config.enabled));
    let _timing = perf.map(|perf| perf.scope("sync_portraits"));
    if changes.affects(
        source_cache.prepared.as_deref(),
        source_cache.gltf.as_ref(),
        source_cache.background.as_ref(),
    ) {
        source_cache.prepared = None;
        cache.invalidate(&mut images);
    }
    cache.tick = cache.tick.wrapping_add(1);
    cache.wanted.clear();
    demands.clear();
    let viewport = windows
        .single()
        .ok()
        .map(|w| Vec2::new(w.physical_width() as f32, w.physical_height() as f32));
    for (entity, person, outfit, node, computed, transform, inherited, clip) in &widgets {
        let appearance = person
            .and_then(|p| cache.known.get(&p.0).copied())
            .or_else(|| outfit.and_then(|o| o.0));
        // Detail images get a 384px source; the ordinary 64–128px medallions
        // share 192px output, independent of whichever page requested it first.
        let logical_size = computed.size().max_element() * computed.inverse_scale_factor();
        let size = if logical_size > 128.0 { 384 } else { 192 };
        let key = appearance.map(|outfit| Key::new(outfit, size, cache.epoch));
        let mut visible =
            node.display != Display::None && inherited.get() && computed.size().min_element() > 0.0;
        let mut parent = hierarchy
            .get(entity)
            .ok()
            .and_then(|(_, parent)| parent.map(ChildOf::parent));
        while visible {
            let Some(entity) = parent else { break };
            let Ok((node, child_of)) = hierarchy.get(entity) else {
                break;
            };
            visible &= node.display != Display::None;
            parent = child_of.map(ChildOf::parent);
        }
        if visible {
            let half = computed.size() * 0.5;
            let corners = [
                -half,
                Vec2::new(half.x, -half.y),
                half,
                Vec2::new(-half.x, half.y),
            ];
            let mut bounds = Rect {
                min: Vec2::splat(f32::INFINITY),
                max: Vec2::splat(f32::NEG_INFINITY),
            };
            for corner in corners {
                let point = transform.transform_point2(corner);
                bounds.min = bounds.min.min(point);
                bounds.max = bounds.max.max(point);
            }
            if let Some(clip) = clip {
                bounds = bounds.intersect(clip.clip);
            }
            if let Some(size) = viewport {
                bounds = bounds.intersect(Rect::from_corners(Vec2::ZERO, size));
            }
            visible = bounds.width() > 0.0 && bounds.height() > 0.0;
        }
        if visible {
            if let Some(key) = key {
                cache.wanted.insert(key);
            }
        }
        demands.push(Demand {
            entity,
            key,
            visible,
        });
    }
    let completed = cache
        .pending
        .as_mut()
        .and_then(|(key, task)| block_on(poll_once(task)).map(|pixels| (*key, pixels)));
    if let Some((key, pixels)) = completed {
        cache.pending = None;
        let _upload = perf.map(|perf| perf.scope("upload_portrait"));
        cache.finish(key, pixels, &mut images);
    }
    let fallback = source_cache
        .fallback
        .get_or_insert_with(|| source.server.load("ui/hud/crest.png"))
        .clone();
    let mut visible = 0;
    let mut ready_count = 0;
    for demand in demands.iter() {
        let cached = demand.key.and_then(|key| {
            if demand.visible {
                cache.cached(key)
            } else {
                cache.peek(key)
            }
        });
        let Ok((mut image, mut status)) = output.get_mut(demand.entity) else {
            continue;
        };
        let ready = cached.is_some();
        let desired = PortraitStatus {
            known: demand.key.is_some(),
            ready,
        };
        if *status != desired {
            *status = desired
        }
        // An explicit empty HUD selection retains its existing decorative
        // frame/crest. Unknown durable people show the neutral heraldic fallback.
        let wanted = cached.unwrap_or_else(|| fallback.clone());
        let color = if demand.key.is_some()
            || widgets
                .get(demand.entity)
                .is_ok_and(|(_, p, ..)| p.is_some())
        {
            Color::WHITE
        } else {
            Color::NONE
        };
        if image.image != wanted {
            image.image = wanted
        }
        if image.color != color {
            image.color = color
        }
        if demand.visible {
            visible += 1;
            ready_count += usize::from(ready);
        }
    }
    // Largest visible portrait first. Repeated people/outfits on several pages
    // collapse into one key, and scrolling away discards any stale result.
    let next = demands
        .iter()
        .filter(|d| d.visible)
        .filter_map(|d| d.key)
        .filter(|key| !cache.has(*key))
        .max_by_key(|key| key.size);
    let queued = cache.wanted.iter().filter(|key| !cache.has(**key)).count();
    if cache.pending.is_none() {
        if let Some(key) = next {
            if cache.make_room(key.bytes(), &mut images) {
                let handle = source_cache
                    .gltf
                    .get_or_insert_with(|| {
                        source.server.load(
                            source
                                .manifest
                                .scene
                                .split('#')
                                .next()
                                .unwrap_or(&source.manifest.scene)
                                .to_owned(),
                        )
                    })
                    .clone();
                let background = source_cache
                    .background
                    .get_or_insert_with(|| source.server.load("ui/ledger/portrait-background.png"))
                    .clone();
                if source_cache.prepared.is_none() {
                    let _preparation = perf.map(|perf| perf.scope("prepare_portrait_source"));
                    source_cache.prepared = source::prepare(&handle, &background, &source, &images);
                    if source_cache.prepared.is_some() {
                        source_cache.preparations += 1;
                    }
                }
                if let Some(prepared) = &source_cache.prepared {
                    let prepared = prepared.clone();
                    cache.pending = Some((
                        key,
                        AsyncComputeTaskPool::get().spawn(async move {
                            raster::render(
                                prepared.dress(key.outfit()),
                                Some(prepared.background.clone()),
                                key.size,
                            )
                        }),
                    ));
                }
            }
        }
    }
    let next_metrics = PortraitMetrics {
        visible,
        ready: ready_count,
        queued,
        pending: cache.pending.is_some(),
        bytes: cache.bytes,
        completed: cache.completed,
        discarded: cache.discarded,
        source_preparations: source_cache.preparations,
    };
    if *metrics != next_metrics {
        *metrics = next_metrics;
    }
}

pub(super) fn clear_world(
    mut cache: ResMut<PortraitCache>,
    mut images: ResMut<Assets<Image>>,
    mut metrics: ResMut<PortraitMetrics>,
    mut statuses: Query<&mut PortraitStatus>,
) {
    cache.clear_world(&mut images);
    *metrics = PortraitMetrics::default();
    for mut status in &mut statuses {
        *status = PortraitStatus::default();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delayed_outfit_is_remembered_after_body_leaves_and_tracks_changes() {
        let mut app = App::new();
        app.init_resource::<PortraitCache>()
            .add_systems(Update, remember_changed);
        let entity = app.world_mut().spawn(PersonId(42)).id();
        app.update();
        assert!(app.world().resource::<PortraitCache>().known.is_empty());
        let original = HeroOutfit::default();
        app.world_mut().entity_mut(entity).insert(original);
        app.update();
        assert_eq!(
            app.world().resource::<PortraitCache>().known[&PersonId(42)],
            original
        );
        let changed = HeroOutfit {
            skin: 3,
            ..original
        };
        app.world_mut().entity_mut(entity).insert(changed);
        app.update();
        app.world_mut().despawn(entity);
        app.update();
        assert_eq!(
            app.world().resource::<PortraitCache>().known[&PersonId(42)],
            changed
        );
    }
    #[test]
    fn unassigned_identity_never_becomes_a_specific_persons_likeness() {
        let mut app = App::new();
        app.init_resource::<PortraitCache>()
            .add_systems(Update, remember_changed);
        let entity = app
            .world_mut()
            .spawn((PersonId::UNASSIGNED, HeroOutfit::default()))
            .id();
        app.update();
        assert!(app.world().resource::<PortraitCache>().known.is_empty());
        app.world_mut().entity_mut(entity).insert(PersonId(99));
        app.update();
        assert!(app
            .world()
            .resource::<PortraitCache>()
            .known
            .contains_key(&PersonId(99)));
    }
}
