use super::*;
use std::time::{Duration, Instant};

fn app() -> App {
    let mut app = App::new();
    app.add_plugins((
        bevy::app::TaskPoolPlugin::default(),
        AssetPlugin {
            file_path: format!("{}/assets", env!("CARGO_MANIFEST_DIR")),
            ..default()
        },
    ));
    app.init_asset::<AudioSource>()
        .init_asset_loader::<bevy::audio::AudioLoader>();
    app.init_resource::<Time<Real>>()
        .init_resource::<AudioSettings>();
    install(&mut app);
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        app.update();
        if SfxCue::ALL.into_iter().all(|cue| {
            app.world()
                .resource::<SfxAssets>()
                .ready_handle(cue, app.world().resource::<AssetServer>())
                .is_some()
        }) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "all shipped sound effects decode and load"
        );
        std::thread::sleep(Duration::from_millis(2));
    }
    app
}

fn request(app: &mut App, cues: &[SfxCue]) {
    for cue in cues {
        app.world_mut().write_message(SfxRequest::new(*cue));
    }
    app.update();
}

fn voices(app: &mut App) -> usize {
    app.world_mut()
        .query::<&UiSoundVoice>()
        .iter(app.world())
        .count()
}

#[test]
fn semantic_cue_wins_and_simultaneous_requests_allocate_one_voice() {
    assert_eq!(prefer(None, SfxCue::CartRoll), None);
    let mut app = app();
    request(
        &mut app,
        &[SfxCue::UiClick, SfxCue::BookOpen, SfxCue::UiConfirm],
    );
    assert_eq!(voices(&mut app), 1);
    assert_eq!(
        app.world().resource::<SfxStats>().started[SfxCue::BookOpen as usize],
        1
    );
    assert_eq!(app.world().resource::<SfxStats>().coalesced, 2);
    request(&mut app, &[SfxCue::UiClick]);
    assert_eq!(
        voices(&mut app),
        1,
        "rapid repeats drop before allocating a decoder"
    );
    assert_eq!(app.world().resource::<SfxStats>().budget_dropped, 1);
}

#[test]
fn voice_budget_counts_pending_and_expired_cues_never_resume() {
    let mut app = app();
    let handle = app
        .world()
        .resource::<SfxAssets>()
        .ready_handle(SfxCue::UiClick, app.world().resource::<AssetServer>())
        .unwrap();
    for _ in 0..MAX_UI_VOICES {
        app.world_mut().spawn((
            UiSoundVoice {
                cue: SfxCue::UiClick,
                started_at: 0.0,
                current_volume: SfxCue::UiClick.gain(),
            },
            AudioPlayer::new(handle.clone()),
            PlaybackSettings::DESPAWN,
        ));
    }
    request(&mut app, &[SfxCue::UiConfirm]);
    assert_eq!(voices(&mut app), MAX_UI_VOICES);
    assert_eq!(app.world().resource::<SfxStats>().budget_dropped, 1);
    app.world_mut()
        .resource_mut::<Time<Real>>()
        .advance_by(Duration::from_millis(300));
    app.update();
    assert_eq!(voices(&mut app), 0);
    assert_eq!(
        app.world().resource::<SfxStats>().expired_pending,
        MAX_UI_VOICES as u64
    );
    assert_eq!(
        app.world().resource::<SfxStats>().started[SfxCue::UiConfirm as usize],
        0
    );
}

#[test]
fn zero_effects_reclaims_live_requests_and_does_not_replay_on_unmute() {
    let mut app = app();
    request(&mut app, &[SfxCue::UiClick]);
    app.world_mut()
        .resource_mut::<AudioSettings>()
        .effects_volume = 0.0;
    request(&mut app, &[SfxCue::UiReject]);
    assert_eq!(voices(&mut app), 0);
    assert_eq!(app.world().resource::<SfxStats>().muted, 1);
    app.world_mut()
        .resource_mut::<AudioSettings>()
        .effects_volume = 1.0;
    app.world_mut()
        .resource_mut::<Time<Real>>()
        .advance_by(Duration::from_secs(1));
    app.update();
    assert_eq!(voices(&mut app), 0);
}
