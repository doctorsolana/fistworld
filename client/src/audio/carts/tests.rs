use super::*;

fn candidate(index: u64, distance: f32, retained: bool) -> Candidate {
    Candidate {
        owner: Entity::from_bits(index),
        position: Vec3::X * distance,
        speed: 2.0,
        score: source_gain(distance, 1.0, 2.0)
            * native_distance_estimate(distance)
            * if retained { 1.15 } else { 1.0 },
    }
}

#[test]
fn nearest_four_movers_win_and_new_close_cart_displaces_retained_distant_cart() {
    let mut ranked = [None; VOICE_LIMIT];
    for index in (1..=20).rev() {
        insert_candidate(&mut ranked, candidate(index, 12.0 + index as f32, true));
    }
    assert_eq!(
        ranked.map(|entry| entry.unwrap().owner.to_bits()),
        [1, 2, 3, 4]
    );
    insert_candidate(&mut ranked, candidate(50, 2.0, false));
    let owners = ranked.map(|entry| entry.unwrap().owner.to_bits());
    assert_eq!(owners, [50, 1, 2, 3]);
    // Removing stopped carts from the next survey releases all four slots.
    ranked.fill(None);
    insert_candidate(&mut ranked, candidate(70, 20.0, false));
    assert_eq!(ranked.iter().flatten().count(), 1);
    assert_eq!(ranked[0].unwrap().owner.to_bits(), 70);
}

#[test]
fn retained_voice_wins_small_audibility_changes_but_not_large_ones() {
    let mut ranked = [None; VOICE_LIMIT];
    insert_candidate(&mut ranked, candidate(1, 25.0, true));
    insert_candidate(&mut ranked, candidate(2, 24.0, false));
    assert_eq!(ranked[0].unwrap().owner.to_bits(), 1);
    insert_candidate(&mut ranked, candidate(3, 18.0, false));
    assert_eq!(ranked[0].unwrap().owner.to_bits(), 3);
}

#[test]
fn animation_intent_stationary_snapshots_teleports_and_hitches_are_silent() {
    assert!(measured_movement(0.2, 2.0, 0.1));
    assert!(!measured_movement(0.0, 2.0, 0.1));
    assert!(!measured_movement(0.2, 0.0, 0.1));
    assert!(!measured_movement(40.0, 400.0, 0.1));
    assert!(!measured_movement(0.2, 2.0, 1.0));
    assert!(!measured_movement(0.2, 2.0, 0.0));
    assert!(!measured_movement(0.2, f32::NAN, 0.1));
    let moving = CharacterMotion::new(Vec3::X * 2.0);
    assert!(eligible(
        Some(&CharacterActivity::Idle),
        Some(&moving),
        false,
        false
    ));
    assert!(!eligible(
        None,
        Some(&CharacterMotion::STATIONARY),
        false,
        false
    ));
    assert!(!eligible(
        Some(&CharacterActivity::Indoors),
        Some(&moving),
        false,
        false
    ));
    assert!(!eligible(None, Some(&moving), true, false));
    assert!(!eligible(None, Some(&moving), false, true));
}

#[test]
fn distance_falloff_is_only_in_native_backend_and_zoom_fades_continuously() {
    // Within the outer cutoff region the sink gain is distance-independent.
    assert_eq!(source_gain(5.0, 1.0, 2.0), source_gain(30.0, 1.0, 2.0));
    assert!(native_distance_estimate(30.0) < native_distance_estimate(5.0));
    assert_eq!(source_gain(HEARING_RADIUS, 1.0, 2.0), 0.0);
    assert_eq!(zoom_gain(12.0), 1.0);
    assert_eq!(zoom_gain(12_000.0), 0.0);
    let mut last = zoom_gain(100.0);
    for zoom in 100..=650 {
        let gain = zoom_gain(zoom as f32);
        assert!(gain <= last && last - gain < 0.003);
        last = gain;
    }
    assert_eq!(source_gain(1.0, zoom_gain(12_000.0), 2.0), 0.0);
}

#[test]
fn native_listener_compensation_preserves_correct_stereo_at_all_tested_positions() {
    // Exercise actual native samples, including when the pinned backend changes.
    fn rodio_pair(emitter: Vec3, left: Vec3, right: Vec3) -> Vec2 {
        let mono = rodio::buffer::SamplesBuffer::new(
            1.try_into().unwrap(),
            44100.try_into().unwrap(),
            vec![1.0; 4],
        );
        let mut native = rodio::source::Spatial::new(
            mono,
            emitter.to_array(),
            left.to_array(),
            right.to_array(),
        );
        Vec2::new(native.next().unwrap(), native.next().unwrap())
    }
    assert!(LISTENER_EAR_GAP / REFERENCE_DISTANCE <= 1.0 / 3.0);
    let listener = cart_spatial_listener();
    for yaw in [-1.4, 0.0, 0.9] {
        let rotation = Quat::from_rotation_y(yaw);
        let left = rotation * listener.left_ear_offset / REFERENCE_DISTANCE;
        let right = rotation * listener.right_ear_offset / REFERENCE_DISTANCE;
        for x in [
            -64.0, -20.0, -12.0, -5.0, -2.0, -0.1, 0.1, 2.0, 5.0, 12.0, 20.0, 64.0,
        ] {
            for y in [-100.0, -10.0, -0.75, 0.0, 10.0, 100.0] {
                for z in [-64.0, -10.0, 0.0, 10.0, 64.0] {
                    let emitter = rotation * Vec3::new(x, y, z) / REFERENCE_DISTANCE;
                    let gain = rodio_pair(emitter, left, right);
                    assert!(
                        (gain.x - gain.y) * x <= 1e-6,
                        "screen position ({x}, {y}, {z}), yaw {yaw}: {gain:?}"
                    );
                    let conventional = rodio_pair(emitter, right, left);
                    assert!(
                        (gain.element_sum() - conventional.element_sum()).abs() < 1e-6,
                        "compensation must preserve total gain"
                    );
                }
            }
        }
    }
}

#[test]
fn fade_reaches_zero_and_playback_speed_does_not_follow_time_warp() {
    let mut fade = 1.0;
    for _ in 0..20 {
        fade = approach(fade, 0.0, 1.0 / 60.0, FADE_SECONDS);
    }
    assert_eq!(fade, 0.0, "a suppressed loop must release its decoder");
    assert_eq!(approach(1.0, 0.0, 1.0, FADE_SECONDS), 0.0);
    assert_eq!(playback_speed(200.0), 1.06);
    assert_eq!(playback_speed(2.0), 1.0);
}

#[test]
fn zero_master_or_effects_clears_admission_between_surveys() {
    let mut app = App::new();
    app.init_resource::<Time<Real>>();
    app.init_resource::<AudioSettings>();
    app.init_resource::<CartAudioState>();
    app.init_resource::<CartAudioMetrics>();
    app.insert_resource(ListeningFrame {
        valid: true,
        detail: 1.0,
        ..default()
    });
    // No survey is due. A live slider change must still discard old intents.
    app.world_mut().resource_mut::<CartAudioState>().desired[0] = Some(candidate(1, 2.0, true));
    app.world_mut().run_system_cached(collect_carts).unwrap();
    assert!(app.world().resource::<CartAudioState>().desired[0].is_some());
    for (master, effects) in [(0.0, 1.0), (1.0, 0.0)] {
        {
            let mut settings = app.world_mut().resource_mut::<AudioSettings>();
            settings.master_volume = master;
            settings.effects_volume = effects;
        }
        app.world_mut().resource_mut::<CartAudioState>().desired[0] = Some(candidate(1, 2.0, true));
        app.world_mut().run_system_cached(collect_carts).unwrap();
        assert!(app
            .world()
            .resource::<CartAudioState>()
            .desired
            .iter()
            .all(Option::is_none));
        assert_eq!(app.world().resource::<CartAudioMetrics>().candidates, 0);
    }
}

#[test]
fn dedicated_listener_follows_focus_and_yaw_independent_of_zoom() {
    let mut app = App::new();
    app.init_resource::<Time<Real>>();
    app.init_resource::<ListeningFrame>();
    app.world_mut()
        .resource_mut::<Time<Real>>()
        .advance_by(std::time::Duration::from_secs(1));
    let camera = app
        .world_mut()
        .spawn(CommanderCamera {
            focus: Vec3::new(100.0, 10.0, 40.0),
            yaw: 0.4,
            zoom: 20.0,
            ..default()
        })
        .id();
    app.world_mut().run_system_cached(update_listener).unwrap();
    let (listener, initial) = app
        .world_mut()
        .query_filtered::<(Entity, &Transform), With<CartAudioListener>>()
        .single(app.world())
        .map(|(entity, transform)| (entity, *transform))
        .unwrap();
    assert_eq!(initial.translation, Vec3::new(100.0, 11.2, 40.0));
    assert_eq!(initial.rotation, Quat::from_rotation_y(0.4));
    app.world_mut()
        .get_mut::<CommanderCamera>(camera)
        .unwrap()
        .zoom = 12_000.0;
    app.world_mut().run_system_cached(update_listener).unwrap();
    assert_eq!(*app.world().get::<Transform>(listener).unwrap(), initial);
    assert_eq!(
        app.world_mut()
            .query::<&SpatialListener>()
            .iter(app.world())
            .count(),
        1
    );
    app.world_mut()
        .get_mut::<CommanderCamera>(camera)
        .unwrap()
        .yaw = -0.8;
    app.world_mut().run_system_cached(update_listener).unwrap();
    let transform = app.world().get::<Transform>(listener).unwrap();
    assert_eq!(transform.rotation, Quat::from_rotation_y(-0.8));
    assert_eq!(transform.translation, initial.translation);
}

#[test]
fn exit_removes_pending_voices_listener_and_logical_ownership() {
    let mut app = App::new();
    app.init_resource::<CartAudioState>();
    app.init_resource::<CartAudioMetrics>();
    app.init_resource::<ListeningFrame>();
    let owner = app.world_mut().spawn_empty().id();
    let voice = app
        .world_mut()
        .spawn(CartRollVoice {
            owner,
            gain: 0.2,
            speed: 1.0,
            cutoff_hz: 4000.0,
            estimated_gain: 0.2,
            filter: FilterControl::new(4000.0),
            born: 0.0,
            last_position: Vec3::ZERO,
            last_moved: 0.0,
            envelope: 1.0,
        })
        .id();
    let listener = app
        .world_mut()
        .spawn((CartAudioListener, SpatialListener::new(4.0)))
        .id();
    {
        let mut state = app.world_mut().resource_mut::<CartAudioState>();
        state.samples.insert(
            owner,
            MovementSample {
                position: Vec3::ZERO,
                at: 1.0,
            },
        );
        state.desired[0] = Some(candidate(owner.to_bits(), 2.0, true));
        state.retry_at = 50.0;
    }
    app.world_mut().run_system_cached(stop_carts).unwrap();
    assert!(app.world().get_entity(voice).is_err());
    assert!(app.world().get_entity(listener).is_err());
    assert!(app.world().get_entity(owner).is_ok());
    let state = app.world().resource::<CartAudioState>();
    assert!(state.samples.is_empty());
    assert!(state.desired.iter().all(Option::is_none));
    assert_eq!(state.retry_at, 0.0);
    assert_eq!(app.world().resource::<CartAudioMetrics>().stops, 1);
}
