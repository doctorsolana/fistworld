//! Real sink + retained-menu rehearsal. Only input and accelerated playback are
//! fixtures; production code owns playback, mute/resume and opening priority.

use super::{CaptureConfig, CaptureState};
use crate::{
    audio::{
        AudioSettings,
        music::{MusicCue, MusicPlayback},
    },
    ui::pause_menu::MusicToggle,
};
use bevy::{
    input::InputSystems,
    prelude::*,
    ui::{UiGlobalTransform, UiSystems},
};
use std::time::Instant;

#[derive(Resource)]
pub(super) struct MusicReview {
    shot: Option<usize>,
    ready: bool,
    action_sent: bool,
    since: Instant,
    background: Option<Entity>,
    paused_at: f64,
}

pub(super) fn install(app: &mut App) {
    if std::env::var("FISTWORLD_CAPTURE_MUSIC").as_deref() != Ok("1") {
        return;
    }
    app.insert_resource(MusicReview {
        shot: None,
        ready: true,
        action_sent: false,
        since: Instant::now(),
        background: None,
        paused_at: 0.0,
    });
    app.add_systems(PreUpdate, input.after(InputSystems).after(UiSystems::Focus));
    app.add_systems(Last, inspect);
}

pub(super) fn ready(review: Option<Res<MusicReview>>) -> bool {
    review.is_none_or(|review| review.ready)
}

fn input(
    state: Res<CaptureState>,
    mut review: ResMut<MusicReview>,
    mut music: ResMut<MusicPlayback>,
    mut buttons: Query<
        (
            &Name,
            &mut Interaction,
            &mut bevy::ui::RelativeCursorPosition,
        ),
        With<MusicToggle>,
    >,
    mut mouse: ResMut<ButtonInput<MouseButton>>,
    mut players: Query<(&MusicCue, &mut AudioSink)>,
) {
    let CaptureState::Settling { shot, .. } = *state else {
        return;
    };
    mouse.release(MouseButton::Left);
    for (_, mut interaction, mut cursor) in &mut buttons {
        interaction.set_if_neq(Interaction::None);
        cursor.cursor_over = false;
        cursor.normalized = None;
    }
    if review.shot != Some(shot) {
        review.shot = Some(shot);
        review.ready = false;
        review.action_sent = false;
        review.since = Instant::now();
    }
    assert!(
        review.since.elapsed().as_secs() < 90,
        "music review timed out at shot {shot}"
    );
    if review.action_sent {
        return;
    }
    match shot {
        1 | 2 | 4 | 5 => {
            let name = if matches!(shot, 1 | 4) {
                "pause-music-off"
            } else {
                "pause-music-on"
            };
            let Some((_, mut interaction, mut cursor)) = buttons
                .iter_mut()
                .find(|(actual, _, _)| actual.as_str() == name)
            else {
                return;
            };
            *interaction = Interaction::Pressed;
            cursor.cursor_over = true;
            cursor.normalized = Some(Vec2::ZERO);
            mouse.press(MouseButton::Left);
        }
        3 => music.request_opening(),
        6 => {
            let Some((_, mut sink)) = players
                .iter_mut()
                .find(|(cue, _)| **cue == MusicCue::Background)
            else {
                return;
            };
            // The Vorbis decoder cannot seek. Run it silently at 100× to exercise
            // actual source completion without waiting six minutes.
            sink.set_volume(bevy::audio::Volume::SILENT);
            sink.set_speed(100.0);
        }
        _ => {}
    }
    review.action_sent = true;
}

fn inspect(
    state: Res<CaptureState>,
    config: Res<CaptureConfig>,
    mut review: ResMut<MusicReview>,
    settings: Res<AudioSettings>,
    players: Query<(Entity, &MusicCue, Option<&AudioSink>)>,
    buttons: Query<
        (
            &ComputedNode,
            &UiGlobalTransform,
            &crate::ui::foundation::UiButtonStyle,
            &Name,
        ),
        With<MusicToggle>,
    >,
) {
    let CaptureState::Settling { shot, .. } = *state else {
        return;
    };
    if review.shot != Some(shot) || !review.action_sent || review.ready {
        return;
    }
    let active: Vec<_> = players.iter().collect();
    assert!(
        active.len() <= 1,
        "opening and background music must not overlap"
    );
    let enabled = !matches!(shot, 1 | 4);
    let name = if enabled {
        "pause-music-on"
    } else {
        "pause-music-off"
    };
    let Some((node, transform, style, _)) = buttons
        .iter()
        .find(|(_, _, _, actual)| actual.as_str() == name)
    else {
        return;
    };
    if settings.music_enabled != enabled || !style.selected {
        return;
    }
    let label = if enabled { "ON" } else { "OFF" };
    let centre = transform.transform_point2(Vec2::ZERO);
    let half = node.size() * 0.5;
    assert!(node.size().min_element() > 0.0);
    assert!(
        (centre - half).cmpge(Vec2::ZERO).all()
            && (centre + half)
                .cmple(Vec2::new(
                    config.resolution[0] as f32,
                    config.resolution[1] as f32
                ))
                .all(),
        "music button fits inside the capture"
    );
    let expected_cue = if shot == 3 {
        MusicCue::Opening
    } else {
        MusicCue::Background
    };
    if matches!(shot, 4 | 6) {
        if !active.is_empty() {
            return;
        }
    } else {
        let Some((entity, cue, Some(sink))) = active.first() else {
            return;
        };
        if **cue != expected_cue || sink.empty() || sink.is_paused() != (shot == 1) {
            return;
        }
        let expected_gain = cue.base_gain() * settings.music_gain();
        if (sink.volume().to_linear() - expected_gain).abs() > 0.001 {
            // A screenshot taken during the gain ramp does not establish the
            // settled mix. Completion's deliberate silent override is excluded.
            return;
        }
        if shot == 0 {
            if review.background.is_none() {
                review.background = Some(*entity);
                info!("music review: background playback ready for audition");
            }
            // Leave an audible sample for an application-audio recorder before
            // the mute rehearsal advances, using actual decoder progress.
            if sink.position().as_secs_f64() < 12.0 {
                return;
            }
        }
        if shot == 1 {
            assert_eq!(review.background, Some(*entity));
            review.paused_at = sink.position().as_secs_f64();
        }
        if shot == 2 {
            assert_eq!(
                review.background,
                Some(*entity),
                "unmute retains the existing decoder"
            );
            if sink.position().as_secs_f64() < review.paused_at + 0.1 {
                return;
            }
        }
    }
    let evidence = serde_json::json!({
        "shot": shot, "music_enabled": settings.music_enabled, "label": label,
        "master_volume": settings.master_volume, "music_volume": settings.music_volume,
        "button_center": centre.to_array(), "button_size": node.size().to_array(),
        "players": active.iter().map(|(entity, cue, sink)| serde_json::json!({
            "entity": format!("{entity:?}"), "cue": format!("{cue:?}"),
            "sink_ready": sink.is_some(), "paused": sink.map(|sink| sink.is_paused()),
            "position_seconds": sink.map(|sink| sink.position().as_secs_f64()),
            "volume": sink.map(|sink| sink.volume().to_linear()),
            "expected_volume": cue.base_gain() * settings.music_gain(),
        })).collect::<Vec<_>>(),
        "input": "production Music button Interaction; opening request and silent 100x playback are explicit fixtures",
    });
    std::fs::write(
        config
            .out_dir
            .join(format!("{}.music.json", config.shots[shot].name)),
        serde_json::to_vec_pretty(&evidence).unwrap(),
    )
    .unwrap();
    review.ready = true;
}
