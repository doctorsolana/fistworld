//! Offline presentation fixture; connected behavioral proof lives in wildlife_live.
use bevy::prelude::*;
use shared::{components::*, terrain::WorldTerrain};

#[derive(Component)]
pub(super) struct ReviewHorse(usize);

pub(super) fn stage(mut commands: Commands, terrain: Res<WorldTerrain>, mut done: Local<bool>) {
    if *done || std::env::var_os("FISTFORCE_CAPTURE_WILDLIFE").is_none() {
        return;
    }
    *done = true;
    for (index, (x, z, activity)) in [
        (-19., -15., HorseActivity::Graze),
        (-14., -14., HorseActivity::Idle),
        (-16., -19., HorseActivity::Alert),
        (-10., -19., HorseActivity::Moving(HorseGait::Walk)),
    ]
    .into_iter()
    .enumerate()
    {
        commands.spawn((
            ReviewHorse(index),
            Horse {
                id: index as u64 + 1,
                rider: None,
            },
            HorseAnimation {
                activity,
                since: 0.,
            },
            PlayerPosition(Vec3::new(x, terrain.get_height(x, z), z)),
            PlayerRotation(index as f32 * 0.85),
            CharacterMotion::STATIONARY,
        ));
    }
}

pub(super) fn ready(
    mut enabled: Local<Option<bool>>,
    mut frames: Local<u32>,
    horses: Query<Has<crate::animals::HorseRig>, With<ReviewHorse>>,
) -> bool {
    if !*enabled.get_or_insert_with(|| std::env::var_os("FISTFORCE_CAPTURE_WILDLIFE").is_some()) {
        return true;
    }
    if horses.iter().count() == 4 && horses.iter().all(|ready| ready) {
        return true;
    }
    *frames += 1;
    assert!(
        *frames < 1200,
        "wildlife production rigs did not become ready"
    );
    false
}

pub(super) fn drive(
    time: Res<Time>,
    terrain: Res<WorldTerrain>,
    state: Res<super::CaptureState>,
    mut elapsed: Local<f32>,
    mut horses: Query<(
        &ReviewHorse,
        &mut PlayerPosition,
        &mut PlayerRotation,
        &mut CharacterMotion,
        &mut HorseAnimation,
    )>,
    clocks: Query<&WorldTime>,
) {
    if horses.is_empty() {
        return;
    }
    if matches!(*state, super::CaptureState::Warmup { .. }) {
        *elapsed = 0.;
    } else {
        *elapsed += time.delta_secs();
    }
    let now = clocks.iter().next().map_or(0., |c| {
        f64::from(c.day) * f64::from(c.cycle_duration()) + f64::from(c.seconds_in_cycle)
    });
    for (horse, mut p, mut yaw, mut motion, mut animation) in &mut horses {
        // Capture fixes the lighting clock. Give the production consumer an
        // explicit animation timeline without changing that lighting fixture.
        animation.since = now - f64::from(*elapsed);
        if horse.0 == 3 {
            p.0.x = -10. + (*elapsed * HorseGait::Walk.speed()).min(8.);
            p.0.y = terrain.get_height(p.0.x, p.0.z);
            yaw.0 = -std::f32::consts::FRAC_PI_2;
            *motion = CharacterMotion::new(Vec3::X * HorseGait::Walk.speed());
        }
    }
}
