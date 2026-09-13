//! Opt-in rehearsal of the startup creator. Only opening the modal is fixture
//! state; wardrobe, dismissal retention and confirmation use real handlers.
//! Offscreen UI receives semantic cursor targets and actual press/release edges.
use super::{CaptureConfig, CaptureState};
use crate::{
    hero::{control::SelectedOutfit, HeroDressed, HeroPreviewRig, HeroVisual},
    ui::hero_creator::HeroCreatorOpen,
};
use bevy::{
    input::InputSystems,
    prelude::*,
    ui::{RelativeCursorPosition, UiGlobalTransform, UiSystems},
};
use shared::components::HeroOutfit;

mod geometry;

#[derive(Resource, Default)]
pub(super) struct Rehearsal {
    shot: Option<usize>,
    phase: u8,
    frames: u32,
    expected: Option<HeroOutfit>,
    ready: bool,
    error: String,
    animation_start: Option<f32>,
    inspected: Option<usize>,
}

pub(super) fn install(app: &mut App) {
    let Ok(enabled) = std::env::var("FISTFORCE_CAPTURE_CREATOR_TOUR") else {
        return;
    };
    assert_eq!(enabled, "1", "FISTFORCE_CAPTURE_CREATOR_TOUR must be 1");
    app.init_resource::<Rehearsal>();
    app.add_systems(PreUpdate, input.after(InputSystems).after(UiSystems::Focus));
    app.add_systems(Last, inspect);
}

pub(super) fn ready(state: Option<Res<Rehearsal>>) -> bool {
    state.is_none_or(|state| state.ready)
}

fn named(world: &mut World, name: &str) -> Option<Entity> {
    world
        .query::<(Entity, &Name)>()
        .iter(world)
        .find(|(_, n)| n.as_str() == name)
        .map(|(e, _)| e)
}

fn is_descendant(world: &World, mut entity: Entity, root: Entity) -> bool {
    while let Some(parent) = world.get::<ChildOf>(entity) {
        entity = parent.parent();
        if entity == root {
            return true;
        }
    }
    false
}

fn rectangle(world: &World, entity: Entity) -> Option<Rect> {
    let node = world.get::<ComputedNode>(entity)?;
    let transform = world.get::<UiGlobalTransform>(entity)?;
    if node.size().min_element() <= 0.0 || !world.get::<InheritedVisibility>(entity)?.get() {
        return None;
    }
    let half = node.size() * 0.5;
    let mut min = Vec2::splat(f32::INFINITY);
    let mut max = Vec2::splat(f32::NEG_INFINITY);
    for corner in [
        -half,
        Vec2::new(half.x, -half.y),
        half,
        Vec2::new(-half.x, half.y),
    ] {
        let p = transform.transform_point2(corner);
        min = min.min(p);
        max = max.max(p);
    }
    Some(Rect { min, max })
}

fn inside(outer: Rect, inner: Rect) -> bool {
    inner.min.cmpge(outer.min - Vec2::ONE).all() && inner.max.cmple(outer.max + Vec2::ONE).all()
}

fn lighting_evidence(world: &mut World) -> Result<Vec<serde_json::Value>, String> {
    let mut lights = Vec::with_capacity(2);
    for name in ["creator-preview-key", "creator-preview-fill"] {
        let mut matches = world.query::<(
            &Name,
            &DirectionalLight,
            &GlobalTransform,
            Option<&Visibility>,
            Option<&InheritedVisibility>,
            Option<&bevy::camera::visibility::ViewVisibility>,
        )>();
        let entries: Vec<_> = matches
            .iter(world)
            .filter(|(entity_name, ..)| entity_name.as_str() == name)
            .collect();
        if entries.len() != 1 {
            return Err(format!(
                "expected one creator studio light {name}, got {}",
                entries.len()
            ));
        }
        for (_, light, pose, visibility, inherited, view) in entries {
            let hidden = visibility.is_some_and(|v| *v == Visibility::Hidden)
                || inherited.is_some_and(|v| !v.get());
            if !light.illuminance.is_finite() || light.illuminance < 0.0 {
                return Err(format!(
                    "creator studio light {name} has invalid illuminance"
                ));
            }
            if light.illuminance == 0.0 || hidden {
                return Err(format!(
                    "creator studio light {name} is not illuminating the open preview"
                ));
            }
            lights.push(serde_json::json!({
                "name":name,"kind":"directional","illuminance":light.illuminance,
                "rotation":pose.to_scale_rotation_translation().1.to_array(),
                "visibility":visibility.map(|v|format!("{v:?}")),
                "inherited_visibility":inherited.map(|v|v.get()),
                "view_visibility":view.map(|v|v.get()),
            }));
        }
    }
    Ok(lights)
}

fn input(world: &mut World) {
    let shot = match *world.resource::<CaptureState>() {
        CaptureState::Settling { shot, .. } => shot,
        CaptureState::Warmup { .. } => usize::MAX,
        _ => return,
    };
    let action = if shot == usize::MAX {
        "warmup".to_owned()
    } else {
        world.resource::<CaptureConfig>().shots[shot].name.clone()
    };
    if world.resource::<Rehearsal>().shot != Some(shot) {
        let outfit = world.resource::<SelectedOutfit>().0;
        let mut state = world.resource_mut::<Rehearsal>();
        state.shot = Some(shot);
        state.phase = 0;
        state.frames = 0;
        state.expected = Some(outfit);
        state.ready = false;
    }
    world
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    world
        .resource_mut::<ButtonInput<KeyCode>>()
        .release(KeyCode::Escape);
    for mut cursor in world.query::<&mut RelativeCursorPosition>().iter_mut(world) {
        cursor.cursor_over = false;
        cursor.normalized = None;
    }
    if let Some(backdrop) = named(world, "creator-backdrop") {
        if let Some(mut interaction) = world.get_mut::<Interaction>(backdrop) {
            interaction.set_if_neq(Interaction::None);
        }
    }
    let phase = world.resource::<Rehearsal>().phase;
    if phase >= 3 {
        return;
    }
    let suffix = action.split_once('-').map_or(action.as_str(), |(_, s)| s);
    let target = if suffix.starts_with("slot-") || suffix.starts_with("skin-") {
        Some(format!("creator-{suffix}"))
    } else {
        match suffix {
            "connecting" => Some("creator-BEGIN JOURNEY".into()),
            "backdrop-retained" => Some("creator-backdrop".into()),
            _ => None,
        }
    };
    if let Some(target) = target {
        let Some(entity) = named(world, &target).filter(|e| rectangle(world, *e).is_some()) else {
            return;
        };
        if target == "creator-backdrop" {
            // Exercise a real backdrop press and verify that the mandatory
            // startup creator remains open. Selectors hit-test the release.
            let Some(mut interaction) = world.get_mut::<Interaction>(entity) else {
                return;
            };
            interaction.set_if_neq(if phase == 1 {
                Interaction::Pressed
            } else {
                Interaction::None
            });
        } else {
            let Some(mut cursor) = world.get_mut::<RelativeCursorPosition>(entity) else {
                return;
            };
            cursor.cursor_over = true;
            cursor.normalized = Some(Vec2::splat(0.5));
        }
        if phase == 1 {
            world
                .resource_mut::<ButtonInput<MouseButton>>()
                .press(MouseButton::Left);
        }
        if phase == 2 && (suffix.starts_with("slot-") || suffix.starts_with("skin-")) {
            let manifest = &world.resource::<crate::hero::HeroManifest>().0;
            let mut expected = world.resource::<Rehearsal>().expected.unwrap();
            let step = if suffix.ends_with("next") { 1 } else { -1 };
            if suffix.starts_with("skin-") {
                expected.cycle_skin(step, manifest.skin.tones.len());
            } else {
                let index: usize = suffix.split('-').nth(1).unwrap().parse().unwrap();
                expected.cycle_slot(index, step, manifest.slots[index].items.len());
            }
            world.resource_mut::<Rehearsal>().expected = Some(expected);
        }
    } else if suffix == "escape-retained" {
        if phase == 1 {
            world
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::Escape);
        }
    } else {
        world.resource_mut::<Rehearsal>().phase = 3;
        return;
    }
    world.resource_mut::<Rehearsal>().phase += 1;
}

fn evidence(world: &mut World, action: &str) -> Result<serde_json::Value, String> {
    let art = world.resource::<crate::ui::hero_creator::CreatorArtwork>();
    if !art.ready(world.resource::<AssetServer>()) {
        return Err("creator material artwork is still loading".into());
    }
    let config = world.resource::<CaptureConfig>();
    let viewport = Rect::from_corners(
        Vec2::ZERO,
        Vec2::new(config.resolution[0] as f32, config.resolution[1] as f32),
    );
    let god_capability = world.resource::<crate::ui::hud::GodCapability>().0;
    let hud_mode = *world.resource::<crate::ui::hud::HudMode>();
    if god_capability || hud_mode != crate::ui::hud::HudMode::Play {
        return Err("startup creator tour requires an ordinary new player".into());
    }
    let expected = world.resource::<Rehearsal>().expected;
    let selected = world.resource::<SelectedOutfit>().0;
    if Some(selected) != expected {
        return Err("selector did not produce the expected outfit".into());
    }
    if !world.resource::<HeroCreatorOpen>().0 {
        return Err("startup creator closed before network confirmation".into());
    }
    let lights = lighting_evidence(world)?;
    let panel_entity = named(world, "creator-panel").ok_or("creator panel missing")?;
    let panel = rectangle(world, panel_entity).ok_or("creator panel has no visible layout")?;
    let pane = named(world, "creator-preview")
        .and_then(|e| rectangle(world, e))
        .ok_or("preview pane has no visible layout")?;
    if !inside(viewport, panel) || !inside(panel, pane) {
        return Err("creator panel or preview exceeds its viewport".into());
    }
    let artwork: Vec<_> = world
        .query::<(Entity, &ImageNode)>()
        .iter(world)
        .filter(|(entity, _)| is_descendant(world, *entity, panel_entity))
        .map(|(_, image)| image.image.id())
        .collect();
    let assets = world.resource::<AssetServer>();
    if artwork
        .iter()
        .any(|id| assets.get_path(*id).is_some() && !assets.is_loaded_with_dependencies(*id))
    {
        return Err("creator artwork dependencies are still loading".into());
    }
    let mut controls = vec!["creator-BEGIN JOURNEY".to_owned()];
    let manifest_slots = world.resource::<crate::hero::HeroManifest>().slots.len();
    for slot in 0..manifest_slots {
        for direction in ["previous", "next"] {
            controls.push(format!("creator-slot-{slot}-{direction}"));
        }
    }
    controls.extend(["creator-skin-previous".into(), "creator-skin-next".into()]);
    for name in &controls {
        let rect = named(world, name)
            .and_then(|e| rectangle(world, e))
            .ok_or_else(|| format!("control not laid out: {name}"))?;
        if !inside(panel, rect) {
            return Err(format!("control extends outside panel: {name}"));
        }
    }
    if action.ends_with("connecting") {
        let shown = world
            .query::<(Entity, &Text)>()
            .iter(world)
            .any(|(entity, text)| {
                text.0.contains("Still connecting")
                    && rectangle(world, entity).is_some_and(|rect| inside(viewport, rect))
            });
        if !shown {
            return Err("offline confirmation did not report connection pending".into());
        }
    }
    let (rig, pose, transform, outfit, speed) = world
        .query_filtered::<(
            Entity,
            &Transform,
            &GlobalTransform,
            &HeroOutfit,
            &HeroVisual,
        ), (With<HeroPreviewRig>, With<HeroDressed>)>()
        .iter(world)
        .next()
        .map(|(e, p, t, o, v)| (e, *p, *t, *o, v.speed()))
        .ok_or("preview wardrobe is not dressed")?;
    if outfit != selected {
        return Err("preview outfit differs from selected outfit".into());
    }
    let forward = (pose.rotation * Vec3::NEG_Z).dot(Vec3::Z);
    if speed.abs() > 0.001 || forward < 0.9999 {
        return Err(format!(
            "preview must idle facing forward: speed={speed}, facing={forward}"
        ));
    }
    let players: Vec<_> = world.query::<(Entity, &AnimationPlayer)>().iter(world)
        .filter(|(e, _)| is_descendant(world, *e, rig))
        .flat_map(|(_, p)| p.playing_animations().filter(|(_, a)| !a.is_paused() && a.weight() > 0.01)
            .map(|(node, a)| serde_json::json!({"node":node.index(),"elapsed":a.elapsed(),"seek_time":a.seek_time(),"weight":a.weight()})))
        .collect();
    let clock = players
        .iter()
        .filter_map(|p| p["elapsed"].as_f64())
        .fold(0.0_f64, f64::max) as f32;
    if players.is_empty() || clock <= 0.0 {
        return Err("preview has no advancing visible animation".into());
    }
    if action.ends_with("idle-later")
        && world
            .resource::<Rehearsal>()
            .animation_start
            .is_some_and(|start| clock <= start + 1.0)
    {
        return Err("preview animation did not advance between idle views".into());
    }
    let geometry = geometry::inspect(world, rig, viewport)?;
    let projected = geometry.bounds;
    let scene_viewport = geometry.scene_viewport;
    if !inside(pane, projected) {
        return Err(format!(
            "preview mesh bounds exceed pane: {projected:?} versus {pane:?}"
        ));
    }
    Ok(serde_json::json!({"open":true,"selected":selected,
        "god_capability":god_capability,"hud_mode":format!("{hud_mode:?}"),
        "panel_pixels":[panel.min.to_array(),panel.max.to_array()],"pane_pixels":[pane.min.to_array(),pane.max.to_array()],
        "preview_posed_bounds_pixels":[projected.min.to_array(),projected.max.to_array()],
        "scene_viewport":[scene_viewport.min.to_array(),scene_viewport.max.to_array()],
        "visible_meshes":geometry.meshes,"posed_vertices":geometry.vertices,"rig_world_position":transform.translation().to_array(),
        "preview_lights":lights,
        "local_rotation":pose.rotation.to_array(),"speed":speed,"forward_dot":forward,
        "animations":players,"visible_controls":controls}))
}

#[cfg(test)]
mod tests {
    use crate::capture_artifact::{CaptureScenario, CaptureTarget};

    #[test]
    fn startup_creator_tour_covers_the_live_manifest_and_uses_composed_output() {
        let manifest: shared::character::CharacterManifest =
            ron::from_str(include_str!("../../assets/characters/Humanoid.ron")).unwrap();
        let player: CaptureScenario = ron::from_str(include_str!(
            "../../../capture/scenarios/character-creator-new-player.ron"
        ))
        .unwrap();
        assert_eq!(
            player
                .environment
                .get("FISTFORCE_CAPTURE_CREATOR_TOUR")
                .map(String::as_str),
            Some("1")
        );
        assert!(!player.environment.contains_key("FISTFORCE_CAPTURE_HUD"));
        player.validate().unwrap();
        assert_eq!(player.target, CaptureTarget::Window);
        for slot in 0..manifest.slots.len() {
            for direction in ["next", "previous"] {
                let suffix = format!("slot-{slot}-{direction}");
                assert!(
                    player.shots.iter().any(|shot| shot.name.ends_with(&suffix)),
                    "the creator tour must exercise the live {} selector {direction}",
                    manifest.slots[slot].name
                );
            }
        }
        for action in [
            "idle-later",
            "skin-next",
            "skin-previous",
            "escape-retained",
            "backdrop-retained",
            "connecting",
        ] {
            assert!(player.shots.iter().any(|s| s.name.ends_with(action)));
        }
        assert!(player
            .shots
            .iter()
            .any(|s| s.name.ends_with("night") && s.time_of_day > 0.75));
        let portrait: CaptureScenario = ron::from_str(include_str!(
            "../../../capture/scenarios/character-creator.ron"
        ))
        .unwrap();
        portrait.validate().unwrap();
        assert_eq!(portrait.target, CaptureTarget::Window);
    }
}

fn inspect(world: &mut World) {
    let shot = match *world.resource::<CaptureState>() {
        CaptureState::Settling { shot, .. } | CaptureState::AwaitingCapture { shot, .. } => shot,
        CaptureState::Warmup { .. } => usize::MAX,
        _ => return,
    };
    if world.resource::<Rehearsal>().shot != Some(shot) {
        return;
    }
    let action = if shot == usize::MAX {
        "warmup".to_owned()
    } else {
        world.resource::<CaptureConfig>().shots[shot].name.clone()
    };
    let result = evidence(world, &action);
    let maximum = if shot == usize::MAX {
        1200
    } else {
        world.resource::<CaptureConfig>().shots[shot]
            .readiness
            .maximum_frames
    };
    {
        let mut state = world.resource_mut::<Rehearsal>();
        state.frames += 1;
        state.ready = state.phase >= 3 && result.is_ok();
        state.error = result.as_ref().err().cloned().unwrap_or_else(|| {
            if state.phase < 3 {
                format!("input gesture incomplete at phase {}", state.phase)
            } else {
                String::new()
            }
        });
        assert!(
            state.ready || state.frames < maximum,
            "creator capture {action} timed out: {}",
            state.error
        );
    }
    if !matches!(
        *world.resource::<CaptureState>(),
        CaptureState::AwaitingCapture { .. }
    ) || world.resource::<Rehearsal>().inspected == Some(shot)
    {
        return;
    }
    let mut record = result.expect("creator capture must be semantically ready");
    if action.ends_with("front") {
        let elapsed = record["animations"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|clip| clip["elapsed"].as_f64())
            .fold(0.0_f64, f64::max);
        world.resource_mut::<Rehearsal>().animation_start = Some(elapsed as f32);
    }
    record["shot"] = action.clone().into();
    record["passed"] = true.into();
    record["input"] =
        "semantic cursor targets and mouse press/release edges through production handlers".into();
    record["scope"] = "offline startup appearance and local UI; no network creation claimed; framing projects current skinned vertices plus PNG review".into();
    record["fixture"] = "creator opening; disconnected new-player appearance draft".into();
    let path = world
        .resource::<CaptureConfig>()
        .out_dir
        .join(format!("{action}.creator.json"));
    std::fs::write(path, serde_json::to_vec_pretty(&record).unwrap())
        .expect("write creator capture evidence");
    world.resource_mut::<Rehearsal>().inspected = Some(shot);
}
