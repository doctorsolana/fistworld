//! Opt-in connected smoke test of real display controls and native window modes.

use super::{display::SelectDisplayMode, *};
use crate::{
    capture::{live_capture_request, CaptureInspection},
    capture_artifact::{request_capture, CaptureCompletions, CaptureTarget},
    render::systems::scaled_target::SceneRenderTarget,
};
use bevy::{
    ecs::system::NonSendMarker,
    input::InputSystems,
    render::view::screenshot::Screenshot,
    ui::{InteractionDisabled, UiSystems},
    window::WindowMode,
};
use std::{path::PathBuf, time::Instant};

#[derive(Clone, Copy, Debug)]
enum Action {
    Mode(DisplayMode),
    Keep,
    LowerScene,
    PreferredOutput,
    LowerOutput,
    Revert,
}

const ACTIONS: &[Action] = &[
    Action::Mode(DisplayMode::Windowed),
    Action::Keep,
    Action::Mode(DisplayMode::Borderless),
    Action::Keep,
    Action::LowerScene,
    Action::Mode(DisplayMode::ExclusiveFullscreen),
    Action::PreferredOutput,
    Action::LowerOutput,
    Action::Revert,
    Action::Mode(DisplayMode::Windowed),
    Action::Keep,
];

#[derive(Resource)]
struct DisplayCapture {
    out: PathBuf,
    started: Instant,
    step: usize,
    sent: bool,
    release: Option<Entity>,
    ticket: Option<u64>,
    stable: u32,
    last_size: Option<UVec2>,
    before_scale: f32,
    before_output: DisplayResolution,
    borderless_size: Option<UVec2>,
    preferred: Option<DisplayResolution>,
    waiting_for_monitor: bool,
}

impl DisplayCapture {
    fn advance(&mut self) {
        self.step += 1;
        self.sent = false;
        self.stable = 0;
    }
}

pub(super) fn install(app: &mut App) {
    let Some(out) = std::env::var_os("FISTWORLD_DISPLAY_CAPTURE_DIR") else {
        return;
    };
    assert!(
        std::env::var_os("FISTFORCE_NO_SETTINGS_FILE").is_some(),
        "display capture requires FISTFORCE_NO_SETTINGS_FILE=1"
    );
    app.insert_resource(DisplayCapture {
        out: out.into(),
        started: Instant::now(),
        step: 0,
        sent: false,
        release: None,
        ticket: None,
        stable: 0,
        last_size: None,
        before_scale: 1.0,
        before_output: DisplayResolution::new(0, 0),
        borderless_size: None,
        preferred: None,
        waiting_for_monitor: false,
    });
    app.add_systems(OnEnter(GameState::Playing), |mut commands: Commands| {
        open_for_capture(&mut commands, "graphics")
    });
    app.add_systems(
        PreUpdate,
        drive
            .after(InputSystems)
            .after(UiSystems::Focus)
            .run_if(in_state(GameState::Playing)),
    );
}

fn drive(
    mut commands: Commands,
    mut state: ResMut<DisplayCapture>,
    settings: Res<GraphicsSettings>,
    windows: Query<(Entity, &Window), With<PrimaryWindow>>,
    monitors: Query<&Monitor, With<PrimaryMonitor>>,
    mut buttons: Query<
        (
            Entity,
            &mut Interaction,
            Option<&SelectDisplayMode>,
            Option<&SliderStep>,
            Option<&DisplayConfirmationAction>,
            Has<InteractionDisabled>,
        ),
        Or<(
            With<SelectDisplayMode>,
            With<SliderStep>,
            With<DisplayConfirmationAction>,
        )>,
    >,
    panels: Query<&Node, With<GraphicsSettingsPanel>>,
    pending: Option<Res<PendingDisplayChange>>,
    target: Res<SceneRenderTarget>,
    images: Res<Assets<Image>>,
    inspection: CaptureInspection,
    cameras: Query<&crate::camera_rts::CommanderCamera>,
    clocks: Query<&shared::components::WorldTime>,
    mut completions: ResMut<CaptureCompletions>,
    mut exit: MessageWriter<AppExit>,
    _main_thread: NonSendMarker,
) {
    assert!(
        state.started.elapsed().as_secs() < 180,
        "display capture timed out at step {}: {:?}",
        state.step,
        ACTIONS.get(state.step)
    );
    if let Some(entity) = state.release.take() {
        if let Ok((_, mut interaction, ..)) = buttons.get_mut(entity) {
            *interaction = Interaction::None;
        }
    }
    if let Some(ticket) = state.ticket {
        let Some(done) = completions.take(ticket) else {
            return;
        };
        assert!(
            done.error.is_none() && !done.comparison_failed,
            "display capture failed: {:?}",
            done.error
        );
        state.ticket = None;
        state.advance();
    }
    let Some(&action) = ACTIONS.get(state.step) else {
        std::fs::write(state.out.join("summary.json"), b"{\"passed\":true,\"input\":\"production UI Interaction actions\",\"final_mode\":\"Windowed\"}").unwrap();
        info!("Display capture passed all mode/resolution/revert actions");
        exit.write(AppExit::Success);
        return;
    };
    if !panels.iter().any(|panel| panel.display == Display::Flex) {
        return;
    }
    let Ok((window_entity, window)) = windows.single() else {
        return;
    };
    let Some(image) = images.get(&target.image) else {
        return;
    };
    if state.step == 0 && inspection.loaded_chunk_count() < 64 {
        return;
    }
    // macOS exposes no active monitors while the session is locked. Native
    // mode changes cannot be verified then; wait before sending any input.
    if monitors.is_empty() {
        if !state.waiting_for_monitor {
            info!("Display capture waiting for an active monitor; unlock the computer to continue");
            state.waiting_for_monitor = true;
        }
        return;
    }
    state.waiting_for_monitor = false;
    let choices = available_display_resolutions(
        settings.display_mode(),
        monitors.iter().next(),
        settings.display_resolution,
    );
    let preferred = *state.preferred.get_or_insert_with(|| {
        let exclusive = available_display_resolutions(
            DisplayMode::ExclusiveFullscreen,
            monitors.iter().next(),
            settings.display_resolution,
        );
        let preferred = if exclusive.contains(&DisplayResolution::new(1920, 1080)) {
            DisplayResolution::new(1920, 1080)
        } else {
            actions::nearest_resolution(settings.display_resolution, &exclusive)
        };
        // Start above the minimum so the following lower-output action really
        // exercises a native video-mode change when the monitor supports one.
        if exclusive.first() == Some(&preferred) && exclusive.len() > 1 {
            exclusive[1]
        } else {
            preferred
        }
    });

    if !state.sent {
        if matches!(action, Action::Keep) && pending.is_none() {
            state.advance();
            return;
        }
        if matches!(action, Action::LowerOutput)
            && choices.first() == Some(&settings.display_resolution)
        {
            info!("Display capture: already at the smallest OS-supported output mode");
            state.advance();
            return;
        }
        state.before_scale = settings.render_scale;
        state.before_output = settings.display_resolution;
        let already_applied = matches!(action, Action::Mode(mode) if settings.display_mode() == mode)
            || matches!(action, Action::PreferredOutput if settings.display_resolution == preferred);
        if !already_applied {
            let next = buttons
                .iter_mut()
                .find(|(_, _, mode, step, confirmation, disabled)| {
                    if *disabled {
                        return false;
                    }
                    match action {
                        Action::Mode(wanted) => mode.is_some_and(|mode| mode.0 == wanted),
                        Action::Keep => confirmation.is_some_and(|action| {
                            matches!(action, DisplayConfirmationAction::Keep)
                        }),
                        Action::Revert => confirmation.is_some_and(|action| {
                            matches!(action, DisplayConfirmationAction::Revert)
                        }),
                        Action::LowerScene => step.is_some_and(|step| {
                            matches!(step.control, SliderControl::RenderScale) && step.delta < 0
                        }),
                        Action::LowerOutput => step.is_some_and(|step| {
                            matches!(step.control, SliderControl::Resolution) && step.delta < 0
                        }),
                        Action::PreferredOutput => step.is_some_and(|step| {
                            matches!(step.control, SliderControl::Resolution)
                                && (step.delta > 0)
                                    == (u64::from(preferred.width) * u64::from(preferred.height)
                                        > u64::from(settings.display_resolution.width)
                                            * u64::from(settings.display_resolution.height))
                        }),
                    }
                });
            let Some((entity, mut interaction, ..)) = next else {
                return;
            };
            info!("Display capture UI action {}: {action:?}", state.step);
            *interaction = Interaction::Pressed;
            state.release = Some(entity);
        }
        state.sent = true;
        state.stable = 0;
        return;
    }

    let action_applied = match action {
        Action::Mode(mode) => settings.display_mode() == mode,
        Action::Keep => pending.is_none(),
        Action::LowerScene => settings.render_scale < state.before_scale,
        Action::PreferredOutput => {
            settings.display_resolution == preferred
                || settings.display_resolution != state.before_output
        }
        Action::LowerOutput => {
            settings.display_resolution.width * settings.display_resolution.height
                < state.before_output.width * state.before_output.height
        }
        Action::Revert => settings.display_mode() == DisplayMode::Borderless && pending.is_none(),
    };
    // Cocoa fullscreen may exclude the notch safe area. Compare the rendered
    // content with winit's actual inner size, not the monitor's full scanout.
    let native = bevy::winit::WINIT_WINDOWS.with_borrow(|windows| {
        let window = windows.get_window(window_entity)?;
        let inner = window.inner_size();
        let monitor = window.current_monitor()?.size();
        Some((
            window.fullscreen(),
            UVec2::new(inner.width, inner.height),
            UVec2::new(monitor.width, monitor.height),
        ))
    });
    let Some((native_fullscreen, native_inner, native_monitor)) = native else {
        return;
    };
    let requested = UVec2::new(
        settings.display_resolution.width,
        settings.display_resolution.height,
    );
    let native_mode_matches = match (settings.display_mode(), &native_fullscreen) {
        (DisplayMode::Windowed, None) => native_inner == requested,
        (DisplayMode::Borderless, Some(winit::window::Fullscreen::Borderless(_))) => {
            native_inner.x == native_monitor.x
        }
        (DisplayMode::ExclusiveFullscreen, Some(winit::window::Fullscreen::Exclusive(mode))) => {
            let video = mode.size();
            UVec2::new(video.width, video.height) == requested && native_monitor == requested
        }
        _ => false,
    };
    let mode_matches = matches!(
        (settings.display_mode(), window.mode),
        (DisplayMode::Windowed, WindowMode::Windowed)
            | (DisplayMode::Borderless, WindowMode::BorderlessFullscreen(_))
            | (
                DisplayMode::ExclusiveFullscreen,
                WindowMode::Fullscreen(_, _)
            )
    );
    let scene = scene_render_resolution(window, settings.render_scale);
    let size = window.physical_size();
    let ready = action_applied
        && mode_matches
        && native_mode_matches
        && native_inner == size
        && image.width() == scene.width
        && image.height() == scene.height;
    state.stable = if ready && state.last_size == Some(size) {
        state.stable + 1
    } else {
        0
    };
    state.last_size = Some(size);
    if state.stable < 12 {
        return;
    }
    if matches!(action, Action::Keep) {
        state.advance();
        return;
    }
    if matches!(action, Action::PreferredOutput) && settings.display_resolution != preferred {
        state.sent = false;
        return;
    }
    if matches!(action, Action::Revert) {
        assert_eq!(
            Some(size),
            state.borderless_size,
            "revert restores the original native content area"
        );
    }
    if settings.display_mode() == DisplayMode::Borderless {
        state.borderless_size = Some(size);
    }
    if matches!(
        action,
        Action::Mode(DisplayMode::ExclusiveFullscreen)
            | Action::PreferredOutput
            | Action::LowerOutput
    ) {
        assert!(
            pending
                .as_ref()
                .is_some_and(|pending| pending.previous_mode == DisplayMode::Borderless),
            "exclusive changes must retain confirmed borderless rollback"
        );
    }
    let name = format!(
        "{:02}-{}",
        state.step,
        match action {
            Action::Mode(DisplayMode::Windowed) => "windowed",
            Action::Mode(DisplayMode::Borderless) => "borderless",
            Action::Mode(DisplayMode::ExclusiveFullscreen) => "exclusive",
            Action::LowerScene => "lower-3d",
            Action::PreferredOutput => "exclusive-preferred",
            Action::LowerOutput => "lower-output",
            Action::Revert => "reverted-borderless",
            Action::Keep => unreachable!(),
        }
    );
    std::fs::create_dir_all(&state.out).unwrap();
    let evidence = serde_json::json!({"action":format!("{action:?}"),"mode":format!("{:?}",settings.display_mode()),"window_mode":format!("{:?}",window.mode),"native_fullscreen":format!("{native_fullscreen:?}"),"native_inner_size":native_inner.to_array(),"native_monitor_size":native_monitor.to_array(),"physical_size":size.to_array(),"scene_size":[image.width(),image.height()],"render_scale":settings.render_scale,"display_resolution":settings.display_resolution,"pending_confirmation":pending.is_some(),"stable_frames":state.stable,"input":"real display UI Interaction"});
    std::fs::write(
        state.out.join(format!("{name}.display.json")),
        serde_json::to_vec_pretty(&evidence).unwrap(),
    )
    .unwrap();
    let mut request = live_capture_request(
        state.out.join(format!("{name}.png")),
        "display-modes",
        &name,
        CaptureTarget::Scene,
    );
    inspection
        .complete_live_request(
            &mut request,
            cameras.iter().next(),
            clocks.iter().next(),
            state.stable,
        )
        .unwrap();
    state.ticket = Some(request_capture(
        &mut commands,
        Screenshot::image(target.image.clone()),
        request,
        &mut completions,
    ));
}
