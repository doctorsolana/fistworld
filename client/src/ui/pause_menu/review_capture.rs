//! Opt-in production-input tour of the retained Escape and settings menus.
//! No native display changes or audio playback/mix claims are made here.

use super::*;
use crate::audio::AudioSettings;
use crate::capture::{CaptureConfig, CaptureState};
use bevy::{
    input::InputSystems,
    ui::{
        CalculatedClip, InteractionDisabled, RelativeCursorPosition, UiGlobalTransform, UiSystems,
    },
};

#[derive(Resource, Default)]
pub(crate) struct MenuReview {
    shot: Option<usize>,
    phase: u8,
    frames: u32,
    pressed: Option<Entity>,
    ready: bool,
    inspected: Option<usize>,
    initial: Option<(f32, f32, AudioSettings)>,
}

pub(super) fn install(app: &mut App) {
    let Ok(flag) = std::env::var("FISTWORLD_CAPTURE_PAUSE_TOUR") else {
        return;
    };
    assert_eq!(flag, "1", "FISTWORLD_CAPTURE_PAUSE_TOUR must be 1");
    assert!(
        std::env::var_os("FISTFORCE_NO_SETTINGS_FILE").is_some(),
        "menu review must isolate player settings"
    );
    app.init_resource::<MenuReview>();
    app.add_systems(
        PreUpdate,
        input
            .after(InputSystems)
            .after(UiSystems::Focus)
            .run_if(resource_exists::<CaptureConfig>),
    );
    app.add_systems(Last, inspect.run_if(resource_exists::<CaptureConfig>));
}

pub(crate) fn ready(review: Option<Res<MenuReview>>) -> bool {
    review.is_none_or(|review| review.ready)
}

fn shot(world: &World) -> Option<usize> {
    match *world.resource::<CaptureState>() {
        CaptureState::Warmup { .. } => Some(usize::MAX),
        CaptureState::Settling { shot, .. } | CaptureState::AwaitingCapture { shot, .. } => {
            Some(shot)
        }
        _ => None,
    }
}

fn action(world: &World, shot: usize) -> String {
    if shot == usize::MAX {
        "warmup".into()
    } else {
        world.resource::<CaptureConfig>().shots[shot]
            .name
            .split_once('-')
            .map_or("", |(_, suffix)| suffix)
            .into()
    }
}

fn descendant(world: &World, mut entity: Entity, root: Entity) -> bool {
    while let Some(parent) = world.get::<ChildOf>(entity) {
        entity = parent.parent();
        if entity == root {
            return true;
        }
    }
    false
}

fn rectangle(world: &World, entity: Entity) -> Option<Rect> {
    let computed = world.get::<ComputedNode>(entity)?;
    if computed.size().min_element() <= 0.0 {
        return None;
    }
    let mut ancestor = Some(entity);
    while let Some(current) = ancestor {
        if world
            .get::<Node>(current)
            .is_some_and(|n| n.display == Display::None)
            || world
                .get::<InheritedVisibility>(current)
                .is_some_and(|v| !v.get())
        {
            return None;
        }
        ancestor = world.get::<ChildOf>(current).map(ChildOf::parent);
    }
    let pose = world.get::<UiGlobalTransform>(entity)?;
    let half = computed.size() * 0.5;
    let mut min = Vec2::splat(f32::INFINITY);
    let mut max = Vec2::splat(f32::NEG_INFINITY);
    for corner in [
        -half,
        Vec2::new(half.x, -half.y),
        half,
        Vec2::new(-half.x, half.y),
    ] {
        let p = pose.transform_point2(corner);
        min = min.min(p);
        max = max.max(p);
    }
    Some(Rect { min, max })
}

fn inside(outer: Rect, inner: Rect) -> bool {
    inner.min.cmpge(outer.min - Vec2::ONE).all() && inner.max.cmple(outer.max + Vec2::ONE).all()
}

fn visible_named(world: &mut World, name: &str) -> Option<Entity> {
    world
        .query::<(Entity, &Name)>()
        .iter(world)
        .find(|(entity, actual)| actual.as_str() == name && rectangle(world, *entity).is_some())
        .map(|(entity, _)| entity)
}

fn target(world: &mut World, action: &str) -> Option<Entity> {
    let name = match action {
        "graphics" => "pause-GRAPHICS",
        "audio" => "pause-AUDIO",
        "master-lower" => "audio-master-decrease",
        "master-restored" => "audio-master-increase",
        "music-off" => "pause-music-off",
        "music-restored" => "pause-music-on",
        "controls" => "pause-CONTROLS",
        "back" => "pause-back",
        "resumed" => "pause-RESUME",
        "scale-lower" | "scale-restored" => {
            let direction = if action == "scale-lower" { -1 } else { 1 };
            return world
                .query::<(Entity, &SliderStep)>()
                .iter(world)
                .find(|(entity, step)| {
                    matches!(step.control, SliderControl::RenderScale)
                        && step.delta == direction
                        && rectangle(world, *entity).is_some()
                })
                .map(|(entity, _)| entity);
        }
        "sensitivity-higher" | "sensitivity-restored" => {
            let direction = if action == "sensitivity-higher" {
                1
            } else {
                -1
            };
            return world
                .query::<(Entity, &InputSliderStep)>()
                .iter(world)
                .find(|(entity, step)| {
                    matches!(step.control, InputSliderControl::MouseSensitivity)
                        && step.delta == direction
                        && rectangle(world, *entity).is_some()
                })
                .map(|(entity, _)| entity);
        }
        _ => return None,
    };
    visible_named(world, name)
}

fn input(world: &mut World) {
    let Some(shot) = shot(world) else { return };
    if !matches!(
        *world.resource::<CaptureState>(),
        CaptureState::Warmup { .. } | CaptureState::Settling { .. }
    ) {
        return;
    }
    let action = action(world, shot);
    if world.resource::<MenuReview>().shot != Some(shot) {
        let initial = (
            world.resource::<GraphicsSettings>().render_scale,
            world.resource::<InputSettings>().mouse_sensitivity,
            world.resource::<AudioSettings>().clone(),
        );
        let mut review = world.resource_mut::<MenuReview>();
        review.initial.get_or_insert(initial);
        review.shot = Some(shot);
        review.phase = 0;
        review.frames = 0;
        review.ready = false;
    }
    world
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    world
        .resource_mut::<ButtonInput<KeyCode>>()
        .release(KeyCode::Escape);
    if let Some(previous) = world.resource_mut::<MenuReview>().pressed.take() {
        if let Some(mut interaction) = world.get_mut::<Interaction>(previous) {
            interaction.set_if_neq(Interaction::None);
        }
        if let Some(mut cursor) = world.get_mut::<RelativeCursorPosition>(previous) {
            cursor.cursor_over = false;
            cursor.normalized = None;
        }
    }
    let phase = world.resource::<MenuReview>().phase;
    if phase >= 3 {
        return;
    }
    if matches!(action.as_str(), "warmup" | "escape") {
        world.resource_mut::<MenuReview>().phase = 3;
        return;
    }
    if action == "reopened" {
        if phase == 1 {
            world
                .resource_mut::<ButtonInput<KeyCode>>()
                .press(KeyCode::Escape);
        }
    } else if phase < 2 {
        let Some(entity) = target(world, &action) else {
            return;
        };
        if world.get::<InteractionDisabled>(entity).is_some() {
            return;
        }
        if let Some(mut interaction) = world.get_mut::<Interaction>(entity) {
            interaction.set_if_neq(if phase == 1 {
                Interaction::Pressed
            } else {
                Interaction::Hovered
            });
        } else {
            return;
        }
        if let Some(mut cursor) = world.get_mut::<RelativeCursorPosition>(entity) {
            cursor.cursor_over = true;
            cursor.normalized = Some(Vec2::ZERO);
        }
        world.resource_mut::<MenuReview>().pressed = Some(entity);
        if phase == 1 {
            world
                .resource_mut::<ButtonInput<MouseButton>>()
                .press(MouseButton::Left);
        }
    }
    world.resource_mut::<MenuReview>().phase += 1;
}

fn evidence(world: &mut World, action: &str) -> Result<serde_json::Value, String> {
    let state = world.resource::<PauseMenuState>();
    let pane = if !world.resource::<PauseMenuOpen>().0 {
        "world"
    } else if state.graphics_open {
        "graphics"
    } else if state.audio_open {
        "audio"
    } else if state.controls_open {
        "controls"
    } else {
        "escape"
    };
    let expected = match action {
        "graphics" | "scale-lower" | "scale-restored" => "graphics",
        "audio" | "master-lower" | "master-restored" | "music-off" | "music-restored" => "audio",
        "controls" | "sensitivity-higher" | "sensitivity-restored" => "controls",
        "resumed" => "world",
        _ => "escape",
    };
    if pane != expected {
        return Err(format!("expected {expected}, actual {pane}"));
    }
    let (scale, sensitivity, original_audio) =
        world.resource::<MenuReview>().initial.as_ref().unwrap();
    let graphics = world.resource::<GraphicsSettings>();
    let input = world.resource::<InputSettings>();
    let audio = world.resource::<AudioSettings>();
    let expected_scale = if action == "scale-lower" {
        graphics.render_scale < *scale - 0.001
    } else {
        (graphics.render_scale - scale).abs() < 0.001
    };
    let expected_sensitivity = if action == "sensitivity-higher" {
        input.mouse_sensitivity > *sensitivity + 0.001
    } else {
        (input.mouse_sensitivity - sensitivity).abs() < 0.001
    };
    let expected_master = if action == "master-lower" {
        audio.master_volume < original_audio.master_volume - 0.001
    } else {
        (audio.master_volume - original_audio.master_volume).abs() < 0.001
    };
    let expected_music = audio.music_enabled
        == if action == "music-off" {
            !original_audio.music_enabled
        } else {
            original_audio.music_enabled
        };
    if !(expected_scale && expected_sensitivity && expected_master && expected_music) {
        return Err(format!(
            "production action {action} has not produced/restored its settings"
        ));
    }
    let values = serde_json::json!({"render_scale":graphics.render_scale,
        "mouse_sensitivity":input.mouse_sensitivity,"audio":audio.clone()});
    let config = world.resource::<CaptureConfig>();
    let viewport = Rect::from_corners(
        Vec2::ZERO,
        Vec2::new(config.resolution[0] as f32, config.resolution[1] as f32),
    );
    let frame_name = if pane == "escape" {
        "pause-menu-frame"
    } else {
        "pause-settings-frame"
    };
    let roots = ["pause-menu-frame", "pause-settings-frame"]
        .into_iter()
        .filter_map(|name| visible_named(world, name))
        .collect::<Vec<_>>();
    if pane == "world" {
        if !roots.is_empty() {
            return Err("Resume left a visible menu frame".into());
        }
        return Ok(
            serde_json::json!({"active_pane":pane,"values":values,"visible_controls":[],"visible_text":[]}),
        );
    }
    if roots.len() != 1 {
        return Err(format!(
            "expected one visible menu frame, got {}",
            roots.len()
        ));
    }
    let frame = visible_named(world, frame_name).ok_or("active frame not visible")?;
    let bounds = rectangle(world, frame).unwrap();
    for (entity, reveal) in world
        .query::<(Entity, &crate::ui::motion::UiReveal)>()
        .iter(world)
    {
        if (entity == frame || descendant(world, entity, frame) || descendant(world, frame, entity))
            && rectangle(world, entity).is_some()
            && !reveal.is_settled()
        {
            return Err("menu reveal spring has not settled".into());
        }
    }
    if !inside(viewport, bounds) {
        return Err(format!("{frame_name} exceeds viewport: {bounds:?}"));
    }
    if pane != "escape" {
        let panel = visible_named(world, &format!("pause-{pane}-panel"))
            .ok_or("active settings content is not laid out")?;
        if !inside(bounds, rectangle(world, panel).unwrap()) {
            return Err("settings content exceeds its frame".into());
        }
    }
    let controls = world
        .query_filtered::<(Entity, Option<&Name>), With<Button>>()
        .iter(world)
        .filter_map(|(entity, name)| {
            (descendant(world, entity, frame))
                .then(|| {
                    rectangle(world, entity).map(|rect| {
                        (
                            entity,
                            name.map_or_else(|| format!("{entity:?}"), |n| n.as_str().to_owned()),
                            rect,
                        )
                    })
                })
                .flatten()
        })
        .collect::<Vec<_>>();
    if controls.is_empty() {
        return Err("menu has no visible buttons".into());
    }
    let mut control_records = Vec::new();
    for (entity, name, rect) in controls {
        if !inside(bounds, rect) {
            return Err(format!("control {name} exceeds menu frame"));
        }
        if world
            .get::<CalculatedClip>(entity)
            .is_some_and(|clip| !inside(clip.clip, rect))
        {
            return Err(format!("control {name} is clipped"));
        }
        control_records.push(serde_json::json!({"name":name,"bounds_pixels":[rect.min.to_array(),rect.max.to_array()],"enabled":world.get::<InteractionDisabled>(entity).is_none()}));
    }
    let mut text_records = Vec::new();
    for (entity, text) in world.query::<(Entity, &Text)>().iter(world) {
        if !descendant(world, entity, frame) {
            continue;
        }
        let Some(rect) = rectangle(world, entity) else {
            continue;
        };
        if !inside(bounds, rect) {
            return Err(format!("text {:?} exceeds menu frame", text.0));
        }
        if world
            .get::<CalculatedClip>(entity)
            .is_some_and(|clip| !inside(clip.clip, rect))
        {
            return Err(format!("text {:?} is clipped", text.0));
        }
        if pane == "controls"
            && ["find your hero", "close menu"]
                .iter()
                .any(|phrase| text.0.to_lowercase().contains(phrase))
        {
            return Err(format!("removed controls entry remains: {}", text.0));
        }
        text_records.push(serde_json::json!({"text":text.0,"bounds_pixels":[rect.min.to_array(),rect.max.to_array()]}));
    }
    let mut images = world.query::<(Entity, &ImageNode)>();
    let assets = world.resource::<AssetServer>();
    for (entity, image) in images.iter(world) {
        if descendant(world, entity, frame)
            && rectangle(world, entity).is_some()
            && assets.get_load_state(image.image.id()).is_some()
            && !assets.is_loaded_with_dependencies(image.image.id())
        {
            return Err("visible menu artwork is still loading".into());
        }
    }
    Ok(
        serde_json::json!({"active_pane":pane,"frame_pixels":[bounds.min.to_array(),bounds.max.to_array()],
        "values":values,"visible_controls":control_records,"visible_text":text_records,"no_clipping":true}),
    )
}

fn inspect(world: &mut World) {
    let Some(shot) = shot(world) else { return };
    if world.resource::<MenuReview>().shot != Some(shot) {
        return;
    }
    let action = action(world, shot);
    let result = evidence(world, &action);
    let maximum = if shot == usize::MAX {
        1200
    } else {
        world.resource::<CaptureConfig>().shots[shot]
            .readiness
            .maximum_frames
    };
    {
        let mut review = world.resource_mut::<MenuReview>();
        review.frames += 1;
        review.ready = review.phase >= 3 && result.is_ok();
        assert!(
            review.ready || review.frames < maximum,
            "menu capture {action} timed out at input phase {}: {:?}",
            review.phase,
            result.as_ref().err()
        );
    }
    if !matches!(
        *world.resource::<CaptureState>(),
        CaptureState::AwaitingCapture { .. }
    ) || world.resource::<MenuReview>().inspected == Some(shot)
    {
        return;
    }
    let mut record = result.expect("menu screenshot must be semantically ready");
    let config = world.resource::<CaptureConfig>();
    let name = &config.shots[shot].name;
    record["shot"] = name.clone().into();
    record["passed"] = true.into();
    record["input"] = "visible production button press/release and Escape keyboard edges".into();
    record["scope"] = "offline retained menu appearance and local settings; no native display, connected gameplay or audio mix acceptance".into();
    std::fs::write(
        config.out_dir.join(format!("{name}.menu.json")),
        serde_json::to_vec_pretty(&record).unwrap(),
    )
    .expect("write menu capture evidence");
    world.resource_mut::<MenuReview>().inspected = Some(shot);
}
