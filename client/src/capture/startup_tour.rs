//! Real retained startup UI inspection. Fixture state is explicitly offline;
//! readiness measures actual image dependencies, layout and native loading pips.

use super::{CaptureConfig, CaptureState};
use bevy::{
    input::InputSystems,
    prelude::*,
    ui::{UiGlobalTransform, UiSystems},
    window::PrimaryWindow,
};

#[derive(Resource)]
pub(super) struct StartupReview {
    mode: String,
    frames: u32,
    ready: bool,
    error: String,
    inspected: Option<usize>,
    capture_shot: Option<usize>,
    action_shot: Option<usize>,
    input_phase: u8,
    animation_reported: bool,
    animation: Vec<serde_json::Value>,
    leaders: Vec<usize>,
    minimum: [f32; 3],
    maximum: [f32; 3],
    group_bounds: Option<Rect>,
}

pub(super) fn install(app: &mut App) {
    let Ok(mode) = std::env::var("FISTFORCE_CAPTURE_FRONTEND") else {
        return;
    };
    assert!(
        matches!(
            mode.as_str(),
            "menu" | "name" | "name-error" | "submitting" | "preparing" | "connecting"
        ),
        "unknown startup capture mode: {mode}"
    );
    if mode == "menu" {
        use crate::ui::main_menu::{ServerAddress, ServerEntry, ServerPresets};
        app.insert_resource(ServerAddress::default());
        app.insert_resource(ServerPresets {
            entries: vec![
                ServerEntry {
                    name: "Local server".into(),
                    ip: "127.0.0.1".into(),
                },
                ServerEntry {
                    name: "Development server".into(),
                    ip: "192.0.2.10".into(),
                },
            ],
            selected_index: Some(0),
        });
    }
    app.insert_resource(StartupReview {
        mode,
        frames: 0,
        ready: false,
        error: String::new(),
        inspected: None,
        capture_shot: None,
        action_shot: None,
        input_phase: 3,
        animation_reported: false,
        animation: Vec::new(),
        leaders: Vec::new(),
        minimum: [f32::INFINITY; 3],
        maximum: [f32::NEG_INFINITY; 3],
        group_bounds: None,
    });
    app.add_systems(PreUpdate, input.after(InputSystems).after(UiSystems::Focus));
    app.add_systems(Last, inspect);
}

fn input(world: &mut World) {
    let capture_shot = match *world.resource::<CaptureState>() {
        CaptureState::Settling { shot, .. } => Some(shot),
        _ => None,
    };
    let action_shot = match *world.resource::<CaptureState>() {
        CaptureState::Settling { shot, .. } | CaptureState::AwaitingCapture { shot, .. } => {
            Some(shot)
        }
        _ => None,
    };
    world.resource_mut::<StartupReview>().capture_shot = capture_shot;
    let Some(shot) = action_shot else {
        return;
    };
    let action = world.resource::<CaptureConfig>().shots[shot].name.clone();
    let interactive = action.ends_with("-hover") || action.contains("presets");
    if world.resource::<StartupReview>().action_shot != Some(shot) {
        let mut review = world.resource_mut::<StartupReview>();
        review.action_shot = Some(shot);
        review.input_phase = if interactive { 0 } else { 3 };
        if interactive {
            review.ready = false;
        }
    }
    world
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    let target = if action.ends_with("-hover") {
        Some("startup-primary")
    } else if action.contains("presets") {
        Some("startup-presets")
    } else {
        None
    };
    let phase = world.resource::<StartupReview>().input_phase;
    let target = target.and_then(|name| named(world, name).map(|(entity, _)| entity));
    if target.is_none() && (action.ends_with("-hover") || action.contains("presets")) {
        return;
    }
    let press = action.contains("presets") && phase == 1;
    for (entity, mut interaction) in world
        .query_filtered::<(Entity, &mut Interaction), With<Button>>()
        .iter_mut(world)
    {
        let expected = if Some(entity) == target {
            if press {
                Interaction::Pressed
            } else {
                Interaction::Hovered
            }
        } else {
            Interaction::None
        };
        interaction.set_if_neq(expected);
    }
    if press {
        world
            .resource_mut::<ButtonInput<MouseButton>>()
            .press(MouseButton::Left);
    }
    world.resource_mut::<StartupReview>().input_phase = (phase + 1).min(3);
}

pub(super) fn ready(review: Option<Res<StartupReview>>) -> bool {
    review.is_none_or(|review| review.ready)
}

fn visible(world: &World, mut entity: Entity) -> bool {
    loop {
        if world
            .get::<Node>(entity)
            .is_some_and(|node| node.display == Display::None)
            || world
                .get::<Visibility>(entity)
                .is_some_and(|v| *v == Visibility::Hidden)
            || world
                .get::<InheritedVisibility>(entity)
                .is_some_and(|v| !v.get())
        {
            return false;
        }
        let Some(parent) = world.get::<ChildOf>(entity) else {
            return true;
        };
        entity = parent.parent();
    }
}

fn descendant(world: &World, mut entity: Entity, ancestor: Entity) -> bool {
    while let Some(parent) = world.get::<ChildOf>(entity) {
        entity = parent.parent();
        if entity == ancestor {
            return true;
        }
    }
    false
}

fn rectangle(world: &World, entity: Entity) -> Option<Rect> {
    if !visible(world, entity) {
        return None;
    }
    let node = world.get::<ComputedNode>(entity)?;
    if node.size().min_element() <= 0.0 {
        return None;
    }
    let transform = world.get::<UiGlobalTransform>(entity)?;
    let half = node.size() * 0.5;
    let mut min = Vec2::splat(f32::INFINITY);
    let mut max = Vec2::splat(f32::NEG_INFINITY);
    for point in [
        -half,
        Vec2::new(half.x, -half.y),
        half,
        Vec2::new(-half.x, half.y),
    ] {
        let point = transform.transform_point2(point);
        min = min.min(point);
        max = max.max(point);
    }
    Some(Rect { min, max })
}

fn named(world: &mut World, name: &str) -> Option<(Entity, Rect)> {
    let entities: Vec<_> = world
        .query::<(Entity, &Name)>()
        .iter(world)
        .filter(|(_, n)| n.as_str() == name)
        .map(|(entity, _)| entity)
        .collect();
    entities
        .into_iter()
        .find_map(|e| rectangle(world, e).map(|r| (e, r)))
}

fn fits(outer: Rect, inner: Rect) -> bool {
    inner.min.cmpge(outer.min - Vec2::splat(1.5)).all()
        && inner.max.cmple(outer.max + Vec2::splat(1.5)).all()
}

fn bounds(rect: Rect) -> serde_json::Value {
    serde_json::json!([rect.min.to_array(), rect.max.to_array()])
}

fn evidence(world: &mut World) -> Result<serde_json::Value, String> {
    let mode = world.resource::<StartupReview>().mode.clone();
    let resolution = world.resource::<CaptureConfig>().resolution;
    let viewport = Rect::from_corners(
        Vec2::ZERO,
        Vec2::new(resolution[0] as f32, resolution[1] as f32),
    );
    let title = world
        .query_filtered::<&Window, With<PrimaryWindow>>()
        .single(world)
        .map_err(|_| "primary window is missing")?
        .title
        .clone();
    let artwork = world.resource::<crate::ui::startup::StartupArtwork>();
    if !artwork.ready(world.resource::<AssetServer>()) {
        return Err("startup artwork is still loading".into());
    }
    let (panel_entity, panel) =
        named(world, "startup-panel").ok_or("startup panel is not laid out")?;
    if world
        .get::<crate::ui::motion::UiReveal>(panel_entity)
        .is_some_and(|reveal| !reveal.is_settled())
    {
        return Err("startup panel arrival is still animating".into());
    }
    if !fits(viewport, panel) {
        return Err(format!("startup panel exceeds viewport: {panel:?}"));
    }
    let mut paper_join = None;
    if matches!(mode.as_str(), "name" | "name-error") {
        let backing = world
            .get::<ImageNode>(panel_entity)
            .ok_or("name panel lacks continuous backing")?;
        let backing_path = world
            .resource::<AssetServer>()
            .get_path(backing.image.id())
            .map(|path| path.to_string())
            .ok_or("name backing has no asset path")?;
        if backing_path != "ui/ledger/wood.jpg" || backing.color.to_srgba().alpha < 0.99 {
            return Err("name panel backing must be opaque wood behind the torn paper".into());
        }
        let (_, header) = named(world, "startup-name-header").ok_or("name header has no layout")?;
        let (_, paper) =
            named(world, "startup-name-paper").ok_or("name parchment has no layout")?;
        let overlap = header.max.y - paper.min.y;
        if overlap < 1.0 || !fits(panel, header) || !fits(panel, paper) {
            return Err(format!(
                "name header/parchment leaves an unsupported seam: overlap={overlap}"
            ));
        }
        paper_join = Some(
            serde_json::json!({"opaque_backing": backing_path, "header_pixels": bounds(header),
            "paper_pixels": bounds(paper), "overlap_pixels": overlap}),
        );
    }
    let (_, backdrop) =
        named(world, "startup-backdrop").ok_or("startup scenic backdrop is missing")?;
    if !fits(backdrop, viewport) {
        return Err("startup backdrop does not cover viewport".into());
    }
    let mut controls = Vec::new();
    let required: &[&str] = match mode.as_str() {
        "menu" => &[
            "startup-server-field",
            "startup-primary",
            "startup-secondary",
        ],
        "name" | "name-error" => &["startup-name-field", "startup-primary", "startup-secondary"],
        _ => &[],
    };
    for name in required {
        let (_, rect) = named(world, name).ok_or_else(|| format!("missing control: {name}"))?;
        if !fits(panel, rect) {
            return Err(format!("control outside panel: {name}"));
        }
        controls.push(serde_json::json!({"name": name, "pixels": bounds(rect)}));
    }
    let entities: Vec<_> = world
        .query::<Entity>()
        .iter(world)
        .filter(|e| rectangle(world, *e).is_some())
        .collect();
    let mut texts = Vec::new();
    let mut images = Vec::new();
    for entity in entities {
        if let Some(text) = world
            .get::<Text>(entity)
            .filter(|text| !text.0.trim().is_empty())
        {
            let rect = rectangle(world, entity).expect("visible node");
            if !fits(viewport, rect) {
                return Err(format!("text outside viewport: {}", text.0));
            }
            if let Some(font) = world.get::<TextFont>(entity) {
                let assets = world.resource::<AssetServer>();
                if let bevy::text::FontSource::Handle(handle) = &font.font {
                    if assets.get_path(handle.id()).is_some()
                        && !assets.is_loaded_with_dependencies(handle.id())
                    {
                        return Err("startup font still loading".into());
                    }
                }
            }
            texts.push(serde_json::json!({"text": text.0, "pixels": bounds(rect),
                "color": world.get::<TextColor>(entity).map(|color| color.0.to_srgba().to_f32_array())}));
        }
        if let Some(image) = world.get::<ImageNode>(entity) {
            let assets = world.resource::<AssetServer>();
            if let Some(path) = assets.get_path(image.image.id()) {
                if !assets.is_loaded_with_dependencies(image.image.id()) {
                    return Err(format!("startup image still loading: {path}"));
                }
                images.push(path.to_string());
            }
        }
    }
    if images.iter().any(|path| path == "ui/fistforce.png") {
        return Err("startup still uses the retired FistForce logo".into());
    }
    if images.is_empty() {
        return Err("startup artwork has not loaded".into());
    }
    let loading = matches!(mode.as_str(), "connecting" | "submitting" | "preparing");
    if loading
        && texts
            .iter()
            .any(|text| text["text"].as_str().is_some_and(|text| text.contains('%')))
    {
        return Err("indeterminate preparation must not display a fake percentage".into());
    }
    let action = world
        .resource::<StartupReview>()
        .action_shot
        .map(|shot| world.resource::<CaptureConfig>().shots[shot].name.clone());
    if world.resource::<StartupReview>().input_phase < 3 {
        return Err("startup semantic input gesture is incomplete".into());
    }
    let mut popup = None;
    let mut hover = None;
    if let Some(action) = action.as_deref() {
        if mode == "menu" {
            let expected = action.ends_with("-presets");
            let expanded = world
                .resource::<crate::ui::main_menu::DropdownState>()
                .expanded;
            let rect = named(world, "startup-preset-options").map(|(_, rect)| rect);
            if expanded != expected || rect.is_some() != expected {
                return Err(format!(
                    "preset popup did not follow production toggle action: {action}"
                ));
            }
            if let Some(rect) = rect {
                if !fits(viewport, rect) {
                    return Err("preset popup exceeds viewport".into());
                }
                let (popup_entity, _) =
                    named(world, "startup-preset-options").expect("popup has layout");
                let (primary, _) =
                    named(world, "startup-primary").ok_or("Connect button missing")?;
                let popup_order = world
                    .get::<bevy::ui::ComputedStackIndex>(popup_entity)
                    .ok_or("popup stack order missing")?
                    .0;
                let primary_order = world
                    .query::<(Entity, &bevy::ui::ComputedStackIndex)>()
                    .iter(world)
                    .filter(|(entity, _)| *entity == primary || descendant(world, *entity, primary))
                    .map(|(_, order)| order.0)
                    .max()
                    .ok_or("Connect stack order missing")?;
                if popup_order <= primary_order {
                    return Err(format!("preset popup draws behind Connect: popup={popup_order}, Connect={primary_order}"));
                }
                for index in 0..2 {
                    let (option, option_rect) = named(world, &format!("startup-preset-{index}"))
                        .ok_or("preset option is not visibly laid out")?;
                    if !fits(rect, option_rect) {
                        return Err("preset row exceeds its popup".into());
                    }
                    if world
                        .get::<bevy::ui::ComputedStackIndex>(option)
                        .is_none_or(|order| order.0 <= primary_order)
                    {
                        return Err("preset option draws behind Connect".into());
                    }
                }
                popup = Some(
                    serde_json::json!({"pixels": bounds(rect), "stack_index": popup_order, "connect_tree_max_stack_index": primary_order}),
                );
            }
        }
        if action.ends_with("-hover") {
            let (entity, _) = named(world, "startup-primary").ok_or("hover button is absent")?;
            if world.get::<Interaction>(entity) != Some(&Interaction::Hovered) {
                return Err("primary button did not receive semantic hover".into());
            }
            let label_y = world.query_filtered::<(&ChildOf, &UiTransform), With<crate::ui::foundation::UiButtonLabel>>()
                .iter(world).find_map(|(parent, transform)| {
                    (parent.parent() == entity).then_some(transform.translation.y)
                });
            let Some(Val::Px(label_y)) = label_y else {
                return Err("hover label spring is missing".into());
            };
            if (label_y + 1.25).abs() > 0.02 {
                return Err("hover label spring is still settling".into());
            }
            if world
                .get::<UiTransform>(entity)
                .is_some_and(|pose| pose.translation != bevy::ui::Val2::ZERO)
            {
                return Err("hover feedback moved the button hit area".into());
            }
            hover = Some(
                serde_json::json!({"interaction": "Hovered", "label_offset_y": label_y, "hit_area_translation": [0.0, 0.0]}),
            );
        }
    }
    let visible_diamonds = world
        .query_filtered::<Entity, With<crate::ui::startup::LoadingDiamond>>()
        .iter(world)
        .filter(|entity| rectangle(world, *entity).is_some())
        .count();
    if visible_diamonds != if loading { 3 } else { 0 } {
        return Err(format!(
            "expected {} visible loading diamonds; got {visible_diamonds}",
            if loading { 3 } else { 0 }
        ));
    }
    if mode == "name-error"
        && !texts.iter().any(|text| {
            text["text"]
                .as_str()
                .is_some_and(|text| text.contains("already online"))
        })
    {
        return Err("name rejection fixture is not visible".into());
    }
    if loading {
        let (status, _) = named(world, "startup-status").ok_or("loading status is missing")?;
        let status_text = world
            .get::<Text>(status)
            .ok_or("loading status must remain native text")?;
        let expected = match mode.as_str() {
            "preparing" => "preparing",
            "connecting" => "connecting",
            _ => "joining",
        };
        if !status_text.0.to_lowercase().contains(expected) {
            return Err(format!(
                "loading status does not match {mode}: {}",
                status_text.0
            ));
        }
        if let Some(color) = world.get::<TextColor>(status) {
            let color = color.0.to_srgba();
            if color.red > 0.4 && color.red > color.green * 1.7 && color.red > color.blue * 1.7 {
                return Err("loading status uses error-red styling".into());
            }
        }
        let (_, group) = named(world, "startup-diamonds").ok_or("loading diamond group missing")?;
        if !fits(panel, group) {
            return Err("loading diamonds exceed panel".into());
        }
        let mut pips = Vec::new();
        let mut brightness = [0.0; 3];
        for index in 0..3 {
            let name = format!("startup-diamond-{index}");
            let (entity, rect) =
                named(world, &name).ok_or_else(|| format!("missing loading diamond: {index}"))?;
            if !fits(panel, rect) {
                return Err(format!("loading diamond {index} exceeds panel"));
            }
            let color = world
                .get::<BackgroundColor>(entity)
                .ok_or("loading diamond has no fill")?
                .0
                .to_srgba();
            let transform = world
                .get::<UiTransform>(entity)
                .ok_or("loading diamond has no rotation")?;
            let angle = transform.rotation.as_radians();
            if ((angle.abs() % std::f32::consts::FRAC_PI_2) - std::f32::consts::FRAC_PI_4).abs()
                > 0.01
            {
                return Err(format!(
                    "loading pip {index} is not a diamond: angle={angle}"
                ));
            }
            brightness[index] =
                (color.red * 0.2126 + color.green * 0.7152 + color.blue * 0.0722) * color.alpha;
            pips.push(
                serde_json::json!({"index": index, "pixels": bounds(rect), "rotation": angle,
                "rgba": color.to_f32_array(), "brightness": brightness[index]}),
            );
        }
        let mut review = world.resource_mut::<StartupReview>();
        if let Some(previous) = review.group_bounds {
            if previous.min.distance(group.min) > 1.0 || previous.max.distance(group.max) > 1.0 {
                return Err("loading group layout shifts while animating".into());
            }
        }
        review.group_bounds = Some(group);
        let leader = (0..3)
            .max_by(|a, b| brightness[*a].total_cmp(&brightness[*b]))
            .unwrap();
        if review.leaders.last() != Some(&leader) {
            review.leaders.push(leader);
        }
        for index in 0..3 {
            review.minimum[index] = review.minimum[index].min(brightness[index]);
            review.maximum[index] = review.maximum[index].max(brightness[index]);
        }
        let sample =
            serde_json::json!({"frame": review.frames, "diamonds": pips, "leader": leader});
        review.animation.push(sample);
    }
    Ok(
        serde_json::json!({"mode": mode, "window_title": title, "window_title_scope": "capture harness diagnostic title; native game branding is verified separately", "viewport": resolution,
        "preset_popup_pixels": popup, "hover": hover, "paper_join": paper_join,
        "input": "offline named button targets; hover and press/release edges through production UI handlers",
        "game_state": format!("{:?}", world.resource::<State<crate::states::GameState>>().get()),
        "name_phase": format!("{:?}", world.resource::<crate::ui::name_entry::NameEntryPhase>()),
        "visible_diamond_count": visible_diamonds,
        "panel_pixels": bounds(panel), "backdrop_pixels": bounds(backdrop), "controls": controls,
        "visible_text": texts, "ready_image_paths": images, "loading": loading,
        "scenic_backdrop": world.resource::<AssetServer>().get_path(
            world.resource::<crate::ui::startup::StartupArtwork>().village.id()).map(|path| path.to_string()),
        "fixture": "offline startup presentation inputs; no connection, submission, or world-generation progress claimed; launcher presets are deterministic fixture entries",
        "passed": true}),
    )
}

fn inspect(world: &mut World) {
    let result = evidence(world);
    {
        let mut review = world.resource_mut::<StartupReview>();
        review.frames += 1;
        review.ready = result.is_ok();
        review.error = result.as_ref().err().cloned().unwrap_or_default();
        assert!(
            review.ready || review.frames < 1200,
            "startup view timed out: {}",
            review.error
        );
    }
    let shot = match *world.resource::<CaptureState>() {
        CaptureState::AwaitingCapture { shot, .. } => Some(shot),
        _ if world.resource::<CaptureConfig>().continuous => {
            world.resource::<StartupReview>().capture_shot
        }
        _ => None,
    };
    if let Some(shot) = shot {
        let config = world.resource::<CaptureConfig>();
        if world.resource::<StartupReview>().inspected != Some(shot)
            && (!config.continuous
                || shot % config.probe_every.max(1) as usize == 0
                || shot + 1 == config.shots.len())
        {
            if let Ok(mut record) = result {
                let name = &config.shots[shot].name;
                record["shot"] = name.clone().into();
                if let Some(sample) = world.resource::<StartupReview>().animation.last() {
                    record["animation_sample"] = sample.clone();
                }
                std::fs::write(
                    config.out_dir.join(format!("{name}.startup.json")),
                    serde_json::to_vec_pretty(&record).unwrap(),
                )
                .expect("write startup view evidence");
                world.resource_mut::<StartupReview>().inspected = Some(shot);
            }
        }
    }
    if matches!(
        *world.resource::<CaptureState>(),
        CaptureState::AwaitingAll { .. }
    ) && !world.resource::<StartupReview>().animation_reported
    {
        let review = world.resource::<StartupReview>();
        if review.animation.is_empty() {
            return;
        }
        for index in 0..3 {
            assert!(
                review.maximum[index] - review.minimum[index] > 0.1,
                "loading diamond {index} never visibly pulsed"
            );
            assert!(
                review.leaders.contains(&index),
                "loading diamond {index} never led the sequence"
            );
        }
        assert!(
            review
                .leaders
                .windows(2)
                .all(|pair| pair[1] == (pair[0] + 1) % 3),
            "loading pulse did not travel left-to-right: {:?}",
            review.leaders
        );
        let report = serde_json::json!({"scope": "continuous native UI animation; no generation progress or network timing measured",
            "passed": true, "leaders": review.leaders, "minimum_brightness": review.minimum,
            "maximum_brightness": review.maximum, "samples": review.animation});
        std::fs::write(
            world
                .resource::<CaptureConfig>()
                .out_dir
                .join("loading-animation.json"),
            serde_json::to_vec_pretty(&report).unwrap(),
        )
        .expect("write startup animation evidence");
        world.resource_mut::<StartupReview>().animation_reported = true;
    }
}
