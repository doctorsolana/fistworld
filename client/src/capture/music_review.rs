//! Real sink + retained-menu rehearsal. Only input and accelerated playback are
//! fixtures; production code owns playback, mute/resume and opening priority.

use super::{CaptureConfig, CaptureState};
use crate::{
    audio::{
        music::{MusicCue, MusicPlayback},
        AudioSettings,
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
    mut buttons: Query<&mut Interaction, With<MusicToggle>>,
    mut players: Query<(&MusicCue, &mut AudioSink)>,
) {
    let CaptureState::Settling { shot, .. } = *state else {
        return;
    };
    for mut interaction in &mut buttons {
        interaction.set_if_neq(Interaction::None);
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
            let Ok(mut interaction) = buttons.single_mut() else {
                return;
            };
            *interaction = Interaction::Pressed;
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
    buttons: Query<(&ComputedNode, &UiGlobalTransform, &Children), With<MusicToggle>>,
    texts: Query<&Text>,
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
    let Some((node, transform, children)) = buttons.iter().next() else {
        return;
    };
    let label = children
        .iter()
        .find_map(|entity| texts.get(entity).ok())
        .map(|text| text.0.as_str())
        .unwrap_or("");
    let enabled = !matches!(shot, 1 | 4);
    if settings.music_enabled != enabled
        || label != if enabled { "MUSIC: ON" } else { "MUSIC: OFF" }
    {
        return;
    }
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
        if shot == 0 {
            if sink.position().as_secs_f64() < 0.1 {
                return;
            }
            review.background = Some(*entity);
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
        "button_center": centre.to_array(), "button_size": node.size().to_array(),
        "players": active.iter().map(|(entity, cue, sink)| serde_json::json!({
            "entity": format!("{entity:?}"), "cue": format!("{cue:?}"),
            "sink_ready": sink.is_some(), "paused": sink.map(|sink| sink.is_paused()),
            "position_seconds": sink.map(|sink| sink.position().as_secs_f64()),
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
