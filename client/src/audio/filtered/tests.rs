use super::*;
use bevy::audio::Source;

fn tone(hz: f32, seconds: f32) -> Arc<MonoClip> {
    let rate = 44100;
    Arc::new(MonoClip {
        samples: (0..(rate as f32 * seconds) as usize)
            .map(|n| (std::f32::consts::TAU * hz * n as f32 / rate as f32).sin() * 0.5)
            .collect(),
        rate: SampleRate::new(rate).unwrap(),
    })
}

fn rms(decoder: &mut impl Iterator<Item = f32>, samples: usize) -> f32 {
    (decoder.take(samples).map(|s| s * s).sum::<f32>() / samples as f32).sqrt()
}

#[test]
fn distant_filter_softens_rattle_but_keeps_low_rolling_body() {
    fn level(frequency: f32, cutoff: f32) -> f32 {
        let (source, _) = FilteredSound::new(tone(frequency, 0.1), true, cutoff);
        let mut decoder = source.decoder();
        decoder.by_ref().take(4410).for_each(drop);
        rms(&mut decoder, 44100)
    }
    let close = level(8000.0, 7000.0);
    let far = level(8000.0, 1200.0);
    assert!(
        far < close * 0.1,
        "distant sharp detail falls at least 20 dB: {close}/{far}"
    );
    assert!(level(200.0, 1200.0) > level(200.0, 7000.0) * 0.9);
    // User mix contract: even directly under a fully zoomed-in camera the cart
    // must suppress close-mic rattle, while preserving its low rolling body.
    let closest = crate::audio::perspective::CART.cutoff_hz(0.0, 0.0);
    let dry_rms = 0.5 / 2.0_f32.sqrt();
    assert!(level(8000.0, closest) < dry_rms * 0.1);
    assert!(level(200.0, closest) > dry_rms * 0.9);
}

#[test]
fn filter_remains_live_after_multiple_loops_without_restarting_cursor_or_history() {
    let (source, control) = FilteredSound::new(tone(8000.0, 0.1), true, 7000.0);
    let mut decoder = source.decoder();
    decoder.by_ref().take(3 * 4410).for_each(drop);
    let before = rms(&mut decoder, 4410);
    control.set_cutoff(1200.0);
    // Wait on the bounded control polling/ramp length in samples, not wall time.
    decoder.by_ref().take(1100).for_each(drop);
    let after = rms(&mut decoder, 4410);
    assert!(after < before * 0.1);
    control.set_cutoff(7000.0);
    decoder.by_ref().take(1100).for_each(drop);
    assert!((rms(&mut decoder, 4410) - before).abs() < 0.001);
    assert_eq!(decoder.total_duration(), None);
}

#[test]
fn source_lifecycle_shares_clip_and_only_loops_when_requested() {
    let clip = tone(500.0, 0.1);
    let (one_shot, _) = FilteredSound::new(clip.clone(), false, 5000.0);
    let (looping, control) = FilteredSound::new(clip.clone(), true, 5000.0);
    assert!(Arc::ptr_eq(&one_shot.clip, &looping.clip));
    assert_eq!(one_shot.decoder().count(), clip.samples.len());
    assert_eq!(
        looping.decoder().take(clip.samples.len() * 3).count(),
        clip.samples.len() * 3
    );
    control.set_cutoff(f32::NAN);
    assert_eq!(control.cutoff_hz(), 5000.0);
    control.set_cutoff(-900.0);
    assert_eq!(control.cutoff_hz(), 100.0);
    let weak = Arc::downgrade(&clip);
    drop(clip);
    drop(one_shot);
    drop(looping);
    assert!(weak.upgrade().is_none());
}

#[test]
fn moving_coefficients_are_finite_nonresonant_and_do_not_click_on_silence() {
    let clip = Arc::new(MonoClip {
        samples: vec![0.0; 512].into(),
        rate: SampleRate::new(44100).unwrap(),
    });
    let (sound, control) = FilteredSound::new(clip, true, 7000.0);
    let mut decoder = sound.decoder();
    for cutoff in [100.0, 20000.0, 900.0, 7000.0] {
        control.set_cutoff(cutoff);
        assert!(decoder.by_ref().take(2000).all(|v| v == 0.0));
    }
    let (sound, control) = FilteredSound::new(tone(4200.0, 0.1), true, 100.0);
    let mut decoder = sound.decoder();
    for cutoff in (100..20000).step_by(73) {
        control.set_cutoff(cutoff as f32);
        assert!(decoder
            .by_ref()
            .take(127)
            .all(|v| v.is_finite() && v.abs() <= 0.5));
    }
}

#[test]
fn actual_cart_decodes_once_within_pcm_budget_and_bad_files_fail_cleanly() {
    let bytes = std::fs::read(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("assets/audio/sfx/vehicles/cart_roll.ogg"),
    )
    .unwrap();
    let clip = cache::decode(AudioSource {
        bytes: bytes.into(),
    })
    .unwrap();
    assert_eq!(clip.rate.get(), 44100);
    assert!(clip.bytes() < 1100000);
    assert!((clip.samples.len() as f32 / 44100.0 - 6.05).abs() < 0.01);
    assert!(cache::decode(AudioSource {
        bytes: Arc::from(&b"not audio"[..])
    })
    .is_err());
}

#[test]
fn native_outer_repeat_is_rejected_before_it_can_buffer_an_infinite_source() {
    let mut app = App::new();
    let voice = app
        .world_mut()
        .spawn((
            AudioPlayer::<FilteredSound>(Handle::default()),
            PlaybackSettings::LOOP,
        ))
        .id();
    app.world_mut()
        .run_system_cached(prevent_outer_loop)
        .unwrap();
    assert!(matches!(
        app.world().get::<PlaybackSettings>(voice).unwrap().mode,
        bevy::audio::PlaybackMode::Once
    ));
}
