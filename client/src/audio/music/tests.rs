use super::*;

fn app() -> App {
    let mut app = App::new();
    app.add_plugins((
        bevy::app::TaskPoolPlugin::default(),
        AssetPlugin {
            file_path: format!("{}/assets", env!("CARGO_MANIFEST_DIR")),
            ..default()
        },
    ));
    app.init_asset::<AudioSource>();
    app.init_asset_loader::<bevy::audio::AudioLoader>();
    app.insert_resource(Time::<Real>::default());
    app.init_resource::<AudioSettings>();
    app.init_resource::<MusicPlayback>();
    app.add_systems(Update, update_music);
    app
}

fn tick(app: &mut App, seconds: u64) {
    app.world_mut()
        .resource_mut::<Time<Real>>()
        .advance_by(std::time::Duration::from_secs(seconds));
    app.update();
}

fn cues(app: &mut App) -> Vec<(Entity, MusicCue, bool)> {
    app.world_mut()
        .query::<(Entity, &MusicCue, &PlaybackSettings)>()
        .iter(app.world())
        .map(|(entity, cue, settings)| (entity, *cue, settings.paused))
        .collect()
}

#[test]
fn disabled_music_does_not_start_or_load_background() {
    let mut app = app();
    app.world_mut()
        .resource_mut::<AudioSettings>()
        .music_enabled = false;
    tick(&mut app, 120);
    assert!(cues(&mut app).is_empty());
    assert!(app.world().resource::<MusicPlayback>().background.is_none());
}

#[test]
fn pending_background_is_single_and_retained_when_toggled() {
    let mut app = app();
    tick(&mut app, 5);
    let initial = cues(&mut app);
    assert_eq!(initial.len(), 1);
    assert_eq!(
        app.world()
            .get::<PlaybackSettings>(initial[0].0)
            .unwrap()
            .volume,
        Volume::Linear(0.4),
        "background cue combines its 80% authored gain with the 50% music default"
    );
    tick(&mut app, 50);
    assert_eq!(cues(&mut app), initial);
    app.world_mut()
        .resource_mut::<AudioSettings>()
        .music_enabled = false;
    tick(&mut app, 5);
    assert_eq!(
        cues(&mut app),
        vec![(initial[0].0, MusicCue::Background, true)]
    );
    app.world_mut()
        .resource_mut::<AudioSettings>()
        .music_enabled = true;
    tick(&mut app, 5);
    assert_eq!(cues(&mut app), initial);
}

#[test]
fn opening_replaces_background_and_disabled_opening_never_resumes_late() {
    let mut app = app();
    tick(&mut app, 5);
    app.world_mut()
        .resource_mut::<MusicPlayback>()
        .request_opening();
    tick(&mut app, 1);
    assert_eq!(cues(&mut app).len(), 1);
    assert_eq!(cues(&mut app)[0].1, MusicCue::Opening);
    let opening = cues(&mut app)[0].0;
    assert_eq!(
        app.world().get::<PlaybackSettings>(opening).unwrap().volume,
        Volume::Linear(0.41),
        "opening retains its own authored gain while respecting the music default"
    );
    app.world_mut()
        .resource_mut::<AudioSettings>()
        .music_enabled = false;
    tick(&mut app, 1);
    assert!(cues(&mut app).is_empty());
    app.world_mut()
        .resource_mut::<AudioSettings>()
        .music_enabled = true;
    tick(&mut app, 5);
    assert_eq!(cues(&mut app)[0].1, MusicCue::Background);
    app.world_mut().run_system_cached(stop_music).unwrap();
    assert!(cues(&mut app).is_empty());
    assert!(app.world().resource::<MusicPlayback>().background.is_none());
}

#[test]
fn chosen_background_ships_as_an_ogg_vorbis_asset() {
    let bytes = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("assets")
            .join(paths::BACKGROUND_MUSIC),
    )
    .unwrap();
    assert!(bytes.starts_with(b"OggS"));
    assert!(bytes.windows(7).any(|window| window == b"\x01vorbis"));
    assert!(
        bytes.len() < 6 * 1024 * 1024,
        "six-minute cue stays within its export budget"
    );
}

#[test]
fn live_levels_update_pending_music_and_zero_pauses_without_restarting() {
    let mut app = app();
    tick(&mut app, 5);
    let entity = cues(&mut app)[0].0;
    {
        let mut settings = app.world_mut().resource_mut::<AudioSettings>();
        settings.master_volume = 0.5;
        settings.music_volume = 0.4;
    }
    tick(&mut app, 1);
    assert!(
        (app.world()
            .get::<PlaybackSettings>(entity)
            .unwrap()
            .volume
            .to_linear()
            - 0.16)
            .abs()
            < 1e-6,
        "live preferences also apply the background cue's authored gain"
    );
    app.world_mut()
        .resource_mut::<AudioSettings>()
        .master_volume = 0.0;
    tick(&mut app, 1);
    assert_eq!(cues(&mut app), vec![(entity, MusicCue::Background, true)]);
    app.world_mut()
        .resource_mut::<AudioSettings>()
        .master_volume = 0.5;
    tick(&mut app, 1);
    assert_eq!(cues(&mut app), vec![(entity, MusicCue::Background, false)]);
    assert!(
        (app.world()
            .get::<PlaybackSettings>(entity)
            .unwrap()
            .volume
            .to_linear()
            - 0.16)
            .abs()
            < 1e-6
    );
}

#[test]
fn live_gain_ramps_for_eighty_milliseconds_then_stops_writing() {
    let mut ramp = MusicGainRamp::new(1.0);
    ramp.retarget(0.2);
    let halfway = ramp.advance(0.04).unwrap();
    assert!((halfway - 0.6).abs() < 1e-6);
    assert_eq!(ramp.advance(0.04), Some(0.2));
    assert_eq!(
        ramp.advance(0.016),
        None,
        "settled gain must not overwrite an explicit silent capture override"
    );
    ramp.retarget(0.2);
    assert_eq!(
        ramp.advance(1.0),
        None,
        "an unrelated effects preference must not restart the music ramp"
    );
}

#[test]
fn successive_slider_edits_start_at_current_gain_and_hitches_complete_once() {
    let mut ramp = MusicGainRamp::new(1.0);
    ramp.retarget(0.2);
    ramp.advance(0.04);
    let before = ramp.current;
    ramp.retarget(0.8);
    assert_eq!(ramp.current, before, "retargeting must not jump the signal");
    assert_eq!(ramp.advance(0.0), None);
    let midway = ramp.advance(0.04).unwrap();
    assert!(midway > before && midway < 0.8);
    assert_eq!(ramp.advance(0.5), Some(0.8));
    assert_eq!(ramp.advance(0.5), None);
    // An immediately paused zero-gain background resumes with a finite fade.
    let mut muted = MusicGainRamp::new(0.0);
    muted.retarget(0.8);
    assert!((muted.advance(0.04).unwrap() - 0.4).abs() < 0.000001);
    assert_eq!(muted.advance(0.04), Some(0.8));
}
