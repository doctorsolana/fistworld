//! Continuous rehearsal of the real retained encyclopedia and its action systems.
//!
//! The capture camera renders to an image, which Bevy's native window hit tester
//! deliberately ignores. Inject semantic Interaction + input edges after Focus;
//! never mutate tab/open state to manufacture a passing action result.
use super::{CaptureConfig, CaptureState};
use crate::ui::encyclopedia::{
    EncyclopediaOpen, EncyclopediaPanel, EncyclopediaRoot, EncyclopediaTab, TabBody, TabButton,
};
use bevy::{
    input::InputSystems,
    prelude::*,
    ui::{UiSystems, UiTransform},
};

#[derive(Resource, Default)]
struct TourFrame(Option<usize>);

pub(super) fn install(app: &mut App) {
    if std::env::var("FISTFORCE_CAPTURE_UI_TOUR").as_deref() != Ok("1") {
        return;
    }
    app.init_resource::<TourFrame>();
    app.add_systems(PreUpdate, input.after(InputSystems).after(UiSystems::Focus));
    app.add_systems(Last, inspect);
}

fn target(frame: usize) -> Option<(EncyclopediaTab, bool)> {
    let (tab, begin) = match frame {
        120..=145 => (EncyclopediaTab::Places, 120),
        210..=235 => (EncyclopediaTab::Army, 210),
        300..=325 => (EncyclopediaTab::Companies, 300),
        390..=415 => (EncyclopediaTab::People, 390),
        _ => return None,
    };
    Some((tab, frame == begin + 24))
}

fn input(
    state: Res<CaptureState>,
    mut frame: ResMut<TourFrame>,
    mut keys: ResMut<ButtonInput<KeyCode>>,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    mut buttons: Query<(&TabButton, &mut Interaction)>,
) {
    frame.0 = match *state {
        CaptureState::Settling { shot, .. } => Some(shot),
        _ => None,
    };
    let Some(frame) = frame.0 else {
        return;
    };
    keys.release(KeyCode::KeyN);
    keys.release(KeyCode::Escape);
    mouse.release(MouseButton::Left);
    if matches!(frame, 0 | 520) {
        keys.press(KeyCode::Escape);
    }
    if matches!(frame, 60 | 580) {
        keys.press(KeyCode::KeyN);
    }
    for (button, mut interaction) in &mut buttons {
        let next = match target(frame) {
            Some((tab, pressed)) if button.0 == tab => {
                if pressed {
                    mouse.press(MouseButton::Left);
                    Interaction::Pressed
                } else {
                    Interaction::Hovered
                }
            }
            _ => Interaction::None,
        };
        interaction.set_if_neq(next);
    }
}

fn inspect(
    frame: Res<TourFrame>,
    config: Res<CaptureConfig>,
    open: Res<EncyclopediaOpen>,
    tab: Res<EncyclopediaTab>,
    roots: Query<(), With<EncyclopediaRoot>>,
    bodies: Query<(&TabBody, &Node)>,
    panels: Query<(&UiTransform, &ComputedNode), With<EncyclopediaPanel>>,
    buttons: Query<(Entity, &TabButton, &Interaction, &UiTransform)>,
    labels: Query<(&ChildOf, &UiTransform), With<crate::ui::foundation::UiButtonLabel>>,
) {
    let Some(frame) = frame.0 else {
        return;
    };
    let visible: Vec<_> = bodies
        .iter()
        .filter(|(_, node)| node.display != Display::None)
        .map(|(body, _)| body.0)
        .collect();
    if matches!(frame, 20 | 540) {
        assert!(
            !open.0 && roots.is_empty(),
            "Escape must close the encyclopedia"
        );
    }
    let expected = match frame {
        90 | 420 | 630 => Some(EncyclopediaTab::People),
        150 => Some(EncyclopediaTab::Places),
        240 => Some(EncyclopediaTab::Army),
        330 => Some(EncyclopediaTab::Companies),
        _ => None,
    };
    if let Some(expected) = expected {
        assert!(
            open.0 && roots.iter().count() == 1,
            "one open book after action"
        );
        assert_eq!(
            *tab, expected,
            "tab action must reach production navigation"
        );
        assert_eq!(
            visible,
            vec![expected],
            "exactly the selected page is visible"
        );
    }
    if matches!(frame, 140 | 230 | 320 | 410) {
        let (expected, _) = target(frame).unwrap();
        let (button, _, interaction, transform) =
            buttons.iter().find(|(_, b, ..)| b.0 == expected).unwrap();
        assert_eq!(*interaction, Interaction::Hovered);
        assert_eq!(
            transform.translation,
            bevy::ui::Val2::ZERO,
            "button hit area stays fixed"
        );
        assert!(
            labels
                .iter()
                .any(|(parent, transform)| parent.parent() == button
                    && matches!(transform.translation.y, Val::Px(y) if y < -0.5)),
            "hover label spring must move"
        );
    }
    if frame % config.probe_every.max(1) as usize != 0 && frame + 1 != config.shots.len() {
        return;
    }
    let panel = panels.iter().next().map(|(transform, node)| {
        serde_json::json!({
            "translation": format!("{:?}", transform.translation), "size": node.size().to_array(),
        })
    });
    let record = serde_json::json!({"frame": frame, "open": open.0,
        "tab": format!("{:?}", *tab), "visible_pages": format!("{visible:?}"),
        "panel": panel, "input": "semantic Interaction and keyboard edges; offscreen target"});
    let path = config
        .out_dir
        .join(format!("{}.ui.json", config.shots[frame].name));
    std::fs::write(path, serde_json::to_vec_pretty(&record).unwrap())
        .expect("write UI rehearsal evidence");
}
